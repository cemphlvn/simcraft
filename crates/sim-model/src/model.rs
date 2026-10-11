//! Models from the best tools: glTF 2.0 (`.glb`), the format Blender, Unity, Unreal and Higgsfield's 3D generators
//! all write. A model is loaded once and made ready for crowds: every clip is sampled once into matrices (a palette
//! per frame), so a character on screen costs one small record (where it is, which frame), not a mesh.
//!
//! Conventions are glTF's: +Y up, +Z forward (the spec's "front of an asset"), metres or any unit (`length` in the
//! view scales it). Skinned meshes and rigid parts (meshes on animated nodes) both play: every node of the scene has
//! a palette entry, and skin joints have one more with their inverse bind matrix.
//!
//! A model can also be built in code (`Model::build`: nodes and parts, no file) and posed live by its owner: the rig
//! stays on the CPU (`Model::rig`), and `Model::pose_into` writes a palette for any set of node transforms.

use std::collections::BTreeMap;
use std::path::Path;

/// A column-major 4x4 matrix (glTF's layout).
pub type Mat4 = [[f32; 4]; 4];

/// The 4x4 identity matrix.
pub const IDENTITY: Mat4 = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

/// The matrix product `a * b` (applies `b` first).
pub fn mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut m = [[0.0f32; 4]; 4];
    for (c, col) in m.iter_mut().enumerate() {
        for (r, v) in col.iter_mut().enumerate() {
            *v = (0..4).map(|k| a[k][r] * b[c][k]).sum();
        }
    }
    m
}

/// Translation, rotation (quaternion x, y, z, w), scale → matrix.
pub fn trs(t: [f32; 3], q: [f32; 4], s: [f32; 3]) -> Mat4 {
    let [x, y, z, w] = q;
    let (xx, yy, zz, xy, xz, yz, wx, wy, wz) = (x * x, y * y, z * z, x * y, x * z, y * z, w * x, w * y, w * z);
    [
        [(1.0 - 2.0 * (yy + zz)) * s[0], 2.0 * (xy + wz) * s[0], 2.0 * (xz - wy) * s[0], 0.0],
        [2.0 * (xy - wz) * s[1], (1.0 - 2.0 * (xx + zz)) * s[1], 2.0 * (yz + wx) * s[1], 0.0],
        [2.0 * (xz + wy) * s[2], 2.0 * (yz - wx) * s[2], (1.0 - 2.0 * (xx + yy)) * s[2], 0.0],
        [t[0], t[1], t[2], 1.0],
    ]
}

/// `m` applied to the point `p` (w = 1).
pub fn transform_point(m: &Mat4, p: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|r| m[0][r] * p[0] + m[1][r] * p[1] + m[2][r] * p[2] + m[3][r])
}

/// A vertex as the GPU skins it: `joints` index the palette, `weights` sum to 1.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ModelVert {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub joints: [u32; 4],
    pub weights: [f32; 4],
}

impl ModelVert {
    /// A vertex that moves with one palette slot only (a rigid part on a node: slot = node index).
    pub fn rigid(pos: [f32; 3], normal: [f32; 3], uv: [f32; 2], slot: u32) -> ModelVert {
        ModelVert { pos, normal, uv, joints: [slot, 0, 0, 0], weights: [1.0, 0.0, 0.0, 0.0] }
    }
}

