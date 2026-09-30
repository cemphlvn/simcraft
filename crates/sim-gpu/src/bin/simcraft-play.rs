//! simcraft-play: a game on its HD stage, in a window (Metal / Vulkan / DX12), or one frame to a PNG.
//!
//!   simcraft-play games/colony3d [--stage FILE] [--seed N] [--speed X]
//!   simcraft-play games/colony3d --shot out.png [--ticks N] [--size 1600x900] [--camera close|wide|nest] [--time S]
//!   simcraft-play games/colony3d --bench N [--size 1600x900]     (N frames offscreen: where the time goes)
//!   simcraft-play games/colony3d --shot out.png --record N [--press ACTION@FRAME]...   (N frames at 30 fps: out_000.png…)
//!
//! A game with a `roam.ron` is first person in a voxel world: WASD, the mouse (click to capture it; esc lets it
//! go), shift runs, space jumps, walking into a wall climbs it, left click drops, right click digs, f smells.
//! `--shot` there looks at the tallest thing built after `--ticks`; `--feel` prints how moving feels (FEEL.md).
//! A game with a `drive.ron` is driven from the seat (c: cockpit → chase → top-down; `--view 2` starts top-down).
//! A game with a `track.ron` plays in first person (keys: its buttons', e.g. 1–4, space, Enter; c camera view;
//! p pauses; R or N starts a new run, V watches the run just ended again; `--view N` starts a shot in view N; `--replay runs/X.jsonl`
//! plays a saved run, in a window or into `--shot`/`--record`). Runs are saved to `runs/<game>-<time>.jsonl`.
//! Keys: space pause · +/- speed · tab next entity · c camera (close → wide → nest) · ←/→ pan · esc quit; the
//! stage's buttons by mouse or by their own keys. The panel is `play.toml` when the game has one (the player's
//! settings), else `engine.toml`; `--panel FILE` picks another.
//! The simulation ticks at the operator's `tick_rate` (engine.toml) times the speed; frames interpolate between
//! ticks; the camera follows on springs, so cutting between shots is a move, not a jump.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use sim_core::{Engine, EntityId, Loaded, Running, World};
use sim_gpu::drive::{Drive, DrivePlay, DrivePress};
use sim_gpu::gpu::Gpu;
use sim_gpu::gpu::World3;
use sim_gpu::math::V3;
use sim_gpu::roam::{Roam, RoamPlay, face_the_work};
use sim_gpu::stage::{ButtonState, PileMemory, Quad, card, season_of};
use sim_gpu::track::{KeyIntent, Track, TrackPlay, load_run, save_run};
use sim_gpu::walker::feel_probe;
use sim_gpu::{Camera, Composer, Stage, load_roam, load_stage, load_track};
use sim_render::Assets;
use sim_render::feel::{CameraFeel, Spring, Tween};
use sim_rules::Game;
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, KeyCode, NamedKey, PhysicalKey};
use winit::window::{CursorGrabMode, Window, WindowId};

#[derive(Clone, Copy, PartialEq, Debug)]
enum Shot {
    /// Close on the followed entity.
    Close,
    /// The whole world across the screen.
    Wide,
    /// Down in the nest, around the cut.
    Nest,
}

struct Play {
    engine: Engine<Running, Game>,
    stage: Stage,
    sizes: sim_gpu::stage::Sizes,
    tween: Tween,
    follow: Option<EntityId>,
    shot: Shot,
    springs: [Spring; 3],
    season: String,
    card: Option<(String, f32)>,
    time: f32,
    piles: PileMemory,
    buttons: Vec<ButtonState>,
    /// The last refusal, shown in the title for a moment: (action, why, when).
    refused: Option<(String, String, f32)>,
}

impl Play {
    fn new(engine: Engine<Running, Game>, stage: Stage, sizes: sim_gpu::stage::Sizes) -> Play {
        let mut p = Play {
            engine,
            stage,
            sizes,
            tween: Tween::default(),
            follow: None,
            shot: Shot::Close,
            springs: [Spring::default(); 3],
            season: String::new(),
            card: None,
            time: 0.0,
            piles: PileMemory::default(),
            buttons: Vec::new(),
            refused: None,
        };
        p.buttons = vec![ButtonState::default(); p.stage.buttons.len()];
        p.follow = p.next_follow(None);
        p.season = p.season_state();
        p
    }

    fn world(&self) -> &World {
        self.engine.world()
    }

    fn season_state(&self) -> String {
        let env = &self.stage.season_env;
        (!env.is_empty())
            .then(|| self.world().of_kind(env).next().map(|e| sim_state::active_part(&e.state).to_string()))
            .flatten()
            .unwrap_or_default()
    }

    /// The next entity of the followed kind after `after` (wrapping) that the stage shows: on the surface or
    /// underground near the cut. Else any of that kind.
    fn next_follow(&self, after: Option<EntityId>) -> Option<EntityId> {
        let plane = self.stage.plane;
        let all: Vec<(EntityId, bool)> =
            self.world().of_kind(&self.stage.follow).map(|e| (e.id, e.z == 0 || (e.y - plane).abs() <= 1)).collect();
        let shown: Vec<EntityId> = all.iter().filter(|(_, v)| *v).map(|(id, _)| *id).collect();
        let ids = if shown.is_empty() { all.iter().map(|(id, _)| *id).collect() } else { shown };
        ids.iter().copied().find(|id| after.is_none_or(|a| *id > a)).or_else(|| ids.first().copied())
    }

    fn shown(&self, id: EntityId) -> bool {
        self.world().get(id).is_some_and(|e| e.z == 0 || (e.y - self.stage.plane).abs() <= 1)
    }

    fn step(&mut self, n: u32) {
        for _ in 0..n {
            if self.engine.outcome().is_some() || self.world().tick >= self.engine.rules().cfg.run.max_ticks {
                return;
            }
            self.tween.remember(self.engine.world());
            self.engine.tick();
        }
        // The followed entity died or went out of sight (deep in the nest, off the cut): follow another.
        if self.follow.is_none_or(|id| !self.shown(id)) {
            self.follow = self.next_follow(self.follow);
        }
        let s = self.season_state();
        if s != self.season {
            self.season.clone_from(&s);
            if let Some(img) = self.stage.season_cards.get(&s) {
                self.card = Some((img.clone(), self.time));
            }
        }
    }

    /// Where the current shot wants the camera: (x, y at the horizon line, cells across).
    fn target(&self) -> (f32, f32, f32) {
        let w = self.world();
        let levels = (w.depth - 1).max(0) as f32;
        match self.shot {
            Shot::Wide => (w.width as f32 / 2.0, levels * 0.45, w.width as f32 * 1.05),
            Shot::Nest => {
                let nest =
                    w.entities().values().find(|e| e.z > 0 && !self.engine.rules().is_hidden(&e.kind) && e.kind != self.stage.follow);
                let x = nest.map_or(w.width as f32 / 2.0, |e| e.x as f32 + 0.5);
                (x, levels * 0.55, self.stage.cells_across * 0.9)
            }
            Shot::Close => {
                let (x, z) = self.follow.and_then(|id| w.get(id)).map_or((w.width as f32 / 2.0, 0.0), |e| {
                    let (x, _, z) = self.tween.at(e.id, (e.x, e.y, e.z));
                    (x + 0.5, z)
                });
                (x, if z > 0.3 { z - 0.6 } else { 0.0 }, self.stage.cells_across)
            }
        }
    }

    fn camera(&mut self, dt: f32) -> Camera {
        let (x, y, across) = self.target();
        let feel = CameraFeel { stiffness: 18.0, damping: 1.0 };
        Camera {
            x: self.springs[0].update(x, dt, feel),
            y: self.springs[1].update(y, dt, feel),
            across: self.springs[2].update(across, dt, CameraFeel { stiffness: 10.0, damping: 1.0 }),
        }
    }

    /// The entity a button acts for: the first of its kind.
    fn actor(&self, b: usize) -> Option<EntityId> {
        self.world().of_kind(&self.stage.buttons[b].on).next().map(|e| e.id)
    }

    /// Would the game take this button's action now? (The same checks as any agent, without queueing it.)
    fn allowed(&self, b: usize) -> Result<sim_core::Group, String> {
        let btn = &self.stage.buttons[b];
        let id = self.actor(b).ok_or_else(|| format!("no {} in the world", btn.on))?;
        self.engine.rules().act(self.world(), None, id, &btn.action, &btn.args)
    }

    /// The player pressed button `b`: the action is queued for the next tick, or refused (and the button says so).
    fn press(&mut self, b: usize) {
        let now = self.time;
        match self.allowed(b) {
            Ok(group) => {
                self.engine.queue(group);
                self.buttons[b].player.play("press", now);
            }
            Err(why) => {
                self.buttons[b].player.play("denied", now);
                self.refused = Some((self.stage.buttons[b].action.clone(), why, now));
            }
        }
    }

    fn hover(&mut self, w: f32, h: f32, x: f32, y: f32) {
        for (i, b) in self.stage.buttons.iter().enumerate() {
            let over = b.hit(w, h, x, y);
            let st = &mut self.buttons[i];
            match (over, st.hover_since) {
                (true, None) => st.hover_since = Some(self.time),
                (false, Some(_)) => st.hover_since = None,
                _ => {}
            }
        }
    }

    fn button_under(&self, w: f32, h: f32, x: f32, y: f32) -> Option<usize> {
        self.stage.buttons.iter().position(|b| b.hit(w, h, x, y))
    }

    fn quads(&mut self, w: f32, h: f32, dt: f32) -> Vec<Quad> {
        let cam = self.camera(dt);
        self.piles.update(&self.stage, self.engine.world(), self.time);
        for i in 0..self.stage.buttons.len() {
            self.buttons[i].enabled = self.allowed(i).is_ok();
        }
        let composer = Composer { stage: &self.stage, game: self.engine.rules(), sizes: &self.sizes, w, h };
        let mut q = composer.compose_with(self.engine.world(), &cam, Some(&self.tween), self.time, Some(&self.piles));
        for (i, b) in self.stage.buttons.iter().enumerate() {
            q.extend(b.quads(&mut self.buttons[i], w, h, self.time));
        }
        if let Some((img, at)) = &self.card {
            q.extend(card(img, &self.sizes, w, h, self.time - at, 3.2));
        }
        q
    }

