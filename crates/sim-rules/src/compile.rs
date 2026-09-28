//! game.ron + engine.toml → compiled rules (sim_core::Rules).
//! Expressions (when/Set/Add/Move) are Rhai expressions; `script` is full Rhai.
//! Scripts cannot change the world: they only return effect maps.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use rayon::prelude::*;
use rhai::{AST, Array, Dynamic, Engine as Rhai, Map, Scope};
use sim_core::{Effect, Entity, EntityId, Group, Rules, World, splitmix64};
use sim_state::{Chart, Memory, NoOracle, NodeId, Oracle, Outcome, PickKind, PickSpec, Spec, TransitionSpec};

use crate::config::EngineConfig;
use crate::game::{Do, EnvDef, GameDef, PickDef, RuleDef, StateDef, Target};
use crate::env::NativeEnv;

/// Compiled state chart: conditions index into `exprs`, action blocks into `blocks`.
type StateChart = Chart<usize, usize>;

/// `near.<kind>` when there is none.
pub const FAR: i64 = 9_999;

/// Keeps FSM transition salts from colliding with rule salts.
const FSM_SALT: u64 = 1 << 32;

/// A core takes this many entities as one chunk; small worlds pay no parallel overhead.
const CHUNK: usize = 64;

/// Context seen by world queries registered in Rhai (`around`, `rand`).
/// Each core has its own context (thread-local): world snapshot, `me`, salt.
#[derive(Default)]
struct QueryCtx {
    world: Option<Arc<World>>,
    me: EntityId,
    pos: (i64, i64),
    salt: u64,
    calls: u64,
    /// `me`'s kind and state text (for `in_state`, `steps_to`).
    kind: String,
    state: String,
}

impl QueryCtx {
    fn around(&self, kind: &str, state: Option<&str>, r: i64) -> i64 {
        let Some(w) = &self.world else { return 0 };
        match state {
            None => w.around(self.pos, self.me, kind, None, r),
            Some(sel) => w.around_where(self.pos, self.me, kind, r, |e| sim_state::in_label(&e.state, sel)),
        }
    }

    /// Distance to the nearest `kind` in that state; FAR if none.
    fn near_in(&self, kind: &str, sel: &str) -> i64 {
        let Some(w) = &self.world else { return FAR };
        let Some(me) = w.get(self.me) else { return FAR };
        w.nearest_where(me, kind, |e| sim_state::in_label(&e.state, sel)).map_or(FAR, |(_, d)| d)
    }

    /// 0..n. Multiple calls in one expression give different numbers; still fully deterministic.
    fn rand(&mut self, n: i64) -> i64 {
        let Some(w) = &self.world else { return 0 };
        if n <= 0 {
            return 0;
        }
        self.calls += 1;
        (w.rand(self.me, self.salt ^ self.calls.wrapping_mul(0xA5A5_5A5A_1234_5678)) % n as u64) as i64
    }
}

thread_local! {
    static CTX: RefCell<QueryCtx> = RefCell::new(QueryCtx::default());
}

pub struct Game {
    pub def: GameDef,
    pub cfg: EngineConfig,
    /// game.ron defaults + engine.toml overrides.
    pub params: BTreeMap<String, i64>,
    /// Fingerprint of game.ron + engine.toml content (is the replay playing the same game?).
    pub source_hash: u64,
    rhai: Rhai,
    /// Rules first, then actions (`is_action`). Salts are assigned in this order.
    rules: Vec<CompiledRule>,
    ends: Vec<CompiledEnd>,
    score: Option<AST>,
    compile_errors: Vec<String>,
    /// None = single core.
    pool: Option<rayon::ThreadPool>,
    /// `p`, built once. Each core copies it into its own scope once (no locks).
    p_map: Map,
    /// Kind → chart (kinds that have a machine).
    kind_charts: Arc<BTreeMap<String, Arc<StateChart>>>,
    /// Condition/score expressions and action blocks of the charts.
    exprs: Vec<AST>,
    blocks: Vec<Vec<CDo>>,
    /// Environment kinds (hidden singletons), in `environments` order.
    envs: Vec<String>,
    /// Environments whose own machine and rules are replaced by native code.
    natives: BTreeMap<String, Arc<dyn NativeEnv>>,
    /// Kind → state text at birth.
    initial_states: BTreeMap<String, String>,
    /// Definitions of rules written inside states (in `rules` order, for validation).
    machine_rules: Vec<RuleDef>,
    /// The `near.<kind>`s expressions actually read. None = all (could not be determined).
    near_kinds: Option<BTreeSet<String>>,
}

struct CompiledRule {
    salt: u64,
    name: String,
    is_action: bool,
    enabled: bool,
    for_kind: String,
    /// Rule written in a machine: which kinds (those using the machine).
    kinds: Option<BTreeSet<String>>,
    /// (machine, path within machine): the state the rule lives in.
    home: Option<(String, String)>,
    state: Option<String>,
    depth: Option<usize>,
    /// Kind → the state nodes it is bound to (`state` or `home`).
    bind: BTreeMap<String, Vec<NodeId>>,
    target: Option<Target>,
    args: Vec<String>,
    when: Option<AST>,
    then: Vec<CDo>,
    script: Option<AST>,
}

enum CDo {
    Set(String, AST),
    Add(String, AST),
    Emit(String),
    Despawn(Target),
    Spawn(String),
    MoveToward(String),
    MoveAway(String),
    Climb(String, String),
    Wander,
    Goto(String),
    Move(AST, AST),
    On(Target, Vec<CDo>),
    Need(String, AST),
    Interrupt(String),
    Back,
}

impl CDo {
    /// Number expressions to evaluate in this action (and nested `On` blocks).
    fn int_exprs<'a>(&'a self, out: &mut Vec<&'a AST>) {
        match self {
            CDo::Set(_, a) | CDo::Add(_, a) | CDo::Need(_, a) => out.push(a),
            CDo::Move(dx, dy) => out.extend([dx, dy]),
            CDo::On(_, ds) => ds.iter().for_each(|d| d.int_exprs(out)),
            _ => {}
        }
    }
}

struct CompiledEnd {
    when: AST,
    result: String,
}

/// Who is who while a rule is evaluated: `Me` the owner, `It` the target.
struct Who<'a> {
    me: &'a Entity,
    it: Option<&'a Entity>,
}

/// Compiles source texts to AST, collecting errors (does not stop at the first).
struct Compiler<'a> {
    rhai: &'a Rhai,
    errors: Vec<String>,
}

impl Compiler<'_> {
    fn expr(&mut self, src: &str, ctx: &str) -> AST {
        self.rhai.compile_expression(src).unwrap_or_else(|e| {
            self.errors.push(format!("{ctx}: `{src}`: {e}"));
            AST::empty()
        })
    }

    fn doo(&mut self, d: &Do, ctx: &str) -> CDo {
        match d {
            Do::Set(p, e) => CDo::Set(p.clone(), self.expr(e, ctx)),
            Do::Add(p, e) => CDo::Add(p.clone(), self.expr(e, ctx)),
            Do::Emit(n) => CDo::Emit(n.clone()),
            Do::Despawn(t) => CDo::Despawn(t.clone()),
            Do::Spawn(k) => CDo::Spawn(k.clone()),
            Do::MoveToward(k) => CDo::MoveToward(k.clone()),
            Do::MoveAway(k) => CDo::MoveAway(k.clone()),
            Do::Climb(k, prop) => CDo::Climb(k.clone(), prop.clone()),
            Do::Wander => CDo::Wander,
            Do::Goto(s) => CDo::Goto(s.clone()),
            Do::Move(dx, dy) => CDo::Move(self.expr(dx, ctx), self.expr(dy, ctx)),
            Do::On(t, ds) => CDo::On(t.clone(), ds.iter().map(|d| self.doo(d, ctx)).collect()),
            Do::Need(p, e) => CDo::Need(p.clone(), self.expr(e, ctx)),
            Do::Interrupt(s) => CDo::Interrupt(s.clone()),
            Do::Back => CDo::Back,
        }
    }

    fn rule(&mut self, r: &RuleDef, salt: u64, is_action: bool, cfg: &EngineConfig) -> CompiledRule {
        let ctx = format!("{} '{}'", if is_action { "action" } else { "rule" }, r.name);
        let script = r.script.as_deref().map(|src| {
            self.rhai.compile(src).unwrap_or_else(|e| {
                self.errors.push(format!("{ctx} script: {e}"));
                AST::empty()
            })
        });
        CompiledRule {
            salt,
            name: r.name.clone(),
            is_action,
            enabled: cfg.switches.get(&r.name).copied().unwrap_or(true),
            for_kind: r.for_kind.clone().unwrap_or_default(),
            kinds: None,
            home: None,
            state: r.state.clone(),
            depth: r.depth,
            bind: BTreeMap::new(),
            target: r.target.clone(),
            args: r.args.clone(),
            when: r.when.as_deref().map(|w| self.expr(w, &ctx)),
            then: r.then.iter().map(|d| self.doo(d, &ctx)).collect(),
            script,
        }
    }
}

