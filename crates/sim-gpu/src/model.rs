//! Models from the best tools: glTF 2.0 (`.glb`), the format Blender, Unity, Unreal and Higgsfield's 3D generators
//! all write. A model is loaded once and made ready for crowds: every clip is sampled once into matrices (a palette
//! per frame), so a character on screen costs one small record (where it is, which frame), not a mesh.
//!
//! Conventions are glTF's: +Y up, +Z forward (the spec's "front of an asset"), metres or any unit (`length` in the
//! view scales it). Skinned meshes and rigid parts (meshes on animated nodes) both play: every node of the scene has
//! a palette entry, and skin joints have one more with their inverse bind matrix.

use std::collections::BTreeMap;
use std::path::Path;

/// A column-major 4x4 matrix (glTF's layout).
pub type Mat4 = [[f32; 4]; 4];

pub const IDENTITY: Mat4 = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

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

#[derive(Clone, Debug)]
pub struct Model {
    pub parts: Vec<Part>,
    pub materials: Vec<Material>,
    pub textures: Vec<Texture>,
    /// Palette slots: every node (by index), then every skin joint again with its inverse bind matrix.
    pub slots: usize,
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

impl Model {
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
        let slots = n + skin_joints.len();

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

        // Rest-pose globals (for rigid meshes, and the bounds).
        let globals = |local: &dyn Fn(usize) -> Mat4| -> Vec<Mat4> {
            let mut g: Vec<Option<Mat4>> = vec![None; n];
            fn walk(i: usize, parent: &[Option<usize>], local: &dyn Fn(usize) -> Mat4, g: &mut Vec<Option<Mat4>>) -> Mat4 {
                if let Some(m) = g[i] {
                    return m;
                }
                let m = match parent[i] {
                    Some(p) => mul(&walk(p, parent, local, g), &local(i)),
                    None => local(i),
                };
                g[i] = Some(m);
                m
            }
            (0..n).map(|i| walk(i, &parent, local, &mut g)).collect()
        };
        let rest_local = |i: usize| trs(rest[i].0, rest[i].1, rest[i].2);
        let rest_globals = globals(&rest_local);

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

        // The palette for one set of node globals.
        let palette = |g: &[Mat4]| -> Vec<Mat4> {
            let mut p: Vec<Mat4> = g.to_vec();
            p.extend(skin_joints.iter().zip(&ibms).map(|(&j, ibm)| mul(&g[j], ibm)));
            p
        };

        // Bounds in the rest pose.
        let rest_palette = palette(&rest_globals);
        let (mut min, mut max) = ([f32::MAX; 3], [f32::MIN; 3]);
        for p in &parts {
            for v in &p.verts {
                let mut q = [0.0f32; 3];
                for k in 0..4 {
                    if v.weights[k] > 0.0 {
                        let t = transform_point(&rest_palette[v.joints[k] as usize], v.pos);
                        (0..3).for_each(|a| q[a] += t[a] * v.weights[k]);
                    }
                }
                (0..3).for_each(|a| {
                    min[a] = min[a].min(q[a]);
                    max[a] = max[a].max(q[a]);
                });
            }
        }

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
                    let l = |i: usize| trs(local[i].0, local[i].1, local[i].2);
                    palette(&globals(&l))
                })
                .collect();
            let name = anim.name().map_or_else(|| format!("clip{ai}"), str::to_string);
            clips.insert(name, Clip { fps: SAMPLE_FPS, duration, frames });
        }
        Ok(Model { parts, materials, textures, slots, nodes, clips, min, max })
    }

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