    fn title(&self, speed: f32, paused: bool) -> String {
        let (season, _) = season_of(self.world(), &self.stage.season_env);
        let refused = match &self.refused {
            Some((a, why, at)) if self.time - at < 4.0 => format!(" — {a}: {why}"),
            _ => String::new(),
        };
        format!(
            "{} — tick {} — {:?} — {} — {:?} shot{refused}",
            self.engine.rules().def.name,
            self.world().tick,
            season,
            if paused { "paused".to_string() } else { format!("{speed:.0} ticks/s") },
            self.shot
        )
    }
}

/// The player's panel: `--panel`, else the game's `play.toml`, else its `engine.toml`.
fn panel_path(dir: &Path, panel: Option<&Path>) -> PathBuf {
    panel.map(Path::to_path_buf).unwrap_or_else(|| {
        let play = dir.join("play.toml");
        if play.exists() { play } else { dir.join("engine.toml") }
    })
}

fn boot(dir: &Path, seed: Option<u64>, panel: &Path) -> Result<Engine<Running, Game>, String> {
    let panel = std::fs::read_to_string(panel).map_err(|e| format!("{}: {e}", panel.display()))?;
    let panel = match seed {
        Some(s) => panel
            .lines()
            .map(|l| if l.trim_start().starts_with("seed") { format!("seed = {s}") } else { l.to_string() })
            .collect::<Vec<_>>()
            .join("\n"),
        None => panel,
    };
    let (world, game) = Game::load_panel(dir, &panel)?;
    Ok(Engine::<Loaded, _>::new(world, game).validate().map_err(|e| e.join("\n"))?.start())
}

/// Records `a.record` frames at 30 fps from the shot state: the simulation ticks at its rate, animations run, and
/// `ACTION@FRAME` presses happen on their frame (with the button's own animation). Frames: `<out>_000.png`, ...
fn record(gpu: &mut Gpu, mut play: Play, a: &Args, out: &Path) -> Result<(), String> {
    let rate = play.engine.rules().cfg.run.tick_rate as f32;
    let presses: Vec<(String, u32)> =
        a.press.iter().filter_map(|p| p.split_once('@').and_then(|(n, f)| Some((n.to_string(), f.parse().ok()?)))).collect();
    let (w, h) = a.size;
    let dt = 1.0 / 30.0;
    let mut clock = 0.0f32;
    let stem = out.with_extension("");
    play.time = a.time;
    for f in 0..a.record {
        for (name, at) in &presses {
            if *at == f
                && let Some(b) = play.stage.buttons.iter().position(|b| b.action == *name)
            {
                play.press(b);
            }
        }
        clock += dt * rate * a.speed;
        let n = clock.floor() as u32;
        clock -= n as f32;
        play.step(n);
        play.tween.alpha = clock;
        play.time += dt;
        let quads = play.quads(w as f32, h as f32, dt);
        gpu.shot(w, h, &quads).save(&PathBuf::from(format!("{}_{f:03}.png", stem.display())))?;
    }
    eprintln!("recorded {} frames to {}_NNN.png (tick {})", a.record, stem.display(), play.world().tick);
    Ok(())
}

/// Plays `a.bench` frames offscreen at 60 fps game time (ticks at the operator's rate) and reports where the
/// time goes: simulation, composing the quads, and the GPU (submit to finish).
fn bench(engine: Engine<Running, Game>, stage: Stage, assets: &Assets, a: &Args) -> Result<(), String> {
    let instance = wgpu::Instance::default();
    let mut gpu = pollster::block_on(Gpu::new(&instance, None, None))?;
    upload_all(&mut gpu, assets);
    let rate = engine.rules().cfg.run.tick_rate as f32;
    let mut play = Play::new(engine, stage, gpu.sizes.clone());
    let (w, h) = a.size;
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("bench"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: gpu.format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let (mut sim, mut compose, mut draw, mut quads_n, mut clock) = (0.0f64, 0.0f64, 0.0f64, 0usize, 0.0f32);
    let dt = 1.0 / 60.0;
    for f in 0..a.bench {
        play.time += dt;
        play.shot = match (f / 200) % 3 {
            0 => Shot::Close,
            1 => Shot::Wide,
            _ => Shot::Nest,
        };
        let t = Instant::now();
        clock += dt * rate * a.speed;
        let n = clock.floor() as u32;
        clock -= n as f32;
        play.step(n);
        play.tween.alpha = clock;
        sim += t.elapsed().as_secs_f64();
        let t = Instant::now();
        let q = play.quads(w as f32, h as f32, dt);
        compose += t.elapsed().as_secs_f64();
        quads_n = quads_n.max(q.len());
        let t = Instant::now();
        gpu.draw(&view, w, h, &q);
        let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        draw += t.elapsed().as_secs_f64();
    }
    let per = |s: f64| s * 1000.0 / a.bench as f64;
    eprintln!(
        "{} frames at {}x{}, up to {quads_n} quads: sim {:.3} ms, compose {:.3} ms, gpu {:.3} ms per frame (tick {})",
        a.bench,
        w,
        h,
        per(sim),
        per(compose),
        per(draw),
        play.world().tick
    );
    Ok(())
}

fn upload_all(gpu: &mut Gpu, assets: &Assets) {
    for (name, img) in &assets.loaded {
        gpu.upload(name, img, 2048);
    }
}

struct Args {
    dir: PathBuf,
    stage: Option<PathBuf>,
    seed: Option<u64>,
    speed: f32,
    shot: Option<PathBuf>,
    ticks: u32,
    size: (u32, u32),
    /// `--size` was given (else a track's `screen` shapes shots and the window).
    sized: bool,
    camera: Shot,
    time: f32,
    bench: u32,
    panel: Option<PathBuf>,
    /// Actions pressed before a shot (as the player would), in order; `ACTION@FRAME` presses during a recording.
    press: Vec<String>,
    record: u32,
    view: usize,
    /// A track's theme to start in (its name; `t` switches in the window).
    theme: String,
    replay: Option<PathBuf>,
    feel: bool,
    /// Drive views: the autopilot drives your car (shots of a car at speed, hands-off laps).
    auto: bool,
    /// Drive views: ride on board this car (its entity id).
    watch: Option<u64>,
    /// Roam shots: close to a crawler instead of facing the work; or a camera placed by hand.
    closeup: bool,
    eye: Vec<f32>,
    /// Frame drops: the window's budget (ms), a headless sweep (frames), a stress run (frames per level), a spike
    /// report to rebuild and profile (and how many times).
    budget: f64,
    sweep: u64,
    stress: u64,
    repro: Option<PathBuf>,
    times: u32,
}

fn args() -> Args {
    let mut a = Args {
        dir: PathBuf::from("games/colony3d"),
        stage: None,
        seed: None,
        speed: 1.0,
        shot: None,
        ticks: 0,
        size: (1600, 900),
        sized: false,
        camera: Shot::Close,
        time: 0.0,
        bench: 0,
        panel: None,
        press: Vec::new(),
        record: 0,
        view: 0,
        theme: String::new(),
        replay: None,
        feel: false,
        auto: false,
        watch: None,
        closeup: false,
        eye: Vec::new(),
        budget: 20.0,
        sweep: 0,
        stress: 0,
        repro: None,
        times: 30,
    };
    let mut it = std::env::args().skip(1);
    while let Some(x) = it.next() {
        match x.as_str() {
            "--stage" => a.stage = it.next().map(PathBuf::from),
            "--seed" => a.seed = it.next().and_then(|s| s.parse().ok()),
            "--speed" => a.speed = it.next().and_then(|s| s.parse().ok()).unwrap_or(1.0),
            "--shot" => a.shot = it.next().map(PathBuf::from),
            "--ticks" => a.ticks = it.next().and_then(|s| s.parse().ok()).unwrap_or(0),
            "--replay" => a.replay = it.next().map(PathBuf::from),
            "--feel" => a.feel = true,
            "--auto" => a.auto = true,
            "--watch" => a.watch = it.next().and_then(|s| s.parse().ok()),
            "--closeup" => a.closeup = true,
            "--eye" => a.eye = it.next().map(|s| s.split(',').filter_map(|v| v.parse().ok()).collect()).unwrap_or_default(),
            "--budget" => a.budget = it.next().and_then(|s| s.parse().ok()).unwrap_or(20.0),
            "--sweep" => a.sweep = it.next().and_then(|s| s.parse().ok()).unwrap_or(1200),
            "--stress" => a.stress = it.next().and_then(|s| s.parse().ok()).unwrap_or(300),
            "--repro" => a.repro = it.next().map(PathBuf::from),
            "--times" => a.times = it.next().and_then(|s| s.parse().ok()).unwrap_or(30),
            "--view" => a.view = it.next().and_then(|s| s.parse().ok()).unwrap_or(0),
            "--theme" => a.theme = it.next().unwrap_or_default(),
            "--record" => a.record = it.next().and_then(|s| s.parse().ok()).unwrap_or(60),
            "--panel" => a.panel = it.next().map(PathBuf::from),
            "--press" => a.press.extend(it.next()),
            "--bench" => a.bench = it.next().and_then(|s| s.parse().ok()).unwrap_or(600),
            "--time" => a.time = it.next().and_then(|s| s.parse().ok()).unwrap_or(0.0),
            "--size" => {
                if let Some((w, h)) = it.next().and_then(|s| s.split_once('x').map(|(w, h)| (w.parse().ok(), h.parse().ok()))) {
                    a.size = (w.unwrap_or(1600), h.unwrap_or(900));
                    a.sized = true;
                }
            }
            "--camera" => {
                a.camera = match it.next().as_deref() {
                    Some("wide") => Shot::Wide,
                    Some("nest") => Shot::Nest,
                    _ => Shot::Close,
                }
            }
            _ => a.dir = PathBuf::from(x),
        }
    }
    a
}

fn main() {
    let a = args();
    let run = || -> Result<(), String> {
        let panel = panel_path(&a.dir, a.panel.as_deref());
        let engine = boot(&a.dir, a.seed, &panel)?;
        if a.dir.join("roam.ron").exists() && a.stage.is_none() {
            let (roam, assets) = load_roam(&a.dir)?;
            return run_roam(engine, roam, assets, &a, &panel);
        }
        if a.dir.join("drive.ron").exists() && a.stage.is_none() {
            let (drive, track) = sim_gpu::drive::load(&a.dir, engine.rules())?;
            let (dir, seed, panel) = (a.dir.clone(), a.seed, panel);
            let reboot: Reboot = Box::new(move || boot(&dir, seed, &panel));
            return run_drive(engine, drive, track, &a, reboot);
        }
        if a.dir.join("track.ron").exists() && a.stage.is_none() {
            let (track, assets) = load_track(&a.dir)?;
            let (dir, seed, panel) = (a.dir.clone(), a.seed, panel);
            let reboot: Reboot = Box::new(move || boot(&dir, seed, &panel));
            return run_track(engine, track, assets, &a, reboot);
        }
        let (stage, assets) = load_stage(&a.dir, a.stage.as_deref())?;
        if a.bench > 0 {
            return bench(engine, stage, &assets, &a);
        }
        if let Some(out) = &a.shot {
            let instance = wgpu::Instance::default();
            let mut gpu = pollster::block_on(Gpu::new(&instance, None, None))?;
            upload_all(&mut gpu, &assets);
            let mut play = Play::new(engine, stage, gpu.sizes.clone());
            play.shot = a.camera;
            play.step(a.ticks);
            play.piles.update(&play.stage, play.engine.world(), play.time);
            if a.record > 0 {
                return record(&mut gpu, play, &a, out);
            }
            // Presses before the shot: each is queued (or refused) and one tick runs, as in play.
            for name in &a.press {
                let Some(b) = play.stage.buttons.iter().position(|b| b.action == *name) else {
                    return Err(format!("no button for action '{name}' on this stage"));
                };
                play.press(b);
                if let Some((act, why, _)) = &play.refused {
                    eprintln!("{act}: {why}");
                }
                play.step(1);
            }
            play.tween.alpha = 1.0;
            play.time = a.time;
            let t = Instant::now();
            let quads = play.quads(a.size.0 as f32, a.size.1 as f32, 0.0);
            let img = gpu.shot(a.size.0, a.size.1, &quads);
            eprintln!("{} quads, frame in {:.2} ms", quads.len(), t.elapsed().as_secs_f64() * 1000.0);
            img.save(out)?;
            eprintln!("wrote {} ({}x{}) at tick {}", out.display(), a.size.0, a.size.1, play.world().tick);
            return Ok(());
        }
        let rate = engine.rules().cfg.run.tick_rate as f32;
        let el = EventLoop::new().map_err(|e| e.to_string())?;
        el.set_control_flow(ControlFlow::Poll);
        let mut app = App {
            play: Some(Start::Stage(Box::new((engine, stage)))),
            assets,
            window: None,
            speed: rate * a.speed,
            paused: false,
            clock: 0.0,
            last: Instant::now(),
            cursor: (0.0, 0.0),
            reboot: None,
            script: None,
            saved: false,
            grabbed: false,
            theme: 0,
            music: None,
            swipe_from: None,
            engine_sound: None,
            voice: None,
            window_size: a.sized.then_some(a.size),
            watch: None,
        };
        el.run_app(&mut app).map_err(|e| e.to_string())
    };
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(2);
    }
}

