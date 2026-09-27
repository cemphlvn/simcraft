//! Headless, deterministik simülasyon çekirdeği.
//! Oyun hakkında hiçbir şey bilmez; yalnızca World, Effect ve tick döngüsü.

mod effect;
mod engine;
mod world;

pub use effect::{Effect, Event, Group, apply};
pub use engine::{Engine, Loaded, Rules, Running, TickReport, Validated};
pub use world::{Entity, EntityId, World, splitmix64};
