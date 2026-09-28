//! Pixel art: an RGBA buffer, sprites from asset packs, procedural backdrop layers, and two ways to show them
//! (true pixels via the kitty graphics protocol, or half-block characters).

use std::collections::BTreeMap;
use std::io::{self, Write};

use serde::Deserialize;

use crate::canvas::{Canvas, Rect, Rgb};

/// An RGB image; `None` pixels are transparent when blitting sprites.
#[derive(Clone, Debug, PartialEq)]
pub struct Pixmap {
    pub w: usize,
    pub h: usize,
    pub px: Vec<Rgb>,
}

impl Pixmap {
    pub fn new(w: usize, h: usize, fill: Rgb) -> Pixmap {
        Pixmap { w, h, px: vec![fill; w * h] }
    }

    pub fn set(&mut self, x: i64, y: i64, c: Rgb) {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
            self.px[y as usize * self.w + x as usize] = c;
        }
    }

    pub fn get(&self, x: i64, y: i64) -> Option<Rgb> {
        (x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h).then(|| self.px[y as usize * self.w + x as usize])
    }

    /// Blend towards `c` by `a` % (fog, tints, light).
    pub fn blend(&mut self, x: i64, y: i64, c: Rgb, a: u32) {
        if let Some(o) = self.get(x, y) {
            self.set(x, y, mix(o, c, a));
        }
    }

    pub fn rect(&mut self, x: i64, y: i64, w: i64, h: i64, c: Rgb) {
        for yy in y..y + h {
            for xx in x..x + w {
                self.set(xx, yy, c);
            }
        }
    }

    /// Nearest-neighbour upscale: pixel art stays crisp.
    pub fn scaled(&self, s: usize) -> Pixmap {
        if s <= 1 {
            return self.clone();
        }
        let mut out = Pixmap::new(self.w * s, self.h * s, Rgb::BLACK);
        for y in 0..out.h {
            let row = &self.px[(y / s) * self.w..(y / s + 1) * self.w];
            for x in 0..out.w {
                out.px[y * out.w + x] = row[x / s];
            }
        }
        out
    }

    /// Half-block characters: one cell = one pixel wide, two pixels tall. Samples to fit `r`.
    pub fn to_cells(&self, c: &mut Canvas, r: Rect) {
        for cy in 0..r.h as usize {
            for cx in 0..r.w as usize {
                let sx = cx * self.w / r.w.max(1) as usize;
                let ty = (cy * 2) * self.h / (r.h as usize * 2).max(1);
                let by = (cy * 2 + 1) * self.h / (r.h as usize * 2).max(1);
                let (top, bottom) = (self.px[ty.min(self.h - 1) * self.w + sx], self.px[by.min(self.h - 1) * self.w + sx]);
                c.pixels(r.x + cx as u16, r.y + cy as u16, top, bottom);
            }
        }
    }

    pub fn fingerprint(&self) -> u64 {
        self.px.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, p| {
            let v = (p.0 as u64) << 16 | (p.1 as u64) << 8 | p.2 as u64;
            (h ^ v).wrapping_mul(0x0100_0000_01b3)
        })
    }
}

pub fn mix(a: Rgb, b: Rgb, pct: u32) -> Rgb {
    let f = |x: u8, y: u8| ((x as u32 * (100 - pct.min(100)) + y as u32 * pct.min(100)) / 100) as u8;
    Rgb(f(a.0, b.0), f(a.1, b.1), f(a.2, b.2))
}

