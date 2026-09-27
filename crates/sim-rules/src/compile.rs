//! game.ron + engine.toml → derlenmiş kurallar (sim_core::Rules).
//! İfadeler (when/Set/Add) Rhai expression, `script` tam Rhai'dir.
//! Script'ler dünyayı değiştiremez: yalnızca effect map'leri döndürür.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use rhai::{AST, Array, Dynamic, Engine as Rhai, Map, Scope};
use sim_core::{Effect, Entity, Group, Rules, World, splitmix64};

use crate::config::EngineConfig;
use crate::game::{Do, GameDef, Target};

/// Hiç yoksa `near.<kind>` bu değeri alır.
pub const FAR: i64 = 9_999;

pub struct Game {
    pub def: GameDef,
    pub cfg: EngineConfig,
    /// game.ron varsayılanları + engine.toml ezmeleri.
    pub params: BTreeMap<String, i64>,
    rhai: Rhai,
    rules: Vec<CompiledRule>,
    fsms: BTreeMap<String, Vec<CompiledTransition>>,
    compile_errors: Vec<String>,
}

struct CompiledRule {
    salt: u64,
    name: String,
    enabled: bool,
    for_kind: String,
    state: Option<String>,
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
}

struct CompiledTransition {
    from: String,
    to: String,
    when: AST,
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
        let game = Self::compile(def, cfg);
        let world = game.initial_world();
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

