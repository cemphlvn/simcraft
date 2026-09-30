//! Cars drawn with a model (`look.model` in drive.ron): a glTF made in any tool, fitted once into the car's frame
//! (x right, y up, z forward, metres, the ground under its middle at the origin) and scaled to the kind's footprint,
//! so every car is one instance of it (`skin::Instance`): its seat on the banking as the transform, its livery as a
//! tint of the model's white paint plus a second colour, its number as decals on the doors and the roof.

use std::path::Path;

use serde::Deserialize;

use super::geom::{Builder, Frame, glyph};
use crate::math::V3;
use crate::model::Model;
use crate::skin::{Frames, Instance};

/// The texture name the model is uploaded as.
pub const CAR_MODEL: &str = "__drive_car";

/// The model and how it sits in its file.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelLook {
    /// A `.glb`, next to the game or in an `assets/` folder above it.
    pub file: String,
    /// The axis the car's nose points along in the file: "+z" (glTF's front), "-z", "+x" or "-x".
    #[serde(default = "plus_z")]
    pub forward: String,
    /// The car's height (m); none: scaled with its length and width.
    #[serde(default)]
    pub height: Option<f32>,
}

fn plus_z() -> String {
    "+z".into()
}

/// A car's paint: the first colour on the body, the second on the rockers and the stripes (and the number plates).
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
pub struct Livery(pub (u8, u8, u8), pub (u8, u8, u8));

/// The model, fitted: its size, where the decals go, the rest pose's palette frames.
pub struct CarModel {
    pub model: Model,
    /// Length, width, height (m).
    pub size: V3,
    /// The door's outer face (m right of the middle), the height and the middle along the car of the number there.
    pub door: V3,
    /// The roof's top (m up) and where its middle is along the car.
    pub roof: (f32, f32),
    pub frames: [u32; 2],
}

impl CarModel {
    /// Loads `file` and fits it to `footprint` (width, length: m).
    pub fn load(path: &Path, look: &ModelLook, footprint: (f32, f32)) -> Result<CarModel, String> {
        let mut model = Model::load(path)?;
        // The file's axes to the car's: forward, up (+y, glTF's), and right = forward × up in the file's
        // right-handed frame. In the car's frame (x right, y up, z forward: left-handed, as the render space is)
        // that is a mirror image, which is what turns a right-handed file's front faces to the camera: the
        // triangles keep their winding (tested below).
        let (fwd, sign): (usize, f32) = match look.forward.as_str() {
            "+z" => (2, 1.0),
            "-z" => (2, -1.0),
            "+x" => (0, 1.0),
            "-x" => (0, -1.0),
            other => return Err(format!("{}: forward '{other}': one of +z, -z, +x, -x", path.display())),
        };
        let side = 2 - fwd; // the other horizontal axis
        let fvec = |i: usize| if i == fwd { sign } else { 0.0 };
        let up = [0.0, 1.0, 0.0];
        let f3 = [fvec(0), fvec(1), fvec(2)];
        let right = [f3[1] * up[2] - f3[2] * up[1], f3[2] * up[0] - f3[0] * up[2], f3[0] * up[1] - f3[1] * up[0]];
        let rsign = right[side];
        let (min, max) = (model.min, model.max);
        let mid = [(min[0] + max[0]) / 2.0, min[1], (min[2] + max[2]) / 2.0];
        let (len, wid, hgt) = (max[fwd] - min[fwd], max[side] - min[side], max[1] - min[1]);
        let (kl, kw) = (footprint.1 / len.max(1e-6), footprint.0 / wid.max(1e-6));
        let kh = look.height.map_or((kl + kw) / 2.0, |h| h / hgt.max(1e-6));
        let to_car = |p: [f32; 3]| V3((p[side] - mid[side]) * rsign * kw, (p[1] - mid[1]) * kh, (p[fwd] - mid[fwd]) * sign * kl);
        let normal_to_car = |n: [f32; 3]| V3(n[side] * rsign / kw, n[1] / kh, n[fwd] * sign / kl).norm();
        for part in &mut model.parts {
            for v in &mut part.verts {
                let (p, n) = (to_car(v.pos), normal_to_car(v.normal));
                v.pos = [p.0, p.1, p.2];
                v.normal = [n.0, n.1, n.2];
            }
        }
        let verts: Vec<V3> = model.parts.iter().flat_map(|p| p.verts.iter().map(|v| V3(v.pos[0], v.pos[1], v.pos[2]))).collect();
        let size = V3(footprint.1, hgt * kh, footprint.0);
        // The door: the outermost point of the body's middle third at number height; the roof: the highest point
        // over the middle, and the middle of what is near that height.
        let door_y = size.1 * 0.48;
        let door_x = verts
            .iter()
            .filter(|p| p.2.abs() < size.0 * 0.15 && (p.1 - door_y).abs() < size.1 * 0.12)
            .map(|p| p.0.abs())
            .fold(0.0f32, f32::max);
        let roof_y = verts.iter().filter(|p| p.0.abs() < size.2 * 0.15).map(|p| p.1).fold(0.0f32, f32::max);
        let top: Vec<f32> = verts.iter().filter(|p| p.0.abs() < size.2 * 0.15 && p.1 > roof_y - 0.03).map(|p| p.2).collect();
        let roof_z = top.iter().sum::<f32>() / top.len().max(1) as f32;
        let frames = Frames::of(&model).at("rest", 0.0).0;
        Ok(CarModel { model, size, door: V3(door_x, door_y, 0.0), roof: (roof_y, roof_z), frames })
    }