/// A deterministic hash for procedural art (same picture every run).
pub fn noise(a: i64, b: i64, seed: u64) -> u64 {
    let mut z = seed ^ (a as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (b as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

// ------------------------------------------------------------------ sprites

/// An animated sprite: frames of palette characters; '.' is transparent.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sprite {
    #[serde(default = "fps")]
    pub fps: u32,
    pub frames: Vec<Vec<String>>,
}

fn fps() -> u32 {
    6
}

impl Sprite {
    pub fn size(&self) -> (usize, usize) {
        let f = self.frames.first();
        let h = f.map_or(0, |f| f.len());
        let w = f.and_then(|f| f.iter().map(|r| r.chars().count()).max()).unwrap_or(0);
        (w, h)
    }

    /// Draws frame `n` with its bottom-left at (x, y); `flip` mirrors it; `shade` % brightness.
    #[allow(clippy::too_many_arguments)]
    pub fn blit(&self, pm: &mut Pixmap, palette: &BTreeMap<char, (u8, u8, u8)>, n: usize, x: i64, y: i64, flip: bool, shade: u32) {
        let Some(frame) = self.frames.get(n % self.frames.len().max(1)) else { return };
        let (w, h) = self.size();
        for (row, line) in frame.iter().enumerate() {
            for (col, ch) in line.chars().enumerate() {
                let Some(&(r, g, b)) = palette.get(&ch) else { continue };
                let cx = if flip { w - 1 - col } else { col };
                pm.set(x + cx as i64, y - h as i64 + 1 + row as i64, Rgb(r, g, b).shade(shade));
            }
        }
    }
}

// ------------------------------------------------------------------ backdrop layers

/// A procedural backdrop layer: designed by parameters, the same every run.
#[derive(Clone, Debug)]
pub enum Backdrop {
    /// A ridge line: colour, height in pixels above the ground, roughness (1 smooth .. 8 jagged), seed.
    Hills((u8, u8, u8), i64, i64, u64),
    /// Tree silhouettes: colour, height, spacing in pixels, seed.
    Trees((u8, u8, u8), i64, i64, u64),
    /// Clouds: colour, height of the band from the top, how many per 100 pixels, seed.
    Clouds((u8, u8, u8), i64, i64, u64),
}

impl Backdrop {
    /// Draws the layer with the camera at `cam` (pixels, already scaled by the layer's speed).
    /// `ground` is the y of the ground line; `tint` blends the layer towards the sky (distance, seasons).
    pub fn draw(&self, pm: &mut Pixmap, cam: i64, ground: i64, tint: Rgb, tint_pct: u32, season: Season) {
        match *self {
            Backdrop::Hills(c, height, rough, seed) => {
                let col = season.recolor(mix(Rgb(c.0, c.1, c.2), tint, tint_pct), false);
                for sx in 0..pm.w as i64 {
                    let wx = sx + cam;
                    // Two octaves of smoothed noise: broad swells and small bumps.
                    let broad = smooth(wx, 48, seed) * height / 100;
                    let fine = smooth(wx, 9, seed ^ 7) * rough / 100;
                    let top = ground - broad - fine;
                    for y in top.max(0)..ground {
                        let shade = if y == top { 112 } else { 100 };
                        pm.set(sx, y, col.shade(shade));
                    }
                    if season == Season::Winter && height > 6 {
                        pm.set(sx, top, mix(col, Rgb(235, 240, 250), 80));
                    }
                }
            }
            Backdrop::Trees(c, height, spacing, seed) => {
                let base = mix(Rgb(c.0, c.1, c.2), tint, tint_pct);
                let first = (cam / spacing - 1) * spacing;
                let mut wx = first;
                while wx < cam + pm.w as i64 + spacing {
                    let n = noise(wx / spacing, 3, seed);
                    let h = height * (70 + (n % 31) as i64) / 100;
                    let x = wx - cam + (n % spacing as u64) as i64 / 2;
                    let crown = season.recolor(base, n.is_multiple_of(3));
                    // Trunk.
                    pm.rect(x, ground - h / 3, 1, h / 3, mix(Rgb(60, 45, 35), tint, tint_pct));
                    // A round crown, lumpy by the noise.
                    let r = (h * 2 / 5).max(2);
                    for dy in -r..=r {
                        for dx in -r..=r {
                            let bump = (noise(dx + x, dy, n) % 3) as i64 - 1;
                            if dx * dx + dy * dy <= (r + bump) * (r + bump) && !(season == Season::Winter && n.is_multiple_of(2)) {
                                let light = if dx < 0 && dy < 0 { 115 } else { 95 };
                                pm.set(x + dx, ground - h / 3 - r + dy, crown.shade(light));
                            }
                        }
                    }
                    if season == Season::Winter && n.is_multiple_of(2) {
                        // Bare branches.
                        for i in 0..r {
                            pm.set(x - i / 2, ground - h / 3 - i, Rgb(70, 55, 45));
                            pm.set(x + i / 2, ground - h / 3 - i, Rgb(70, 55, 45));
                        }
                    }
                    wx += spacing;
                }
            }
            Backdrop::Clouds(c, band, per100, seed) => {
                let col = mix(Rgb(c.0, c.1, c.2), tint, tint_pct / 2);
                let cell = (100 / per100.max(1)).max(12);
                for k in (cam / cell - 2)..=((cam + pm.w as i64) / cell + 2) {
                    let n = noise(k, 11, seed);
                    if n.is_multiple_of(3) {
                        continue; // gaps between clouds
                    }
                    let cx = k * cell - cam + (n % cell as u64) as i64;
                    let cy = 4 + (n / 7 % band.max(1) as u64) as i64;
                    // A cloud is a few overlapping puffs on a flat base, lit from above.
                    let puffs = 2 + (n / 13 % 3) as i64;
                    for i in 0..puffs {
                        let px = cx + i * 4 - puffs * 2;
                        let r = 2 + (noise(k, i, seed) % 3) as i64;
                        for dy in -r..=0 {
                            for dx in -r..=r {
                                if dx * dx + dy * dy <= r * r {
                                    let lit = if dy < -r / 2 { 104 } else { 96 };
                                    pm.set(px + dx, cy + dy, col.shade(lit));
                                }
                            }
                        }
                    }
                    for dx in -puffs * 2 - 2..=puffs * 2 + 2 {
                        pm.set(cx + dx, cy + 1, col.shade(86));
                    }
                }
            }
        }
    }
}

/// 0..100: noise smoothed over `period` pixels (cosine-free, integer).
fn smooth(x: i64, period: i64, seed: u64) -> i64 {
    let k = x.div_euclid(period);
    let t = x.rem_euclid(period);
    let a = (noise(k, 0, seed) % 101) as i64;
    let b = (noise(k + 1, 0, seed) % 101) as i64;
    // Smoothstep in integers.
    let s = t * t * (3 * period - 2 * t) / (period * period);
    a + (b - a) * s / period
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Season {
    #[default]
    Summer,
    Autumn,
    Winter,
}

impl Season {
    pub fn from_state(s: &str) -> Season {
        if s.contains("Winter") {
            Season::Winter
        } else if s.contains("Autumn") {
            Season::Autumn
        } else {
            Season::Summer
        }
    }

    /// Foliage colour through the year; `accent` picks a second autumn hue.
    pub fn recolor(self, c: Rgb, accent: bool) -> Rgb {
        match self {
            Season::Summer => c,
            Season::Autumn => mix(c, if accent { Rgb(200, 90, 40) } else { Rgb(190, 150, 60) }, 55),
            Season::Winter => mix(c, Rgb(150, 160, 175), 55),
        }
    }

    /// Sky gradient (top, horizon), warmer with `warmth` 0..100.
    pub fn sky(self, warmth: i64) -> (Rgb, Rgb) {
        let w = warmth.clamp(0, 100) as u32;
        match self {
            Season::Summer => (mix(Rgb(70, 120, 200), Rgb(40, 110, 215), w), mix(Rgb(170, 205, 235), Rgb(250, 225, 170), w / 2)),
            Season::Autumn => (Rgb(95, 110, 160), mix(Rgb(215, 170, 130), Rgb(240, 150, 90), w)),
            Season::Winter => (Rgb(120, 135, 160), Rgb(210, 215, 225)),
        }
    }
}

// ------------------------------------------------------------------ kitty graphics protocol

/// Sends `pm` as an image placed at cell (col, row) filling `cols × rows` cells. Same `id` replaces the previous
/// frame without flicker. Zlib-compressed RGBA, chunked base64, no terminal replies (q=2), cursor kept (C=1).
pub fn kitty(out: &mut impl Write, pm: &Pixmap, id: u32, col: u16, row: u16, cols: u16, rows: u16) -> io::Result<()> {
    let mut raw = Vec::with_capacity(pm.px.len() * 3);
    for p in &pm.px {
        raw.extend_from_slice(&[p.0, p.1, p.2]);
    }
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    z.write_all(&raw)?;
    let data = base64(&z.finish()?);
    write!(out, "\x1b[{};{}H", row + 1, col + 1)?;
    let chunks: Vec<&[u8]> = data.as_bytes().chunks(4096).collect();
    for (i, chunk) in chunks.iter().enumerate() {
        let more = u8::from(i + 1 < chunks.len());
        if i == 0 {
            write!(out, "\x1b_Ga=T,f=24,o=z,s={},v={},i={id},p=1,c={cols},r={rows},C=1,q=2,z=-1,m={more};", pm.w, pm.h)?;
        } else {
            write!(out, "\x1b_Gm={more};")?;
        }
        out.write_all(chunk)?;
        out.write_all(b"\x1b\\")?;
    }
    Ok(())
}

/// Writes a PNG (RGB, 8-bit): for looking at a frame outside the terminal.
pub fn png(pm: &Pixmap) -> io::Result<Vec<u8>> {
    fn crc(data: &[u8]) -> u32 {
        let mut c = 0xffff_ffffu32;
        for b in data {
            c ^= *b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc(&body).to_be_bytes());
    }
    let mut raw = Vec::with_capacity(pm.h * (pm.w * 3 + 1));
    for y in 0..pm.h {
        raw.push(0);
        for p in &pm.px[y * pm.w..(y + 1) * pm.w] {
            raw.extend_from_slice(&[p.0, p.1, p.2]);
        }
    }
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(&raw)?;
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(pm.w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(pm.h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z.finish()?);
    chunk(&mut out, b"IEND", &[]);
    Ok(out)
}

/// Removes an image (when leaving the view).
pub fn kitty_delete(out: &mut impl Write, id: u32) -> io::Result<()> {
    write!(out, "\x1b_Ga=d,d=I,i={id},q=2\x1b\\")
}

/// Does this terminal speak the kitty graphics protocol?
pub fn kitty_supported() -> bool {
    let env = |k: &str| std::env::var(k).unwrap_or_default();
    std::env::var_os("KITTY_WINDOW_ID").is_some()
        || env("TERM").contains("kitty")
        || env("TERM").contains("ghostty")
        || matches!(env("TERM_PROGRAM").as_str(), "ghostty" | "WezTerm")
}

pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        s.push(T[(n >> 18) as usize & 63] as char);
        s.push(T[(n >> 12) as usize & 63] as char);
        s.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        s.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
    }

    #[test]
    fn kitty_frames_are_chunked_compressed_and_replace_by_id() {
        let pm = Pixmap::new(64, 64, Rgb(10, 20, 30));
        let mut out = Vec::new();
        kitty(&mut out, &pm, 7, 2, 3, 10, 5).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.starts_with("\x1b[4;3H\x1b_Ga=T,f=24,o=z,s=64,v=64,i=7,p=1,c=10,r=5,C=1,q=2"), "{}", &s[..80]);
        assert!(s.ends_with("\x1b\\") && s.contains("m=0;"), "last chunk closes the image");
    }

    #[test]
    fn png_has_a_valid_signature_and_header() {
        let png = png(&Pixmap::new(3, 2, Rgb::WHITE)).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&png[12..16], b"IHDR");
        assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 3);
    }

    #[test]
    fn upscaling_keeps_pixels_crisp() {
        let mut p = Pixmap::new(2, 1, Rgb::BLACK);
        p.set(1, 0, Rgb::WHITE);
        let s = p.scaled(3);
        assert_eq!((s.w, s.h), (6, 3));
        assert_eq!(s.get(2, 2), Some(Rgb::BLACK));
        assert_eq!(s.get(3, 0), Some(Rgb::WHITE));
    }

    #[test]
    fn a_sprite_frame_blits_with_transparency() {
        let palette = [('k', (1, 2, 3))].into_iter().collect();
        let sprite = Sprite { fps: 6, frames: vec![vec![".k".into(), "k.".into()]] };
        let mut p = Pixmap::new(2, 2, Rgb::WHITE);
        sprite.blit(&mut p, &palette, 0, 0, 1, false, 100);
        assert_eq!(p.px, vec![Rgb::WHITE, Rgb(1, 2, 3), Rgb(1, 2, 3), Rgb::WHITE]);
    }
}

