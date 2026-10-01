//! Named benchmark scenes and what to measure on them (`docs/research/building-a-physics-engine.md` §3.3): the
//! same scenes every step, so a solver change shows up as numbers, not impressions.

use std::time::Instant;

use super::math::V3;
use super::{BodyDef, BodyId, Material, Shape, World};

/// Every scene's name.
pub const NAMES: [&str; 5] = ["stack10", "pyramid21", "cans5", "wall40", "shot"];

/// The ground: a fixed slab whose top is y = 0.
pub fn ground(w: &mut World) -> BodyId {
    let mut d = BodyDef::new(Shape::Box { half: V3::new(20.0, 0.5, 20.0) }, V3::new(0.0, -0.5, 0.0));
    d.fixed = true;
    w.add(d)
}

fn cube(w: &mut World, pos: V3, half: V3, asleep: bool) -> BodyId {
    let mut d = BodyDef::new(Shape::Box { half }, pos);
    d.asleep = asleep;
    w.add(d)
}

/// `n` boxes of 0.4 m, one on another, half a millimetre apart.
pub fn stack(w: &mut World, n: usize, asleep: bool) {
    for i in 0..n {
        cube(w, V3::new(0.0, 0.2 + i as f32 * 0.4005, 0.0), V3::splat(0.2), asleep);
    }
}

/// A pyramid with `base` boxes in its bottom row.
pub fn pyramid(w: &mut World, base: usize, asleep: bool) {
    for row in 0..base {
        for k in 0..base - row {
            let x = (k as f32 - (base - row - 1) as f32 * 0.5) * 0.42;
            cube(w, V3::new(x, 0.2 + row as f32 * 0.4005, 0.0), V3::splat(0.2), asleep);
        }
    }
}

/// `n` cans (octagonal prisms), one on another.
pub fn cans(w: &mut World, n: usize, asleep: bool) {
    for i in 0..n {
        let mut d = BodyDef::new(Shape::Prism { radius: 0.12, half_height: 0.18, sides: 8 }, V3::new(0.0, 0.18 + i as f32 * 0.3605, 0.0));
        d.material = Material { friction: 0.5, restitution: 0.2, density: 300.0 };
        d.asleep = asleep;
        w.add(d);
    }
}

/// A wall of bricks, `cols` × `rows`, every other row offset by half a brick.
pub fn wall(w: &mut World, cols: usize, rows: usize, asleep: bool) {
    let half = V3::new(0.25, 0.15, 0.15);
    for r in 0..rows {
        let shift = if r % 2 == 1 { 0.25 } else { 0.0 };
        for c in 0..cols {
            let x = (c as f32 - (cols - 1) as f32 * 0.5) * 0.502 + shift;
            cube(w, V3::new(x, 0.15 + r as f32 * 0.3005, 0.0), half, asleep);
        }
    }
}

/// A ball of radius 0.15 m at `speed` m/s, aimed at (`x`, `y`, 0) from `dist` m away along +z.
pub fn ball(w: &mut World, x: f32, y: f32, dist: f32, speed: f32) -> BodyId {
    let mut d = BodyDef::new(Shape::Sphere { radius: 0.15 }, V3::new(x, y, dist));
    d.vel = V3::new(0.0, 0.0, -speed);
    d.material = Material { friction: 0.4, restitution: 0.3, density: 4000.0 };
    w.add(d)
}

/// A scene by name, ground included.
pub fn scene(name: &str) -> Option<World> {
    let mut w = World::new();
    ground(&mut w);
    match name {
        "stack10" => stack(&mut w, 10, false),
        "pyramid21" => pyramid(&mut w, 6, false),
        "cans5" => cans(&mut w, 5, false),
        "wall40" => wall(&mut w, 8, 5, false),
        "shot" => {
            wall(&mut w, 8, 5, true);
            ball(&mut w, 0.0, 0.6, 3.0, 25.0);
        }
        _ => return None,
    }
    Some(w)
}

/// What a scene did over a run.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SceneMetrics {
    pub steps: f32,
    /// The deepest overlap left after any step (m).
    pub max_penetration: f32,
    /// How far the body that started highest ended from where it started (m).
    pub final_drift_top: f32,
    /// When every body was first asleep (s); the run's length if never.
    pub time_to_sleep: f32,
    pub awake_peak: f32,
    /// Bodies whose centre ended below the ground's top.
    pub below_ground: f32,
    pub mean_step_us: f32,
    pub max_step_us: f32,
}

impl SceneMetrics {
    pub fn to_json(&self) -> String {
        format!(
            "{{\"steps\":{},\"max_penetration\":{:.6},\"final_drift_top\":{:.6},\"time_to_sleep\":{:.4},\"awake_peak\":{},\"below_ground\":{},\"mean_step_us\":{:.2},\"max_step_us\":{:.2}}}",
            self.steps,
            self.max_penetration,
            self.final_drift_top,
            self.time_to_sleep,
            self.awake_peak,
            self.below_ground,
            self.mean_step_us,
            self.max_step_us
        )
    }
}

/// Runs `world` for `steps` of `dt` and measures it.
pub fn measure(world: &mut World, steps: u32, dt: f32) -> SceneMetrics {
    let top = world.iter().filter(|(_, b)| !b.fixed).max_by(|a, b| a.1.pos.y.total_cmp(&b.1.pos.y)).map(|(id, b)| (id, b.pos));
    let mut m = SceneMetrics { steps: steps as f32, time_to_sleep: steps as f32 * dt, ..SceneMetrics::default() };
    let mut total = 0.0f32;
    let mut slept = false;
    for k in 0..steps {
        let t = Instant::now();
        world.step(dt);
        let us = t.elapsed().as_secs_f32() * 1e6;
        total += us;
        m.max_step_us = m.max_step_us.max(us);
        let s = world.stats();
        m.max_penetration = m.max_penetration.max(s.max_penetration);
        m.awake_peak = m.awake_peak.max(s.awake as f32);
        if !slept && s.awake == 0 && k > 0 {
            slept = true;
            m.time_to_sleep = (k + 1) as f32 * dt;
        }
        if slept && s.awake > 0 {
            // Woke again: it only counts once it stays asleep.
            slept = false;
            m.time_to_sleep = steps as f32 * dt;
        }
    }
    m.mean_step_us = total / steps.max(1) as f32;
    if let Some((id, p0)) = top {
        m.final_drift_top = world.get(id).map_or(f32::MAX, |b| (b.pos - p0).length());
    }
    m.below_ground = world.iter().filter(|(_, b)| !b.fixed && b.pos.y < 0.0).count() as f32;
    m
}
