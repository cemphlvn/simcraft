//! The slingshot: a finger's pull becomes an aim (a point on the tower and how hard), an aim becomes a launch, and a
//! launch is previewed with the solver's own integrator so the dotted arc lands exactly where the stone will.
//!
//! The pull is relative: touch anywhere, drag down to pull, sideways to aim (the shot goes opposite the pull, as a
//! slingshot does). The pull picks a **height on the tower**, evenly from just above the pedestal to just over the
//! top, and sideways picks a point across it; the launch angle is solved so the stone flies there (the low ballistic
//! arc). A harder pull is also a faster stone. Before step 009 the pull set speed and elevation and the hit fell
//! wherever they led: unevenly (16.8×), and a third of the tower out of reach (EVALS).

use sim_physics::rigid::{V3, World};

use super::tuning::Sling;
use crate::gesture::Px;

/// Under this power a release puts the stone back instead of shooting (a touch, not a pull).
pub const MIN_POWER: f32 = 0.12;

/// What a pull can aim at: heights `low` to `high` on the plane `z = plane_z` (the tower's middle).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reach {
    pub low: f32,
    pub high: f32,
    pub plane_z: f32,
    pub gravity: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Aim {
    /// 0 (no pull) to 1 (a full pull).
    pub power: f32,
    /// The point on the tower's plane it aims at.
    pub target: V3,
    /// Where the pouch is pulled to (the stone leaves from here) and the launch velocity.
    pub from: V3,
    pub vel: V3,
}

impl Aim {
    /// The aim of a finger that went down at `start` and is now at `now`, on a `w` × `h` screen.
    pub fn from_drag(start: Px, now: Px, w: f32, h: f32, t: &Sling, reach: &Reach) -> Aim {
        let power = ((now.y - start.y) / (h * t.pull_screen)).clamp(0.0, 1.0);
        let side = (now.x - start.x) / w;
        let up = ((power - MIN_POWER) / (1.0 - MIN_POWER)).clamp(0.0, 1.0);
        let target = V3::new(-side * t.aim_width, reach.low + (reach.high - reach.low) * up, reach.plane_z);
        Aim::towards(power, target, t, reach.gravity)
    }

    /// The launch that reaches `target` at the speed `power` gives.
    pub fn towards(power: f32, target: V3, t: &Sling, gravity: f32) -> Aim {
        let rest = V3::new(t.at.0, t.at.1, t.at.2);
        let flat = V3::new(target.x - rest.x, 0.0, target.z - rest.z);
        let along = flat.normalized();
        let from = rest - along * (t.pouch_travel * power) - V3::Y * (0.18 * power);
        let speed = t.min_speed + (t.max_speed - t.min_speed) * power;
        Aim { power, target, from, vel: launch(from, target, speed, gravity) }
    }

    pub fn speed(&self) -> f32 {
        self.vel.length()
    }
}

/// The velocity of `speed` from `from` that passes through `to` under `gravity` on the low arc (45° if it can't
/// reach: the furthest a throw goes).
pub fn launch(from: V3, to: V3, speed: f32, gravity: f32) -> V3 {
    let flat = V3::new(to.x - from.x, 0.0, to.z - from.z);
    let d = flat.length().max(1e-3);
    let dy = to.y - from.y;
    let (v2, g) = (speed * speed, gravity);
    let disc = v2 * v2 - g * (g * d * d + 2.0 * dy * v2);
    let angle = if disc >= 0.0 { ((v2 - disc.sqrt()) / (g * d)).atan() } else { std::f32::consts::FRAC_PI_4 };
    (flat * (1.0 / d) * angle.cos() + V3::Y * angle.sin()) * speed
}

/// The predicted flight: points every tick, and where it first hits something.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Preview {
    pub points: Vec<V3>,
    /// The stone's centre when it first touches something, and that surface's normal.
    pub hit: Option<(V3, V3)>,
}

