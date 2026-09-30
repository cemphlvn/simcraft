//! Agent protocol: one JSON request per line, one JSON response. `simcraft-agent` (stdio)
//! and `sim-ffi` (C API) use the same `Session`; the protocol has a single definition.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{Value, json};
use sim_core::{Engine, Group, Loaded, Msg, Running, Snapshot};
use sim_rules::Game;

#[derive(Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    Info {
        #[serde(rename = "as", default)]
        seat: Option<String>,
    },
    Observe {
        entity: Option<u64>,
        #[serde(rename = "as", default)]
        seat: Option<String>,
        /// Only what is within this many cells of your first entity (map and entities): a large world observed by a
        /// player that only needs its surroundings.
        #[serde(default)]
        near: Option<i64>,
    },
    Act {
        #[serde(rename = "as", default)]
        seat: Option<String>,
        actions: Vec<ActionReq>,
    },
    Step {
        n: Option<u64>,
    },
    /// A field at every voxel (x fastest, then y, then z).
    Field {
        name: String,
    },
    Hash,
    Snapshot,
    Restore {
        snapshot: Box<Snapshot>,
        /// If given, fingerprint of the game the snapshot was taken from; must match this game's.
        #[serde(default)]
        source: Option<String>,
    },
    Quit,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionReq {
    entity: u64,
    #[serde(rename = "do")]
    action: String,
    #[serde(default)]
    args: BTreeMap<String, i64>,
}

pub struct Session {
    pub engine: Engine<Running, Game>,
}

impl Session {
    pub fn handle(&mut self, req: Request) -> Result<Value, String> {
        match req {
            Request::Info { seat } => Ok(self.info(seat.as_deref())),
            Request::Observe { entity, seat, near } => self.observe(entity, seat.as_deref(), near),
            Request::Act { seat, actions } => self.act(seat.as_deref(), &actions),
            Request::Step { n } => Ok(self.step(n.unwrap_or(1))),
            Request::Field { name } => {
                let w = self.engine.world();
                let values = w.field_values(&name).ok_or_else(|| format!("no field '{name}'"))?;
                Ok(json!({ "tick": w.tick, "name": name, "width": w.width, "height": w.height, "depth": w.depth, "values": values }))
            }
            Request::Hash => {
                let w = self.engine.world();
                Ok(json!({ "tick": w.tick, "hash": format!("{:016x}", w.hash()) }))
            }
            Request::Snapshot => {
                let w = self.engine.world();
                Ok(json!({
                    "tick": w.tick,
                    "hash": format!("{:016x}", w.hash()),
                    "game": self.game().def.name,
                    "source": format!("{:016x}", self.game().source_hash),
                    "snapshot": self.engine.snapshot(),
                }))
            }
            Request::Restore { snapshot, source } => {
                let ours = format!("{:016x}", self.game().source_hash);
                if let Some(s) = source.filter(|s| *s != ours) {
                    return Err(format!("snapshot is from another game.ron/engine.toml (source {s}, this one {ours})"));
                }
                self.engine.restore(*snapshot).map_err(|e| e.join("; "))?;
                let w = self.engine.world();
                Ok(json!({ "tick": w.tick, "hash": format!("{:016x}", w.hash()) }))
            }
            Request::Quit => unreachable!("handled in main"),
        }
    }

    pub fn game(&self) -> &Game {
        self.engine.rules()
    }

