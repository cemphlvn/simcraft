//! Effects that only show what happened: debris chips flying from a hit. They never touch the physics (the solver
//! does not see them), live a second, bounce off the ground and the pedestal, and shrink away.

use sim_physics::rigid::{Quat, V3};

/// A small deterministic random source (xorshift): the same run throws the same chips.
#[derive(Clone, Copy, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1)
    }

    pub fn bits(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// Uniform in 0..1.
    pub fn unit(&mut self) -> f32 {
        (self.bits() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Uniform in −1..1.
    pub fn signed(&mut self) -> f32 {
        self.unit() * 2.0 - 1.0
    }

    /// A random direction in the half space around `n`.
    pub fn hemisphere(&mut self, n: V3) -> V3 {
        loop {
            let v = V3::new(self.signed(), self.signed(), self.signed());
            let l = v.length_sq();
            if l > 0.01 && l <= 1.0 {
                let v = v * (1.0 / l.sqrt());
                return if v.dot(n) < 0.0 { -v } else { v };
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chip {
    pub pos: V3,
    pub prev: V3,
    pub vel: V3,
    pub rot: Quat,
    pub spin: V3,
    pub size: f32,
    pub color: [f32; 3],
    pub age: f32,
    pub life: f32,
    /// Sparks glow and do not bounce.
    pub spark: bool,
}

/// What a chip lands on: the ground (y = 0) or, inside `radius` of the y axis, the pedestal's top at `top`.
#[derive(Clone, Copy, Debug)]
pub struct Floor {
    pub radius: f32,
    pub top: f32,
}

impl Floor {
    fn at(self, p: V3) -> f32 {
        if p.x * p.x + p.z * p.z < self.radius * self.radius && p.y > self.top - 0.2 { self.top } else { 0.0 }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Chips {
    pub list: Vec<Chip>,
}

impl Chips {
    /// Throws `n` chips of `color` from `at`, away from the surface `normal`, at about `speed` m/s.
    #[allow(clippy::too_many_arguments)]
    pub fn burst(&mut self, rng: &mut Rng, at: V3, normal: V3, n: u32, speed: f32, size: f32, color: [f32; 3], life: f32) {
        for _ in 0..n {
            let d = (rng.hemisphere(normal) + V3::Y * 0.6).normalized();
            let s = speed * (0.4 + rng.unit() * 0.8);
            let tint = 0.8 + rng.unit() * 0.35;
            self.list.push(Chip {
                pos: at,
                prev: at,
                vel: d * s,
                rot: Quat::axis_angle(V3::new(rng.signed(), rng.signed(), rng.signed()), rng.unit() * 6.0),
                spin: V3::new(rng.signed(), rng.signed(), rng.signed()) * 14.0,
                size: size * (0.5 + rng.unit()),
                color: [color[0] * tint, color[1] * tint, color[2] * tint],
                age: 0.0,
                life: life * (0.6 + rng.unit() * 0.6),
                spark: false,
            });
        }
    }

    /// Bright sparks that fly fast and fade (a hard hit's flare).
    pub fn sparks(&mut self, rng: &mut Rng, at: V3, normal: V3, n: u32, speed: f32) {
        for _ in 0..n {
            let d = (rng.hemisphere(normal) + normal * 0.5).normalized();
            self.list.push(Chip {
                pos: at,
                prev: at,
                vel: d * speed * (0.6 + rng.unit() * 0.8),
                rot: Quat::IDENTITY,
                spin: V3::ZERO,
                size: 0.022 + rng.unit() * 0.02,
                color: [1.0, 0.85, 0.45],
                age: 0.0,
                life: 0.18 + rng.unit() * 0.2,
                spark: true,
            });
        }
    }

    pub fn step(&mut self, dt: f32, gravity: f32, floor: Floor) {
        for c in &mut self.list {
            c.prev = c.pos;
            c.age += dt;
            c.vel.y -= gravity * dt * if c.spark { 0.3 } else { 1.0 };
            c.vel = c.vel * (1.0 - dt * if c.spark { 3.0 } else { 0.4 });
            c.pos += c.vel * dt;
            c.rot = c.rot.integrate(c.spin, dt);
            let y = floor.at(c.pos) + c.size;
            if c.pos.y < y && !c.spark {
                c.pos.y = y;
                c.vel = V3::new(c.vel.x * 0.55, -c.vel.y * 0.35, c.vel.z * 0.55);
                c.spin = c.spin * 0.6;
            }
        }
        self.list.retain(|c| c.age < c.life);
    }

    /// A chip's size now: full, then shrinking over the last third of its life.
    pub fn scale(c: &Chip) -> f32 {
        let left = 1.0 - c.age / c.life;
        (left * 3.0).min(1.0) * c.size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chips_fly_land_and_are_gone_after_their_life() {
        let mut rng = Rng::new(1);
        let mut c = Chips::default();
        c.burst(&mut rng, V3::new(0.0, 1.5, 0.0), V3::Z, 10, 3.0, 0.05, [1.0, 0.5, 0.1], 1.0);
        c.sparks(&mut rng, V3::new(0.0, 1.5, 0.0), V3::Z, 5, 6.0);
        assert_eq!(c.list.len(), 15);
        let floor = Floor { radius: 1.5, top: 1.0 };
        for _ in 0..30 {
            c.step(1.0 / 60.0, 9.81, floor);
            assert!(c.list.iter().filter(|c| !c.spark).all(|c| c.pos.y >= c.size - 1e-4), "never under the ground");
        }
        for _ in 0..120 {
            c.step(1.0 / 60.0, 9.81, floor);
        }
        assert!(c.list.is_empty());
    }

    #[test]
    fn the_same_seed_throws_the_same_chips() {
        let throw = || {
            let mut rng = Rng::new(7);
            let mut c = Chips::default();
            c.burst(&mut rng, V3::ZERO, V3::Y, 4, 2.0, 0.05, [1.0; 3], 1.0);
            c.list.iter().map(|c| c.vel).collect::<Vec<_>>()
        };
        assert_eq!(throw(), throw());
    }
}
