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
use crate::track::{Frame, Press};
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
    /// How the terrain's surface is drawn: `Blocks` (a cube per voxel) or `Smooth` (one continuous, rounded
    /// surface through the same voxels: flat ground stays exactly on the voxel faces).
    #[serde(default)]
    pub surface: Surface,
    /// A camera like a macro lens: depth of field, sun shadows, haze (None = everything sharp and unshadowed).
    #[serde(default)]
    pub lens: Option<LensSpec>,
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
    /// Where the view was loaded from (models are found relative to it).
    #[serde(skip)]
    pub dir: std::path::PathBuf,
    /// Images for model texture overrides (from the asset packs), ready for the GPU.
    #[serde(skip)]
    pub images: BTreeMap<String, crate::model::Texture>,
    /// The models its kinds use, by file, loaded (`load_roam`).
    #[serde(skip)]
    pub models: BTreeMap<String, std::sync::Arc<crate::model::Model>>,
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
    #[serde(default = "pale")]
    pub color: (u8, u8, u8),
    #[serde(default = "pale")]
    pub head: (u8, u8, u8),
    /// Body length (voxels).
    #[serde(default = "d_size")]
    pub size: f32,
    /// A prop that, when non-zero, puts a ball in its mandibles; or a state it shows a ball in.
    #[serde(default)]
    pub carries_in: Option<String>,
    /// A model (glTF) instead of the built-in body: its clips follow the game's states (see `ModelSpec`).
    #[serde(default)]
    pub model: Option<ModelSpec>,
}

fn d_size() -> f32 {
    0.8
}

fn pale() -> (u8, u8, u8) {
    (230, 220, 200)
}

/// How a model plays a kind: the contract between a modelling tool's file and the game.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelSpec {
    /// The `.glb` (relative to the game's folder, else an `assets/` folder above it). glTF conventions: +Y up,
    /// +Z forward, feet at y = 0.
    pub file: String,
    /// Lighter versions, from a distance (voxels) on: `[(8.0, "termite_lod1.glb"), ...]`.
    #[serde(default)]
    pub lods: Vec<(f32, String)>,
    /// Body length in voxels along +Z (the model is scaled to it).
    #[serde(default = "d_size")]
    pub length: f32,
    /// The game's states → clips: the first entry whose state selector the entity is in (`*` = any).
    #[serde(default)]
    pub clips: Vec<(String, String)>,
    /// The clip when it did not move this tick (e.g. `idle`).
    #[serde(default)]
    pub still: Option<String>,
    /// Clip playback rate (1 = as authored).
    #[serde(default = "one")]
    pub rate: f32,
    /// The socket node where a carried ball sits (`carries_in` says when).
    #[serde(default)]
    pub carry: Option<String>,
    /// Material name → an image from the asset packs (e.g. a Higgsfield texture) as its base colour.
    #[serde(default)]
    pub textures: BTreeMap<String, String>,
}

impl ModelSpec {
    /// The clip for an entity in `state`, moving or not.
    pub fn clip_for(&self, state: &str, moving: bool) -> String {
        if !moving && let Some(s) = &self.still {
            return s.clone();
        }
        self.clips
            .iter()
            .find(|(sel, _)| sel == "*" || sim_state::in_label(state, sel))
            .map_or_else(|| "rest".to_string(), |(_, c)| c.clone())
    }

    /// Every file it uses (the model and its lighter versions).
    pub fn files(&self) -> impl Iterator<Item = &String> {
        std::iter::once(&self.file).chain(self.lods.iter().map(|(_, f)| f))
    }
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

/// Is this view voxel solid? Inside the world: its terrain. Beside it: untouched ground up to `ground` (the view
/// height of the ground's top at the start, what the plain around the world shows), so a pit dug at the edge has an
/// outer wall instead of a hole into the void.
fn solid_voxel(w: &World, v: [i64; 3], ground: i64) -> bool {
    let (x, y, z) = to_world(w, v);
    if (0..w.width).contains(&x) && (0..w.height).contains(&y) { w.is_terrain(x, y, z) } else { (0..ground).contains(&v[1]) }
}

/// A voxel face: its normal, the two in-face axes u and v, its shade.
type Face = ([i64; 3], [i64; 3], [i64; 3], f32);

/// A voxel in world coordinates.
pub type Voxel = (i64, i64, i64);

/// Terrain as meshes, one per material: only faces open to air, each corner darkened by the voxels around it
/// (ambient occlusion), so shapes read without lights. Rebuilt when the terrain changes.
/// `aperture`: how fast things blur away from the focus (0 = pinhole; 0.05–0.15 reads as macro); `haze`: light
/// scattered towards the sun in the fog; `shadow`: how dark the sun's shade (0 = no shadows).
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LensSpec {
    #[serde(default)]
    pub aperture: f32,
    #[serde(default)]
    pub haze: f32,
    #[serde(default)]
    pub shadow: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
pub enum Surface {
    #[default]
    Blocks,
    Smooth,
}

/// The terrain as one smooth surface (surface nets): samples at voxel centres (solid or not), a vertex in each
/// cell the surface crosses, a quad across each edge between a solid and an open sample. Flat ground lies exactly on
/// the voxel faces (walking and clinging match what is drawn); edges and corners are rounded by relaxing each
/// vertex towards its neighbours, inside its cell. Lighting is baked per vertex from a smooth normal (sun, sky,
/// ground) and how enclosed the point is (ambient occlusion). One mesh per material (the solid side's value).
pub fn smooth_terrain_meshes(w: &World, field: &str, materials: &BTreeMap<i64, Material>, ground: i64) -> Vec<Mesh> {
    smooth_terrain(w, field, materials, ground).0
}

/// The drawn smooth surface, for standing on it: a vertex and a normal per cell the surface crosses.
#[derive(Clone, Debug, Default)]
pub struct SurfaceMap {
    origin: [i64; 3],
    dims: [usize; 3],
    verts: Vec<Option<V3>>,
    normals: Vec<V3>,
}

impl SurfaceMap {
    /// The point of the drawn surface under `p` (the nearest surface vertex's tangent plane) and its normal; None
    /// where there is no surface nearby.
    pub fn stand(&self, p: V3) -> Option<(V3, V3)> {
        let c = [(p.0 - 0.5).floor() as i64, (p.1 - 0.5).floor() as i64, (p.2 - 0.5).floor() as i64];
        let mut best: Option<(f32, V3, V3)> = None;
        for dz in -1..=1 {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let q = [c[0] + dx - self.origin[0], c[1] + dy - self.origin[1], c[2] + dz - self.origin[2]];
                    if q.iter().zip(self.dims).any(|(v, d)| *v < 0 || *v as usize >= d) {
                        continue;
                    }
                    let i = (q[2] as usize * self.dims[1] + q[1] as usize) * self.dims[0] + q[0] as usize;
                    let (Some(v), n) = (self.verts[i], self.normals[i]) else { continue };
                    let d = (v - p).dot(v - p);
                    if n.dot(n) > 0.0 && best.is_none_or(|b| d < b.0) {
                        best = Some((d, v, n.norm()));
                    }
                }
            }
        }
        let (d, v, n) = best?;
        (d < 1.5).then(|| (p - n.scale((p - v).dot(n)), n))
    }
}

