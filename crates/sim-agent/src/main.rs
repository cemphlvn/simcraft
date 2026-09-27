//! Agent arayüzü: stdin'den satır başına bir JSON istek, stdout'a satır başına bir JSON cevap.
//!
//!   simcraft-agent [GAME_DIR] [--config engine.toml]
//!
//! Komutlar: info · observe · act · step · hash · quit  (ayrıntı: docs/architecture.md)

use std::collections::BTreeMap;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use serde::Deserialize;
use serde_json::{Value, json};
use sim_core::{Effect, Engine, Group, Loaded, Running, World};
use sim_rules::Game;

#[derive(Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
enum Request {
    Info,
    Observe { entity: Option<u64> },
    Act { actions: Vec<ActionReq> },
    Step { n: Option<u64> },
    Hash,
    Quit,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ActionReq {
    entity: u64,
    #[serde(rename = "move")]
    mv: [i64; 2],
}

struct Session {
    engine: Engine<Running, Game>,
}

impl Session {
    fn handle(&mut self, req: Request) -> Result<Value, String> {
        match req {
            Request::Info => Ok(self.info()),
            Request::Observe { entity } => self.observe(entity),
            Request::Act { actions } => self.act(actions),
            Request::Step { n } => Ok(self.step(n.unwrap_or(1))),
            Request::Hash => {
                let w = self.engine.world();
                Ok(json!({ "tick": w.tick, "hash": format!("{:016x}", w.hash()) }))
            }
            Request::Quit => unreachable!("handled in main"),
        }
    }

    fn game(&self) -> &Game {
        self.engine.rules()
    }

    fn info(&self) -> Value {
        let g = self.game();
        let w = self.engine.world();
        let kinds: BTreeMap<_, _> = g
            .def
            .kinds
            .iter()
            .map(|(name, k)| {
                (name, json!({ "glyph": k.glyph.to_string(), "props": k.props, "states": g.states_of(name) }))
            })
            .collect();
        json!({
            "game": g.def.name,
            "world": { "width": w.width, "height": w.height },
            "max_ticks": g.cfg.run.max_ticks,
            "kinds": kinds,
            "controllable": g.cfg.agent.controllable,
            "switches": g.switches(),
            "params": g.params,
            "commands": {
                "info": "{\"cmd\":\"info\"}",
                "observe": "{\"cmd\":\"observe\"} or {\"cmd\":\"observe\",\"entity\":ID} (local view, '@' = you)",
                "act": "{\"cmd\":\"act\",\"actions\":[{\"entity\":ID,\"move\":[dx,dy]}]}  dx,dy in -1..1; applied next step, overrides rule movement",
                "step": "{\"cmd\":\"step\",\"n\":N}",
                "hash": "{\"cmd\":\"hash\"}",
                "quit": "{\"cmd\":\"quit\"}",
            },
        })
    }

    fn counts(&self) -> BTreeMap<&str, usize> {
        let w = self.engine.world();
        self.game().def.kinds.keys().map(|k| (k.as_str(), w.count(k))).collect()
    }

    /// kind → durum → adet (tek durumlu kind'lar dahil).
    fn states(&self) -> BTreeMap<&str, BTreeMap<&str, usize>> {
        let mut out: BTreeMap<&str, BTreeMap<&str, usize>> = BTreeMap::new();
        for e in self.engine.world().entities().values() {
            *out.entry(e.kind.as_str()).or_default().entry(e.state.as_str()).or_default() += 1;
        }
        out
    }

    fn observe(&self, entity: Option<u64>) -> Result<Value, String> {
        let w = self.engine.world();
        let Some(id) = entity else {
            let (x0, y0, x1, y1) = (0, 0, w.width - 1, w.height - 1);
            return Ok(json!({
                "tick": w.tick,
                "counts": self.counts(),
                "states": self.states(),
                "map": self.render(x0, y0, x1, y1, None),
                "entities": w.entities().values().collect::<Vec<_>>(),
            }));
        };
        let me = w.get(id).ok_or(format!("entity {id} does not exist"))?;
        let r = self.game().cfg.agent.observe_radius;
        let (x0, y0) = w.clamp(me.x - r, me.y - r);
        let (x1, y1) = w.clamp(me.x + r, me.y + r);
        let visible: Vec<_> = w
            .entities()
            .values()
            .filter(|e| e.id != id && (x0..=x1).contains(&e.x) && (y0..=y1).contains(&e.y))
            .collect();
        Ok(json!({
            "tick": w.tick,
            "me": me,
            "origin": [x0, y0],
            "map": self.render(x0, y0, x1, y1, Some(id)),
            "visible": visible,
        }))
    }

