//! Pictures of the phone games played by their autopilots, no window.
//!
//!   simcraft-shot maze|gumball OUT_DIR [--level N] [--at T,T,...] [--size WxH]
//!       frames at T seconds as PNGs plus a contact sheet (sheet.png); `--level` picks MAZE's level (from 1)

#[cfg(any(target_os = "ios", target_os = "android"))]
fn main() {}

#[cfg(not(any(target_os = "ios", target_os = "android")))]
fn main() {
    if let Err(e) = desktop::main() {
        eprintln!("simcraft-shot: {e}");
        std::process::exit(2);
    }
}

#[cfg(not(any(target_os = "ios", target_os = "android")))]
mod desktop {
    use std::path::PathBuf;

    use sim_mobile::draw::Renderer;
    use sim_mobile::gumball::{self, Gumball};
    use sim_mobile::layer::{Color, Frame};
    use sim_mobile::maze::Maze;
    use sim_mobile::maze::pilot::Pilot;
    use sim_mobile::playground::{Card, Layout};
    use sim_mobile::sensors::Sense;

    const USAGE: &str = "usage: simcraft-shot maze|gumball OUT_DIR [--level N] [--at T,T,...] [--size WxH]";

    fn opt(args: &[String], name: &str) -> Option<String> {
        args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
    }

    /// A game the shot tool can play: one tick with its autopilot at the wheel.
    trait Played: Card {
        fn play(&mut self, sense: &Sense, out: &mut Vec<sim_mobile::haptics::Pulse>);
    }

    struct MazeRun(Maze, Pilot);

    impl Card for MazeRun {
        fn name(&self) -> &'static str {
            self.0.name()
        }
        fn layout(&mut self, l: &Layout) {
            self.0.layout(l);
        }
        fn draw(&self, alpha: f32, fit: &sim_mobile::layer::Fit, frame: &mut Frame) {
            self.0.draw(alpha, fit, frame);
        }
        fn scene(&self, alpha: f32, l: &Layout) -> Option<sim_mobile::draw3d::Scene3> {
            self.0.scene(alpha, l)
        }
    }

    impl Played for MazeRun {
        fn play(&mut self, sense: &Sense, out: &mut Vec<sim_mobile::haptics::Pulse>) {
            self.0.pilot = Some(self.1.tilt(&self.0));
            self.0.step(sense, out);
        }
    }

    impl Played for Gumball {
        fn play(&mut self, sense: &Sense, out: &mut Vec<sim_mobile::haptics::Pulse>) {
            self.pilot = (self.reload <= 0.0).then(|| gumball::pilot::choose(self));
            self.step(sense, out);
            self.pilot = None;
        }
    }

    pub fn main() -> Result<(), String> {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let game = args.first().ok_or(USAGE)?.clone();
        let out = PathBuf::from(args.get(1).ok_or(USAGE)?);
        std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
        let level: usize = opt(&args, "--level").map_or(Ok(1), |s| s.parse().map_err(|e| format!("--level: {e}")))?;
        let mut want: Vec<f32> = opt(&args, "--at")
            .unwrap_or_else(|| "0,1.5,3,5".into())
            .split(',')
            .map(|s| s.trim().parse::<f32>().map_err(|e| format!("--at {s}: {e}")))
            .collect::<Result<_, _>>()?;
        want.sort_by(f32::total_cmp);
        let (w, h) = match opt(&args, "--size") {
            Some(s) => {
                let (a, b) = s.split_once('x').ok_or("--size WxH")?;
                (a.parse::<u32>().map_err(|e| e.to_string())?, b.parse::<u32>().map_err(|e| e.to_string())?)
            }
            None => (590, 1278),
        };
        let mut g: Box<dyn Played> = match game.as_str() {
            "maze" => {
                let mut m = Maze::default();
                m.go(level as isize - 1);
                let p = Pilot::new(&m).ok_or("the level has no way through")?;
                Box::new(MazeRun(m, p))
            }
            "gumball" => Box::new(Gumball::default()),
            _ => return Err(USAGE.into()),
        };
        let instance = wgpu::Instance::default();
        let mut r = pollster::block_on(Renderer::headless(&instance, wgpu::TextureFormat::Rgba8UnormSrgb))?;
        let layout = Layout::new(w as f32, h as f32, w as f32 / 393.0);
        let mut frames = Vec::new();
        let mut pulses = Vec::new();
        let end = (want.last().copied().unwrap_or(0.0) * 60.0).ceil() as u64;
        for tick in 0..=end {
            g.layout(&layout);
            while frames.len() < want.len() && tick as f32 / 60.0 >= want[frames.len()] - 1e-4 {
                let mut frame = Frame::default();
                g.draw(1.0, &layout.fit, &mut frame);
                let scene = g.scene(1.0, &layout);
                let rgba = r.capture(w, h, Color::hex(0x1b1440).0, scene.as_ref(), &frame.sorted())?;
                frames.push(image::RgbaImage::from_raw(w, h, rgba).ok_or("a frame of the wrong size")?);
            }
            g.play(&Sense { gravity: None }, &mut pulses);
        }
        for (i, f) in frames.iter().enumerate() {
            f.save(out.join(format!("{game}_{i:02}.png"))).map_err(|e| e.to_string())?;
        }
        let (tw, th) = (w / 2, h / 2);
        let mut sheet = image::RgbaImage::from_pixel(tw * frames.len().max(1) as u32, th, image::Rgba([0, 0, 0, 255]));
        for (i, f) in frames.iter().enumerate() {
            let small = image::imageops::resize(f, tw, th, image::imageops::FilterType::Triangle);
            image::imageops::overlay(&mut sheet, &small, (i as u32 * tw) as i64, 0);
        }
        sheet.save(out.join("sheet.png")).map_err(|e| e.to_string())?;
        println!("{} frames of {game} at {want:?} s → {}", frames.len(), out.display());
        Ok(())
    }
}
