//! game.ron + engine.toml → derlenmiş kurallar (sim_core::Rules).
//! İfadeler (when/Set/Add/Move) Rhai expression, `script` tam Rhai'dir.
//! Script'ler dünyayı değiştiremez: yalnızca effect map'leri döndürür.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::rc::Rc;

use rhai::{AST, Array, Dynamic, Engine as Rhai, Map, Scope};
use sim_core::{Effect, Entity, EntityId, Group, Rules, World, splitmix64};

use crate::config::EngineConfig;
use crate::game::{Do, GameDef, RuleDef, Target};

/// Hiç yoksa `near.<kind>` bu değeri alır.
pub const FAR: i64 = 9_999;

/// FSM geçişlerinin tuzu kural tuzlarıyla çakışmasın.
const FSM_SALT: u64 = 1 << 32;

/// Rhai'ye kayıtlı dünya sorgularının (`around`, `rand`) gördüğü bağlam.
/// Tick başında dünyanın anlık görüntüsü bağlanır; her değerlendirmeden önce `me` ve tuz.
#[derive(Default)]
struct QueryCtx {
    world: Option<Rc<World>>,
    me: EntityId,
    pos: (i64, i64),
    salt: u64,
    calls: u64,
}

impl QueryCtx {
    fn around(&self, kind: &str, state: Option<&str>, r: i64) -> i64 {
        self.world.as_ref().map_or(0, |w| w.around(self.pos, self.me, kind, state, r))
    }

    /// 0..n. Aynı ifadede birden çok çağrı farklı sayı verir; yine de tamamen deterministik.
    fn rand(&mut self, n: i64) -> i64 {
        let Some(w) = &self.world else { return 0 };
        if n <= 0 {
            return 0;
        }
        self.calls += 1;
        (w.rand(self.me, self.salt ^ self.calls.wrapping_mul(0xA5A5_5A5A_1234_5678)) % n as u64) as i64
    }
}

pub struct Game {
    pub def: GameDef,
    pub cfg: EngineConfig,
    /// game.ron varsayılanları + engine.toml ezmeleri.
    pub params: BTreeMap<String, i64>,
    rhai: Rhai,
    /// Önce kurallar, sonra eylemler (`is_action`). Tuzlar bu sırayla verilir.
    rules: Vec<CompiledRule>,
    fsms: BTreeMap<String, Vec<CompiledTransition>>,
    ends: Vec<CompiledEnd>,
    compile_errors: Vec<String>,
    ctx: Rc<RefCell<QueryCtx>>,
    /// `p` bir kez kurulur, entity'ler arasında paylaşılır (kopyalanmaz).
    p_shared: Dynamic,
    /// İfadelerin gerçekten okuduğu `near.<kind>`'lar. None = hepsi (belirlenemedi).
    near_kinds: Option<BTreeSet<String>>,
}

struct CompiledRule {
    salt: u64,
    name: String,
    is_action: bool,
    enabled: bool,
    for_kind: String,
    state: Option<String>,
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
    Wander,
    Goto(String),
    Move(AST, AST),
    On(Target, Vec<CDo>),
}

impl CDo {
    /// Bu eylemde (ve iç içe `On` bloklarında) değerlendirilecek sayı ifadeleri.
    fn int_exprs<'a>(&'a self, out: &mut Vec<&'a AST>) {
        match self {
            CDo::Set(_, a) | CDo::Add(_, a) => out.push(a),
            CDo::Move(dx, dy) => out.extend([dx, dy]),
            CDo::On(_, ds) => ds.iter().for_each(|d| d.int_exprs(out)),
            _ => {}
        }
    }
}

struct CompiledTransition {
    from: String,
    to: String,
    when: AST,
}

struct CompiledEnd {
    when: AST,
    result: String,
}

/// Bir kural değerlendirilirken kim kimdir: `Me` sahibi, `It` hedefi.
struct Who<'a> {
    me: &'a Entity,
    it: Option<&'a Entity>,
}

