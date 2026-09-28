//! simcraft-view: run a game with its view, in the terminal, at a high refresh rate.
//!
//!   simcraft-view games/colony [--seed N] [--speed TICKS_PER_SEC] [--fps N] [--view FILE]
//!   simcraft-view games/colony --dump TICKS [--size WxH]     (one frame as text, no terminal)
//!
//! The simulation runs in-process at its own speed; frames are drawn at their own rate.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind};
use sim_core::{Engine, Loaded, Running, World};
use sim_render::projection::Projection;
use sim_render::{Assets, Canvas, Registry, Renderer, Scene, TerminalGuard, Theme, Ui, View};
use sim_rules::Game;

struct App {
    engine: Engine<Running, Game>,
    view: View,
    registry: Registry,
    theme: Theme,
    assets: Assets,
    ui: Ui,
    series: Vec<String>,
}

/// `name.ron` in the game's folder, or in `<ancestor>/<shared>/` (e.g. `assets/`, `themes/`).
fn find(dir: &Path, shared: &str, name: &str) -> Option<PathBuf> {
    let file = format!("{name}.ron");
    std::iter::once(dir.join(&file)).chain(dir.ancestors().map(|a| a.join(shared).join(&file))).find(|p| p.exists())
}

/// Series the view shows (from `Series` props), else props of kinds that exist once.
fn view_series(view: &View) -> Vec<String> {
    fn walk(n: &sim_render::Node, out: &mut Vec<String>) {
        match n {
            sim_render::Node::Rows(v) | sim_render::Node::Cols(v) => v.iter().for_each(|(_, c)| walk(c, out)),
            sim_render::Node::C { name, props, .. } if name == "Series" => {
                if let Ok(p) = sim_render::props::<sim_render::component::SeriesProps>(props) {
                    out.extend(p.names);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(&view.layout, &mut out);
    out
}

fn load(dir: &Path, seed: Option<u64>, view_file: Option<&Path>) -> Result<App, String> {
    let config = match seed {
        Some(s) => {
            let panel = std::fs::read_to_string(dir.join("engine.toml")).map_err(|e| e.to_string())?;
            let mut out = String::new();
            for line in panel.lines() {
                if line.trim_start().starts_with("seed") {
                    out.push_str(&format!("seed = {s}\n"));
                } else {
                    out.push_str(line);
                    out.push('\n');
                }
            }
            let tmp = std::env::temp_dir().join(format!("simcraft-view-{s}.toml"));
            std::fs::write(&tmp, out).map_err(|e| e.to_string())?;
            Some(tmp)
        }
        None => None,
    };
    let (world, game) = Game::load(dir, config.as_deref())?;
    let engine = Engine::<Loaded, _>::new(world, game).validate().map_err(|e| e.join("\n"))?.start();
    let depth = engine.world().depth;
    let view_path = view_file.map_or_else(|| dir.join("view.ron"), Path::to_path_buf);
    let mut view = match std::fs::read_to_string(&view_path) {
        Ok(src) => ron::from_str::<View>(&src).map_err(|e| format!("{}: {e}", view_path.display()))?,
        Err(_) if view_file.is_none() => View::default_for(depth),
        Err(e) => return Err(format!("{}: {e}", view_path.display())),
    };
    let registry = Registry::default();
    let worlds = view.resolve(&registry, depth).map_err(|e| format!("view.ron:\n  {}", e.join("\n  ")))?;
    let mut theme = Theme::builtin(&view.theme).ok_or_else(|| format!("view.ron: unknown theme '{}' (dark, light)", view.theme))?;
    if let Ok(src) = std::fs::read_to_string(dir.join("theme.ron")) {
        theme = theme.merged(ron::from_str(&src).map_err(|e| format!("theme.ron: {e}"))?);
    }
    let mut assets = Assets::default();
    for pack in &view.assets {
        let path = find(dir, "assets", pack).ok_or_else(|| format!("asset pack '{pack}' not found (assets/{pack}.ron)"))?;
        let src = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let mut pack: Assets = ron::from_str(&src).map_err(|e| format!("{}: {e}", path.display()))?;
        pack.load_images(path.parent().unwrap_or(Path::new(".")))?;
        assets = assets.merged(pack);
    }
    let series = { let s = view_series(&view); if s.is_empty() { default_series(engine.world()) } else { s } };
    let selected = pick(engine.world(), engine.rules(), None);
    let mut ui = Ui::new(worlds);
    ui.feel = view.feel.clone();
    ui.selected = selected;
    Ok(App { engine, view, registry, theme, assets, ui, series })
}

/// Props of kinds that exist once (the nest, a market...): worth watching.
fn default_series(w: &World) -> Vec<String> {
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    w.entities().values().for_each(|e| *counts.entry(&e.kind).or_default() += 1);
    w.entities()
        .values()
        .filter(|e| counts[e.kind.as_str()] == 1)
        .flat_map(|e| e.props.keys().map(move |p| format!("{}.{p}", e.kind)))
        .collect()
}

/// Next entity to inspect: visible kinds with a state machine, sparse enough to be individuals.
fn pick(w: &World, g: &Game, after: Option<u64>) -> Option<u64> {
    let interesting = |kind: &str| {
        !g.is_hidden(kind) && g.def.kinds.get(kind).is_some_and(|k| k.fsm.is_some()) && w.count(kind) <= 200
    };
    // The most numerous individual kind first (ants before bushes), then by id.
    let mut kinds: Vec<&str> = g.def.kinds.keys().map(String::as_str).filter(|k| interesting(k)).collect();
    kinds.sort_by_key(|k| std::cmp::Reverse(w.count(k)));
    let ids: Vec<u64> = kinds.iter().flat_map(|k| w.of_kind(k).map(|e| e.id)).collect();
    match after {
        Some(a) => ids.iter().position(|id| *id == a).and_then(|i| ids.get(i + 1)).or(ids.first()).copied(),
        None => ids.first().copied(),
    }
}

impl App {
    fn step(&mut self, n: u32) {
        for _ in 0..n {
            if self.engine.outcome().is_some() || self.engine.world().tick >= self.engine.rules().cfg.run.max_ticks {
                break;
            }
            self.ui.tween.borrow_mut().remember(self.engine.world());
            let report = self.engine.tick();
            if !report.events.is_empty() {
                let mut c: BTreeMap<&str, usize> = BTreeMap::new();
                report.events.iter().for_each(|e| *c.entry(e.name.as_str()).or_default() += 1);
                let line: Vec<String> = c.iter().map(|(k, n)| if *n > 1 { format!("{k} ×{n}") } else { k.to_string() }).collect();
                self.ui.events.push_back(format!("tick {:>5}  {}", report.tick, line.join(", ")));
                while self.ui.events.len() > 50 {
                    self.ui.events.pop_front();
                }
            }
            self.record();
        }
        self.ui.tick = self.engine.world().tick;
        self.ui.outcome = self.engine.outcome().map(String::from);
        if self.ui.selected.and_then(|id| self.engine.world().get(id)).is_none() {
            self.ui.selected = pick(self.engine.world(), self.engine.rules(), None);
        }
    }

    fn record(&mut self) {
        let w = self.engine.world();
        for name in &self.series {
            let v = match name.split_once('.') {
                Some(("count", kind)) => w.count(kind) as i64,
                Some((kind, prop)) => w.of_kind(kind).map(|e| e.props.get(prop).copied().unwrap_or(0)).sum(),
                None => 0,
            };
            let h = self.ui.history.entry(name.clone()).or_default();
            h.push_back(v);
            if h.len() > 240 {
                h.pop_front();
            }
        }
    }

    fn draw(&mut self, canvas: &mut Canvas) {
        self.ui.frame += 1;
        self.ui.hits.borrow_mut().clear();
        self.ui.images.borrow_mut().clear();
        let scene = Scene { world: self.engine.world(), game: self.engine.rules(), assets: &self.assets };
        self.view.draw(&self.registry, &self.theme, &scene, &self.ui, canvas);
    }

    /// A click on a component: a 2.5D world goes to its perspective's `click` state.
    fn click(&mut self, x: u16, y: u16) {
        if let Some(Projection::Layers(l)) = self.ui.hit(x, y).and_then(|i| self.ui.worlds.get_mut(i)) {
            l.click();
        }
    }

    fn key(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Char('q') | KeyCode::Esc => return false,
            KeyCode::Char(' ') => self.ui.paused = !self.ui.paused,
            KeyCode::Char('+') | KeyCode::Char('=') => self.ui.speed = (self.ui.speed * 2.0).min(2000.0),
            KeyCode::Char('-') => self.ui.speed = (self.ui.speed / 2.0).max(0.5),
            KeyCode::Char('s') => self.step(1),
            KeyCode::Tab => self.ui.selected = pick(self.engine.world(), self.engine.rules(), self.ui.selected),
            KeyCode::Char('p') => self.ui.worlds.iter_mut().for_each(|p| {
                if let Projection::Layers(l) = p {
                    l.click();
                }
            }),
            _ => {
                let depth = self.engine.world().depth;
                for p in self.ui.worlds.iter_mut() {
                    match (p, code) {
                        (Projection::Dim3(cam), KeyCode::Left) => cam.yaw -= 10.0,
                        (Projection::Dim3(cam), KeyCode::Right) => cam.yaw += 10.0,
                        (Projection::Dim3(cam), KeyCode::Up) => cam.pitch = (cam.pitch + 5.0).min(89.0),
                        (Projection::Dim3(cam), KeyCode::Down) => cam.pitch = (cam.pitch - 5.0).max(-10.0),
                        (Projection::Dim3(cam), KeyCode::Char(']')) => cam.cut = Some(cam.cut.map_or(0, |c| c + 1)),
                        (Projection::Dim3(cam), KeyCode::Char('[')) => cam.cut = cam.cut.and_then(|c| (c > 0).then(|| c - 1)),
                        (Projection::Dim2 { level, .. }, KeyCode::Down) => *level = (*level + 1).min(depth - 1),
                        (Projection::Dim2 { level, .. }, KeyCode::Up) => *level = (*level - 1).max(0),
                        _ => {}
                    }
                }
            }
        }
        true
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut ansi = false;
    let mut blocks = false;
    let mut png_out: Option<PathBuf> = None;
    let mut perspective = 0usize;
    let mut view_file: Option<PathBuf> = None;
    let (mut dir, mut seed, mut speed, mut fps, mut dump, mut size) = (PathBuf::from("games/colony"), None, 10.0f32, 60.0f32, None, (120u16, 40u16));
    while let Some(a) = args.next() {
        match a.as_str() {
            "--seed" => seed = args.next().and_then(|s| s.parse().ok()),
            "--speed" => speed = args.next().and_then(|s| s.parse().ok()).unwrap_or(speed),
            "--fps" => fps = args.next().and_then(|s| s.parse().ok()).unwrap_or(fps),
            "--dump" => dump = args.next().and_then(|s| s.parse::<u32>().ok()),
            "--ansi" => ansi = true,
            "--blocks" => blocks = true,
            "--png" => png_out = args.next().map(PathBuf::from),
            "--view" => view_file = args.next().map(PathBuf::from),
            "--perspective" => perspective = args.next().and_then(|s| s.parse().ok()).unwrap_or(0),
            "--size" => {
                if let Some((w, h)) = args.next().and_then(|s| s.split_once('x').map(|(w, h)| (w.to_string(), h.to_string()))) {
                    size = (w.parse().unwrap_or(size.0), h.parse().unwrap_or(size.1));
                }
            }
            _ => dir = PathBuf::from(a),
        }
    }
    let mut app = match load(&dir, seed, view_file.as_deref()) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    app.ui.speed = speed;
    app.ui.graphics = !blocks && sim_render::pixel::kitty_supported();

    if let Some(ticks) = dump {
        app.step(ticks);
        for p in app.ui.worlds.iter_mut() {
            if let Projection::Layers(l) = p {
                (0..perspective).for_each(|_| l.click());
            }
        }
        let mut canvas = Canvas::new(size.0, size.1);
        if let Some(path) = &png_out {
            app.ui.graphics = true; // render the pixel images at full resolution
            app.draw(&mut canvas);
            let images = app.ui.images.borrow();
            let Some((_, pm)) = images.first() else {
                eprintln!("this view has no pixel component (Diorama)");
                std::process::exit(2);
            };
            std::fs::write(path, sim_render::pixel::png(pm).expect("encodes")).expect("writes");
            eprintln!("wrote {} ({}x{})", path.display(), pm.w, pm.h);
            return;
        }
        let t = Instant::now();
        app.draw(&mut canvas);
        let ms = t.elapsed().as_secs_f32() * 1000.0;
        if ansi {
            // Colours included: `simcraft-view game --dump 300 --ansi > frame.ans; cat frame.ans`.
            let mut out = io::stdout();
            let _ = Renderer::new().present(&canvas, &mut out);
            println!("\x1b[0m");
        } else {
            for y in 0..canvas.h {
                println!("{}", canvas.row_text(y).trim_end());
            }
        }
        eprintln!("frame drawn in {ms:.2} ms");
        return;
    }

    if let Err(e) = run(&mut app, fps) {
        eprintln!("{e}");
    }
}

fn run(app: &mut App, fps: f32) -> io::Result<()> {
    let _guard = TerminalGuard::enter()?;
    let mut out = io::stdout();
    let mut renderer = Renderer::new();
    let (w, h) = crossterm::terminal::size()?;
    let mut canvas = Canvas::new(w, h);
    let measure = |app: &mut App| {
        if let Ok(ws) = crossterm::terminal::window_size()
            && ws.columns > 0
            && ws.rows > 0
            && ws.width > 0
        {
            app.ui.cell_px = (ws.width / ws.columns, ws.height / ws.rows);
        } else {
            app.ui.graphics = false; // pixel size unknown: half-blocks
        }
    };
    measure(app);
    let mut sent: BTreeMap<usize, (u64, sim_render::Rect)> = BTreeMap::new();
    let frame = Duration::from_secs_f32(1.0 / fps.max(1.0));
    let mut sim_clock = 0.0f32;
    let mut last = Instant::now();
    let mut frames = 0u32;
    let mut fps_clock = Instant::now();
    loop {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32();
        app.ui.dt = dt;
        last = now;
        if !app.ui.paused {
            sim_clock += dt * app.ui.speed;
            let n = sim_clock.floor() as u32;
            if n > 0 {
                sim_clock -= n as f32;
                app.step(n.min(2000));
            }
        }
        // How far into the next tick we are: sprites are drawn between the last two ticks.
        app.ui.tween.borrow_mut().alpha = if app.ui.paused { 1.0 } else { sim_clock.clamp(0.0, 1.0) };
        app.draw(&mut canvas);
        renderer.present(&canvas, &mut out)?;
        if app.ui.graphics {
            // Pixel images after the cells; only the ones whose picture or place changed.
            use std::io::Write;
            let images = app.ui.images.borrow();
            for (i, (rect, pm)) in images.iter().enumerate() {
                let fp = pm.fingerprint();
                if sent.get(&i) != Some(&(fp, *rect)) {
                    sim_render::pixel::kitty(&mut out, pm, 100 + i as u32, rect.x, rect.y, rect.w, rect.h)?;
                    sent.insert(i, (fp, *rect));
                }
            }
            out.flush()?;
        }
        frames += 1;
        if fps_clock.elapsed() >= Duration::from_secs(1) {
            app.ui.fps = frames as f32 / fps_clock.elapsed().as_secs_f32();
            frames = 0;
            fps_clock = Instant::now();
        }
        let wait = frame.saturating_sub(now.elapsed());
        if event::poll(wait)? {
            match event::read()? {
                Event::Key(k) if k.kind == KeyEventKind::Press => {
                    if !app.key(k.code) {
                        return Ok(());
                    }
                }
                Event::Mouse(m) if m.kind == MouseEventKind::Down(MouseButton::Left) => app.click(m.column, m.row),
                Event::Resize(w, h) => {
                    canvas = Canvas::new(w, h);
                    measure(app);
                    sent.clear();
                    renderer.invalidate();
                }
                _ => {}
            }
        }
    }
}
