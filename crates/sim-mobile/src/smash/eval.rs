//! SMASH's evals (`games/smash/eval.toml`, log in `games/smash/EVALS.md`): every scripted shot is played on a
//! fresh tower through the same calls a finger makes, at 60 ticks a second, and measured. Saved steps live in
//! `games/smash/evals/NNN-<name>.json`; a run is compared with the last saved step against each metric's goal.

use std::collections::BTreeMap;
use std::path::Path;

use sim_physics::rigid::V3;

use super::tuning::Tuning;
use super::{Event, Smash};
use crate::gesture::Px;
use crate::playground::{Layout, TICK_RATE};

/// A scripted pull: where the finger goes down and where it lets go (shares of the screen), how long it takes.
#[derive(Clone, Copy, Debug)]
pub struct Script {
    pub name: &'static str,
    pub from: (f32, f32),
    pub to: (f32, f32),
    pub pull_s: f32,
}

/// The fixed shots, the evals' "seeds".
pub fn scripts(t: &Tuning) -> Vec<Script> {
    let full = t.sling.pull_screen * 1.05;
    let s = |name, dx: f32, k: f32| Script { name, from: (0.5, 0.55), to: (0.5 + dx, 0.55 + full * k), pull_s: 0.45 };
    vec![
        s("center_full", 0.0, 1.0),
        s("center_mid", 0.0, 0.6),
        s("center_soft", 0.0, 0.3),
        s("aim_left", 0.07, 0.85),
        s("aim_right", -0.07, 0.85),
    ]
}

pub const SCREEN: (f32, f32) = (1179.0, 2556.0);

/// The level the scripted shots and the precision probe play, whatever comes before it in the game (the series
/// stays comparable as levels are added: it is the tower steps 000–011 were measured on).
pub const EVAL_LEVEL: &str = "FORTRESS";

/// Called every tick with the game, the tick (since touch-down) and seconds since the release (negative before).
pub type Watch<'a> = dyn FnMut(&Smash, u64, f32) + 'a;

/// Plays `script` on a fresh tower and returns its metrics.
/// `layout` is the screen it is played on (the evals use [`SCREEN`]; pictures their own size).
pub fn run(t: &Tuning, script: &Script, layout: &Layout, watch: Option<&mut Watch>) -> BTreeMap<&'static str, f64> {
    run_on(t, EVAL_LEVEL, script, layout, watch)
}