/// Kaynak metinleri AST'ye çevirir, hataları toplar (ilk hatada durmaz).
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
            Do::Wander => CDo::Wander,
            Do::Goto(s) => CDo::Goto(s.clone()),
            Do::Move(dx, dy) => CDo::Move(self.expr(dx, ctx), self.expr(dy, ctx)),
            Do::On(t, ds) => CDo::On(t.clone(), ds.iter().map(|d| self.doo(d, ctx)).collect()),
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
            for_kind: r.for_kind.clone(),
            state: r.state.clone(),
            target: r.target.clone(),
            args: r.args.clone(),
            when: r.when.as_deref().map(|w| self.expr(w, &ctx)),
            then: r.then.iter().map(|d| self.doo(d, &ctx)).collect(),
            script,
        }
    }
}

/// Bir `Do` ağacındaki tüm ifade kaynakları (near analizi için).
fn do_sources<'a>(d: &'a Do, out: &mut Vec<&'a str>) {
    match d {
        Do::Set(_, e) | Do::Add(_, e) => out.push(e),
        Do::Move(dx, dy) => out.extend([dx.as_str(), dy.as_str()]),
        Do::On(_, ds) => ds.iter().for_each(|d| do_sources(d, out)),
        _ => {}
    }
}

/// Kaynaklarda `near.<kind>` kullanımlarını bulur. `near` başka türlü
/// kullanılıyorsa (ör. `near["x"]`) güvenli tarafta kalıp None döner.
fn near_refs<'a>(sources: impl Iterator<Item = &'a str>) -> Option<BTreeSet<String>> {
    let mut kinds = BTreeSet::new();
    for src in sources {
        let mut rest = src;
        while let Some(i) = rest.find("near") {
            let before = rest[..i].chars().next_back();
            rest = &rest[i + 4..];
            if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                continue; // başka bir kelimenin parçası
            }
            let Some(tail) = rest.strip_prefix('.') else {
                if rest.starts_with(|c: char| c.is_alphanumeric() || c == '_') {
                    continue; // `nearest` gibi
                }
                return None;
            };
            let ident: String = tail.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
            kinds.insert(ident);
        }
    }
    Some(kinds)
}