/// Collected from machines: expressions, action blocks, rules written in states.
#[derive(Default)]
struct Machines {
    exprs: Vec<AST>,
    blocks: Vec<Vec<CDo>>,
    /// (machine, path within machine, rule)
    rules: Vec<(String, String, RuleDef)>,
}

impl Compiler<'_> {
    /// A state in `game.ron` → sim-state definition. In a flat (legacy) machine states
    /// are inferred from transitions: `(initial, transitions)` is enough.
    fn spec(&mut self, m: &mut Machines, machine: &str, path: &str, d: &StateDef, root: bool) -> Spec<usize, usize> {
        let ctx = |what: String| match path {
            "" => format!("fsm '{machine}' {what}"),
            p => format!("fsm '{machine}' state '{p}' {what}"),
        };
        let join = |c: &str| if path.is_empty() { c.to_string() } else { format!("{path}.{c}") };
        let mut states: Vec<(String, Spec<usize, usize>)> =
            d.states.iter().map(|(n, sd)| (n.clone(), self.spec(m, machine, &join(n), sd, false))).collect();
        if root && d.states.is_empty() && d.layers.is_empty() && d.uses.is_none() && !d.transitions.is_empty() {
            let mut names: BTreeSet<String> = d.initial.iter().cloned().collect();
            for t in &d.transitions {
                names.extend((t.from != "*").then(|| t.from.clone()));
                names.extend(t.to.clone());
            }
            states = names.into_iter().map(|n| (n, Spec::default())).collect();
        }
        let layers = d.layers.iter().map(|(n, sd)| (n.clone(), self.spec(m, machine, &join(n), sd, false))).collect();
        let enter = self.block(m, &d.enter, &ctx("enter".into()));
        let exit = self.block(m, &d.exit, &ctx("exit".into()));
        let mut transitions = Vec::new();
        for t in &d.transitions {
            let what = format!("transition {} -> {}", t.from, t.to.as_deref().unwrap_or("back"));
            let then = self.block(m, &t.then, &ctx(what.clone()));
            m.exprs.push(self.expr(&t.when, &ctx(what)));
            transitions.push(TransitionSpec {
                from: t.from.clone(),
                to: t.to.clone(),
                back: t.back,
                interrupt: t.interrupt,
                when: m.exprs.len() - 1,
                then,
            });
        }
        let pick = d.pick.as_ref().map(|p| {
            let (kind, opts) = match p {
                PickDef::First(o) => (PickKind::First, o),
                PickDef::Best(o) => (PickKind::Best, o),
            };
            let options = opts
                .iter()
                .map(|(st, e)| {
                    m.exprs.push(self.expr(e, &ctx(format!("pick '{st}'"))));
                    (st.clone(), m.exprs.len() - 1)
                })
                .collect();
            PickSpec { kind, options }
        });
        m.rules.extend(d.rules.iter().map(|r| (machine.to_string(), path.to_string(), r.clone())));
        Spec {
            initial: d.initial.clone(),
            states,
            layers,
            uses: d.uses.clone(),
            remember: d.remember,
            pick,
            recheck: d.recheck,
            enter,
            exit,
            transitions,
        }
    }
}

impl Compiler<'_> {
    /// Adds an action block if non-empty; `Spec` carries the block's index.
    fn block(&mut self, m: &mut Machines, ds: &[Do], ctx: &str) -> Vec<usize> {
        if ds.is_empty() {
            return Vec::new();
        }
        let b = ds.iter().map(|x| self.doo(x, ctx)).collect();
        m.blocks.push(b);
        vec![m.blocks.len() - 1]
    }
}

/// All expression sources in a state tree.
fn state_sources<'a>(d: &'a StateDef, out: &mut Vec<&'a str>) {
    for t in &d.transitions {
        out.push(&t.when);
        t.then.iter().for_each(|x| do_sources(x, out));
    }
    if let Some(PickDef::First(o) | PickDef::Best(o)) = &d.pick {
        out.extend(o.iter().map(|(_, e)| e.as_str()));
    }
    d.enter.iter().chain(&d.exit).for_each(|x| do_sources(x, out));
    for r in &d.rules {
        out.extend(r.when.as_deref());
        out.extend(r.script.as_deref());
        r.then.iter().for_each(|x| do_sources(x, out));
    }
    d.states.values().chain(d.layers.values()).for_each(|c| state_sources(c, out));
}

/// Fewest transitions from a kind's state text to `sel` (FAR if unreachable).
fn steps(charts: &BTreeMap<String, Arc<StateChart>>, kind: &str, state: &str, sel: &str) -> i64 {
    let Some(c) = charts.get(kind) else { return FAR };
    let Ok(m) = c.decode(sim_state::active_part(state)) else { return FAR };
    c.steps_to(&m, &c.resolve(sel))
}

fn map_str(m: &Map, key: &str) -> String {
    m.get(key).and_then(|d| d.clone().into_string().ok()).unwrap_or_default()
}

/// All expression sources in a `Do` tree (for near analysis).
fn do_sources<'a>(d: &'a Do, out: &mut Vec<&'a str>) {
    match d {
        Do::Set(_, e) | Do::Add(_, e) | Do::Need(_, e) => out.push(e),
        Do::Move(dx, dy) => out.extend([dx.as_str(), dy.as_str()]),
        Do::On(_, ds) => ds.iter().for_each(|d| do_sources(d, out)),
        _ => {}
    }
}

/// Finds `near.<kind>` uses in sources. If `near` is used any other way
/// (e.g. `near["x"]`) it stays on the safe side and returns None.
fn near_refs<'a>(sources: impl Iterator<Item = &'a str>) -> Option<BTreeSet<String>> {
    let mut kinds = BTreeSet::new();
    for src in sources {
        let mut rest = src;
        while let Some(i) = rest.find("near") {
            let before = rest[..i].chars().next_back();
            rest = &rest[i + 4..];
            if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                continue; // part of another word
            }
            let Some(tail) = rest.strip_prefix('.') else {
                if rest.starts_with(|c: char| c.is_alphanumeric() || c == '_') {
                    continue; // like `nearest`
                }
                return None;
            };
            let ident: String = tail.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
            kinds.insert(ident);
        }
    }
    Some(kinds)
}

/// Entity view given to Rhai (`me`, `it`).
fn entity_map(e: &Entity, dist: Option<i64>) -> Map {
    let mut m = Map::new();
    m.insert("id".into(), Dynamic::from(e.id as i64));
    m.insert("kind".into(), Dynamic::from(e.kind.clone()));
    m.insert("state".into(), Dynamic::from(sim_state::active_part(&e.state).to_string()));
    m.insert("x".into(), Dynamic::from(e.x));
    m.insert("y".into(), Dynamic::from(e.y));
    for (k, v) in &e.props {
        m.insert(k.as_str().into(), Dynamic::from(*v));
    }
    if let Some(d) = dist {
        m.insert("dist".into(), Dynamic::from(d));
    }
    m
}