    /// ASCII harita: agent'lar için en ucuz, en okunaklı gözlem.
    fn render(&self, x0: i64, y0: i64, x1: i64, y1: i64, me: Option<u64>) -> Vec<String> {
        let w = self.engine.world();
        let cols = (x1 - x0 + 1) as usize;
        let mut rows = vec![vec!['.'; cols]; (y1 - y0 + 1) as usize];
        let mut paint = |x: i64, y: i64, c: char| {
            if (x0..=x1).contains(&x) && (y0..=y1).contains(&y) {
                rows[(y - y0) as usize][(x - x0) as usize] = c;
            }
        };
        for e in w.entities().values() {
            paint(e.x, e.y, self.game().glyph_of(e));
        }
        if let Some(e) = me.and_then(|id| w.get(id)) {
            paint(e.x, e.y, '@');
        }
        rows.into_iter().map(String::from_iter).collect()
    }

    fn act(&mut self, actions: Vec<ActionReq>) -> Result<Value, String> {
        let w: &World = self.engine.world();
        let controllable = &self.game().cfg.agent.controllable;
        let mut effects = Vec::new();
        for a in &actions {
            let e = w.get(a.entity).ok_or(format!("entity {} does not exist", a.entity))?;
            if !controllable.contains(&e.kind) {
                return Err(format!("kind '{}' is not controllable (engine.toml [agent])", e.kind));
            }
            let [dx, dy] = a.mv;
            if !(-1..=1).contains(&dx) || !(-1..=1).contains(&dy) {
                return Err("move must be within -1..1".into());
            }
            effects.push(Effect::Move { e: a.entity, dx, dy });
        }
        // Her aksiyon ayrı grup: biri ölürse diğerleri düşmesin.
        for (a, ef) in actions.iter().zip(effects) {
            self.engine.queue(Group { source: "agent".into(), actor: Some(a.entity), effects: vec![ef] });
        }
        Ok(json!({ "queued": actions.len(), "applies_at_tick": self.engine.world().tick }))
    }

    fn step(&mut self, n: u64) -> Value {
        let max = self.game().cfg.run.max_ticks;
        let mut events = Vec::new();
        let mut hash = self.engine.world().hash();
        for _ in 0..n {
            if self.engine.world().tick >= max {
                break;
            }
            let report = self.engine.tick();
            hash = report.hash;
            events.extend(report.events);
        }
        let tick = self.engine.world().tick;
        json!({
            "tick": tick,
            "done": tick >= max,
            "hash": format!("{hash:016x}"),
            "counts": self.counts(),
            "states": self.states(),
            "events": events,
        })
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut dir = PathBuf::from("games/wolf_sheep");
    let mut config = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--config" => config = args.next().map(PathBuf::from),
            _ => dir = PathBuf::from(a),
        }
    }

    let out = io::stdout();
    let mut out = out.lock();
    let mut emit = |v: Value| {
        writeln!(out, "{v}").ok();
        out.flush().ok();
    };

    let (world, game) = match Game::load(&dir, config.as_deref()) {
        Ok(x) => x,
        Err(e) => {
            emit(json!({ "ok": false, "stage": "load", "errors": [e] }));
            std::process::exit(2);
        }
    };
    let engine = match Engine::<Loaded, _>::new(world, game).validate() {
        Ok(e) => e.start(),
        Err(errors) => {
            emit(json!({ "ok": false, "stage": "validate", "errors": errors }));
            std::process::exit(2);
        }
    };

    let mut session = Session { engine };
    let hello = json!({ "ok": true, "ready": session.game().def.name, "hint": "send {\"cmd\":\"info\"}" });
    emit(hello);

    for line in io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let resp = match serde_json::from_str::<Request>(&line) {
            Ok(Request::Quit) => break,
            Ok(req) => match session.handle(req) {
                Ok(mut v) => {
                    v["ok"] = json!(true);
                    v
                }
                Err(e) => json!({ "ok": false, "error": e }),
            },
            Err(e) => json!({ "ok": false, "error": format!("bad request: {e}") }),
        };
        emit(resp);
    }
}