    /// The instance for a car seated at `fr` in `livery` (sRGB colours).
    pub fn instance(&self, fr: &Frame, livery: &Livery) -> Instance {
        let lin = |c: (u8, u8, u8)| [c.0, c.1, c.2].map(|v| (v as f32 / 255.0).powf(2.2));
        let (a, b) = (lin(livery.0), lin(livery.1));
        let row = |k: usize| {
            let g = |v: V3| [v.0, v.1, v.2][k];
            [g(fr.r), g(fr.u), g(fr.f), g(fr.o)]
        };
        Instance {
            m: [row(0), row(1), row(2)],
            frames: [self.frames[0], self.frames[1], 0, 0],
            params: [0.0, b[0], b[1], b[2]],
            tint: [a[0], a[1], a[2], 2.0],
        }
    }

    /// The car's number on both doors and the roof: blocky digits on a plate in the livery's second colour.
    pub fn decals(&self, b: &mut Builder, fr: &Frame, number: i64, livery: &Livery) {
        let text = number.abs().to_string();
        let plate = super::geom::rgb(livery.1);
        // Digits dark on a light plate, light on a dark one.
        let lum = plate[0] * 0.3 + plate[1] * 0.59 + plate[2] * 0.11;
        let ink = if lum > 0.5 { [0.05, 0.05, 0.06] } else { [0.97, 0.97, 0.97] };
        let (h, lift) = (0.46, 0.012);
        for side in [-1.0f32, 1.0] {
            // Read from outside: toward the nose on the right door, toward the tail on the left.
            let o = V3(side * (self.door.0 + lift), self.door.1, self.door.2);
            let along = V3(0.0, 0.0, side);
            number_at(b, fr, o, along, V3(0.0, 1.0, 0.0), h, &text, plate, ink);
        }
        // The roof: read from the right of the car (from the grandstands), top of the digits toward its left.
        let o = V3(0.0, self.roof.0 + lift, self.roof.1);
        number_at(b, fr, o, V3(0.0, 0.0, 1.0), V3(-1.0, 0.0, 0.0), 0.55, &text, plate, ink);
    }
}

