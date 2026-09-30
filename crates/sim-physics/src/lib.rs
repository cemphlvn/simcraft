//! simcraft's physics layer: deterministic, integer-only, and blind to games. The core hands it plain data (a
//! vehicle's numbers, its controls, its state), it steps them, and hands plain data back
//! (`docs/plans/physics-and-vehicles.md`).

pub mod contact;
pub mod driver;
pub mod fixed;
pub mod track;
pub mod vehicle;

pub use driver::{Ahead, Pilot, Plan, ahead_of, sit_on};
pub use fixed::{Angle, Fx, TURN, curve};
pub use track::{Place, Pose, SideDef, Track, TrackDef};
pub use vehicle::{Fleet, Params, VehicleDef};