/// Rhai'ye verilen entity görünümü (`me`, `it`).
fn entity_map(e: &Entity, dist: Option<i64>) -> Map {
    let mut m = Map::new();
    m.insert("id".into(), Dynamic::from(e.id as i64));
    m.insert("kind".into(), Dynamic::from(e.kind.clone()));
    m.insert("state".into(), Dynamic::from(e.state.clone()));
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
    /// `dir/game.ron` + `config` (varsayılan `dir/engine.toml`).
    pub fn load(dir: &Path, config: Option<&Path>) -> Result<(World, Game), String> {
        let read = |p: &Path| std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()));
        let game_src = read(&dir.join("game.ron"))?;
        let cfg_path = config.map(Path::to_path_buf).unwrap_or_else(|| dir.join("engine.toml"));
        let cfg_src = read(&cfg_path)?;
        Self::from_strs(&game_src, &cfg_src)
    }

    pub fn from_strs(game_ron: &str, engine_toml: &str) -> Result<(World, Game), String> {
        let def: GameDef = ron::from_str(game_ron).map_err(|e| format!("game.ron: {e}"))?;
        let cfg: EngineConfig = toml::from_str(engine_toml).map_err(|e| format!("engine.toml: {e}"))?;
        let mut game = Self::compile(def, cfg);
        let (world, errs) = game.initial_world();
        game.compile_errors.extend(errs);
        Ok((world, game))
    }

    fn compile(def: GameDef, cfg: EngineConfig) -> Game {
        let mut rhai = Rhai::new();
        rhai.set_max_operations(cfg.rhai.max_operations);
        rhai.set_max_call_levels(cfg.rhai.max_call_levels);
        rhai.set_fail_on_invalid_map_property(true);
        // stdout agent protokolüne ait; script'ler oraya yazamaz.
        rhai.on_print(|_| {});
        rhai.on_debug(|_, _, _| {});

        let ctx = Rc::new(RefCell::new(QueryCtx::default()));
        let c = ctx.clone();
        rhai.register_fn("around", move |kind: &str, r: i64| c.borrow().around(kind, None, r));
        let c = ctx.clone();
        rhai.register_fn("around", move |kind: &str, state: &str, r: i64| c.borrow().around(kind, Some(state), r));
        let c = ctx.clone();
        rhai.register_fn("rand", move |n: i64| c.borrow_mut().rand(n));

        let mut cc = Compiler { rhai: &rhai, errors: Vec::new() };
        let fsms = def
            .fsms
            .iter()
            .map(|(name, f)| {
                let ts = f
                    .transitions
                    .iter()
                    .map(|t| CompiledTransition {
                        from: t.from.clone(),
                        to: t.to.clone(),
                        when: cc.expr(&t.when, &format!("fsm '{name}' {}->{}", t.from, t.to)),
                    })
                    .collect();
                (name.clone(), ts)
            })
            .collect();
        // Tuzlar: kural i → i+1 (eski oyunların gidişatı değişmesin), eylemler sonra.
        let n = def.rules.len() as u64;
        let mut rules: Vec<CompiledRule> =
            def.rules.iter().enumerate().map(|(i, r)| cc.rule(r, i as u64 + 1, false, &cfg)).collect();
        rules.extend(def.actions.iter().enumerate().map(|(i, r)| cc.rule(r, n + i as u64 + 1, true, &cfg)));
        let ends = def
            .end
            .iter()
            .map(|e| CompiledEnd { when: cc.expr(&e.when, &format!("end '{}'", e.result)), result: e.result.clone() })
            .collect();
        let errors = cc.errors;

        let mut params = def.params.clone();
        params.extend(cfg.params.iter().map(|(k, v)| (k.clone(), *v)));
        let p: Map = params.iter().map(|(k, v)| (k.as_str().into(), Dynamic::from(*v))).collect();
        let p_shared = Dynamic::from_map(p).into_shared();

        let mut sources: Vec<&str> = Vec::new();
        for f in def.fsms.values() {
            sources.extend(f.transitions.iter().map(|t| t.when.as_str()));
        }
        for r in def.rules.iter().chain(&def.actions) {
            sources.extend(r.when.as_deref());
            sources.extend(r.script.as_deref());
            r.then.iter().for_each(|d| do_sources(d, &mut sources));
        }
        sources.extend(def.end.iter().map(|e| e.when.as_str()));
        let near_kinds = near_refs(sources.into_iter());

        Game { def, cfg, params, rhai, rules, fsms, ends, compile_errors: errors, ctx, p_shared, near_kinds }
    }

    /// Dünya boyutu `layout`'tan ya da panelin `[world]`'ünden. Önce layout yerleşir,
    /// sonra `[spawn]`. Solid kind'lar karıştırılmış boş hücrelere tek tek yerleşir.
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
                    let Some(kind) = l.legend.get(&ch) else {
                        errs.push(format!("layout row {y}, column {x}: '{ch}' is not in the legend"));
                        continue;
                    };
                    if !self.def.kinds.contains_key(kind) {
                        continue; // check_refs raporlar
                    }
                    let (state, props) = self.template(kind);
                    world.spawn(kind, &state, x as i64, y as i64, props);
                }
            }
        }

        let mut c = 0u64;
        for (kind, n) in &self.cfg.spawn {
            let Some(def) = self.def.kinds.get(kind) else {
                continue; // validate raporlar
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

    /// Yeni doğan bir kind'ın başlangıç durumu ve prop'ları.
    pub fn template(&self, kind: &str) -> (String, BTreeMap<String, i64>) {
        let Some(k) = self.def.kinds.get(kind) else { return ("-".into(), BTreeMap::new()) };
        let state = k
            .fsm
            .as_ref()
            .and_then(|f| self.def.fsms.get(f))
            .map_or_else(|| "-".into(), |f| f.initial.clone());
        (state, k.props.clone())
    }

    pub fn glyph(&self, kind: &str) -> char {
        self.def.kinds.get(kind).map_or('?', |k| k.glyph)
    }

    /// Duruma özel glyph varsa o, yoksa kind'ın glyph'i.
    pub fn glyph_of(&self, e: &Entity) -> char {
        self.def.kinds.get(&e.kind).map_or('?', |k| k.glyphs.get(&e.state).copied().unwrap_or(k.glyph))
    }

    pub fn states_of(&self, kind: &str) -> BTreeSet<String> {
        let mut s = BTreeSet::new();
        let fsm = self.def.kinds.get(kind).and_then(|k| k.fsm.as_ref()).and_then(|f| self.def.fsms.get(f));
        match fsm {
            Some(f) => {
                s.insert(f.initial.clone());
                for t in &f.transitions {
                    s.insert(t.from.clone());
                    s.insert(t.to.clone());
                }
            }
            None => {
                s.insert("-".into());
            }
        }
        s
    }

    /// Etkin şalter durumu (kural ya da eylem adı → açık mı).
    pub fn switches(&self) -> BTreeMap<String, bool> {
        self.rules.iter().map(|r| (r.name.clone(), r.enabled)).collect()
    }

    /// Agent bir eylem ister. Dünya `act` ile bir sonraki tick arasında değişmediği
    /// için eylem hemen değerlendirilir; sonuç (Group) tick'te kurallardan önce uygulanır.
    pub fn act(&self, world: &World, id: EntityId, name: &str, args: &BTreeMap<String, i64>) -> Result<Group, String> {
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
        if !Self::applies(rule, &e.kind) {
            return Err(format!("action '{name}' is for '{}', entity {id} is a '{}'", rule.for_kind, e.kind));
        }
        if rule.state.as_ref().is_some_and(|s| *s != e.state) {
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
                let needs: Vec<String> = target.into_iter().chain(def.when.clone()).collect();
                Err(format!("refused: needs {}", needs.join(" and ")))
            }
        }
    }

    /// Tick başına bir kez; entity'ler arasında paylaşılır.
    fn counts(&self, world: &World) -> Dynamic {
        let m: Map =
            self.def.kinds.keys().map(|k| (k.as_str().into(), Dynamic::from(world.count(k) as i64))).collect();
        Dynamic::from_map(m).into_shared()
    }

    /// Kural ifadelerinin gördüğü dünya: me, p, tick, near, count (+ kurala özel roll, it, arg).
    fn base_scope(&self, world: &World, e: &Entity, counts: &Dynamic) -> Scope<'static> {
        let near: Map = self
            .def
            .kinds
            .keys()
            .filter(|k| self.near_kinds.as_ref().is_none_or(|ks| ks.contains(*k)))
            .map(|k| (k.as_str().into(), Dynamic::from(world.nearest(e, k).map_or(FAR, |(_, d)| d))))
            .collect();

        let mut scope = Scope::new();
        scope.push_constant("me", entity_map(e, None));
        scope.push_constant("p", self.p_shared.clone());
        scope.push_constant("tick", world.tick as i64);
        scope.push_constant("near", near);
        scope.push_constant("count", counts.clone());
        scope
    }

    /// `end` ifadelerinin gördüğü dünya: p, tick, count.
    fn world_scope(&self, world: &World) -> Scope<'static> {
        let mut scope = Scope::new();
        scope.push_constant("p", self.p_shared.clone());
        scope.push_constant("tick", world.tick as i64);
        scope.push_constant("count", self.counts(world));
        scope
    }

    /// Sorgu bağlamını bir entity'ye ve tuza bağlar (her değerlendirmeden önce).
    fn bind(&self, e: &Entity, salt: u64) {
        let mut c = self.ctx.borrow_mut();
        (c.me, c.pos, c.salt, c.calls) = (e.id, (e.x, e.y), salt, 0);
    }

    fn bind_world(&self, world: Option<&World>) {
        self.ctx.borrow_mut().world = world.map(|w| Rc::new(w.clone()));
    }

    fn eval_bool(&self, scope: &mut Scope, ast: &AST) -> Result<bool, String> {
        self.rhai.eval_ast_with_scope::<bool>(scope, ast).map_err(|e| e.to_string())
    }

    fn eval_int(&self, scope: &mut Scope, ast: &AST) -> Result<i64, String> {
        self.rhai.eval_ast_with_scope::<i64>(scope, ast).map_err(|e| e.to_string())
    }

    /// Kopya yok: kurala özel değişkenler eklenir, sonra scope geri sarılır.
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
            Some(Target::Nearest(k)) => match world.nearest(e, k) {
                Some((t, d)) => {
                    scope.push_constant("it", entity_map(t, Some(d)));
                    Some(t)
                }
                None => return Ok(None),
            },
            Some(t) => return Err(format!("target must be Nearest(kind), got {t:?}")),
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
                return Ok(None); // hedef yok → ateşleme anlamsız
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
            Target::Nearest(k) => world.nearest(who.me, k).map(|(t, _)| t),
        }
    }

    /// `subj` eylemin uygulandığı entity: normalde sahibi, `On(...)` içinde hedef.
    /// false = hedef bulunamadı; kural bu tick ateşlenmez.
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
            CDo::Goto(st) => out.push(Effect::SetState { e: subj.id, state: st.clone() }),
            CDo::Move(dx, dy) => {
                let (dx, dy) = (self.eval_int(scope, dx)?, self.eval_int(scope, dy)?);
                out.push(Effect::Move { e: subj.id, dx, dy });
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
        rule.for_kind == "*" || rule.for_kind == kind
    }

    fn fsm_of(&self, kind: &str) -> Option<&Vec<CompiledTransition>> {
        self.def.kinds.get(kind)?.fsm.as_ref().and_then(|f| self.fsms.get(f))
    }

    /// Bir hedefin olası kind'ları (doğrulama için).
    fn target_kinds(&self, r: &RuleDef, t: &Target, errs: &mut Vec<String>) -> Vec<String> {
        match t {
            Target::Me => self.def.kinds.keys().filter(|k| r.for_kind == "*" || **k == r.for_kind).cloned().collect(),
            Target::It => match &r.target {
                Some(Target::Nearest(k)) => vec![k.clone()],
                _ => {
                    errs.push(format!("'{}': uses It but has no target", r.name));
                    vec![]
                }
            },
            Target::Nearest(k) => {
                if !self.def.kinds.contains_key(k) {
                    errs.push(format!("'{}': unknown kind '{k}'", r.name));
                }
                vec![k.clone()]
            }
        }
    }

    /// `subjects`: eylemin uygulandığı entity'nin olası kind'ları.
    fn check_do(&self, r: &RuleDef, d: &Do, subjects: &[String], errs: &mut Vec<String>) {
        match d {
            Do::Despawn(t) => {
                self.target_kinds(r, t, errs);
            }
            Do::Spawn(k) | Do::MoveToward(k) | Do::MoveAway(k) if !self.def.kinds.contains_key(k) => {
                errs.push(format!("'{}': unknown kind '{k}'", r.name));
            }
            Do::Goto(st) if !subjects.iter().any(|k| self.states_of(k).contains(st)) => {
                errs.push(format!("'{}': Goto to unknown state '{st}'", r.name));
            }
            Do::On(t, ds) => {
                let subs = self.target_kinds(r, t, errs);
                for d in ds {
                    self.check_do(r, d, &subs, errs);
                }
            }
            _ => {}
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
            let states = self.states_of(name);
            for st in k.glyphs.keys().filter(|st| !states.contains(*st)) {
                errs.push(format!("kind '{name}': glyph for unknown state '{st}'"));
            }
        }
        if let Some(l) = &self.def.layout {
            for (ch, k) in l.legend.iter().filter(|(_, k)| !known(k)) {
                errs.push(format!("layout legend '{ch}': unknown kind '{k}'"));
            }
        }

        let mut seen = BTreeSet::new();
        let defs = self.def.rules.iter().chain(&self.def.actions);
        for (r, def) in self.rules.iter().zip(defs) {
            let what = if r.is_action { "action" } else { "rule" };
            if !seen.insert(&r.name) {
                errs.push(format!("{what} '{}': duplicate name (switches need unique names)", r.name));
            }
            if r.for_kind != "*" && !known(&r.for_kind) {
                errs.push(format!("{what} '{}': unknown kind '{}'", r.name, r.for_kind));
            }
            let subjects: Vec<String> = self.def.kinds.keys().filter(|k| Self::applies(r, k)).cloned().collect();
            if let Some(s) = &r.state
                && !subjects.iter().any(|k| self.states_of(k).contains(s))
            {
                errs.push(format!("{what} '{}': state '{s}' does not exist for '{}'", r.name, r.for_kind));
            }
            match &r.target {
                None => {}
                Some(Target::Nearest(k)) if known(k) => {}
                Some(Target::Nearest(k)) => errs.push(format!("{what} '{}': unknown target kind '{k}'", r.name)),
                Some(t) => errs.push(format!("{what} '{}': target must be Nearest(kind), got {t:?}", r.name)),
            }
            if !r.is_action && !r.args.is_empty() {
                errs.push(format!("rule '{}': only actions take args", r.name));
            }
            for d in &def.then {
                self.check_do(def, d, &subjects, errs);
            }
        }
    }

    /// Her ifadeyi her ilgili kind'ın şablonuyla bir kez koşturur:
    /// yazım hatalı prop, tanımsız parametre, yanlış tip → yükleme anında yakalanır.
    fn dry_run(&self, world: &World, errs: &mut Vec<String>) {
        let counts = self.counts(world);
        self.bind_world(Some(world));
        let synthetic = |kind: &str| {
            let (state, props) = self.template(kind);
            Entity { id: 0, kind: kind.into(), state, x: 0, y: 0, props }
        };
        for kind in self.def.kinds.keys() {
            let e = synthetic(kind);
            let mut base = self.base_scope(world, &e, &counts);
            self.bind(&e, 0);

            for t in self.fsm_of(kind).into_iter().flatten() {
                if let Err(m) = self.eval_bool(&mut base, &t.when) {
                    errs.push(format!("fsm transition {}->{} on '{kind}': {m}", t.from, t.to));
                }
            }
            for r in self.rules.iter().filter(|r| Self::applies(r, kind)) {
                let mut scope = base.clone();
                scope.push_constant("roll", 0_i64);
                if let Some(Target::Nearest(k)) = &r.target {
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

    fn eval(&self, world: &World) -> Vec<Group> {
        let counts = self.counts(world);
        let mut out = Vec::new();
        let error = |source: &str, e: &Entity, m: String| Group {
            source: source.into(),
            actor: Some(e.id),
            effects: vec![Effect::Emit { e: e.id, name: format!("error: {m}") }],
        };

        self.bind_world(Some(world));
        for e in world.entities().values() {
            let mut base = self.base_scope(world, e, &counts);

            // FSM: ilk eşleşen geçiş; kurallar bu tick eski durumu görür.
            let transitions = self.fsm_of(&e.kind).into_iter().flatten().enumerate();
            for (i, t) in transitions.filter(|(_, t)| t.from == e.state) {
                self.bind(e, FSM_SALT + i as u64);
                match self.eval_bool(&mut base, &t.when) {
                    Ok(true) => {
                        out.push(Group {
                            source: "fsm".into(),
                            actor: Some(e.id),
                            effects: vec![Effect::SetState { e: e.id, state: t.to.clone() }],
                        });
                        break;
                    }
                    Ok(false) => {}
                    Err(m) => {
                        out.push(error("fsm", e, m));
                        break;
                    }
                }
            }

            let active = self.rules.iter().filter(|r| {
                !r.is_action && r.enabled && Self::applies(r, &e.kind) && r.state.as_ref().is_none_or(|s| *s == e.state)
            });
            for rule in active {
                match self.eval_rule(world, e, rule, &mut base) {
                    Ok(Some(g)) => out.push(g),
                    Ok(None) => {}
                    Err(m) => out.push(error(&rule.name, e, m)),
                }
            }
        }
        self.bind_world(None);
        out
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

fn wander(world: &World, e: &Entity, salt: u64) -> Effect {
    let r = world.rand(e.id, salt ^ 0x5741_4E44);
    Effect::Move { e: e.id, dx: (r % 3) as i64 - 1, dy: ((r / 3) % 3) as i64 - 1 }
}

/// Script çıktısı: `#{op: "add", prop: "hunger", value: -1}` gibi map'ler.
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
