//! simcraft-feel: measure game feel, eval-driven (like tools/eval.py, for how a game *looks in motion*).
//!
//!   simcraft-feel games/colony3d --view games/colony3d/views/generated.ron
//!   simcraft-feel ... --set interpolate=false --set stiffness=0      # try settings without editing the view
//!   simcraft-feel ... --save "spring camera"                         # record a step, compare with the last one
//!
//! Plays the game headless at a fixed frame rate with the viewer's own code (display list, tween, camera spring),
//! following one ant, and measures what a player feels. Same seeds every run: numbers only move when the feel does.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use sim_core::{Engine, Loaded, Running, World};
use sim_render::diorama::DioramaProps;
use sim_render::feel::{Feel, Spring, Tween};
use sim_render::{Assets, Node, View, display};
use sim_rules::Game;

const SEEDS: [u64; 3] = [1, 2, 3];
/// A typical window, in art pixels (the view is 72 px tall; a 16:9 window at integer scale is ~160 px wide).
const VIEW_W: i64 = 160;

struct Settings {
    feel: Feel,
    fps: f32,
    speed: f32,
    seconds: f32,
}

#[derive(Default, Clone, Copy)]
struct Metrics {
    /// Largest camera move between two frames (px). Lower = no lurches.
    cam_jump: f32,
    /// Mean change of camera velocity per frame (px/frame²). Lower = smoother.
    cam_jerk: f32,
    /// Mean distance between the camera and where it wants to be (px). Lower = more responsive.
    cam_lag: f32,
    /// 95th percentile of a moving sprite's step between frames (px). Lower = gliding, not hopping.
    sprite_step_p95: f32,
    /// Share of frames in which a sprite that is walking does not move on screen. Lower = no stutter.
    stutter: f32,
    /// Mean time to build and rasterize one frame (ms). Info.
    frame_ms: f32,
}

const NAMES: [(&str, &str, &str); 6] = [
    ("cam_jump", "min", "largest camera move in one frame (px)"),
    ("cam_jerk", "min", "camera jerk (px/frame²)"),
    ("cam_lag", "min", "camera behind its goal (px)"),
    ("sprite_step_p95", "min", "a walking sprite's step per frame, p95 (px)"),
    ("stutter", "min", "walking but frozen on screen (share of frames)"),
    ("frame_ms", "info", "frame cost (ms)"),
];

impl Metrics {
    fn get(&self, name: &str) -> f32 {
        match name {
            "cam_jump" => self.cam_jump,
            "cam_jerk" => self.cam_jerk,
            "cam_lag" => self.cam_lag,
            "sprite_step_p95" => self.sprite_step_p95,
            "stutter" => self.stutter,
            _ => self.frame_ms,
        }
    }
}

fn boot(dir: &Path, seed: u64) -> Result<Engine<Running, Game>, String> {
    let panel = std::fs::read_to_string(dir.join("engine.toml")).map_err(|e| e.to_string())?;
    let panel: String = panel
        .lines()
        .map(|l| if l.trim_start().starts_with("seed") { format!("seed = {seed}") } else { l.to_string() })
        .collect::<Vec<_>>()
        .join("\n");
    let (world, game) = Game::load_panel(dir, &panel)?;
    Ok(Engine::<Loaded, _>::new(world, game).validate().map_err(|e| e.join("; "))?.start())
}

/// The ant to follow: the first entity of the most numerous kind with a state machine (as the viewer picks).
fn follow(w: &World, g: &Game) -> Option<u64> {
    let mut kinds: Vec<&str> =
        g.def.kinds.keys().map(String::as_str).filter(|k| !g.is_hidden(k) && g.def.kinds[*k].fsm.is_some() && w.count(k) <= 200).collect();
    kinds.sort_by_key(|k| std::cmp::Reverse(w.count(k)));
    kinds.first().and_then(|k| w.of_kind(k).next()).map(|e| e.id)
}

