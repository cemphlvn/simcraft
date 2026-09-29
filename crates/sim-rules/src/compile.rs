//! game.ron + engine.toml → compiled rules (sim_core::Rules).
//! Expressions (when/Set/Add/Move) are Rhai expressions; `script` is full Rhai.
//! Scripts cannot change the world: they only return effect maps.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::marker::PhantomData;
use std::path::Path;
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(feature = "parallel")]
use rayon::prelude::*;

/// Worker threads for rule evaluation (none without the `parallel` feature, e.g. in the browser).
#[cfg(feature = "parallel")]
type Pool = rayon::ThreadPool;
#[cfg(not(feature = "parallel"))]
type Pool = ();
use rhai::{AST, Array, Dynamic, Engine as Rhai, Map, Scope};
use sim_core::{Effect, Entity, EntityId, Group, Rules, World, splitmix64};
use sim_state::{Chart, Memory, NoOracle, NodeId, Oracle, Outcome, PickKind, PickSpec, Spec, TransitionSpec};

use crate::brain::Brain;
use crate::config::EngineConfig;
use crate::env::NativeEnv;
use crate::game::{Do, EnvDef, GameDef, Perception, PickDef, RuleDef, StateDef, Target};

/// Compiled state chart: conditions index into `exprs`, action blocks into `blocks`.
type StateChart = Chart<usize, usize>;

/// `near.<kind>` when there is none.
pub const FAR: i64 = 9_999;

/// Keeps FSM transition salts from colliding with rule salts.
const FSM_SALT: u64 = 1 << 32;

/// Senses draw their own randomness.
const SENSE_SALT: u64 = 1 << 33;
const BRAIN_SALT: u64 = 1 << 34;
const ACTION_SALT: u64 = 1 << 35;
const MACHINE_SALT: u64 = 1 << 36;
const ENV_SALT: u64 = 1 << 37;

/// A salt from a rule's identity: a range bit plus 32 bits of an FNV hash of its names.
fn identity_salt(range: u64, parts: &[&str]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in parts {
        for b in p.bytes().chain(std::iter::once(0)) {
            h = (h ^ b as u64).wrapping_mul(0x0100_0000_01b3);
        }
    }
    range | (h & 0xFFFF_FFFF)
}
const GENOME_SALT: u64 = 0x6E0E_6E0E;

/// Work the rules do, counted exactly: the same run gives the same numbers on every machine and at any core count,
/// so they can be snapshot-tested. What efficient code is measured by (time is noise; work is not).
#[derive(Default)]
pub struct WorkCounters {
    evals: AtomicU64,
    queries: AtomicU64,
    maps: AtomicU64,
}

/// A reading of the counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct Work {
    /// Rhai expression evaluations (conditions, values, senses, scripts).
    pub evals: u64,
    /// Spatial and field queries (`near`, targets, `around`, `field`, `nearest_prop`, `toward_*`...).
    pub queries: u64,
    /// Entity maps built for expressions (`me`, `it`).
    pub maps: u64,
}

/// Per rule: how often it was checked (an entity it applies to) and how often it fired (produced a group).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct RuleWork {
    pub name: String,
    pub checks: u64,
    pub fires: u64,
}

fn bump(c: &AtomicU64) {
    c.fetch_add(1, Ordering::Relaxed);
}

/// A core takes at least this many entities as one job; below `PARALLEL_FROM` entities with something to evaluate
/// (idle kinds cost nothing), a tick stays on one core: small worlds pay no parallel overhead.
const CHUNK: usize = 16;
const PARALLEL_FROM: usize = 48;

/// Context seen by world queries registered in Rhai (`around`, `rand`).
/// Each core has its own context (thread-local): the world being read, `me`, salt.
#[derive(Default)]
struct QueryCtx {
    /// Set only while a `Bound` guard lives (see `Game::bind_world`): borrowed, never copied.
    world: Option<NonNull<World>>,
    /// The game's work counters (queries are counted where they happen).
    work: Option<Arc<WorkCounters>>,
    me: EntityId,
    pos: (i64, i64, i64),
    salt: u64,
    calls: u64,
    /// `me`'s kind and state text (for `in_state`, `steps_to`).
    kind: String,
    state: String,
}

impl QueryCtx {
    fn world(&self) -> Option<&World> {
        // SAFETY: `world` is set from a `&'w World` by `Game::bind_world` and reset when its `Bound<'w>` guard
        // drops; queries run synchronously on this thread inside that call, so the borrow is still live.
        self.world.map(|p| unsafe { p.as_ref() })
    }

    fn counted(&self) {
        if let Some(w) = &self.work {
            bump(&w.queries);
        }
    }

    fn around(&self, kind: &str, state: Option<&str>, r: i64) -> i64 {
        self.counted();
        let Some(w) = self.world() else { return 0 };
        match state {
            None => w.around(self.pos, self.me, kind, None, r),
            Some(sel) => w.around_where(self.pos, self.me, kind, r, |e| sim_state::in_label(&e.state, sel)),
        }
    }

    /// A field's value at my voxel offset by (dx, dy, dz); 0 outside the world.
    fn field(&self, name: &str, dx: i64, dy: i64, dz: i64) -> Result<i64, String> {
        self.counted();
        let Some(w) = self.world() else { return Ok(0) };
        if w.field_values(name).is_none() {
            return Err(format!("no field '{name}' (declare it in `fields`)"));
        }
        let (x, y, z) = self.pos;
        Ok(w.field(name, x + dx, y + dy, z + dz).unwrap_or(0))
    }

    /// A prop of the nearest `kind` within `r`; `default` if there is none that close.
    fn nearest_prop(&self, kind: &str, prop: &str, r: i64, default: i64) -> i64 {
        self.counted();
        let Some(w) = self.world() else { return default };
        let Some(me) = w.get(self.me) else { return default };
        match w.nearest(me, kind) {
            Some((e, d)) if d <= r => e.props.get(prop).copied().unwrap_or(default),
            _ => default,
        }
    }

    /// Which way the nearest `kind` (in that state, if given) lies along x (`axis` 0) or y (1): -1, 0 or 1;
    /// 0 if there is none.
    fn toward(&self, kind: &str, sel: Option<&str>, axis: u8) -> i64 {
        self.counted();
        let Some(w) = self.world() else { return 0 };
        let Some(me) = w.get(self.me) else { return 0 };
        let found = match sel {
            None => w.nearest(me, kind),
            Some(sel) => w.nearest_where(me, kind, |e| sim_state::in_label(&e.state, sel)),
        };
        found.map_or(0, |(t, _)| if axis == 0 { (t.x - me.x).signum() } else { (t.y - me.y).signum() })
    }

    /// Distance to the nearest `kind` in that state; FAR if none.
    fn near_in(&self, kind: &str, sel: &str) -> i64 {
        self.counted();
        let Some(w) = self.world() else { return FAR };
        let Some(me) = w.get(self.me) else { return FAR };
        w.nearest_where(me, kind, |e| sim_state::in_label(&e.state, sel)).map_or(FAR, |(_, d)| d)
    }

    /// True on `n` evenly spaced ticks out of every `secs` seconds of game time (Bresenham): an even cadence
    /// at any tick rate, where `rand` would give uneven gaps. Each entity has its own phase.
    fn pace(&self, n: i64, secs: i64, tick_rate: i64) -> bool {
        let Some(w) = self.world() else { return false };
        let period = secs.max(1) * tick_rate;
        if n <= 0 {
            return false;
        }
        if n >= period {
            return true;
        }
        let phase = (sim_core::splitmix64(self.me) % period as u64) as i64;
        let t = (w.tick as i64 + phase) % period;
        (t + 1) * n / period > t * n / period
    }

    /// 0..n. Multiple calls in one expression give different numbers; still fully deterministic.
    fn rand(&mut self, n: i64) -> i64 {
        if n <= 0 || self.world.is_none() {
            return 0;
        }
        self.calls += 1;
        let Some(w) = self.world() else { return 0 };
        (w.rand(self.me, self.salt ^ self.calls.wrapping_mul(0xA5A5_5A5A_1234_5678)) % n as u64) as i64
    }
}

thread_local! {
    static CTX: RefCell<QueryCtx> = RefCell::new(QueryCtx::default());
}

/// A world lent to this thread's queries; the lifetime keeps the world alive and unchanged while it is bound.
#[must_use = "the world is unbound when this guard drops"]
struct Bound<'w> {
    prev: Option<NonNull<World>>,
    _world: PhantomData<&'w World>,
}

