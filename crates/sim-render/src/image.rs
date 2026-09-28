//! Images for asset packs: PNG in and out, and `pixelate`, which turns any picture (for example one generated with
//! Higgsfield) into true pixel art: key out a background colour, crop to the content, downscale by area average,
//! reduce to a small palette (median cut). Deterministic: the same source gives the same pixels.

use std::io::{BufWriter, Cursor};
use std::path::Path;

use crate::canvas::Rgb;

/// RGBA, 8 bits per channel. Alpha 0 is transparent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub w: usize,
    pub h: usize,
    pub px: Vec<[u8; 4]>,
}

impl Image {
    pub fn get(&self, x: usize, y: usize) -> [u8; 4] {
        self.px[y * self.w + x]
    }

    pub fn load(path: &Path) -> Result<Image, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut dec = png::Decoder::new(Cursor::new(bytes));
        dec.set_transformations(png::Transformations::normalize_to_color8() | png::Transformations::ALPHA);
        let mut reader = dec.read_info().map_err(|e| format!("{}: {e}", path.display()))?;
        let mut buf = vec![0; reader.output_buffer_size().ok_or("image too large")?];
        let info = reader.next_frame(&mut buf).map_err(|e| format!("{}: {e}", path.display()))?;
        let (w, h) = (info.width as usize, info.height as usize);
        let channels = info.line_size / w;
        let px = (0..w * h)
            .map(|i| {
                let p = &buf[i * channels..];
                match channels {
                    4 => [p[0], p[1], p[2], p[3]],
                    3 => [p[0], p[1], p[2], 255],
                    2 => [p[0], p[0], p[0], p[1]],
                    _ => [p[0], p[0], p[0], 255],
                }
            })
            .collect();
        Ok(Image { w, h, px })
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut enc = png::Encoder::new(BufWriter::new(file), self.w as u32, self.h as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().map_err(|e| e.to_string())?;
        let data: Vec<u8> = self.px.iter().flatten().copied().collect();
        writer.write_image_data(&data).map_err(|e| e.to_string())
    }

    /// Makes pixels close to `key` transparent (e.g. a magenta generation background).
    pub fn key(&mut self, key: Rgb, tolerance: i32) {
        for p in &mut self.px {
            let d = (p[0] as i32 - key.0 as i32).abs() + (p[1] as i32 - key.1 as i32).abs() + (p[2] as i32 - key.2 as i32).abs();
            if d <= tolerance {
                p[3] = 0;
            }
        }
    }

    /// Crops away fully transparent rows and columns at the edges.
    pub fn crop(&self) -> Image {
        let opaque = |x: usize, y: usize| self.get(x, y)[3] > 0;
        let rows: Vec<usize> = (0..self.h).filter(|&y| (0..self.w).any(|x| opaque(x, y))).collect();
        let cols: Vec<usize> = (0..self.w).filter(|&x| (0..self.h).any(|y| opaque(x, y))).collect();
        let (Some(&y0), Some(&y1), Some(&x0), Some(&x1)) = (rows.first(), rows.last(), cols.first(), cols.last()) else {
            return self.clone();
        };
        let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
        Image { w, h, px: (0..w * h).map(|i| self.get(x0 + i % w, y0 + i / w)).collect() }
    }