/// A texture of a model (RGBA8, straight alpha).
#[derive(Clone, Debug)]
pub struct Texture {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

/// PBR metallic-roughness, as glTF has it (texture indices into `Model::textures`).
#[derive(Clone, Debug)]
pub struct Material {
    pub name: String,
    pub base_color: [f32; 4],
    pub base_tex: Option<usize>,
    pub normal_tex: Option<usize>,
    /// G = roughness, B = metallic (glTF).
    pub mr_tex: Option<usize>,
    /// R = ambient occlusion (often the same image as `mr_tex`: an "ORM" map).
    pub occlusion_tex: Option<usize>,
    pub metallic: f32,
    pub roughness: f32,
}

/// Triangles with one material.
#[derive(Clone, Debug)]
pub struct Part {
    pub verts: Vec<ModelVert>,
    pub indices: Vec<u32>,
    pub material: usize,
}

/// A clip sampled at `fps`: `frames[f]` is the palette (one matrix per palette slot) at time f / fps.
#[derive(Clone, Debug)]
pub struct Clip {
    pub fps: f32,
    pub duration: f32,
    pub frames: Vec<Vec<Mat4>>,
}

/// A node of a model built in code: its name (a socket, a key to pose it by), its parent (an index into the same
/// list; `None` = a root) and its rest transform relative to the parent.
#[derive(Clone, Debug)]
pub struct Node {
    pub name: String,
    pub parent: Option<usize>,
    pub rest: Mat4,
}

/// The skeleton of a model, kept on the CPU so it can be posed live. Parents always come before their children in
/// the evaluation order (no cycles: `Rig::new` refuses them), every joint is a node, and every joint has its inverse
/// bind matrix. Palette layout: slot i < `nodes()` is node i's global transform; slot `nodes() + k` is joint k's
/// global transform times its inverse bind matrix.
#[derive(Clone, Debug)]
pub struct Rig {
    parents: Vec<Option<usize>>,
    rest: Vec<Mat4>,
    joints: Vec<usize>,
    inverse_binds: Vec<Mat4>,
    order: Vec<usize>,
}

impl Rig {
    /// A rig from node parents, rest transforms (local), and skin joints (node indices) with their inverse binds.
    /// Refused, with what is wrong: lists of different lengths, a parent or joint that is not a node, a cycle.
    pub fn new(parents: Vec<Option<usize>>, rest: Vec<Mat4>, joints: Vec<usize>, inverse_binds: Vec<Mat4>) -> Result<Rig, String> {
        let n = parents.len();
        if rest.len() != n {
            return Err(format!("{n} parents but {} rest transforms", rest.len()));
        }
        if inverse_binds.len() != joints.len() {
            return Err(format!("{} joints but {} inverse bind matrices", joints.len(), inverse_binds.len()));
        }
        if let Some(j) = joints.iter().find(|&&j| j >= n) {
            return Err(format!("joint {j} is not a node ({n} nodes)"));
        }
        let mut depth = vec![0usize; n];
        for (i, d) in depth.iter_mut().enumerate() {
            let mut at = i;
            while let Some(p) = parents[at] {
                if p >= n {
                    return Err(format!("node {at}: parent {p} is not a node ({n} nodes)"));
                }
                *d += 1;
                if *d > n {
                    return Err(format!("node {i}: its parents form a cycle"));
                }
                at = p;
            }
        }
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by_key(|&i| depth[i]);
        Ok(Rig { parents, rest, joints, inverse_binds, order })
    }

    /// How many nodes (the first palette slots).
    pub fn nodes(&self) -> usize {
        self.parents.len()
    }

    /// Palette slots: every node, then every joint.
    pub fn slots(&self) -> usize {
        self.parents.len() + self.joints.len()
    }

    /// Each node's parent (`None` = a root).
    pub fn parents(&self) -> &[Option<usize>] {
        &self.parents
    }

    /// Each node's rest transform, relative to its parent.
    pub fn rest(&self) -> &[Mat4] {
        &self.rest
    }

    /// The skin's joints (node indices); joint k has palette slot `nodes() + k`.
    pub fn joints(&self) -> &[usize] {
        &self.joints
    }

    /// Joint k's inverse bind matrix.
    pub fn inverse_binds(&self) -> &[Mat4] {
        &self.inverse_binds
    }

    /// Writes into `out` (resized to `slots()`; no allocation once it is that big) the palette of the rest pose with
    /// some nodes' local transforms replaced: `locals` holds (node index, transform relative to its parent). Children
    /// follow their parents and skins follow their joints. Entries for nodes that do not exist are ignored; a node
    /// listed twice takes the last.
    pub fn pose_into(&self, locals: &[(usize, Mat4)], out: &mut Vec<Mat4>) {
        out.clear();
        out.extend_from_slice(&self.rest);
        out.resize(self.slots(), IDENTITY);
        for &(node, m) in locals {
            if node < self.nodes() {
                out[node] = m;
            }
        }
        self.globalise(out);
    }

    /// The palette of the rest pose.
    pub fn rest_palette(&self) -> Vec<Mat4> {
        let mut p = Vec::new();
        self.pose_into(&[], &mut p);
        p
    }