impl Drop for Bound<'_> {
    fn drop(&mut self) {
        CTX.with(|c| c.borrow_mut().world = self.prev);
    }
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
    pool: Option<Pool>,
    /// `p`, built once. Each core copies it into its own scope once (no locks).
    p_map: Map,
    /// Kind → chart (kinds that have a machine).
    kind_charts: Arc<BTreeMap<String, Arc<StateChart>>>,
    /// Condition/score expressions and action blocks of the charts.
    exprs: Vec<AST>,
    /// `exprs[i]` checked natively, when it is a simple condition (see `native_guard`).
    expr_guards: Vec<Option<Vec<Term>>>,
    blocks: Vec<Vec<CDo>>,
    /// Environment kinds (hidden singletons), in `environments` order.
    envs: Vec<String>,
    /// `perception: Senses`: kinds see external reality only through their senses.
    strict: bool,
    /// Fields pinned onto level 0 every tick: (field, world-level expression).
    field_tops: Vec<(String, AST)>,
    /// Kind → its senses (name, expression).
    senses: BTreeMap<String, Vec<(String, AST)>>,
    /// Kind → its brain (learning agents).
    brains: BTreeMap<String, Arc<Brain>>,
    /// Work done so far (see `work`).
    work: Arc<WorkCounters>,
    /// Kind → how it is evaluated each tick (see `Plan`). Computed on first use (natives attach after compiling).
    plans: std::sync::OnceLock<BTreeMap<String, Plan>>,
    /// Fast paths on (native guards). Off only to prove they change nothing (conformance tests).
    fast: std::sync::atomic::AtomicBool,
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
    /// Work: entities it was checked for, groups it produced.
    checks: AtomicU64,
    fires: AtomicU64,
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
    /// `when`, checked natively when it is simple (see `native_guard`).
    guard: Option<Vec<Term>>,
    then: Vec<CDo>,
    script: Option<AST>,
    /// Which per-entity names its expressions mention.
    sees: Sees,
}

/// How a kind is evaluated each tick; every shortcut here is exact (`fast_paths_change_nothing` proves it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Plan {
    /// Nothing to evaluate (no rule, machine, senses, brain or native step): skipped.
    idle: bool,
    /// Every rule has a native guard (no senses, brain or native step): when the machine stays put and every guard
    /// is false, skipped before any scope is built.
    guarded: bool,
    /// Which per-entity maps to build.
    sees: Sees,
}

impl Plan {
    const FULL: Plan = Plan { idle: false, guarded: false, sees: Sees { me: true, near: true } };
}

/// Does a piece of source mention `me` / `near`? Read from the text, so it can only err towards building
/// a map nobody reads (a word in a string literal), never towards leaving out one that is read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Sees {
    me: bool,
    near: bool,
}

impl Sees {
    fn of(src: &str) -> Sees {
        let mut out = Sees::default();
        for w in src.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
            out.me |= w == "me";
            out.near |= w == "near";
        }
        out
    }

    fn or(self, o: Sees) -> Sees {
        Sees { me: self.me || o.me, near: self.near || o.near }
    }
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
    Move3(AST, AST, AST),
    MoveBy(AST, AST),
    SetField(String, AST),
    AddField(String, AST),
    SetFieldAt(String, AST, AST, AST, AST),
    ClimbField(String),
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
            CDo::Move3(dx, dy, dz) => out.extend([dx, dy, dz]),
            CDo::MoveBy(dx, dy) => out.extend([dx, dy]),
            CDo::SetField(_, a) | CDo::AddField(_, a) => out.push(a),
            CDo::SetFieldAt(_, dx, dy, dz, v) => out.extend([dx, dy, dz, v]),
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
            Do::Move3(dx, dy, dz) => CDo::Move3(self.expr(dx, ctx), self.expr(dy, ctx), self.expr(dz, ctx)),
            Do::MoveBy(dx, dy) => CDo::MoveBy(self.expr(dx, ctx), self.expr(dy, ctx)),
            Do::SetField(f, e) => CDo::SetField(f.clone(), self.expr(e, ctx)),
            Do::AddField(f, e) => CDo::AddField(f.clone(), self.expr(e, ctx)),
            Do::SetFieldAt(f, dx, dy, dz, v) => {
                CDo::SetFieldAt(f.clone(), self.expr(dx, ctx), self.expr(dy, ctx), self.expr(dz, ctx), self.expr(v, ctx))
            }
            Do::ClimbField(f) => CDo::ClimbField(f.clone()),
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
            checks: AtomicU64::new(0),
            fires: AtomicU64::new(0),
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
            guard: r.when.as_deref().and_then(native_guard),
            then: r.then.iter().map(|d| self.doo(d, &ctx)).collect(),
            script,
            sees: Sees::of(&format!("{r:?}")),
        }
    }
}