impl Game {
    /// `dir/game.ron` + `config` (default `dir/engine.toml`).
    pub fn load(dir: &Path, config: Option<&Path>) -> Result<(World, Game), String> {
        let read = |p: &Path| std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()));
        let game_src = read(&dir.join("game.ron"))?;
        let cfg_path = config.map(Path::to_path_buf).unwrap_or_else(|| dir.join("engine.toml"));
        let cfg_src = read(&cfg_path)?;
        let def: GameDef = ron::from_str(&game_src).map_err(|e| format!("game.ron: {e}"))?;
        let mut envs = Vec::new();
        for name in &def.environments {
            // envs/<name>.ron in the game's folder or the nearest ancestor that has one.
            let found = dir.ancestors().map(|a| a.join("envs").join(format!("{name}.ron"))).find(|p| p.exists());
            let path = found
                .ok_or_else(|| format!("environment '{name}': no envs/{name}.ron next to or above {}", dir.display()))?;
            envs.push((name.clone(), read(&path)?));
        }
        Self::from_parts(&game_src, &cfg_src, &envs)
    }

    pub fn from_strs(game_ron: &str, engine_toml: &str) -> Result<(World, Game), String> {
        Self::from_parts(game_ron, engine_toml, &[])
    }

    /// `envs`: (name, text of `envs/<name>.ron`) for every environment the game lists.
    pub fn from_parts(game_ron: &str, engine_toml: &str, envs: &[(String, String)]) -> Result<(World, Game), String> {
        let mut def: GameDef = ron::from_str(game_ron).map_err(|e| format!("game.ron: {e}"))?;
        let cfg: EngineConfig = toml::from_str(engine_toml).map_err(|e| format!("engine.toml: {e}"))?;
        let parsed: Vec<EnvDef> = envs
            .iter()
            .map(|(n, src)| ron::from_str(src).map_err(|e| format!("envs/{n}.ron: {e}")))
            .collect::<Result<_, _>>()?;
        let merge_errors = def.merge_envs(&parsed).err().unwrap_or_default();
        let mut game = Self::compile(def, cfg);
        game.compile_errors.extend(merge_errors);
        let texts: Vec<&str> = envs.iter().flat_map(|(_, s)| ["\0", s.as_str()]).collect();
        game.source_hash = [game_ron, "\0", engine_toml].into_iter().chain(texts).flat_map(|s| s.bytes()).fold(
            0xcbf2_9ce4_8422_2325_u64,
            |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3),
        );
        let (world, errs) = game.initial_world();
        game.compile_errors.extend(errs);
        Ok((world, game))
    }

    fn compile(def: GameDef, cfg: EngineConfig) -> Game {
        let mut rhai = Rhai::new();
        rhai.set_max_operations(cfg.rhai.max_operations);
        rhai.set_max_call_levels(cfg.rhai.max_call_levels);
        rhai.set_fail_on_invalid_map_property(true);
        // stdout belongs to the agent protocol; scripts cannot write there.
        rhai.on_print(|_| {});
        rhai.on_debug(|_, _, _| {});

        rhai.register_fn("around", |kind: &str, r: i64| CTX.with(|c| c.borrow().around(kind, None, r)));
        rhai.register_fn("around", |kind: &str, state: &str, r: i64| {
            CTX.with(|c| c.borrow().around(kind, Some(state), r))
        });
        rhai.register_fn("rand", |n: i64| CTX.with(|c| c.borrow_mut().rand(n)));
        rhai.register_fn("clamp", |x: i64, lo: i64, hi: i64| x.max(lo).min(hi));
        rhai.register_fn("pct", |x: i64, percent: i64| x * percent / 100);
        rhai.register_fn("ramp", |x: i64, len: i64, peak: i64| if len <= 0 { 0 } else { (x * peak / len).clamp(0, peak) });
        rhai.register_fn("triangle", |x: i64, len: i64, peak: i64| {
            if len <= 0 || x < 0 || x > len { 0 } else { peak - (x * 2 * peak / len - peak).abs() }
        });
        rhai.register_fn("near_in", |kind: &str, sel: &str| CTX.with(|c| c.borrow().near_in(kind, sel)));
        rhai.register_fn("in_state", |sel: &str| CTX.with(|c| sim_state::in_label(&c.borrow().state, sel)));
        rhai.register_fn("in_state", |m: Map, sel: &str| sim_state::in_label(&map_str(&m, "state"), sel));
        rhai.register_fn("depth_in", |sel: &str| CTX.with(|c| sim_state::depth_in_label(&c.borrow().state, sel)));
        rhai.register_fn("depth_in", |m: Map, sel: &str| sim_state::depth_in_label(&map_str(&m, "state"), sel));
        let pool = match cfg.run.threads {
            1 => None,
            n => rayon::ThreadPoolBuilder::new().num_threads(n).build().ok(),
        };

        let mut cc = Compiler { rhai: &rhai, errors: Vec::new() };
        let mut mach = Machines::default();
        let specs: BTreeMap<String, Spec<usize, usize>> =
            def.fsms.iter().map(|(name, f)| (name.clone(), cc.spec(&mut mach, name, "", f, true))).collect();
        // Salts: rule i → i+1 (so existing games keep their trajectory), actions after.
        let n = def.rules.len() as u64;
        let mut rules: Vec<CompiledRule> =
            def.rules.iter().enumerate().map(|(i, r)| cc.rule(r, i as u64 + 1, false, &cfg)).collect();
        rules.extend(def.actions.iter().enumerate().map(|(i, r)| cc.rule(r, n + i as u64 + 1, true, &cfg)));
        // Rules written in states come last: existing games' salts do not shift.
        let base = rules.len() as u64;
        for (i, (m, p, r)) in mach.rules.iter().enumerate() {
            let mut c = cc.rule(r, base + i as u64 + 1, false, &cfg);
            c.home = Some((m.clone(), p.clone()));
            rules.push(c);
        }
        // Environment rules after everything: adding an environment never shifts a game's salts.
        let base = rules.len() as u64;
        rules.extend(def.env_rules.iter().enumerate().map(|(i, r)| cc.rule(r, base + i as u64 + 1, false, &cfg)));
        let ends = def
            .end
            .iter()
            .map(|e| CompiledEnd { when: cc.expr(&e.when, &format!("end '{}'", e.result)), result: e.result.clone() })
            .collect();
        let score = def.score.as_deref().map(|s| cc.expr(s, "score"));
        let mut errors = cc.errors;

        let mut charts = BTreeMap::new();
        for name in def.fsms.keys() {
            match Chart::build(name, &specs) {
                Ok(c) => {
                    charts.insert(name.clone(), Arc::new(c));
                }
                Err(es) => errors.extend(es.into_iter().map(|e| format!("fsm '{name}': {e}"))),
            }
        }
        let kind_charts: BTreeMap<String, Arc<StateChart>> = def
            .kinds
            .iter()
            .filter_map(|(k, kd)| Some((k.clone(), charts.get(kd.fsm.as_ref()?)?.clone())))
            .collect();
        for r in &mut rules {
            match &r.home {
                Some((m, p)) => {
                    for (k, c) in &kind_charts {
                        let ids = c.with_origin(m, p);
                        if !ids.is_empty() {
                            r.bind.insert(k.clone(), ids);
                        }
                    }
                    r.kinds = Some(r.bind.keys().cloned().collect());
                }
                None => {
                    if let Some(sel) = &r.state {
                        for (k, c) in kind_charts.iter().filter(|(k, _)| r.for_kind == "*" || **k == r.for_kind) {
                            let ids = c.resolve(sel);
                            if !ids.is_empty() {
                                r.bind.insert(k.clone(), ids);
                            }
                        }
                    }
                }
            }
        }
        let initial_states = kind_charts.iter().map(|(k, c)| (k.clone(), c.encode(&c.initial()))).collect();
        let kind_charts = Arc::new(kind_charts);
        let kc = kind_charts.clone();
        rhai.register_fn("steps_to", move |sel: &str| {
            CTX.with(|c| {
                let c = c.borrow();
                steps(&kc, &c.kind, &c.state, sel)
            })
        });
        let kc = kind_charts.clone();
        rhai.register_fn("steps_to", move |m: Map, sel: &str| steps(&kc, &map_str(&m, "kind"), &map_str(&m, "state"), sel));

        let mut params = def.params.clone();
        params.extend(cfg.params.iter().map(|(k, v)| (k.clone(), *v)));
        let p: Map = params.iter().map(|(k, v)| (k.as_str().into(), Dynamic::from(*v))).collect();
        let p_map = p;

        let mut sources: Vec<&str> = Vec::new();
        for f in def.fsms.values() {
            state_sources(f, &mut sources);
        }
        for r in def.rules.iter().chain(&def.actions).chain(&def.env_rules) {
            sources.extend(r.when.as_deref());
            sources.extend(r.script.as_deref());
            r.then.iter().for_each(|d| do_sources(d, &mut sources));
        }
        sources.extend(def.end.iter().map(|e| e.when.as_str()));
        sources.extend(def.score.as_deref());
        let near_kinds = near_refs(sources.into_iter());

        let envs = def.env_kinds.clone();
        Game {
            def,
            cfg,
            params,
            source_hash: 0,
            rhai,
            rules,
            ends,
            score,
            compile_errors: errors,
            pool,
            p_map,
            near_kinds,
            kind_charts,
            envs,
            natives: BTreeMap::new(),
            machine_rules: mach.rules.into_iter().map(|(_, _, r)| r).collect(),
            exprs: mach.exprs,
            blocks: mach.blocks,
            initial_states,
        }
    }

    /// World size from `layout` or the panel's `[world]`. Layout is placed first,
    /// then `[spawn]`. Solid kinds are placed one by one into shuffled empty cells.
    fn initial_world(&self) -> (World, Vec<String>) {
        let mut errs = Vec::new();
        let (w, h) = match (&self.def.layout, &self.cfg.world) {
            (Some(l), Some(wc)) => {
                if l.size() != (wc.width, wc.height) {
                    errs.push(format!(
                        "engine.toml [world] {}x{} does not match game.ron layout {}x{}",
                        wc.width,
                        wc.height,
                        l.size().0,
                        l.size().1
                    ));
                }
                l.size()
            }
            (Some(l), None) => l.size(),
            (None, Some(wc)) => (wc.width, wc.height),
            (None, None) => {
                errs.push("engine.toml: [world] is required when game.ron has no layout".into());
                (1, 1)
            }
        };
        let mut world = World::new(self.cfg.run.seed, w, h);
        world.set_solid(self.def.kinds.iter().filter(|(_, k)| k.solid).map(|(n, _)| n.clone()).collect());

        if let Some(l) = &self.def.layout {
            for (y, row) in l.rows.iter().enumerate() {
                for (x, ch) in row.chars().enumerate() {
                    if ch == '.' || ch == ' ' {
                        continue;
                    }
                    let Some(entry) = l.legend.get(&ch) else {
                        errs.push(format!("layout row {y}, column {x}: '{ch}' is not in the legend"));
                        continue;
                    };
                    let kind = entry.kind();
                    if !self.def.kinds.contains_key(kind) {
                        continue; // check_refs reports it
                    }
                    let (state, mut props) = self.template(kind);
                    props.extend(entry.props().into_iter().flatten().map(|(k, v)| (k.clone(), *v)));
                    world.spawn(kind, &state, x as i64, y as i64, props);
                }
            }
        }

        // Environments the layout did not place: one each, at (0, 0). Placed twice is an error.
        for name in &self.envs {
            match world.count(name) {
                0 => {
                    let (state, props) = self.template(name);
                    world.spawn(name, &state, 0, 0, props);
                }
                1 => {}
                n => errs.push(format!("environment '{name}' is placed {n} times in the layout; it is one world")),
            }
        }

        let mut c = 0u64;
        for (kind, n) in &self.cfg.spawn {
            let Some(def) = self.def.kinds.get(kind) else {
                continue; // validate reports it
            };
            if def.solid {
                let placed = self.place_solid(&mut world, kind, *n);
                if placed < *n {
                    errs.push(format!("engine.toml [spawn]: {kind} = {n}, but only {placed} free cells"));
                }
                continue;
            }
            for _ in 0..*n {
                let x = splitmix64(world.seed.wrapping_add(2 * c)) % world.width as u64;
                let y = splitmix64(world.seed.wrapping_add(2 * c + 1)) % world.height as u64;
                let (state, props) = self.template(kind);
                world.spawn(kind, &state, x as i64, y as i64, props);
                c += 1;
            }
        }
        (world, errs)
    }

    fn place_solid(&self, world: &mut World, kind: &str, n: u32) -> u32 {
        let mut cells: Vec<i64> = (0..world.width * world.height).collect();
        let salt = splitmix64(kind.bytes().fold(world.seed, |h, b| splitmix64(h ^ b as u64)));
        for i in (1..cells.len()).rev() {
            let j = (splitmix64(salt ^ i as u64) % (i as u64 + 1)) as usize;
            cells.swap(i, j);
        }
        let mut placed = 0;
        for cell in cells {
            if placed == n {
                break;
            }
            let (state, props) = self.template(kind);
            if world.spawn(kind, &state, cell % world.width, cell / world.width, props).is_some() {
                placed += 1;
            }
        }
        placed
    }

    /// Initial state and props of a newborn kind.
    pub fn template(&self, kind: &str) -> (String, BTreeMap<String, i64>) {
        let Some(k) = self.def.kinds.get(kind) else { return ("-".into(), BTreeMap::new()) };
        let state = self.initial_states.get(kind).cloned().unwrap_or_else(|| "-".into());
        (state, k.props.clone())
    }

    pub fn glyph(&self, kind: &str) -> char {
        self.def.kinds.get(kind).map_or('?', |k| k.glyph)
    }

    /// The state-specific glyph if any, else the kind's glyph.
    /// Glyph of the deepest matching state (`Flow` wins over `Work`).
    pub fn glyph_of(&self, e: &Entity) -> char {
        let Some(k) = self.def.kinds.get(&e.kind) else { return '?' };
        let (Some(c), false) = (self.kind_charts.get(&e.kind), k.glyphs.is_empty()) else {
            return k.glyphs.get(&e.state).copied().unwrap_or(k.glyph);
        };
        let Ok(m) = c.decode(&e.state) else { return k.glyph };
        let mut best: Option<(usize, char)> = None;
        for (sel, g) in &k.glyphs {
            if let Some(d) = c.deepest(&m, &c.resolve(sel))
                && best.is_none_or(|(b, _)| d > b)
            {
                best = Some((d, *g));
            }
        }
        best.map_or(k.glyph, |(_, g)| g)
    }

    /// All states of a kind (with paths); `-` if it has no machine.
    pub fn states_of(&self, kind: &str) -> BTreeSet<String> {
        match self.kind_charts.get(kind) {
            Some(c) => c.paths().map(str::to_string).collect(),
            None => ["-".to_string()].into(),
        }
    }

    /// Replaces an environment's own machine and rules with native code. Check it with `conformance`.
    pub fn set_native_env(&mut self, name: &str, native: Arc<dyn NativeEnv>) -> Result<(), String> {
        if !self.envs.iter().any(|e| e == name) {
            return Err(format!("'{name}' is not an environment of this game"));
        }
        self.natives.insert(name.to_string(), native);
        Ok(())
    }

    /// Whether a kind is drawn (environments are not).
    pub fn is_hidden(&self, kind: &str) -> bool {
        self.def.kinds.get(kind).is_some_and(|k| k.hidden)
    }

    /// State as observers see it: active ones only.
    pub fn state_label<'e>(&self, e: &'e Entity) -> &'e str {
        sim_state::active_part(&e.state)
    }

    /// Effective switch state (rule or action name → on?).
    pub fn switches(&self) -> BTreeMap<String, bool> {
        self.rules.iter().map(|r| (r.name.clone(), r.enabled)).collect()
    }

    /// An agent requests an action. Since the world does not change between `act` and the next tick,
    /// the action is evaluated at once; the result (Group) is applied at the tick, before rules.
    pub fn act(
        &self,
        world: &World,
        seat: Option<&str>,
        id: EntityId,
        name: &str,
        args: &BTreeMap<String, i64>,
    ) -> Result<Group, String> {
        let rule = self
            .rules
            .iter()
            .find(|r| r.is_action && r.name == name)
            .ok_or_else(|| format!("unknown action '{name}' (see info)"))?;
        if !rule.enabled {
            return Err(format!("action '{name}' is switched off (engine.toml [switches])"));
        }
        let e = world.get(id).ok_or_else(|| format!("entity {id} does not exist"))?;
        if !self.cfg.agent.controllable.contains(&e.kind) {
            return Err(format!("kind '{}' is not controllable (engine.toml [agent])", e.kind));
        }
        if !self.owns(seat, e)? {
            return Err(format!("entity {id} is not yours"));
        }
        if !Self::applies(rule, &e.kind) {
            return Err(format!("action '{name}' is for '{}', entity {id} is a '{}'", rule.for_kind, e.kind));
        }
        if !self.bound(rule, e, self.memory(e).ok().flatten().as_ref()) {
            return Err(format!("action '{name}' needs state '{}'", rule.state.as_deref().unwrap_or_default()));
        }
        if args.keys().collect::<BTreeSet<_>>() != rule.args.iter().collect::<BTreeSet<_>>() {
            return Err(format!("action '{name}' takes args {:?}", rule.args));
        }

        let counts = self.counts(world);
        self.bind_world(Some(world));
        let mut scope = self.base_scope(world, e, &counts);
        let arg: Map = args.iter().map(|(k, v)| (k.as_str().into(), Dynamic::from(*v))).collect();
        scope.push_constant("arg", arg);
        let out = self.eval_rule(world, e, rule, &mut scope);
        self.bind_world(None);

        let def = self.def.actions.iter().find(|a| a.name == name).expect("compiled from def");
        match out? {
            Some(g) => Ok(g),
            None => {
                let target = def.target.as_ref().map(|t| format!("{t:?}"));
                let mut needs: Vec<String> = target.into_iter().chain(def.when.clone()).collect();
                def.then.iter().for_each(|d| need_texts(d, &mut needs));
                Err(format!("refused: needs {}", needs.join(" and ")))
            }
        }
    }

    /// Without seats anyone controls everything. With seats, `as` is required and `owner` must match.
    pub fn owns(&self, seat: Option<&str>, e: &Entity) -> Result<bool, String> {
        let seats = &self.cfg.agent.seats;
        if seats.is_empty() {
            return Ok(true);
        }
        let seat = seat.ok_or_else(|| format!("this game has seats {:?}; send \"as\"", seats.keys().collect::<Vec<_>>()))?;
        let n = seats.get(seat).ok_or_else(|| format!("unknown seat '{seat}'"))?;
        Ok(e.props.get("owner") == Some(n))
    }

    /// Scoreboard: `score` sum per seat (per entity if there are no seats).
    pub fn scores(&self, world: &World) -> BTreeMap<String, i64> {
        let mut out = BTreeMap::new();
        let Some(score) = &self.score else { return out };
        let counts = self.counts(world);
        self.bind_world(Some(world));
        let by_owner: BTreeMap<i64, &String> = self.cfg.agent.seats.iter().map(|(s, n)| (*n, s)).collect();
        for e in world.entities().values().filter(|e| self.cfg.agent.controllable.contains(&e.kind)) {
            let mut scope = self.base_scope(world, e, &counts);
            self.bind(e, 0);
            let v = self.eval_int(&mut scope, score).unwrap_or(0);
            let key = match e.props.get("owner").and_then(|n| by_owner.get(n)) {
                Some(seat) if !by_owner.is_empty() => (*seat).clone(),
                _ => e.id.to_string(),
            };
            *out.entry(key).or_insert(0) += v;
        }
        self.bind_world(None);
        out
    }

    /// Once per tick.
    fn counts(&self, world: &World) -> Map {
        self.def.kinds.keys().map(|k| (k.as_str().into(), Dynamic::from(world.count(k) as i64))).collect()
    }

    /// Constant during a tick: p, tick, count. Built once per core;
    /// per-entity variables are pushed on top and rewound.
    fn tick_scope(&self, world: &World, counts: &Map) -> Scope<'static> {
        let mut scope = Scope::new();
        scope.push_constant("p", self.p_map.clone());
        scope.push_constant("tick", world.tick as i64);
        scope.push_constant("count", counts.clone());
        scope.push_constant("env", self.env_map(world));
        scope
    }

    /// `env.<name>.<prop>` and `env.<name>.state`: the environments at the start of the tick.
    fn env_map(&self, world: &World) -> Map {
        self.envs
            .iter()
            .filter_map(|name| {
                let e = world.of_kind(name).next()?;
                let mut m: Map = e.props.iter().map(|(k, v)| (k.as_str().into(), Dynamic::from(*v))).collect();
                m.insert("state".into(), Dynamic::from(sim_state::active_part(&e.state).to_string()));
                Some((name.as_str().into(), Dynamic::from(m)))
            })
            .collect()
    }

    /// Per entity: me, near.
    fn push_entity(&self, scope: &mut Scope<'static>, world: &World, e: &Entity) {
        let near: Map = self
            .def
            .kinds
            .keys()
            .filter(|k| self.near_kinds.as_ref().is_none_or(|ks| ks.contains(*k)))
            .map(|k| (k.as_str().into(), Dynamic::from(world.nearest(e, k).map_or(FAR, |(_, d)| d))))
            .collect();
        scope.push_constant("me", entity_map(e, None));
        scope.push_constant("near", near);
    }

    /// World seen by rule expressions: p, tick, count, me, near (+ per-rule roll, it, arg).
    fn base_scope(&self, world: &World, e: &Entity, counts: &Map) -> Scope<'static> {
        let mut scope = self.tick_scope(world, counts);
        self.push_entity(&mut scope, world, e);
        scope
    }

    /// World seen by `end` expressions: p, tick, count.
    fn world_scope(&self, world: &World) -> Scope<'static> {
        self.tick_scope(world, &self.counts(world))
    }

    /// Binds the query context to an entity and salt (before each evaluation).
    fn bind(&self, e: &Entity, salt: u64) {
        CTX.with(|c| {
            let mut c = c.borrow_mut();
            (c.me, c.pos, c.salt, c.calls) = (e.id, (e.x, e.y), salt, 0);
            if c.state != e.state {
                c.state.clone_from(&e.state);
            }
            if c.kind != e.kind {
                c.kind.clone_from(&e.kind);
            }
        });
    }

    /// Binds the world to this core's context (creates it if missing).
    fn bind_world(&self, world: Option<&World>) {
        self.bind_snapshot(world.map(|w| Arc::new(w.clone())).as_ref());
    }

    fn bind_snapshot(&self, snap: Option<&Arc<World>>) {
        CTX.with(|c| {
            let mut c = c.borrow_mut();
            if c.world.as_ref().map(Arc::as_ptr) != snap.map(Arc::as_ptr) {
                c.world = snap.cloned();
            }
        });
    }

    /// All of an entity's groups this tick: FSM transition first, then rules (in order).
    fn eval_entity(&self, world: &World, e: &Entity, scope: &mut Scope<'static>) -> Vec<Group> {
        let len = scope.len();
        self.push_entity(scope, world, e);
        let out = self.eval_entity_in(world, e, scope);
        scope.rewind(len);
        out
    }

    fn eval_entity_in(&self, world: &World, e: &Entity, base: &mut Scope<'static>) -> Vec<Group> {
        let mut out = Vec::new();
        let error = |source: &str, m: String| Group {
            source: source.into(),
            actor: Some(e.id),
            effects: vec![Effect::Emit { e: e.id, name: format!("error: {m}") }],
        };

        // A native environment replaces its own machine and rules with one pure step.
        if let Some(native) = self.natives.get(&e.kind) {
            let (props, state) = native.step(world.tick, &self.params, &e.props, &e.state);
            let mut effects: Vec<Effect> = props
                .into_iter()
                .filter(|(k, v)| e.props.get(k) != Some(v))
                .map(|(prop, v)| Effect::Set { e: e.id, prop, v })
                .collect();
            if state != e.state {
                effects.push(Effect::SetState { e: e.id, state });
            }
            if !effects.is_empty() {
                out.push(Group { source: "env".into(), actor: Some(e.id), effects });
            }
            return out;
        }

        // State chart: one group (transitions, picks, enter/exit). Rules see the old state this tick.
        let chart = self.kind_charts.get(&e.kind);
        let mem = match self.memory(e) {
            Ok(m) => m,
            Err(m) => {
                out.push(error("fsm", m));
                None
            }
        };
        if let (Some(c), Some(m)) = (chart, &mem) {
            let stepped = c.step(m, &mut RhaiOracle { game: self, world, e, scope: base });
            match stepped.and_then(|o| self.state_group(world, e, c, o, base)) {
                Ok(Some(g)) => out.push(g),
                Ok(None) => {}
                Err(m) => out.push(error("fsm", m)),
            }
        }

        let active = self
            .rules
            .iter()
            .filter(|r| !r.is_action && r.enabled && Self::applies(r, &e.kind) && self.bound(r, e, mem.as_ref()));
        for rule in active {
            match self.eval_rule(world, e, rule, base) {
                Ok(Some(g)) => out.push(g),
                Ok(None) => {}
                Err(m) => out.push(error(&rule.name, m)),
            }
        }
        out
    }

    /// Memory of an entity with a machine (None if it has none).
    fn memory(&self, e: &Entity) -> Result<Option<Memory>, String> {
        self.kind_charts.get(&e.kind).map(|c| c.decode(&e.state)).transpose()
    }

    /// Is the rule bound to this entity's current state (`state:` or written inside the state)?
    fn bound(&self, r: &CompiledRule, e: &Entity, mem: Option<&Memory>) -> bool {
        if r.state.is_none() && r.home.is_none() {
            return true;
        }
        let (Some(c), Some(m)) = (self.kind_charts.get(&e.kind), mem) else { return false };
        r.bind.get(&e.kind).is_some_and(|ids| c.in_any(m, ids, r.depth))
    }

    /// Chart change → group: new state first, then exit / then / enter actions.
    fn state_group(
        &self,
        world: &World,
        e: &Entity,
        c: &StateChart,
        o: Outcome<usize>,
        scope: &mut Scope<'static>,
    ) -> Result<Option<Group>, String> {
        if !o.changed {
            return Ok(None);
        }
        let mut effects = vec![Effect::SetState { e: e.id, state: c.encode(&o.mem) }];
        let who = Who { me: e, it: None };
        self.run_blocks(world, &who, e, &o.actions, FSM_SALT + o.salt.unwrap_or(0), scope, &mut effects)?;
        Ok(Some(Group { source: "fsm".into(), actor: Some(e.id), effects }))
    }

    /// Chart action blocks. An action whose target is missing is skipped (not the transition).
    #[allow(clippy::too_many_arguments)]
    fn run_blocks<'a>(
        &self,
        world: &'a World,
        who: &Who<'a>,
        subj: &'a Entity,
        blocks: &[usize],
        salt: u64,
        scope: &mut Scope<'static>,
        out: &mut Vec<Effect>,
    ) -> Result<(), String> {
        if blocks.is_empty() {
            return Ok(());
        }
        let len = scope.len();
        scope.push_constant("roll", world.roll(who.me.id, salt));
        self.bind(who.me, salt);
        let mut res = Ok(());
        'blocks: for &b in blocks {
            for d in &self.blocks[b] {
                if let Err(m) = self.eval_do(world, who, subj, d, salt, scope, out) {
                    res = Err(m);
                    break 'blocks;
                }
            }
        }
        scope.rewind(len);
        res
    }

    /// `Goto`, `Interrupt`, `Back`: a change in the subject's chart + its actions.
    #[allow(clippy::too_many_arguments)]
    fn change_state<'a>(
        &self,
        world: &'a World,
        who: &Who<'a>,
        subj: &'a Entity,
        d: &CDo,
        salt: u64,
        scope: &mut Scope<'static>,
        out: &mut Vec<Effect>,
    ) -> Result<(), String> {
        let c = self.kind_charts.get(&subj.kind).ok_or_else(|| format!("kind '{}' has no fsm", subj.kind))?;
        let m = c.decode(&subj.state)?;
        let target = match d {
            CDo::Goto(sel) | CDo::Interrupt(sel) => Some(unique(c, sel)?),
            _ => None,
        };
        fn change(c: &StateChart, m: &Memory, d: &CDo, t: Option<NodeId>, o: &mut impl Oracle<usize>) -> Result<Outcome<usize>, String> {
            match (d, t) {
                (CDo::Goto(_), Some(t)) => c.goto(m, t, o),
                (CDo::Interrupt(_), Some(t)) => c.interrupt(m, t, o),
                _ => c.back(m, o),
            }
        }
        // Another entity's picks cannot be evaluated here (expressions see `me`): falls back.
        let o = if subj.id == who.me.id {
            change(c, &m, d, target, &mut RhaiOracle { game: self, world, e: subj, scope })?
        } else {
            change(c, &m, d, target, &mut NoOracle)?
        };
        out.push(Effect::SetState { e: subj.id, state: c.encode(&o.mem) });
        for &b in &o.actions {
            for x in &self.blocks[b] {
                self.eval_do(world, who, subj, x, salt, scope, out)?;
            }
        }
        Ok(())
    }

    fn eval_bool(&self, scope: &mut Scope, ast: &AST) -> Result<bool, String> {
        self.rhai.eval_ast_with_scope::<bool>(scope, ast).map_err(|e| e.to_string())
    }

    fn eval_int(&self, scope: &mut Scope, ast: &AST) -> Result<i64, String> {
        self.rhai.eval_ast_with_scope::<i64>(scope, ast).map_err(|e| e.to_string())
    }

    /// No copy: per-rule variables are pushed, then the scope is rewound.
    fn eval_rule(
        &self,
        world: &World,
        e: &Entity,
        rule: &CompiledRule,
        scope: &mut Scope<'static>,
    ) -> Result<Option<Group>, String> {
        let len = scope.len();
        let out = self.eval_rule_in(world, e, rule, scope);
        scope.rewind(len);
        out
    }

    fn eval_rule_in(
        &self,
        world: &World,
        e: &Entity,
        rule: &CompiledRule,
        scope: &mut Scope<'static>,
    ) -> Result<Option<Group>, String> {
        scope.push_constant("roll", world.roll(e.id, rule.salt));
        self.bind(e, rule.salt);

        let it = match &rule.target {
            None => None,
            Some(t @ (Target::Nearest(_) | Target::NearestIn(..))) => match nearest(world, e, t) {
                Some((t, d)) => {
                    scope.push_constant("it", entity_map(t, Some(d)));
                    Some(t)
                }
                None => return Ok(None),
            },
            Some(t) => return Err(format!("target must be Nearest(kind) or NearestIn(kind, state), got {t:?}")),
        };

        if let Some(w) = &rule.when
            && !self.eval_bool(scope, w)?
        {
            return Ok(None);
        }

        let who = Who { me: e, it };
        let mut effects = Vec::new();
        for d in &rule.then {
            if !self.eval_do(world, &who, e, d, rule.salt, scope, &mut effects)? {
                return Ok(None); // no target → firing is meaningless
            }
        }

        if let Some(script) = &rule.script {
            let out: Array = self.rhai.eval_ast_with_scope(scope, script).map_err(|e| e.to_string())?;
            for item in out {
                effects.push(effect_from_map(e, item)?);
            }
        }

        Ok((!effects.is_empty()).then(|| Group { source: rule.name.clone(), actor: Some(e.id), effects }))
    }

    fn resolve<'a>(&self, world: &'a World, who: &Who<'a>, t: &Target) -> Option<&'a Entity> {
        match t {
            Target::Me => Some(who.me),
            Target::It => who.it,
            Target::Nearest(_) | Target::NearestIn(..) => nearest(world, who.me, t).map(|(t, _)| t),
        }
    }

    /// `subj` is the entity the action applies to: normally the owner, the target inside `On(...)`.
    /// false = target not found; the rule does not fire this tick.
    #[allow(clippy::too_many_arguments)]
    fn eval_do<'a>(
        &self,
        world: &'a World,
        who: &Who<'a>,
        subj: &'a Entity,
        d: &CDo,
        salt: u64,
        scope: &mut Scope<'static>,
        out: &mut Vec<Effect>,
    ) -> Result<bool, String> {
        match d {
            CDo::Set(prop, ast) => out.push(Effect::Set { e: subj.id, prop: prop.clone(), v: self.eval_int(scope, ast)? }),
            CDo::Add(prop, ast) => out.push(Effect::Add { e: subj.id, prop: prop.clone(), d: self.eval_int(scope, ast)? }),
            CDo::Emit(name) => out.push(Effect::Emit { e: subj.id, name: name.clone() }),
            CDo::Despawn(t) => match self.resolve(world, who, t) {
                Some(x) => out.push(Effect::Despawn { e: x.id }),
                None => return Ok(false),
            },
            CDo::Spawn(k) => {
                let (state, props) = self.template(k);
                out.push(Effect::Spawn { kind: k.clone(), state, x: subj.x, y: subj.y, props });
            }
            CDo::MoveToward(k) => {
                if let Some((t, _)) = world.nearest(subj, k) {
                    out.push(Effect::Move { e: subj.id, dx: t.x - subj.x, dy: t.y - subj.y });
                }
            }
            CDo::MoveAway(k) => match world.nearest(subj, k) {
                Some((_, 0)) => out.push(wander(world, subj, salt)),
                Some((t, _)) => out.push(Effect::Move { e: subj.id, dx: subj.x - t.x, dy: subj.y - t.y }),
                None => {}
            },
            CDo::Wander => out.push(wander(world, subj, salt)),
            CDo::Climb(k, prop) => {
                if let Some((dx, dy)) = climb(world, subj, k, prop, salt) {
                    out.push(Effect::Move { e: subj.id, dx, dy });
                }
            }
            CDo::Goto(_) | CDo::Interrupt(_) | CDo::Back => self.change_state(world, who, subj, d, salt, scope, out)?,
            CDo::Move(dx, dy) => {
                let (dx, dy) = (self.eval_int(scope, dx)?, self.eval_int(scope, dy)?);
                out.push(Effect::Move { e: subj.id, dx, dy });
            }
            CDo::Need(prop, ast) => {
                let min = self.eval_int(scope, ast)?;
                if subj.props.get(prop).copied().unwrap_or(0) < min {
                    return Ok(false); // not enough even now → does not fire
                }
                out.push(Effect::Need { e: subj.id, prop: prop.clone(), min });
            }
            CDo::On(t, ds) => {
                let Some(x) = self.resolve(world, who, t) else { return Ok(false) };
                for d in ds {
                    if !self.eval_do(world, who, x, d, salt, scope, out)? {
                        return Ok(false);
                    }
                }
            }
        }
        Ok(true)
    }

    fn applies(rule: &CompiledRule, kind: &str) -> bool {
        match &rule.kinds {
            Some(ks) => ks.contains(kind),
            None => rule.for_kind == "*" || rule.for_kind == kind,
        }
    }

    /// Possible kinds of a target (for validation). `me`: kinds that may own the rule.
    fn target_kinds(&self, name: &str, target: Option<&Target>, me: &[String], t: &Target, errs: &mut Vec<String>) -> Vec<String> {
        match t {
            Target::Me => me.to_vec(),
            Target::It => match target {
                Some(Target::Nearest(k) | Target::NearestIn(k, _)) => vec![k.clone()],
                _ => {
                    errs.push(format!("'{name}': uses It but has no target"));
                    vec![]
                }
            },
            Target::Nearest(k) | Target::NearestIn(k, _) => {
                if !self.def.kinds.contains_key(k) {
                    errs.push(format!("'{name}': unknown kind '{k}'"));
                } else if let Target::NearestIn(_, sel) = t {
                    self.check_selector(&format!("'{name}'"), std::slice::from_ref(k), sel, errs);
                }
                vec![k.clone()]
            }
        }
    }

    /// The selector must resolve to a state of at least one of the kinds.
    fn check_selector(&self, what: &str, kinds: &[String], sel: &str, errs: &mut Vec<String>) {
        let found = kinds.iter().any(|k| self.kind_charts.get(k).is_some_and(|c| !c.resolve(sel).is_empty()));
        if !found {
            errs.push(format!("{what}: state '{sel}' does not exist for '{}'", kinds.join("', '")));
        }
    }

    /// `subjects`: possible kinds of the entity the action applies to; `me`: those of the rule's owner.
    #[allow(clippy::too_many_arguments)]
    fn check_do(
        &self,
        name: &str,
        target: Option<&Target>,
        me: &[String],
        d: &Do,
        subjects: &[String],
        errs: &mut Vec<String>,
    ) {
        match d {
            Do::Despawn(t) => {
                self.target_kinds(name, target, me, t, errs);
            }
            Do::Spawn(k) | Do::MoveToward(k) | Do::MoveAway(k) if !self.def.kinds.contains_key(k) => {
                errs.push(format!("'{name}': unknown kind '{k}'"));
            }
            Do::Climb(k, prop) => match self.def.kinds.get(k) {
                None => errs.push(format!("'{name}': Climb on unknown kind '{k}'")),
                Some(kd) if !kd.props.contains_key(prop) => {
                    errs.push(format!("'{name}': Climb: kind '{k}' has no prop '{prop}'"))
                }
                _ => {}
            },
            Do::Goto(st) | Do::Interrupt(st) => {
                let verb = if matches!(d, Do::Goto(_)) { "Goto" } else { "Interrupt" };
                let charts: Vec<&Arc<StateChart>> = subjects.iter().filter_map(|k| self.kind_charts.get(k)).collect();
                if !charts.iter().any(|c| !c.resolve(st).is_empty()) {
                    errs.push(format!("'{name}': {verb} to unknown state '{st}'"));
                }
                for c in &charts {
                    match c.resolve(st)[..] {
                        [one] if verb == "Interrupt" && !c.interruptible(one) => {
                            errs.push(format!("'{name}': cannot Interrupt into '{st}': it is a layer, not a state"));
                        }
                        [_, _, ..] => errs.push(format!("'{name}': {verb} '{st}' is ambiguous; use a path like 'Parent.{st}'")),
                        _ => {}
                    }
                }
            }
            Do::Back if !subjects.iter().any(|k| self.kind_charts.contains_key(k)) => {
                errs.push(format!("'{name}': Back on a kind without fsm"));
            }
            Do::On(t, ds) => {
                let subs = self.target_kinds(name, target, me, t, errs);
                for d in ds {
                    self.check_do(name, target, me, d, &subs, errs);
                }
            }
            _ => {}
        }
    }

    /// The machine's enter/exit/then actions: the subject is any kind using that machine.
    fn check_machine(&self, machine: &str, path: &str, d: &StateDef, kinds: &[String], errs: &mut Vec<String>) {
        let name = if path.is_empty() { format!("fsm {machine}") } else { format!("fsm {machine} state {path}") };
        let all = d.enter.iter().chain(&d.exit).chain(d.transitions.iter().flat_map(|t| &t.then));
        for x in all {
            self.check_do(&name, None, kinds, x, kinds, errs);
        }
        for (c, cd) in d.states.iter().chain(&d.layers) {
            let p = if path.is_empty() { c.clone() } else { format!("{path}.{c}") };
            self.check_machine(machine, &p, cd, kinds, errs);
        }
    }

    fn check_refs(&self, errs: &mut Vec<String>) {
        let known = |k: &str| self.def.kinds.contains_key(k);
        for k in self.cfg.spawn.keys().filter(|k| !known(k)) {
            errs.push(format!("engine.toml [spawn]: unknown kind '{k}'"));
        }
        for s in self.cfg.switches.keys().filter(|s| !self.rules.iter().any(|r| &r.name == *s)) {
            errs.push(format!("engine.toml [switches]: no rule or action named '{s}'"));
        }
        for p in self.cfg.params.keys().filter(|p| !self.def.params.contains_key(*p)) {
            errs.push(format!("engine.toml [params]: '{p}' is not declared in game.ron params"));
        }
        for k in self.cfg.agent.controllable.iter().filter(|k| !known(k)) {
            errs.push(format!("engine.toml [agent] controllable: unknown kind '{k}'"));
        }
        for (name, k) in &self.def.kinds {
            if let Some(f) = &k.fsm
                && !self.def.fsms.contains_key(f)
            {
                errs.push(format!("kind '{name}': unknown fsm '{f}'"));
            }
            match self.kind_charts.get(name) {
                Some(c) => {
                    for st in k.glyphs.keys().filter(|st| c.resolve(st).is_empty()) {
                        errs.push(format!("kind '{name}': glyph for unknown state '{st}'"));
                    }
                }
                None => {
                    for st in k.glyphs.keys().filter(|st| *st != "-") {
                        errs.push(format!("kind '{name}': glyph for unknown state '{st}'"));
                    }
                }
            }
        }
        for (m, d) in &self.def.fsms {
            let kinds: Vec<String> = self
                .kind_charts
                .iter()
                .filter(|(_, c)| !c.with_origin(m, "").is_empty())
                .map(|(k, _)| k.clone())
                .collect();
            self.check_machine(m, "", d, &kinds, errs);
        }
        if let Some(l) = &self.def.layout {
            for (ch, entry) in &l.legend {
                let Some(kind) = self.def.kinds.get(entry.kind()) else {
                    errs.push(format!("layout legend '{ch}': unknown kind '{}'", entry.kind()));
                    continue;
                };
                for p in entry.props().into_iter().flatten().map(|(p, _)| p).filter(|p| !kind.props.contains_key(*p)) {
                    errs.push(format!("layout legend '{ch}': '{}' has no prop '{p}'", entry.kind()));
                }
            }
        }
        if !self.cfg.agent.seats.is_empty() {
            for k in &self.cfg.agent.controllable {
                if self.def.kinds.get(k).is_some_and(|d| !d.props.contains_key("owner")) {
                    errs.push(format!("engine.toml [agent] seats: controllable kind '{k}' needs an 'owner' prop"));
                }
            }
        }

        let mut seen = BTreeSet::new();
        let defs = self.def.rules.iter().chain(&self.def.actions).chain(&self.machine_rules).chain(&self.def.env_rules);
        for (r, def) in self.rules.iter().zip(defs) {
            let what = if r.is_action { "action" } else { "rule" };
            if !seen.insert(&r.name) {
                errs.push(format!("{what} '{}': duplicate name (switches need unique names)", r.name));
            }
            let subjects: Vec<String> = self.def.kinds.keys().filter(|k| Self::applies(r, k)).cloned().collect();
            match &r.home {
                Some((m, p)) => {
                    let place = if p.is_empty() { format!("fsm '{m}'") } else { format!("state '{p}' of fsm '{m}'") };
                    if def.for_kind.is_some() {
                        errs.push(format!("{what} '{}' is written inside {place}: drop `for`", r.name));
                    }
                    if def.state.is_some() {
                        errs.push(format!("{what} '{}' is written inside {place}: drop `state`", r.name));
                    }
                }
                None => {
                    match &def.for_kind {
                        None => errs.push(format!("{what} '{}': needs `for` (or write it inside a state)", r.name)),
                        Some(k) if k != "*" && !known(k) => errs.push(format!("{what} '{}': unknown kind '{k}'", r.name)),
                        _ => {}
                    }
                    if let Some(s) = &r.state
                        && r.bind.is_empty()
                    {
                        errs.push(format!("{what} '{}': state '{s}' does not exist for '{}'", r.name, r.for_kind));
                    }
                    if r.depth.is_some() && r.state.is_none() {
                        errs.push(format!("{what} '{}': `depth` needs `state`", r.name));
                    }
                }
            }
            match &r.target {
                None => {}
                Some(Target::Nearest(k)) if known(k) => {}
                Some(Target::NearestIn(k, sel)) if known(k) => {
                    self.check_selector(&format!("{what} '{}' target", r.name), std::slice::from_ref(k), sel, errs);
                }
                Some(Target::Nearest(k) | Target::NearestIn(k, _)) => {
                    errs.push(format!("{what} '{}': unknown target kind '{k}'", r.name))
                }
                Some(t) => {
                    errs.push(format!("{what} '{}': target must be Nearest(kind) or NearestIn(kind, state), got {t:?}", r.name))
                }
            }
            if !r.is_action && !r.args.is_empty() {
                errs.push(format!("rule '{}': only actions take args", r.name));
            }
            for d in &def.then {
                self.check_do(&r.name, def.target.as_ref(), &subjects, d, &subjects, errs);
            }
        }
    }

    /// Runs each expression once against each relevant kind's template:
    /// misspelled prop, undeclared parameter, wrong type → caught at load time.
    fn dry_run(&self, world: &World, errs: &mut Vec<String>) {
        let counts = self.counts(world);
        self.bind_world(Some(world));
        let synthetic = |kind: &str| {
            let (state, props) = self.template(kind);
            Entity { id: 0, kind: kind.into(), state, x: 0, y: 0, props }
        };
        for kind in self.def.kinds.keys() {
            let e = synthetic(kind);
            let base = self.base_scope(world, &e, &counts);
            self.bind(&e, 0);

            if let Some(c) = self.kind_charts.get(kind) {
                let mut scope = base.clone();
                scope.push_constant("roll", 0_i64);
                for n in &c.nodes {
                    let at = if n.path.is_empty() { "root".to_string() } else { format!("'{}'", n.path) };
                    let mut fail = |m: String| errs.push(format!("fsm state {at} on '{kind}': {m}"));
                    for t in &n.transitions {
                        if let Err(m) = self.eval_bool(&mut scope, &self.exprs[t.when]) {
                            fail(m);
                        }
                    }
                    if let sim_state::Shape::Or { pick: Some(p), .. } = &n.shape {
                        for (_, g, _) in &p.options {
                            let r = match p.kind {
                                PickKind::First => self.eval_bool(&mut scope, &self.exprs[*g]).map(|_| ()),
                                PickKind::Best => self.eval_int(&mut scope, &self.exprs[*g]).map(|_| ()),
                            };
                            if let Err(m) = r {
                                fail(m);
                            }
                        }
                    }
                    let blocks = n.enter.iter().chain(&n.exit).chain(n.transitions.iter().flat_map(|t| &t.then));
                    for &b in blocks {
                        let mut exprs = Vec::new();
                        self.blocks[b].iter().for_each(|d| d.int_exprs(&mut exprs));
                        for ast in exprs {
                            if let Err(m) = self.eval_int(&mut scope, ast) {
                                fail(m);
                            }
                        }
                    }
                }
            }
            for r in self.rules.iter().filter(|r| Self::applies(r, kind)) {
                let mut scope = base.clone();
                scope.push_constant("roll", 0_i64);
                if let Some(Target::Nearest(k) | Target::NearestIn(k, _)) = &r.target {
                    scope.push_constant("it", entity_map(&synthetic(k), Some(0)));
                }
                if r.is_action {
                    let arg: Map = r.args.iter().map(|a| (a.as_str().into(), Dynamic::from(0_i64))).collect();
                    scope.push_constant("arg", arg);
                }
                let mut fail = |m: String| errs.push(format!("'{}' on '{kind}': {m}", r.name));
                if let Some(w) = &r.when
                    && let Err(m) = self.eval_bool(&mut scope, w)
                {
                    fail(m);
                }
                let mut exprs = Vec::new();
                r.then.iter().for_each(|d| d.int_exprs(&mut exprs));
                for ast in exprs {
                    if let Err(m) = self.eval_int(&mut scope, ast) {
                        fail(m);
                    }
                }
                if let Some(s) = &r.script {
                    match self.rhai.eval_ast_with_scope::<Array>(&mut scope, s) {
                        Ok(items) => {
                            for item in items {
                                if let Err(m) = effect_from_map(&e, item) {
                                    fail(m);
                                }
                            }
                        }
                        Err(m) => fail(m.to_string()),
                    }
                }
            }
        }
        let mut scope = self.world_scope(world);
        for end in &self.ends {
            if let Err(m) = self.eval_bool(&mut scope, &end.when) {
                errs.push(format!("end '{}': {m}", end.result));
            }
        }
        if let Some(score) = &self.score {
            for k in &self.cfg.agent.controllable {
                let e = synthetic(k);
                let mut scope = self.base_scope(world, &e, &counts);
                if let Err(m) = self.eval_int(&mut scope, score) {
                    errs.push(format!("score on '{k}': {m}"));
                }
            }
        }
        self.bind_world(None);
    }
}