fn run(dir: &Path, props: &DioramaProps, assets: &Assets, s: &Settings, seed: u64) -> Result<Metrics, String> {
    let mut engine = boot(dir, seed)?;
    let frames = (s.seconds * s.fps) as usize;
    let dt = 1.0 / s.fps;
    let (mut tween, mut spring, mut facing) = (Tween::default(), Spring::default(), BTreeMap::new());
    let (mut clock, mut frame) = (0.0f32, 0u64);
    let mut selected = follow(engine.world(), engine.rules());
    let (mut cams, mut lags) = (Vec::new(), Vec::new());
    let mut last_pos: BTreeMap<u64, (f32, f32)> = BTreeMap::new();
    let (mut steps, mut walking, mut frozen) = (Vec::new(), 0usize, 0usize);
    let mut cost = 0.0f64;
    let mut strips = None;
    let mut section = None;
    for _ in 0..frames {
        clock += dt * s.speed;
        while clock >= 1.0 {
            clock -= 1.0;
            tween.remember(engine.world());
            engine.tick();
        }
        if engine.outcome().is_some() {
            break;
        }
        if selected.and_then(|id| engine.world().get(id)).is_none() {
            selected = follow(engine.world(), engine.rules());
        }
        tween.alpha = clock;
        frame += 1;
        let t0 = Instant::now();
        let scene = sim_render::Scene { world: engine.world(), game: engine.rules(), assets };
        let list = display::compose(&scene, props, frame, selected, &mut facing, s.feel.interpolate.then_some(&tween), s.feel.walk_bob);
        if strips.as_ref().is_none_or(|(k, _)| *k != list.strips_key) {
            strips = Some((list.strips_key, display::cook_strips(&scene, props, &list)?));
        }
        if section.as_ref().is_none_or(|(k, _)| *k != list.section_key) {
            section = Some((list.section_key, display::cook_section(&scene, props, &list)));
        }
        let target = list.sprites.iter().find(|d| d.selected).map(|d| d.x + props.tile as f32 / 2.0);
        let goal = display::camera(list.world_px, target.map_or(list.world_px / 2, |t| t.round() as i64), VIEW_W) as f32;
        let cam = spring.update(goal, dt, s.feel.camera);
        let _ = display::rasterize(
            &scene,
            &list,
            &strips.as_ref().unwrap().1,
            &section.as_ref().unwrap().1,
            VIEW_W as usize,
            cam.round() as i64,
            frame,
        );
        cost += t0.elapsed().as_secs_f64();
        cams.push(cam.round());
        lags.push((cam - goal).abs());
        // Sprite motion on screen, by entity (sprites carry their entity only through order; use the world ids).
        let ids: Vec<u64> = sprite_ids(&scene, props);
        for (d, id) in list.sprites.iter().zip(ids) {
            let now = (d.x.round(), d.floor.round());
            // Walking in a direction the side view shows (along x or between levels); depth (y) moves barely show.
            let in_sim = tween.prev.get(&id).zip(engine.world().get(id)).is_some_and(|(p, e)| p.0 != e.x || p.2 != e.z);
            if let Some(&before) = last_pos.get(&id) {
                let step = (now.0 - before.0).abs().max((now.1 - before.1).abs());
                if step <= 3.0 * props.tile as f32 && (step > 0.0 || in_sim) {
                    steps.push(step);
                }
                if in_sim {
                    walking += 1;
                    if step == 0.0 {
                        frozen += 1;
                    }
                }
            }
            last_pos.insert(id, now);
        }
    }
    let n = cams.len().max(3);
    let vel: Vec<f32> = cams.windows(2).map(|w| w[1] - w[0]).collect();
    steps.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Ok(Metrics {
        cam_jump: vel.iter().fold(0.0f32, |m, v| m.max(v.abs())),
        cam_jerk: vel.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>() / (n - 2) as f32,
        cam_lag: lags.iter().sum::<f32>() / lags.len().max(1) as f32,
        sprite_step_p95: steps.get(steps.len() * 95 / 100).copied().unwrap_or(0.0),
        stutter: frozen as f32 / walking.max(1) as f32,
        frame_ms: (cost * 1000.0 / cams.len().max(1) as f64) as f32,
    })
}

/// Entity ids in the order `compose` emits sprites (the same filter and sort).
fn sprite_ids(scene: &sim_render::Scene, p: &DioramaProps) -> Vec<u64> {
    let mut ents: Vec<_> = scene.world.entities().values().filter(|e| scene.visible(e)).collect();
    ents.sort_by_key(|e| (e.z > 0, e.y, e.id));
    ents.into_iter()
        .filter(|e| {
            scene.assets.look(e, scene.game.state_label(e)).sprite.is_some_and(|s| scene.assets.sprites.contains_key(&s))
                && !(e.z > 0 && (e.y - p.plane).abs() > 3)
        })
        .map(|e| e.id)
        .collect()
}