/// Collected from machines: expressions, action blocks, rules written in states.
#[derive(Default)]
struct Machines {
    exprs: Vec<AST>,
    expr_guards: Vec<Option<Vec<Term>>>,
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
            m.expr_guards.push(native_guard(&t.when));
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
                    m.expr_guards.push(None);
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

/// A state tree without its rules (they are checked as rules).
fn strip_rules(d: &mut StateDef) {
    d.rules.clear();
    d.states.values_mut().chain(d.layers.values_mut()).for_each(strip_rules);
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

/// Names in an expression that read the world directly (`env`, `tick`, `count`), outside string literals.
fn direct_reads(src: &str) -> Vec<&'static str> {
    let mut found = Vec::new();
    let chars: Vec<char> = src.chars().collect();
    let (mut i, mut in_str) = (0, false);
    while i < chars.len() {
        let ch = chars[i];
        let after_ident = i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_' || chars[i - 1] == '.');
        if ch == '"' {
            in_str = !in_str;
        } else if !in_str && (ch.is_alphabetic() || ch == '_') && !after_ident {
            let word: String = chars[i..].iter().take_while(|c| c.is_alphanumeric() || **c == '_').collect();
            for name in ["env", "tick", "count"] {
                if word == name && !found.contains(&name) {
                    found.push(name);
                }
            }
            i += word.chars().count();
            continue;
        }
        i += 1;
    }
    found
}

/// All expression sources in a `Do` tree (for near analysis).
fn do_sources<'a>(d: &'a Do, out: &mut Vec<&'a str>) {
    match d {
        Do::Set(_, e) | Do::Add(_, e) | Do::Need(_, e) => out.push(e),
        Do::Move(dx, dy) => out.extend([dx.as_str(), dy.as_str()]),
        Do::Move3(dx, dy, dz) => out.extend([dx.as_str(), dy.as_str(), dz.as_str()]),
        Do::MoveBy(dx, dy) => out.extend([dx.as_str(), dy.as_str()]),
        Do::SetField(_, e) | Do::AddField(_, e) => out.push(e),
        Do::SetFieldAt(_, dx, dy, dz, v) => out.extend([dx.as_str(), dy.as_str(), dz.as_str(), v.as_str()]),
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
/// A condition simple enough to check without the interpreter: comparisons of `me.<prop>` with a number or a
/// `p.<param>`, joined by `&&` (`me.scent > 0`, `me.timer == 0 && me.over == 0`). Most guards look like this, and
/// they are checked for every entity every tick.
#[derive(Clone, Debug, PartialEq)]
struct Term {
    prop: String,
    op: Cmp,
    rhs: Operand,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Cmp {
    Gt,
    Ge,
    Lt,
    Le,
    Eq,
    Ne,
}

#[derive(Clone, Debug, PartialEq)]
enum Operand {
    Lit(i64),
    Param(String),
}

fn ident(s: &str) -> bool {
    let mut c = s.chars();
    c.next().is_some_and(|f| f.is_ascii_alphabetic() || f == '_') && c.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The native form of a condition, or None (then the interpreter checks it).
fn native_guard(src: &str) -> Option<Vec<Term>> {
    if src.trim() == "true" {
        return Some(Vec::new());
    }
    if src.contains("||") || src.contains('(') || src.contains('"') || src.contains('#') {
        return None;
    }
    src.split("&&")
        .map(|t| {
            let t = t.trim();
            let (op, at, len) = [(">=", Cmp::Ge), ("<=", Cmp::Le), ("==", Cmp::Eq), ("!=", Cmp::Ne), (">", Cmp::Gt), ("<", Cmp::Lt)]
                .into_iter()
                .find_map(|(sym, op)| t.find(sym).map(|at| (op, at, sym.len())))?;
            let (lhs, rhs) = (t[..at].trim(), t[at + len..].trim());
            let prop = lhs.strip_prefix("me.").filter(|p| ident(p))?;
            let rhs = match rhs.strip_prefix("p.") {
                Some(p) if ident(p) => Operand::Param(p.to_string()),
                Some(_) => return None,
                None => Operand::Lit(rhs.parse().ok()?),
            };
            Some(Term { prop: prop.to_string(), op, rhs })
        })
        .collect()
}

/// The guard's value for this entity, or None if a name is not there (the interpreter then reports it).
fn check_guard(terms: &[Term], e: &Entity, params: &BTreeMap<String, i64>) -> Option<bool> {
    for t in terms {
        // As in `me`: props shadow the coordinates.
        let l = match e.props.get(&t.prop) {
            Some(v) => *v,
            None => match t.prop.as_str() {
                "x" => e.x,
                "y" => e.y,
                "z" => e.z,
                "id" => e.id as i64,
                _ => return None,
            },
        };
        let r = match &t.rhs {
            Operand::Lit(v) => *v,
            Operand::Param(p) => *params.get(p)?,
        };
        let ok = match t.op {
            Cmp::Gt => l > r,
            Cmp::Ge => l >= r,
            Cmp::Lt => l < r,
            Cmp::Le => l <= r,
            Cmp::Eq => l == r,
            Cmp::Ne => l != r,
        };
        if !ok {
            return Some(false);
        }
    }
    Some(true)
}

fn entity_map(e: &Entity, dist: Option<i64>) -> Map {
    let mut m = Map::new();
    m.insert("id".into(), Dynamic::from(e.id as i64));
    m.insert("kind".into(), Dynamic::from(e.kind.clone()));
    m.insert("state".into(), Dynamic::from(sim_state::active_part(&e.state).to_string()));
    m.insert("x".into(), Dynamic::from(e.x));
    m.insert("y".into(), Dynamic::from(e.y));
    m.insert("z".into(), Dynamic::from(e.z));
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
        let cfg_path = config.map(Path::to_path_buf).unwrap_or_else(|| dir.join("engine.toml"));
        let cfg_src = read(&cfg_path)?;
        Self::load_panel(dir, &cfg_src)
    }

    /// `dir/game.ron` (with its environments) and a panel given as text (tests override seeds and params).
    pub fn load_panel(dir: &Path, cfg_src: &str) -> Result<(World, Game), String> {
        let read = |p: &Path| std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()));
        let game_src = read(&dir.join("game.ron"))?;
        let def: GameDef = ron::from_str(&game_src).map_err(|e| format!("game.ron: {e}"))?;
        let mut envs = Vec::new();
        for name in &def.environments {
            // envs/<name>.ron in the game's folder or the nearest ancestor that has one.
            let found = dir.ancestors().map(|a| a.join("envs").join(format!("{name}.ron"))).find(|p| p.exists());
            let path = found.ok_or_else(|| format!("environment '{name}': no envs/{name}.ron next to or above {}", dir.display()))?;
            envs.push((name.clone(), read(&path)?));
        }
        Self::from_parts(&game_src, cfg_src, &envs)
    }

    pub fn from_strs(game_ron: &str, engine_toml: &str) -> Result<(World, Game), String> {
        Self::from_parts(game_ron, engine_toml, &[])
    }

    /// `envs`: (name, text of `envs/<name>.ron`) for every environment the game lists.
    pub fn from_parts(game_ron: &str, engine_toml: &str, envs: &[(String, String)]) -> Result<(World, Game), String> {
        let mut def: GameDef = ron::from_str(game_ron).map_err(|e| format!("game.ron: {e}"))?;
        let cfg: EngineConfig = toml::from_str(engine_toml).map_err(|e| format!("engine.toml: {e}"))?;
        let parsed: Vec<EnvDef> =
            envs.iter().map(|(n, src)| ron::from_str(src).map_err(|e| format!("envs/{n}.ron: {e}"))).collect::<Result<_, _>>()?;
        let merge_errors = def.merge_envs(&parsed).err().unwrap_or_default();
        let mut game = Self::compile(def, cfg);
        game.compile_errors.extend(merge_errors);
        let texts: Vec<&str> = envs.iter().flat_map(|(_, s)| ["\0", s.as_str()]).collect();
        game.source_hash = [game_ron, "\0", engine_toml]
            .into_iter()
            .chain(texts)
            .flat_map(|s| s.bytes())
            .fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3));
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
        rhai.register_fn("around", |kind: &str, state: &str, r: i64| CTX.with(|c| c.borrow().around(kind, Some(state), r)));
        rhai.register_fn("rand", |n: i64| CTX.with(|c| c.borrow_mut().rand(n)));
        let rate = cfg.run.tick_rate;
        rhai.register_fn("pace", move |n: i64| CTX.with(|c| c.borrow().pace(n, 1, rate)));
        rhai.register_fn("pace", move |n: i64, secs: i64| CTX.with(|c| c.borrow().pace(n, secs, rate)));
        rhai.register_fn("clamp", |x: i64, lo: i64, hi: i64| x.max(lo).min(hi));
        rhai.register_fn("pct", |x: i64, percent: i64| x * percent / 100);
        rhai.register_fn("ramp", |x: i64, len: i64, peak: i64| if len <= 0 { 0 } else { (x * peak / len).clamp(0, peak) });
        rhai.register_fn(
            "triangle",
            |x: i64, len: i64, peak: i64| {
                if len <= 0 || x < 0 || x > len { 0 } else { peak - (x * 2 * peak / len - peak).abs() }
            },
        );
        rhai.register_fn("toward_x", |kind: &str| CTX.with(|c| c.borrow().toward(kind, None, 0)));
        rhai.register_fn("toward_y", |kind: &str| CTX.with(|c| c.borrow().toward(kind, None, 1)));
        rhai.register_fn("toward_x", |kind: &str, sel: &str| CTX.with(|c| c.borrow().toward(kind, Some(sel), 0)));
        rhai.register_fn("toward_y", |kind: &str, sel: &str| CTX.with(|c| c.borrow().toward(kind, Some(sel), 1)));
        rhai.register_fn("near_in", |kind: &str, sel: &str| CTX.with(|c| c.borrow().near_in(kind, sel)));
        rhai.register_fn("field", |name: &str| -> Result<i64, Box<rhai::EvalAltResult>> {
            CTX.with(|c| c.borrow().field(name, 0, 0, 0)).map_err(Into::into)
        });
        rhai.register_fn("field_at", |name: &str, dx: i64, dy: i64, dz: i64| -> Result<i64, Box<rhai::EvalAltResult>> {
            CTX.with(|c| c.borrow().field(name, dx, dy, dz)).map_err(Into::into)
        });
        rhai.register_fn("nearest_prop", |kind: &str, prop: &str, r: i64, default: i64| {
            CTX.with(|c| c.borrow().nearest_prop(kind, prop, r, default))
        });
        rhai.register_fn("in_state", |sel: &str| CTX.with(|c| sim_state::in_label(&c.borrow().state, sel)));
        rhai.register_fn("in_state", |m: Map, sel: &str| sim_state::in_label(&map_str(&m, "state"), sel));
        rhai.register_fn("depth_in", |sel: &str| CTX.with(|c| sim_state::depth_in_label(&c.borrow().state, sel)));
        rhai.register_fn("depth_in", |m: Map, sel: &str| sim_state::depth_in_label(&map_str(&m, "state"), sel));
        #[cfg(feature = "parallel")]
        let pool = match cfg.run.threads {
            1 => None,
            n => rayon::ThreadPoolBuilder::new().num_threads(n).build().ok(),
        };
        #[cfg(not(feature = "parallel"))]
        let pool: Option<Pool> = None;

