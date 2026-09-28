//! Olay veriyolu: motorda olan her şey tek sırayla, abonelere yayınlanır.
//! Çekirdek I/O yapmaz; dosyaya, sokete, LOBI'ye yazan `Sink`'ler host'ta yaşar.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::effect::Event;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Msg {
    /// Koşu başladı. `source_hash`: game.ron + engine.toml içeriği (replay aynı oyunu doğrular).
    Start { game: String, seed: u64, source_hash: u64, hash: u64 },
    /// Bir agent eylemi istendi (kabul ya da ret). `tick`: uygulanacağı tick.
    Act {
        tick: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        seat: Option<String>,
        entity: u64,
        action: String,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        args: BTreeMap<String, i64>,
        ok: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// Oyunun ya da motorun ürettiği olay (kill, house, short, conflict…).
    Event(Event),
    /// Tick bitti; durumun parmak izi.
    Tick { tick: u64, hash: u64 },
    /// Oyun bitti.
    End { tick: u64, result: String },
}

impl Msg {
    /// Filtre için ad: olaylarda olay adı, diğerlerinde tür adı.
    pub fn name(&self) -> &str {
        match self {
            Msg::Start { .. } => "start",
            Msg::Act { .. } => "act",
            Msg::Event(e) => &e.name,
            Msg::Tick { .. } => "tick",
            Msg::End { .. } => "end",
        }
    }
}

/// Abone. Yayın sırası = motorun olay sırası.
pub trait Sink: Send {
    fn publish(&mut self, msg: &Msg);
}

/// Bellek içi kayıt (testler, gömülü kullanım): yayından sonra da okunabilir.
impl Sink for Arc<Mutex<Vec<Msg>>> {
    fn publish(&mut self, msg: &Msg) {
        if let Ok(mut v) = self.lock() {
            v.push(msg.clone());
        }
    }
}

/// Hangi mesajlar: hepsi ya da adı listede olanlar.
#[derive(Clone, Debug, Default)]
pub enum Filter {
    #[default]
    All,
    Only(BTreeSet<String>),
}

impl Filter {
    pub fn only<I: IntoIterator<Item = S>, S: Into<String>>(names: I) -> Self {
        Filter::Only(names.into_iter().map(Into::into).collect())
    }

    fn accepts(&self, msg: &Msg) -> bool {
        match self {
            Filter::All => true,
            Filter::Only(names) => names.contains(msg.name()),
        }
    }
}

#[derive(Default)]
pub struct Bus {
    subs: Vec<(Filter, Box<dyn Sink>)>,
}

impl Bus {
    pub fn subscribe(&mut self, filter: Filter, sink: Box<dyn Sink>) {
        self.subs.push((filter, sink));
    }

    pub fn is_empty(&self) -> bool {
        self.subs.is_empty()
    }

    pub fn publish(&mut self, msg: Msg) {
        for (filter, sink) in &mut self.subs {
            if filter.accepts(&msg) {
                sink.publish(&msg);
            }
        }
    }
}