    fn info(&self, seat: Option<&str>) -> Value {
        let g = self.game();
        let w = self.engine.world();
        let kinds: BTreeMap<_, _> = g
            .def
            .kinds
            .iter()
            .map(|(name, k)| {
                (name, json!({ "glyph": k.glyph.to_string(), "props": k.props, "states": g.states_of(name), "hidden": k.hidden, "glyphs": k.glyphs }))
            })
            .collect();
        json!({
            "game": g.def.name,
            "world": { "width": w.width, "height": w.height },
            "max_ticks": g.cfg.run.max_ticks,
            "tick_rate": g.cfg.run.tick_rate,
            "kinds": kinds,
            "controllable": g.cfg.agent.controllable,
            "seats": g.cfg.agent.seats.keys().collect::<Vec<_>>(),
            "you": self.yours(seat),
            "score": g.def.score,
            "switches": g.switches(),
            "params": g.params,
            "actions": g.def.actions.iter().map(|a| json!({
                "name": a.name,
                "for": a.for_kind,
                "args": a.args,
                "target": a.target.as_ref().map(|t| format!("{t:?}")),
                "when": a.when,
            })).collect::<Vec<_>>(),
            "commands": {
                "info": "{\"cmd\":\"info\"}",
                "observe": "{\"cmd\":\"observe\"} (add \"near\":R for only R cells around you) or {\"cmd\":\"observe\",\"entity\":ID} (local view, '@' = you)",
                "act": "{\"cmd\":\"act\",\"as\":\"<seat>\",\"actions\":[{\"entity\":ID,\"do\":\"<action>\",\"args\":{...}}]}  see `actions`; `as` only when the game has seats; applied next step, before rules",
                "step": "{\"cmd\":\"step\",\"n\":N}  stops early when the game ends (`done`, `result`)",
                "field": "{\"cmd\":\"field\",\"name\":\"<field>\"}  the field at every voxel (x fastest, then y, then z)",
                "hash": "{\"cmd\":\"hash\"}",
                "quit": "{\"cmd\":\"quit\"}",
            },
        })
    }

    /// Entities the agent (or its seat, if any) can control.
    fn yours(&self, seat: Option<&str>) -> Vec<u64> {
        let g = self.game();
        let ctl = &g.cfg.agent.controllable;
        let entities = self.engine.world().entities().values();
        entities.filter(|e| ctl.contains(&e.kind) && g.owns(seat, e) == Ok(true)).map(|e| e.id).collect()
    }

    fn counts(&self) -> BTreeMap<&str, usize> {
        let w = self.engine.world();
        self.game().def.kinds.keys().map(|k| (k.as_str(), w.count(k))).collect()
    }

    /// kind → state → count (including single-state kinds).
    fn states(&self) -> BTreeMap<&str, BTreeMap<&str, usize>> {
        let mut out: BTreeMap<&str, BTreeMap<&str, usize>> = BTreeMap::new();
        for e in self.engine.world().entities().values() {
            *out.entry(e.kind.as_str()).or_default().entry(self.game().state_label(e)).or_default() += 1;
        }
        out
    }

    fn observe(&self, entity: Option<u64>, seat: Option<&str>, near: Option<i64>) -> Result<Value, String> {
        let w = self.engine.world();
        let Some(id) = entity else {
            let you = self.yours(seat);
            let centre = you.first().and_then(|id| w.get(*id));
            let (x0, y0, x1, y1) = match (near, centre) {
                (Some(r), Some(c)) => {
                    let (x0, y0) = w.clamp(c.x - r, c.y - r);
                    let (x1, y1) = w.clamp(c.x + r, c.y + r);
                    (x0, y0, x1, y1)
                }
                _ => (0, 0, w.width - 1, w.height - 1),
            };
            let inside = |e: &&sim_core::Entity| (x0..=x1).contains(&e.x) && (y0..=y1).contains(&e.y);
            return Ok(json!({
                "tick": w.tick,
                "you": you,
                "scores": self.game().scores(w),
                "counts": self.counts(),
                "states": self.states(),
                "origin": [x0, y0],
                "map": self.render(x0, y0, x1, y1, None),
                "entities": w.entities().values().filter(inside).collect::<Vec<_>>(),
            }));
        };
        let me = w.get(id).ok_or_else(|| format!("entity {id} does not exist"))?;
        let r = self.game().cfg.agent.observe_radius;
        let (x0, y0) = w.clamp(me.x - r, me.y - r);
        let (x1, y1) = w.clamp(me.x + r, me.y + r);
        let visible: Vec<_> =
            w.entities().values().filter(|e| e.id != id && (x0..=x1).contains(&e.x) && (y0..=y1).contains(&e.y)).collect();
        Ok(json!({
            "tick": w.tick,
            "me": me,
            "origin": [x0, y0],
            "map": self.render(x0, y0, x1, y1, Some(id)),
            "visible": visible,
        }))
    }

