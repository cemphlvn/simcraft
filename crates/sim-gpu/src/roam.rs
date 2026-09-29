//! First person in a voxel world (`games/<name>/roam.ron`): you are one entity of the game. WASD walks, the mouse
//! looks, shift runs, space jumps, walking into a wall climbs it; the mouse buttons dig and drop.
//!
//! Your body moves continuously in this view (`walker`); the game follows it one voxel a tick through a declared
//! action (`crawl`), and digging and dropping are declared actions too, checked like any agent's. So the game stays
//! the single truth, deterministic and replayable, and the feel of moving is the view's.
//!
//! Space: X = world x, Z = world y, Y up = depth - 1 - z (level 0 is the top of the world); one voxel, one unit.

use std::collections::BTreeMap;

use rayon::prelude::*;
use serde::Deserialize;
use sim_core::{Entity, World};
use sim_render::feel::Tween;
use sim_rules::Game;

use crate::gpu::{Mesh, Vert3};
use crate::math::V3;
use crate::stage::{BLOB, Quad, WHITE, Wrap};
use crate::track::Frame;
use crate::walker::{Input, WalkFeel, Walker, ray};

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Roam {
    pub assets: Vec<String>,
    /// The kind you are (one entity, controllable).
    pub you: String,
    /// Declared actions: each takes `dx`, `dy`, `dz` (from you to the voxel).
    pub actions: RoamActions,
    /// The terrain field is drawn voxel by voxel; each value with its material.
    pub materials: BTreeMap<i64, Material>,
    /// The painted sky behind everything (an image), and the fog colour it fades to at the horizon.
    #[serde(default)]
    pub sky: Option<String>,
    pub fog: (u8, u8, u8),
    /// Distance (voxels) at which the world has faded into the fog.
    #[serde(default = "d_far")]
    pub far: f32,
    /// How the other kinds look.
    #[serde(default)]
    pub kinds: BTreeMap<String, Body>,
    /// An invisible field made visible while a key is held (the pheromone).
    #[serde(default)]
    pub smell: Option<Smell>,
    #[serde(default)]
    pub feel: WalkFeel,
    #[serde(default)]
    pub keys: Keys,
}