        let mut cc = Compiler { rhai: &rhai, errors: Vec::new() };
        let mut mach = Machines::default();
        let specs: BTreeMap<String, Spec<usize, usize>> =
            def.fsms.iter().map(|(name, f)| (name.clone(), cc.spec(&mut mach, name, "", f, true))).collect();
        // Salts: top-level rule i → i+1 (append new rules at the end and nothing shifts). Actions and rules written
        // in states get a salt from who they are (name, machine, state), not from where they stand: adding an action
        // or a rule anywhere never changes the dice of the others.
        let mut rules: Vec<CompiledRule> = def.rules.iter().enumerate().map(|(i, r)| cc.rule(r, i as u64 + 1, false, &cfg)).collect();
        rules.extend(def.actions.iter().map(|r| cc.rule(r, identity_salt(ACTION_SALT, &["action", &r.name]), true, &cfg)));
        for (m, p, r) in &mach.rules {
            let mut c = cc.rule(r, identity_salt(MACHINE_SALT, &[m, p, &r.name]), false, &cfg);
            c.home = Some((m.clone(), p.clone()));
            rules.push(c);
        }
        // Environment rules after everything, salted by name too: an environment's dice do not depend on the game.
        rules.extend(def.env_rules.iter().map(|r| cc.rule(r, identity_salt(ENV_SALT, &["env", &r.name]), false, &cfg)));
        let ends = def
            .end
            .iter()
            .map(|e| CompiledEnd { when: cc.expr(&e.when, &format!("end '{}'", e.result)), result: e.result.clone() })
            .collect();
        let score = def.score.as_deref().map(|s| cc.expr(s, "score"));
        let field_tops: Vec<(String, AST)> =
            def.fields.iter().filter_map(|(n, f)| Some((n.clone(), cc.expr(f.top.as_ref()?, &format!("field '{n}' top"))))).collect();
        let senses: BTreeMap<String, Vec<(String, AST)>> = def
            .kinds
            .iter()
            .filter(|(_, k)| !k.senses.is_empty())
            .map(|(kind, k)| {
                let compiled = k.senses.iter().map(|(n, src)| (n.clone(), cc.expr(src, &format!("kind '{kind}' sense '{n}'")))).collect();
                (kind.clone(), compiled)
            })
            .collect();
        let mut brain_inputs: BTreeMap<String, Vec<(String, AST)>> = BTreeMap::new();
        for (kind, k) in &def.kinds {
            if let Some(b) = &k.brain {
                let inputs =
                    b.inputs.iter().map(|(n, src)| (n.clone(), cc.expr(src, &format!("kind '{kind}' brain input '{n}'")))).collect();
                brain_inputs.insert(kind.clone(), inputs);
            }
        }
        let mut errors = cc.errors;
        let mut brains = BTreeMap::new();
        for (kind, inputs) in brain_inputs {
            let def_b = def.kinds[&kind].brain.as_ref().expect("has a brain");
            if def.perception != Perception::Senses {
                errors.push(format!("kind '{kind}': a brain needs `perception: Senses` (its inputs are what it senses)"));
            }
            match Brain::new(def_b, inputs) {
                Ok(b) => {
                    brains.insert(kind, Arc::new(b));
                }
                Err(e) => errors.push(format!("kind '{kind}' brain: {e}")),
            }
        }

        let mut charts = BTreeMap::new();
        for name in def.fsms.keys() {
            match Chart::build(name, &specs) {
                Ok(c) => {
                    charts.insert(name.clone(), Arc::new(c));
                }
                Err(es) => errors.extend(es.into_iter().map(|e| format!("fsm '{name}': {e}"))),
            }
        }
        let kind_charts: BTreeMap<String, Arc<StateChart>> =
            def.kinds.iter().filter_map(|(k, kd)| Some((k.clone(), charts.get(kd.fsm.as_ref()?)?.clone()))).collect();
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
        sources.extend(def.kinds.values().flat_map(|k| k.senses.values().map(String::as_str)));
        sources.extend(def.kinds.values().filter_map(|k| k.brain.as_ref()).flat_map(|b| b.inputs.values().map(String::as_str)));
        sources.extend(def.score.as_deref());
        let near_kinds = near_refs(sources.into_iter());

