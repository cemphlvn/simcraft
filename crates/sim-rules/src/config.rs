//! engine.toml — the steam engine's operator panel.
//! game.ron says *what* the game is; this file says *how* the engine runs.

use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineConfig {
    pub run: RunCfg,
    /// Not needed if game.ron has a `layout` (if given, it must match).
    #[serde(default)]
    pub world: Option<WorldCfg>,
    /// Initial population: kind → count.
    #[serde(default)]
    pub spawn: BTreeMap<String, u32>,
    /// Switches: rule name → on/off. Unlisted rules are on.
    #[serde(default)]
    pub switches: BTreeMap<String, bool>,
    /// Hyperparameters: override game.ron defaults. `p.<name>` in rules.
    #[serde(default)]
    pub params: BTreeMap<String, i64>,
    #[serde(default)]
    pub rhai: RhaiCfg,
    #[serde(default)]
    pub agent: AgentCfg,
    #[serde(default)]
    pub bus: BusCfg,
}

/// Event bus outputs. Both optional.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct BusCfg {
    /// Writes every message as JSONL to this file (relative to the working dir). Input for replay.
    pub log: Option<String>,
    /// TCP address for live viewers (e.g. "127.0.0.1:7878"); every connected client gets every message.
    pub listen: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunCfg {
    pub seed: u64,
    pub max_ticks: u64,
    /// Core count for rule evaluation (number of boilers). 0 = all, 1 = single core.
    /// Does not change the result: the same seed gives the same hash at any core count.
    #[serde(default)]
    pub threads: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldCfg {
    pub width: i64,
    pub height: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct RhaiCfg {
    /// Safety valve: max operations a single expression/script may perform.
    pub max_operations: u64,
    pub max_call_levels: usize,
}

impl Default for RhaiCfg {
    fn default() -> Self {
        Self { max_operations: 10_000, max_call_levels: 16 }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct AgentCfg {
    /// Kinds agents may control via `act`.
    pub controllable: Vec<String>,
    /// Radius seen around an entity via `observe`.
    pub observe_radius: i64,
    /// Multiplayer: seat name → owner number. If set, an agent speaks with `as` and
    /// controls only entities whose `owner` prop is its own number.
    pub seats: BTreeMap<String, i64>,
}

impl Default for AgentCfg {
    fn default() -> Self {
        Self { controllable: Vec::new(), observe_radius: 5, seats: BTreeMap::new() }
    }
}