/// A game in first person: a screenshot, a recording, or a window.
/// Starts the game over (a new run, or a replay from the start).
type Reboot = Box<dyn Fn() -> Result<Engine<Running, Game>, String>>;

fn run_track(engine: Engine<Running, Game>, track: Track, assets: Assets, a: &Args, reboot: Reboot) -> Result<(), String> {
    let script = a.replay.as_deref().map(load_run).transpose()?;
    let theme = match a.theme.as_str() {
        "" => 0,
        name => {
            1 + track
                .themes
                .iter()
                .position(|t| t.name == name)
                .ok_or_else(|| format!("no theme '{name}' (themes: {:?})", track.themes.iter().map(|t| &t.name).collect::<Vec<_>>()))?
        }
    };
    // How it feels, measured (FEEL.md): the run replayed headless at 60 frames a second, `--record` frames (600).
    if a.feel {
        let assets_sizes = assets.loaded.iter().map(|(n, i)| (n.clone(), (i.w as u32, i.h as u32))).collect();
        let mut play = TrackPlay::new(engine, track, assets_sizes);
        play.set_theme(theme);
        play.script = script;
        let frames = if a.record > 0 { a.record } else { 600 };
        let report = sim_gpu::track::feel_probe(&mut play, frames, 60.0);
        println!("{}", serde_json::to_string(&report).map_err(|e| e.to_string())?);
        return Ok(());
    }
    if let Some(out) = &a.shot {
        let instance = wgpu::Instance::default();
        let mut gpu = pollster::block_on(Gpu::new(&instance, None, None))?;
        upload_all(&mut gpu, &assets);
        let mut play = TrackPlay::new(engine, track, gpu.sizes.clone());
        play.set_theme(theme);
        play.script = script;
        play.replay_marks = false; // a recording of a run is the run, not a replay in a window
        play.step(a.ticks);
        // Effects of the warm-up ticks are over by the time the shot starts.
        play.rig.fx = Default::default();
        play.rig.view = a.view;
        // A track shaped like a phone is shot like one (twice its logical size), unless `--size` says otherwise.
        let (w, h) = match play.track.screen {
            Some((sw, sh)) if !a.sized => (sw * 2, sh * 2),
            _ => a.size,
        };
        let presses: Vec<(String, u32)> = a
            .press
            .iter()
            .map(|p| p.split_once('@').map_or_else(|| (p.clone(), 0), |(n, f)| (n.to_string(), f.parse().unwrap_or(0))))
            .collect();
        let frames = a.record.max(1);
        let rate = play.engine.rules().cfg.run.tick_rate as f32;
        let (dt, mut clock) = (1.0 / 30.0, 0.0f32);
        play.time = a.time;
        let stem = out.with_extension("");
        for f in 0..frames {
            for (key, at) in &presses {
                if *at == f && !play.key(key) {
                    return Err(format!("no button with key '{key}' on this track"));
                }
            }
            if a.record > 0 {
                clock += dt * rate * a.speed * play.time_scale();
                let n = clock.floor() as u32;
                clock -= n as f32;
                play.step(n);
                play.tween.alpha = clock;
                play.time += dt;
            } else {
                play.tween.alpha = 1.0;
            }
            let fr = play.frame(w as f32, h as f32, if a.record > 0 { dt } else { 0.0 });
            let world = World3::new(fr.eye.view_proj(w as f32, h as f32), fr.fog, &fr.meshes);
            let img = gpu.shot_scene(w, h, &fr.back, Some(world), &fr.front);
            let path = if a.record > 0 { PathBuf::from(format!("{}_{f:03}.png", stem.display())) } else { out.clone() };
            img.save(&path)?;
        }
        eprintln!("wrote {} ({frames} frame(s), {w}x{h}) at tick {} — {}", out.display(), play.world().tick, play.title(rate, false));
        return Ok(());
    }
    let rate = engine.rules().cfg.run.tick_rate as f32;
    let el = EventLoop::new().map_err(|e| e.to_string())?;
    el.set_control_flow(ControlFlow::Poll);
    let window_size = if a.sized { Some(a.size) } else { track.screen };
    let mut app = App {
        play: Some(Start::Track(Box::new((engine, track)))),
        assets,
        window: None,
        speed: rate * a.speed,
        paused: false,
        clock: 0.0,
        last: Instant::now(),
        cursor: (0.0, 0.0),
        reboot: Some(reboot),
        script,
        saved: false,
        grabbed: false,
        theme,
        music: Some(Music::new(&a.dir)),
        swipe_from: None,
        engine_sound: None,
        voice: None,
        window_size,
        watch: None,
    };
    el.run_app(&mut app).map_err(|e| e.to_string())
}

/// Driving from the seat (`drive.ron`): how it feels (numbers), a screenshot or a recording, or a window.
fn run_drive(engine: Engine<Running, Game>, drive: Drive, track: sim_physics::Track, a: &Args, reboot: Reboot) -> Result<(), String> {
    let script = a.replay.as_deref().map(sim_gpu::drive::load_run).transpose()?;
    // How it feels, measured: the autopilot drives you (unless a replay does) at 60 frames a second.
    if a.feel {
        let mut play = DrivePlay::new(engine, drive, track);
        play.auto_you = script.is_none();
        play.script = script;
        play.rig.view = a.view;
        play.watch = a.watch;
        let frames = if a.record > 0 { a.record } else { 2400 };
        let report = sim_gpu::drive::feel_probe(&mut play, frames, 60.0);
        println!("{}", serde_json::to_string(&report).map_err(|e| e.to_string())?);
        return Ok(());
    }
    if a.bench > 0 {
        return bench_drive(engine, drive, track, a);
    }
    if let Some(out) = &a.shot {
        let instance = wgpu::Instance::default();
        let mut gpu = pollster::block_on(Gpu::new(&instance, None, None))?;
        let (w, h) = match drive.screen {
            Some((sw, sh)) if !a.sized => (sw * 2, sh * 2),
            _ => a.size,
        };
        let mut play = DrivePlay::new(engine, drive, track);
        play.upload(&mut gpu);
        play.auto_you = a.auto && script.is_none();
        play.script = script;
        play.rig.view = a.view;
        play.watch = a.watch;
        let rate = play.engine.rules().cfg.run.tick_rate as f32;
        // Warm-up: the ticks asked for, with frames at 60 a second so the head and the horizon are where they would
        // be in a window.
        // (At least two ticks: a game places its cars on its first.)
        let warm = (a.ticks.max(2) as f32 / rate * 60.0).ceil() as u32;
        let mut clock = 0.0f32;
        for _ in 0..warm {
            clock += rate / 60.0;
            let n = clock.floor() as u32;
            clock -= n as f32;
            play.step(n);
            play.alpha = clock;
            play.time += 1.0 / 60.0;
            play.frame(w as f32, h as f32, 1.0 / 60.0);
        }
        play.time = play.time.max(a.time);
        let frames = a.record.max(1);
        let stem = out.with_extension("");
        let dt = 1.0 / 30.0;
        for f in 0..frames {
            if a.record > 0 {
                clock += dt * rate * a.speed;
                let n = clock.floor() as u32;
                clock -= n as f32;
                play.step(n);
                play.alpha = clock;
                play.time += dt;
            }
            let fr = play.frame(w as f32, h as f32, if a.record > 0 { dt } else { 1.0 / 60.0 });
            let img = play.shot(&mut gpu, w, h, &fr);
            let path = if a.record > 0 { PathBuf::from(format!("{}_{f:03}.png", stem.display())) } else { out.clone() };
            img.save(&path)?;
        }
        eprintln!("wrote {} ({frames} frame(s), {w}x{h}) at tick {} — {}", out.display(), play.world().tick, play.title(rate, false));
        return Ok(());
    }
    let rate = engine.rules().cfg.run.tick_rate as f32;
    let el = EventLoop::new().map_err(|e| e.to_string())?;
    el.set_control_flow(ControlFlow::Poll);
    let window_size = if a.sized { Some(a.size) } else { drive.screen };
    let mut app = App {
        play: Some(Start::Drive(Box::new((engine, drive, track, a.auto, script, a.view)))),
        assets: Assets::default(),
        window: None,
        speed: rate * a.speed,
        paused: false,
        clock: 0.0,
        last: Instant::now(),
        cursor: (0.0, 0.0),
        reboot: Some(reboot),
        script: None,
        saved: false,
        grabbed: false,
        theme: 0,
        music: None,
        swipe_from: None,
        engine_sound: None,
        voice: None,
        window_size,
        watch: None,
    };
    el.run_app(&mut app).map_err(|e| e.to_string())
}

