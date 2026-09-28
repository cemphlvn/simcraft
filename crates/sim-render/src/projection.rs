//! How a world is shown: dimensionality is a view, not the world.
//!
//! `Dim2(level)` top-down · `Dim2_5` levels stacked · `Dim3` ray-cast voxels with an orbit camera and a cutaway ·
//! `CustomDim` any world axis to screen x / y, the rest fixed (cross-sections, and worlds with more axes later).

use serde::Deserialize;

use crate::canvas::{Cell, Rect, Rgb};
use crate::component::Ctx;
use crate::layout::{Size, cols, rows};
use crate::scene::Scene;
use crate::style::Style;

/// A world axis: 0 = x, 1 = y, 2 = z (level).
pub type Axis = usize;

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub enum Projection {
    /// One level, top-down.
    Dim2 {
        #[serde(default)]
        level: i64,
        #[serde(default)]
        tint: Option<String>,
    },
    /// Every level, stacked: side by side (`across: true`) or one above the other.
    Dim2_5 {
        #[serde(default)]
        across: bool,
        #[serde(default)]
        tint: Option<String>,
    },
    /// Voxels from an orbit camera.
    Dim3(Camera),
    /// `n` world axes; `x` and `y` go to the screen, `fixed` pins the others: (axis, value).
    CustomDim {
        n: usize,
        x: Axis,
        y: Axis,
        #[serde(default)]
        fixed: Vec<(Axis, i64)>,
        #[serde(default)]
        tint: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Camera {
    /// Degrees around the vertical axis.
    #[serde(default = "yaw")]
    pub yaw: f32,
    /// Degrees above the horizon.
    #[serde(default = "pitch")]
    pub pitch: f32,
    /// Distance as a multiple of the world's size.
    #[serde(default = "zoom")]
    pub zoom: f32,
    /// Cutaway: voxels with world y below this are not drawn (see inside). None = no cut.
    #[serde(default)]
    pub cut: Option<i64>,
}

fn yaw() -> f32 {
    35.0
}
fn pitch() -> f32 {
    30.0
}
fn zoom() -> f32 {
    1.0
}

impl Default for Camera {
    fn default() -> Self {
        Camera { yaw: yaw(), pitch: pitch(), zoom: zoom(), cut: None }
    }
}

/// A projection as written in view.ron: `(dim: "2D", level: 0)`, `(dim: "2.5D")`, `(dim: "3D", yaw: 30)`,
/// `(dim: "custom", n: 3, x: 0, y: 2, fixed: [(1, 8)])`.
/// Props are plain values (no `Some(...)` needed): `cut: -1` = no cut, `tint: ""` = no tint.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProjectionSpec {
    pub dim: String,
    pub level: i64,
    pub across: bool,
    pub tint: String,
    pub yaw: f32,
    pub pitch: f32,
    pub zoom: f32,
    pub cut: i64,
    pub n: usize,
    pub x: i64,
    pub y: i64,
    pub fixed: Vec<(Axis, i64)>,
}

impl Default for ProjectionSpec {
    fn default() -> Self {
        ProjectionSpec {
            dim: "2D".into(),
            level: 0,
            across: false,
            tint: String::new(),
            yaw: yaw(),
            pitch: pitch(),
            zoom: zoom(),
            cut: -1,
            n: 3,
            x: -1,
            y: -1,
            fixed: Vec::new(),
        }
    }
}

impl ProjectionSpec {
    pub fn build(self) -> Result<Projection, String> {
        let tint = (!self.tint.is_empty()).then_some(self.tint);
        Ok(match self.dim.as_str() {
            "2D" | "2d" => Projection::Dim2 { level: self.level, tint },
            "2.5D" | "2_5D" | "2.5d" => Projection::Dim2_5 { across: self.across, tint },
            "3D" | "3d" => Projection::Dim3(Camera {
                yaw: self.yaw,
                pitch: self.pitch,
                zoom: self.zoom,
                cut: (self.cut >= 0).then_some(self.cut),
            }),
            "custom" | "CustomDim" => Projection::CustomDim {
                n: self.n,
                x: usize::try_from(self.x).map_err(|_| "custom projection needs x (the world axis for screen x)")?,
                y: usize::try_from(self.y).map_err(|_| "custom projection needs y (the world axis for screen y)")?,
                fixed: self.fixed,
                tint,
            },
            other => return Err(format!("dim '{other}': use \"2D\", \"2.5D\", \"3D\" or \"custom\"")),
        })
    }
}

impl Projection {
    pub fn draw(&self, ctx: &mut Ctx, r: Rect) {
        match self {
            Projection::Dim2 { level, tint } => plane(ctx, r, (0, 1), &[(2, *level)], tint.as_deref()),
            Projection::Dim2_5 { across, tint } => {
                let d = ctx.scene.world.depth.max(1) as usize;
                let parts = vec![Size::Fill; d];
                let areas = if *across { cols(r, &parts) } else { rows(r, &parts) };
                for (z, area) in areas.into_iter().enumerate() {
                    plane(ctx, area, (0, 1), &[(2, z as i64)], tint.as_deref());
                    let dim = ctx.style.color("dim");
                    ctx.canvas.text(area, 0, 0, &format!("level {z}"), dim);
                }
            }
            Projection::CustomDim { x, y, fixed, tint, .. } => plane(ctx, r, (*x, *y), fixed, tint.as_deref()),
            Projection::Dim3(cam) => voxels(ctx, r, cam),
        }
    }

    pub fn title(&self) -> String {
        match self {
            Projection::Dim2 { level, .. } => format!("world · 2D · level {level}"),
            Projection::Dim2_5 { .. } => "world · 2.5D · all levels".into(),
            Projection::Dim3(cam) => format!(
                "world · 3D · yaw {:.0}° pitch {:.0}°{}",
                cam.yaw,
                cam.pitch,
                cam.cut.map(|c| format!(" · cut y<{c}")).unwrap_or_default()
            ),
            Projection::CustomDim { n, x, y, fixed, .. } => format!("world · {n}D · axes {x},{y} · fixed {fixed:?}"),
        }
    }
}

/// A 2D plane through the world: screen x/y are two world axes, the others are fixed.
fn plane(ctx: &mut Ctx, r: Rect, (ax, ay): (Axis, Axis), fixed: &[(Axis, i64)], tint: Option<&str>) {
    let (scene, style, c) = (ctx.scene, &ctx.style, &mut *ctx.canvas);
    let size = [scene.world.width, scene.world.height, scene.world.depth];
    for sy in 0..r.h as i64 {
        for sx in 0..r.w as i64 {
            let mut p = [0i64; 3];
            for &(a, v) in fixed {
                if a < 3 {
                    p[a] = v;
                }
            }
            p[ax.min(2)] = sx;
            p[ay.min(2)] = sy;
            if p[ax.min(2)] >= size[ax.min(2)] || p[ay.min(2)] >= size[ay.min(2)] || p[2] >= size[2] {
                continue;
            }
            let bg = scene.ground(style, p[0], p[1], p[2], tint);
            let cell = match scene.top(p[0], p[1], p[2]) {
                Some(e) => Cell { ch: scene.glyph(e), fg: scene.color(e), bg },
                None if scene.terrain(p[0], p[1], p[2]) => Cell { ch: '░', fg: style.color("soil_glyph"), bg },
                None => Cell { ch: ' ', fg: style.color("dim"), bg },
            };
            c.put(r.x + sx as u16, r.y + sy as u16, cell);
        }
    }
}

type V3 = [f32; 3];

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn mul(a: V3, s: f32) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn norm(a: V3) -> V3 {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt().max(1e-6);
    mul(a, 1.0 / l)
}

/// Render space: X = world x, Y = up (= -z, so level 0 is on top), Z = world y.
fn voxel_at(scene: &Scene, style: &Style, cam: &Camera, v: [i64; 3]) -> Option<Rgb> {
    let (x, y, z) = (v[0], v[2], -v[1]);
    if !scene.world.in_bounds3(x, y, z) || cam.cut.is_some_and(|cut| y < cut) {
        return None;
    }
    if let Some(e) = scene.top(x, y, z) {
        return Some(scene.voxel(e));
    }
    scene.terrain(x, y, z).then(|| style.color("soil"))
}

/// Ray-casts voxels (Amanatides & Woo) into half-block pixels: two pixels per cell vertically.
fn voxels(ctx: &mut Ctx, r: Rect, cam: &Camera) {
    let (scene, style, c) = (ctx.scene, &ctx.style, &mut *ctx.canvas);
    let (sky_low, sky_high) = (style.color("sky_low"), style.color("sky_high"));
    let w = scene.world;
    let size = [w.width as f32, w.depth as f32, w.height as f32];
    // Level 0 spans render Y 0..1 and level d-1 spans -(d-1)..-(d-2): look at the middle of that.
    let center = [size[0] / 2.0, 1.0 - size[1] / 2.0, size[2] / 2.0];
    let radius = (size[0] * size[0] + size[1] * size[1] + size[2] * size[2]).sqrt() * cam.zoom;
    let (yaw, pitch) = (cam.yaw.to_radians(), cam.pitch.to_radians());
    let eye = add(center, [radius * pitch.cos() * yaw.sin(), radius * pitch.sin(), -radius * pitch.cos() * yaw.cos()]);
    let fwd = norm(sub(center, eye));
    let right = norm(cross(fwd, [0.0, 1.0, 0.0]));
    let up = cross(right, fwd);
    let (pw, ph) = (r.w as f32, r.h as f32 * 2.0);
    // Terminal cells are about twice as tall as wide; half blocks make pixels square.
    let fov = 0.9;
    let pixel = |px: f32, py: f32| -> Rgb {
        let u = (px / pw - 0.5) * 2.0 * fov * (pw / ph);
        let v = (0.5 - py / ph) * 2.0 * fov;
        let dir = norm(add(fwd, add(mul(right, u), mul(up, v))));
        cast(scene, style, cam, eye, dir, radius * 2.5).unwrap_or_else(|| sky(v, sky_low, sky_high))
    };
    for cy in 0..r.h {
        for cx in 0..r.w {
            let top = pixel(cx as f32 + 0.5, cy as f32 * 2.0 + 0.5);
            let bottom = pixel(cx as f32 + 0.5, cy as f32 * 2.0 + 1.5);
            c.pixels(r.x + cx, r.y + cy, top, bottom);
        }
    }
}

fn sky(v: f32, low: Rgb, high: Rgb) -> Rgb {
    let t = ((v + 1.0) * 0.5).clamp(0.0, 1.0);
    let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t) as u8;
    Rgb(mix(low.0, high.0), mix(low.1, high.1), mix(low.2, high.2))
}

