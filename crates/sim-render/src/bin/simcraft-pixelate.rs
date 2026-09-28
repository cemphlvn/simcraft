//! simcraft-pixelate: turn any picture into pixel art for an asset pack.
//!
//!   simcraft-pixelate IN.png OUT.png --height 24 [--colors 12] [--key ff00ff | --no-key] [--mode] [--outline PCT]
//!                     [--frames N]
//!
//! Keys out the background colour (default magenta, the colour we ask image generators for), crops to the
//! content, scales to HEIGHT pixels, reduces to COLORS colours. Same input, same output.
//! `--mode`: reduce colours first, then take each block's most common colour (crisp edges; for sprites).
//! `--outline PCT`: darken the silhouette's edge to PCT % (readable small sprites).
//! `--frames N`: IN is a sheet of N frames side by side; writes OUT_0.png .. OUT_{N-1}.png, all the same size,
//! bottoms aligned (feet on the same floor).

use std::path::PathBuf;

use sim_render::Rgb;
use sim_render::image::{Image, pixelate_with};

fn main() {
    let mut args = std::env::args().skip(1);
    let (mut files, mut height, mut colors, mut key) = (Vec::new(), 24usize, 12usize, Some(Rgb(255, 0, 255)));
    let (mut mode, mut outline, mut frames) = (false, None, 0usize);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--height" => height = args.next().and_then(|s| s.parse().ok()).unwrap_or(height),
            "--colors" => colors = args.next().and_then(|s| s.parse().ok()).unwrap_or(colors),
            "--no-key" => key = None,
            "--mode" => mode = true,
            "--outline" => outline = args.next().and_then(|s| s.parse().ok()),
            "--frames" => frames = args.next().and_then(|s| s.parse().ok()).unwrap_or(0),
            "--key" => {
                key = args.next().and_then(|s| {
                    let v = u32::from_str_radix(s.trim_start_matches('#'), 16).ok()?;
                    Some(Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
                })
            }
            _ => files.push(PathBuf::from(a)),
        }
    }
    let [input, output] = files.as_slice() else {
        eprintln!(
            "usage: simcraft-pixelate IN.png OUT.png --height 24 [--colors 12] [--key ff00ff | --no-key] [--mode] [--outline PCT] [--frames N]"
        );
        std::process::exit(2);
    };
    let src = Image::load(input).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(2);
    });
    let save = |img: &Image, path: &std::path::Path| {
        img.save(path).unwrap_or_else(|e| {
            eprintln!("{e}");
            std::process::exit(2);
        });
        eprintln!("{} ({}x{}) -> {} ({}x{}, {colors} colours)", input.display(), src.w, src.h, path.display(), img.w, img.h);
    };
    if frames == 0 {
        save(&pixelate_with(&src, key, height, colors, mode, outline), output);
        return;
    }
    // A sheet: one slice per frame, each pixelated at the same scale, padded to one size with the feet aligned.
    let mut keyed = src.clone();
    if let Some(k) = key {
        keyed.key(k, 150);
    }
    let sheet = keyed.crop();
    let slice = sheet.w / frames;
    let parts: Vec<Image> = (0..frames)
        .map(|i| Image { w: slice, h: sheet.h, px: (0..slice * sheet.h).map(|j| sheet.get(i * slice + j % slice, j / slice)).collect() })
        .collect();
    let tallest = parts.iter().map(|p| p.crop().h).max().unwrap_or(1);
    let scaled: Vec<Image> = parts
        .iter()
        .map(|p| {
            let c = p.crop();
            pixelate_with(&c, None, (height * c.h).div_ceil(tallest).max(1), colors, mode, outline)
        })
        .collect();
    let (w, h) = (scaled.iter().map(|i| i.w).max().unwrap_or(1), scaled.iter().map(|i| i.h).max().unwrap_or(1));
    let stem = output.with_extension("");
    for (i, img) in scaled.iter().enumerate() {
        let mut framed = Image { w, h, px: vec![[0, 0, 0, 0]; w * h] };
        let (ox, oy) = ((w - img.w) / 2, h - img.h);
        for y in 0..img.h {
            for x in 0..img.w {
                framed.px[(oy + y) * w + ox + x] = img.get(x, y);
            }
        }
        save(&framed, &std::path::PathBuf::from(format!("{}_{i}.png", stem.display())));
    }
}
