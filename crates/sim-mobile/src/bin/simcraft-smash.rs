//! SMASH from the command line: evals and pictures, no window.
//!
//!   simcraft-smash check [--dir DIR]                                      the tuning parses and makes sense
//!   simcraft-smash eval [--save NAME] [--check] [--set path=value]...   measure every scripted shot (games/smash)
//!   simcraft-smash shot OUT_DIR [--shot NAME] [--level NAME] [--at T,T,...] [--size WxH] [--set path=value]...
//!       frames of one scripted shot at T seconds from the release (negative: while aiming), as PNGs plus a
//!       contact sheet (sheet.png)

#[cfg(any(target_os = "ios", target_os = "android"))]
fn main() {}

#[cfg(not(any(target_os = "ios", target_os = "android")))]
fn main() {
    if let Err(e) = desktop::main() {
        eprintln!("simcraft-smash: {e}");
        std::process::exit(2);
    }
}

#[cfg(not(any(target_os = "ios", target_os = "android")))]
mod desktop {
    use std::path::{Path, PathBuf};

    use sim_mobile::draw::Renderer;
    use sim_mobile::layer::{Color, Frame};
    use sim_mobile::playground::{Card, Layout};
    use sim_mobile::smash::eval::{self, Report};
    use sim_mobile::smash::tuning::Tuning;

    pub fn main() -> Result<(), String> {
        let mut sets = Vec::new();
        let mut rest = Vec::new();
        let mut it = std::env::args().skip(1);
        while let Some(a) = it.next() {
            if a == "--set" {
                sets.push(it.next().ok_or("--set needs path=value")?);
            } else {
                rest.push(a);
            }
        }
        let dir = PathBuf::from(opt(&rest, "--dir").unwrap_or_else(|| "games/smash".into()));
        let src = std::fs::read_to_string(dir.join("smash.ron")).map_err(|e| format!("{}: {e}", dir.join("smash.ron").display()))?;
        let t = match Tuning::parse(&src, &sets) {
            Ok(t) => t,
            Err(e) if rest.first().map(String::as_str) == Some("check") => {
                println!("error {}: {e}", dir.display());
                return Ok(());
            }
            Err(e) => return Err(e),
        };
        match rest.first().map(String::as_str) {
            Some("check") => {
                for p in t.problems() {
                    println!("error {}: {p}", dir.display());
                }
                Ok(())
            }
            Some("eval") => run_eval(&dir, &t, &rest, &sets),
            Some("shot") => shot(&t, &rest),
            _ => {
                Err("usage: simcraft-smash eval [--save NAME] [--check] [--set path=value] | shot OUT_DIR [--shot NAME] [--at T,..]".into())
            }
        }
    }

    fn opt(args: &[String], key: &str) -> Option<String> {
        args.iter().position(|a| a == key).and_then(|i| args.get(i + 1).cloned())
    }

    fn run_eval(dir: &Path, t: &Tuning, args: &[String], sets: &[String]) -> Result<(), String> {
        let goals = eval::goals(dir)?;
        let saved = eval::saved(dir);
        let last = saved.last().map(|(_, p)| eval::load_means(p)).transpose()?;
        if args.iter().any(|a| a == "--check") {
            let (_, old, old_sets) = last.ok_or("no saved step to check")?;
            let src = std::fs::read_to_string(dir.join("smash.ron")).map_err(|e| e.to_string())?;
            let t = Tuning::parse(&src, &old_sets)?;
            let r = Report::run(&t);
            let moved: Vec<String> = r
                .names()
                .into_iter()
                .filter(|k| !Report::timed(k))
                .filter(|k| old.get(*k).is_none_or(|o| (o - r.mean(k)).abs() > 1e-9_f64.max(o.abs() * 1e-6)))
                .map(|k| format!("{k}: {} → {}", old.get(k).copied().unwrap_or(f64::NAN), r.mean(k)))
                .collect();
            if moved.is_empty() {
                println!("smash: identical to step {:03}", saved.last().map_or(0, |s| s.0));
                return Ok(());
            }
            return Err(format!("moved since the last saved step:\n  {}", moved.join("\n  ")));
        }
        let r = Report::run(t);
        println!("{}", eval::table(&r, &goals, last.as_ref().map(|l| &l.1)));
        if let Some(name) = opt(args, "--save") {
            let step = saved.last().map_or(0, |s| s.0 + 1);
            let slug: String = name.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
            let path = dir.join("evals").join(format!("{step:03}-{}.json", slug.trim_matches('-')));
            std::fs::create_dir_all(dir.join("evals")).map_err(|e| e.to_string())?;
            std::fs::write(&path, r.json(step, &name, sets)).map_err(|e| e.to_string())?;
            println!("\nsaved {}", path.display());
        }
        Ok(())
    }