/// [`run`] on level `level`.
pub fn run_on(t: &Tuning, level: &str, script: &Script, layout: &Layout, mut watch: Option<&mut Watch>) -> BTreeMap<&'static str, f64> {
    let mut g = Smash::at_level(t.clone(), level);
    crate::playground::Card::layout(&mut g, layout);
    let (w, h) = layout.screen;
    let px = |f: (f32, f32)| Px::new(f.0 * w, f.1 * h);
    let mut out = Vec::new();
    let dt = 1.0 / TICK_RATE as f32;
    let idle = (0.3 * TICK_RATE as f32) as u64;
    let pull = (script.pull_s * TICK_RATE as f32).max(1.0) as u64;
    let hold = (0.15 * TICK_RATE as f32) as u64;
    let release = idle + pull + hold;
    let end = release + (8.0 * TICK_RATE as f32) as u64;
    let mut eyes: Vec<V3> = Vec::new();
    let mut cam_jerk = 0.0f64;
    let mut tower_in_view = 1.0f64;
    let (mut flight_ticks, mut stone_seen) = (0u32, 0u32);
    let mut impact_in_view = 0.0;
    let mut settle = 8.0f64;
    let (mut peak_awake, mut peak_pen) = (0u32, 0.0f32);
    let (mut step_sum, mut steps) = (0.0f64, 0u32);
    let mut step_times: Vec<f64> = Vec::new();
    let mut trauma_peak = 0.0f32;
    let mut first_hit: Option<(f64, V3)> = None;
    let mut hit_normal = V3::Y;
    let (mut aim_ticks, mut pouch_seen) = (0u32, 0u32);
    let (mut after_ticks, mut fg_clear) = (0u32, 0u32);
    let mut fg_share = 0.0f64;
    for tick in 0..end {
        if tick == idle {
            g.press(px(script.from), &mut out);
        } else if tick > idle && tick <= idle + pull {
            let k = (tick - idle) as f32 / pull as f32;
            let at = (script.from.0 + (script.to.0 - script.from.0) * k, script.from.1 + (script.to.1 - script.from.1) * k);
            g.pull(px(at), &mut out);
        } else if tick == release {
            g.release(&mut out);
        }
        g.tick(&mut out);
        let since = (tick as f32 - release as f32) * dt;
        if let Some(w) = watch.as_mut() {
            w(&g, tick, since);
        }
        // Camera smoothness on the spring's pose (shake is wanted motion and measured as trauma).
        eyes.push(g.rig.pose.eye);
        if eyes.len() >= 4 {
            let n = eyes.len();
            let j = eyes[n - 1] - eyes[n - 2] * 3.0 + eyes[n - 3] * 3.0 - eyes[n - 4];
            cam_jerk = cam_jerk.max((j.length() * (TICK_RATE as f32).powi(3)) as f64);
        }
        trauma_peak = trauma_peak.max(g.rig.trauma);
        let cam = g.rig.camera(1.0);
        let on = |p: V3, margin: f32| {
            let (x, y, front) = cam.project(p, w, h);
            front && x > w * margin && x < w * (1.0 - margin) && y > h * margin && y < h * (1.0 - margin)
        };
        if tick > release {
            // The slingshot's highest point (a stone in the pouch) below the pedestal's foot: it frames the shot
            // without covering it.
            let sl = &g.t.sling;
            let top = V3::new(sl.at.0, sl.at.1 + g.t.stone.radius * 2.0, sl.at.2);
            let (_, y_sling, _) = cam.project(top, w, h);
            let (_, y_foot, _) = cam.project(V3::new(0.0, 0.1, 0.0), w, h);
            after_ticks += 1;
            fg_clear += u32::from(y_sling > y_foot);
            // While watching (from 0.3 s): the fork, or the stone in the pouch once it is loaded again.
            if since > 0.3 {
                let lift = if g.loaded { g.t.stone.radius * 2.0 } else { 0.05 };
                let (_, y_top, _) = cam.project(V3::new(sl.at.0, sl.at.1 + lift, sl.at.2), w, h);
                fg_share = fg_share.max(((h - y_top) / h).clamp(0.0, 1.0) as f64);
            }
        }
        if g.drag.is_some() {
            aim_ticks += 1;
            let pouch = g.aim.from;
            let r = g.t.stone.radius;
            pouch_seen += u32::from(on(pouch + V3::new(0.0, -r, 0.0), 0.02) && on(pouch + V3::new(0.0, r * 1.6, 0.0), 0.02));
        }
        let hit_yet = g.stones.iter().any(|s| s.hit.is_some()) || first_hit.is_some();
        if tick >= idle && !hit_yet {
            let pieces: Vec<V3> =
                g.pieces.iter().filter(|p| p.kind.is_piece() && !p.cleared).filter_map(|p| g.world.get(p.id).map(|b| b.pos)).collect();
            if !pieces.is_empty() {
                let seen = pieces.iter().filter(|&&p| on(p, 0.02)).count();
                tower_in_view = tower_in_view.min(seen as f64 / pieces.len() as f64);
            }
        }
        if tick > release
            && !hit_yet
            && let Some(b) = g.stones.last().and_then(|s| g.world.get(s.id))
        {
            flight_ticks += 1;
            stone_seen += u32::from(on(b.pos, 0.0));
        }
        if first_hit.is_none() {
            for e in &g.log {
                if let Event::StoneHit { tick: et, contact, normal, .. } = *e {
                    let at = contact;
                    hit_normal = normal;
                    first_hit = Some(((et as f64 - release as f64 - 1.0) * dt as f64, at));
                    impact_in_view = if on(at, 0.05) { 1.0 } else { 0.0 };
                }
            }
        }
        let st = g.world.stats();
        peak_awake = peak_awake.max(st.awake);
        if st.max_penetration > peak_pen && std::env::var_os("SMASH_DEBUG_PEN").is_some() {
            eprintln!(
                "{} tick {tick}: {:.1} mm between {:?}",
                script.name,
                st.max_penetration * 1000.0,
                st.deepest.map(|(a, b)| (super::Kind::of_user(a), super::Kind::of_user(b)))
            );
        }
        peak_pen = peak_pen.max(st.max_penetration);
        if tick > release {
            step_sum += g.step_us as f64;
            step_times.push(g.step_us as f64);
            steps += 1;
            if st.awake == 0 && settle >= 8.0 && since > 0.1 {
                settle = since as f64;
            }
            if st.awake == 0 && since > 0.5 && g.chips.list.is_empty() {
                break;
            }
        }
    }
    let predicted = g.log.iter().find_map(|e| if let Event::Shot { predicted, .. } = *e { predicted } else { None });
    let aim_error = match (predicted, first_hit) {
        // Measured across the hit surface: where on the target, which is what a player sees. Along the normal the
        // solver's point is an anchor from the step's start (in the air for a speculative contact), so not comparable.
        (Some(p), Some((_, at))) => {
            let d = p - at;
            (d - hit_normal * d.dot(hit_normal)).length() as f64
        }
        // A preview that saw no target, or a stone that touched nothing: the worst honest value.
        _ => 5.0,
    };
    let total = g.total().max(1);
    let cleared = (total - g.left().min(total)) as f64 / total as f64;
    let count = |f: fn(&Event) -> bool| g.log.iter().filter(|e| f(e)).count() as f64;
    let mut m = BTreeMap::new();
    m.insert("cleared_pct", cleared * 100.0);
    m.insert("first_hit_s", first_hit.map_or(6.0, |f| f.0));
    m.insert("aim_error_m", aim_error);
    m.insert("settle_s", settle);
    m.insert("tower_in_view", tower_in_view);
    m.insert("impact_in_view", impact_in_view);
    m.insert("stone_in_view", if flight_ticks > 0 { stone_seen as f64 / flight_ticks as f64 } else { 0.0 });
    m.insert("cam_jerk", cam_jerk);
    m.insert("hitstop", count(|e| matches!(e, Event::HitStop { .. })).min(1.0));
    m.insert("slowmo", count(|e| matches!(e, Event::SlowMo { .. })).min(1.0));
    m.insert("trauma_peak", trauma_peak as f64);
    m.insert("pulses", count(|e| matches!(e, Event::Pulse { .. })));
    m.insert("fg_clear", if after_ticks > 0 { fg_clear as f64 / after_ticks as f64 } else { 0.0 });
    m.insert("fg_share", fg_share);
    m.insert("pouch_in_view", if aim_ticks > 0 { pouch_seen as f64 / aim_ticks as f64 } else { 0.0 });
    // The most haptic pulses felt in any one second.
    let ticks: Vec<u64> = g.log.iter().filter_map(|e| if let Event::Pulse { tick, .. } = *e { Some(tick) } else { None }).collect();
    let peak =
        ticks.iter().enumerate().map(|(i, &t0)| ticks[i..].iter().take_while(|&&t| t < t0 + TICK_RATE as u64).count()).max().unwrap_or(0);
    m.insert("pulses_per_s_peak", peak as f64);
    m.insert("peak_pen_mm", peak_pen as f64 * 1000.0);
    m.insert("peak_awake", peak_awake as f64);
    m.insert("step_us_mean", if steps > 0 { step_sum / steps as f64 } else { 0.0 });
    // The 95th percentile, not the worst: single slow steps come and go with the OS scheduler (EVALS step 007).
    step_times.sort_by(f64::total_cmp);
    m.insert("step_us_p95", step_times.get(step_times.len() * 95 / 100).copied().unwrap_or(0.0));
    m
}

