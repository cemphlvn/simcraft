//! Headless, deterministic simulation core.
//! Knows nothing about any game; only World, Effect and the tick loop.

mod bus;
mod drive;
mod effect;
mod engine;
mod world;

pub use bus::{Bus, Filter, Msg, Sink};
pub use drive::{Grid, Surface, VEHICLE_INPUTS, VEHICLE_PROPS};
pub use effect::{Effect, Event, Group, apply};
pub use engine::{Engine, Loaded, Rules, Running, SNAPSHOT_FORMAT, Snapshot, TickReport, Validated};
pub use sim_physics;
pub use world::{Entity, EntityId, FAR, FINE, MOTION_PROPS, Motion, World, WorldSnapshot, splitmix64};