    /// `palette[..nodes()]` holds local transforms; turns them into globals (parents first) and fills the joints.
    fn globalise(&self, palette: &mut [Mat4]) {
        for &i in &self.order {
            if let Some(p) = self.parents[i] {
                palette[i] = mul(&palette[p], &palette[i]);
            }
        }
        let n = self.nodes();
        for (k, (&j, ibm)) in self.joints.iter().zip(&self.inverse_binds).enumerate() {
            palette[n + k] = mul(&palette[j], ibm);
        }
    }
}

/// A model ready to upload: its mesh, materials and textures, its rig and its clips baked to palettes.
#[derive(Clone, Debug)]
pub struct Model {
    pub parts: Vec<Part>,
    pub materials: Vec<Material>,
    pub textures: Vec<Texture>,
    /// Palette slots: every node (by index), then every skin joint again with its inverse bind matrix.
    pub slots: usize,
    /// The skeleton on the CPU, for live poses (`slots == rig.slots()`).
    pub rig: Rig,
    /// Node name → node index (sockets: where a carried thing goes).
    pub nodes: BTreeMap<String, usize>,
    /// Clip name → sampled palettes. `rest` is always there (the pose as modelled).
    pub clips: BTreeMap<String, Clip>,
    /// In the rest pose (model units).
    pub min: [f32; 3],
    pub max: [f32; 3],
}

/// Clips are sampled this often (frames per second of animation time); between frames the GPU blends.
pub const SAMPLE_FPS: f32 = 30.0;

struct Channel {
    node: usize,
    times: Vec<f32>,
    /// 3 (translation, scale) or 4 (rotation) values per key; cubic splines keep only the value of each triple.
    values: Vec<f32>,
    width: usize,
    kind: u8,
    step: bool,
}

fn sample(ch: &Channel, t: f32) -> Vec<f32> {
    let w = ch.width;
    let n = ch.times.len();
    let at = |i: usize| ch.values[i * w..i * w + w].to_vec();
    if n == 0 {
        return vec![0.0; w];
    }
    if t <= ch.times[0] {
        return at(0);
    }
    if t >= ch.times[n - 1] {
        return at(n - 1);
    }
    let i = ch.times.partition_point(|&x| x <= t) - 1;
    if ch.step {
        return at(i);
    }
    let (t0, t1) = (ch.times[i], ch.times[i + 1]);
    let k = if t1 > t0 { (t - t0) / (t1 - t0) } else { 0.0 };
    let (a, mut b) = (at(i), at(i + 1));
    if ch.kind == 1 {
        // Rotations: the shorter way round, then normalised linear interpolation (close to slerp at 30 fps).
        if a.iter().zip(&b).map(|(x, y)| x * y).sum::<f32>() < 0.0 {
            b.iter_mut().for_each(|v| *v = -*v);
        }
        let mut q: Vec<f32> = a.iter().zip(&b).map(|(x, y)| x + (y - x) * k).collect();
        let len = q.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-9);
        q.iter_mut().for_each(|v| *v /= len);
        return q;
    }
    a.iter().zip(&b).map(|(x, y)| x + (y - x) * k).collect()
}

/// Where the vertices are in a palette's pose: (min, max), model units.
fn bounds(parts: &[Part], palette: &[Mat4]) -> ([f32; 3], [f32; 3]) {
    let (mut min, mut max) = ([f32::MAX; 3], [f32::MIN; 3]);
    for p in parts {
        for v in &p.verts {
            let mut q = [0.0f32; 3];
            for k in 0..4 {
                if v.weights[k] > 0.0 {
                    let t = transform_point(&palette[v.joints[k] as usize], v.pos);
                    (0..3).for_each(|a| q[a] += t[a] * v.weights[k]);
                }
            }
            (0..3).for_each(|a| {
                min[a] = min[a].min(q[a]);
                max[a] = max[a].max(q[a]);
            });
        }
    }
    (min, max)
}