impl Rules for Game {
    fn validate(&self, world: &World) -> Result<(), Vec<String>> {
        let mut errs = self.compile_errors.clone();
        self.check_refs(&mut errs);
        if errs.is_empty() {
            self.dry_run(world, &mut errs);
        }
        if errs.is_empty() { Ok(()) } else { Err(errs) }
    }

    /// Evaluation only reads → entities are split across cores. Results merge in id
    /// order, so the core count does not change the result.
    fn eval(&self, world: &World) -> Vec<Group> {
        let counts = self.counts(world);
        let snap = Arc::new(world.clone());
        let entities: Vec<&Entity> = world.entities().values().collect();
        // Each worker builds its own scope once: no shared, locked values.
        let init = || self.tick_scope(world, &counts);
        let per = |scope: &mut Scope<'static>, e: &&Entity| {
            self.bind_snapshot(Some(&snap));
            self.eval_entity(world, e, scope)
        };
        // In a small world, distributing work costs more than the work: sequential path.
        let pool = self.pool.as_ref().filter(|_| entities.len() >= 4 * CHUNK);
        let groups: Vec<Vec<Group>> = match pool {
            Some(pool) => pool.install(|| entities.par_iter().with_min_len(CHUNK).map_init(init, per).collect()),
            None => {
                let mut scope = init();
                entities.iter().map(|e| per(&mut scope, e)).collect()
            }
        };
        groups.into_iter().flatten().collect()
    }

    /// Restored world: every entity is a known kind, its state valid in that kind's chart.
    fn check_world(&self, world: &World) -> Result<(), Vec<String>> {
        let mut errs = Vec::new();
        for e in world.entities().values() {
            if !self.def.kinds.contains_key(&e.kind) {
                errs.push(format!("entity {}: unknown kind '{}'", e.id, e.kind));
                continue;
            }
            match self.kind_charts.get(&e.kind) {
                Some(c) => {
                    if let Err(m) = c.decode(&e.state) {
                        errs.push(format!("entity {} ({}): {m}", e.id, e.kind));
                    }
                }
                None if e.state != "-" => errs.push(format!("entity {} ({}): has no fsm, state '{}'", e.id, e.kind, e.state)),
                None => {}
            }
        }
        if errs.is_empty() { Ok(()) } else { Err(errs) }
    }

    fn outcome(&self, world: &World) -> Option<String> {
        if self.ends.is_empty() {
            return None;
        }
        let mut scope = self.world_scope(world);
        for end in &self.ends {
            match self.eval_bool(&mut scope, &end.when) {
                Ok(true) => return Some(end.result.clone()),
                Ok(false) => {}
                Err(m) => return Some(format!("error: {m}")),
            }
        }
        None
    }
}