/// `text` centred on `o` (car frame) in a plane spanned by `along` (reading direction) and `up`, digits `h` tall,
/// on a plate.
#[allow(clippy::too_many_arguments)]
fn number_at(b: &mut Builder, fr: &Frame, o: V3, along: V3, up: V3, h: f32, text: &str, plate: [f32; 3], ink: [f32; 3]) {
    let cell = h / 7.0;
    let w = text.len() as f32 * 6.0 * cell - cell;
    let out = along.cross(up).scale(-0.002);
    let at = |x: f32, y: f32| fr.at(o + along.scale(x - w / 2.0) + up.scale(h / 2.0 - y));
    let uv = [[0.0; 2]; 4];
    let pad = cell * 1.5;
    b.quad([at(-pad, -pad), at(w + pad, -pad), at(w + pad, h + pad), at(-pad, h + pad)], uv, plate, 1.0);
    for (i, c) in text.chars().enumerate() {
        let rows = glyph(c);
        for (r, bits) in rows.iter().enumerate() {
            for col in 0..5 {
                if bits & (0x10 >> col) == 0 {
                    continue;
                }
                let (x, y) = ((i * 6 + col) as f32 * cell, r as f32 * cell);
                let q = [at(x, y), at(x + cell, y), at(x + cell, y + cell), at(x, y + cell)].map(|p| p + fr.dir(out));
                b.quad(q, uv, ink, 1.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn car() -> CarModel {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/race/stock_car.glb");
        let look = ModelLook { file: String::new(), forward: "-x".into(), height: Some(1.3) };
        CarModel::load(&path, &look, (1.996, 4.912)).expect("loads and fits")
    }

    #[test]
    fn the_model_is_fitted_to_the_footprint_nose_forward_wheels_on_the_ground() {
        let c = car();
        let pts: Vec<[f32; 3]> = c.model.parts.iter().flat_map(|p| p.verts.iter().map(|v| v.pos)).collect();
        let span = |a: usize| {
            let (lo, hi) = pts.iter().fold((f32::MAX, f32::MIN), |(l, h), p| (l.min(p[a]), h.max(p[a])));
            (lo, hi)
        };
        let ((x0, x1), (y0, y1), (z0, z1)) = (span(0), span(1), span(2));
        assert!((x1 - x0 - 1.996).abs() < 0.01 && (z1 - z0 - 4.912).abs() < 0.01, "{x0}..{x1} × {z0}..{z1}");
        assert!(y0.abs() < 1e-3 && (y1 - 1.3).abs() < 0.01, "on the ground, 1.3 m high: {y0}..{y1}");
        // The nose (+z) is the low end: the hood slopes down to it; the deck and spoiler stand higher at the tail.
        let top = |from: f32, to: f32| pts.iter().filter(|p| p[2] > from && p[2] < to).map(|p| p[1]).fold(0.0f32, f32::max);
        assert!(top(2.2, 2.5) < top(-2.5, -2.2), "nose {} vs tail {}", top(2.2, 2.5), top(-2.5, -2.2));
        // The roof over the middle, the doors near the body's side.
        assert!(c.roof.1.abs() < 0.8 && c.roof.0 > 1.2, "roof {:?}", c.roof);
        assert!(c.door.0 > 0.85 && c.door.0 < 1.0, "door {:?}", c.door);
    }

    #[test]
    fn mirroring_into_the_cars_frame_keeps_the_triangles_facing_out() {
        let c = car();
        let (mut agree, mut all) = (0usize, 0usize);
        for p in &c.model.parts {
            for t in p.indices.as_chunks::<3>().0 {
                let v = |i: u32| V3(p.verts[i as usize].pos[0], p.verts[i as usize].pos[1], p.verts[i as usize].pos[2]);
                let n = p.verts[t[0] as usize].normal;
                let g = (v(t[1]) - v(t[0])).cross(v(t[2]) - v(t[0]));
                all += 1;
                // Left-handed render space: a front face's corners turn the other way round its normal.
                agree += usize::from(g.dot(V3(n[0], n[1], n[2])) < 0.0);
            }
        }
        assert!(agree as f32 > all as f32 * 0.8, "{agree} of {all} triangles wound as their normals say");
    }
}
