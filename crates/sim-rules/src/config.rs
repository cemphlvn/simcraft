//! engine.toml — buharlı motorun yönetici paneli.
//! Oyunun *ne* olduğunu game.ron söyler; motorun *nasıl* çalışacağını bu dosya.

use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineConfig {
    pub run: RunCfg,
    pub world: WorldCfg,
    /// Başlangıç nüfusu: kind → adet.
    #[serde(default)]
    pub spawn: BTreeMap<String, u32>,
    /// Şalterler: kural adı → açık/kapalı. Yazılmayan kural açıktır.
    #[serde(default)]
    pub switches: BTreeMap<String, bool>,
    /// Hiperparametreler: game.ron'daki varsayılanları ezer. Kurallarda `p.<ad>`.
    #[serde(default)]
    pub params: BTreeMap<String, i64>,
    #[serde(default)]
    pub rhai: RhaiCfg,
    #[serde(default)]
    pub agent: AgentCfg,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunCfg {
    pub seed: u64,
    pub max_ticks: u64,
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
    /// Emniyet ventili: tek bir ifade/script'in yapabileceği en fazla işlem.
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
    /// Agent'ların `act` ile yönetebileceği kind'lar.
    pub controllable: Vec<String>,
    /// `observe` ile entity etrafında görülen yarıçap.
    pub observe_radius: i64,
}

impl Default for AgentCfg {
    fn default() -> Self {
        Self { controllable: Vec::new(), observe_radius: 5 }
    }
}
