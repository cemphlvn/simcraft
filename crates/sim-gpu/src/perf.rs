//! Frame drops, found and reproduced. Every frame records what it did (phase timings, which are noise, and exact
//! counts, which are not). A frame over budget writes a spike report: the tick, the world's hash, the camera, the
//! phases, and the log of every action since the start. Since the game is deterministic, `replay` rebuilds that
//! exact state from the game's seed and the log, proves it by the hash, and the frame can be rendered again and
//! again under a profiler. A sweep plays a scripted session twice: a spike on the same frame with the same hash
//! both times has a cause in the game or the engine; one that does not came from outside (the OS, the driver).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sim_core::{Engine, Running};
use sim_rules::Game;

use crate::roam::{FrameStats, Roam, RoamPlay};
use crate::track::Press;
use crate::walker::Walker;

/// One frame, measured.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FrameRecord {
    pub frame: u64,
    pub tick: u64,
    /// Moving the body (every frame).
    pub walk_ms: f64,
    /// Handing the frame to the GPU and presenting it (the GPU's own time shows up here when it is behind).
    pub draw_ms: f64,
    /// The window's own work (its title, events).
    #[serde(default)]
    pub window_ms: f64,
    /// Everything this frame did (walk + ticks + build + draw), and the time since the previous frame.
    pub work_ms: f64,
    pub interval_ms: f64,
    #[serde(flatten)]
    pub stats: FrameStats,
}

impl FrameRecord {
    /// The phase that took most of the frame, in words, with its counts.
    pub fn cause(&self) -> String {
        let s = &self.stats;
        let build_rest = (s.build_ms - s.terrain_ms * s.terrain_rebuilt as u8 as f64).max(0.0);
        let mut phases = vec![
            (s.ticks_ms, format!("{} tick(s) of the game {:.2} ms", s.ticks, s.ticks_ms)),
            (build_rest, format!("building the frame {build_rest:.2} ms ({} vertices, {} bodies)", s.verts, s.bodies)),
            (self.draw_ms, format!("drawing and presenting {:.2} ms", self.draw_ms)),
            (self.walk_ms, format!("moving the body {:.2} ms", self.walk_ms)),
            (self.window_ms, format!("the window (title, events) {:.2} ms", self.window_ms)),
        ];
        if s.terrain_rebuilt {
            phases.push((s.terrain_ms, format!("rebuilding the terrain {:.2} ms ({} vertices)", s.terrain_ms, s.terrain_verts)));
        }
        phases.sort_by(|a, b| b.0.total_cmp(&a.0));
        let unaccounted = self.interval_ms - self.work_ms;
        if unaccounted > phases[0].0 && unaccounted > 4.0 {
            return format!("outside this frame's work: {unaccounted:.2} ms between frames (OS, driver, vsync)");
        }
        phases.swap_remove(0).1
    }
}

/// A frame over budget, with everything needed to rebuild it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Spike {
    pub game: PathBuf,
    pub panel: PathBuf,
    pub seed: Option<u64>,
    /// The world's hash after this frame's ticks (hex): the replay must arrive here.
    pub hash: String,
    pub alpha: f32,
    pub smell_on: bool,
    pub cause: String,
    pub record: FrameRecord,
    pub walker: Walker,
    pub log: Vec<Press>,
}

/// Watches frames against a budget; keeps every record for the summary and writes spike reports.
pub struct Watch {
    pub budget_ms: f64,
    pub records: Vec<FrameRecord>,
    pub spikes: Vec<(FrameRecord, String, u64)>,
    /// Where reports go (None: keep them in memory only).
    pub dir: Option<PathBuf>,
    pub written: Vec<PathBuf>,
    last_written: Option<u64>,
}

impl Watch {
    pub fn new(budget_ms: f64, dir: Option<PathBuf>) -> Watch {
        Watch { budget_ms, records: Vec::new(), spikes: Vec::new(), dir, written: Vec::new(), last_written: None }
    }