/// One run of every shot: per shot, and mean / min / max per metric.
pub struct Report {
    pub shots: Vec<(&'static str, BTreeMap<&'static str, f64>)>,
}

impl Report {
    pub fn run(t: &Tuning) -> Report {
        let layout = Layout::new(SCREEN.0, SCREEN.1, 3.0);
        let mut shots: Vec<_> = scripts(t).iter().map(|s| (s.name, run(t, s, &layout, None))).collect();
        // How finely a finger aims: one row of its own (its metrics appear only there).
        shots.push(("precision", super::precision::measure(t, &layout)));
        Report { shots }
    }

    pub fn mean(&self, k: &str) -> f64 {
        let v: Vec<f64> = self.shots.iter().filter_map(|(_, m)| m.get(k).copied()).collect();
        v.iter().sum::<f64>() / v.len().max(1) as f64
    }

    pub fn names(&self) -> Vec<&'static str> {
        let all: std::collections::BTreeSet<&'static str> = self.shots.iter().flat_map(|(_, m)| m.keys().copied()).collect();
        all.into_iter().collect()
    }

    /// Timings vary run to run; `--check` leaves them out.
    pub fn timed(k: &str) -> bool {
        k.starts_with("step_us")
    }

    pub fn json(&self, step: u32, name: &str, sets: &[String]) -> String {
        let mut metrics = serde_json::Map::new();
        for k in self.names() {
            let v: Vec<f64> = self.shots.iter().filter_map(|(_, m)| m.get(k).copied()).collect();
            let mut o = serde_json::Map::new();
            o.insert("mean".into(), self.mean(k).into());
            o.insert("min".into(), v.iter().copied().fold(f64::MAX, f64::min).into());
            o.insert("max".into(), v.iter().copied().fold(f64::MIN, f64::max).into());
            metrics.insert(k.into(), o.into());
        }
        let mut shots = serde_json::Map::new();
        for (s, m) in &self.shots {
            shots.insert((*s).into(), serde_json::to_value(m).unwrap_or_default());
        }
        let doc = serde_json::json!({ "step": step, "name": name, "set": sets, "metrics": metrics, "shots": shots });
        serde_json::to_string_pretty(&doc).unwrap_or_default()
    }
}