/// The smooth surface's meshes, and the map to stand on it.
pub fn smooth_terrain(w: &World, field: &str, materials: &BTreeMap<i64, Material>, ground: i64) -> (Vec<Mesh>, SurfaceMap) {
    let Some(vals) = w.field_values(field) else { return (Vec::new(), SurfaceMap::default()) };
    let (wd, ht, dp) = (w.width, w.height, w.depth);
    // View sample grid: X = x, Y = up (depth - 1 - z), Z = y. Outside the world: below is solid, above is open,
    // beside it the untouched ground up to `ground` (as the plain shows it; the first material).
    let outside = *materials.keys().next().unwrap_or(&1);
    let value = |x: i64, yv: i64, z: i64| -> i64 {
        if yv < 0 {
            return outside;
        }
        if yv >= dp {
            return 0;
        }
        if !(0..wd).contains(&x) || !(0..ht).contains(&z) {
            return if yv < ground { outside } else { 0 };
        }
        vals[((dp - 1 - yv) * ht * wd + z * wd + x) as usize]
    };
    let solid = |x: i64, y: i64, z: i64| value(x, y, z) != 0;
    // Cells span samples (i..i+1) in each axis; sample i sits at i + 0.5. Two margin cells around the world (the
    // outer walls of pits dug at the edge).
    let (x0, x1, y0, y1, z0, z1) = (-2i64, wd + 1, -1i64, dp, -2i64, ht + 1);
    let (nx, ny, nz) = ((x1 - x0) as usize, (y1 - y0) as usize, (z1 - z0) as usize);
    let cell = |x: i64, y: i64, z: i64| ((z - z0) as usize * ny + (y - y0) as usize) * nx + (x - x0) as usize;
    let mut vert: Vec<Option<V3>> = vec![None; nx * ny * nz];
    for z in z0..z1 {
        for y in y0..y1 {
            for x in x0..x1 {
                let c = |dx: i64, dy: i64, dz: i64| solid(x + dx, y + dy, z + dz);
                let corners = [c(0, 0, 0), c(1, 0, 0), c(0, 1, 0), c(1, 1, 0), c(0, 0, 1), c(1, 0, 1), c(0, 1, 1), c(1, 1, 1)];
                if corners.iter().all(|&s| s) || corners.iter().all(|&s| !s) {
                    continue;
                }
                // The mean of the crossing points (edge midpoints) of the 12 cell edges.
                let (mut sum, mut n) = (V3(0.0, 0.0, 0.0), 0.0f32);
                for (a, b) in [(0, 1), (2, 3), (4, 5), (6, 7), (0, 2), (1, 3), (4, 6), (5, 7), (0, 4), (1, 5), (2, 6), (3, 7)] {
                    if corners[a] != corners[b] {
                        let p = |i: usize| V3((i & 1) as f32, ((i >> 1) & 1) as f32, ((i >> 2) & 1) as f32);
                        sum = sum + (p(a) + p(b)).scale(0.5);
                        n += 1.0;
                    }
                }
                let base = V3(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5);
                vert[cell(x, y, z)] = Some(base + sum.scale(1.0 / n));
            }
        }
    }
    // Relax: each vertex moves halfway to the mean of its neighbours, and stays in its cell. Flat stays flat.
    for _ in 0..2 {
        let prev = vert.clone();
        for z in z0..z1 {
            for y in y0..y1 {
                for x in x0..x1 {
                    let Some(v) = prev[cell(x, y, z)] else { continue };
                    let (mut sum, mut n) = (V3(0.0, 0.0, 0.0), 0.0f32);
                    for (dx, dy, dz) in [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)] {
                        let (a, b, c) = (x + dx, y + dy, z + dz);
                        if (x0..x1).contains(&a)
                            && (y0..y1).contains(&b)
                            && (z0..z1).contains(&c)
                            && let Some(u) = prev[cell(a, b, c)]
                        {
                            sum = sum + u;
                            n += 1.0;
                        }
                    }
                    if n > 0.0 {
                        let m = v + (sum.scale(1.0 / n) - v).scale(0.5);
                        let lo = V3(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5);
                        vert[cell(x, y, z)] = Some(V3(
                            m.0.clamp(lo.0 + 0.02, lo.0 + 0.98),
                            m.1.clamp(lo.1 + 0.02, lo.1 + 0.98),
                            m.2.clamp(lo.2 + 0.02, lo.2 + 0.98),
                        ));
                    }
                }
            }
        }
    }
    // Quads: across each sample edge whose ends differ, joining the four cells around that edge (by cell index:
    // a vertex is its cell).
    let mut quads: Vec<([usize; 4], V3, i64)> = Vec::new();
    for z in z0 + 1..z1 {
        for y in y0 + 1..y1 {
            for x in x0 + 1..x1 {
                let here = value(x, y, z);
                for axis in 0..3 {
                    let (dx, dy, dz) = [(1, 0, 0), (0, 1, 0), (0, 0, 1)][axis];
                    let there = value(x + dx, y + dy, z + dz);
                    if (here != 0) == (there != 0) || x + dx >= x1 || y + dy >= y1 || z + dz >= z1 {
                        continue;
                    }
                    // The four cells sharing this edge: offsets in the two other axes.
                    let (u, v) = [((0, 1, 0), (0, 0, 1)), ((0, 0, 1), (1, 0, 0)), ((1, 0, 0), (0, 1, 0))][axis];
                    let at = |a: i64, b: i64| cell(x - u.0 * a - v.0 * b, y - u.1 * a - v.1 * b, z - u.2 * a - v.2 * b);
                    let q = [at(1, 1), at(0, 1), at(0, 0), at(1, 0)];
                    if q.iter().any(|&c| vert[c].is_none()) {
                        continue;
                    }
                    // Normal: from the solid sample towards the open one.
                    let sign = if here != 0 { 1.0 } else { -1.0 };
                    let n = V3(dx as f32 * sign, dy as f32 * sign, dz as f32 * sign);
                    quads.push((q, n, if here != 0 { here } else { there }));
                }
            }
        }
    }
    let pos = |c: usize| vert[c].expect("checked when the quad was made");
    // Smooth normals: the mean of the face normals around each vertex, per cell.
    let mut normals = vec![V3(0.0, 0.0, 0.0); vert.len()];
    for (q, n, _) in &quads {
        let face = (pos(q[2]) - pos(q[0])).cross(pos(q[3]) - pos(q[1]));
        let face = if face.dot(*n) < 0.0 { face.scale(-1.0) } else { face };
        for &c in q {
            normals[c] = normals[c] + face;
        }
    }
    // How enclosed a point is: the share of open samples in the 4x4x4 block around it, from a 3D prefix sum of the
    // open samples (eight lookups a query instead of 64).
    let (px0, py0, pz0) = (x0 - 3, y0 - 3, z0 - 3);
    let (pnx, pny, pnz) = ((x1 - x0 + 7) as usize, (y1 - y0 + 7) as usize, (z1 - z0 + 7) as usize);
    let pidx = |x: usize, y: usize, z: usize| (z * (pny + 1) + y) * (pnx + 1) + x;
    let mut prefix = vec![0i32; (pnx + 1) * (pny + 1) * (pnz + 1)];
    for z in 0..pnz {
        for y in 0..pny {
            for x in 0..pnx {
                let open = !solid(px0 + x as i64, py0 + y as i64, pz0 + z as i64) as i32;
                prefix[pidx(x + 1, y + 1, z + 1)] =
                    open + prefix[pidx(x, y + 1, z + 1)] + prefix[pidx(x + 1, y, z + 1)] + prefix[pidx(x + 1, y + 1, z)]
                        - prefix[pidx(x, y, z + 1)]
                        - prefix[pidx(x, y + 1, z)]
                        - prefix[pidx(x + 1, y, z)]
                        + prefix[pidx(x, y, z)];
            }
        }
    }
    let openness = |p: V3| -> f32 {
        let lo = |v: f32, o: i64| ((v - 2.0).floor() as i64 - o).max(0) as usize;
        let (ax, ay, az) = (lo(p.0, px0).min(pnx - 4), lo(p.1, py0).min(pny - 4), lo(p.2, pz0).min(pnz - 4));
        let (bx, by, bz) = (ax + 4, ay + 4, az + 4);
        let sum = prefix[pidx(bx, by, bz)] - prefix[pidx(ax, by, bz)] - prefix[pidx(bx, ay, bz)] - prefix[pidx(bx, by, az)]
            + prefix[pidx(ax, ay, bz)]
            + prefix[pidx(ax, by, az)]
            + prefix[pidx(bx, ay, az)]
            - prefix[pidx(ax, ay, az)];
        sum as f32 / 64.0
    };
    // Light per vertex, once per cell: sun, sky and ground by the smooth normal, times the openness.
    let sun = V3(0.35, 0.85, 0.4).norm();
    let mut light = vec![f32::NAN; vert.len()];
    let mut by: BTreeMap<i64, Vec<Vert3>> = BTreeMap::new();
    for (q, n, material) in quads {
        let Some(mat) = materials.get(&material) else { continue };
        let tint = rgb(mat.tint);
        // Texture along the quad's own axis (its sample-edge direction): no stretching on walls or floors.
        let uv = |p: V3| -> [f32; 2] {
            let (a, b) = if n.1 != 0.0 {
                (p.0, p.2)
            } else if n.0 != 0.0 {
                (p.2, -p.1)
            } else {
                (p.0, -p.1)
            };
            [a * mat.scale, b * mat.scale]
        };
        let face = (pos(q[2]) - pos(q[0])).cross(pos(q[3]) - pos(q[1]));
        let order: [usize; 6] = if face.dot(n) >= 0.0 { [0, 1, 2, 0, 2, 3] } else { [0, 2, 1, 0, 3, 2] };
        let verts = by.entry(material).or_default();
        for i in order {
            let c = q[i];
            let p = pos(c);
            if light[c].is_nan() {
                let nn = if normals[c].dot(normals[c]) > 0.0 { normals[c].norm() } else { n };
                let l = 0.42 + 0.34 * (nn.1 * 0.5 + 0.5) + 0.38 * nn.dot(sun).max(0.0);
                let ao = 0.55 + 0.9 * openness(p).min(0.5);
                light[c] = (l * ao).min(1.15);
            }
            let l = light[c];
            verts.push(Vert3 { pos: [p.0, p.1, p.2], uv: uv(p), color: [tint[0] * l, tint[1] * l, tint[2] * l, 1.0], fog: 0.0 });
        }
    }
    let meshes = by.into_iter().map(|(val, verts)| Mesh { image: materials[&val].image.clone(), wrap: Wrap::Repeat, verts }).collect();
    (meshes, SurfaceMap { origin: [x0, y0, z0], dims: [nx, ny, nz], verts: vert, normals })
}

