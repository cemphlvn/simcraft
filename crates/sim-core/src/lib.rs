//! Headless, deterministic simulation core.
//! Knows nothing about any game; only World, Effect and the tick loop.

mod bus;
mod effect;
mod engine;
mod world;

pub use bus::{Bus, Filter, Msg, Sink};
pub use effect::{Effect, Event, Group, apply};
pub use engine::{Engine, Loaded, Rules, Running, SNAPSHOT_FORMAT, Snapshot, TickReport, Validated};
pub use world::{Entity, EntityId, World, WorldSnapshot, splitmix64};