/// The goals in `eval.toml`: metric → max | min | info.
pub fn goals(dir: &Path) -> Result<BTreeMap<String, String>, String> {
    let src = std::fs::read_to_string(dir.join("eval.toml")).map_err(|e| format!("eval.toml: {e}"))?;
    let v: toml::Value = toml::from_str(&src).map_err(|e| format!("eval.toml: {e}"))?;
    let mut out = BTreeMap::new();
    if let Some(m) = v.get("metrics").and_then(|m| m.as_table()) {
        for (k, d) in m {
            out.insert(k.clone(), d.get("goal").and_then(|g| g.as_str()).unwrap_or("info").to_string());
        }
    }
    Ok(out)
}

/// The saved steps, oldest first: (step, path).
pub fn saved(dir: &Path) -> Vec<(u32, std::path::PathBuf)> {
    let mut v: Vec<(u32, std::path::PathBuf)> = std::fs::read_dir(dir.join("evals"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let n = p.file_name()?.to_str()?.get(..3)?.parse().ok()?;
            (p.extension()? == "json").then_some((n, p))
        })
        .collect();
    v.sort();
    v
}

/// A saved step: its name, metric means and the `--set` probes it ran with.
pub type Saved = (String, BTreeMap<String, f64>, Vec<String>);

/// A saved step's means.
pub fn load_means(path: &Path) -> Result<Saved, String> {
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let name = v["name"].as_str().unwrap_or("").to_string();
    let sets = v["set"].as_array().map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect()).unwrap_or_default();
    let mut m = BTreeMap::new();
    if let Some(o) = v["metrics"].as_object() {
        for (k, x) in o {
            if let Some(f) = x["mean"].as_f64() {
                m.insert(k.clone(), f);
            }
        }
    }
    Ok((name, m, sets))
}

/// "better", "worse" or "=" for a move from `old` to `new` under `goal`.
pub fn verdict(goal: &str, old: f64, new: f64) -> &'static str {
    let close = (new - old).abs() <= 1e-6_f64.max(old.abs() * 0.005);
    match goal {
        _ if close => "=",
        "max" if new > old => "better",
        "min" if new < old => "better",
        "max" | "min" => "worse",
        _ => "",
    }
}

/// The table printed after a run: per metric the mean now, the last step's, the verdict.
pub fn table(r: &Report, goals: &BTreeMap<String, String>, last: Option<&BTreeMap<String, f64>>) -> String {
    let mut s = format!("{:<16} {:>6} {:>10} {:>10}  {}\n", "metric", "goal", "now", "last", "");
    for k in r.names() {
        let g = goals.get(k).map_or("info", String::as_str);
        let now = r.mean(k);
        let (old, v) = match last.and_then(|l| l.get(k)) {
            // Timings move with whatever else the machine is doing: only a large move is a verdict.
            Some(&o) if Report::timed(k) && (now - o).abs() < o.abs() * 0.3 => (format!("{o:>10.3}"), "~"),
            Some(&o) => (format!("{o:>10.3}"), verdict(g, o, now)),
            None => (format!("{:>10}", "-"), ""),
        };
        s += &format!("{k:<16} {g:>6} {now:>10.3} {old}  {v}\n");
    }
    s += "\nper shot:";
    for k in ["cleared_pct", "aim_error_m", "first_hit_s", "settle_s", "tower_in_view", "stone_in_view"] {
        s += &format!("\n  {k:<14}");
        for (name, m) in &r.shots {
            s += &format!(" {name}={:.2}", m.get(k).copied().unwrap_or(f64::NAN));
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::smash::camera::Pose;

    #[test]
    fn verdicts_follow_goals() {
        assert_eq!(verdict("max", 1.0, 2.0), "better");
        assert_eq!(verdict("min", 1.0, 2.0), "worse");
        assert_eq!(verdict("min", 1.0, 1.001), "=");
        assert_eq!(verdict("info", 1.0, 5.0), "");
    }

    #[test]
    fn the_aim_pose_is_where_the_camera_starts() {
        let t = Tuning::embedded();
        let g = Smash::new(t.clone());
        let p: Pose = Smash::aim_pose(&t, Default::default());
        assert_eq!(g.rig.pose, p);
    }
}