    fn shot(t: &Tuning, args: &[String]) -> Result<(), String> {
        let out = PathBuf::from(args.get(1).ok_or("shot needs an output folder")?);
        std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        let name = opt(args, "--shot").unwrap_or_else(|| "center_full".into());
        let script = eval::scripts(t).into_iter().find(|s| s.name == name).ok_or_else(|| format!("no shot {name}"))?;
        let mut want: Vec<f32> = opt(args, "--at")
            .unwrap_or_else(|| "-0.3,0,0.15,0.3,0.45,0.7,1.0,1.6,2.5".into())
            .split(',')
            .map(|s| s.trim().parse::<f32>().map_err(|e| format!("--at {s}: {e}")))
            .collect::<Result<_, _>>()?;
        let (w, h) = match opt(args, "--size") {
            Some(s) => {
                let (a, b) = s.split_once('x').ok_or("--size WxH")?;
                (a.parse::<u32>().map_err(|e| e.to_string())?, b.parse::<u32>().map_err(|e| e.to_string())?)
            }
            None => (590, 1278),
        };
        let instance = wgpu::Instance::default();
        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let mut r = pollster::block_on(Renderer::headless(&instance, format))?;
        let layout = Layout::new(w as f32, h as f32, w as f32 / 393.0);
        let mut frames: Vec<image::RgbaImage> = Vec::new();
        want.sort_by(f32::total_cmp);
        let mut next = 0;
        let mut err = None;
        let mut watch = |g: &sim_mobile::smash::Smash, _tick: u64, since: f32| {
            if next < want.len() && since >= want[next] - 1e-4 {
                let mut frame = Frame::default();
                Card::draw(g, 1.0, &layout.fit, &mut frame);
                let scene = g.scene(1.0, &layout);
                match render(&mut r, w, h, scene.as_ref(), &frame.sorted()) {
                    Ok(img) => frames.push(img),
                    Err(e) => err = Some(e),
                }
                next += 1;
            }
        };
        let level = opt(args, "--level").unwrap_or_else(|| eval::EVAL_LEVEL.into());
        eval::run_on(t, &level, &script, &layout, Some(&mut watch));
        if let Some(e) = err {
            return Err(e);
        }
        for (i, f) in frames.iter().enumerate() {
            f.save(out.join(format!("{name}_{i:02}.png"))).map_err(|e| e.to_string())?;
        }
        // A contact sheet: every frame side by side at half size.
        let (tw, th) = (w / 2, h / 2);
        let mut sheet = image::RgbaImage::from_pixel(tw * frames.len().max(1) as u32, th, image::Rgba([0, 0, 0, 255]));
        for (i, f) in frames.iter().enumerate() {
            let small = image::imageops::resize(f, tw, th, image::imageops::FilterType::Triangle);
            image::imageops::overlay(&mut sheet, &small, (i as u32 * tw) as i64, 0);
        }
        sheet.save(out.join("sheet.png")).map_err(|e| e.to_string())?;
        println!("{} frames of {name} at {:?} s → {}", frames.len(), want, out.display());
        Ok(())
    }

    fn render(
        r: &mut Renderer,
        w: u32,
        h: u32,
        scene: Option<&sim_mobile::draw3d::Scene3>,
        shapes: &[sim_mobile::layer::Shape],
    ) -> Result<image::RgbaImage, String> {
        let tex = r.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shot"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: r.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        r.draw(&view, w, h, Color::hex(0x1b1440).0, scene, shapes);
        let row = (w * 4).div_ceil(256) * 256;
        let buf = r.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("shot read"),
            size: (row * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = r.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("shot") });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        r.queue.submit([enc.finish()]);
        let slice = buf.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        r.device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| e.to_string())?;
        let data = slice.get_mapped_range();
        let mut img = image::RgbaImage::new(w, h);
        for y in 0..h {
            let src = &data[(y * row) as usize..(y * row + w * 4) as usize];
            img.as_mut()[(y * w * 4) as usize..((y + 1) * w * 4) as usize].copy_from_slice(src);
        }
        Ok(img)
    }
}
