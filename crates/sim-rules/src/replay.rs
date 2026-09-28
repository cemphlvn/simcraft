//! Replay: replays a bus recording on the same game and verifies every tick's hash.
//! The recording carries only accepted acts and tick hashes; determinism produces the rest.

use sim_core::{Engine, Msg, Running};

use crate::Game;

#[derive(Debug, PartialEq)]
pub struct ReplayReport {
    pub ticks: u64,
    pub acts: u64,
}

pub fn replay(engine: &mut Engine<Running, Game>, log: &[Msg]) -> Result<ReplayReport, String> {
    let mut report = ReplayReport { ticks: 0, acts: 0 };
    for msg in log {
        match msg {
            Msg::Start { hash, .. } if *hash != engine.world().hash() => {
                return Err(format!("start state differs: log {hash:016x}, now {:016x}", engine.world().hash()));
            }
            Msg::Act { tick, seat, entity, action, args, ok: true, .. } => {
                let now = engine.world().tick;
                if *tick != now {
                    return Err(format!("act recorded for tick {tick} but replay is at tick {now}"));
                }
                let group = engine
                    .rules()
                    .act(engine.world(), seat.as_deref(), *entity, action, args)
                    .map_err(|e| format!("tick {tick}: recorded '{action}' now fails: {e}"))?;
                engine.queue(group);
                report.acts += 1;
            }
            Msg::Restore { tick, hash, snapshot } => {
                engine.restore((**snapshot).clone()).map_err(|e| format!("tick {tick}: restore failed: {}", e.join("; ")))?;
                if engine.world().hash() != *hash {
                    return Err(format!("restore at tick {tick}: log {hash:016x}, now {:016x}", engine.world().hash()));
                }
            }
            Msg::Tick { tick, hash } => {
                let r = engine.tick();
                if (r.tick, r.hash) != (*tick, *hash) {
                    return Err(format!("diverged at tick {tick}: log {hash:016x}, replay tick {} {:016x}", r.tick, r.hash));
                }
                report.ticks += 1;
            }
            _ => {}
        }
    }
    Ok(report)
}