    /// ASCII map: the cheapest, most readable observation for agents.
    fn render(&self, x0: i64, y0: i64, x1: i64, y1: i64, me: Option<u64>) -> Vec<String> {
        let w = self.engine.world();
        let cols = (x1 - x0 + 1) as usize;
        let mut rows = vec![vec!['.'; cols]; (y1 - y0 + 1) as usize];
        let mut paint = |x: i64, y: i64, c: char| {
            if (x0..=x1).contains(&x) && (y0..=y1).contains(&y) {
                rows[(y - y0) as usize][(x - x0) as usize] = c;
            }
        };
        for e in w.entities().values().filter(|e| !self.game().is_hidden(&e.kind)) {
            paint(e.x, e.y, self.game().glyph_of(e));
        }
        if let Some(e) = me.and_then(|id| w.get(id)) {
            paint(e.x, e.y, '@');
        }
        rows.into_iter().map(String::from_iter).collect()
    }

    /// Each request is evaluated separately: if one is rejected, the others are still queued.
    fn act(&mut self, seat: Option<&str>, actions: &[ActionReq]) -> Result<Value, String> {
        let (world, game) = (self.engine.world(), self.engine.rules());
        let outcomes: Vec<Result<Group, String>> = actions.iter().map(|a| game.act(world, seat, a.entity, &a.action, &a.args)).collect();
        let mut results = Vec::new();
        for (a, out) in actions.iter().zip(outcomes) {
            let tick = self.engine.world().tick;
            let error = out.as_ref().err().cloned();
            self.engine.bus().publish(&Msg::Act {
                tick,
                seat: seat.map(String::from),
                entity: a.entity,
                action: a.action.clone(),
                args: a.args.clone(),
                ok: error.is_none(),
                error: error.clone(),
            });
            match out {
                Ok(group) => {
                    self.engine.queue(group);
                    results.push(json!({ "entity": a.entity, "do": a.action, "ok": true }));
                }
                Err(e) => results.push(json!({ "entity": a.entity, "do": a.action, "ok": false, "error": e })),
            }
        }
        Ok(json!({ "results": results, "applies_at_tick": self.engine.world().tick }))
    }

    fn step(&mut self, n: u64) -> Value {
        let max = self.game().cfg.run.max_ticks;
        let mut events = Vec::new();
        // One hash for the step, at its end (not one per tick nobody reads).
        self.engine.hash_every_tick(false);
        for _ in 0..n {
            if self.engine.world().tick >= max || self.engine.outcome().is_some() {
                break;
            }
            events.extend(self.engine.tick().events);
        }
        let hash = self.engine.world().hash();
        let tick = self.engine.world().tick;
        json!({
            "tick": tick,
            "done": tick >= max || self.engine.outcome().is_some(),
            "result": self.engine.outcome(),
            "scores": self.game().scores(self.engine.world()),
            "hash": format!("{hash:016x}"),
            "counts": self.counts(),
            "states": self.states(),
            "events": events,
        })
    }
}

impl Session {
    /// Setup from texts (the host reads files itself: Unity StreamingAssets, Unreal content).
    /// Errors come back in the same format as the stdio startup error.
    pub fn from_strs(game_ron: &str, engine_toml: &str) -> Result<Session, Value> {
        let (world, game) = Game::from_strs(game_ron, engine_toml).map_err(|e| json!({ "ok": false, "stage": "load", "errors": [e] }))?;
        let engine = Engine::<Loaded, _>::new(world, game)
            .validate()
            .map_err(|errors| json!({ "ok": false, "stage": "validate", "errors": errors }))?
            .start();
        Ok(Session { engine })
    }

    /// One line → one response. None for `quit`.
    pub fn handle_line(&mut self, line: &str) -> Option<Value> {
        Some(match serde_json::from_str::<Request>(line) {
            Ok(Request::Quit) => return None,
            Ok(req) => match self.handle(req) {
                Ok(mut v) => {
                    v["ok"] = json!(true);
                    v
                }
                Err(e) => json!({ "ok": false, "error": e }),
            },
            Err(e) => json!({ "ok": false, "error": format!("bad request: {e}") }),
        })
    }
}