    /// Records a frame; if it is over budget, keeps it as a spike (and writes a report, at most one a second).
    pub fn frame(&mut self, rec: FrameRecord, play: &RoamPlay, source: (&Path, &Path, Option<u64>)) -> Option<PathBuf> {
        let over = rec.work_ms.max(rec.interval_ms) > self.budget_ms && rec.frame > 30;
        self.records.push(rec.clone());
        if !over {
            return None;
        }
        let hash = play.world().hash();
        let cause = rec.cause();
        self.spikes.push((rec.clone(), cause.clone(), hash));
        let dir = self.dir.clone()?;
        if self.last_written.is_some_and(|f| rec.frame < f + 60) || self.written.len() >= 20 {
            return None;
        }
        self.last_written = Some(rec.frame);
        let spike = Spike {
            game: source.0.to_path_buf(),
            panel: source.1.to_path_buf(),
            seed: source.2,
            hash: format!("{hash:016x}"),
            alpha: play.tween.alpha,
            smell_on: play.smell_on,
            cause,
            record: rec,
            walker: play.walker.clone(),
            log: play.log.clone(),
        };
        let path = dir.join(format!("{}-tick{}-frame{}.json", play.engine.rules().def.name, spike.record.tick, spike.record.frame));
        std::fs::create_dir_all(&dir).ok()?;
        std::fs::write(&path, serde_json::to_string_pretty(&spike).ok()?).ok()?;
        self.written.push(path.clone());
        Some(path)
    }

    /// p50 / p95 / p99 / max of the frames' work and intervals, and the spikes by cause.
    pub fn summary(&self) -> String {
        let pct = |mut v: Vec<f64>, p: f64| -> f64 {
            if v.is_empty() {
                return 0.0;
            }
            v.sort_by(f64::total_cmp);
            v[((v.len() - 1) as f64 * p).round() as usize]
        };
        let line = |name: &str, v: Vec<f64>| {
            format!(
                "{name:9} p50 {:6.2}  p95 {:6.2}  p99 {:6.2}  max {:6.2} ms",
                pct(v.clone(), 0.5),
                pct(v.clone(), 0.95),
                pct(v.clone(), 0.99),
                pct(v, 1.0)
            )
        };
        let mut out = format!(
            "{} frames, budget {:.1} ms, {} over\n{}\n{}",
            self.records.len(),
            self.budget_ms,
            self.spikes.len(),
            line("work", self.records.iter().map(|r| r.work_ms).collect()),
            line("interval", self.records.iter().map(|r| r.interval_ms).collect()),
        );
        for (r, cause, hash) in self.spikes.iter().take(12) {
            out += &format!("\n  frame {:5} tick {:5} hash {hash:016x}: {:.2} ms — {cause}", r.frame, r.tick, r.work_ms.max(r.interval_ms));
        }
        for p in &self.written {
            out += &format!("\n  report: {} (replay: --repro {})", p.display(), p.display());
        }
        out
    }
}

/// Rebuilds the state of a spike: a fresh game from the same panel and seed, the logged actions replayed tick by
/// tick up to the spike's tick, checked against its hash; then the camera as it was.
pub fn replay(spike: &Spike, engine: Engine<Running, Game>, roam: Roam) -> Result<RoamPlay, String> {
    replay_checked(spike, engine, roam, true)
}

/// `replay` up to the spike's tick without checking the hash, still replaying (to time the ticks after it).
pub fn replay_to(spike: &Spike, engine: Engine<Running, Game>, roam: Roam) -> Result<RoamPlay, String> {
    replay_checked(spike, engine, roam, false)
}

fn replay_checked(spike: &Spike, engine: Engine<Running, Game>, roam: Roam, check: bool) -> Result<RoamPlay, String> {
    let mut play = RoamPlay::new(engine, roam);
    play.script = Some((spike.log.clone(), 0));
    while play.world().tick < spike.record.tick {
        let before = play.world().tick;
        play.step(1);
        if play.world().tick == before {
            return Err(format!("the game ended at tick {before}, before the spike's tick {}", spike.record.tick));
        }
    }
    if let Some((why, _)) = &play.refused {
        return Err(why.clone());
    }
    let hash = format!("{:016x}", play.world().hash());
    if check && hash != spike.hash {
        return Err(format!(
            "tick {}: hash {hash}, the spike's {} — not the same state (another game.ron, engine.toml or engine build?)",
            spike.record.tick, spike.hash
        ));
    }
    play.walker = spike.walker.clone();
    play.tween.alpha = spike.alpha;
    play.smell_on = spike.smell_on;
    if check {
        play.script = None;
    }
    Ok(play)
}

