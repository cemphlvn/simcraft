//! Game definition (game.ron, A: declarative + B: Rhai) and engine panel (engine.toml).

mod compile;
pub mod config;
pub mod game;
mod replay;

pub use compile::{FAR, Game};
pub use replay::{ReplayReport, replay};