/// Evaluates chart conditions in Rhai; `me` is that entity.
struct RhaiOracle<'g, 'a, 's> {
    game: &'g Game,
    world: &'a World,
    e: &'a Entity,
    scope: &'s mut Scope<'static>,
}

impl Oracle<usize> for RhaiOracle<'_, '_, '_> {
    fn test(&mut self, g: &usize, salt: u64) -> Result<bool, String> {
        let len = self.scope.len();
        self.scope.push_constant("roll", self.world.roll(self.e.id, FSM_SALT + salt));
        self.game.bind(self.e, FSM_SALT + salt);
        let r = self.game.eval_bool(self.scope, &self.game.exprs[*g]);
        self.scope.rewind(len);
        r
    }
    fn score(&mut self, g: &usize, salt: u64) -> Result<i64, String> {
        let len = self.scope.len();
        self.scope.push_constant("roll", self.world.roll(self.e.id, FSM_SALT + salt));
        self.game.bind(self.e, FSM_SALT + salt);
        let r = self.game.eval_int(self.scope, &self.game.exprs[*g]);
        self.scope.rewind(len);
        r
    }
}

/// `Nearest(kind)` or `NearestIn(kind, state)`.
fn nearest<'w>(world: &'w World, from: &Entity, t: &Target) -> Option<(&'w Entity, i64)> {
    match t {
        Target::Nearest(k) => world.nearest(from, k),
        Target::NearestIn(k, sel) => world.nearest_where(from, k, |e| sim_state::in_label(&e.state, sel)),
        _ => None,
    }
}

