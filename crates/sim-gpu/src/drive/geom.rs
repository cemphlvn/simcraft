//! Building blocks for the drive view: meshes in a car's frame (boxes, tubes, panels, text), lighting baked into
//! vertex colours, and the pictures it makes itself (asphalt, grass, crowd, fence, window net, a dot-matrix font).
//! Everything is generated, so a game needs no art to get a track that reads as a real one; a game may still
//! bring its own.

use sim_render::image::Image;

use crate::gpu::{Mesh, Vert3};
use crate::math::V3;
use crate::stage::{Quad, WHITE, Wrap};

pub const ASPHALT: &str = "__drive_asphalt";
pub const GRASS: &str = "__drive_grass";
pub const CROWD: &str = "__drive_crowd";
pub const FENCE: &str = "__drive_fence";
pub const NET: &str = "__drive_net";
pub const FONT: &str = "__drive_font";
pub const CONCRETE: &str = "__drive_concrete";
/// The rear-view mirror's picture: rendered each frame from behind the car (`Gpu::render_into`).
pub const MIRROR: &str = "__drive_mirror";

/// Colour as the renderer takes it (sRGB, 0..1).
pub fn rgb(c: (u8, u8, u8)) -> [f32; 3] {
    [c.0 as f32 / 255.0, c.1 as f32 / 255.0, c.2 as f32 / 255.0]
}

pub fn rgba(c: [f32; 3], k: f32, a: f32) -> [f32; 4] {
    [c[0] * k, c[1] * k, c[2] * k, a]
}

/// Sunlight on a surface facing `n`: sky light everywhere, the sun on what faces it.
pub fn shade(n: V3, sun: V3) -> f32 {
    0.58 + 0.52 * n.norm().dot(sun).max(0.0)
}

/// A frame in the world: origin and three axes (right, up, forward), as a car or a head has.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub o: V3,
    pub r: V3,
    pub u: V3,
    pub f: V3,
}

impl Frame {
    /// A point given in this frame (x right, y up, z forward).
    pub fn at(&self, p: V3) -> V3 {
        self.o + self.r.scale(p.0) + self.u.scale(p.1) + self.f.scale(p.2)
    }
    pub fn dir(&self, d: V3) -> V3 {
        self.r.scale(d.0) + self.u.scale(d.1) + self.f.scale(d.2)
    }
}

/// Triangles for one texture, in a frame, lit by the sun.
pub struct Builder {
    pub image: String,
    pub wrap: Wrap,
    pub verts: Vec<Vert3>,
    pub sun: V3,
}

impl Builder {
    pub fn new(image: &str, wrap: Wrap, sun: V3) -> Builder {
        Builder { image: image.into(), wrap, verts: Vec::new(), sun }
    }

    pub fn mesh(self) -> Mesh {
        Mesh { image: self.image, wrap: self.wrap, verts: self.verts }
    }

    pub fn vert(&mut self, p: V3, uv: [f32; 2], c: [f32; 4]) {
        self.verts.push(Vert3 { pos: [p.0, p.1, p.2], uv, color: c, fog: 0.0 });
    }

    /// A quad from four corners in order (a b c d), with per-corner colours.
    pub fn quad_c(&mut self, p: [V3; 4], uv: [[f32; 2]; 4], c: [[f32; 4]; 4]) {
        for i in [0usize, 1, 2, 0, 2, 3] {
            self.vert(p[i], uv[i], c[i]);
        }
    }

    /// A flat quad lit by its own normal (from its corners), one colour.
    pub fn quad(&mut self, p: [V3; 4], uv: [[f32; 2]; 4], color: [f32; 3], alpha: f32) {
        let n = (p[1] - p[0]).cross(p[3] - p[0]);
        let n = if n.dot(n) < 1e-12 { (p[2] - p[1]).cross(p[3] - p[1]) } else { n };
        let c = rgba(color, shade(n, self.sun), alpha);
        self.quad_c(p, uv, [c; 4]);
    }

    /// An unlit quad (lights, screens, paint that glows).
    pub fn glow(&mut self, p: [V3; 4], uv: [[f32; 2]; 4], c: [f32; 4]) {
        self.quad_c(p, uv, [c; 4]);
    }