#[cfg(test)]
mod bench {
    use super::*;

    /// `cargo test --release -p sim-render bench_frame_costs -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn bench_frame_costs() {
        // A diorama-sized picture with varied pixels (like the real art).
        let mut art = Pixmap::new(129, 84, Rgb::BLACK);
        for (i, p) in art.px.iter_mut().enumerate() {
            let n = noise(i as i64, 0, 1);
            *p = Rgb((n % 256) as u8 / 8 * 8, (n / 256 % 256) as u8 / 8 * 8, 60);
        }
        let t = |f: &mut dyn FnMut()| {
            let s = std::time::Instant::now();
            for _ in 0..50 {
                f();
            }
            s.elapsed().as_secs_f64() * 1000.0 / 50.0
        };
        let scaled = art.scaled(7);
        let up = t(&mut || drop(art.scaled(7)));
        let mut size7 = 0;
        let send7 = t(&mut || {
            let mut out = Vec::new();
            kitty(&mut out, &scaled, 1, 0, 0, 100, 40).unwrap();
            size7 = out.len();
        });
        let mut size1 = 0;
        let send1 = t(&mut || {
            let mut out = Vec::new();
            kitty(&mut out, &art, 1, 0, 0, 100, 40).unwrap();
            size1 = out.len();
        });
        println!("upscale x7 on the CPU:        {up:6.2} ms");
        println!("encode+compress 903x588 (x7): {send7:6.2} ms, {:>7} bytes to the terminal", size7);
        println!("encode+compress 129x84 (x1):  {send1:6.2} ms, {:>7} bytes to the terminal", size1);
    }
}