    /// Area-average downscale to `w × h`. A target pixel is transparent when most of its area is.
    pub fn downscale(&self, w: usize, h: usize) -> Image {
        let mut px = Vec::with_capacity(w * h);
        for ty in 0..h {
            for tx in 0..w {
                let (x0, x1) = (tx * self.w / w, ((tx + 1) * self.w / w).max(tx * self.w / w + 1));
                let (y0, y1) = (ty * self.h / h, ((ty + 1) * self.h / h).max(ty * self.h / h + 1));
                let (mut sum, mut n, mut total) = ([0u64; 3], 0u64, 0u64);
                for y in y0..y1.min(self.h) {
                    for x in x0..x1.min(self.w) {
                        let p = self.get(x, y);
                        total += 1;
                        if p[3] > 0 {
                            sum[0] += p[0] as u64;
                            sum[1] += p[1] as u64;
                            sum[2] += p[2] as u64;
                            n += 1;
                        }
                    }
                }
                px.push(if n * 2 < total || n == 0 {
                    [0, 0, 0, 0]
                } else {
                    [(sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8, 255]
                });
            }
        }
        Image { w, h, px }
    }

    /// Reduces opaque pixels to at most `n` colours (median cut, then nearest colour).
    pub fn quantize(&mut self, n: usize) {
        let colors: Vec<[u8; 3]> = self.px.iter().filter(|p| p[3] > 0).map(|p| [p[0], p[1], p[2]]).collect();
        if colors.is_empty() {
            return;
        }
        let mut boxes = vec![colors];
        while boxes.len() < n {
            // Split the box with the widest channel range.
            let range = |b: &Vec<[u8; 3]>, c: usize| {
                let (lo, hi) = b.iter().fold((255u8, 0u8), |(lo, hi), p| (lo.min(p[c]), hi.max(p[c])));
                hi.saturating_sub(lo)
            };
            let Some((i, c)) = boxes
                .iter()
                .enumerate()
                .flat_map(|(i, _)| (0..3).map(move |c| (i, c)))
                .filter(|&(i, _)| boxes[i].len() > 1)
                .max_by_key(|&(i, c)| (range(&boxes[i], c), std::cmp::Reverse(i)))
            else {
                break;
            };
            let mut b = boxes.swap_remove(i);
            b.sort_by_key(|p| (p[c], p[0], p[1], p[2]));
            let second = b.split_off(b.len() / 2);
            boxes.push(b);
            boxes.push(second);
        }
        let palette: Vec<[u8; 3]> = boxes
            .iter()
            .map(|b| {
                let s = b.iter().fold([0u64; 3], |s, p| [s[0] + p[0] as u64, s[1] + p[1] as u64, s[2] + p[2] as u64]);
                let n = b.len() as u64;
                [(s[0] / n) as u8, (s[1] / n) as u8, (s[2] / n) as u8]
            })
            .collect();
        for p in self.px.iter_mut().filter(|p| p[3] > 0) {
            let best = palette
                .iter()
                .min_by_key(|c| {
                    let d = |a: u8, b: u8| (a as i32 - b as i32).pow(2);
                    d(c[0], p[0]) + d(c[1], p[1]) + d(c[2], p[2])
                })
                .expect("palette is not empty");
            *p = [best[0], best[1], best[2], 255];
        }
    }
}

/// Everything at once: key the background, crop, scale to `height` pixels (width by aspect), `colors` colours.
pub fn pixelate(src: &Image, key: Option<Rgb>, height: usize, colors: usize) -> Image {
    let mut img = src.clone();
    if let Some(k) = key {
        img.key(k, 150);
    }
    let img = img.crop();
    let w = (img.w * height / img.h.max(1)).max(1);
    let mut out = img.downscale(w, height);
    out.quantize(colors);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(w: usize, h: usize, f: impl Fn(usize, usize) -> [u8; 4]) -> Image {
        Image { w, h, px: (0..w * h).map(|i| f(i % w, i / w)).collect() }
    }

    #[test]
    fn pixelate_keys_crops_scales_and_limits_colours() {
        // A 40x40 picture: magenta above, a gradient block below.
        let src = img(40, 40, |x, y| if y < 20 { [255, 0, 255, 255] } else { [(x * 6) as u8, 100, (y * 3) as u8, 255] });
        let out = pixelate(&src, Some(Rgb(255, 0, 255)), 5, 3);
        assert_eq!((out.w, out.h), (10, 5), "the magenta half is cropped away, the rest scaled to 5 rows");
        let distinct: std::collections::BTreeSet<[u8; 4]> = out.px.iter().copied().collect();
        assert!(distinct.len() <= 3, "{} colours", distinct.len());
        assert!(out.px.iter().all(|p| p[3] == 255));
    }

    #[test]
    fn png_round_trips_with_transparency() {
        let a = img(3, 2, |x, y| [x as u8 * 50, y as u8 * 90, 7, if x == 1 { 0 } else { 255 }]);
        let path = std::env::temp_dir().join("simcraft-image-test.png");
        a.save(&path).unwrap();
        assert_eq!(Image::load(&path).unwrap(), a);
    }
}
