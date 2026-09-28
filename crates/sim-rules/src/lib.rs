//! Game definition (game.ron, A: declarative + B: Rhai) and engine panel (engine.toml).

mod brain;
mod compile;
pub mod config;
mod env;
pub mod game;
mod replay;

pub use compile::{FAR, Game, RuleWork, Work};
pub use env::{NativeEnv, conformance};
pub use replay::{ReplayReport, replay};