/// The drive view's whole frame, GPU included (`--bench N`): the autopilot drives you (`--watch`, `--view` as for
/// shots), each frame rendered offscreen at the window's size (the mirror too) and waited for. Prints the mean of
/// each part and the frame's p99 and how many frames went over 8 ms (the budget for 120 Hz).
fn bench_drive(engine: Engine<Running, Game>, drive: Drive, track: sim_physics::Track, a: &Args) -> Result<(), String> {
    let instance = wgpu::Instance::default();
    let mut gpu = pollster::block_on(Gpu::new(&instance, None, None))?;
    let (w, h) = match drive.screen {
        Some(s) if !a.sized => s,
        _ => a.size,
    };
    let mut play = DrivePlay::new(engine, drive, track);
    play.upload(&mut gpu);
    play.auto_you = true;
    play.rig.view = a.view;
    play.watch = a.watch;
    let rate = play.engine.rules().cfg.run.tick_rate as f32;
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("bench"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: gpu.format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let (mut sim, mut compose, mut draw, mut clock) = (0.0f64, 0.0f64, 0.0f64, 0.0f32);
    let mut frames: Vec<f64> = Vec::new();
    let dt = 1.0 / 60.0;
    for f in 0..a.bench {
        let whole = Instant::now();
        let t = Instant::now();
        clock += dt * rate * a.speed;
        let n = clock.floor() as u32;
        clock -= n as f32;
        play.step(n);
        play.alpha = clock;
        play.time += dt;
        sim += t.elapsed().as_secs_f64();
        let t = Instant::now();
        let fr = play.frame(w as f32, h as f32, dt);
        compose += t.elapsed().as_secs_f64();
        let t = Instant::now();
        play.render(&mut gpu, &view, w, h, &fr);
        let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        draw += t.elapsed().as_secs_f64();
        // After the first frames (pipelines and uploads warm up).
        if f >= 30 {
            frames.push(whole.elapsed().as_secs_f64() * 1000.0);
        }
    }
    let per = |s: f64| s * 1000.0 / a.bench as f64;
    frames.sort_by(f64::total_cmp);
    let p99 = frames.get(frames.len() * 99 / 100).copied().unwrap_or(0.0);
    let over = frames.iter().filter(|m| **m > 8.0).count();
    eprintln!(
        "{} frames at {w}x{h} (view {}): sim {:.3} ms, compose {:.3} ms, gpu {:.3} ms per frame; p99 {p99:.2} ms, over 8 ms: {over}, worst {:.2} ms (tick {})",
        a.bench,
        a.view,
        per(sim),
        per(compose),
        per(draw),
        frames.last().copied().unwrap_or(0.0),
        play.world().tick
    );
    Ok(())
}

/// First person in a voxel world: how moving feels (numbers), a screenshot, or a window.
fn run_roam(engine: Engine<Running, Game>, roam: Roam, assets: Assets, a: &Args, panel: &Path) -> Result<(), String> {
    if let Some(file) = &a.repro {
        return repro_roam(engine, roam, &assets, a, file);
    }
    if a.sweep > 0 {
        return sweep_roam(&roam, &assets, a, panel);
    }
    if a.stress > 0 {
        return stress_roam(&roam, &assets, a, panel);
    }
    if a.feel {
        let report = feel_probe(roam.feel);
        println!("{}", serde_json::to_string(&report).map_err(|e| e.to_string())?);
        return Ok(());
    }
    if a.bench > 0 {
        return bench_roam(engine, roam, &assets, a);
    }
    if let Some(out) = &a.shot {
        let instance = wgpu::Instance::default();
        let mut gpu = pollster::block_on(Gpu::new(&instance, None, None))?;
        upload_all(&mut gpu, &assets);
        let mut play = RoamPlay::new(engine, roam);
        if a.camera == Shot::Wide {
            play.overview =
                Some(sim_gpu::math::Eye { pos: V3(0.0, 0.0, 0.0), target: V3(0.0, 0.0, 1.0), roll: 0.0, fov: 60.0, near: 0.1, far: 300.0 });
        }
        play.step(a.ticks);
        play.tween.alpha = 1.0;
        face_the_work(&mut play, 2);
        if a.closeup {
            sim_gpu::roam::close_up(&mut play, "termite");
        }
        // Any camera, for looking at one spot: --eye x,y,z,tx,ty,tz (view space: y up).
        if let [x, y, z, tx, ty, tz] = a.eye[..] {
            play.overview =
                Some(sim_gpu::math::Eye { pos: V3(x, y, z), target: V3(tx, ty, tz), roll: 0.0, fov: 50.0, near: 0.02, far: 300.0 });
        }
        play.smell_on = a.press.iter().any(|p| p == "f");
        // Settle the body on the ground before the picture.
        for _ in 0..30 {
            play.walk(1.0 / 60.0, Default::default());
        }
        let (w, h) = a.size;
        let fr = play.frame(w as f32, h as f32, 1.0 / 60.0);
        let world = play.world3(&mut gpu, &fr, w, h);
        gpu.shot_scene(w, h, &fr.back, Some(world), &fr.front).save(out)?;
        let tris: usize = fr.meshes.iter().chain(play.terrain().1).map(|m| m.verts.len() / 3).sum();
        eprintln!("wrote {} ({w}x{h}) at tick {}, {tris} triangles — {}", out.display(), play.world().tick, play.title());
        return Ok(());
    }
    let rate = engine.rules().cfg.run.tick_rate as f32;
    let el = EventLoop::new().map_err(|e| e.to_string())?;
    el.set_control_flow(ControlFlow::Poll);
    let mut app = App {
        play: Some(Start::Roam(Box::new((engine, roam)))),
        assets,
        window: None,
        speed: rate * a.speed,
        paused: false,
        clock: 0.0,
        last: Instant::now(),
        cursor: (0.0, 0.0),
        reboot: None,
        script: None,
        saved: false,
        grabbed: false,
        theme: 0,
        music: None,
        swipe_from: None,
        engine_sound: None,
        voice: None,
        window_size: None,
        watch: Some((
            sim_gpu::perf::Watch::new(a.budget, Some(PathBuf::from("runs/spikes"))),
            0,
            Instant::now(),
            (a.dir.clone(), panel.to_path_buf(), a.seed),
        )),
    };
    el.run_app(&mut app).map_err(|e| e.to_string())
}

fn headless_gpu(assets: &Assets) -> Result<Gpu, String> {
    let instance = wgpu::Instance::default();
    let mut gpu = pollster::block_on(Gpu::new(&instance, None, None))?;
    upload_all(&mut gpu, assets);
    Ok(gpu)
}

/// A spike report, rebuilt: the same state (checked by its hash), the same camera; the frame rendered `a.times`
/// times, first cold (terrain not built yet, as after a rebuild) then warm, and the ticks before it timed again.
fn repro_roam(engine: Engine<Running, Game>, roam: Roam, assets: &Assets, a: &Args, file: &Path) -> Result<(), String> {
    use sim_gpu::perf::{Spike, replay};
    let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let spike: Spike = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", file.display()))?;
    eprintln!("spike: frame {} at tick {}, {:.2} ms: {}", spike.record.frame, spike.record.tick, spike.record.work_ms, spike.cause);
    let mut play = replay(&spike, engine, roam)?;
    eprintln!("state rebuilt: tick {}, hash {} (the same)", play.world().tick, spike.hash);
    let mut gpu = headless_gpu(assets)?;
    let (w, h) = a.size;
    let target = sim_gpu::perf::offscreen(&gpu, w, h);
    let mut runs = Vec::new();
    for i in 0..a.times.max(2) {
        play.stats = Default::default();
        let t = Instant::now();
        let fr = play.frame(w as f32, h as f32, 0.0);
        let build = t.elapsed().as_secs_f64() * 1000.0;
        let t = Instant::now();
        play.render(&mut gpu, &target, w, h, &fr);
        let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        let draw = t.elapsed().as_secs_f64() * 1000.0;
        if i == 0 {
            eprintln!(
                "cold (terrain built now): build {build:.2} ms (terrain {:.2} ms, {} vertices), draw {draw:.2} ms",
                play.stats.terrain_ms, play.stats.terrain_verts
            );
        } else {
            runs.push((build, draw, play.stats.verts, play.stats.bodies));
        }
    }
    let med = |mut v: Vec<f64>| {
        v.sort_by(f64::total_cmp);
        v[v.len() / 2]
    };
    let (b, d) = (med(runs.iter().map(|r| r.0).collect()), med(runs.iter().map(|r| r.1).collect()));
    eprintln!("warm, median of {}: build {b:.2} ms, draw {d:.2} ms, {} vertices, {} bodies", runs.len(), runs[0].2, runs[0].3);
    // What every dig or drop costs once the GPU is warm: the terrain rebuilt and uploaded again.
    let mut rebuilt = Vec::new();
    for _ in 0..a.times.max(2) {
        play.forget_terrain();
        let t = Instant::now();
        let fr = play.frame(w as f32, h as f32, 0.0);
        let build = t.elapsed().as_secs_f64() * 1000.0;
        let t = Instant::now();
        play.render(&mut gpu, &target, w, h, &fr);
        let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        rebuilt.push((build, t.elapsed().as_secs_f64() * 1000.0));
    }
    let (rb, rd) = (med(rebuilt.iter().map(|r| r.0).collect()), med(rebuilt.iter().map(|r| r.1).collect()));
    eprintln!("terrain rebuilt with the GPU warm, median: build {rb:.2} ms, draw {rd:.2} ms (the cost of a dig or a drop)");
    // The ticks that frame ran, timed again from the state before them.
    if spike.record.stats.ticks > 0 {
        let back = sim_gpu::perf::Spike {
            record: sim_gpu::perf::FrameRecord { tick: spike.record.tick - spike.record.stats.ticks as u64, ..spike.record.clone() },
            ..spike.clone()
        };
        let engine = boot(&spike.game, spike.seed, &spike.panel)?;
        let (roam, _) = load_roam(&spike.game)?;
        let mut before = sim_gpu::perf::replay_to(&back, engine, roam)?;
        let t = Instant::now();
        before.step(spike.record.stats.ticks);
        eprintln!("its {} tick(s) again: {:.2} ms", spike.record.stats.ticks, t.elapsed().as_secs_f64() * 1000.0);
    }
    Ok(())
}

/// The scripted player, twice, headless: spikes on the same frame with the same hash in both runs are the game's
/// or the engine's (reports written for them); the others came from outside.
fn sweep_roam(roam: &Roam, assets: &Assets, a: &Args, panel: &Path) -> Result<(), String> {
    use sim_gpu::perf::{Watch, compare_runs, run_frames};
    let mut gpu = headless_gpu(assets)?;
    let mut watches = Vec::new();
    for run in 0..2 {
        let mut play = RoamPlay::new(boot(&a.dir, a.seed, panel)?, roam.clone());
        play.step(a.ticks);
        // The first run writes reports (runs/spikes/), so a spike can be rebuilt with --repro.
        let mut watch = Watch::new(a.budget, (run == 0).then(|| PathBuf::from("runs/spikes")));
        run_frames(&mut gpu, &mut play, a.sweep, a.size, &mut watch, None, (&a.dir, panel, a.seed));
        eprintln!("run {}: {}", run + 1, watch.summary());
        watches.push(watch);
    }
    let (both, once) = compare_runs(&watches[0], &watches[1]);
    eprintln!("\nin both runs, same frame and hash ({}): the game's or the engine's", both.len());
    for s in &both {
        eprintln!("  frame {:5} tick {:5} hash {:016x}: {}", s.frame, s.tick, s.hash, s.cause);
    }
    eprintln!("in one run only ({}): from outside (OS, driver)", once.len());
    Ok(())
}

/// Every load pushed through its levels (each drawn kind, the terrain, the shown field), `a.stress` frames each:
/// where a load breaks the budget, and which phase breaks it.
fn stress_roam(roam: &Roam, assets: &Assets, a: &Args, panel: &Path) -> Result<(), String> {
    use sim_gpu::perf::{Watch, loads, run_frames};
    let text = std::fs::read_to_string(panel).map_err(|e| format!("{}: {e}", panel.display()))?;
    let base = |kind: &str| {
        text.lines().find_map(|l| l.trim().strip_prefix(&format!("{kind} =")).and_then(|v| v.trim().parse::<u32>().ok())).unwrap_or(10)
    };
    let mut gpu = headless_gpu(assets)?;
    println!(
        "{:34} {:>9} {:>9} {:>9} {:>9} {:>8} {:>6}  worst phase",
        "load", "tick p50", "work p50", "work p95", "work max", "verts", "over"
    );
    for levels in loads(roam, base) {
        let mut broke = false;
        for load in levels {
            let started = Game::load_panel(&a.dir, &load.panel(&text)).and_then(|(mut world, game)| {
                let terrain = game.def.terrain.clone().unwrap_or_default();
                load.world(&mut world, &terrain, a.seed.unwrap_or(1));
                Ok(Engine::<Loaded, _>::new(world, game).validate().map_err(|e| e.join("; "))?.start())
            });
            let engine = match started {
                Ok(e) => e,
                Err(e) => {
                    println!("{:34} does not fit: {e}", load.describe());
                    continue;
                }
            };
            let mut play = RoamPlay::new(engine, roam.clone());
            let mut watch = Watch::new(a.budget, None);
            let smell = matches!(load, sim_gpu::perf::Load::Field(..)).then_some(true);
            run_frames(&mut gpu, &mut play, a.stress, a.size, &mut watch, smell, (&a.dir, panel, a.seed));
            let pct = |mut v: Vec<f64>, p: f64| {
                v.sort_by(f64::total_cmp);
                v[((v.len() - 1) as f64 * p).round() as usize]
            };
            let r = &watch.records;
            let worst = r.iter().skip(30).max_by(|x, y| x.work_ms.total_cmp(&y.work_ms)).map(|x| x.cause()).unwrap_or_default();
            let ticked: Vec<f64> = r.iter().filter(|x| x.stats.ticks > 0).map(|x| x.stats.ticks_ms / x.stats.ticks as f64).collect();
            println!(
                "{:34} {:>9.2} {:>9.2} {:>9.2} {:>9.2} {:>8} {:>6}  {worst}",
                load.describe(),
                if ticked.is_empty() { 0.0 } else { pct(ticked, 0.5) },
                pct(r.iter().map(|x| x.work_ms).collect(), 0.5),
                pct(r.iter().map(|x| x.work_ms).collect(), 0.95),
                pct(r.iter().skip(30).map(|x| x.work_ms).collect(), 1.0),
                r.iter().map(|x| x.stats.verts).max().unwrap_or(0),
                watch.spikes.len(),
            );
            if !broke && pct(r.iter().map(|x| x.work_ms).collect(), 0.95) > a.budget {
                println!("  ^ p95 over the {:.1} ms budget from here", a.budget);
                broke = true;
            }
        }
    }
    Ok(())
}

/// Roam: `a.bench` frames offscreen at 60 fps, walking a circle while the colony builds (after `--ticks` of warm-up):
/// where the time goes per frame (simulation, the body, building the frame, the GPU), and the frame's size.
fn bench_roam(engine: Engine<Running, Game>, roam: Roam, assets: &Assets, a: &Args) -> Result<(), String> {
    let instance = wgpu::Instance::default();
    let mut gpu = pollster::block_on(Gpu::new(&instance, None, None))?;
    upload_all(&mut gpu, assets);
    let rate = engine.rules().cfg.run.tick_rate as f32;
    let mut play = RoamPlay::new(engine, roam);
    play.step(a.ticks);
    let (w, h) = a.size;
    let target = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("bench"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: gpu.format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let (mut sim, mut body, mut compose, mut draw, mut clock, mut tris) = (0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f32, 0usize);
    let mut worst = 0.0f64;
    let dt = 1.0 / 60.0;
    for f in 0..a.bench {
        let whole = Instant::now();
        let input = sim_gpu::walker::Input { forward: 1.0, look: (3.0, 0.0), ..Default::default() };
        let t = Instant::now();
        play.walk(dt, input);
        body += t.elapsed().as_secs_f64();
        let t = Instant::now();
        clock += dt * rate * a.speed;
        let n = clock.floor() as u32;
        clock -= n as f32;
        play.step(n);
        play.tween.alpha = clock;
        sim += t.elapsed().as_secs_f64();
        let t = Instant::now();
        let fr = play.frame(w as f32, h as f32, dt);
        compose += t.elapsed().as_secs_f64();
        tris = tris.max(fr.meshes.iter().chain(play.terrain().1).map(|m| m.verts.len() / 3).sum());
        let t = Instant::now();
        let world = play.world3(&mut gpu, &fr, w, h);
        gpu.render(&view, w, h, &fr.back, Some(world), &fr.front);
        let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        draw += t.elapsed().as_secs_f64();
        // After the first frames (pipelines and the first terrain upload warm up): the spikes a player feels.
        if f >= 30 {
            worst = worst.max(whole.elapsed().as_secs_f64());
        }
    }
    let per = |s: f64| s * 1000.0 / a.bench as f64;
    eprintln!(
        "{} frames at {w}x{h}, up to {tris} triangles: sim {:.3} ms, body {:.3} ms, frame {:.3} ms, gpu {:.3} ms, worst frame {:.2} ms (tick {}, {} threads)",
        a.bench,
        per(sim),
        per(body),
        per(compose),
        per(draw),
        worst * 1000.0,
        play.world().tick,
        rayon::current_num_threads()
    );
    Ok(())
}

/// Where a finished run is kept: `runs/<game>-<unix seconds>.jsonl`.
fn run_path(game: &str) -> PathBuf {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    PathBuf::from(format!("runs/{game}-{secs}.jsonl"))
}

enum Start {
    Stage(Box<(Engine<Running, Game>, Stage)>),
    Track(Box<(Engine<Running, Game>, Track)>),
    Roam(Box<(Engine<Running, Game>, Roam)>),
    /// The drive view, whether the autopilot drives you, a run to replay, the camera to start in.
    Drive(Box<DriveStart>),
}

type DriveStart = (Engine<Running, Game>, Drive, sim_physics::Track, bool, Option<Vec<DrivePress>>, usize);

/// A track's music: its current loop, found next to the game or in an `assets/` folder above it. No sound device,
/// no music, no error: the game plays silently.
struct Music {
    sink: Option<rodio::MixerDeviceSink>,
    player: Option<rodio::Player>,
    playing: Option<String>,
    muted: bool,
    dir: PathBuf,
}

impl Music {
    fn new(dir: &Path) -> Music {
        let sink = rodio::DeviceSinkBuilder::open_default_sink().ok().map(|mut s| {
            s.log_on_drop(false);
            s
        });
        Music { sink, player: None, playing: None, muted: false, dir: dir.to_path_buf() }
    }

    /// Plays `want` on loop if it is not playing already (none = silence).
    fn follow(&mut self, want: Option<&str>) {
        let want = want.filter(|_| !self.muted);
        if want == self.playing.as_deref() {
            return;
        }
        if let Some(p) = self.player.take() {
            p.stop();
        }
        self.playing = want.map(str::to_string);
        let (Some(sink), Some(file)) = (&self.sink, want) else { return };
        let path =
            std::iter::once(self.dir.join(file)).chain(self.dir.ancestors().map(|a| a.join("assets").join(file))).find(|p| p.exists());
        let Some(Ok(f)) = path.map(std::fs::File::open) else {
            eprintln!("music: '{file}' not found next to the game or in an assets/ folder above it");
            return;
        };
        match rodio::Decoder::new_looped(std::io::BufReader::new(f)) {
            Ok(src) => {
                let player = rodio::Player::connect_new(sink.mixer());
                player.set_volume(0.6);
                player.append(src);
                self.player = Some(player);
            }
            Err(e) => eprintln!("music: {file}: {e}"),
        }
    }
}

/// The engine's note, made while it plays rather than from recordings: harmonics of the engine's cycle (the
/// firing frequency, rpm / 60 × cylinders / 2, strongest; the half order a cross-plane V8 burbles at), a rasp of
/// noise pulsed at the firing rate that grows with throttle, and wind that grows with the square of speed. It lives
/// in the host: the simulation never hears it, and a machine without a sound device plays silently.
struct EngineSound {
    _sink: rodio::MixerDeviceSink,
    _player: rodio::Player,
    shared: Arc<EngineShared>,
    muted: bool,
}

#[derive(Default)]
struct EngineShared {
    rpm: std::sync::atomic::AtomicU32,
    /// ‰ of throttle, cm/s of speed, ‰ of volume.
    load: std::sync::atomic::AtomicU32,
    speed: std::sync::atomic::AtomicU32,
    gain: std::sync::atomic::AtomicU32,
}

impl EngineSound {
    fn new(s: &sim_gpu::drive::Sound) -> Option<EngineSound> {
        if !s.engine {
            return None;
        }
        let mut sink = rodio::DeviceSinkBuilder::open_default_sink().ok()?;
        sink.log_on_drop(false);
        let shared = Arc::new(EngineShared::default());
        shared.gain.store((s.volume * 1000.0) as u32, std::sync::atomic::Ordering::Relaxed);
        let player = rodio::Player::connect_new(sink.mixer());
        player.append(Note::new(shared.clone(), s.cylinders.max(1) as f32, s.wind));
        Some(EngineSound { _sink: sink, _player: player, shared, muted: false })
    }

    /// The car you drive now: rpm (or one made up from speed when the car has no gearbox yet), throttle, speed.
    fn follow(&self, rpm: Option<f32>, throttle: f32, speed: f32, volume: f32) {
        use std::sync::atomic::Ordering::Relaxed;
        let rpm = rpm.unwrap_or_else(|| 1500.0 + speed.max(0.0) * 95.0);
        self.shared.rpm.store(rpm.max(0.0) as u32, Relaxed);
        self.shared.load.store((throttle.clamp(0.0, 1.0) * 1000.0) as u32, Relaxed);
        self.shared.speed.store((speed.max(0.0) * 100.0) as u32, Relaxed);
        self.shared.gain.store(if self.muted { 0 } else { (volume * 1000.0) as u32 }, Relaxed);
    }
}

/// The spotter on the radio (`sound.voice` in drive.ron): the HUD's calls and the race's flags as recorded lines,
/// one at a time (`drive::audio::Radio` decides when). No sound device or no lines: silence.
struct SpotterVoice {
    sink: rodio::MixerDeviceSink,
    player: Option<rodio::Player>,
    radio: sim_gpu::drive::audio::Radio,
    /// Each line's file and length (s), found once.
    lines: std::collections::BTreeMap<String, (PathBuf, f32)>,
    green: bool,
    checkered: bool,
    muted: bool,
}

/// Every call and flag the spotter may say.
const CALLS: [&str; 12] = [
    "car_low",
    "car_high",
    "three_wide",
    "still_there",
    "clear",
    "clear_low",
    "clear_high",
    "inside",
    "green",
    "white_flag",
    "checkered",
    "wreck",
];

impl SpotterVoice {
    fn new(p: &DrivePlay) -> Option<SpotterVoice> {
        use rodio::Source;
        let v = &p.drive.sound.voice;
        let mut lines = std::collections::BTreeMap::new();
        for call in CALLS.iter().map(|c| c.to_string()).chain(v.lines.keys().cloned()) {
            let Some(path) = v.line(&p.drive.dir, &call) else { continue };
            let Ok(dec) = std::fs::File::open(&path)
                .map(std::io::BufReader::new)
                .map_err(|e| e.to_string())
                .and_then(|f| rodio::Decoder::new(f).map_err(|e| e.to_string()))
            else {
                continue;
            };
            let len = dec.total_duration().map(|d| d.as_secs_f32()).unwrap_or_else(|| {
                let (ch, rate) = (dec.channels().get() as f32, dec.sample_rate().get() as f32);
                dec.count() as f32 / ch / rate
            });
            lines.insert(call, (path, len));
        }
        if lines.is_empty() {
            return None;
        }
        let mut sink = rodio::DeviceSinkBuilder::open_default_sink().ok()?;
        sink.log_on_drop(false);
        Some(SpotterVoice { sink, player: None, radio: Default::default(), lines, green: false, checkered: false, muted: false })
    }

    /// A frame: the HUD's call, the green flag when the field first rolls, the checkered when the race is over.
    fn follow(&mut self, p: &DrivePlay) {
        let now = p.time;
        if !self.green && p.world().tick >= 2 {
            self.green = true;
            self.radio.announce("green", now);
        }
        if !self.checkered && (p.engine.outcome().is_some() || p.world().tick + 1 >= p.engine.rules().cfg.run.max_ticks) {
            self.checkered = true;
            self.radio.announce("checkered", now);
        }
        let lines = &self.lines;
        let say = self.radio.hear(p.last_hud.voice.as_deref(), now, &p.drive.sound.voice, |c| lines.get(c).map(|l| l.1));
        let Some(call) = say else { return };
        if self.muted {
            return;
        }
        let Some((path, _)) = self.lines.get(&call) else { return };
        if let Ok(src) = std::fs::File::open(path)
            .map(std::io::BufReader::new)
            .map_err(|e| e.to_string())
            .and_then(|f| rodio::Decoder::new(f).map_err(|e| e.to_string()))
        {
            let player = rodio::Player::connect_new(self.sink.mixer());
            player.set_volume(p.drive.sound.voice.volume);
            player.append(src);
            self.player = Some(player);
        }
    }
}

/// The synthesised note, one sample at a time (mono, 44.1 kHz).
struct Note {
    shared: Arc<EngineShared>,
    phase: f64,
    rpm: f32,
    load: f32,
    speed: f32,
    gain: f32,
    seed: u32,
    rasp: f32,
    wind: (f32, f32),
    cylinders: f32,
    wind_k: f32,
}

const RATE: u32 = 44_100;

impl Note {
    fn new(shared: Arc<EngineShared>, cylinders: f32, wind_k: f32) -> Note {
        Note {
            shared,
            phase: 0.0,
            rpm: 1500.0,
            load: 0.0,
            speed: 0.0,
            gain: 0.0,
            seed: 0x1234_5678,
            rasp: 0.0,
            wind: (0.0, 0.0),
            cylinders,
            wind_k,
        }
    }

    fn noise(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        self.seed as f32 / u32::MAX as f32 * 2.0 - 1.0
    }
}

impl Iterator for Note {
    type Item = rodio::Sample;

    fn next(&mut self) -> Option<rodio::Sample> {
        use std::sync::atomic::Ordering::Relaxed;
        // Follow the car smoothly (a few ms), so a new frame's value never clicks.
        let k = 0.0015;
        self.rpm += (self.shared.rpm.load(Relaxed) as f32 - self.rpm) * k;
        self.load += (self.shared.load.load(Relaxed) as f32 / 1000.0 - self.load) * k;
        self.speed += (self.shared.speed.load(Relaxed) as f32 / 100.0 - self.speed) * k;
        self.gain += (self.shared.gain.load(Relaxed) as f32 / 1000.0 - self.gain) * k;
        let cycle = (self.rpm / 120.0).max(1.0) as f64; // one four-stroke cycle is two turns of the crank
        self.phase = (self.phase + cycle / RATE as f64).fract();
        let tau = std::f64::consts::TAU;
        let fire = self.cylinders;
        let mut harm = 0.0f32;
        for n in 1..=32u32 {
            let f = cycle as f32 * n as f32;
            if f > 7000.0 {
                break;
            }
            let order = n as f32;
            let a = if order == fire {
                1.0
            } else if order == fire / 2.0 {
                0.55
            } else if order == fire * 2.0 {
                0.4
            } else if order == fire * 1.5 {
                0.28
            } else if order == fire * 3.0 {
                0.18
            } else {
                0.05 * (0.4 + self.load) / order.sqrt()
            };
            harm += a * ((tau * n as f64 * self.phase).sin() as f32);
        }
        // Combustion rasp: noise, pulsed at the firing rate, louder under load.
        let pulse = 0.5 + 0.5 * ((tau * fire as f64 * self.phase).sin() as f32);
        let w = self.noise();
        self.rasp += (w - self.rasp) * 0.25;
        let rasp = self.rasp * pulse * pulse * self.load * (0.25 + self.rpm / 12_000.0) * 0.6;
        // Wind: noise with the lows and the highs taken off, with the square of speed.
        let w2 = self.noise();
        self.wind.0 += (w2 - self.wind.0) * 0.2;
        self.wind.1 += (self.wind.0 - self.wind.1) * 0.02;
        let wind = (self.wind.0 - self.wind.1) * self.wind_k * (self.speed / 80.0).powi(2);
        let x = harm * 0.18 * (0.35 + 0.65 * self.load) + rasp + wind;
        Some((x * 1.4).tanh() * self.gain)
    }
}

impl rodio::Source for Note {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> rodio::ChannelCount {
        std::num::NonZero::new(1).expect("one")
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        std::num::NonZero::new(RATE).expect("a rate")
    }
    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}

/// A key's name for drive views (arrows, letters, space), from what it types.
fn drive_key(k: &Key) -> Option<String> {
    Some(match k.as_ref() {
        Key::Named(NamedKey::ArrowUp) => "up".into(),
        Key::Named(NamedKey::ArrowDown) => "down".into(),
        Key::Named(NamedKey::ArrowLeft) => "left".into(),
        Key::Named(NamedKey::ArrowRight) => "right".into(),
        Key::Named(NamedKey::Space) => "space".into(),
        Key::Named(NamedKey::Shift) => "shift".into(),
        Key::Named(NamedKey::Enter) => "enter".into(),
        Key::Named(NamedKey::Escape) => "esc".into(),
        Key::Named(NamedKey::Backspace) => "backspace".into(),
        Key::Named(NamedKey::Tab) => "tab".into(),
        Key::Character(c) => c.to_lowercase(),
        _ => return None,
    })
}

/// What a window plays: a stage (side view) or a track (first person).
enum Session {
    Stage(Box<Play>),
    Track(Box<TrackPlay>),
    Roam(Box<RoamPlay>),
    Drive(Box<DrivePlay>),
}

impl Session {
    fn advance(&mut self, dt: f32, paused: bool, clock: &mut f32, speed: f32) {
        // A hitstop slows the world, not the camera.
        let warp = if let Session::Track(p) = self { p.time_scale() } else { 1.0 };
        let (time, n) = {
            *clock += if paused { 0.0 } else { dt * speed * warp };
            let n = clock.floor() as u32;
            *clock -= n as f32;
            (dt, n.min(200))
        };
        match self {
            Session::Stage(p) => {
                p.time += time;
                p.step(n);
                p.tween.alpha = if paused { 1.0 } else { clock.clamp(0.0, 1.0) };
            }
            Session::Track(p) => {
                p.time += time;
                p.step(n);
                p.tween.alpha = if paused { 1.0 } else { clock.clamp(0.0, 1.0) };
            }
            Session::Drive(p) => {
                p.time += time;
                p.step(n);
                p.alpha = if paused { 1.0 } else { clock.clamp(0.0, 1.0) };
            }
            // The body moves every frame; the game ticks at its rate and follows it.
            Session::Roam(p) => {
                let input = p.input;
                p.input.look = (0.0, 0.0);
                p.walk(time, input);
                p.step(n);
                p.tween.alpha = if paused { 1.0 } else { clock.clamp(0.0, 1.0) };
            }
        }
    }

    fn draw(&mut self, gpu: &mut Gpu, view: &wgpu::TextureView, w: u32, h: u32, dt: f32) {
        match self {
            Session::Stage(p) => {
                let quads = p.quads(w as f32, h as f32, dt);
                gpu.draw(view, w, h, &quads);
            }
            Session::Track(p) => {
                let fr = p.frame(w as f32, h as f32, dt);
                let world = World3::new(fr.eye.view_proj(w as f32, h as f32), fr.fog, &fr.meshes);
                gpu.render(view, w, h, &fr.back, Some(world), &fr.front);
            }
            Session::Roam(p) => {
                let fr = p.frame(w as f32, h as f32, dt);
                let world = p.world3(gpu, &fr, w, h);
                gpu.render(view, w, h, &fr.back, Some(world), &fr.front);
            }
            Session::Drive(p) => {
                let fr = p.frame(w as f32, h as f32, dt);
                p.render(gpu, view, w, h, &fr);
            }
        }
    }

    fn title(&self, speed: f32, paused: bool) -> String {
        match self {
            Session::Stage(p) => p.title(speed, paused),
            Session::Track(p) => p.title(speed, paused),
            Session::Roam(p) => p.title(),
            Session::Drive(p) => p.title(speed, paused),
        }
    }
}

struct Window3 {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    gpu: Gpu,
    play: Session,
}

struct App {
    play: Option<Start>,
    assets: Assets,
    window: Option<Window3>,
    speed: f32,
    paused: bool,
    clock: f32,
    last: Instant,
    cursor: (f32, f32),
    /// Track games: how to start over, a replay to play first, whether this run is saved yet.
    reboot: Option<Reboot>,
    script: Option<Vec<sim_gpu::track::Press>>,
    saved: bool,
    /// Roam: the mouse is captured (looking), else free (click to capture).
    grabbed: bool,
    /// A track's theme to start in.
    theme: usize,
    /// A track's music (a theme's loop).
    music: Option<Music>,
    /// Where a press (mouse or finger) started: its release decides the swipe.
    swipe_from: Option<(f32, f32)>,
    /// A drive view's engine note (none without a sound device).
    engine_sound: Option<EngineSound>,
    /// A drive view's spotter on the radio.
    voice: Option<SpotterVoice>,
    /// The window's logical size (a track's `screen`, or `--size`); none = the default.
    window_size: Option<(u32, u32)>,
    /// Roam: every frame measured against the budget; spike reports to `runs/spikes/` (game, panel, seed).
    watch: Option<FrameWatch>,
}

/// The window's frame watch: the watch, the frame count, the last frame's start, and where the game came from
/// (game, panel, seed) for its spike reports.
type FrameWatch = (sim_gpu::perf::Watch, u64, Instant, (PathBuf, PathBuf, Option<u64>));

/// A key's name as views write it ("w", "shift", "space"), by position on the keyboard, so WASD is WASD on any layout.
fn key_name(k: PhysicalKey) -> Option<String> {
    let PhysicalKey::Code(c) = k else { return None };
    let s = format!("{c:?}");
    Some(match c {
        KeyCode::Space => "space".into(),
        KeyCode::ShiftLeft | KeyCode::ShiftRight => "shift".into(),
        KeyCode::ControlLeft | KeyCode::ControlRight => "ctrl".into(),
        KeyCode::Tab => "tab".into(),
        _ => s.strip_prefix("Key").or_else(|| s.strip_prefix("Digit"))?.to_lowercase(),
    })
}

impl App {
    fn grab(&mut self, on: bool) {
        let Some(w) = &self.window else { return };
        let ok =
            !on || w.window.set_cursor_grab(CursorGrabMode::Locked).or_else(|_| w.window.set_cursor_grab(CursorGrabMode::Confined)).is_ok();
        if !on {
            let _ = w.window.set_cursor_grab(CursorGrabMode::None);
        }
        w.window.set_cursor_visible(!(on && ok));
        self.grabbed = on && ok;
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let Some(start) = self.play.take() else { return };
        let window = Arc::new(
            el.create_window(Window::default_attributes().with_title("simcraft").with_inner_size({
                let (w, h) = self.window_size.unwrap_or((1440, 810));
                winit::dpi::LogicalSize::new(w as f64, h as f64)
            }))
            .expect("a window"),
        );
        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window.clone()).expect("a surface");
        let mut gpu = pollster::block_on(Gpu::new(&instance, Some(&surface), None)).expect("a GPU");
        upload_all(&mut gpu, &self.assets);
        // The pictures live on the GPU now: the CPU copies (tens of MB) are not needed any more.
        self.assets = Assets::default();
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: gpu.format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
        };
        surface.configure(&gpu.device, &config);
        let play = match start {
            Start::Stage(b) => {
                let (engine, stage) = *b;
                Session::Stage(Box::new(Play::new(engine, stage, gpu.sizes.clone())))
            }
            Start::Track(b) => {
                let (engine, track) = *b;
                let mut p = TrackPlay::new(engine, track, gpu.sizes.clone());
                p.set_theme(self.theme);
                p.script = self.script.take();
                Session::Track(Box::new(p))
            }
            Start::Roam(b) => {
                let (engine, roam) = *b;
                Session::Roam(Box::new(RoamPlay::new(engine, roam)))
            }
            Start::Drive(b) => {
                let (engine, drive, track, auto, script, view) = *b;
                let mut p = DrivePlay::new(engine, drive, track);
                p.upload(&mut gpu);
                p.rig.view = view;
                p.auto_you = auto && script.is_none();
                p.script = script;
                self.engine_sound = EngineSound::new(&p.drive.sound);
                self.voice = SpotterVoice::new(&p);
                Session::Drive(Box::new(p))
            }
        };
        self.window = Some(Window3 { window, surface, config, gpu, play });
        self.last = Instant::now();
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let Some(w) = self.window.as_mut() else { return };
        if let Session::Roam(p) = &mut w.play {
            match &event {
                WindowEvent::KeyboardInput { event: k, .. } => {
                    let down = k.state == ElementState::Pressed;
                    if down && k.logical_key == Key::Named(NamedKey::Escape) {
                        if self.grabbed {
                            self.grab(false);
                        } else {
                            el.exit();
                        }
                        return;
                    }
                    if let Some(name) = key_name(k.physical_key) {
                        let keys = &p.roam.keys;
                        let axis = |on: bool| if on && down { 1.0 } else { 0.0 };
                        let i = &mut p.input;
                        if name == keys.forward || name == keys.back {
                            i.forward = if name == keys.forward { axis(true) } else { -axis(true) };
                        } else if name == keys.left || name == keys.right {
                            i.strafe = if name == keys.right { axis(true) } else { -axis(true) };
                        } else if name == keys.run {
                            i.run = down;
                        } else if name == keys.jump {
                            i.jump = down;
                        } else if name == keys.smell {
                            p.smell_on = down;
                        } else if name == "p" && down && !k.repeat {
                            self.paused = !self.paused;
                        }
                    }
                    return;
                }
                WindowEvent::MouseInput { state: ElementState::Pressed, button, .. } => {
                    if !self.grabbed {
                        self.grab(true);
                    } else if *button == MouseButton::Left {
                        p.use_target(false);
                    } else if *button == MouseButton::Right {
                        p.use_target(true);
                    }
                    return;
                }
                WindowEvent::Focused(false) => {
                    p.input = Default::default();
                    self.grab(false);
                    return;
                }
                _ => {}
            }
        }
        // Driving: held keys ramp the pedals and the wheel (the play turns them into the game's action); C swaps
        // cockpit and chase, O hands your car to the autopilot, P pauses, R restarts, M mutes, Esc quits.
        if let Some(Session::Drive(p)) = self.window.as_mut().map(|w| &mut w.play) {
            match &event {
                WindowEvent::KeyboardInput { event: k, .. } => {
                    let Some(name) = drive_key(&k.logical_key) else { return };
                    if k.state != ElementState::Pressed {
                        p.held.remove(&name);
                        return;
                    }
                    p.held.insert(name.clone());
                    if k.repeat {
                        return;
                    }
                    match name.as_str() {
                        "esc" => el.exit(),
                        "c" => p.cycle_view(),
                        "tab" => p.watch_next(),
                        "p" => self.paused = !self.paused,
                        "o" => p.auto_you = !p.auto_you,
                        "m" => {
                            if let Some(s) = &mut self.engine_sound {
                                s.muted = !s.muted;
                            }
                            if let Some(v) = &mut self.voice {
                                v.muted = !v.muted;
                            }
                        }
                        "r" => {
                            if let Some(Ok(engine)) = self.reboot.as_ref().map(|r| r()) {
                                if !p.log.is_empty() {
                                    let _ = sim_gpu::drive::save_run(&run_path(&p.engine.rules().def.name), &p.log);
                                }
                                p.restart(engine);
                                self.clock = 0.0;
                                if let Some(v) = &mut self.voice {
                                    (v.green, v.checkered) = (false, false);
                                }
                            }
                        }
                        other => {
                            p.key(other);
                        }
                    }
                    return;
                }
                WindowEvent::Focused(false) => {
                    p.held.clear();
                    return;
                }
                _ => {}
            }
        }
        let Some(w) = self.window.as_mut() else { return };
        match event {
            WindowEvent::CloseRequested => {
                if let Session::Drive(p) = &w.play
                    && !p.log.is_empty()
                    && p.script.is_none()
                {
                    let path = run_path(&p.engine.rules().def.name);
                    if sim_gpu::drive::save_run(&path, &p.log).is_ok() {
                        eprintln!("run saved: {} (replay: --replay {})", path.display(), path.display());
                    }
                }
                el.exit()
            }
            WindowEvent::Resized(size) => {
                w.config.width = size.width.max(1);
                w.config.height = size.height.max(1);
                w.surface.configure(&w.gpu.device, &w.config);
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x as f32, position.y as f32);
                let (cw, ch) = (w.config.width as f32, w.config.height as f32);
                match &mut w.play {
                    Session::Stage(p) => p.hover(cw, ch, self.cursor.0, self.cursor.1),
                    Session::Track(p) => p.hover(cw, ch, self.cursor.0, self.cursor.1),
                    Session::Roam(_) | Session::Drive(_) => {}
                }
            }
            WindowEvent::MouseInput { state: ElementState::Pressed, button: winit::event::MouseButton::Left, .. } => {
                let (cw, ch) = (w.config.width as f32, w.config.height as f32);
                match &mut w.play {
                    Session::Stage(p) => {
                        if let Some(b) = p.button_under(cw, ch, self.cursor.0, self.cursor.1) {
                            p.press(b);
                        }
                    }
                    // A track: a press on a button is a click; anywhere else it may become a swipe (on release).
                    Session::Track(p) => {
                        if p.track.buttons.iter().any(|b| b.hit(cw, ch, self.cursor.0, self.cursor.1)) {
                            p.click(cw, ch, self.cursor.0, self.cursor.1);
                        } else {
                            self.swipe_from = Some(self.cursor);
                        }
                    }
                    Session::Roam(_) | Session::Drive(_) => {}
                }
            }
            // A mouse drag is a swipe (the same code as a finger): its direction picks the move, a short one is a tap.
            WindowEvent::MouseInput { state: ElementState::Released, button: winit::event::MouseButton::Left, .. } => {
                if let (Session::Track(p), Some((x0, y0))) = (&mut w.play, self.swipe_from.take()) {
                    let min = w.config.height as f32 * 0.035;
                    p.swipe(sim_gpu::track::swipe_dir(self.cursor.0 - x0, self.cursor.1 - y0, min));
                }
            }
            WindowEvent::Touch(t) => {
                if let Session::Track(p) = &mut w.play {
                    let at = (t.location.x as f32, t.location.y as f32);
                    match t.phase {
                        winit::event::TouchPhase::Started => self.swipe_from = Some(at),
                        winit::event::TouchPhase::Ended => {
                            if let Some((x0, y0)) = self.swipe_from.take() {
                                let min = w.config.height as f32 * 0.035;
                                p.swipe(sim_gpu::track::swipe_dir(at.0 - x0, at.1 - y0, min));
                            }
                        }
                        winit::event::TouchPhase::Cancelled => self.swipe_from = None,
                        winit::event::TouchPhase::Moved => {}
                    }
                }
            }
            // A key let go: its buttons' release actions (hold to crouch, hold to walk).
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Released && matches!(w.play, Session::Track(_)) => {
                let Session::Track(p) = &mut w.play else { return };
                let name = match event.logical_key.as_ref() {
                    Key::Named(NamedKey::Shift) => "shift".to_string(),
                    Key::Named(NamedKey::Space) => "space".to_string(),
                    Key::Named(NamedKey::ArrowUp) => "up".to_string(),
                    Key::Named(NamedKey::ArrowDown) => "down".to_string(),
                    Key::Character(c) => c.to_lowercase(),
                    _ => return,
                };
                p.key_up(&name);
            }
            // First person: the track's keys go to its buttons (1–4, space, Enter...); p pauses, esc quits.
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed && !event.repeat && matches!(w.play, Session::Track(_)) =>
            {
                let Session::Track(p) = &mut w.play else { return };
                let name = match event.logical_key.as_ref() {
                    Key::Named(NamedKey::Escape) => return el.exit(),
                    Key::Named(NamedKey::Space) => "space".to_string(),
                    Key::Named(NamedKey::Enter) => "enter".to_string(),
                    Key::Named(NamedKey::ArrowLeft) => "left".to_string(),
                    Key::Named(NamedKey::ArrowRight) => "right".to_string(),
                    Key::Named(NamedKey::ArrowUp) => "up".to_string(),
                    Key::Named(NamedKey::ArrowDown) => "down".to_string(),
                    Key::Named(NamedKey::Shift) => "shift".to_string(),
                    Key::Character("p") => {
                        self.paused = !self.paused;
                        return;
                    }
                    Key::Character("c") => {
                        p.cycle_view();
                        return;
                    }
                    // T: the next theme (the same run, another world). M: music off / on.
                    Key::Character("t") => {
                        let next = p.theme + 1;
                        p.set_theme(next);
                        return;
                    }
                    Key::Character("m") => {
                        if let Some(m) = &mut self.music {
                            m.muted = !m.muted;
                        }
                        return;
                    }
                    Key::Character(c) => c.to_lowercase(), // Shift+R is still R
                    _ => return,
                };
                // What the key means is decided by the play (tested there): R and N start a new run, V watches the
                // run just ended again, any game key after the end or during a replay plays.
                match p.intent(&name) {
                    KeyIntent::Press(k) => {
                        p.key(&k);
                    }
                    KeyIntent::NewRun | KeyIntent::WatchAgain => {
                        let watch = p.intent(&name) == KeyIntent::WatchAgain;
                        let Some(reboot) = &self.reboot else { return };
                        let Ok(engine) = reboot() else { return };
                        if !self.saved && p.script.is_none() && !p.log.is_empty() {
                            let _ = save_run(&run_path(&p.engine.rules().def.name), &p.log);
                        }
                        self.saved = false;
                        **p = p.restart(engine, watch);
                        self.clock = 0.0;
                    }
                    KeyIntent::None => {}
                }
            }
            WindowEvent::KeyboardInput { event, .. }
                if event.state == ElementState::Pressed
                    && !event.repeat
                    && let Session::Stage(p) = &mut w.play
                    && let Key::Character(c) = event.logical_key.as_ref()
                    && let Some(b) = p.stage.buttons.iter().position(|b| b.key.as_deref() == Some(c)) =>
            {
                p.press(b);
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed && matches!(w.play, Session::Stage(_)) => {
                let Session::Stage(p) = &mut w.play else { return };
                match event.logical_key.as_ref() {
                    Key::Named(NamedKey::Escape) => el.exit(),
                    Key::Named(NamedKey::Space) => self.paused = !self.paused,
                    Key::Named(NamedKey::Tab) => p.follow = p.next_follow(p.follow),
                    Key::Named(NamedKey::ArrowLeft) => p.springs[0].pos -= 2.0,
                    Key::Named(NamedKey::ArrowRight) => p.springs[0].pos += 2.0,
                    Key::Character("+") | Key::Character("=") => self.speed = (self.speed * 2.0).min(2000.0),
                    Key::Character("-") => self.speed = (self.speed / 2.0).max(0.25),
                    Key::Character("c") => {
                        p.shot = match p.shot {
                            Shot::Close => Shot::Wide,
                            Shot::Wide => Shot::Nest,
                            Shot::Nest => Shot::Close,
                        }
                    }
                    _ => {}
                }
            }
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let dt = now.duration_since(self.last).as_secs_f32().min(0.1);
                self.last = now;
                if let Session::Roam(p) = &mut w.play {
                    p.stats = Default::default();
                }
                let t = Instant::now();
                w.play.advance(dt, self.paused, &mut self.clock, self.speed);
                if let (Session::Track(p), Some(m)) = (&w.play, &mut self.music) {
                    m.follow(p.track.music.as_deref());
                }
                if let (Session::Drive(p), Some(s)) = (&w.play, &self.engine_sound)
                    && let Some(c) = p.cars().into_iter().find(|c| c.you)
                {
                    let vol = if self.paused { 0.0 } else { p.drive.sound.volume };
                    s.follow(c.rpm, c.throttle, c.speed, vol);
                }
                if let (Session::Drive(p), Some(v)) = (&w.play, &mut self.voice) {
                    v.follow(p);
                }
                let advance_ms = t.elapsed().as_secs_f64() * 1000.0;
                // A run that just ended is kept, so it can be replayed later (`--replay`).
                if let Session::Track(p) = &w.play
                    && p.engine.outcome().is_some()
                    && p.script.is_none()
                    && !self.saved
                {
                    let path = run_path(&p.engine.rules().def.name);
                    if save_run(&path, &p.log).is_ok() {
                        eprintln!("run saved: {} (replay: --replay {})", path.display(), path.display());
                    }
                    self.saved = true;
                }
                let (cw, ch) = (w.config.width, w.config.height);
                let t = Instant::now();
                match w.surface.get_current_texture() {
                    wgpu::CurrentSurfaceTexture::Success(frame) | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
                        w.play.draw(&mut w.gpu, &view, cw, ch, dt);
                        frame.present();
                    }
                    wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {}
                    _ => w.surface.configure(&w.gpu.device, &w.config),
                }
                let draw_all_ms = t.elapsed().as_secs_f64() * 1000.0;
                let t = Instant::now();
                w.window.set_title(&w.play.title(self.speed, self.paused));
                let window_ms = t.elapsed().as_secs_f64() * 1000.0;
                if let (Session::Roam(p), Some((watch, frame, prev, source))) = (&w.play, self.watch.as_mut()) {
                    let s = p.stats.clone();
                    let rec = sim_gpu::perf::FrameRecord {
                        frame: *frame,
                        tick: p.world().tick,
                        walk_ms: (advance_ms - s.ticks_ms).max(0.0),
                        draw_ms: (draw_all_ms - s.build_ms).max(0.0),
                        window_ms,
                        work_ms: now.elapsed().as_secs_f64() * 1000.0,
                        interval_ms: now.duration_since(*prev).as_secs_f64() * 1000.0,
                        stats: s,
                    };
                    *prev = now;
                    *frame += 1;
                    if let Some(path) = watch.frame(rec, p, (&source.0, &source.1, source.2)) {
                        eprintln!("frame over budget: {} ({})", path.display(), watch.spikes.last().map_or("", |s| s.1.as_str()));
                    }
                }
            }
            _ => {}
        }
    }

    /// Raw mouse movement (not the cursor): looking around in first person, while the mouse is captured.
    fn device_event(&mut self, _: &ActiveEventLoop, _: DeviceId, event: DeviceEvent) {
        if let (true, Some(w), DeviceEvent::MouseMotion { delta }) = (self.grabbed, self.window.as_mut(), event)
            && let Session::Roam(p) = &mut w.play
        {
            p.input.look.0 += delta.0 as f32;
            p.input.look.1 += delta.1 as f32;
        }
    }

    /// Roam: how the frames went (percentiles, spikes and their reports).
    fn exiting(&mut self, _: &ActiveEventLoop) {
        if let Some((watch, ..)) = &self.watch {
            eprintln!("{}", watch.summary());
        }
    }

    fn about_to_wait(&mut self, _: &ActiveEventLoop) {
        if let Some(w) = &self.window {
            w.window.request_redraw();
        }
    }
}
