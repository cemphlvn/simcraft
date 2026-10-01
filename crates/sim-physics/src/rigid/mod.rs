//! Rigid bodies in 3D: boxes and spheres that stack, rest, topple and get hit (`docs/architecture.md`, Rigid
//! bodies). Tier 2 of the physics tiers (`docs/research/mobile-types.md`), built for stack-and-topple games.
//!
//! **Floats, for now** (decided 2026-10-02): the solver is `f32` while a game's feel is being tuned; the same binary
//! gives the same result. It moves to [`crate::fixed`] once the feel is locked. Nothing here touches `sim-core`.
//!
//! A step: broadphase (sweep and prune over boxes grown by how far each body can move), narrowphase (contact
//! manifolds of up to four points, speculative so a fast projectile cannot pass through), then a soft-step solver
//! (Catto, Solver2D / Box2D v3) in substeps: integrate velocities, warm start, solve with soft contacts, integrate
//! positions, relax, and a restitution pass at the end. Islands of touching bodies fall asleep together; a body
//! created asleep (a level's tower) stands perfectly still until something awake touches it.

mod collide;
mod hull;
pub mod math;
pub mod scenes;
#[cfg(test)]
mod tests;
mod world;

pub use math::{M3, Quat, V3};
pub use world::{Tuning, World};

/// What a body is made of.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Material {
    /// Coulomb friction (the pair uses the geometric mean).
    pub friction: f32,
    /// Bounce, 0 (dead) to 1 (elastic); the pair uses the larger.
    pub restitution: f32,
    /// kg per m³.
    pub density: f32,
}

impl Default for Material {
    fn default() -> Material {
        Material { friction: 0.6, restitution: 0.1, density: 500.0 }
    }
}

/// A body's shape, centred on its position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    /// A box of half extents `half` along its local axes.
    Box {
        half: V3,
    },
    Sphere {
        radius: f32,
    },
    /// An upright prism with `sides` faces round its y axis (a can, a jar, a column): a convex stand-in for a
    /// cylinder, which no casual game collides exactly (`docs/research/smash-physics.md` §3).
    Prism {
        radius: f32,
        half_height: f32,
        sides: u8,
    },
}

impl Shape {
    pub fn volume(&self) -> f32 {
        match *self {
            Shape::Box { half } => 8.0 * half.x * half.y * half.z,
            Shape::Sphere { radius } => 4.0 / 3.0 * std::f32::consts::PI * radius.powi(3),
            Shape::Prism { radius, half_height, sides } => {
                let n = f32::from(sides);
                0.5 * n * radius * radius * (std::f32::consts::TAU / n).sin() * 2.0 * half_height
            }
        }
    }
}

/// A body's handle: its slot and the slot's generation (a removed body's handle never finds its successor).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BodyId {
    pub index: u32,
    pub generation: u32,
}

/// How to make a body.
#[derive(Clone, Copy, Debug)]
pub struct BodyDef {
    pub shape: Shape,
    pub pos: V3,
    pub rot: Quat,
    pub vel: V3,
    /// Angular velocity (radians a second, world frame).
    pub ang: V3,
    pub material: Material,
    /// Never moves (the ground, a pedestal).
    pub fixed: bool,
    /// Starts asleep: stands still until something awake touches it.
    pub asleep: bool,
    /// The game's own tag (what it is, which material it breaks like).
    pub user: u32,
    /// Collision layers: two bodies touch only if each one's `layer` is in the other's `mask` (debris that hits the
    /// ground but not the tower: thin, light pieces under heavy ones are what a solver handles worst).
    pub layer: u32,
    pub mask: u32,
}

impl BodyDef {
    pub fn new(shape: Shape, pos: V3) -> BodyDef {
        BodyDef {
            shape,
            pos,
            rot: Quat::IDENTITY,
            vel: V3::ZERO,
            ang: V3::ZERO,
            material: Material::default(),
            fixed: false,
            asleep: false,
            user: 0,
            layer: 1,
            mask: u32::MAX,
        }
    }
}

/// A body as it is now.
#[derive(Clone, Copy, Debug)]
pub struct Body {
    pub shape: Shape,
    pub pos: V3,
    pub rot: Quat,
    pub vel: V3,
    pub ang: V3,
    /// Where it was before the last step (for drawing between steps).
    pub prev_pos: V3,
    pub prev_rot: Quat,
    pub material: Material,
    /// 0 for a fixed body.
    pub inv_mass: f32,
    /// Inverse inertia about the local axes.
    pub inv_inertia: V3,
    pub fixed: bool,
    pub asleep: bool,
    pub user: u32,
    pub layer: u32,
    pub mask: u32,
}

impl Body {
    /// Whether this body and `o` can touch at all (their layers and masks).
    pub fn collides(&self, o: &Body) -> bool {
        self.layer & o.mask != 0 && o.layer & self.mask != 0
    }

    /// Its pose `alpha` of the way from before the last step to now.
    pub fn pose_at(&self, alpha: f32) -> (V3, Quat) {
        (self.prev_pos.lerp(self.pos, alpha), self.prev_rot.nlerp(self.rot, alpha))
    }

    pub fn mass(&self) -> f32 {
        if self.inv_mass > 0.0 { 1.0 / self.inv_mass } else { f32::INFINITY }
    }
}

/// Two bodies hitting each other in the last step: for breaking, sound, haptics and camera shake.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Impact {
    pub a: BodyId,
    pub b: BodyId,
    /// Where (the deepest contact point) and the normal, from `a` to `b`.
    pub point: V3,
    pub normal: V3,
    /// The total normal impulse the step applied between them (N·s).
    pub impulse: f32,
    /// How fast they were closing along the normal when the step began (m/s).
    pub speed: f32,
    /// They were not touching the step before.
    pub new: bool,
}

/// What the last step did (for the stats line and perf evals).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stats {
    pub bodies: u32,
    pub awake: u32,
    pub pairs: u32,
    pub manifolds: u32,
    pub points: u32,
    /// The deepest penetration left after the step (m): how well the solver converged.
    pub max_penetration: f32,
    /// The `user` tags of the pair that overlapped most (what to look at when penetration is high).
    pub deepest: Option<(u32, u32)>,
}