/// A player that is the same every run: walks, sweeps the view, hops, digs and drops, holds the smell key.
pub fn scripted_input(frame: u64) -> (crate::walker::Input, Option<bool>, bool) {
    let t = frame as f32 / 60.0;
    let input = crate::walker::Input {
        forward: 1.0,
        strafe: if (frame / 240).is_multiple_of(2) { 0.0 } else { 0.6 },
        run: (frame / 180) % 3 == 2,
        jump: frame % 150 < 3,
        look: ((t * 0.9).sin() * 6.0, (t * 0.5).cos() * 1.5),
    };
    let use_target = match frame % 240 {
        100 => Some(true),
        220 => Some(false),
        _ => None,
    };
    (input, use_target, (frame / 300) % 2 == 1)
}

/// An offscreen target the size of a window.
pub fn offscreen(gpu: &crate::gpu::Gpu, w: u32, h: u32) -> wgpu::TextureView {
    gpu.device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("perf"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: gpu.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

/// Plays `frames` frames of the scripted player at 60 fps game time, headless, each one measured into `watch`.
/// `smell`: hold the smell key the whole time (Some(true)), never, or as the script does (None).
#[allow(clippy::too_many_arguments)]
pub fn run_frames(
    gpu: &mut crate::gpu::Gpu,
    play: &mut RoamPlay,
    frames: u64,
    (w, h): (u32, u32),
    watch: &mut Watch,
    smell: Option<bool>,
    source: (&Path, &Path, Option<u64>),
) {
    use std::time::Instant;
    let target = offscreen(gpu, w, h);
    let rate = play.engine.rules().cfg.run.tick_rate as f32;
    let dt = 1.0 / 60.0;
    let mut clock = 0.0f32;
    let mut last = Instant::now();
    let smell_override = smell;
    for frame in 0..frames {
        let start = Instant::now();
        let (input, use_target, smell) = scripted_input(frame);
        let smell_key = smell_override;
        play.stats = FrameStats::default();
        play.smell_on = smell_key.unwrap_or(smell);
        let t = Instant::now();
        play.walk(dt, input);
        let walk_ms = t.elapsed().as_secs_f64() * 1000.0;
        if let Some(dig) = use_target {
            play.use_target(dig);
        }
        clock += dt * rate;
        let n = clock.floor() as u32;
        clock -= n as f32;
        play.step(n);
        play.tween.alpha = clock;
        let fr = play.frame(w as f32, h as f32, dt);
        let t = Instant::now();
        play.render(gpu, &target, w, h, &fr);
        let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        let draw_ms = t.elapsed().as_secs_f64() * 1000.0;
        let rec = FrameRecord {
            frame,
            tick: play.world().tick,
            walk_ms,
            draw_ms,
            window_ms: 0.0,
            work_ms: start.elapsed().as_secs_f64() * 1000.0,
            interval_ms: last.elapsed().as_secs_f64() * 1000.0,
            stats: play.stats.clone(),
        };
        last = Instant::now();
        watch.frame(rec, play, source);
    }
}

/// A frame over budget in a sweep: where, the state's hash, why.
#[derive(Clone, Debug, PartialEq)]
pub struct SweepSpike {
    pub frame: u64,
    pub tick: u64,
    pub hash: u64,
    pub cause: String,
}

/// Spikes that came back in both runs (same frame, same hash): caused by the game or the engine, reproducible;
/// and the ones seen once: from outside.
pub fn compare_runs(a: &Watch, b: &Watch) -> (Vec<SweepSpike>, Vec<SweepSpike>) {
    let spikes = |w: &Watch| -> Vec<SweepSpike> {
        w.spikes.iter().map(|(r, c, h)| SweepSpike { frame: r.frame, tick: r.tick, hash: *h, cause: c.clone() }).collect()
    };
    let (sa, sb) = (spikes(a), spikes(b));
    let again = |s: &SweepSpike, other: &[SweepSpike]| other.iter().any(|o| o.frame == s.frame && o.hash == s.hash);
    let both = sa.iter().filter(|s| again(s, &sb)).cloned().collect();
    let once = sa.iter().filter(|s| !again(s, &sb)).chain(sb.iter().filter(|s| !again(s, &sa))).cloned().collect();
    (both, once)
}

/// A load pushed to a level: many of a kind, a world full of built terrain, or a field (the smell) everywhere.
#[derive(Clone, Debug)]
pub enum Load {
    /// This many entities of a kind (the panel's [spawn]).
    Kind(String, u32),
    /// This share (%) of the world's columns built up with terrain value `value`, to random heights.
    Terrain(i64, u32),
    /// This field at `value` in this share (%) of the open voxels (what the view draws when it is shown).
    Field(String, i64, u32),
}

impl Load {
    pub fn describe(&self) -> String {
        match self {
            Load::Kind(k, n) => format!("{n} {k}"),
            Load::Terrain(v, pct) => format!("terrain {v} in {pct}% of columns"),
            Load::Field(f, _, pct) => format!("{f} in {pct}% of the air"),
        }
    }

    /// The panel with this load's spawn count.
    pub fn panel(&self, panel: &str) -> String {
        let Load::Kind(kind, n) = self else { return panel.to_string() };
        let mut out: Vec<String> = Vec::new();
        let (mut in_spawn, mut done) = (false, false);
        for line in panel.lines() {
            let t = line.trim_start();
            if t.starts_with('[') {
                if in_spawn && !done {
                    out.push(format!("{kind} = {n}"));
                    done = true;
                }
                in_spawn = t.starts_with("[spawn]");
            }
            if in_spawn && t.starts_with(&format!("{kind} =")) {
                out.push(format!("{kind} = {n}"));
                done = true;
                continue;
            }
            out.push(line.to_string());
        }
        if !done {
            out.push(format!("[spawn]\n{kind} = {n}"));
        }
        out.join("\n")
    }

    /// The world with this load's terrain or field (before the game starts).
    pub fn world(&self, world: &mut sim_core::World, terrain: &str, seed: u64) {
        let (w, h, d) = (world.width, world.height, world.depth);
        let top = (0..d).find(|&z| world.is_terrain(0, 0, z)).unwrap_or(d);
        match self {
            Load::Kind(..) => {}
            Load::Terrain(value, pct) => {
                for y in 0..h {
                    for x in 0..w {
                        let r = sim_core::splitmix64(seed ^ ((y * w + x) as u64));
                        if r % 100 < *pct as u64 {
                            let height = 1 + (r >> 8) % (top.max(1) as u64);
                            for z in (top - height as i64).max(0)..top {
                                world.set_field(terrain, x, y, z, *value);
                            }
                        }
                    }
                }
            }
            Load::Field(field, value, pct) => {
                for z in 0..d {
                    for y in 0..h {
                        for x in 0..w {
                            let r = sim_core::splitmix64(seed ^ (((z * h + y) * w + x) as u64) ^ 0x534D_4F4B);
                            if !world.is_terrain(x, y, z) && r % 100 < *pct as u64 {
                                world.set_field(field, x, y, z, *value);
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The loads a roam view can be stressed with: each drawn kind, the terrain, the shown field; four levels each.
pub fn loads(roam: &Roam, base_count: impl Fn(&str) -> u32) -> Vec<Vec<Load>> {
    let mut out: Vec<Vec<Load>> = roam
        .kinds
        .keys()
        .map(|k| {
            let n = base_count(k).max(10);
            [1, 4, 8, 16].into_iter().map(|m| Load::Kind(k.clone(), n * m)).collect()
        })
        .collect();
    let built = roam.materials.keys().copied().max().unwrap_or(1);
    out.push([10, 30, 60, 100].into_iter().map(|p| Load::Terrain(built, p)).collect());
    if let Some(s) = &roam.smell {
        out.push([5, 20, 50, 100].into_iter().map(|p| Load::Field(s.field.clone(), s.full, p)).collect());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::Loaded;

    fn mound() -> (Engine<Running, Game>, Roam) {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../games/mound");
        let (world, game) = Game::load(&dir, None).expect("loads");
        let engine = Engine::<Loaded, _>::new(world, game).validate().expect("valid").start();
        (engine, crate::load_roam(&dir).expect("roam").0)
    }

    /// The scripted player for `frames` frames, no GPU: the body, the ticks, digging and dropping, the frame.
    fn play_script(play: &mut RoamPlay, frames: u64) {
        let (dt, mut clock) = (1.0 / 60.0, 0.0f32);
        for frame in 0..frames {
            let (input, use_target, smell) = scripted_input(frame);
            play.stats = FrameStats::default();
            play.smell_on = smell;
            play.walk(dt, input);
            if let Some(dig) = use_target {
                play.use_target(dig);
            }
            clock += dt * 20.0;
            let n = clock.floor() as u32;
            clock -= n as f32;
            play.step(n);
            play.tween.alpha = clock;
            play.frame(320.0, 180.0, dt);
        }
    }

    #[test]
    fn a_spike_rebuilds_to_the_same_state_by_its_hash() {
        let (engine, roam) = mound();
        let mut play = RoamPlay::new(engine, roam.clone());
        play_script(&mut play, 700);
        assert!(play.log.iter().any(|p| p.action != "crawl"), "the script dug or dropped: {:?}", play.log.len());
        let spike = Spike {
            game: PathBuf::new(),
            panel: PathBuf::new(),
            seed: None,
            hash: format!("{:016x}", play.world().hash()),
            alpha: play.tween.alpha,
            smell_on: play.smell_on,
            cause: String::new(),
            record: FrameRecord { tick: play.world().tick, ..FrameRecord::default() },
            walker: play.walker.clone(),
            log: play.log.clone(),
        };
        let seen = (play.stats.verts, play.stats.bodies);
        let (engine, _) = mound();
        let mut again = replay(&spike, engine, roam.clone()).expect("the same state");
        again.frame(320.0, 180.0, 0.0);
        assert_eq!((again.stats.verts, again.stats.bodies), seen, "the same frame: vertices and bodies drawn");

        // A log from another game (or a changed rule) does not arrive at the hash: said, not hidden.
        let mut wrong = spike;
        wrong.hash = "0000000000000000".into();
        let (engine, _) = mound();
        let err = replay(&wrong, engine, roam).err().expect("must fail");
        assert!(err.contains("not the same state"), "{err}");
    }

    #[test]
    fn a_load_sets_its_spawn_count_and_nothing_else() {
        let panel = "[run]\nseed = 1\n\n[spawn]\ntermite = 120\nyou = 1\n\n[agent]\ncontrollable = [\"you\"]\n";
        let out = Load::Kind("termite".into(), 960).panel(panel);
        assert!(out.contains("termite = 960") && out.contains("you = 1") && !out.contains("termite = 120"), "{out}");
        let added = Load::Kind("queen".into(), 3).panel(panel);
        assert!(added.contains("[spawn]\ntermite = 120\nyou = 1\n\nqueen = 3") || added.contains("queen = 3"), "{added}");
    }

    #[test]
    fn only_spikes_on_the_same_frame_and_state_count_as_the_games() {
        let rec = |frame| FrameRecord { frame, ..FrameRecord::default() };
        let mut a = Watch::new(1.0, None);
        let mut b = Watch::new(1.0, None);
        a.spikes = vec![(rec(40), "terrain".into(), 7), (rec(90), "tick".into(), 9)];
        b.spikes = vec![(rec(40), "terrain".into(), 7), (rec(90), "tick".into(), 8), (rec(120), "draw".into(), 3)];
        let (both, once) = compare_runs(&a, &b);
        assert_eq!(both.iter().map(|s| s.frame).collect::<Vec<_>>(), vec![40], "same frame, same hash");
        assert_eq!(once.len(), 3, "frame 90 in another state (twice), frame 120 in one run");
    }
}