impl Model {
    /// Loads a glTF 2.0 file (`.glb` or `.gltf`): its triangle meshes, PBR materials and textures, its node
    /// hierarchy and first skin, and every animation sampled at `SAMPLE_FPS`.
    pub fn load(path: &Path) -> Result<Model, String> {
        let (doc, buffers, images) = gltf::import(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let data = |b: gltf::Buffer| Some(&*buffers[b.index()]);

        // Nodes: rest transforms and parents.
        let n = doc.nodes().len();
        let mut rest = vec![([0.0f32; 3], [0.0f32, 0.0, 0.0, 1.0], [1.0f32; 3]); n];
        let mut parent = vec![None; n];
        let mut nodes = BTreeMap::new();
        for node in doc.nodes() {
            let (t, r, s) = node.transform().decomposed();
            rest[node.index()] = (t, r, s);
            for c in node.children() {
                parent[c.index()] = Some(node.index());
            }
            if let Some(name) = node.name() {
                nodes.entry(name.to_string()).or_insert_with(|| node.index());
            }
        }
        // Skin joints get palette slots after the nodes: slot n + k is joint k of the (first) skin.
        let skin = doc.skins().next();
        let skin_joints: Vec<usize> = skin.as_ref().map(|s| s.joints().map(|j| j.index()).collect()).unwrap_or_default();
        let ibms: Vec<Mat4> = match &skin {
            Some(s) => {
                let read: Vec<Mat4> = s.reader(data).read_inverse_bind_matrices().map(|m| m.collect()).unwrap_or_default();
                if read.is_empty() { vec![IDENTITY; skin_joints.len()] } else { read }
            }
            None => Vec::new(),
        };
        let rest_locals = rest.iter().map(|&(t, r, s)| trs(t, r, s)).collect();
        let rig = Rig::new(parent, rest_locals, skin_joints, ibms).map_err(|e| format!("{}: {e}", path.display()))?;
        let slots = rig.slots();

        // Textures (RGBA8).
        let textures: Vec<Texture> = images
            .iter()
            .map(|img| {
                use gltf::image::Format;
                let px = img.pixels.len() / (img.width * img.height).max(1) as usize;
                let rgba = match img.format {
                    Format::R8G8B8A8 => img.pixels.clone(),
                    Format::R8G8B8 => img.pixels.as_chunks::<3>().0.iter().flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
                    Format::R8G8 => img.pixels.as_chunks::<2>().0.iter().flat_map(|c| [c[0], c[1], 0, 255]).collect(),
                    Format::R8 => img.pixels.iter().flat_map(|&v| [v, v, v, 255]).collect(),
                    // 16-bit and float images: keep the high byte of each channel.
                    _ => img.pixels.chunks_exact(px.max(1)).flat_map(|c| [c[1 % c.len()], c[3 % c.len()], c[5 % c.len()], 255]).collect(),
                };
                Texture { w: img.width, h: img.height, rgba }
            })
            .collect();
        let tex_of = |t: Option<gltf::texture::Texture>| t.map(|t| t.source().index());
        let mut materials: Vec<Material> = doc
            .materials()
            .map(|m| {
                let pbr = m.pbr_metallic_roughness();
                Material {
                    name: m.name().unwrap_or("").to_string(),
                    base_color: pbr.base_color_factor(),
                    base_tex: tex_of(pbr.base_color_texture().map(|i| i.texture())),
                    normal_tex: tex_of(m.normal_texture().map(|i| i.texture())),
                    mr_tex: tex_of(pbr.metallic_roughness_texture().map(|i| i.texture())),
                    occlusion_tex: tex_of(m.occlusion_texture().map(|i| i.texture())),
                    metallic: pbr.metallic_factor(),
                    roughness: pbr.roughness_factor(),
                }
            })
            .collect();
        let default_material = materials.len();
        materials.push(Material {
            name: "default".into(),
            base_color: [0.8, 0.8, 0.8, 1.0],
            base_tex: None,
            normal_tex: None,
            mr_tex: None,
            occlusion_tex: None,
            metallic: 0.0,
            roughness: 0.6,
        });

        // Meshes: skinned primitives use their skin's joints; rigid ones weigh fully on their node's slot.
        let mut parts = Vec::new();
        for node in doc.nodes() {
            let Some(mesh) = node.mesh() else { continue };
            let skinned = node.skin().is_some();
            for prim in mesh.primitives() {
                if prim.mode() != gltf::mesh::Mode::Triangles {
                    continue;
                }
                let r = prim.reader(data);
                let Some(pos) = r.read_positions() else { continue };
                let pos: Vec<[f32; 3]> = pos.collect();
                let normals: Vec<[f32; 3]> = r.read_normals().map(|x| x.collect()).unwrap_or_else(|| vec![[0.0, 1.0, 0.0]; pos.len()]);
                let uvs: Vec<[f32; 2]> = r.read_tex_coords(0).map(|x| x.into_f32().collect()).unwrap_or_else(|| vec![[0.0; 2]; pos.len()]);
                let joints: Vec<[u16; 4]> = r.read_joints(0).map(|x| x.into_u16().collect()).unwrap_or_default();
                let weights: Vec<[f32; 4]> = r.read_weights(0).map(|x| x.into_f32().collect()).unwrap_or_default();
                let verts = (0..pos.len())
                    .map(|i| {
                        let (joints, weights) = if skinned && i < joints.len() && i < weights.len() {
                            let w = weights[i];
                            let sum = (w[0] + w[1] + w[2] + w[3]).max(1e-6);
                            (joints[i].map(|j| (n + j as usize).min(slots - 1) as u32), w.map(|x| x / sum))
                        } else {
                            ([node.index() as u32, 0, 0, 0], [1.0, 0.0, 0.0, 0.0])
                        };
                        ModelVert { pos: pos[i], normal: normals[i], uv: uvs[i], joints, weights }
                    })
                    .collect();
                let indices: Vec<u32> = r.read_indices().map(|x| x.into_u32().collect()).unwrap_or_else(|| (0..pos.len() as u32).collect());
                parts.push(Part { verts, indices, material: prim.material().index().unwrap_or(default_material) });
            }
        }
        if parts.is_empty() {
            return Err(format!("{}: no triangle meshes", path.display()));
        }

        let rest_palette = rig.rest_palette();
        let (min, max) = bounds(&parts, &rest_palette);

        // Clips: sampled once.
        let mut clips = BTreeMap::new();
        clips.insert("rest".to_string(), Clip { fps: SAMPLE_FPS, duration: 0.0, frames: vec![rest_palette] });
        for (ai, anim) in doc.animations().enumerate() {
            let mut chans = Vec::new();
            for ch in anim.channels() {
                let r = ch.reader(data);
                let Some(times) = r.read_inputs() else { continue };
                let times: Vec<f32> = times.collect();
                use gltf::animation::util::ReadOutputs;
                let (values, width, kind): (Vec<f32>, usize, u8) = match r.read_outputs() {
                    Some(ReadOutputs::Translations(it)) => (it.flatten().collect(), 3, 0),
                    Some(ReadOutputs::Rotations(it)) => (it.into_f32().flatten().collect(), 4, 1),
                    Some(ReadOutputs::Scales(it)) => (it.flatten().collect(), 3, 2),
                    _ => continue, // morph weights: not played yet
                };
                let interp = ch.sampler().interpolation();
                let values = if interp == gltf::animation::Interpolation::CubicSpline {
                    // (in-tangent, value, out-tangent) per key: keep the values.
                    values.chunks_exact(width * 3).flat_map(|c| c[width..2 * width].to_vec()).collect()
                } else {
                    values
                };
                chans.push(Channel {
                    node: ch.target().node().index(),
                    times,
                    values,
                    width,
                    kind,
                    step: interp == gltf::animation::Interpolation::Step,
                });
            }
            let duration = chans.iter().filter_map(|c| c.times.last().copied()).fold(0.0f32, f32::max);
            let count = (duration * SAMPLE_FPS).round() as usize + 1;
            let frames = (0..count)
                .map(|f| {
                    let t = f as f32 / SAMPLE_FPS;
                    let mut local = rest.clone();
                    for c in &chans {
                        let v = sample(c, t);
                        match c.kind {
                            0 => local[c.node].0 = [v[0], v[1], v[2]],
                            1 => local[c.node].1 = [v[0], v[1], v[2], v[3]],
                            _ => local[c.node].2 = [v[0], v[1], v[2]],
                        }
                    }
                    let mut palette: Vec<Mat4> = local.iter().map(|&(t, r, s)| trs(t, r, s)).collect();
                    palette.resize(slots, IDENTITY);
                    rig.globalise(&mut palette);
                    palette
                })
                .collect();
            let name = anim.name().map_or_else(|| format!("clip{ai}"), str::to_string);
            clips.insert(name, Clip { fps: SAMPLE_FPS, duration, frames });
        }
        Ok(Model { parts, materials, textures, slots, rig, nodes, clips, min, max })
    }

    /// A model built in code, without a file: a node hierarchy and parts whose vertices weigh on node slots (slot i =
    /// node i; `ModelVert::rigid`). Its only clip is `rest`. Refused, with what is wrong: no parts, a parent that is
    /// not a node, a cycle, two nodes with one name, a vertex on a missing slot, an index past the part's vertices,
    /// a missing material or texture, a texture whose pixels do not match its size.
    pub fn build(nodes: &[Node], parts: Vec<Part>, materials: Vec<Material>, textures: Vec<Texture>) -> Result<Model, String> {
        if parts.is_empty() {
            return Err("no parts".into());
        }
        let mut names = BTreeMap::new();
        for (i, node) in nodes.iter().enumerate() {
            if let Some(first) = names.insert(node.name.clone(), i) {
                return Err(format!("nodes {first} and {i} are both named '{}'", node.name));
            }
        }
        let rig = Rig::new(nodes.iter().map(|n| n.parent).collect(), nodes.iter().map(|n| n.rest).collect(), Vec::new(), Vec::new())?;
        let slots = rig.slots();
        for (pi, part) in parts.iter().enumerate() {
            if part.material >= materials.len() {
                return Err(format!("part {pi}: material {} is missing ({} materials)", part.material, materials.len()));
            }
            if let Some(v) = part.verts.iter().find(|v| v.joints.iter().any(|&j| j as usize >= slots)) {
                return Err(format!("part {pi}: a vertex weighs on slot {:?}, the model has {slots} slots", v.joints));
            }
            if let Some(i) = part.indices.iter().find(|&&i| i as usize >= part.verts.len()) {
                return Err(format!("part {pi}: index {i} past its {} vertices", part.verts.len()));
            }
        }
        for m in &materials {
            for t in [m.base_tex, m.normal_tex, m.mr_tex, m.occlusion_tex].into_iter().flatten() {
                if t >= textures.len() {
                    return Err(format!("material '{}': texture {t} is missing ({} textures)", m.name, textures.len()));
                }
            }
        }
        for (i, t) in textures.iter().enumerate() {
            if t.w == 0 || t.h == 0 || t.rgba.len() != (t.w * t.h * 4) as usize {
                return Err(format!("texture {i}: {} bytes for {}x{} RGBA8", t.rgba.len(), t.w, t.h));
            }
        }
        let rest_palette = rig.rest_palette();
        let (min, max) = bounds(&parts, &rest_palette);
        let clips = BTreeMap::from([("rest".to_string(), Clip { fps: SAMPLE_FPS, duration: 0.0, frames: vec![rest_palette] })]);
        Ok(Model { parts, materials, textures, slots, rig, nodes: names, clips, min, max })
    }

    /// Writes the palette of the rest pose with some nodes' local transforms replaced (see `Rig::pose_into`).
    pub fn pose_into(&self, locals: &[(usize, Mat4)], out: &mut Vec<Mat4>) {
        self.rig.pose_into(locals, out);
    }

    /// How many triangles, all parts.
    pub fn triangles(&self) -> usize {
        self.parts.iter().map(|p| p.indices.len() / 3).sum()
    }

    /// Where a node is in the rest pose (a socket), model units.
    pub fn node_rest(&self, name: &str) -> Option<[f32; 3]> {
        let i = *self.nodes.get(name)?;
        let m = &self.clips.get("rest")?.frames[0][i];
        Some([m[3][0], m[3][1], m[3][2]])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn termite() -> Model {
        Model::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/models/termite.glb")).expect("loads")
    }

    #[test]
    fn a_rigged_model_loads_with_its_clips_sockets_and_textures() {
        let m = termite();
        assert!(m.triangles() > 10_000, "{} triangles", m.triangles());
        for clip in ["rest", "walk", "carry", "dig", "idle"] {
            assert!(m.clips.contains_key(clip), "clip {clip}: {:?}", m.clips.keys().collect::<Vec<_>>());
        }
        let walk = &m.clips["walk"];
        assert!((walk.duration - 1.0).abs() < 0.05 && walk.frames.len() >= 30, "a 1 s cycle sampled at 30 fps");
        assert!(walk.frames.iter().all(|f| f.len() == m.slots), "a full palette every frame");
        assert!(m.node_rest("carry").is_some(), "the carry socket");
        let base = m.materials.iter().find_map(|x| x.base_tex).expect("a base colour texture");
        assert_eq!((m.textures[base].w, m.textures[base].h), (1024, 1024));
        // +Z forward, +Y up: longest along Z, feet at y = 0.
        let size: Vec<f32> = (0..3).map(|a| m.max[a] - m.min[a]).collect();
        assert!(size[2] > size[0] && size[2] > size[1] && m.min[1].abs() < 0.05, "{size:?}, min {:?}", m.min);
    }

    fn fnv(h: &mut u64, bytes: &[u8]) {
        for &b in bytes {
            *h ^= u64::from(b);
            *h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }

    /// The loader as it was before the rig moved to the CPU (sim-gpu, 57897be): same triangles, nodes and palettes,
    /// bit for bit. FNV-1a over every part's vertex then index bytes, and over every clip's palettes in name order.
    #[test]
    fn the_loader_gives_the_same_meshes_and_palettes_as_before() {
        let expect = [
            ("termite.glb", 11999, 47, 25, 0x4e86_7866_2d3d_58a6u64, 0x3519_a1cc_27bb_7b0bu64),
            ("termite_lod1.glb", 3000, 47, 25, 0xf410_f826_c05f_c364, 0xcc9f_d097_ba16_8053),
            ("termite_lod2.glb", 800, 39, 21, 0xcf33_7478_a045_0904, 0xb287_f82d_4298_ad93),
        ];
        for (file, triangles, slots, nodes, mesh_hash, palette_hash) in expect {
            let m = Model::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/models").join(file)).expect("loads");
            let (mut mesh, mut pal) = (0xcbf2_9ce4_8422_2325u64, 0xcbf2_9ce4_8422_2325u64);
            for p in &m.parts {
                fnv(&mut mesh, bytemuck::cast_slice(&p.verts));
                fnv(&mut mesh, bytemuck::cast_slice(&p.indices));
            }
            m.clips.values().flat_map(|c| &c.frames).for_each(|f| fnv(&mut pal, bytemuck::cast_slice(f)));
            assert_eq!((m.triangles(), m.slots, m.nodes.len(), m.parts.len()), (triangles, slots, nodes, 1), "{file}");
            assert_eq!(m.clips.keys().map(String::as_str).collect::<Vec<_>>(), ["carry", "dig", "idle", "rest", "walk"], "{file}");
            assert_eq!((mesh, pal), (mesh_hash, palette_hash), "{file}: mesh {mesh:#x}, palettes {pal:#x}");
        }
    }

    #[test]
    fn a_live_pose_of_nothing_is_the_rest_pose_and_a_posed_joint_carries_its_skin() {
        let m = termite();
        let mut out = Vec::new();
        m.pose_into(&[], &mut out);
        assert_eq!(out, m.clips["rest"].frames[0], "bit for bit");
        assert_eq!(m.rig.slots(), m.slots);
        // Lift the front leg: its node and the skin slot of its joint both move; the head does not.
        let leg = m.nodes["leg_front_l_upper"];
        let k = m.rig.joints().iter().position(|&j| j == leg).expect("the leg is a joint");
        let mut lifted = m.rig.rest()[leg];
        lifted[3][1] += 0.5;
        m.pose_into(&[(leg, lifted)], &mut out);
        let rest = &m.clips["rest"].frames[0];
        let shift = |slot: usize| -> [f32; 3] { std::array::from_fn(|a| out[slot][3][a] - rest[slot][3][a]) };
        let (node, skin) = (shift(leg), shift(m.rig.nodes() + k));
        let length = node.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!(length > 0.01 && (0..3).all(|a| (node[a] - skin[a]).abs() < 1e-4), "node {node:?} skin {skin:?}");
        assert_eq!(out[m.nodes["head"]], rest[m.nodes["head"]]);
    }

    fn plain() -> Material {
        Material {
            name: "plain".into(),
            base_color: [1.0; 4],
            base_tex: None,
            normal_tex: None,
            mr_tex: None,
            occlusion_tex: None,
            metallic: 0.0,
            roughness: 0.5,
        }
    }

    fn at(x: f32, y: f32, z: f32) -> Mat4 {
        trs([x, y, z], [0.0, 0.0, 0.0, 1.0], [1.0; 3])
    }

    /// A unit quad weighing on `slot`, in the XY plane around the origin.
    fn quad(slot: u32) -> Part {
        let v = |x: f32, y: f32| ModelVert::rigid([x, y, 0.0], [0.0, 0.0, -1.0], [0.0; 2], slot);
        Part { verts: vec![v(-0.5, -0.5), v(0.5, -0.5), v(0.5, 0.5), v(-0.5, 0.5)], indices: vec![0, 1, 2, 0, 2, 3], material: 0 }
    }

    fn keys() -> Vec<Node> {
        vec![
            Node { name: "board".into(), parent: None, rest: at(10.0, 0.0, 0.0) },
            Node { name: "key".into(), parent: Some(0), rest: at(0.0, 1.0, 0.0) },
        ]
    }

    #[test]
    fn a_model_built_in_code_rests_and_poses_with_children_following_parents() {
        let m = Model::build(&keys(), vec![quad(1)], vec![plain()], Vec::new()).expect("builds");
        assert_eq!((m.slots, m.nodes["key"], m.clips.keys().map(String::as_str).collect::<Vec<_>>()), (2, 1, vec!["rest"]));
        assert_eq!((m.min, m.max), ([9.5, 0.5, 0.0], [10.5, 1.5, 0.0]), "the quad hangs on the key");
        assert_eq!(m.node_rest("key"), Some([10.0, 1.0, 0.0]));
        let mut out = Vec::new();
        m.pose_into(&[(0, at(20.0, 0.0, 0.0))], &mut out);
        assert_eq!(out[1][3][..3], [20.0, 1.0, 0.0], "the key follows the board");
        m.pose_into(&[(1, at(0.0, -1.0, 0.0)), (7, at(5.0, 5.0, 5.0))], &mut out);
        assert_eq!((out[0], out[1][3][..3].to_vec()), (at(10.0, 0.0, 0.0), vec![10.0, -1.0, 0.0]), "a missing node is ignored");
    }

    #[test]
    fn a_model_built_wrong_is_refused_with_what_is_wrong() {
        let refused = |nodes: &[Node], part: Part, material: Material, textures: Vec<Texture>| {
            Model::build(nodes, vec![part], vec![material], textures).expect_err("refused")
        };
        let mut orphan = keys();
        orphan[1].parent = Some(5);
        assert!(refused(&orphan, quad(0), plain(), Vec::new()).contains("parent 5 is not a node"));
        let mut cycle = keys();
        cycle[0].parent = Some(1);
        assert!(refused(&cycle, quad(0), plain(), Vec::new()).contains("cycle"));
        let mut twins = keys();
        twins[1].name = "board".into();
        assert!(refused(&twins, quad(0), plain(), Vec::new()).contains("both named 'board'"));
        assert!(refused(&keys(), quad(2), plain(), Vec::new()).contains("2 slots"));
        let mut far = quad(0);
        far.indices.push(9);
        assert!(refused(&keys(), far, plain(), Vec::new()).contains("index 9"));
        let mut lost = quad(0);
        lost.material = 3;
        assert!(refused(&keys(), lost, plain(), Vec::new()).contains("material 3 is missing"));
        let textured = Material { base_tex: Some(0), ..plain() };
        assert!(refused(&keys(), quad(0), textured.clone(), Vec::new()).contains("texture 0 is missing"));
        let short = Texture { w: 2, h: 2, rgba: vec![0; 4] };
        assert!(refused(&keys(), quad(0), textured, vec![short]).contains("4 bytes for 2x2"));
        assert_eq!(Model::build(&keys(), Vec::new(), vec![plain()], Vec::new()).expect_err("refused"), "no parts");
    }

    #[test]
    fn walking_moves_the_legs_and_the_weights_hold_together() {
        let m = termite();
        let walk = &m.clips["walk"];
        let moved = |slot: usize| {
            let (a, b) = (&walk.frames[0][slot], &walk.frames[walk.frames.len() / 4][slot]);
            (0..4).flat_map(|c| (0..4).map(move |r| (c, r))).map(|(c, r)| (a[c][r] - b[c][r]).abs()).fold(0.0, f32::max)
        };
        let leg = m.nodes["leg_front_l_upper"];
        assert!(moved(leg) > 0.01, "the front leg swings");
        for p in &m.parts {
            for v in &p.verts {
                let sum: f32 = v.weights.iter().sum();
                assert!((sum - 1.0).abs() < 1e-3 && v.joints.iter().all(|&j| (j as usize) < m.slots), "{v:?}");
            }
        }
    }
}