fn find_diorama(n: &Node) -> Option<&ron::Value> {
    match n {
        Node::Rows(v) | Node::Cols(v) => v.iter().find_map(|(_, c)| find_diorama(c)),
        Node::C { name, props, .. } if name == "Diorama" => Some(props),
        _ => None,
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let (mut dir, mut view_path, mut sets, mut save) = (PathBuf::from("games/colony3d"), None, Vec::new(), None);
    let mut s = Settings { feel: Feel::default(), fps: 60.0, speed: 0.0, seconds: 40.0 };
    while let Some(a) = args.next() {
        match a.as_str() {
            "--view" => view_path = args.next().map(PathBuf::from),
            "--set" => sets.extend(args.next()),
            "--save" => save = args.next(),
            "--fps" => s.fps = args.next().and_then(|v| v.parse().ok()).unwrap_or(s.fps),
            "--speed" => s.speed = args.next().and_then(|v| v.parse().ok()).unwrap_or(s.speed),
            "--seconds" => s.seconds = args.next().and_then(|v| v.parse().ok()).unwrap_or(s.seconds),
            _ => dir = PathBuf::from(a),
        }
    }
    if save.is_some() && !sets.is_empty() {
        eprintln!("--set is for trying; put the change in the view, then --save");
        std::process::exit(2);
    }
    let view_path = view_path.unwrap_or_else(|| dir.join("view.ron"));
    let view: View = ron::Options::default()
        .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
        .from_str(&std::fs::read_to_string(&view_path).expect("reads the view"))
        .unwrap_or_else(|e| panic!("{}: {e}", view_path.display()));
    s.feel = view.feel.clone();
    for kv in &sets {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        match k {
            "interpolate" => s.feel.interpolate = v == "true",
            "stiffness" => s.feel.camera.stiffness = v.parse().unwrap_or(0.0),
            "damping" => s.feel.camera.damping = v.parse().unwrap_or(1.0),
            "bob" | "walk_bob" => s.feel.walk_bob = v.parse().unwrap_or(0),
            _ => {
                eprintln!("unknown setting '{k}' (interpolate, stiffness, damping, bob)");
                std::process::exit(2);
            }
        }
    }
    if s.speed <= 0.0 {
        // 1x: the operator's tick rate from engine.toml.
        s.speed = boot(&dir, SEEDS[0]).unwrap_or_else(|e| panic!("{e}")).rules().cfg.run.tick_rate as f32;
    }
    let props: DioramaProps = sim_render::props(find_diorama(&view.layout).expect("the view has a Diorama")).expect("props");
    let mut assets = Assets::default();
    for pack in &view.assets {
        let path = std::iter::once(dir.join(format!("{pack}.ron")))
            .chain(dir.ancestors().map(|a| a.join("assets").join(format!("{pack}.ron"))))
            .find(|p| p.exists())
            .expect("asset pack");
        let mut p: Assets = ron::Options::default()
            .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
            .from_str(&std::fs::read_to_string(&path).unwrap())
            .unwrap();
        p.load_images(path.parent().unwrap()).unwrap();
        assets = assets.merged(p);
    }

    let per: Vec<Metrics> = SEEDS.iter().map(|&seed| run(&dir, &props, &assets, &s, seed).unwrap_or_else(|e| panic!("{e}"))).collect();
    let mean = |name: &str| per.iter().map(|m| m.get(name)).sum::<f32>() / per.len() as f32;

    let history_dir = dir.join("feel-evals");
    let mut history: Vec<PathBuf> =
        std::fs::read_dir(&history_dir).map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).collect()).unwrap_or_default();
    history.sort();
    let last: Option<BTreeMap<String, f32>> = history.last().and_then(|p| {
        let text = std::fs::read_to_string(p).ok()?;
        Some(
            text.lines()
                .filter_map(|l| {
                    let (k, v) = l.split_once(':')?;
                    Some((k.trim().trim_matches('"').to_string(), v.trim().trim_end_matches(',').parse().ok()?))
                })
                .collect(),
        )
    });
    println!(
        "{}: feel at {} fps, {} ticks/s, {} s, seeds {:?}   interpolate={} stiffness={} damping={} bob={}{}",
        dir.display(),
        s.fps,
        s.speed,
        s.seconds,
        SEEDS,
        s.feel.interpolate,
        s.feel.camera.stiffness,
        s.feel.camera.damping,
        s.feel.walk_bob,
        history.last().map(|p| format!("   (vs {})", p.file_name().unwrap().to_string_lossy())).unwrap_or_default()
    );
    for (name, goal, what) in NAMES {
        let now = mean(name);
        let before = last.as_ref().and_then(|l| l.get(name)).copied();
        let verdict = match (goal, before) {
            ("min", Some(b)) if (now - b).abs() < 0.005 => "=",
            ("min", Some(b)) if now < b => "better",
            ("min", Some(_)) => "worse",
            _ => "",
        };
        println!(
            "  {name:<16} {now:>9.3}   {:>9}  {verdict:<6}  {goal:<4}  {what}",
            before.map_or_else(|| "-".to_string(), |b| format!("{b:.3}"))
        );
    }
    if let Some(note) = save {
        std::fs::create_dir_all(&history_dir).expect("creates feel-evals");
        let slug: String = note.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { '-' }).collect();
        let path = history_dir.join(format!("{:03}-{}.json", history.len(), &slug[..slug.len().min(40)]));
        let mut out = format!("{{\n  \"step\": {},\n  \"change\": \"{}\",\n", history.len(), note.replace('"', "'"));
        out += &format!(
            "  \"interpolate\": {},\n  \"stiffness\": {},\n  \"damping\": {},\n  \"walk_bob\": {},\n",
            s.feel.interpolate, s.feel.camera.stiffness, s.feel.camera.damping, s.feel.walk_bob
        );
        let body: Vec<String> = NAMES.iter().map(|(n, ..)| format!("  \"{n}\": {:.4}", mean(n))).collect();
        out += &body.join(",\n");
        out += "\n}\n";
        std::fs::write(&path, out).expect("writes");
        println!("saved {}", path.display());
    }
}