pub fn terrain_meshes(w: &World, field: &str, materials: &BTreeMap<i64, Material>, ground: i64) -> Vec<Mesh> {
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
                    if solid_voxel(w, out, ground) {
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
                        let (s1, s2, cr) =
                            (solid_voxel(w, at(su, 0), ground), solid_voxel(w, at(0, sv), ground), solid_voxel(w, at(su, sv), ground));
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
    // Outer walls: the untouched ground beside the world, where it faces an open voxel inside it (a pit at the edge).
    if let Some((&val, mat)) = materials.iter().next() {
        let tint = rgb(mat.tint);
        let verts = by.entry(val).or_default();
        for (x, z, n) in (0..wd)
            .flat_map(|x| [(x, -1, [0i64, 0, 1]), (x, ht, [0, 0, -1])])
            .chain((0..ht).flat_map(|z| [(-1, z, [1i64, 0, 0]), (wd, z, [-1, 0, 0])]))
        {
            for yv in 0..ground {
                let out = [x + n[0], yv, z + n[2]];
                if solid_voxel(w, out, ground) {
                    continue;
                }
                let (u, v) = if n[0] != 0 { ([0i64, 0, 1], [0i64, 1, 0]) } else { ([1, 0, 0], [0, 1, 0]) };
                let base: [f32; 3] = std::array::from_fn(|k| [x, yv, z][k] as f32 + if n[k] > 0 { 1.0 } else { 0.0 });
                let corners: Vec<[f32; 3]> = [(0i64, 0i64), (1, 0), (1, 1), (0, 1)]
                    .into_iter()
                    .map(|(du, dv)| std::array::from_fn(|k| base[k] + (u[k] * du + v[k] * dv) as f32))
                    .collect();
                let l = 0.62;
                for j in [0usize, 1, 2, 0, 2, 3] {
                    let p = corners[j];
                    let uv = if n[0] != 0 { [p[2] * mat.scale, -p[1] * mat.scale] } else { [p[0] * mat.scale, -p[1] * mat.scale] };
                    verts.push(Vert3 { pos: p, uv, color: [tint[0] * l, tint[1] * l, tint[2] * l, 1.0], fog: 0.0 });
                }
            }
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
                let (ii, jj) = [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)][k];
                let uv = [ii as f32 / seg as f32, jj as f32 / rings as f32];
                out.push(Vert3 { pos: [p.0, p.1, p.2], uv, color: [color[0] * l, color[1] * l, color[2] * l, 1.0], fog: 0.0 });
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

/// What a frame (and the ticks before it) did: timings (noise) and exact counts (not noise). Reset by the caller.
#[derive(Clone, Debug, Default, serde::Serialize, Deserialize)]
pub struct FrameStats {
    pub ticks: u32,
    pub ticks_ms: f64,
    pub build_ms: f64,
    pub terrain_rebuilt: bool,
    pub terrain_ms: f64,
    pub terrain_verts: usize,
    pub bodies: usize,
    pub verts: usize,
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
    /// Every action queued so far, in queue order (the order `apply` sees): with the game's seed and panel, the
    /// whole run. A spike report carries it, so the state at any frame can be rebuilt and checked by its hash.
    pub log: Vec<Press>,
    /// Replaying a log: its presses (queued at their ticks) and how far it got; input is ignored.
    pub script: Option<(Vec<Press>, usize)>,
    /// What the last frame and ticks did (timings and exact counts), for the spike watch.
    pub stats: FrameStats,
    /// A camera that is not yours (an overview for shots); None = your eyes.
    pub overview: Option<crate::math::Eye>,
    /// Terrain meshes and the field they were built from.
    terrain: (Vec<i64>, Vec<Mesh>),
    /// Bumped at each rebuild: the GPU keeps the terrain until it changes (`Gpu::keep`).
    terrain_version: u64,
    /// Crawlers' headings (last move) and gait phases.
    gait: BTreeMap<u64, (V3, f32)>,
    /// The drawn smooth surface, to stand crawlers on (None with block terrain: the voxel faces are what is drawn).
    surface: Option<SurfaceMap>,
    /// Where the lens is focused (eases towards what the crosshair looks at, like autofocus).
    focus: f32,
    /// Each entity's clip time, and when (view time) it last moved.
    clock: BTreeMap<u64, f32>,
    moved_at: BTreeMap<u64, f32>,
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
            log: Vec::new(),
            script: None,
            stats: FrameStats::default(),
            overview: None,
            terrain: (Vec::new(), Vec::new()),
            terrain_version: 0,
            gait: BTreeMap::new(),
            clock: BTreeMap::new(),
            moved_at: BTreeMap::new(),
            focus: 4.0,
            surface: None,
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
        let t = std::time::Instant::now();
        let mut ran = 0;
        for _ in 0..n {
            if self.engine.outcome().is_some() || self.world().tick >= self.engine.rules().cfg.run.max_ticks {
                break;
            }
            ran += 1;
            if self.script.is_some() {
                self.queue_script();
            } else if let Some(e) = self.you() {
                let (fx, fy, fz) = self.feet_voxel();
                let d = [(fx - e.x).signum(), (fy - e.y).signum(), (fz - e.z).signum()];
                if d != [0, 0, 0] {
                    let args: BTreeMap<String, i64> = [("dx", d[0]), ("dy", d[1]), ("dz", d[2])].map(|(k, v)| (k.to_string(), v)).into();
                    let crawl = self.roam.actions.crawl.clone();
                    // Refused while you are in the air (nothing to cling to): the game catches up when you land.
                    let _ = self.act(&crawl, args);
                }
            }
            self.tween.remember(self.engine.world());
            self.engine.tick();
        }
        self.stats.ticks += ran;
        self.stats.ticks_ms += t.elapsed().as_secs_f64() * 1000.0;
    }

    /// Queues an action of yours if the game takes it (and logs it); the refusal otherwise.
    fn act(&mut self, action: &str, args: BTreeMap<String, i64>) -> Result<(), String> {
        let Some(id) = self.you().map(|e| e.id) else { return Err("you are not in the world".into()) };
        let g = self.engine.rules().act(self.world(), None, id, action, &args)?;
        self.engine.queue(g);
        self.log.push(Press { tick: self.world().tick, action: action.to_string(), args });
        Ok(())
    }

    /// Replay: queue the presses logged at this tick, in their order. A press the game refuses now means the
    /// replay has left the recorded run (a different game or engine): said at once, not discovered later.
    fn queue_script(&mut self) {
        let tick = self.world().tick;
        let Some((presses, at)) = self.script.take() else { return };
        let mut at = at;
        while at < presses.len() && presses[at].tick <= tick {
            let p = presses[at].clone();
            if let Err(why) = self.act(&p.action, p.args.clone()) {
                self.refused = Some((format!("replay diverged at tick {tick}: {} refused: {why}", p.action), self.time));
            }
            at += 1;
        }
        self.script = Some((presses, at));
    }

    /// The body moves (every frame, not every tick).
    pub fn walk(&mut self, dt: f32, input: Input) {
        let world = self.engine.world();
        self.walker.step(dt, input, &|x, y, z| solid_for_body(world, [x, y, z]));
    }

    /// What the crosshair points at: (solid voxel, open voxel before it), in world coordinates.
    pub fn target(&self) -> Option<(Voxel, Voxel)> {
        let w = self.world();
        // Only the world's own terrain can be dug or built on: outside it is air to the look ray.
        let hit = ray(self.walker.eye_pos(), self.walker.look_dir(), self.roam.actions.reach, &|x, y, z| solid_voxel(w, [x, y, z], 0))?;
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
        if self.script.is_some() {
            return;
        }
        let Some(e) = self.you() else { return };
        let (to, action) = if dig { (hit, self.roam.actions.dig.clone()) } else { (before, self.roam.actions.drop.clone()) };
        let args: BTreeMap<String, i64> =
            [("dx", to.0 - e.x), ("dy", to.1 - e.y), ("dz", to.2 - e.z)].map(|(k, v)| (k.to_string(), v)).into();
        if let Err(why) = self.act(&action, args) {
            self.refused = Some((format!("{action}: {why}"), self.time));
        }
    }

    pub fn frame(&mut self, w: f32, h: f32, dt: f32) -> Frame {
        let started = std::time::Instant::now();
        let fr = self.build_frame(w, h, dt);
        self.stats.build_ms = started.elapsed().as_secs_f64() * 1000.0;
        // Everything drawn: this frame's meshes, the terrain the GPU keeps, and every model instance's triangles.
        let models: usize =
            fr.models.iter().map(|d| d.instances.len() * self.roam.models.get(&d.model).map_or(0, |m| m.triangles() * 3)).sum();
        self.stats.verts = fr.meshes.iter().chain(&self.terrain.1).map(|m| m.verts.len()).sum::<usize>() + models;
        fr
    }

    fn build_frame(&mut self, w: f32, h: f32, dt: f32) -> Frame {
        self.time += dt;
        let eye = self.overview.unwrap_or_else(|| self.walker.eye());
        // Autofocus: on what the middle of the view looks at (within 40 voxels), easing there.
        {
            let world = self.engine.world();
            let look = (eye.target - eye.pos).norm();
            let hit = ray(eye.pos, look, 40.0, &|x, y, z| solid_voxel(world, [x, y, z], self.ground_top as i64));
            let want = match self.overview {
                // A placed camera (a shot) focuses on what it is aimed at.
                Some(o) => (o.target - o.pos).dot(look).max(0.3),
                None => hit.map_or(40.0, |(v, _)| {
                    let c = V3(v[0] as f32 + 0.5, v[1] as f32 + 0.5, v[2] as f32 + 0.5);
                    (c - eye.pos).dot(look).max(0.3)
                }),
            };
            // The first frame focuses at once (a camera switched on), later ones ease.
            let first = self.time <= dt.max(1.0 / 60.0) * 1.5;
            self.focus = if first { want } else { self.focus + (want - self.focus) * (1.0 - (-6.0 * dt.max(1.0 / 60.0)).exp()) };
        }
        let world = self.engine.world();
        let fog = rgb(self.roam.fog);
        // Terrain: rebuilt only when it changed.
        let tf = self.engine.rules().def.terrain.clone().unwrap_or_default();
        self.stats.terrain_rebuilt = false;
        if world.field_values(&tf).is_some_and(|v| v != self.terrain.0.as_slice()) {
            let t = std::time::Instant::now();
            let meshes = match self.roam.surface {
                Surface::Blocks => terrain_meshes(world, &tf, &self.roam.materials, self.ground_top as i64),
                Surface::Smooth => {
                    let (meshes, map) = smooth_terrain(world, &tf, &self.roam.materials, self.ground_top as i64);
                    self.surface = Some(map);
                    meshes
                }
            };
            self.stats.terrain_ms = t.elapsed().as_secs_f64() * 1000.0;
            self.stats.terrain_rebuilt = true;
            self.stats.terrain_verts = meshes.iter().map(|m| m.verts.len()).sum();
            self.terrain = (world.field_values(&tf).map(<[i64]>::to_vec).unwrap_or_default(), meshes);
            self.terrain_version += 1;
        }
        // The terrain is not in the frame: the GPU keeps it (`terrain()`).
        let mut meshes: Vec<Mesh> = Vec::new();

        // The plain beyond the world's edge, at ground level: the ground goes on to the horizon. Tiles outside the
        // world only (inside, the terrain is the ground), small enough for the fog to fade across them.
        if let Some(m) = self.roam.materials.get(&1).or_else(|| self.roam.materials.values().next()) {
            // A hair below the ground's top: the smooth surface runs half a voxel past the edge, over the plain.
            let (y, tile, reach) = (self.ground_top - 0.01, 8i64, (self.roam.far as i64 / 8 + 1) * 8);
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
            // And on to the horizon: four large quads past the tiles (fully in the fog there).
            let (r0, r1) = (reach as f32, self.roam.far * 20.0);
            let (w0, h0) = (world.width as f32, world.height as f32);
            for (x0, z0, x1, z1) in
                [(-r1, -r1, w0 + r1, -r0), (-r1, h0 + r0, w0 + r1, h0 + r1), (-r1, -r0, -r0, h0 + r0), (w0 + r0, -r0, w0 + r1, h0 + r0)]
            {
                let q = [V3(x0, y, z0), V3(x1, y, z0), V3(x1, y, z1), V3(x0, y, z1)];
                for k in [0usize, 1, 2, 0, 2, 3] {
                    let p = q[k];
                    v.push(Vert3 { pos: [p.0, p.1, p.2], uv: [p.0 * m.scale, p.2 * m.scale], color: c, fog: 1.0 });
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
        // Gaits first (they remember), then every body on all cores. Kinds with a model become instances.
        let mut jobs = Vec::new();
        let mut models: BTreeMap<String, crate::skin::ModelDraw> = BTreeMap::new();
        let mut balls: Vec<(V3, V3, f32)> = Vec::new();
        let (look_dir, far_away) = (eye.basis().2, self.roam.far);
        for e in world.entities().values() {
            let Some(body) = self.roam.kinds.get(&e.kind) else { continue };
            if let Some(spec) = &body.model {
                let prev = self.tween.prev.get(&e.id).copied().unwrap_or((e.x, e.y, e.z));
                let moved = V3((e.x - prev.0) as f32, (prev.2 - e.z) as f32, (e.y - prev.1) as f32);
                let g = self.gait.entry(e.id).or_insert((V3(0.0, 0.0, 1.0), (e.id % 7) as f32));
                if moved.dot(moved) > 0.0 {
                    g.0 = moved.norm();
                    self.moved_at.insert(e.id, self.time);
                }
                // Walking = moved in the last half second (a crawler steps a few times a second, not every tick).
                let moving = self.moved_at.get(&e.id).is_some_and(|t| self.time - t < 0.5);
                let heading = g.0;
                // Every entity has its own clock (offset by id, so a crowd does not step in lockstep).
                let t = self.clock.entry(e.id).or_insert((e.id % 97) as f32 * 0.137);
                *t += dt * spec.rate;
                let time = *t;
                let (x, y, z) = self.tween.at(e.id, (e.x, e.y, e.z));
                let (feet, up) = self.stand(world, x, y, z);
                let to = feet - eye.pos;
                let dist = to.dot(to).sqrt();
                if dist > far_away || (dist > 1.5 && to.dot(look_dir) < -0.2 * dist) {
                    continue;
                }
                let file = spec.lods.iter().rev().find(|(from, _)| dist >= *from).map_or(&spec.file, |(_, f)| f);
                let Some(model) = self.roam.models.get(file) else { continue };
                let frames = crate::skin::Frames::of(model);
                let clip = spec.clip_for(&e.state, moving);
                let ([a, b], blend) = frames.at(&clip, time);
                // The model's axes on the surface: +Z along the heading, +Y along the surface normal, +X to its
                // left (glTF is right-handed, this view left-handed: +X to the left keeps it unmirrored).
                let fwd = (heading - up.scale(heading.dot(up))).norm();
                let fwd = if fwd.dot(fwd) > 0.5 { fwd } else { up.cross(V3(1.0, 0.0, 0.0)).norm() };
                let left = fwd.cross(up).norm();
                let s = spec.length / (model.max[2] - model.min[2]).max(1e-6);
                let (mx, my, mz) = (left.scale(s), up.scale(s), fwd.scale(s));
                let m = [[mx.0, my.0, mz.0, feet.0], [mx.1, my.1, mz.1, feet.1], [mx.2, my.2, mz.2, feet.2]];
                models
                    .entry(file.clone())
                    .or_insert_with(|| crate::skin::ModelDraw { model: file.clone(), instances: Vec::new() })
                    .instances
                    .push(crate::skin::Instance { m, frames: [a, b, 0, 0], params: [blend, 0.0, 0.0, 0.0], tint: [1.0; 4] });
                let carrying =
                    body.carries_in.as_deref().is_some_and(|c| e.props.get(c).is_some_and(|v| *v != 0) || sim_state::in_label(&e.state, c));
                if carrying && let Some(socket) = spec.carry.as_ref().and_then(|n| model.nodes.get(n)) {
                    // Where the socket is in this frame of the clip, then in the world.
                    let (i, j, k) = frames.frame_numbers(&clip, time);
                    let c = model.clips.get(&clip).or_else(|| model.clips.get("rest"));
                    if let Some(c) = c {
                        let (p0, p1) = (c.frames[i][*socket][3], c.frames[j.min(c.frames.len() - 1)][*socket][3]);
                        let p = [p0[0] + (p1[0] - p0[0]) * k, p0[1] + (p1[1] - p0[1]) * k, p0[2] + (p1[2] - p0[2]) * k];
                        let world_p = feet + mx.scale(p[0]) + my.scale(p[1]) + mz.scale(p[2]);
                        balls.push((world_p, up, spec.length * 0.075));
                    }
                }
                continue;
            }
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
        let surface = &self.surface;
        let (look, far) = (eye.basis().2, self.roam.far);
        let parts: Vec<Vec<Vert3>> = jobs
            .par_iter()
            .with_min_len(16)
            .filter_map(|(e, body, g)| {
                let (x, y, z) = tween.at(e.id, (e.x, e.y, e.z));
                let (feet, up) =
                    surface.as_ref().and_then(|m| m.stand(cling_point(world, x, y, z).0)).unwrap_or_else(|| cling_point(world, x, y, z));
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
        self.stats.bodies = parts.len() + models.values().map(|d| d.instances.len()).sum::<usize>();
        bodies.extend(parts.into_iter().flatten());
        // Carried balls, in the built mud's own texture.
        let mut carried = Vec::new();
        for (at, up, r) in balls {
            let side = up.cross(V3(1.0, 0.0, 0.0)).norm();
            ellipsoid(&mut carried, at, [side, up, up.cross(side)], [r; 3], [0.95, 0.9, 0.85]);
        }
        if let Some(m) = self.roam.materials.get(&2) {
            meshes.push(Mesh { image: m.image.clone(), wrap: Wrap::Repeat, verts: carried });
        }
        let models: Vec<crate::skin::ModelDraw> = models.into_values().collect();
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
        back.push(Quad { top: [fog[0], fog[1], fog[2], 1.0], bottom: [fog[0], fog[1], fog[2], 1.0], ..plain(WHITE, 0.0, 0.0, w, h) });
        if let Some(sky) = &self.roam.sky {
            let hgt = (horizon + h * 0.05).max(1.0);
            let u0 = self.walker.view_yaw / 120.0;
            back.push(Quad { uv: [u0, 0.0, u0 + w / h * 0.6, 1.0], wrap: Wrap::MirrorX, ..plain(sky, 0.0, horizon - hgt, w, hgt) });
        }

        // In front: a small crosshair.
        let cross = if refused_now { [1.0, 0.35, 0.3, 0.9] } else { [1.0, 1.0, 1.0, 0.75] };
        let front = vec![plain_c(BLOB, w / 2.0 - 4.0, h / 2.0 - 4.0, 8.0, 8.0, cross)];
        Frame { back, eye, fog, meshes, models, front }
    }

    /// Forgets the built terrain: the next frame rebuilds and re-uploads it (what a dig or a drop causes).
    pub fn forget_terrain(&mut self) {
        self.terrain.0.clear();
    }

    /// Where a crawler at (x, y, z) stands: on the drawn surface (smooth terrain), else on its voxel's face.
    fn stand(&self, world: &World, x: f32, y: f32, z: f32) -> (V3, V3) {
        let on_face = cling_point(world, x, y, z);
        self.surface.as_ref().and_then(|m| m.stand(on_face.0)).unwrap_or(on_face)
    }

    /// The terrain meshes and their version (bumped at each rebuild), for `Gpu::keep`.
    pub fn terrain(&self) -> (u64, &[Mesh]) {
        (self.terrain_version, &self.terrain.1)
    }

    /// Draws a frame: the terrain kept on the GPU (uploaded only when it changed), fog by distance from the eye.
    pub fn render(&self, gpu: &mut crate::gpu::Gpu, target: &wgpu::TextureView, w: u32, h: u32, fr: &Frame) {
        let world = self.world3(gpu, fr, w, h);
        gpu.render(target, w, h, &fr.back, Some(world), &fr.front);
    }

    /// The 3D part of a frame, for `Gpu::render` or `Gpu::shot_scene`.
    pub fn world3<'a>(&self, gpu: &mut crate::gpu::Gpu, fr: &'a Frame, w: u32, h: u32) -> crate::gpu::World3<'a> {
        const TERRAIN: &[u32] = &[0];
        gpu.keep(0, self.terrain_version, &self.terrain.1);
        // Models go to the GPU the first time they are drawn (with their texture overrides).
        for d in &fr.models {
            if gpu.skin.as_ref().is_some_and(|s| s.has(&d.model)) {
                continue;
            }
            if let Some(m) = self.roam.models.get(&d.model) {
                let spec = self.roam.kinds.values().filter_map(|b| b.model.as_ref()).find(|s| s.files().any(|f| *f == d.model));
                let overrides: BTreeMap<String, crate::model::Texture> = spec
                    .map(|s| s.textures.iter().filter_map(|(mat, img)| Some((mat.clone(), self.roam.images.get(img)?.clone()))).collect())
                    .unwrap_or_default();
                gpu.upload_model(&d.model, m, &overrides);
            }
        }
        let mut world = crate::gpu::World3::new(fr.eye.view_proj(w as f32, h as f32), fr.fog, &fr.meshes);
        world.eye = [fr.eye.pos.0, fr.eye.pos.1, fr.eye.pos.2];
        world.fog_range = self.fog_range();
        world.kept = TERRAIN;
        world.models = &fr.models;
        world.light.sun_dir = [0.35, 0.85, 0.4];
        if let Some(l) = self.roam.lens {
            let look = (fr.eye.target - fr.eye.pos).norm();
            let c = fr.eye.pos + V3(look.0, 0.0, look.2).scale(8.0);
            if l.shadow > 0.0 {
                world.shadow = Some(crate::gpu::Shadow { center: [c.0, self.ground_top, c.2], radius: 22.0, strength: l.shadow });
            }
            if l.aperture > 0.0 {
                world.lens = Some(crate::gpu::Lens { focus: self.focus, aperture: l.aperture, near: fr.eye.near, far: fr.eye.far });
            }
            world.haze = l.haze;
            world.light.haze = l.haze;
        }
        world
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

/// A close look at a crawler of `kind` on flat ground (for `--shot --closeup`): the camera a body length away at
/// its height, looking at it from the front-left, as a macro lens would.
pub fn close_up(p: &mut RoamPlay, kind: &str) {
    let w = p.world();
    let pick =
        w.of_kind(kind).find(|e| w.is_terrain(e.x, e.y, e.z + 1) && !w.is_terrain(e.x + 1, e.y, e.z) && !w.is_terrain(e.x, e.y + 1, e.z));
    let Some(e) = pick else { return };
    let (feet, _) = cling_point(w, e.x as f32, e.y as f32, e.z as f32);
    let target = feet + V3(0.0, 0.18, 0.0);
    p.overview = Some(crate::math::Eye { pos: target + V3(-0.75, 0.35, -0.95), target, roll: 0.0, fov: 38.0, near: 0.02, far: 200.0 });
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
        let meshes = terrain_meshes(&w, "mud", &mats, 1);
        let faces = |img: &str| meshes.iter().find(|m| m.image == img).map_or(0, |m| m.verts.len() / 6);
        // Earth: 16 tops (one under the ball), 16 bottoms; no outer sides: the ground continues beside the world.
        assert_eq!(faces("earth"), 15 + 16);
        // The ball: its top and four sides; not its bottom (earth under it).
        assert_eq!(faces("mud"), 5);
        let ao = meshes.iter().find(|m| m.image == "earth").unwrap().verts.iter().map(|v| v.color[0]).fold(1.0f32, f32::min);
        assert!(ao < 0.6, "corners beside the ball are darker");
    }

    #[test]
    fn a_smooth_surface_keeps_flat_ground_on_the_voxel_faces_and_rounds_a_pillar() {
        // Ground one level deep on a 10x10 world, a ball at (1, 1): the ground's top is y = 1 (the bottom level is
        // view y 0..1), and far from the ball it stays exactly there.
        let mut w = World::new3(1, 10, 10, 4);
        w.add_field("mud", 0);
        w.set_terrain(Some("mud".into()));
        for y in 0..10 {
            for x in 0..10 {
                w.set_field("mud", x, y, 3, 1);
            }
        }
        w.set_field("mud", 1, 1, 2, 2);
        let mats: BTreeMap<i64, Material> =
            [(1, "earth"), (2, "mud")].map(|(k, n)| (k, Material { image: n.into(), scale: 1.0, tint: (255, 255, 255) })).into();
        let meshes = smooth_terrain_meshes(&w, "mud", &mats, 1);
        let earth = meshes.iter().find(|m| m.image == "earth").expect("earth");
        let tops: Vec<f32> =
            earth.verts.iter().filter(|v| (v.pos[0] - 6.5).abs() < 1.6 && (v.pos[2] - 6.5).abs() < 1.6).map(|v| v.pos[1]).collect();
        assert!(!tops.is_empty() && tops.iter().all(|y| (y - 1.0).abs() < 1e-4), "flat ground on the face: {tops:?}");
        // A pillar three balls high: its side is rounded (vertices between the voxel's corners, not on them).
        for z in 0..3 {
            w.set_field("mud", 5, 5, z, 2);
        }
        let meshes = smooth_terrain_meshes(&w, "mud", &mats, 1);
        let mud = meshes.iter().find(|m| m.image == "mud").expect("the pillar");
        let mid = mud.verts.iter().filter(|v| v.pos[1] > 2.0 && v.pos[1] < 3.0);
        let radii: Vec<f32> = mid.map(|v| ((v.pos[0] - 5.5).powi(2) + (v.pos[2] - 5.5).powi(2)).sqrt()).collect();
        let (lo, hi) = radii.iter().fold((f32::MAX, 0.0f32), |(a, b), r| (a.min(*r), b.max(*r)));
        assert!(!radii.is_empty() && hi < 0.7 && hi - lo < 0.2, "a round column, not a square one: radii {lo}..{hi}");
    }

    #[test]
    fn crawlers_stand_on_the_drawn_surface_and_tilt_with_its_curve() {
        let mut w = World::new3(1, 10, 10, 5);
        w.add_field("mud", 0);
        w.set_terrain(Some("mud".into()));
        for y in 0..10 {
            for x in 0..10 {
                w.set_field("mud", x, y, 4, 1);
            }
        }
        for z in 1..4 {
            w.set_field("mud", 5, 5, z, 2);
        }
        let mats: BTreeMap<i64, Material> =
            [(1, "earth"), (2, "mud")].map(|(k, n)| (k, Material { image: n.into(), scale: 1.0, tint: (255, 255, 255) })).into();
        let (_, map) = smooth_terrain(&w, "mud", &mats, 1);
        // Flat ground, far from the pillar: exactly the voxel face, facing up.
        let (p, n) = map.stand(V3(1.5, 1.0, 1.5)).expect("ground");
        assert!((p.1 - 1.0).abs() < 1e-4 && (n.1 - 1.0).abs() < 1e-4, "{p:?} {n:?}");
        // On the pillar's top edge (the voxel face's corner): the drawn surface is rounded there.
        let (p, n) = map.stand(V3(5.95, 4.0, 5.5)).expect("the pillar's edge");
        assert!(n.1 < 0.98 && n.0 > 0.1, "tilted outwards over the rounded edge: {n:?}");
        assert!((p - V3(5.95, 4.0, 5.5)).dot(p - V3(5.95, 4.0, 5.5)).sqrt() < 0.5, "close to where it clings: {p:?}");
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