        let mut errors = Vec::new();
        let mut expr = |src: &str, ctx: &str| match rhai.compile_expression(src) {
            Ok(ast) => ast,
            Err(e) => {
                errors.push(format!("{ctx}: `{src}`: {e}"));
                AST::empty()
            }
        };

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
                        when: expr(&t.when, &format!("fsm '{name}' {}->{}", t.from, t.to)),
                    })
                    .collect();
                (name.clone(), ts)
            })
            .collect();

        let mut rules = Vec::new();
        let mut script_errors = Vec::new();
        for (i, r) in def.rules.iter().enumerate() {
            let ctx = format!("rule '{}'", r.name);
            let when = r.when.as_deref().map(|w| expr(w, &ctx));
            let then = r
                .then
                .iter()
                .map(|d| match d {
                    Do::Set(p, e) => CDo::Set(p.clone(), expr(e, &ctx)),
                    Do::Add(p, e) => CDo::Add(p.clone(), expr(e, &ctx)),
                    Do::Emit(n) => CDo::Emit(n.clone()),
                    Do::Despawn(t) => CDo::Despawn(t.clone()),
                    Do::Spawn(k) => CDo::Spawn(k.clone()),
                    Do::MoveToward(k) => CDo::MoveToward(k.clone()),
                    Do::MoveAway(k) => CDo::MoveAway(k.clone()),
                    Do::Wander => CDo::Wander,
                })
                .collect();
            rules.push(CompiledRule {
                salt: i as u64 + 1,
                name: r.name.clone(),
                enabled: cfg.switches.get(&r.name).copied().unwrap_or(true),
                for_kind: r.for_kind.clone(),
                state: r.state.clone(),
                when,
                then,
                script: None,
            });
            if let Some(src) = &r.script {
                let script = match rhai.compile(src) {
                    Ok(ast) => ast,
                    Err(e) => {
                        script_errors.push(format!("{ctx} script: {e}"));
                        AST::empty()
                    }
                };
                rules.last_mut().expect("just pushed").script = Some(script);
            }
        }

        errors.extend(script_errors);
        let mut params = def.params.clone();
        params.extend(cfg.params.iter().map(|(k, v)| (k.clone(), *v)));

        Game { def, cfg, params, rhai, rules, fsms, compile_errors: errors }
    }

    /// Başlangıç nüfusu engine.toml [spawn]'dan, konumlar seed'den.
    fn initial_world(&self) -> World {
        let mut world = World::new(self.cfg.run.seed, self.cfg.world.width, self.cfg.world.height);
        let mut c = 0u64;
        for (kind, n) in &self.cfg.spawn {
            if !self.def.kinds.contains_key(kind) {
                continue; // validate raporlar
            }
            for _ in 0..*n {
                let x = splitmix64(world.seed.wrapping_add(2 * c)) % world.width as u64;
                let y = splitmix64(world.seed.wrapping_add(2 * c + 1)) % world.height as u64;
                let (state, props) = self.template(kind);
                world.spawn(kind, &state, x as i64, y as i64, props);
                c += 1;
            }
        }
        world
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

    /// Etkin şalter durumu (kural adı → açık mı).
    pub fn switches(&self) -> BTreeMap<String, bool> {
        self.rules.iter().map(|r| (r.name.clone(), r.enabled)).collect()
    }

    fn counts(&self, world: &World) -> Map {
        self.def.kinds.keys().map(|k| (k.as_str().into(), Dynamic::from(world.count(k) as i64))).collect()
    }

    /// Kural ifadelerinin gördüğü dünya: me, p, tick, near, count (+ kurala özel roll).
    fn base_scope(&self, world: &World, e: &Entity, counts: &Map) -> Scope<'static> {
        let mut me = Map::new();
        me.insert("id".into(), Dynamic::from(e.id as i64));
        me.insert("kind".into(), Dynamic::from(e.kind.clone()));
        me.insert("state".into(), Dynamic::from(e.state.clone()));
        me.insert("x".into(), Dynamic::from(e.x));
        me.insert("y".into(), Dynamic::from(e.y));
        for (k, v) in &e.props {
            me.insert(k.as_str().into(), Dynamic::from(*v));
        }
        let near: Map = self
            .def
            .kinds
            .keys()
            .map(|k| (k.as_str().into(), Dynamic::from(world.nearest(e, k).map_or(FAR, |(_, d)| d))))
            .collect();
        let p: Map = self.params.iter().map(|(k, v)| (k.as_str().into(), Dynamic::from(*v))).collect();

        let mut scope = Scope::new();
        scope.push_constant("me", me);
        scope.push_constant("p", p);
        scope.push_constant("tick", world.tick as i64);
        scope.push_constant("near", near);
        scope.push_constant("count", counts.clone());
        scope
    }

    fn eval_bool(&self, scope: &mut Scope, ast: &AST) -> Result<bool, String> {
        self.rhai.eval_ast_with_scope::<bool>(scope, ast).map_err(|e| e.to_string())
    }

    fn eval_int(&self, scope: &mut Scope, ast: &AST) -> Result<i64, String> {
        self.rhai.eval_ast_with_scope::<i64>(scope, ast).map_err(|e| e.to_string())
    }

    fn eval_rule(&self, world: &World, e: &Entity, rule: &CompiledRule, base: &Scope<'static>) -> Result<Option<Group>, String> {
        let mut scope = base.clone();
        scope.push_constant("roll", world.roll(e.id, rule.salt));

        if let Some(w) = &rule.when
            && !self.eval_bool(&mut scope, w)?
        {
            return Ok(None);
        }

        let mut effects = Vec::new();
        for d in &rule.then {
            match d {
                CDo::Set(prop, ast) => {
                    effects.push(Effect::Set { e: e.id, prop: prop.clone(), v: self.eval_int(&mut scope, ast)? })
                }
                CDo::Add(prop, ast) => {
                    effects.push(Effect::Add { e: e.id, prop: prop.clone(), d: self.eval_int(&mut scope, ast)? })
                }
                CDo::Emit(name) => effects.push(Effect::Emit { e: e.id, name: name.clone() }),
                CDo::Despawn(Target::Me) => effects.push(Effect::Despawn { e: e.id }),
                CDo::Despawn(Target::Nearest(k)) => match world.nearest(e, k) {
                    Some((t, _)) => effects.push(Effect::Despawn { e: t.id }),
                    None => return Ok(None), // hedef yoksa ateşleme anlamsız
                },
                CDo::Spawn(k) => {
                    let (state, props) = self.template(k);
                    effects.push(Effect::Spawn { kind: k.clone(), state, x: e.x, y: e.y, props });
                }
                CDo::MoveToward(k) => {
                    if let Some((t, _)) = world.nearest(e, k) {
                        effects.push(Effect::Move { e: e.id, dx: t.x - e.x, dy: t.y - e.y });
                    }
                }
                CDo::MoveAway(k) => match world.nearest(e, k) {
                    Some((_, 0)) => effects.push(wander(world, e, rule.salt)),
                    Some((t, _)) => effects.push(Effect::Move { e: e.id, dx: e.x - t.x, dy: e.y - t.y }),
                    None => {}
                },
                CDo::Wander => effects.push(wander(world, e, rule.salt)),
            }
        }

        if let Some(script) = &rule.script {
            let out: Array = self.rhai.eval_ast_with_scope(&mut scope, script).map_err(|e| e.to_string())?;
            for item in out {
                effects.push(effect_from_map(e, item)?);
            }
        }

        Ok((!effects.is_empty()).then(|| Group { source: rule.name.clone(), actor: Some(e.id), effects }))
    }

    fn applies(rule: &CompiledRule, kind: &str) -> bool {
        rule.for_kind == "*" || rule.for_kind == kind
    }

    fn fsm_of(&self, kind: &str) -> Option<&Vec<CompiledTransition>> {
        self.def.kinds.get(kind)?.fsm.as_ref().and_then(|f| self.fsms.get(f))
    }

    fn check_refs(&self, errs: &mut Vec<String>) {
        let known = |k: &str| self.def.kinds.contains_key(k);
        for k in self.cfg.spawn.keys().filter(|k| !known(k)) {
            errs.push(format!("engine.toml [spawn]: unknown kind '{k}'"));
        }
        for s in self.cfg.switches.keys().filter(|s| !self.rules.iter().any(|r| &r.name == *s)) {
            errs.push(format!("engine.toml [switches]: no rule named '{s}'"));
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
        }
        let mut seen = BTreeSet::new();
        for (r, def) in self.rules.iter().zip(&self.def.rules) {
            if !seen.insert(&r.name) {
                errs.push(format!("rule '{}': duplicate name (switches need unique names)", r.name));
            }
            if r.for_kind != "*" && !known(&r.for_kind) {
                errs.push(format!("rule '{}': unknown kind '{}'", r.name, r.for_kind));
            }
            if let Some(s) = &r.state {
                let kinds: Vec<&String> = self.def.kinds.keys().filter(|k| Self::applies(r, k)).collect();
                if !kinds.iter().any(|k| self.states_of(k).contains(s)) {
                    errs.push(format!("rule '{}': state '{s}' does not exist for '{}'", r.name, r.for_kind));
                }
            }
            for d in &def.then {
                let k = match d {
                    Do::Despawn(Target::Nearest(k)) | Do::Spawn(k) | Do::MoveToward(k) | Do::MoveAway(k) => k,
                    _ => continue,
                };
                if !known(k) {
                    errs.push(format!("rule '{}': unknown kind '{k}'", r.name));
                }
            }
        }
    }

    /// Her ifadeyi her ilgili kind'ın şablonuyla bir kez koşturur:
    /// yazım hatalı prop, tanımsız parametre, yanlış tip → yükleme anında yakalanır.
    fn dry_run(&self, world: &World, errs: &mut Vec<String>) {
        let counts = self.counts(world);
        for kind in self.def.kinds.keys() {
            let (state, props) = self.template(kind);
            let e = Entity { id: 0, kind: kind.clone(), state, x: 0, y: 0, props };
            let base = self.base_scope(world, &e, &counts);

            for t in self.fsm_of(kind).into_iter().flatten() {
                if let Err(m) = self.eval_bool(&mut base.clone(), &t.when) {
                    errs.push(format!("fsm transition {}->{} on '{kind}': {m}", t.from, t.to));
                }
            }
            for r in self.rules.iter().filter(|r| Self::applies(r, kind)) {
                let mut scope = base.clone();
                scope.push_constant("roll", 0_i64);
                let mut fail = |m: String| errs.push(format!("rule '{}' on '{kind}': {m}", r.name));
                if let Some(w) = &r.when
                    && let Err(m) = self.eval_bool(&mut scope, w)
                {
                    fail(m);
                }
                for d in &r.then {
                    if let CDo::Set(_, ast) | CDo::Add(_, ast) = d
                        && let Err(m) = self.eval_int(&mut scope, ast)
                    {
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

        for e in world.entities.values() {
            let base = self.base_scope(world, e, &counts);

            // FSM: ilk eşleşen geçiş; kurallar bu tick eski durumu görür.
            for t in self.fsm_of(&e.kind).into_iter().flatten().filter(|t| t.from == e.state) {
                match self.eval_bool(&mut base.clone(), &t.when) {
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
                r.enabled && Self::applies(r, &e.kind) && r.state.as_ref().is_none_or(|s| *s == e.state)
            });
            for rule in active {
                match self.eval_rule(world, e, rule, &base) {
                    Ok(Some(g)) => out.push(g),
                    Ok(None) => {}
                    Err(m) => out.push(error(&rule.name, e, m)),
                }
            }
        }
        out
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
