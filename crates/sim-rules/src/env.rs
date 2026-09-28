//! Native environments: an environment's own machine and rules replaced by code (Rust now, C++ through
//! the C API later), held to the `.ron` reference by `conformance`.

use std::collections::BTreeMap;
use std::sync::Arc;

use sim_core::{Engine, Loaded, Running};

use crate::Game;

/// A pure step: same inputs, same outputs; integers only. `state` is the full state string
/// (as the `.ron` machine keeps it). Returns the environment's props and state after this tick.
pub trait NativeEnv: Send + Sync {
    fn step(
        &self,
        tick: u64,
        params: &BTreeMap<String, i64>,
        props: &BTreeMap<String, i64>,
        state: &str,
    ) -> (BTreeMap<String, i64>, String);
}

/// Runs the game twice, once with the `.ron` environment and once with `native`, and compares the
/// environment and the world hash every tick. Ok(ticks) if identical; the first difference otherwise.
pub fn conformance(
    game_ron: &str,
    engine_toml: &str,
    envs: &[(String, String)],
    name: &str,
    native: Arc<dyn NativeEnv>,
    ticks: u64,
) -> Result<u64, String> {
    let boot = |native: Option<Arc<dyn NativeEnv>>| -> Result<Engine<Running, Game>, String> {
        let (world, mut game) = Game::from_parts(game_ron, engine_toml, envs)?;
        if let Some(n) = native {
            game.set_native_env(name, n)?;
        }
        Ok(Engine::<Loaded, _>::new(world, game).validate().map_err(|e| e.join("; "))?.start())
    };
    let (mut reference, mut candidate) = (boot(None)?, boot(Some(native))?);
    let env = |e: &Engine<Running, Game>| e.world().of_kind(name).next().map(|x| (x.state.clone(), x.props.clone()));
    for _ in 0..ticks {
        let (a, b) = (reference.tick(), candidate.tick());
        if env(&reference) != env(&candidate) {
            return Err(format!("tick {}: .ron {:?} vs native {:?}", a.tick, env(&reference), env(&candidate)));
        }
        if a.hash != b.hash {
            return Err(format!("tick {}: environment equal but world hash differs", a.tick));
        }
        if a.outcome.is_some() {
            return Ok(a.tick);
        }
    }
    Ok(ticks)
}