fn cast(scene: &Scene, style: &Style, cam: &Camera, eye: V3, dir: V3, max: f32) -> Option<Rgb> {
    let mut v = [eye[0].floor() as i64, eye[1].floor() as i64, eye[2].floor() as i64];
    let step = [dir[0].signum() as i64, dir[1].signum() as i64, dir[2].signum() as i64];
    let delta = [(1.0 / dir[0]).abs(), (1.0 / dir[1]).abs(), (1.0 / dir[2]).abs()];
    let mut side = [0.0f32; 3];
    for i in 0..3 {
        side[i] = if dir[i] > 0.0 { (v[i] as f32 + 1.0 - eye[i]) * delta[i] } else { (eye[i] - v[i] as f32) * delta[i] };
    }
    let mut t = 0.0;
    let mut face = 1;
    while t < max {
        if let Some(col) = voxel_at(scene, style, cam, v) {
            // Faces lit from above-left; distance fog.
            let light = [80, 100, 65][face];
            let fog = (100.0 - (t / max) * 60.0).max(35.0) as u32;
            return Some(col.shade(light * fog / 100));
        }
        let i = if side[0] < side[1] { if side[0] < side[2] { 0 } else { 2 } } else if side[1] < side[2] { 1 } else { 2 };
        t = side[i];
        side[i] += delta[i];
        v[i] += step[i];
        face = i;
    }
    None
}