/// The selector must resolve to a single state (`Goto`, `Interrupt`).
fn unique(c: &StateChart, sel: &str) -> Result<NodeId, String> {
    match c.resolve(sel)[..] {
        [one] => Ok(one),
        [] => Err(format!("unknown state '{sel}'")),
        _ => Err(format!("state '{sel}' is ambiguous; use a path like 'Parent.{sel}'")),
    }
}

/// A refused action's `Need` conditions, in readable form.
fn need_texts(d: &Do, out: &mut Vec<String>) {
    match d {
        Do::Need(p, e) => out.push(format!("{p} >= {e}")),
        Do::On(_, ds) => ds.iter().for_each(|d| need_texts(d, out)),
        _ => {}
    }
}

/// The neighbouring cell with the highest `prop` on a `kind` entity, if higher than here.
/// Candidates are visited from a shuffled start, so ties do not always pull the same way.
fn climb(world: &World, e: &Entity, kind: &str, prop: &str, salt: u64) -> Option<(i64, i64)> {
    let value = |x: i64, y: i64| {
        world.at(x, y).iter().map(|id| &world.entities()[id]).find(|x| x.kind == kind).and_then(|x| x.props.get(prop)).copied()
    };
    let cells = if world.is_solid(&e.kind) { world.free_neighbors(e.x, e.y) } else { world.neighbors(e.x, e.y) };
    if cells.is_empty() {
        return None;
    }
    let start = (world.rand(e.id, salt ^ 0x434C_494D) % cells.len() as u64) as usize;
    let mut best = (value(e.x, e.y).unwrap_or(0), None);
    for i in 0..cells.len() {
        let (x, y) = cells[(start + i) % cells.len()];
        if let Some(v) = value(x, y)
            && v > best.0
        {
            best = (v, Some((x - e.x, y - e.y)));
        }
    }
    best.1
}