/// Flies a stone of `radius` from `from` at `vel` through `world` under `gravity`, `substeps` a tick at `rate`
/// ticks a second (the solver's semi-implicit integrator), for at most `secs`.
#[allow(clippy::too_many_arguments)]
pub fn preview(world: &World, from: V3, vel: V3, radius: f32, gravity: f32, rate: f32, substeps: u32, secs: f32) -> Preview {
    let h = 1.0 / (rate * substeps as f32);
    let (mut p, mut v) = (from, vel);
    let mut out = Preview { points: vec![p], hit: None };
    let ticks = (secs * rate) as u32;
    for _ in 0..ticks {
        let start = p;
        for _ in 0..substeps {
            v.y -= gravity * h;
            p += v * h;
        }
        // The stone swept along the segment, as the solver sees it (an exact sphere, and only what a stone hits).
        let seg = p - start;
        let len = seg.length();
        if len > 1e-6
            && let Some((_, t, n)) = world.sphere_cast(start, seg, radius, len, super::SOLID, u32::MAX)
        {
            let at = start + seg * (t / len);
            out.points.push(at);
            out.hit = Some((at, n));
            return out;
        }
        out.points.push(p);
        if p.y < -1.0 {
            break;
        }
    }
    out
}

/// The released band: the pouch springs back through its rest point and wobbles there (an underdamped spring).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Band {
    /// The pouch's offset from rest, and its velocity.
    pub off: V3,
    pub vel: V3,
}

impl Band {
    pub fn step(&mut self, dt: f32, hz: f32, zeta: f32) {
        let w = std::f32::consts::TAU * hz;
        self.vel += (self.off * (-w * w) - self.vel * (2.0 * zeta * w)) * dt;
        self.off += self.vel * dt;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::smash::tuning::Tuning;

    fn reach() -> Reach {
        Reach { low: 1.15, high: 4.1, plane_z: 0.0, gravity: 9.81 }
    }

    #[test]
    fn pulling_down_climbs_the_tower_evenly_and_sideways_aims_the_other_way() {
        let t = Tuning::embedded().sling;
        let (w, h) = (1000.0, 2000.0);
        let start = Px::new(500.0, 1000.0);
        let full = Aim::from_drag(start, Px::new(500.0, 1000.0 + h * t.pull_screen * 2.0), w, h, &t, &reach());
        assert_eq!(full.power, 1.0, "a pull past full is full");
        assert!((full.target.y - 4.1).abs() < 1e-4, "a full pull aims over the top");
        let ys: Vec<f32> = (2..=10)
            .map(|k| Aim::from_drag(start, Px::new(500.0, 1000.0 + h * t.pull_screen * k as f32 / 10.0), w, h, &t, &reach()).target.y)
            .collect();
        let steps: Vec<f32> = ys.windows(2).map(|p| p[1] - p[0]).collect();
        assert!(steps.iter().all(|s| (s - steps[0]).abs() < 1e-4), "even steps: {steps:?}");
        let left = Aim::from_drag(start, Px::new(300.0, 1300.0), w, h, &t, &reach());
        assert!(left.target.x > 0.0 && left.vel.x > 0.0, "pull left, shoot right");
        let up = Aim::from_drag(start, Px::new(500.0, 900.0), w, h, &t, &reach());
        assert_eq!(up.power, 0.0, "pushing up is no pull");
    }

    #[test]
    fn the_launch_passes_through_its_target() {
        let (from, to) = (V3::new(0.0, 1.2, 9.0), V3::new(0.5, 2.5, 0.0));
        let v = launch(from, to, 20.0, 9.81);
        assert!((v.length() - 20.0).abs() < 1e-3);
        // Fly it: where it crosses z = 0, it is at the target.
        let (mut p, mut vel) = (from, v);
        while p.z > 0.0 {
            vel.y -= 9.81 * 1e-4;
            p += vel * 1e-4;
        }
        assert!((p - to).length() < 0.01, "{p:?}");
    }

    #[test]
    fn a_harder_pull_is_faster_and_comes_back_further() {
        let t = Tuning::embedded().sling;
        let target = V3::new(0.0, 2.0, 0.0);
        let weak = Aim::towards(0.2, target, &t, 9.81);
        let full = Aim::towards(1.0, target, &t, 9.81);
        assert!(full.speed() > weak.speed());
        assert!((full.speed() - t.max_speed).abs() < 1e-3);
        assert!(full.from.z > weak.from.z, "the pouch comes back towards the player");
    }

    #[test]
    fn a_released_band_overshoots_then_settles() {
        let mut b = Band { off: V3::new(0.0, 0.0, 0.9), vel: V3::ZERO };
        let mut min_z = 0.0f32;
        for _ in 0..240 {
            b.step(1.0 / 60.0, 7.0, 0.25);
            min_z = min_z.min(b.off.z);
        }
        assert!(min_z < -0.2, "it snaps past rest: {min_z}");
        assert!(b.off.length() < 0.01, "and comes to rest: {:?}", b.off);
    }
}
