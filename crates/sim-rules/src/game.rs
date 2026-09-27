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
    /// Elle çizilmiş harita. Varsa dünya boyutu buradan gelir.
    #[serde(default)]
    pub layout: Option<Layout>,
    /// Agent'ların isteyebileceği eylemler. Kuralla aynı yapı; yalnızca istenince çalışır.
    #[serde(default)]
    pub actions: Vec<RuleDef>,
    /// Oyun sonu koşulları; ilk doğru olan sonucu belirler.
    #[serde(default)]
    pub end: Vec<EndDef>,
    /// Kontrol edilebilir her entity için puan ifadesi; koltuk (seat) başına toplanır.
    #[serde(default)]
    pub score: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    /// glyph → kind ya da (kind, {prop: değer}). '.' ve ' ' boş hücredir.
    pub legend: BTreeMap<char, Legend>,
    pub rows: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Legend {
    Kind(String),
    /// Bu glyph'teki entity'lere özel prop'lar (ör. `("village", {"owner": 1})`).
    With(String, BTreeMap<String, i64>),
}

impl Legend {
    pub fn kind(&self) -> &str {
        match self {
            Legend::Kind(k) | Legend::With(k, _) => k,
        }
    }
    pub fn props(&self) -> Option<&BTreeMap<String, i64>> {
        match self {
            Legend::Kind(_) => None,
            Legend::With(_, p) => Some(p),
        }
    }
}

impl Layout {
    pub fn size(&self) -> (i64, i64) {
        let w = self.rows.iter().map(|r| r.chars().count()).max().unwrap_or(0);
        (w as i64, self.rows.len() as i64)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndDef {
    /// Dünya düzeyinde Rhai ifadesi (`count`, `p`, `tick`) → bool.
    pub when: String,
    pub result: String,
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
    /// Hedef: `Nearest(kind)`. Yoksa kural ateşlenmez. İfadelerde `it` (`it.dist` dahil).
    #[serde(default)]
    pub target: Option<Target>,
    /// Yalnızca eylemler: agent'ın vermesi gereken argümanlar. İfadelerde `arg.<ad>`.
    #[serde(default)]
    pub args: Vec<String>,
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
    /// Bir adım; dx, dy ifadedir (ör. `Move("arg.dx", "arg.dy")`).
    Move(String, String),
    /// İçindeki eylemleri başka bir entity'ye uygular: `On(It, [Add("hp", "-3")])`.
    On(Target, Vec<Do>),
    /// prop >= ifade olmalı. İstek anında ve uygulama anında (canlı durumda) denetlenir;
    /// tutmazsa grubun tamamı düşer (aynı tick'te iki alıcı tek stok).
    Need(String, String),
}

#[derive(Debug, Clone, Deserialize)]
pub enum Target {
    /// Kuralın sahibi.
    Me,
    /// Kuralın `target`'ı.
    It,
    Nearest(String),
}