fn wander(world: &World, e: &Entity, salt: u64) -> Effect {
    let r = world.rand(e.id, salt ^ 0x5741_4E44);
    Effect::Move { e: e.id, dx: (r % 3) as i64 - 1, dy: ((r / 3) % 3) as i64 - 1 }
}

/// Script output: maps like `#{op: "add", prop: "hunger", value: -1}`.
fn effect_from_map(e: &Entity, item: Dynamic) -> Result<Effect, String> {
    let m = item.try_cast::<Map>().ok_or("script must return an array of maps")?;
    let s = |k: &str| {
        m.get(k).and_then(|d| d.clone().into_string().ok()).ok_or(format!("effect map needs string '{k}'"))
    };
    let i = |k: &str| m.get(k).and_then(|d| d.as_int().ok()).ok_or(format!("effect map needs int '{k}'"));
    Ok(match s("op")?.as_str() {
        "set" => Effect::Set { e: e.id, prop: s("prop")?, v: i("value")? },
        "add" => Effect::Add { e: e.id, prop: s("prop")?, d: i("value")? },
        "emit" => Effect::Emit { e: e.id, name: s("name")? },
        "move" => Effect::Move { e: e.id, dx: i("dx")?, dy: i("dy")? },
        "despawn" => Effect::Despawn { e: e.id },
        op => return Err(format!("unknown op '{op}' (set|add|emit|move|despawn)")),
    })
}
