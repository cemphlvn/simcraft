//! Agent interface: one JSON request per line on stdin, one JSON response per line on stdout.
//!
//!   simcraft-agent [GAME_DIR] [--config engine.toml]
//!   simcraft-agent [GAME_DIR] [--config engine.toml] --replay run.jsonl
//!
//! Commands: info · observe · act · step · hash · snapshot · restore · quit  (details: docs/architecture.md)

use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

mod sinks;

use serde_json::{Value, json};
use sim_agent::Session;
use sim_core::{Engine, Filter, Loaded, Msg, Running};
use sim_rules::Game;

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
        match session.handle_line(&line) {
            Some(resp) => emit(resp),
            None => break,
        }
    }
}

/// Attaches the `[bus]` outputs from the config; returns where they were attached.
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

/// Replays the recorded actions in the same game; compares each tick's hash.
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
