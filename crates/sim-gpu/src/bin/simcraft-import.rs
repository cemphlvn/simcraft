//! simcraft-import: a generated picture becomes a game texture (the import step, like a texture's import settings).
//!
//!   simcraft-import IN.png OUT.png [--max 1024] [--crop] [--bottom] [--white] [--tile PCT]
//!
//! `--crop` trims fully transparent edges (a cut-out sprite); `--bottom` also trims transparent rows above
//! and keeps the full width (a scenery band standing on its bottom edge). The longest side is scaled down to
//! `--max` with an area average (no pixelation: HD art stays smooth; the GPU builds the mipmaps).
//! `--white`: a painting on a white ground (scenery a background remover would erase whole): near-white,
//! unsaturated pixels fade out softly, so haze at the top edge becomes a gradient into the sky.
//! `--tile PCT`: make a band tile seamlessly along x: its last PCT % is cross-faded into its start (and cut off),
//! so the right edge flows into the left one. Scenery can then repeat without a seam or a mirror.

use std::path::PathBuf;

use sim_render::image::Image;

fn main() {
    let mut args = std::env::args().skip(1);
    let (mut files, mut max, mut crop, mut bottom, mut white) = (Vec::<PathBuf>::new(), 1024usize, false, false, false);
    let mut tile = 0usize;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--max" => max = args.next().and_then(|s| s.parse().ok()).unwrap_or(max),
            "--crop" => crop = true,
            "--bottom" => bottom = true,
            "--white" => white = true,
            "--tile" => tile = args.next().and_then(|s| s.parse().ok()).unwrap_or(15),
            _ => files.push(a.into()),
        }
    }
    let [input, output] = files.as_slice() else {
        eprintln!("usage: simcraft-import IN.png OUT.png [--max 1024] [--crop] [--bottom]");
        std::process::exit(2);
    };
    let mut img = Image::load(input).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(2);
    });
    if white {
        for p in &mut img.px {
            let (lo, hi) = (p[0].min(p[1]).min(p[2]) as i32, p[0].max(p[1]).max(p[2]) as i32);
            // Whiteness from 205 (opaque) to 250 (clear), only where the colour is nearly grey.
            if hi - lo < 28 && lo > 205 {
                p[3] = (p[3] as i32 * (250 - lo).clamp(0, 45) / 45) as u8;
            }
        }
    }
    // Nearly invisible specks left by a background remover are noise: drop them.
    for p in &mut img.px {
        if p[3] < 12 {
            *p = [0, 0, 0, 0];
        }
    }
    if crop {
        img = img.crop();
    } else if bottom {
        let first = (0..img.h).find(|&y| (0..img.w).any(|x| img.get(x, y)[3] > 0)).unwrap_or(0);
        let last = (0..img.h).rev().find(|&y| (0..img.w).any(|x| img.get(x, y)[3] > 0)).unwrap_or(img.h - 1);
        let h = last + 1 - first;
        img = Image { w: img.w, h, px: img.px[first * img.w..(last + 1) * img.w].to_vec() };
    }
    if tile > 0 {
        img = seamless(&img, img.w * tile.min(45) / 100);
    }
    let s = (max as f32 / img.w.max(img.h) as f32).min(1.0);
    let (w, h) = (((img.w as f32 * s).round() as usize).max(1), ((img.h as f32 * s).round() as usize).max(1));
    let out = if s < 1.0 { downscale_alpha(&img, w, h) } else { img };
    out.save(output).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(2);
    });
    eprintln!("{} -> {} ({}x{})", input.display(), output.display(), out.w, out.h);
}

/// Cross-fades the last `o` columns into the first `o` and drops them: column 0 now continues column w-1.
fn seamless(img: &Image, o: usize) -> Image {
    let w = img.w - o;
    let mut px = Vec::with_capacity(w * img.h);
    for y in 0..img.h {
        for x in 0..w {
            let p = img.get(x, y);
            px.push(if x < o {
                let q = img.get(w + x, y);
                let t = (x as f32 + 0.5) / o as f32;
                let t = t * t * (3.0 - 2.0 * t);
                std::array::from_fn(|c| (q[c] as f32 + (p[c] as f32 - q[c] as f32) * t).round() as u8)
            } else {
                p
            });
        }
    }
    Image { w, h: img.h, px }
}

/// Area average weighted by alpha (edges keep their colour instead of darkening), alpha averaged too.
fn downscale_alpha(img: &Image, w: usize, h: usize) -> Image {
    let mut px = Vec::with_capacity(w * h);
    for ty in 0..h {
        for tx in 0..w {
            let (x0, x1) = (tx * img.w / w, ((tx + 1) * img.w / w).max(tx * img.w / w + 1));
            let (y0, y1) = (ty * img.h / h, ((ty + 1) * img.h / h).max(ty * img.h / h + 1));
            let (mut s, mut a, mut n) = ([0u64; 3], 0u64, 0u64);
            for y in y0..y1.min(img.h) {
                for x in x0..x1.min(img.w) {
                    let p = img.get(x, y);
                    let pa = p[3] as u64;
                    s[0] += p[0] as u64 * pa;
                    s[1] += p[1] as u64 * pa;
                    s[2] += p[2] as u64 * pa;
                    a += pa;
                    n += 1;
                }
            }
            px.push(match std::num::NonZeroU64::new(a) {
                None => [0, 0, 0, 0],
                Some(d) => [(s[0] / d) as u8, (s[1] / d) as u8, (s[2] / d) as u8, (a / n.max(1)) as u8],
            });
        }
    }
    Image { w, h, px }
}
