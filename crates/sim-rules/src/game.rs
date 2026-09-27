//! game.ron — oyun tasarımcısının dünyası: kind'lar, FSM'ler, kurallar.

use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename = "Game", deny_unknown_fields)]
pub struct GameDef {
    pub name: String,
    pub kinds: BTreeMap<String, KindDef>,
    #[serde(default)]
    pub fsms: BTreeMap<String, FsmDef>,
    /// Tasarımcının açtığı ayar düğmeleri ve varsayılanları. Yönetici engine.toml'dan ezer.
    #[serde(default)]
    pub params: BTreeMap<String, i64>,
    pub rules: Vec<RuleDef>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KindDef {
    pub glyph: char,
    #[serde(default)]
    pub props: BTreeMap<String, i64>,
    #[serde(default)]
    pub fsm: Option<String>,
    /// Aynı hücrede en fazla bir solid bulunur; solid'ler birbirinin içinden geçemez.
    #[serde(default)]
    pub solid: bool,
    /// Duruma göre glyph (yoksa `glyph`).
    #[serde(default)]
    pub glyphs: BTreeMap<String, char>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FsmDef {
    pub initial: String,
    pub transitions: Vec<TransitionDef>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransitionDef {
    pub from: String,
    pub to: String,
    /// Rhai ifadesi → bool. İlk eşleşen geçiş kazanır.
    pub when: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleDef {
    pub name: String,
    /// Hangi kind'a uygulanır; "*" hepsi.
    #[serde(rename = "for")]
    pub for_kind: String,
    #[serde(default)]
    pub state: Option<String>,
    /// Rhai ifadesi → bool. Yoksa her tick ateşlenir.
    #[serde(default)]
    pub when: Option<String>,
    /// A katmanı: bildirimsel eylemler.
    #[serde(default)]
    pub then: Vec<Do>,
    /// B katmanı (kaçış kapısı): effect map dizisi döndüren Rhai script'i.
    #[serde(default)]
    pub script: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub enum Do {
    /// prop = ifade
    Set(String, String),
    /// prop += ifade
    Add(String, String),
    Emit(String),
    Despawn(Target),
    /// Kendi konumunda yeni bir kind doğurur.
    Spawn(String),
    MoveToward(String),
    MoveAway(String),
    Wander,
    /// FSM durumunu değiştirir (etkisiyle birlikte: `[Goto("Fire"), Emit("lightning")]`).
    Goto(String),
}

#[derive(Debug, Clone, Deserialize)]
pub enum Target {
    Me,
    Nearest(String),
}
