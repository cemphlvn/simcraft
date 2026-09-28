//! Game feel: how the simulation is shown between ticks. Never touches the simulation.
//!
//! The simulation ticks at a fixed rate; frames are drawn faster and interpolate between the last two ticks
//! ("fix your timestep"), and the camera follows with a critically damped spring instead of snapping.

use std::collections::BTreeMap;

use serde::Deserialize;
use sim_core::{EntityId, World};

/// A view's feel, as written in view.ron (`feel: (...)`).
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Feel {
    /// Draw positions between the last two ticks.
    pub interpolate: bool,
    pub camera: CameraFeel,
    /// Pixels a moving sprite bobs up (0 = none).
    pub walk_bob: i64,
}

impl Default for Feel {
    fn default() -> Self {
        Feel { interpolate: true, camera: CameraFeel::default(), walk_bob: 1 }
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CameraFeel {
    /// Spring stiffness (higher = faster follow). 0 = snap.
    pub stiffness: f32,
    /// Damping ratio: 1.0 = critically damped (fastest without overshoot), < 1 bouncy, > 1 sluggish.
    pub damping: f32,
}

impl Default for CameraFeel {
    fn default() -> Self {
        CameraFeel { stiffness: 40.0, damping: 1.0 }
    }
}

/// A 1D spring (the camera's x). Frame-rate independent: substeps keep it stable at any dt.
#[derive(Clone, Copy, Debug, Default)]
pub struct Spring {
    pub pos: f32,
    pub vel: f32,
    started: bool,
}

impl Spring {
    pub fn update(&mut self, target: f32, dt: f32, feel: CameraFeel) -> f32 {
        if !self.started || feel.stiffness <= 0.0 {
            self.started = true;
            self.pos = target;
            self.vel = 0.0;
            return self.pos;
        }
        let steps = ((dt / (1.0 / 240.0)).ceil() as usize).clamp(1, 32);
        let h = dt / steps as f32;
        let k = feel.stiffness;
        let c = 2.0 * feel.damping * k.sqrt();
        for _ in 0..steps {
            let a = k * (target - self.pos) - c * self.vel;
            self.vel += a * h;
            self.pos += self.vel * h;
        }
        self.pos
    }
}

/// Where every entity was at the previous tick, and how far we are into the current one (0.0 ..= 1.0).
#[derive(Clone, Debug, Default)]
pub struct Tween {
    pub prev: BTreeMap<EntityId, (i64, i64, i64)>,
    pub alpha: f32,
}

impl Tween {
    /// Call just before ticking: remember where everything is now.
    pub fn remember(&mut self, world: &World) {
        self.prev = world.entities().values().map(|e| (e.id, (e.x, e.y, e.z))).collect();
    }

    /// An entity's position between the last two ticks (new entities are where they are).
    pub fn at(&self, id: EntityId, now: (i64, i64, i64)) -> (f32, f32, f32) {
        let a = self.alpha.clamp(0.0, 1.0);
        match self.prev.get(&id) {
            // A long jump (a spawn at the same id, a teleport) is not interpolated.
            Some(&(x, y, z)) if (x - now.0).abs() <= 2 && (y - now.1).abs() <= 2 && (z - now.2).abs() <= 2 => {
                (x as f32 + (now.0 - x) as f32 * a, y as f32 + (now.1 - y) as f32 * a, z as f32 + (now.2 - z) as f32 * a)
            }
            _ => (now.0 as f32, now.1 as f32, now.2 as f32),
        }
    }

    pub fn moving(&self, id: EntityId, now: (i64, i64, i64)) -> bool {
        self.prev.get(&id).is_some_and(|p| *p != now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_critically_damped_spring_arrives_without_overshoot() {
        let mut s = Spring::default();
        s.update(0.0, 0.016, CameraFeel::default());
        let mut max: f32 = 0.0;
        for _ in 0..240 {
            max = max.max(s.update(100.0, 1.0 / 120.0, CameraFeel::default()));
        }
        assert!((s.pos - 100.0).abs() < 0.5, "arrives: {}", s.pos);
        assert!(max <= 100.5, "no overshoot: {max}");
    }

    #[test]
    fn the_spring_does_not_depend_on_the_frame_rate() {
        let run = |fps: f32| {
            let mut s = Spring::default();
            s.update(0.0, 0.0, CameraFeel::default());
            // The same quarter second at every frame rate.
            for _ in 0..(fps as usize / 4) {
                s.update(50.0, 1.0 / fps, CameraFeel::default());
            }
            s.pos
        };
        assert!((run(40.0) - run(160.0)).abs() < 1.0, "{} vs {}", run(40.0), run(160.0));
    }

    #[test]
    fn tween_glides_between_cells_but_not_across_jumps() {
        let mut t = Tween::default();
        t.prev.insert(1, (4, 0, 0));
        t.prev.insert(2, (0, 0, 0));
        t.alpha = 0.25;
        assert_eq!(t.at(1, (5, 0, 0)), (4.25, 0.0, 0.0));
        assert_eq!(t.at(2, (20, 0, 0)), (20.0, 0.0, 0.0), "a teleport is not smeared");
        assert_eq!(t.at(3, (7, 1, 0)), (7.0, 1.0, 0.0), "a newborn is where it is");
    }
}