    /// A box in frame `fr`: centre and half extents in the frame's axes; faces lit by their normals.
    pub fn cuboid(&mut self, fr: &Frame, c: V3, h: V3, color: [f32; 3]) {
        let p = |x: f32, y: f32, z: f32| fr.at(V3(c.0 + x * h.0, c.1 + y * h.1, c.2 + z * h.2));
        let uv = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        // Each face wound so that (b - a) × (d - a) points out.
        let faces = [
            [p(-1., 1., -1.), p(-1., 1., 1.), p(1., 1., 1.), p(1., 1., -1.)],     // top
            [p(-1., -1., -1.), p(1., -1., -1.), p(1., -1., 1.), p(-1., -1., 1.)], // bottom
            [p(-1., -1., 1.), p(1., -1., 1.), p(1., 1., 1.), p(-1., 1., 1.)],     // front
            [p(1., -1., -1.), p(-1., -1., -1.), p(-1., 1., -1.), p(1., 1., -1.)], // back
            [p(1., -1., 1.), p(1., -1., -1.), p(1., 1., -1.), p(1., 1., 1.)],     // right
            [p(-1., -1., -1.), p(-1., -1., 1.), p(-1., 1., 1.), p(-1., 1., -1.)], // left
        ];
        for f in faces {
            self.quad(f, uv, color, 1.0);
        }
    }

    /// A round tube from `a` to `b` (points in frame `fr`), `sides` flat faces, lit by their normals.
    pub fn tube(&mut self, fr: &Frame, a: V3, b: V3, radius: f32, sides: usize, color: [f32; 3]) {
        let (wa, wb) = (fr.at(a), fr.at(b));
        let axis = (wb - wa).norm();
        let helper = if axis.1.abs() > 0.9 { V3(1.0, 0.0, 0.0) } else { V3(0.0, 1.0, 0.0) };
        let e1 = axis.cross(helper).norm();
        let e2 = axis.cross(e1).norm();
        let ring = |t: f32| e1.scale(t.cos() * radius) + e2.scale(t.sin() * radius);
        let step = std::f32::consts::TAU / sides as f32;
        for i in 0..sides {
            let (o0, o1) = (ring(i as f32 * step), ring((i + 1) as f32 * step));
            let n = ring((i as f32 + 0.5) * step);
            let c = rgba(color, shade(n, self.sun), 1.0);
            let uv = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
            self.quad_c([wa + o0, wb + o0, wb + o1, wa + o1], uv, [c; 4]);
        }
    }

    /// A disc (wheel face) of radius `r` around `c`, facing `n` (frame axes), `sides` wedges.
    pub fn disc(&mut self, fr: &Frame, c: V3, n: V3, r: f32, sides: usize, color: [f32; 3]) {
        let wc = fr.at(c);
        let wn = fr.dir(n).norm();
        let helper = if wn.1.abs() > 0.9 { V3(1.0, 0.0, 0.0) } else { V3(0.0, 1.0, 0.0) };
        let e1 = wn.cross(helper).norm();
        let e2 = wn.cross(e1).norm();
        let k = rgba(color, shade(wn, self.sun), 1.0);
        let step = std::f32::consts::TAU / sides as f32;
        for i in 0..sides {
            let (t0, t1) = (i as f32 * step, (i + 1) as f32 * step);
            let p0 = wc + e1.scale(t0.cos() * r) + e2.scale(t0.sin() * r);
            let p1 = wc + e1.scale(t1.cos() * r) + e2.scale(t1.sin() * r);
            self.vert(wc, [0.5, 0.5], k);
            self.vert(p1, [0.5, 0.5], k);
            self.vert(p0, [0.5, 0.5], k);
        }
    }
}

// ---------------------------------------------------------------- text

/// The characters the font has, in atlas order (16 a row).
pub const CHARS: &str = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ.:-+/# ";

