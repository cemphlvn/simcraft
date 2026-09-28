//! Event bus: everything that happens in the engine is published to subscribers in one order.
//! The core does no I/O; `Sink`s that write to files, sockets or LOBI live in the host.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::effect::Event;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Msg {
    /// Run started. `source_hash`: game.ron + engine.toml contents (replay verifies the same game).
    Start { game: String, seed: u64, source_hash: u64, hash: u64 },
    /// An agent action was requested (accepted or rejected). `tick`: the tick it applies on.
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
    /// An event produced by the game or the engine (kill, house, short, conflict…).
    Event(Event),
    /// Tick finished; fingerprint of the state.
    Tick { tick: u64, hash: u64 },
    /// Game over.
    End { tick: u64, result: String },
    /// The engine was restored to a snapshot. Replay continues from the same snapshot.
    Restore { tick: u64, hash: u64, snapshot: Box<crate::engine::Snapshot> },
}

impl Msg {
    /// Name for filtering: the event name for events, the variant name otherwise.
    pub fn name(&self) -> &str {
        match self {
            Msg::Start { .. } => "start",
            Msg::Act { .. } => "act",
            Msg::Event(e) => &e.name,
            Msg::Tick { .. } => "tick",
            Msg::End { .. } => "end",
            Msg::Restore { .. } => "restore",
        }
    }
}

/// Subscriber. Publish order = the engine's event order.
pub trait Sink: Send {
    fn publish(&mut self, msg: &Msg);
}

/// In-memory log (tests, embedded use): readable after publishing too.
impl Sink for Arc<Mutex<Vec<Msg>>> {
    fn publish(&mut self, msg: &Msg) {
        if let Ok(mut v) = self.lock() {
            v.push(msg.clone());
        }
    }
}

/// Which messages: all, or those whose name is in the list.
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

    pub fn publish(&mut self, msg: &Msg) {
        for (filter, sink) in &mut self.subs {
            if filter.accepts(msg) {
                sink.publish(msg);
            }
        }
    }
}
