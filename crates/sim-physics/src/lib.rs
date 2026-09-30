//! simcraft's physics layer: deterministic, integer-only, and blind to games. The core hands it plain data (a
//! vehicle's numbers, its controls, its state), it steps them, and hands plain data back
//! (`docs/plans/physics-and-vehicles.md`).

pub mod fixed;

pub use fixed::{Angle, Fx, TURN, curve};