/// 5 × 7 glyphs, one byte a row, the high bit of the five on the left.
pub fn glyph(c: char) -> [u8; 7] {
    match c {
        '0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        '2' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
        '3' => [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E],
        '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        '5' => [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
        '6' => [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
        'A' => [0x0E, 0x11, 0x11, 0x11, 0x1F, 0x11, 0x11],
        'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        'C' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        'D' => [0x1C, 0x12, 0x11, 0x11, 0x11, 0x12, 0x1C],
        'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        'G' => [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0F],
        'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'I' => [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
        'J' => [0x07, 0x02, 0x02, 0x02, 0x02, 0x12, 0x0C],
        'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        'N' => [0x11, 0x11, 0x19, 0x15, 0x13, 0x11, 0x11],
        'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        'Q' => [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
        'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0A],
        'X' => [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
        'Y' => [0x11, 0x11, 0x11, 0x0A, 0x04, 0x04, 0x04],
        'Z' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
        '.' => [0, 0, 0, 0, 0, 0x0C, 0x0C],
        ':' => [0, 0x0C, 0x0C, 0, 0x0C, 0x0C, 0],
        '-' => [0, 0, 0, 0x1F, 0, 0, 0],
        '+' => [0, 0x04, 0x04, 0x1F, 0x04, 0x04, 0],
        '/' => [0, 0x01, 0x02, 0x04, 0x08, 0x10, 0],
        '#' => [0x0A, 0x0A, 0x1F, 0x0A, 0x1F, 0x0A, 0x0A],
        _ => [0; 7],
    }
}

/// Atlas cells: 6 × 8 dots a glyph (a column and a row of spacing), `DOT` pixels a dot.
const DOT: usize = 8;
const CELL: (usize, usize) = (6 * DOT, 8 * DOT);
const COLS: usize = 16;

/// A character's place in the font atlas (u0, v0, u1, v1); unknown characters are blank.
pub fn glyph_uv(c: char) -> [f32; 4] {
    let i = CHARS.find(c.to_ascii_uppercase()).unwrap_or(CHARS.len() - 1);
    let rows = CHARS.len().div_ceil(COLS);
    let (w, h) = ((COLS * CELL.0) as f32, (rows * CELL.1) as f32);
    let (x, y) = (((i % COLS) * CELL.0) as f32, ((i / COLS) * CELL.1) as f32);
    [x / w, y / h, (x + CELL.0 as f32) / w, (y + CELL.1 as f32) / h]
}

/// Width of one character for text `size` tall (cells are 6:8).
pub fn char_w(size: f32) -> f32 {
    size * 0.75
}

/// 2D text: one quad a character, `size` pixels tall, top-left at (x, y).
pub fn text(out: &mut Vec<Quad>, s: &str, x: f32, y: f32, size: f32, color: [f32; 4]) {
    for (i, c) in s.chars().enumerate() {
        if c == ' ' {
            continue;
        }
        out.push(Quad {
            image: FONT.into(),
            x: x + i as f32 * char_w(size),
            y,
            w: char_w(size),
            h: size,
            uv: glyph_uv(c),
            top: color,
            bottom: color,
            blur: 0.0,
            desat: 0.0,
            wrap: Wrap::Clamp,
            rot: 0.0,
        });
    }
}

/// Text in the world, on a plane: `origin` its top-left, `right` and `down` one character cell's edges.
pub fn text3(b: &mut Builder, s: &str, origin: V3, right: V3, down: V3, color: [f32; 4]) {
    for (i, c) in s.chars().enumerate() {
        if c == ' ' {
            continue;
        }
        let [u0, v0, u1, v1] = glyph_uv(c);
        let a = origin + right.scale(i as f32);
        b.quad_c([a, a + right, a + right + down, a + down], [[u0, v0], [u1, v0], [u1, v1], [u0, v1]], [color; 4]);
    }
}

/// A plain rectangle on screen.
pub fn rect(out: &mut Vec<Quad>, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) {
    out.push(Quad {
        image: WHITE.into(),
        x,
        y,
        w,
        h,
        uv: [0.0, 0.0, 1.0, 1.0],
        top: color,
        bottom: color,
        blur: 0.0,
        desat: 0.0,
        wrap: Wrap::Clamp,
        rot: 0.0,
    });
}

// ---------------------------------------------------------------- pictures

/// A hash of integers to 0..1 (deterministic noise for textures and scenery).
pub fn hash01(a: i64, b: i64, c: i64) -> f32 {
    let mut x = (a as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (b as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F) ^ (c as u64);
    x ^= x >> 31;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 29;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 32;
    (x >> 40) as f32 / (1u64 << 24) as f32
}

/// Smooth value noise on a periodic lattice (`period` cells), so textures tile.
fn value_noise(x: f32, y: f32, period: i64, seed: i64) -> f32 {
    let (xi, yi) = (x.floor() as i64, y.floor() as i64);
    let (fx, fy) = (x - x.floor(), y - y.floor());
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let h = |i: i64, j: i64| hash01(i.rem_euclid(period), j.rem_euclid(period), seed);
    let a = h(xi, yi) + (h(xi + 1, yi) - h(xi, yi)) * s(fx);
    let b = h(xi, yi + 1) + (h(xi + 1, yi + 1) - h(xi, yi + 1)) * s(fx);
    a + (b - a) * s(fy)
}

fn img(w: usize, h: usize, f: impl Fn(usize, usize) -> [u8; 4]) -> Image {
    Image { w, h, px: (0..w * h).map(|i| f(i % w, i / w)).collect() }
}

fn grey(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0) as u8
}

/// Asphalt: aggregate speckle on a grey that the vertex colour tints (neutral around 0.8, so colours read true).
fn asphalt() -> Image {
    img(256, 256, |x, y| {
        let (fx, fy) = (x as f32, y as f32);
        let low = value_noise(fx / 32.0, fy / 32.0, 8, 1) * 0.10 + value_noise(fx / 8.0, fy / 8.0, 32, 2) * 0.08;
        let grain = hash01(x as i64, y as i64, 3);
        let stone = if grain > 0.93 {
            0.12
        } else if grain < 0.06 {
            -0.14
        } else {
            (grain - 0.5) * 0.08
        };
        let v = grey(0.72 + low + stone);
        [v, v, v, 255]
    })
}

/// Concrete: finer and lighter than asphalt, with faint trowel streaks.
fn concrete() -> Image {
    img(256, 256, |x, y| {
        let (fx, fy) = (x as f32, y as f32);
        let low = value_noise(fx / 40.0, fy / 40.0, 6, 11) * 0.08 + value_noise(fx / 4.0, fy / 64.0, 64, 12) * 0.05;
        let v = grey(0.80 + low + (hash01(x as i64, y as i64, 13) - 0.5) * 0.05);
        [v, v, v, 255]
    })
}

/// Grass mowed in stripes (race-day infield): two shades along one axis, blades as noise.
fn grass() -> Image {
    img(256, 256, |x, y| {
        let (fx, fy) = (x as f32, y as f32);
        let stripe = if (x / 128) % 2 == 0 { 1.0 } else { 0.84 };
        let n = value_noise(fx / 16.0, fy / 16.0, 16, 21) * 0.18 + hash01(x as i64, y as i64, 22) * 0.16;
        let k = stripe * (0.74 + n);
        [grey(0.95 * k), grey(1.0 * k), grey(0.82 * k), 255]
    })
}

/// A grandstand full of people: rows of seats, most taken by shirts of every colour, some empty.
fn crowd() -> Image {
    const SHIRTS: [(u8, u8, u8); 10] = [
        (200, 40, 40),
        (240, 240, 235),
        (30, 60, 150),
        (250, 200, 40),
        (40, 40, 45),
        (230, 110, 30),
        (60, 140, 70),
        (120, 60, 140),
        (180, 180, 190),
        (90, 170, 210),
    ];
    img(256, 256, |x, y| {
        // 16 rows of 32 seats in the picture; each seat 8 × 16 pixels.
        let (row, seat) = (y / 16, x / 8);
        let (in_x, in_y) = (x % 8, y % 16);
        if in_y >= 13 {
            return [70, 72, 78, 255]; // the step in front of the row
        }
        let taken = hash01(row as i64, seat as i64, 31) < 0.86;
        if !taken || in_x == 0 || in_x == 7 {
            return if in_y > 6 { [55, 85, 140, 255] } else { [45, 70, 120, 255] }; // a blue seat
        }
        let head = in_y < 4 && (2..6).contains(&in_x);
        if head {
            let skin = hash01(row as i64, seat as i64, 32);
            let c = if skin < 0.3 {
                (95, 60, 40)
            } else if skin < 0.7 {
                (200, 150, 115)
            } else {
                (160, 110, 80)
            };
            return [c.0, c.1, c.2, 255];
        }
        if in_y < 4 {
            return [55, 85, 140, 255];
        }
        let s = SHIRTS[(hash01(row as i64, seat as i64, 33) * SHIRTS.len() as f32) as usize % SHIRTS.len()];
        [s.0, s.1, s.2, 255]
    })
}

/// Catch fence: chain-link diamonds and a heavy horizontal cable (transparent between wires).
fn fence() -> Image {
    img(128, 128, |x, y| {
        let (a, b) = ((x + y) % 32, (x + 128 - y) % 32);
        let wire = a < 2 || b < 2;
        let cable = (60..64).contains(&y);
        if cable {
            [150, 152, 150, 255]
        } else if wire {
            [175, 178, 176, 255]
        } else {
            [0, 0, 0, 0]
        }
    })
}

/// The driver's window net: black webbing in squares.
fn net() -> Image {
    img(128, 128, |x, y| if x % 32 < 5 || y % 32 < 5 { [22, 22, 24, 255] } else { [0, 0, 0, 0] })
}

/// The dot-matrix font: white round dots on transparent (tinted by vertex or quad colour).
fn font() -> Image {
    let rows = CHARS.len().div_ceil(COLS);
    let (w, h) = (COLS * CELL.0, rows * CELL.1);
    let mut px = vec![[0u8; 4]; w * h];
    for (i, c) in CHARS.chars().enumerate() {
        let g = glyph(c);
        let (cx, cy) = ((i % COLS) * CELL.0, (i / COLS) * CELL.1);
        for (r, bits) in g.iter().enumerate() {
            for col in 0..5 {
                if bits & (0x10 >> col) == 0 {
                    continue;
                }
                for dy in 0..DOT {
                    for dx in 0..DOT {
                        let (u, v) = (dx as f32 + 0.5 - DOT as f32 / 2.0, dy as f32 + 0.5 - DOT as f32 / 2.0);
                        let d = (u * u + v * v).sqrt() / (DOT as f32 / 2.0);
                        let a = ((1.08 - d) / 0.2).clamp(0.0, 1.0);
                        let (x, y) = (cx + col * DOT + dx + DOT / 2, cy + r * DOT + dy + DOT / 2);
                        px[y * w + x] = [255, 255, 255, (a * 255.0) as u8];
                    }
                }
            }
        }
    }
    Image { w, h, px }
}

/// Every picture the drive view makes for itself, by name.
pub fn textures() -> Vec<(String, Image)> {
    vec![
        (ASPHALT.into(), asphalt()),
        (CONCRETE.into(), concrete()),
        (GRASS.into(), grass()),
        (CROWD.into(), crowd()),
        (FENCE.into(), fence()),
        (NET.into(), net()),
        (FONT.into(), font()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_character_has_its_own_cell_and_unknown_ones_are_blank() {
        let a = glyph_uv('A');
        let b = glyph_uv('B');
        assert!(a != b && a[2] > a[0] && a[3] > a[1]);
        assert_eq!(glyph_uv('?'), glyph_uv(' '));
        assert_eq!(glyph_uv('a'), glyph_uv('A'), "lower case reads as upper case");
    }

    #[test]
    fn a_tube_is_closed_and_a_box_has_six_faces() {
        let fr = Frame { o: V3(0.0, 0.0, 0.0), r: V3(1.0, 0.0, 0.0), u: V3(0.0, 1.0, 0.0), f: V3(0.0, 0.0, 1.0) };
        let mut b = Builder::new(WHITE, Wrap::Clamp, V3(0.0, 1.0, 0.0));
        b.tube(&fr, V3(0.0, 0.0, 0.0), V3(0.0, 0.0, 1.0), 0.1, 6, [1.0; 3]);
        assert_eq!(b.verts.len(), 6 * 6);
        let far = b.verts.iter().map(|v| (v.pos[0].powi(2) + v.pos[1].powi(2)).sqrt()).fold(0.0, f32::max);
        assert!((far - 0.1).abs() < 1e-4, "every vertex on the radius");
        let mut c = Builder::new(WHITE, Wrap::Clamp, V3(0.0, 1.0, 0.0));
        c.cuboid(&fr, V3(0.0, 0.0, 0.0), V3(1.0, 1.0, 1.0), [1.0; 3]);
        assert_eq!(c.verts.len(), 36);
        // The top face is lit by the sun overhead, the bottom is not.
        assert!(c.verts[0].color[0] > c.verts[6].color[0]);
    }
}
