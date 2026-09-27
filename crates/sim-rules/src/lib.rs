//! Oyun tanımı (game.ron, A: bildirimsel + B: Rhai) ve motor paneli (engine.toml).

mod compile;
pub mod config;
pub mod game;

pub use compile::{FAR, Game};
