//! simcraft-pixelate: turn any picture into pixel art for an asset pack.
//!
//!   simcraft-pixelate IN.png OUT.png --height 24 [--colors 12] [--key ff00ff | --no-key]
//!
//! Keys out the background colour (default magenta, the colour we ask image generators for), crops to the
//! content, scales to HEIGHT pixels, reduces to COLORS colours. Same input, same output.

use std::path::PathBuf;

use sim_render::Rgb;
use sim_render::image::{Image, pixelate};

fn main() {
    let mut args = std::env::args().skip(1);
    let (mut files, mut height, mut colors, mut key) = (Vec::new(), 24usize, 12usize, Some(Rgb(255, 0, 255)));
    while let Some(a) = args.next() {
        match a.as_str() {
            "--height" => height = args.next().and_then(|s| s.parse().ok()).unwrap_or(height),
            "--colors" => colors = args.next().and_then(|s| s.parse().ok()).unwrap_or(colors),
            "--no-key" => key = None,
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
        eprintln!("usage: simcraft-pixelate IN.png OUT.png --height 24 [--colors 12] [--key ff00ff | --no-key]");
        std::process::exit(2);
    };
    let src = Image::load(input).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(2);
    });
    let out = pixelate(&src, key, height, colors);
    out.save(output).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(2);
    });
    eprintln!("{} ({}x{}) -> {} ({}x{}, {colors} colours)", input.display(), src.w, src.h, output.display(), out.w, out.h);
}