        let envs = def.env_kinds.clone();
        let strict = def.perception == Perception::Senses;
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
            strict,
            field_tops,
            senses,
            brains,
            work: Arc::default(),
            plans: std::sync::OnceLock::new(),
            fast: std::sync::atomic::AtomicBool::new(true),
            natives: BTreeMap::new(),
            machine_rules: mach.rules.into_iter().map(|(_, _, r)| r).collect(),
            exprs: mach.exprs,
            expr_guards: mach.expr_guards,
            blocks: mach.blocks,
            initial_states,
        }
    }

    /// World size from `layout` or the panel's `[world]`. Layout is placed first,
    /// then `[spawn]`. Solid kinds are placed one by one into shuffled empty cells.
    fn initial_world(&self) -> (World, Vec<String>) {
        let mut errs = Vec::new();
        let (w, h, d) = match (&self.def.layout, &self.cfg.world) {
            (Some(l), Some(wc)) => {
                if l.size3() != (wc.width, wc.height, wc.depth) {
                    let (lw, lh, ld) = l.size3();
                    errs.push(format!(
                        "engine.toml [world] {}x{}x{} does not match game.ron layout {lw}x{lh}x{ld}",
                        wc.width, wc.height, wc.depth
                    ));
                }
                l.size3()
            }
            (Some(l), None) => l.size3(),
            (None, Some(wc)) => (wc.width, wc.height, wc.depth),
            (None, None) => {
                errs.push("engine.toml: [world] is required when game.ron has no layout".into());
                (1, 1, 1)
            }
        };
        let mut world = World::new3(self.cfg.run.seed, w, h, d);
        world.set_solid(self.def.kinds.iter().filter(|(_, k)| k.solid).map(|(n, _)| n.clone()).collect());
        world.set_cling(self.def.kinds.iter().filter(|(_, k)| k.cling).map(|(n, _)| n.clone()).collect());
        for (name, f) in &self.def.fields {
            world.add_field(name, f.init);
            if let Some(from) = f.from_level {
                for z in 0..from.clamp(0, d) {
                    for y in 0..h {
                        for x in 0..w {
                            world.set_field(name, x, y, z, 0);
                        }
                    }
                }
            }
        }
        world.set_terrain(self.def.terrain.clone());

        if let Some(l) = &self.def.layout {
            if !l.rows.is_empty() && !l.levels.is_empty() {
                errs.push("layout: use `rows` (one level) or `levels` (3D), not both".into());
            }
            for (z, level) in l.all_levels().into_iter().enumerate() {
                for (y, row) in level.iter().enumerate() {
                    for (x, ch) in row.chars().enumerate() {
                        let (x, y, z) = (x as i64, y as i64, z as i64);
                        if let Some(values) = l.cells.get(&ch) {
                            for (name, v) in values {
                                world.set_field(name, x, y, z, *v);
                            }
                        }
                        if ch == '.' || ch == ' ' || (l.cells.contains_key(&ch) && !l.legend.contains_key(&ch)) {
                            continue;
                        }
                        let Some(entry) = l.legend.get(&ch) else {
                            errs.push(format!("layout level {z}, row {y}, column {x}: '{ch}' is not in the legend or cells"));
                            continue;
                        };
                        let kind = entry.kind();
                        if !self.def.kinds.contains_key(kind) {
                            continue; // check_refs reports it
                        }
                        let (state, mut props) = self.template(kind);
                        props.extend(entry.props().into_iter().flatten().map(|(k, v)| (k.clone(), *v)));
                        world.spawn3(kind, &state, x, y, z, props);
                    }
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
            if def.solid || def.cling {
                let placed = self.place_shuffled(&mut world, kind, *n);
                if placed < *n {
                    let room = if def.cling { "voxels against terrain" } else { "free cells" };
                    errs.push(format!("engine.toml [spawn]: {kind} = {n}, but only {placed} {room}"));
                }
                continue;
            }
            for _ in 0..*n {
                let x = splitmix64(world.seed.wrapping_add(2 * c)) % world.width as u64;
                let y = splitmix64(world.seed.wrapping_add(2 * c + 1)) % world.height as u64;
                // A level only in 3D, from its own stream: 2D worlds draw exactly as before.
                let z = if world.depth > 1 { splitmix64(world.seed ^ 0x5A5A_0000 ^ c) % world.depth as u64 } else { 0 };
                let (state, props) = self.template(kind);
                world.spawn3(kind, &state, x as i64, y as i64, z as i64, props);
                c += 1;
            }
        }
        // Learning agents start with random weights, each its own.
        for (kind, b) in &self.brains {
            let ids: Vec<u64> = world.of_kind(kind).map(|e| e.id).collect();
            for id in ids {
                world.set_genome(id, b.fresh(splitmix64(world.seed ^ GENOME_SALT ^ id.wrapping_mul(0x9E37_79B9_7F4A_7C15))));
            }
        }
        (world, errs)
    }

    /// Solid and clinging kinds: one by one into shuffled voxels where they can stand.
    fn place_shuffled(&self, world: &mut World, kind: &str, n: u32) -> u32 {
        let mut cells: Vec<i64> = (0..world.width * world.height * world.depth).collect();
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
            let (x, y, z) = (cell % world.width, (cell / world.width) % world.height, cell / (world.width * world.height));
            if world.can_enter(kind, x, y, z) && world.spawn3(kind, &state, x, y, z, props).is_some() {
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

    /// The work done since the game was loaded (or since `reset_work`): exact, deterministic counts.
    pub fn work(&self) -> Work {
        let w = &self.work;
        let g = |c: &AtomicU64| c.load(Ordering::Relaxed);
        Work { evals: g(&w.evals), queries: g(&w.queries), maps: g(&w.maps) }
    }

    /// Per rule (in salt order: rules, actions, rules in states, environment rules): checks and fires so far.
    pub fn rule_work(&self) -> Vec<RuleWork> {
        self.rules
            .iter()
            .map(|r| RuleWork { name: r.name.clone(), checks: r.checks.load(Ordering::Relaxed), fires: r.fires.load(Ordering::Relaxed) })
            .collect()
    }

    /// Turns the fast paths (native guards, skipping idle kinds) off or on: the interpreter alone must give the
    /// same world, tick for tick (see the conformance test).
    pub fn set_fast_paths(&self, on: bool) {
        self.fast.store(on, Ordering::Relaxed);
    }

    pub fn reset_work(&self) {
        for c in [&self.work.evals, &self.work.queries, &self.work.maps] {
            c.store(0, Ordering::Relaxed);
        }
        for r in &self.rules {
            r.checks.store(0, Ordering::Relaxed);
            r.fires.store(0, Ordering::Relaxed);
        }
    }

    /// What an entity senses right now (for inspectors): sense name → value as text.
    pub fn senses_of(&self, world: &World, e: &Entity) -> Vec<(String, String)> {
        if !self.senses.contains_key(&e.kind) && !self.brains.contains_key(&e.kind) {
            return Vec::new();
        }
        let counts = self.counts(world);
        let bound = self.bind_world(world);
        let mut full = self.tick_scope(world, &counts);
        let (map, errs) = self.sense_map(&mut full, world, e);
        drop(bound);
        let mut out: Vec<(String, String)> = map.into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        out.extend(errs.into_iter().map(|m| ("error".to_string(), m)));
        out
    }

    /// Evaluates a world-level expression (what `end` sees: `tick`, `p`, `count`, `env`), plus `extra` values
    /// (test runners add `states`, `sum`, `events`...). For tests and tools, not for the tick loop.
    pub fn eval_world(&self, world: &World, src: &str, extra: Map) -> Result<Dynamic, String> {
        let ast = self.rhai.compile_expression(src).map_err(|e| format!("`{src}`: {e}"))?;
        let mut scope = self.world_scope(world);
        for (k, v) in extra {
            scope.push_constant(k.to_string(), v);
        }
        let bound = self.bind_world(world);
        let out = self.rhai.eval_ast_with_scope::<Dynamic>(&mut scope, &ast).map_err(|e| format!("`{src}`: {e}"));
        drop(bound);
        out
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
    pub fn act(&self, world: &World, seat: Option<&str>, id: EntityId, name: &str, args: &BTreeMap<String, i64>) -> Result<Group, String> {
        let rule =
            self.rules.iter().find(|r| r.is_action && r.name == name).ok_or_else(|| format!("unknown action '{name}' (see info)"))?;
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
        let bound = self.bind_world(world);
        let mut scope = self.entity_scope(world, e, &counts);
        let arg: Map = args.iter().map(|(k, v)| (k.as_str().into(), Dynamic::from(*v))).collect();
        scope.push_constant("arg", arg);
        let out = self.eval_rule(world, e, rule, &mut scope);
        drop(bound);

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
        let bound = self.bind_world(world);
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
        drop(bound);
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
        scope.push_constant("tick_rate", self.cfg.run.tick_rate);
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
        self.push_seen(scope, world, e, Sees { me: true, near: true });
    }

    /// `me` and `near`, each only if `sees` says an expression reads it.
    fn push_seen(&self, scope: &mut Scope<'static>, world: &World, e: &Entity, sees: Sees) {
        if sees.me {
            bump(&self.work.maps);
            scope.push_constant("me", entity_map(e, None));
        }
        if !sees.near {
            return;
        }
        let near: Map = self
            .def
            .kinds
            .keys()
            .filter(|k| self.near_kinds.as_ref().is_none_or(|ks| ks.contains(*k)))
            .map(|k| {
                bump(&self.work.queries);
                (k.as_str().into(), Dynamic::from(world.nearest(e, k).map_or(FAR, |(_, d)| d)))
            })
            .collect();
        scope.push_constant("near", near);
    }

    /// World seen by rule expressions: p, tick, count, me, near (+ per-rule roll, it, arg).
    fn base_scope(&self, world: &World, e: &Entity, counts: &Map) -> Scope<'static> {
        let mut scope = self.tick_scope(world, counts);
        self.push_entity(&mut scope, world, e);
        scope
    }

    /// Does this kind see the world only through its senses?
    fn agent_view(&self, kind: &str) -> bool {
        self.strict && !self.envs.iter().any(|e| e == kind)
    }

    /// What an agent's own expressions start from: its params and the tick rate, nothing external.
    /// (`pace` is the agent's own rhythm, like `rand`: allowed without a sense.)
    fn agent_scope(&self) -> Scope<'static> {
        let mut scope = Scope::new();
        scope.push_constant("p", self.p_map.clone());
        scope.push_constant("tick_rate", self.cfg.run.tick_rate);
        scope
    }

    /// The entity's senses, evaluated in the full world. Errors are returned, the rest still counts.
    fn sense_map(&self, full: &mut Scope<'static>, world: &World, e: &Entity) -> (Map, Vec<String>) {
        let mut out = Map::new();
        let mut errs = Vec::new();
        if let Some(senses) = self.senses.get(&e.kind) {
            let len = full.len();
            self.push_entity(full, world, e);
            self.bind(e, SENSE_SALT);
            for (name, ast) in senses {
                bump(&self.work.evals);
                match self.rhai.eval_ast_with_scope::<Dynamic>(full, ast) {
                    Ok(v) => {
                        out.insert(name.as_str().into(), v);
                    }
                    Err(m) => errs.push(format!("sense '{name}': {m}")),
                }
            }
            full.rewind(len);
        }
        // The brain thinks after the senses, on what the kind's rules would see; its choice becomes a sense.
        if let Some(b) = self.brains.get(&e.kind) {
            match self.think(b, world, e, &out) {
                Ok(choice) => {
                    out.insert(b.def.sense.as_str().into(), Dynamic::from(choice));
                }
                Err(m) => errs.push(format!("brain: {m}")),
            }
        }
        (out, errs)
    }

    /// The brain's choice for this entity: inputs evaluated in the agent's own view, then its genome runs.
    fn think(&self, b: &Brain, world: &World, e: &Entity, sense: &Map) -> Result<String, String> {
        let mut scope = self.agent_scope();
        self.push_entity(&mut scope, world, e);
        scope.push_constant("sense", sense.clone());
        self.bind(e, BRAIN_SALT);
        let values = b
            .inputs
            .iter()
            .map(|(n, ast)| self.eval_int(&mut scope, ast).map_err(|m| format!("input '{n}': {m}")))
            .collect::<Result<Vec<i64>, String>>()?;
        let fresh;
        let genome = if e.genome.len() == b.genes {
            e.genome.as_slice()
        } else {
            fresh = b.fresh(splitmix64(world.seed ^ GENOME_SALT ^ e.id));
            fresh.as_slice()
        };
        let i = b.think(genome, &values)?;
        Ok(b.def.outputs[i.min(b.def.outputs.len() - 1)].clone())
    }

    /// The scope an entity's own expressions see: the full world, or (perception: Senses) me + sense.
    fn entity_scope(&self, world: &World, e: &Entity, counts: &Map) -> Scope<'static> {
        if !self.agent_view(&e.kind) {
            return self.base_scope(world, e, counts);
        }
        let mut full = self.tick_scope(world, counts);
        let (sense, _) = self.sense_map(&mut full, world, e);
        let mut scope = self.agent_scope();
        self.push_entity(&mut scope, world, e);
        scope.push_constant("sense", sense);
        scope
    }

    /// A load error about reading the world directly gets a pointer to senses.
    fn hint(&self, m: String) -> String {
        let direct = ["Variable not found: env", "Variable not found: tick", "Variable not found: count"];
        if self.strict && direct.iter().any(|d| m.contains(d)) {
            format!("{m} (agents perceive the world through their kind's `senses`; see Perception in docs/architecture.md)")
        } else {
            m
        }
    }

    /// World seen by `end` expressions: p, tick, count.
    fn world_scope(&self, world: &World) -> Scope<'static> {
        self.tick_scope(world, &self.counts(world))
    }

    /// Binds the query context to an entity and salt (before each evaluation).
    fn bind(&self, e: &Entity, salt: u64) {
        CTX.with(|c| {
            let mut c = c.borrow_mut();
            (c.me, c.pos, c.salt, c.calls) = (e.id, (e.x, e.y, e.z), salt, 0);
            if c.state != e.state {
                c.state.clone_from(&e.state);
            }
            if c.kind != e.kind {
                c.kind.clone_from(&e.kind);
            }
        });
    }

    /// Lends `world` to this core's queries until the guard drops (then the previous binding is back).
    /// Borrowed, not copied: binding is as cheap on a 10 000-entity world as on an empty one.
    fn bind_world<'w>(&self, world: &'w World) -> Bound<'w> {
        CTX.with(|c| {
            let mut c = c.borrow_mut();
            if c.work.as_ref().map(Arc::as_ptr) != Some(Arc::as_ptr(&self.work)) {
                c.work = Some(self.work.clone());
            }
            Bound { prev: c.world.replace(NonNull::from(world)), _world: PhantomData }
        })
    }

    /// How a kind is evaluated each tick (computed on first use: natives attach after compiling).
    fn plan(&self, kind: &str) -> Plan {
        if !self.fast.load(Ordering::Relaxed) {
            return Plan::FULL;
        }
        let plans = self.plans.get_or_init(|| {
            // Machines are scanned whole (a kind's chart may be built from several); only errs towards `true`.
            let machines = Sees::of(&format!("{:?}", self.def.fsms));
            self.def
                .kinds
                .iter()
                .map(|(k, def)| {
                    let rules: Vec<&CompiledRule> =
                        self.rules.iter().filter(|r| !r.is_action && r.enabled && Self::applies(r, k)).collect();
                    let (native, chart) = (self.natives.contains_key(k), self.kind_charts.contains_key(k));
                    let plain = !native && !self.senses.contains_key(k) && !self.brains.contains_key(k);
                    let sees = if native {
                        Sees::default()
                    } else {
                        let r = rules.iter().fold(Sees::default(), |a, r| a.or(r.sees));
                        r.or(if chart { machines } else { Sees::default() }).or(Sees::of(&format!("{def:?}")))
                    };
                    let plan =
                        Plan { idle: plain && !chart && rules.is_empty(), guarded: plain && rules.iter().all(|r| r.guard.is_some()), sees };
                    (k.clone(), plan)
                })
                .collect()
        });
        plans.get(kind).copied().unwrap_or(Plan::FULL)
    }

    /// A guarded kind produces nothing here if its machine (checked natively) stays put and every rule's guard is
    /// false: known without building a scope. Anything that needs the interpreter goes the full way.
    fn all_guards_false(&self, e: &Entity) -> bool {
        let Ok(mem) = self.memory(e) else { return false };
        if let (Some(c), Some(m)) = (self.kind_charts.get(&e.kind), &mem)
            && !c.step(m, &mut GuardOracle { game: self, e }).is_ok_and(|o| !o.changed)
        {
            return false;
        }
        let mut rules =
            self.rules.iter().filter(|r| !r.is_action && r.enabled && Self::applies(r, &e.kind) && self.bound(r, e, mem.as_ref()));
        let quiet = rules.clone().all(|r| r.guard.as_deref().and_then(|g| check_guard(g, e, &self.params)) == Some(false));
        if quiet {
            rules.by_ref().for_each(|r| bump(&r.checks));
        }
        quiet
    }

    /// All of an entity's groups this tick: FSM transition first, then rules (in order).
    fn eval_entity(&self, world: &World, e: &Entity, scopes: &mut Scopes) -> Vec<Group> {
        let plan = self.plan(&e.kind);
        if plan.idle || (plan.guarded && self.all_guards_false(e)) {
            return Vec::new();
        }
        if !self.agent_view(&e.kind) {
            let scope = &mut scopes.full;
            let len = scope.len();
            self.push_seen(scope, world, e, plan.sees);
            let out = self.eval_entity_in(world, e, scope);
            scope.rewind(len);
            return out;
        }
        let (sense, errs) = self.sense_map(&mut scopes.full, world, e);
        let scope = &mut scopes.agent;
        let len = scope.len();
        self.push_seen(scope, world, e, plan.sees);
        scope.push_constant("sense", sense);
        let mut out: Vec<Group> = errs
            .into_iter()
            .map(|m| Group {
                source: "sense".into(),
                actor: Some(e.id),
                effects: vec![Effect::Emit { e: e.id, name: format!("error: {m}") }],
            })
            .collect();
        out.extend(self.eval_entity_in(world, e, scope));
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
            let mut effects: Vec<Effect> =
                props.into_iter().filter(|(k, v)| e.props.get(k) != Some(v)).map(|(prop, v)| Effect::Set { e: e.id, prop, v }).collect();
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
            match stepped.and_then(|o| self.state_group(world, e, c, &o, base)) {
                Ok(Some(g)) => out.push(g),
                Ok(None) => {}
                Err(m) => out.push(error("fsm", m)),
            }
        }

        let active = self.rules.iter().filter(|r| !r.is_action && r.enabled && Self::applies(r, &e.kind) && self.bound(r, e, mem.as_ref()));
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
        o: &Outcome<usize>,
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
        bump(&self.work.evals);
        self.rhai.eval_ast_with_scope::<bool>(scope, ast).map_err(|e| e.to_string())
    }

    fn eval_int(&self, scope: &mut Scope, ast: &AST) -> Result<i64, String> {
        bump(&self.work.evals);
        self.rhai.eval_ast_with_scope::<i64>(scope, ast).map_err(|e| e.to_string())
    }

    /// No copy: per-rule variables are pushed, then the scope is rewound.
    fn eval_rule(&self, world: &World, e: &Entity, rule: &CompiledRule, scope: &mut Scope<'static>) -> Result<Option<Group>, String> {
        let len = scope.len();
        let out = self.eval_rule_in(world, e, rule, scope);
        scope.rewind(len);
        out
    }

    fn eval_rule_in(&self, world: &World, e: &Entity, rule: &CompiledRule, scope: &mut Scope<'static>) -> Result<Option<Group>, String> {
        bump(&rule.checks);
        // A simple condition is checked without the interpreter; false ends here, before any other work.
        let native = rule.guard.as_deref().filter(|_| self.fast.load(Ordering::Relaxed)).and_then(|g| check_guard(g, e, &self.params));
        if native == Some(false) {
            return Ok(None);
        }
        scope.push_constant("roll", world.roll(e.id, rule.salt));
        self.bind(e, rule.salt);

        let it = match &rule.target {
            None => None,
            Some(t @ (Target::Nearest(_) | Target::NearestIn(..))) => {
                bump(&self.work.queries);
                let Some((t, d)) = nearest(world, e, t) else { return Ok(None) };
                bump(&self.work.maps);
                scope.push_constant("it", entity_map(t, Some(d)));
                Some(t)
            }
            Some(t) => return Err(format!("target must be Nearest(kind) or NearestIn(kind, state), got {t:?}")),
        };

        if native.is_none()
            && let Some(w) = &rule.when
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
            bump(&self.work.evals);
            let out: Array = self.rhai.eval_ast_with_scope(scope, script).map_err(|e| e.to_string())?;
            for item in out {
                effects.push(effect_from_map(e, item)?);
            }
        }

        if effects.is_empty() {
            return Ok(None);
        }
        bump(&rule.fires);
        Ok(Some(Group { source: rule.name.clone(), actor: Some(e.id), effects }))
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
                let genome = self.brains.get(k).map_or_else(Vec::new, |b| {
                    let parent = (subj.kind == *k).then_some(subj.genome.as_slice());
                    b.child(parent, world.rand(subj.id, salt ^ GENOME_SALT))
                });
                out.push(Effect::Spawn { kind: k.clone(), state, x: subj.x, y: subj.y, z: subj.z, props, genome });
            }
            CDo::MoveToward(k) => {
                if let Some((t, _)) = world.nearest(subj, k) {
                    out.push(Effect::Move { e: subj.id, dx: t.x - subj.x, dy: t.y - subj.y, dz: t.z - subj.z });
                }
            }
            CDo::MoveAway(k) => match world.nearest(subj, k) {
                Some((_, 0)) => out.push(wander(world, subj, salt)),
                Some((t, _)) => out.push(Effect::Move { e: subj.id, dx: subj.x - t.x, dy: subj.y - t.y, dz: subj.z - t.z }),
                None => {}
            },
            CDo::Wander => out.push(wander(world, subj, salt)),
            CDo::Climb(k, prop) => {
                if let Some((dx, dy, dz)) = climb(world, subj, k, prop, salt) {
                    out.push(Effect::Move { e: subj.id, dx, dy, dz });
                }
            }
            CDo::Goto(_) | CDo::Interrupt(_) | CDo::Back => self.change_state(world, who, subj, d, salt, scope, out)?,
            CDo::Move(dx, dy) => {
                let (dx, dy) = (self.eval_int(scope, dx)?, self.eval_int(scope, dy)?);
                // Move takes one step, whatever it is asked: say so, or a "move 2" silently becomes a move 1.
                if dx.abs() > 1 || dy.abs() > 1 {
                    out.push(Effect::Emit { e: subj.id, name: format!("clamped: Move({dx}, {dy}) takes one step; MoveBy moves exactly") });
                }
                out.push(Effect::Move { e: subj.id, dx, dy, dz: 0 });
            }
            CDo::Move3(dx, dy, dz) => {
                let (dx, dy, dz) = (self.eval_int(scope, dx)?, self.eval_int(scope, dy)?, self.eval_int(scope, dz)?);
                out.push(Effect::Move { e: subj.id, dx, dy, dz });
            }
            CDo::MoveBy(dx, dy) => {
                let (dx, dy) = (self.eval_int(scope, dx)?, self.eval_int(scope, dy)?);
                out.push(Effect::MoveBy { e: subj.id, dx, dy, dz: 0 });
            }
            CDo::SetField(name, ast) => {
                let v = self.eval_int(scope, ast)?;
                out.push(Effect::FieldSet { name: name.clone(), x: subj.x, y: subj.y, z: subj.z, v });
            }
            CDo::AddField(name, ast) => {
                let d = self.eval_int(scope, ast)?;
                out.push(Effect::FieldAdd { name: name.clone(), x: subj.x, y: subj.y, z: subj.z, d });
            }
            // Exactly there (a reach): the game guards how far, e.g. `when: "abs(arg.dx) <= p.reach"`.
            CDo::SetFieldAt(name, dx, dy, dz, v) => {
                let (dx, dy, dz) = (self.eval_int(scope, dx)?, self.eval_int(scope, dy)?, self.eval_int(scope, dz)?);
                let v = self.eval_int(scope, v)?;
                out.push(Effect::FieldSet { name: name.clone(), x: subj.x + dx, y: subj.y + dy, z: subj.z + dz, v });
            }
            CDo::ClimbField(name) => {
                if let Some((dx, dy, dz)) = climb_field(world, subj, name, salt) {
                    out.push(Effect::Move { e: subj.id, dx, dy, dz });
                }
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
    fn check_do(&self, name: &str, target: Option<&Target>, me: &[String], d: &Do, subjects: &[String], errs: &mut Vec<String>) {
        match d {
            Do::Despawn(t) => {
                self.target_kinds(name, target, me, t, errs);
            }
            Do::Spawn(k) | Do::MoveToward(k) | Do::MoveAway(k) if !self.def.kinds.contains_key(k) => {
                errs.push(format!("'{name}': unknown kind '{k}'"));
            }
            Do::Climb(k, prop) => match self.def.kinds.get(k) {
                None => errs.push(format!("'{name}': Climb on unknown kind '{k}'")),
                Some(kd) if !kd.props.contains_key(prop) => errs.push(format!("'{name}': Climb: kind '{k}' has no prop '{prop}'")),
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
            Do::SetField(f, _) | Do::AddField(f, _) | Do::SetFieldAt(f, ..) | Do::ClimbField(f) if !self.def.fields.contains_key(f) => {
                errs.push(format!("'{name}': no field '{f}' (declare it in `fields`)"));
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
            let kinds: Vec<String> =
                self.kind_charts.iter().filter(|(_, c)| !c.with_origin(m, "").is_empty()).map(|(k, _)| k.clone()).collect();
            self.check_machine(m, "", d, &kinds, errs);
        }
        if let Some(t) = &self.def.terrain
            && !self.def.fields.contains_key(t)
        {
            errs.push(format!("terrain '{t}' is not a declared field"));
        }
        if self.def.terrain.is_none()
            && let Some((k, _)) = self.def.kinds.iter().find(|(_, k)| k.cling)
        {
            errs.push(format!("kind '{k}' clings, but the world has no `terrain` to cling to"));
        }
        if let Some(l) = &self.def.layout {
            for (ch, values) in &l.cells {
                for f in values.keys().filter(|f| !self.def.fields.contains_key(*f)) {
                    errs.push(format!("layout cells '{ch}': no field '{f}'"));
                }
            }
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

        // Perception: a kind's own expressions may not read the world directly (the dry run can miss
        // a read behind `&&`, so the sources are scanned too).
        if self.strict {
            let agents: Vec<&String> = self.def.kinds.keys().filter(|k| self.agent_view(k)).collect();
            let mut report = |what: String, srcs: Vec<&str>| {
                let mut names: Vec<&str> = srcs.iter().flat_map(|s| direct_reads(s)).collect();
                names.sort_unstable();
                names.dedup();
                if !names.is_empty() {
                    errs.push(self.hint(format!("{what} reads {} directly: Variable not found: {}", names.join(", "), names[0])));
                }
            };
            let defs = self.def.rules.iter().chain(&self.def.actions).chain(&self.machine_rules);
            for (r, def) in self.rules.iter().zip(defs) {
                if agents.iter().any(|k| Self::applies(r, k)) {
                    let mut srcs: Vec<&str> = def.when.iter().chain(&def.script).map(String::as_str).collect();
                    def.then.iter().for_each(|d| do_sources(d, &mut srcs));
                    report(format!("rule '{}'", r.name), srcs);
                }
            }
            for (m, d) in &self.def.fsms {
                let used = agents.iter().any(|k| self.kind_charts.get(*k).is_some_and(|c| !c.with_origin(m, "").is_empty()));
                if used {
                    let mut srcs = Vec::new();
                    let mut without_rules = d.clone();
                    strip_rules(&mut without_rules);
                    state_sources(&without_rules, &mut srcs);
                    report(format!("fsm '{m}'"), srcs);
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
                Some(Target::Nearest(k) | Target::NearestIn(k, _)) => errs.push(format!("{what} '{}': unknown target kind '{k}'", r.name)),
                Some(t) => errs.push(format!("{what} '{}': target must be Nearest(kind) or NearestIn(kind, state), got {t:?}", r.name)),
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
        let bound = self.bind_world(world);
        let synthetic = |kind: &str| {
            let (state, props) = self.template(kind);
            Entity { id: 0, kind: kind.into(), state, x: 0, y: 0, z: 0, props, genome: Vec::new() }
        };
        for kind in self.def.kinds.keys() {
            let e = synthetic(kind);
            let mut full = self.base_scope(world, &e, &counts);
            for (name, ast) in self.senses.get(kind).into_iter().flatten() {
                if let Err(m) = self.rhai.eval_ast_with_scope::<Dynamic>(&mut full, ast) {
                    errs.push(format!("kind '{kind}' sense '{name}': {m}"));
                }
            }
            if let Some(b) = self.brains.get(kind) {
                let mut full = self.tick_scope(world, &counts);
                let (sense, _) = self.sense_map(&mut full, world, &e);
                if let Err(m) = self.think(b, world, &e, &sense) {
                    errs.push(format!("kind '{kind}' brain {m}"));
                }
            }
            let base = self.entity_scope(world, &e, &counts);
            self.bind(&e, 0);

            if let Some(c) = self.kind_charts.get(kind) {
                let mut scope = base.clone();
                scope.push_constant("roll", 0_i64);
                for n in &c.nodes {
                    let at = if n.path.is_empty() { "root".to_string() } else { format!("'{}'", n.path) };
                    let mut fail = |m: String| errs.push(format!("fsm state {at} on '{kind}': {}", self.hint(m)));
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
                let mut fail = |m: String| errs.push(format!("'{}' on '{kind}': {}", r.name, self.hint(m)));
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
        for (name, ast) in &self.field_tops {
            if let Err(m) = self.eval_int(&mut scope, ast) {
                errs.push(format!("field '{name}' top: {m}"));
            }
        }
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
        drop(bound);
    }
}

impl Rules for Game {
    fn validate(&self, world: &World) -> Result<(), Vec<String>> {
        let mut errs = self.compile_errors.clone();
        if self.cfg.run.tick_rate <= 0 {
            errs.push(format!("engine.toml: tick_rate must be at least 1 (got {})", self.cfg.run.tick_rate));
        }
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
        let entities: Vec<&Entity> = world.entities().values().collect();
        // Each worker builds its own scope once: no shared, locked values.
        let init = || Scopes { full: self.tick_scope(world, &counts), agent: self.agent_scope() };
        let per = |scope: &mut Scopes, e: &&Entity| {
            let _bound = self.bind_world(world);
            self.eval_entity(world, e, scope)
        };
        // In a small world, distributing work costs more than the work: sequential path.
        let active = entities.iter().filter(|e| !self.plan(&e.kind).idle).count();
        let pool = self.pool.as_ref().filter(|_| active >= PARALLEL_FROM);
        let groups: Vec<Vec<Group>> = match pool {
            #[cfg(feature = "parallel")]
            Some(pool) => pool.install(|| entities.par_iter().with_min_len(CHUNK).map_init(init, per).collect()),
            _ => {
                let mut scope = init();
                entities.iter().map(|e| per(&mut scope, e)).collect()
            }
        };
        groups.into_iter().flatten().collect()
    }

    /// Fields: diffuse through the 6 face neighbours, then pin level 0 to each `top` expression.
    /// Integers, voxel order, native: deterministic and fast.
    fn physics(&self, world: &mut World) {
        // Crawlers left over nothing (their floor dug away) fall until they touch terrain.
        world.settle_clingers();
        let mut tops = Vec::new();
        if self.def.fields.values().any(|f| f.top.is_some()) {
            let mut scope = self.world_scope(world);
            for (name, ast) in &self.field_tops {
                if let Ok(v) = self.eval_int(&mut scope, ast) {
                    tops.push((name.clone(), v));
                }
            }
        }
        // Diffusion then decay, in one pass per field. Each voxel's new value reads only the old field, so levels are
        // computed side by side on the game's cores (large worlds) and the result is the same at any core count.
        for (name, f) in self.def.fields.iter().filter(|(_, f)| f.diffusion > 0 || f.decay > 0) {
            let Some(old) = world.field_values(name) else { continue };
            let (w, h, d) = (world.width as usize, world.height as usize, world.depth as usize);
            let (diffusion, keep) = (f.diffusion, 100 - f.decay);
            let plane = w * h;
            let level = |z: usize, out: &mut [i64]| {
                for y in 0..h {
                    for x in 0..w {
                        let i = z * plane + y * w + x;
                        let here = old[i];
                        let mut v = here;
                        if diffusion > 0 {
                            let (mut sum, mut k) = (0i64, 0i64);
                            let mut add = |j: usize| {
                                sum += old[j];
                                k += 1;
                            };
                            if x > 0 {
                                add(i - 1);
                            }
                            if x + 1 < w {
                                add(i + 1);
                            }
                            if y > 0 {
                                add(i - w);
                            }
                            if y + 1 < h {
                                add(i + w);
                            }
                            if z > 0 {
                                add(i - plane);
                            }
                            if z + 1 < d {
                                add(i + plane);
                            }
                            if k > 0 {
                                v = here + (sum - k * here) * diffusion / (100 * k);
                            }
                        }
                        out[y * w + x] = if f.decay > 0 { v * keep / 100 } else { v };
                    }
                }
            };
            let mut new = vec![0i64; old.len()];
            match self.pool.as_ref().filter(|_| old.len() >= 16_384) {
                #[cfg(feature = "parallel")]
                Some(pool) => pool.install(|| new.par_chunks_mut(plane).enumerate().for_each(|(z, out)| level(z, out))),
                _ => new.chunks_mut(plane).enumerate().for_each(|(z, out)| level(z, out)),
            }
            world.replace_field(name, new);
        }
        // Boundary last: level 0 holds its pinned value at the end of every tick.
        for (name, v) in tops {
            for y in 0..world.height {
                for x in 0..world.width {
                    world.set_field(&name, x, y, 0, v);
                }
            }
        }
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

/// One worker's scopes: the full world (senses, environments) and an agent's own view.
struct Scopes {
    full: Scope<'static>,
    agent: Scope<'static>,
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
        if self.game.fast.load(Ordering::Relaxed)
            && let Some(v) = self.game.expr_guards[*g].as_deref().and_then(|t| check_guard(t, self.e, &self.game.params))
        {
            return Ok(v);
        }
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

/// Answers chart conditions from native guards only; anything else is an error (the caller then takes the full path).
struct GuardOracle<'g, 'a> {
    game: &'g Game,
    e: &'a Entity,
}

impl Oracle<usize> for GuardOracle<'_, '_> {
    fn test(&mut self, g: &usize, _salt: u64) -> Result<bool, String> {
        self.game.expr_guards[*g].as_deref().and_then(|t| check_guard(t, self.e, &self.game.params)).ok_or_else(String::new)
    }
    fn score(&mut self, _g: &usize, _salt: u64) -> Result<i64, String> {
        Err(String::new())
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
fn climb(world: &World, e: &Entity, kind: &str, prop: &str, salt: u64) -> Option<(i64, i64, i64)> {
    let value = |x: i64, y: i64, z: i64| {
        world.at3(x, y, z).iter().map(|id| &world.entities()[id]).find(|x| x.kind == kind).and_then(|x| x.props.get(prop)).copied()
    };
    let here = value(e.x, e.y, e.z).unwrap_or(0);
    uphill(world, e, salt, here, value)
}

/// The open neighbouring voxel with the most of a field, if more than here.
fn climb_field(world: &World, e: &Entity, name: &str, salt: u64) -> Option<(i64, i64, i64)> {
    let here = world.field(name, e.x, e.y, e.z).unwrap_or(0);
    uphill(world, e, salt, here, |x, y, z| world.field(name, x, y, z))
}

/// One step towards the highest `value` among the neighbours a mover could enter; shuffled start breaks ties.
fn uphill(world: &World, e: &Entity, salt: u64, here: i64, value: impl Fn(i64, i64, i64) -> Option<i64>) -> Option<(i64, i64, i64)> {
    let cells = world.enterable_neighbors(&e.kind, e.x, e.y, e.z);
    if cells.is_empty() {
        return None;
    }
    let start = (world.rand(e.id, salt ^ 0x434C_494D) % cells.len() as u64) as usize;
    let mut best = (here, None);
    for i in 0..cells.len() {
        let (x, y, z) = cells[(start + i) % cells.len()];
        if let Some(v) = value(x, y, z)
            && v > best.0
        {
            best = (v, Some((x - e.x, y - e.y, z - e.z)));
        }
    }
    best.1
}

fn wander(world: &World, e: &Entity, salt: u64) -> Effect {
    let r = world.rand(e.id, salt ^ 0x5741_4E44);
    // A crawler picks among the voxels it can stand in (most random steps would leave the surface).
    if world.clings(&e.kind) {
        let cells = world.enterable_neighbors(&e.kind, e.x, e.y, e.z);
        let (dx, dy, dz) = cells.get((r % cells.len().max(1) as u64) as usize).map_or((0, 0, 0), |c| (c.0 - e.x, c.1 - e.y, c.2 - e.z));
        return Effect::Move { e: e.id, dx, dy, dz };
    }
    // In 3D the same draw also picks a level step; at depth 1 dx and dy are exactly as before.
    let dz = if world.depth > 1 { ((r / 9) % 3) as i64 - 1 } else { 0 };
    Effect::Move { e: e.id, dx: (r % 3) as i64 - 1, dy: ((r / 3) % 3) as i64 - 1, dz }
}

/// Script output: maps like `#{op: "add", prop: "hunger", value: -1}`.
fn effect_from_map(e: &Entity, item: Dynamic) -> Result<Effect, String> {
    let m = item.try_cast::<Map>().ok_or("script must return an array of maps")?;
    let s = |k: &str| m.get(k).and_then(|d| d.clone().into_string().ok()).ok_or_else(|| format!("effect map needs string '{k}'"));
    let i = |k: &str| m.get(k).and_then(|d| d.as_int().ok()).ok_or_else(|| format!("effect map needs int '{k}'"));
    Ok(match s("op")?.as_str() {
        "set" => Effect::Set { e: e.id, prop: s("prop")?, v: i("value")? },
        "add" => Effect::Add { e: e.id, prop: s("prop")?, d: i("value")? },
        "emit" => Effect::Emit { e: e.id, name: s("name")? },
        "move" => Effect::Move { e: e.id, dx: i("dx")?, dy: i("dy")?, dz: i("dz").unwrap_or(0) },
        "despawn" => Effect::Despawn { e: e.id },
        op => return Err(format!("unknown op '{op}' (set|add|emit|move|despawn)")),
    })
}
