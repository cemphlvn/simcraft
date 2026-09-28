//! Agent arayüzü: stdin'den satır başına bir JSON istek, stdout'a satır başına bir JSON cevap.
//!
//!   simcraft-agent [GAME_DIR] [--config engine.toml]
//!   simcraft-agent [GAME_DIR] [--config engine.toml] --replay run.jsonl
//!
//! Komutlar: info · observe · act · step · hash · quit  (ayrıntı: docs/architecture.md)

use std::collections::BTreeMap;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

mod sinks;

use serde::Deserialize;
use serde_json::{Value, json};
use sim_core::{Engine, Filter, Group, Loaded, Msg, Running};
use sim_rules::Game;

#[derive(Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
enum Request {
    Info {
        #[serde(rename = "as", default)]
        seat: Option<String>,
    },
    Observe {
        entity: Option<u64>,
        #[serde(rename = "as", default)]
        seat: Option<String>,
    },
    Act {
        #[serde(rename = "as", default)]
        seat: Option<String>,
        actions: Vec<ActionReq>,
    },
    Step { n: Option<u64> },
    Hash,
    Quit,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ActionReq {
    entity: u64,
    #[serde(rename = "do")]
    action: String,
    #[serde(default)]
    args: BTreeMap<String, i64>,
}

struct Session {
    engine: Engine<Running, Game>,
}

impl Session {
    fn handle(&mut self, req: Request) -> Result<Value, String> {
        match req {
            Request::Info { seat } => Ok(self.info(seat.as_deref())),
            Request::Observe { entity, seat } => self.observe(entity, seat.as_deref()),
            Request::Act { seat, actions } => self.act(seat.as_deref(), actions),
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

    fn info(&self, seat: Option<&str>) -> Value {
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
                "observe": "{\"cmd\":\"observe\"} or {\"cmd\":\"observe\",\"entity\":ID} (local view, '@' = you)",
                "act": "{\"cmd\":\"act\",\"as\":\"<seat>\",\"actions\":[{\"entity\":ID,\"do\":\"<action>\",\"args\":{...}}]}  see `actions`; `as` only when the game has seats; applied next step, before rules",
                "step": "{\"cmd\":\"step\",\"n\":N}  stops early when the game ends (`done`, `result`)",
                "hash": "{\"cmd\":\"hash\"}",
                "quit": "{\"cmd\":\"quit\"}",
            },
        })
    }

    /// Agent'ın (koltuk varsa o koltuğun) yönetebileceği entity'ler.
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

    /// kind → durum → adet (tek durumlu kind'lar dahil).
    fn states(&self) -> BTreeMap<&str, BTreeMap<&str, usize>> {
        let mut out: BTreeMap<&str, BTreeMap<&str, usize>> = BTreeMap::new();
        for e in self.engine.world().entities().values() {
            *out.entry(e.kind.as_str()).or_default().entry(e.state.as_str()).or_default() += 1;
        }
        out
    }

    fn observe(&self, entity: Option<u64>, seat: Option<&str>) -> Result<Value, String> {
        let w = self.engine.world();
        let Some(id) = entity else {
            let (x0, y0, x1, y1) = (0, 0, w.width - 1, w.height - 1);
            return Ok(json!({
                "tick": w.tick,
                "you": self.yours(seat),
                "scores": self.game().scores(w),
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

    /// Her istek ayrı değerlendirilir: biri reddedilse de diğerleri kuyruğa girer.
    fn act(&mut self, seat: Option<&str>, actions: Vec<ActionReq>) -> Result<Value, String> {
        let (world, game) = (self.engine.world(), self.engine.rules());
        let outcomes: Vec<Result<Group, String>> =
            actions.iter().map(|a| game.act(world, seat, a.entity, &a.action, &a.args)).collect();
        let mut results = Vec::new();
        for (a, out) in actions.iter().zip(outcomes) {
            let tick = self.engine.world().tick;
            let error = out.as_ref().err().cloned();
            self.engine.bus().publish(Msg::Act {
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
        let mut hash = self.engine.world().hash();
        for _ in 0..n {
            if self.engine.world().tick >= max || self.engine.outcome().is_some() {
                break;
            }
            let report = self.engine.tick();
            hash = report.hash;
            events.extend(report.events);
        }
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

fn main() {
    let mut args = std::env::args().skip(1);
    let mut dir = PathBuf::from("games/wolf_sheep");
    let mut config = None;
    let mut replay_log = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--config" => config = args.next().map(PathBuf::from),
            "--replay" => replay_log = args.next().map(PathBuf::from),
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

    let mut engine = engine;
    if let Some(log) = replay_log {
        let (ok, v) = run_replay(&mut engine, &log);
        emit(v);
        std::process::exit(if ok { 0 } else { 1 });
    }

    let bus = match attach_bus(&mut engine) {
        Ok(b) => b,
        Err(e) => {
            emit(json!({ "ok": false, "stage": "bus", "errors": [e] }));
            std::process::exit(2);
        }
    };
    let (game, seed, source_hash, hash) = {
        let g = engine.rules();
        (g.def.name.clone(), g.cfg.run.seed, g.source_hash, engine.world().hash())
    };
    engine.bus().publish(Msg::Start { game, seed, source_hash, hash });

    let mut session = Session { engine };
    let hello = json!({ "ok": true, "ready": session.game().def.name, "bus": bus, "hint": "send {\"cmd\":\"info\"}" });
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

/// Paneldeki `[bus]` çıkışlarını bağlar; nereye bağlandığını döner.
fn attach_bus(engine: &mut Engine<Running, Game>) -> Result<Value, String> {
    let cfg = engine.rules().cfg.bus.clone();
    let mut info = json!({});
    if let Some(path) = &cfg.log {
        let sink = sinks::FileSink::create(Path::new(path)).map_err(|e| format!("[bus] log {path}: {e}"))?;
        engine.bus().subscribe(Filter::All, Box::new(sink));
        info["log"] = json!(path);
    }
    if let Some(addr) = &cfg.listen {
        let (sink, local) = sinks::TcpSink::listen(addr).map_err(|e| format!("[bus] listen {addr}: {e}"))?;
        engine.bus().subscribe(Filter::All, Box::new(sink));
        info["listen"] = json!(local);
    }
    Ok(info)
}

/// Kayıttaki eylemleri aynı oyunda yeniden oynatır; her tick'in hash'ini karşılaştırır.
fn run_replay(engine: &mut Engine<Running, Game>, path: &Path) -> (bool, Value) {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => return (false, json!({ "ok": false, "stage": "replay", "error": format!("{}: {e}", path.display()) })),
    };
    let mut log = Vec::new();
    for (i, line) in text.lines().enumerate().filter(|(_, l)| !l.trim().is_empty()) {
        match serde_json::from_str::<Msg>(line) {
            Ok(m) => log.push(m),
            Err(e) => return (false, json!({ "ok": false, "stage": "replay", "error": format!("line {}: {e}", i + 1) })),
        }
    }
    let same_source = log.iter().find_map(|m| match m {
        Msg::Start { source_hash, .. } => Some(*source_hash == engine.rules().source_hash),
        _ => None,
    });
    match sim_rules::replay(engine, &log) {
        Ok(r) => (true, json!({ "ok": true, "verified_ticks": r.ticks, "acts": r.acts, "same_source": same_source })),
        Err(e) => (false, json!({ "ok": false, "stage": "replay", "error": e, "same_source": same_source })),
    }
}