fn d_far() -> f32 {
    60.0
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoamActions {
    pub crawl: String,
    pub dig: String,
    pub drop: String,
    /// How far you reach (voxels): matches the game's own check.
    #[serde(default = "d_reach")]
    pub reach: f32,
    /// The prop that says you hold a ball (drawn in your mandibles).
    #[serde(default)]
    pub carrying: Option<String>,
}

fn d_reach() -> f32 {
    2.5
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Material {
    pub image: String,
    /// Texture repeats per voxel.
    #[serde(default = "one")]
    pub scale: f32,
    /// Colour multiplied in (sRGB).
    #[serde(default = "white")]
    pub tint: (u8, u8, u8),
}

fn one() -> f32 {
    1.0
}

fn white() -> (u8, u8, u8) {
    (255, 255, 255)
}

/// A crawler's body, built from segments (abdomen, thorax, head) and six legs that step as it moves.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Body {
    pub color: (u8, u8, u8),
    pub head: (u8, u8, u8),
    /// Body length (voxels).
    #[serde(default = "d_size")]
    pub size: f32,
    /// A prop that, when non-zero, puts a ball in its mandibles; or a state it shows a ball in.
    #[serde(default)]
    pub carries_in: Option<String>,
}

fn d_size() -> f32 {
    0.8
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Smell {
    pub field: String,
    pub color: (u8, u8, u8),
    /// The value drawn at full strength; below a twentieth of it nothing is drawn.
    pub full: i64,
    /// Voxels around you that are shown.
    #[serde(default = "d_radius")]
    pub radius: i64,
}

fn d_radius() -> i64 {
    10
}

/// Key names: "a".."z", "0".."9", "space", "shift", "ctrl", "tab".
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Keys {
    pub forward: String,
    pub back: String,
    pub left: String,
    pub right: String,
    pub run: String,
    pub jump: String,
    pub smell: String,
}

impl Default for Keys {
    fn default() -> Self {
        let s = |x: &str| x.to_string();
        Keys { forward: s("w"), back: s("s"), left: s("a"), right: s("d"), run: s("shift"), jump: s("space"), smell: s("f") }
    }
}

fn rgb(c: (u8, u8, u8)) -> [f32; 3] {
    [c.0 as f32 / 255.0, c.1 as f32 / 255.0, c.2 as f32 / 255.0]
}

/// World voxel → view voxel and back (Y is up, the world's z is down).
fn to_view(w: &World, x: i64, y: i64, z: i64) -> [i64; 3] {
    [x, w.depth - 1 - z, y]
}

fn to_world(w: &World, v: [i64; 3]) -> (i64, i64, i64) {
    (v[0], v[2], w.depth - 1 - v[1])
}

/// Solid for the body: terrain, the floor under the world, and the world's sides (you stay in).
pub fn solid_for_body(w: &World, v: [i64; 3]) -> bool {
    let (x, y, z) = to_world(w, v);
    if z >= w.depth {
        return true;
    }
    if x < 0 || y < 0 || x >= w.width || y >= w.height {
        return true;
    }
    z >= 0 && w.is_terrain(x, y, z)
}

fn solid_voxel(w: &World, v: [i64; 3]) -> bool {
    let (x, y, z) = to_world(w, v);
    w.is_terrain(x, y, z)
}

/// A voxel face: its normal, the two in-face axes u and v, its shade.
type Face = ([i64; 3], [i64; 3], [i64; 3], f32);

/// A voxel in world coordinates.
pub type Voxel = (i64, i64, i64);

/// Terrain as meshes, one per material: only faces open to air, each corner darkened by the voxels around it
/// (ambient occlusion), so shapes read without lights. Rebuilt when the terrain changes.
pub fn terrain_meshes(w: &World, field: &str, materials: &BTreeMap<i64, Material>) -> Vec<Mesh> {
    let Some(vals) = w.field_values(field) else { return Vec::new() };
    let (wd, ht) = (w.width, w.height);
    const FACES: [Face; 6] = [
        ([0, 1, 0], [1, 0, 0], [0, 0, 1], 1.0),
        ([0, -1, 0], [1, 0, 0], [0, 0, 1], 0.5),
        ([1, 0, 0], [0, 0, 1], [0, 1, 0], 0.8),
        ([-1, 0, 0], [0, 0, 1], [0, 1, 0], 0.8),
        ([0, 0, 1], [1, 0, 0], [0, 1, 0], 0.68),
        ([0, 0, -1], [1, 0, 0], [0, 1, 0], 0.68),
    ];
    const AO: [f32; 4] = [0.5, 0.68, 0.84, 1.0];
    // One level per job, on all cores; merged in level order, so the meshes are the same at any core count.
    let plane = (wd * ht) as usize;
    let levels: Vec<BTreeMap<i64, Vec<Vert3>>> = (0..w.depth as usize)
        .into_par_iter()
        .map(|level| {
            let mut by: BTreeMap<i64, Vec<Vert3>> = BTreeMap::new();
            for (i, &val) in vals.iter().enumerate().skip(level * plane).take(plane) {
                if val == 0 {
                    continue;
                }
                let Some(mat) = materials.get(&val) else { continue };
                let i = i as i64;
                let (x, y, z) = (i % wd, (i / wd) % ht, i / (wd * ht));
                let c = to_view(w, x, y, z);
                let tint = rgb(mat.tint);
                let verts = by.entry(val).or_default();
                for (n, u, v, shade) in FACES {
                    let out = [c[0] + n[0], c[1] + n[1], c[2] + n[2]];
                    if solid_voxel(w, out) {
                        continue;
                    }
                    // The face's plane: the voxel's side toward n.
                    let base: [f32; 3] = std::array::from_fn(|k| c[k] as f32 + if n[k] > 0 { 1.0 } else { 0.0 });
                    let mut corners = [[0f32; 3]; 4];
                    let mut light = [0f32; 4];
                    for (j, (du, dv)) in [(0i64, 0i64), (1, 0), (1, 1), (0, 1)].into_iter().enumerate() {
                        corners[j] = std::array::from_fn(|k| base[k] + (u[k] * du + v[k] * dv) as f32);
                        // Occluders: the voxels beside the corner, just outside the face.
                        let su = if du == 0 { -1 } else { 1 };
                        let sv = if dv == 0 { -1 } else { 1 };
                        let at = |a: i64, b: i64| std::array::from_fn(|k| out[k] + u[k] * a + v[k] * b);
                        let (s1, s2, cr) = (solid_voxel(w, at(su, 0)), solid_voxel(w, at(0, sv)), solid_voxel(w, at(su, sv)));
                        let level = if s1 && s2 { 0 } else { 3 - (s1 as usize + s2 as usize + cr as usize) };
                        light[j] = AO[level] * shade;
                    }
                    let uv = |p: [f32; 3]| -> [f32; 2] {
                        let (a, b) = match n {
                            [0, _, 0] => (p[0], p[2]),
                            [_, 0, 0] => (p[2], -p[1]),
                            _ => (p[0], -p[1]),
                        };
                        [a * mat.scale, b * mat.scale]
                    };
                    for j in [0usize, 1, 2, 0, 2, 3] {
                        let p = corners[j];
                        let l = light[j];
                        verts.push(Vert3 { pos: p, uv: uv(p), color: [tint[0] * l, tint[1] * l, tint[2] * l, 1.0], fog: 0.0 });
                    }
                }
            }
            by
        })
        .collect();
    let mut by: BTreeMap<i64, Vec<Vert3>> = BTreeMap::new();
    for level in levels {
        for (val, verts) in level {
            by.entry(val).or_default().extend(verts);
        }
    }
    by.into_iter()
        .map(|(val, verts)| {
            let m = &materials[&val];
            Mesh { image: m.image.clone(), wrap: Wrap::Repeat, verts }
        })
        .collect()
}

/// An ellipsoid (radii along a local frame) as triangles, lit from above-front.
fn ellipsoid(out: &mut Vec<Vert3>, c: V3, axes: [V3; 3], radii: [f32; 3], color: [f32; 3]) {
    ellipsoid_at(out, c, axes, radii, color, true);
}

/// `fine`: 10 × 6 facets (near); else 6 × 4 (far away, where the facets are a pixel or two).
fn ellipsoid_at(out: &mut Vec<Vert3>, c: V3, axes: [V3; 3], radii: [f32; 3], color: [f32; 3], fine: bool) {
    let (seg, rings) = if fine { (10usize, 6usize) } else { (6, 4) };
    // The unit sphere's points, computed once (no trigonometry per vertex per frame).
    static UNIT: std::sync::OnceLock<[Vec<V3>; 2]> = std::sync::OnceLock::new();
    let table = |seg: usize, rings: usize| -> Vec<V3> {
        (0..=rings)
            .flat_map(|j| {
                (0..=seg).map(move |i| {
                    let th = std::f32::consts::PI * j as f32 / rings as f32;
                    let ph = std::f32::consts::TAU * i as f32 / seg as f32;
                    V3(th.sin() * ph.cos(), th.cos(), th.sin() * ph.sin())
                })
            })
            .collect()
    };
    let units = UNIT.get_or_init(|| [table(10, 6), table(6, 4)]);
    let unit = &units[if fine { 0 } else { 1 }];
    let sun = V3(0.35, 0.85, 0.4).norm();
    let point = |i: usize, j: usize| -> (V3, V3) {
        let n = unit[j * (seg + 1) + i];
        let p = axes[0].scale(n.0 * radii[0]) + axes[1].scale(n.1 * radii[1]) + axes[2].scale(n.2 * radii[2]);
        let normal = (axes[0].scale(n.0 / radii[0]) + axes[1].scale(n.1 / radii[1]) + axes[2].scale(n.2 / radii[2])).norm();
        (c + p, normal)
    };
    for j in 0..rings {
        for i in 0..seg {
            let q = [point(i, j), point(i + 1, j), point(i + 1, j + 1), point(i, j + 1)];
            for k in [0usize, 1, 2, 0, 2, 3] {
                let (p, n) = q[k];
                let l = 0.45 + 0.55 * n.dot(sun).max(0.0);
                out.push(Vert3 { pos: [p.0, p.1, p.2], uv: [0.5, 0.5], color: [color[0] * l, color[1] * l, color[2] * l, 1.0], fog: 0.0 });
            }
        }
    }
}

/// A thin segment (a leg) between two points.
fn stick(out: &mut Vec<Vert3>, a: V3, b: V3, up: V3, width: f32, color: [f32; 3]) {
    let side = (b - a).cross(up).norm().scale(width);
    let lift = up.scale(width);
    for (s, l) in [(side, 0.7f32), (lift, 1.0)] {
        let q = [a - s, a + s, b + s, b - s];
        for k in [0usize, 1, 2, 0, 2, 3] {
            let p = q[k];
            out.push(Vert3 { pos: [p.0, p.1, p.2], uv: [0.5, 0.5], color: [color[0] * l, color[1] * l, color[2] * l, 1.0], fog: 0.0 });
        }
    }
}

/// A crawler on the surface it clings to: `up` is that surface's normal, `fwd` where it heads, `phase` its gait.
#[allow(clippy::too_many_arguments)]
pub fn crawler(out: &mut Vec<Vert3>, body: &Body, feet: V3, up: V3, fwd: V3, phase: f32, carrying: bool, ball: [f32; 3]) {
    crawler_at(out, body, feet, up, fwd, phase, carrying, ball, true);
}

/// `fine`: full body and six legs (near); else coarse segments, no legs (far: a few pixels).
#[allow(clippy::too_many_arguments)]
pub fn crawler_at(out: &mut Vec<Vert3>, body: &Body, feet: V3, up: V3, fwd: V3, phase: f32, carrying: bool, ball: [f32; 3], fine: bool) {
    let f = (fwd - up.scale(fwd.dot(up))).norm();
    let f = if f.dot(f) > 0.5 { f } else { up.cross(V3(1.0, 0.0, 0.0)).norm() };
    let side = up.cross(f).norm();
    let s = body.size;
    let (col, head) = (rgb(body.color), rgb(body.head));
    let lift = up.scale(0.09 * s);
    let at = |along: f32| feet + lift + f.scale(along * s);
    // Abdomen (long, pale), thorax, head (darker, hard).
    ellipsoid_at(out, at(-0.22), [side, up, f], [0.14 * s, 0.1 * s, 0.24 * s], col, fine);
    ellipsoid_at(out, at(0.06), [side, up, f], [0.09 * s, 0.07 * s, 0.09 * s], col, fine);
    ellipsoid_at(out, at(0.22), [side, up, f], [0.09 * s, 0.075 * s, 0.1 * s], head, fine);
    if !fine {
        if carrying {
            ellipsoid_at(out, at(0.42), [side, up, f], [0.1 * s; 3], ball, false);
        }
        return;
    }
    let dark = [head[0] * 0.6, head[1] * 0.6, head[2] * 0.6];
    for m in [-1.0f32, 1.0] {
        let root = at(0.3) + side.scale(m * 0.04 * s);
        stick(out, root, root + f.scale(0.1 * s) - side.scale(m * 0.03 * s), up, 0.012 * s, dark);
    }
    // Six legs: alternating tripods swing with the gait.
    for (k, along) in [0.12f32, 0.04, -0.04].into_iter().enumerate() {
        for m in [-1.0f32, 1.0] {
            let swing = (phase + if (k % 2 == 0) == (m > 0.0) { 0.0 } else { std::f32::consts::PI }).sin() * 0.06 * s;
            let hip = at(along) + side.scale(m * 0.06 * s);
            let knee = hip + side.scale(m * 0.14 * s) + up.scale(0.06 * s) + f.scale(swing * 0.5);
            let foot = feet + side.scale(m * 0.24 * s) + f.scale(along * s + swing);
            stick(out, hip, knee, up, 0.012 * s, col);
            stick(out, knee, foot, up, 0.01 * s, col);
        }
    }
    if carrying {
        ellipsoid(out, at(0.42), [side, up, f], [0.1 * s; 3], ball);
    }
}

/// Where an entity clings: the face of its voxel that touches terrain (floor first, then walls, then ceiling),
/// as (feet position on that face, the face's normal, pointing away from the terrain).
pub fn cling_point(w: &World, x: f32, y: f32, z: f32) -> (V3, V3) {
    let (ix, iy, iz) = (x.round() as i64, y.round() as i64, z.round() as i64);
    let centre = V3(x + 0.5, (w.depth - 1) as f32 - z + 0.5, y + 0.5);
    for (dx, dy, dz, n) in [
        (0, 0, 1, V3(0.0, 1.0, 0.0)),
        (1, 0, 0, V3(-1.0, 0.0, 0.0)),
        (-1, 0, 0, V3(1.0, 0.0, 0.0)),
        (0, 1, 0, V3(0.0, 0.0, -1.0)),
        (0, -1, 0, V3(0.0, 0.0, 1.0)),
        (0, 0, -1, V3(0.0, -1.0, 0.0)),
    ] {
        if w.is_terrain(ix + dx, iy + dy, iz + dz) {
            return (centre - n.scale(0.5), n);
        }
    }
    (centre - V3(0.0, 0.5, 0.0), V3(0.0, 1.0, 0.0))
}

/// Everything the roam view keeps between frames.
pub struct RoamPlay {
    pub engine: sim_core::Engine<sim_core::Running, Game>,
    pub roam: Roam,
    pub walker: Walker,
    pub tween: Tween,
    pub time: f32,
    pub smell_on: bool,
    /// What the keys and the mouse ask right now (the window fills it; `look` is used up each frame).
    pub input: Input,
    /// A camera that is not yours (an overview for shots); None = your eyes.
    pub overview: Option<crate::math::Eye>,
    /// Terrain meshes and the field they were built from.
    terrain: (Vec<i64>, Vec<Mesh>),
    /// Bumped at each rebuild: the GPU keeps the terrain until it changes (`Gpu::keep`).
    terrain_version: u64,
    /// Crawlers' headings (last move) and gait phases.
    gait: BTreeMap<u64, (V3, f32)>,
    /// The last refused action and when (the target flashes red).
    pub refused: Option<(String, f32)>,
    /// Actions queued for the next tick (crawl is added each tick).
    pub ground_top: f32,
}

impl RoamPlay {
    pub fn new(engine: sim_core::Engine<sim_core::Running, Game>, roam: Roam) -> RoamPlay {
        let (pos, ground_top) = {
            let w = engine.world();
            let you = w.of_kind(&roam.you).next();
            let (feet, _) = you.map_or((V3(w.width as f32 / 2.0, w.depth as f32, w.height as f32 / 2.0), V3(0.0, 1.0, 0.0)), |e| {
                cling_point(w, e.x as f32, e.y as f32, e.z as f32)
            });
            // Where the ground's surface is (for the plain beyond the world's edge): the first terrain from the top.
            let top = (0..w.depth).find(|&z| w.is_terrain(0, 0, z)).map_or(0.0, |z| (w.depth - z) as f32);
            (feet, top)
        };
        let walker = Walker::new(roam.feel, pos, 35.0);
        RoamPlay {
            engine,
            roam,
            walker,
            tween: Tween::default(),
            time: 0.0,
            smell_on: false,
            input: Input::default(),
            overview: None,
            terrain: (Vec::new(), Vec::new()),
            terrain_version: 0,
            gait: BTreeMap::new(),
            refused: None,
            ground_top,
        }
    }

    pub fn world(&self) -> &World {
        self.engine.world()
    }

    fn you(&self) -> Option<&Entity> {
        self.world().of_kind(&self.roam.you).next()
    }

    /// The voxel your feet are in, in world coordinates.
    pub fn feet_voxel(&self) -> (i64, i64, i64) {
        let w = self.world();
        let p = self.walker.pos;
        to_world(w, [p.0.floor() as i64, (p.1 + 0.05).floor() as i64, p.2.floor() as i64])
    }

    /// Runs `n` ticks; before each, the game's you steps toward where your body is (one voxel at most).
    pub fn step(&mut self, n: u32) {
        for _ in 0..n {
            if self.engine.outcome().is_some() || self.world().tick >= self.engine.rules().cfg.run.max_ticks {
                return;
            }
            if let Some(e) = self.you() {
                let (fx, fy, fz) = self.feet_voxel();
                let d = [(fx - e.x).signum(), (fy - e.y).signum(), (fz - e.z).signum()];
                if d != [0, 0, 0] {
                    let args: BTreeMap<String, i64> = [("dx", d[0]), ("dy", d[1]), ("dz", d[2])].map(|(k, v)| (k.to_string(), v)).into();
                    if let Ok(g) = self.engine.rules().act(self.world(), None, e.id, &self.roam.actions.crawl, &args) {
                        self.engine.queue(g);
                    }
                }
            }
            self.tween.remember(self.engine.world());
            self.engine.tick();
        }
    }

    /// The body moves (every frame, not every tick).
    pub fn walk(&mut self, dt: f32, input: Input) {
        let world = self.engine.world();
        self.walker.step(dt, input, &|x, y, z| solid_for_body(world, [x, y, z]));
    }

    /// What the crosshair points at: (solid voxel, open voxel before it), in world coordinates.
    pub fn target(&self) -> Option<(Voxel, Voxel)> {
        let w = self.world();
        let hit = ray(self.walker.eye_pos(), self.walker.look_dir(), self.roam.actions.reach, &|x, y, z| solid_voxel(w, [x, y, z]))?;
        Some((to_world(w, hit.0), to_world(w, hit.1)))
    }

    fn carrying(&self) -> bool {
        let prop = self.roam.actions.carrying.as_deref();
        prop.is_some_and(|p| self.you().and_then(|e| e.props.get(p)).is_some_and(|v| *v != 0))
    }

    /// Dig (the voxel looked at) or drop (into the open voxel before it): a declared action, refused or queued.
    pub fn use_target(&mut self, dig: bool) {
        let Some((hit, before)) = self.target() else {
            self.refused = Some(("nothing within reach".into(), self.time));
            return;
        };
        let Some(e) = self.you() else { return };
        let (to, action) = if dig { (hit, &self.roam.actions.dig) } else { (before, &self.roam.actions.drop) };
        let args: BTreeMap<String, i64> =
            [("dx", to.0 - e.x), ("dy", to.1 - e.y), ("dz", to.2 - e.z)].map(|(k, v)| (k.to_string(), v)).into();
        match self.engine.rules().act(self.world(), None, e.id, action, &args) {
            Ok(g) => self.engine.queue(g),
            Err(why) => self.refused = Some((format!("{action}: {why}"), self.time)),
        }
    }

    pub fn frame(&mut self, w: f32, h: f32, dt: f32) -> Frame {
        self.time += dt;
        let eye = self.overview.unwrap_or_else(|| self.walker.eye());
        let world = self.engine.world();
        let fog = rgb(self.roam.fog);
        // Terrain: rebuilt only when it changed.
        let tf = self.engine.rules().def.terrain.clone().unwrap_or_default();
        if world.field_values(&tf).is_some_and(|v| v != self.terrain.0.as_slice()) {
            let meshes = terrain_meshes(world, &tf, &self.roam.materials);
            self.terrain = (world.field_values(&tf).map(<[i64]>::to_vec).unwrap_or_default(), meshes);
            self.terrain_version += 1;
        }
        // The terrain is not in the frame: the GPU keeps it (`terrain()`).
        let mut meshes: Vec<Mesh> = Vec::new();

        // The plain beyond the world's edge, at ground level: the ground goes on to the horizon. Tiles outside the
        // world only (inside, the terrain is the ground), small enough for the fog to fade across them.
        if let Some(m) = self.roam.materials.get(&1).or_else(|| self.roam.materials.values().next()) {
            let (y, tile, reach) = (self.ground_top, 8i64, (self.roam.far as i64 / 8 + 1) * 8);
            let tint = rgb(m.tint);
            let c = [tint[0] * 0.92, tint[1] * 0.92, tint[2] * 0.92, 1.0];
            let mut v = Vec::new();
            for tz in (-reach..world.height + reach).step_by(tile as usize) {
                for tx in (-reach..world.width + reach).step_by(tile as usize) {
                    // Inside the world the plain is its floor (under the lowest level: bedrock you cannot dig through).
                    let inside = tx >= 0 && tz >= 0 && tx + tile <= world.width && tz + tile <= world.height;
                    let y = if inside { 0.0 } else { y };
                    let (x0, z0, x1, z1) = (tx as f32, tz as f32, (tx + tile) as f32, (tz + tile) as f32);
                    let q = [V3(x0, y, z0), V3(x1, y, z0), V3(x1, y, z1), V3(x0, y, z1)];
                    for k in [0usize, 1, 2, 0, 2, 3] {
                        let p = q[k];
                        v.push(Vert3 { pos: [p.0, p.1, p.2], uv: [p.0 * m.scale, p.2 * m.scale], color: c, fog: 0.0 });
                    }
                }
            }
            meshes.push(Mesh { image: m.image.clone(), wrap: Wrap::Repeat, verts: v });
        }

        // Crawlers, between ticks, on whatever they cling to.
        let mut bodies = Vec::new();
        let ball = self.roam.materials.get(&2).map_or([0.45, 0.3, 0.2], |m| {
            let t = rgb(m.tint);
            [t[0] * 0.7, t[1] * 0.7, t[2] * 0.7]
        });
        // Gaits first (they remember), then every body on all cores.
        let mut jobs = Vec::new();
        for e in world.entities().values() {
            let Some(body) = self.roam.kinds.get(&e.kind) else { continue };
            let prev = self.tween.prev.get(&e.id).copied().unwrap_or((e.x, e.y, e.z));
            let moved = V3((e.x - prev.0) as f32, (prev.2 - e.z) as f32, (e.y - prev.1) as f32);
            let g = self.gait.entry(e.id).or_insert((V3(0.0, 0.0, 1.0), (e.id % 7) as f32));
            if moved.dot(moved) > 0.0 {
                g.0 = moved.norm();
                g.1 += dt * 14.0;
            }
            jobs.push((e, body, *g));
        }
        let tween = &self.tween;
        let (look, far) = (eye.basis().2, self.roam.far);
        let parts: Vec<Vec<Vert3>> = jobs
            .par_iter()
            .with_min_len(16)
            .filter_map(|(e, body, g)| {
                let (x, y, z) = tween.at(e.id, (e.x, e.y, e.z));
                let (feet, up) = cling_point(world, x, y, z);
                // Not drawn: lost in the fog, or behind you (with a margin for the field of view's edges).
                let to = feet - eye.pos;
                let dist = to.dot(to).sqrt();
                if dist > far || (dist > 1.5 && to.dot(look) < -0.2 * dist) {
                    return None;
                }
                let carrying =
                    body.carries_in.as_deref().is_some_and(|c| e.props.get(c).is_some_and(|v| *v != 0) || sim_state::in_label(&e.state, c));
                let mut v = Vec::with_capacity(1600);
                crawler_at(&mut v, body, feet, up, g.0, g.1, carrying, ball, dist < 12.0);
                Some(v)
            })
            .collect();
        bodies.extend(parts.into_iter().flatten());
        meshes.push(Mesh { image: WHITE.into(), wrap: Wrap::Clamp, verts: bodies });

        // What you hold, low in front of your eyes, swaying with your step; your mandibles either side.
        let (right, up, fwd) = eye.basis();
        let mut held = Vec::new();
        let sway = (self.time * 2.0).sin() * 0.004;
        let jaw = self.walker.eye_pos() + fwd.scale(0.22) - up.scale(0.1 + sway);
        // Mandibles: dark, curved hooks at the bottom corners of the view, closing a little as you walk.
        let mandible = [0.16, 0.1, 0.06];
        let close = 0.01 * (self.time * 6.0).sin() * (self.walker.speed() / self.walker.feel.walk).min(1.0);
        for m in [-1.0f32, 1.0] {
            let root = jaw + right.scale(m * 0.13) - up.scale(0.05);
            let mid = jaw + right.scale(m * (0.1 - close)) + fwd.scale(0.06) - up.scale(0.035);
            let tip = jaw + right.scale(m * (0.045 - close)) + fwd.scale(0.1) - up.scale(0.03);
            stick(&mut held, root, mid, fwd, 0.009, mandible);
            stick(&mut held, mid, tip, fwd, 0.005, mandible);
        }
        if self.carrying() {
            ellipsoid(&mut held, jaw + fwd.scale(0.06), [right, up, fwd], [0.06, 0.055, 0.06], ball);
        }
        meshes.push(Mesh { image: WHITE.into(), wrap: Wrap::Clamp, verts: held });

        // The voxel you point at: its face glows (red for a moment after a refusal).
        let refused_now = self.refused.as_ref().is_some_and(|(_, at)| self.time - at < 0.4);
        if let Some((hit, before)) = self.target() {
            let a = to_view(world, hit.0, hit.1, hit.2);
            let b = to_view(world, before.0, before.1, before.2);
            let n = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let glow = if refused_now { [0.9, 0.2, 0.15, 0.35] } else { [0.35, 0.3, 0.2, 0.22] };
            meshes.push(Mesh { image: WHITE.into(), wrap: Wrap::Clamp, verts: face_quad(a, n, glow) });
        }

        // The smell, made visible: soft glows where the field is strong, near you.
        if self.smell_on
            && let Some(s) = &self.roam.smell
            && world.field_values(&s.field).is_some()
        {
            let (fx, fy, fz) = self.feet_voxel();
            let col = rgb(s.color);
            let mut v = Vec::new();
            for z in (fz - s.radius).max(0)..=(fz + s.radius).min(world.depth - 1) {
                for y in (fy - s.radius).max(0)..=(fy + s.radius).min(world.height - 1) {
                    for x in (fx - s.radius).max(0)..=(fx + s.radius).min(world.width - 1) {
                        let val = world.field(&s.field, x, y, z).unwrap_or(0);
                        if val * 20 < s.full || world.is_terrain(x, y, z) {
                            continue;
                        }
                        let k = (val as f32 / s.full as f32).min(1.0);
                        let c = V3(x as f32 + 0.5, (world.depth - 1 - z) as f32 + 0.5, y as f32 + 0.5);
                        let r = 0.18 + 0.3 * k;
                        let q = [
                            c - right.scale(r) - up.scale(r),
                            c + right.scale(r) - up.scale(r),
                            c + right.scale(r) + up.scale(r),
                            c - right.scale(r) + up.scale(r),
                        ];
                        let uvs = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
                        let a = 0.55 * k;
                        for j in [0usize, 1, 2, 0, 2, 3] {
                            let p = q[j];
                            v.push(Vert3 { pos: [p.0, p.1, p.2], uv: uvs[j], color: [col[0] * a, col[1] * a, col[2] * a, a], fog: 0.0 });
                        }
                    }
                }
            }
            meshes.push(Mesh { image: BLOB.into(), wrap: Wrap::Clamp, verts: v });
        }

        // Fog by distance from the eye is the GPU's (`fog_range`), for the kept terrain and these meshes alike.

        // Behind: sky above the horizon, turning with your view.
        let mut back = Vec::new();
        let horizon = eye.project(eye.pos + V3(fwd.0, 0.0, fwd.2).norm().scale(1000.0), w, h).map_or(h * 0.5, |p| p.1);
        back.push(Quad {
            top: [fog[0] * 0.8, fog[1] * 0.9, 1.0, 1.0],
            bottom: [fog[0], fog[1], fog[2], 1.0],
            ..plain(WHITE, 0.0, 0.0, w, h)
        });
        if let Some(sky) = &self.roam.sky {
            let hgt = (horizon + h * 0.05).max(1.0);
            let u0 = self.walker.view_yaw / 120.0;
            back.push(Quad { uv: [u0, 0.0, u0 + w / h * 0.6, 1.0], wrap: Wrap::MirrorX, ..plain(sky, 0.0, horizon - hgt, w, hgt) });
        }

        // In front: a small crosshair.
        let cross = if refused_now { [1.0, 0.35, 0.3, 0.9] } else { [1.0, 1.0, 1.0, 0.75] };
        let front = vec![plain_c(BLOB, w / 2.0 - 4.0, h / 2.0 - 4.0, 8.0, 8.0, cross)];
        Frame { back, eye, fog, meshes, front }
    }

    /// The terrain meshes and their version (bumped at each rebuild), for `Gpu::keep`.
    pub fn terrain(&self) -> (u64, &[Mesh]) {
        (self.terrain_version, &self.terrain.1)
    }

    /// Fog by distance: starts at, and fully fogged this much farther (for `World3::fog_range`).
    pub fn fog_range(&self) -> [f32; 2] {
        [self.roam.far * 0.35, self.roam.far * 0.65]
    }

    pub fn title(&self) -> String {
        let w = self.world();
        let refused = match &self.refused {
            Some((why, at)) if self.time - at < 3.0 => format!(" — {why}"),
            _ => String::new(),
        };
        let hands = if self.carrying() { "carrying mud" } else { "empty-handed" };
        format!(
            "{} — tick {} — {hands} — smell {} (f){refused}",
            self.engine.rules().def.name,
            w.tick,
            if self.smell_on { "on" } else { "off" }
        )
    }
}

fn plain(image: &str, x: f32, y: f32, w: f32, h: f32) -> Quad {
    plain_c(image, x, y, w, h, [1.0; 4])
}

fn plain_c(image: &str, x: f32, y: f32, w: f32, h: f32, c: [f32; 4]) -> Quad {
    Quad {
        image: image.into(),
        x,
        y,
        w,
        h,
        uv: [0.0, 0.0, 1.0, 1.0],
        top: c,
        bottom: c,
        blur: 0.0,
        desat: 0.0,
        wrap: Wrap::Clamp,
        rot: 0.0,
    }
}

/// The face of voxel `a` toward normal `n`, pushed out a hair so it draws over the terrain.
fn face_quad(a: [i64; 3], n: [i64; 3], c: [f32; 4]) -> Vec<Vert3> {
    let axis = (0..3).find(|&k| n[k] != 0).unwrap_or(1);
    let (u, v) = match axis {
        0 => (2, 1),
        1 => (0, 2),
        _ => (0, 1),
    };
    let mut base: [f32; 3] = std::array::from_fn(|k| a[k] as f32);
    base[axis] += if n[axis] > 0 { 1.004 } else { -0.004 };
    let corner = |du: f32, dv: f32| {
        let mut p = base;
        p[u] += du;
        p[v] += dv;
        p
    };
    let q = [corner(0.0, 0.0), corner(1.0, 0.0), corner(1.0, 1.0), corner(0.0, 1.0)];
    let pm = [c[0] * c[3], c[1] * c[3], c[2] * c[3], c[3]];
    [0usize, 1, 2, 0, 2, 3].iter().map(|&j| Vert3 { pos: q[j], uv: [0.5, 0.5], color: pm, fog: 0.0 }).collect()
}

/// A quick look for tests and `--shot`: stand a few voxels from the tallest built column, facing it.
pub fn face_the_work(p: &mut RoamPlay, value: i64) {
    let w = p.world();
    let tf = p.engine.rules().def.terrain.clone().unwrap_or_default();
    let Some(vals) = w.field_values(&tf) else { return };
    let mut cols: BTreeMap<(i64, i64), i64> = BTreeMap::new();
    for (i, v) in vals.iter().enumerate() {
        if *v == value {
            let i = i as i64;
            *cols.entry((i % w.width, (i / w.width) % w.height)).or_default() += 1;
        }
    }
    let Some(((cx, cy), _)) = cols.iter().max_by_key(|(k, n)| (**n, std::cmp::Reverse(**k))) else { return };
    let target = V3(*cx as f32 + 0.5, p.ground_top + 1.0, *cy as f32 + 0.5);
    p.overview = p.overview.map(|e| crate::math::Eye { pos: target + V3(-9.0, 11.0, -9.0), target, ..e });
    let from = V3((target.0 - 5.0).max(1.0), p.ground_top, (target.2 - 5.0).max(1.0));
    let d = target - from;
    p.walker.pos = from;
    p.walker.yaw = d.0.atan2(d.2).to_degrees();
    p.walker.view_yaw = p.walker.yaw;
    p.walker.pitch = 5.0;
    p.walker.view_pitch = 5.0;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world() -> World {
        let mut w = World::new3(1, 4, 4, 4);
        w.add_field("mud", 0);
        w.set_terrain(Some("mud".into()));
        for y in 0..4 {
            for x in 0..4 {
                w.set_field("mud", x, y, 3, 1);
            }
        }
        w.set_field("mud", 1, 1, 2, 2);
        w
    }

    #[test]
    fn only_faces_open_to_air_are_meshed() {
        let w = world();
        let mats: BTreeMap<i64, Material> =
            [(1, "earth"), (2, "mud")].map(|(k, n)| (k, Material { image: n.into(), scale: 1.0, tint: (255, 255, 255) })).into();
        let meshes = terrain_meshes(&w, "mud", &mats);
        let faces = |img: &str| meshes.iter().find(|m| m.image == img).map_or(0, |m| m.verts.len() / 6);
        // Earth: 16 tops (one under the ball), 16 bottoms, 16 outer sides.
        assert_eq!(faces("earth"), 15 + 16 + 16);
        // The ball: its top and four sides; not its bottom (earth under it).
        assert_eq!(faces("mud"), 5);
        let ao = meshes.iter().find(|m| m.image == "earth").unwrap().verts.iter().map(|v| v.color[0]).fold(1.0f32, f32::min);
        assert!(ao < 0.6, "corners beside the ball are darker");
    }

    #[test]
    fn a_crawler_clings_to_the_floor_or_the_wall_beside_it() {
        let mut w = world();
        let (feet, up) = cling_point(&w, 0.0, 0.0, 2.0);
        assert_eq!(up, V3(0.0, 1.0, 0.0));
        assert!((feet.1 - 1.0).abs() < 1e-6, "stands on the ground's top face");
        // A column two balls high; one level above the ground beside it there is no floor, only its side.
        w.set_field("mud", 1, 1, 1, 2);
        let (feet, up) = cling_point(&w, 0.0, 1.0, 1.0);
        assert_eq!(up, V3(-1.0, 0.0, 0.0), "clings to the column's side, facing away from it");
        assert!((feet.0 - 1.0).abs() < 1e-6, "on the column's face");
    }
}
