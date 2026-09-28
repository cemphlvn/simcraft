//! Headless, deterministik simülasyon çekirdeği.
//! Oyun hakkında hiçbir şey bilmez; yalnızca World, Effect ve tick döngüsü.

mod bus;
mod effect;
mod engine;
mod world;

pub use bus::{Bus, Filter, Msg, Sink};
pub use effect::{Effect, Event, Group, apply};
pub use engine::{Engine, Loaded, Rules, Running, SNAPSHOT_FORMAT, Snapshot, TickReport, Validated};
pub use world::{Entity, EntityId, World, WorldSnapshot, splitmix64};
