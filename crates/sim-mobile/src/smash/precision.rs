//! How precisely a finger can aim (EVALS step 008 on): the slingshot driven through the real input path on a fresh
//! tower, reading where the preview says the stone will first touch. No shot is fired except to measure the release.
//!
//! - `aim_v_cm_px` / `aim_h_cm_px`: how far the hit point moves on the tower per pixel of finger travel (down,
//!   sideways): smaller is finer, as long as the whole tower stays reachable.
//! - `aim_linearity`: the largest over the smallest vertical step along the pull (1 = the hit climbs evenly).
//! - `reach_pct`: the share of the tower's rows (centre column) and columns (at mid pull) a finger can hit.
//! - `release_shift_cm`: how far the shot moves when the finger rolls 12 px as it lifts (every real release does).
//! - `swim_px`: how far the aim mark drifts on screen after the finger stops (the camera catching up).
//! - `parallax_px`: how far the far palms shift against the tower when aiming fully sideways (the depth you see).

use std::collections::BTreeMap;

use sim_physics::rigid::V3;

use super::tuning::Tuning;
use super::{Event, Smash};
use crate::gesture::Px;
use crate::playground::{Card, Layout};

const START: (f32, f32) = (0.5, 0.55);

struct Probe {
    g: Smash,
    w: f32,
    h: f32,
}

impl Probe {
    fn new(t: &Tuning, layout: &Layout) -> Probe {
        let mut g = Smash::at_level(t.clone(), super::eval::EVAL_LEVEL);
        g.layout(layout);
        Probe { g, w: layout.screen.0, h: layout.screen.1 }
    }

    fn px(&self, dx: f32, dy: f32) -> Px {
        Px::new(START.0 * self.w + dx, START.1 * self.h + dy)
    }

    /// Down at the start, glide to (`dx`, `dy`) pixels from it over `secs`, then hold for `hold` seconds.
    fn aim(&mut self, dx: f32, dy: f32, secs: f32, hold: f32) {
        let mut out = Vec::new();
        self.g.build();
        let start = self.px(0.0, 0.0);
        self.g.press(start, &mut out);
        let n = (secs * 60.0).max(1.0) as u32;
        for i in 1..=n {
            let k = i as f32 / n as f32;
            let at = self.px(dx * k, dy * k);
            self.g.pull(at, &mut out);
            self.g.tick(&mut out);
        }
        for _ in 0..(hold * 60.0) as u32 {
            self.g.tick(&mut out);
        }
    }

    /// Where the preview says the stone first touches now.
    fn hit(&self) -> Option<V3> {
        self.g.preview.hit.map(|(c, n)| c - n * self.g.t.stone.radius)
    }

    /// Where the stone's centre is when it first touches: this moves smoothly with the pull (the contact point
    /// jumps at every seam between two pieces).
    fn centre(&self) -> Option<V3> {
        self.g.preview.hit.map(|(c, _)| c)
    }

    fn screen(&self, p: V3) -> (f32, f32) {
        let (x, y, _) = self.g.rig.camera(1.0).project(p, self.w, self.h);
        (x, y)
    }
}

pub fn measure(t: &Tuning, layout: &Layout) -> BTreeMap<&'static str, f64> {
    let mut p = Probe::new(t, layout);
    let full = t.sling.pull_screen * p.h;
    let mut m = BTreeMap::new();
    // Vertical: the hit's height along the pull, 10 px steps, on the tower's front.
    let mut steps = Vec::new();
    let mut heights = Vec::new();
    let mut k = 0.15;
    while k <= 1.0 {
        p.aim(0.0, full * k, 0.25, 0.1);
        let a = p.centre();
        p.aim(0.0, full * k + 10.0, 0.25, 0.1);
        let b = p.centre();
        if let (Some(a), Some(b)) = (a, b)
            && a.z > -0.5
            && b.z > -0.5
        {
            steps.push(((b.y - a.y).abs() * 100.0 / 10.0) as f64);
            heights.push(a.y);
        }
        k += 0.1;
    }
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len().max(1) as f64;
    m.insert("aim_v_cm_px", mean(&steps));
    let (lo, hi) = steps.iter().fold((f64::MAX, 0.0f64), |(l, h), &s| (l.min(s.max(1e-3)), h.max(s)));
    m.insert("aim_linearity", if steps.len() > 1 { hi / lo } else { 99.0 });
    // Horizontal, at mid pull.
    p.aim(-10.0, full * 0.6, 0.25, 0.1);
    let a = p.hit();
    p.aim(10.0, full * 0.6, 0.25, 0.1);
    let b = p.hit();
    m.insert(
        "aim_h_cm_px",
        match (a, b) {
            (Some(a), Some(b)) => ((b.x - a.x).abs() * 100.0 / 20.0) as f64,
            _ => 99.0,
        },
    );
    // Reach: rows on the centre column, columns at mid pull.
    let tw = p.g.level().clone();
    let cell = t.pieces.cell;
    let rows: Vec<f32> = (0..tw.rows.len()).map(|r| t.pedestal.top + cell.1 * (r as f32 + 0.5)).collect();
    let (ylo, yhi) = heights.iter().fold((f32::MAX, f32::MIN), |(l, h), &y| (l.min(y), h.max(y)));
    let rows_hit = rows.iter().filter(|&&y| y >= ylo - cell.1 * 0.5 && y <= yhi + cell.1 * 0.5).count();
    let width = tw.rows.iter().map(|r| r.chars().count()).max().unwrap_or(1);
    let mut xs = Vec::new();
    for s in [-0.45f32, 0.45] {
        p.aim(s * p.w, full * 0.6, 0.25, 0.1);
        if let Some(h) = p.hit() {
            xs.push(h.x);
        }
    }
    let half = width as f32 * cell.0 / 2.0;
    let cols_hit = (0..width)
        .filter(|&c| {
            let x = (c as f32 + 0.5) * cell.0 - half;
            xs.iter().any(|&e| e <= x) && xs.iter().any(|&e| e >= x)
        })
        .count();
    m.insert("reach_pct", 50.0 * (rows_hit as f64 / rows.len().max(1) as f64 + cols_hit as f64 / width as f64));
    // Release: aim, then the finger rolls 12 px as it lifts.
    p.aim(30.0, full * 0.6, 0.3, 0.2);
    let before = p.hit();
    let mut out = Vec::new();
    p.g.pull(p.px(30.0 - 8.5, full * 0.6 + 8.5), &mut out);
    p.g.release(&mut out);
    // The fired stone is in the world now: take it out, or the check would fly into it.
    if let Some(s) = p.g.stones.pop() {
        p.g.world.remove(s.id);
    }
    // Where the stone actually goes: its real launch flown through the same preview.
    let fired = p.g.log.iter().find_map(|e| if let Event::Shot { from, vel, .. } = *e { Some((from, vel)) } else { None }).and_then(
        |(from, vel)| {
            let t = &p.g.t;
            super::sling::preview(&p.g.world, from, vel, t.stone.radius, t.physics.gravity, 60.0, t.physics.substeps, 1.4)
                .hit
                .map(|(c, n)| c - n * t.stone.radius)
        },
    );
    m.insert(
        "release_shift_cm",
        match (before, fired) {
            (Some(a), Some(b)) => ((a - b).length() * 100.0) as f64,
            _ => 99.0,
        },
    );
    // Swim: aim sideways, stop, and watch the mark for 0.6 s.
    p.aim(0.1 * p.w, full * 0.6, 0.3, 0.0);
    let mut swim = 0.0f64;
    if let Some(mark) = p.hit() {
        let (x0, y0) = p.screen(mark);
        let mut out = Vec::new();
        for _ in 0..36 {
            p.g.tick(&mut out);
            let (x, y) = p.screen(mark);
            swim = swim.max(((x - x0).powi(2) + (y - y0).powi(2)).sqrt() as f64);
        }
    }
    m.insert("swim_px", swim);
    // Parallax: a far palm against the tower, aiming fully sideways versus straight.
    let palm = V3::new(-11.0, 7.0, -20.0);
    let tower = V3::new(0.0, t.pedestal.top + 1.0, 0.0);
    p.aim(0.0, full * 0.6, 0.3, 1.0);
    let rel0 = p.screen(palm).0 - p.screen(tower).0;
    p.aim(0.3 * p.w, full * 0.6, 0.3, 1.0);
    let rel1 = p.screen(palm).0 - p.screen(tower).0;
    m.insert("parallax_px", (rel1 - rel0).abs() as f64);
    m
}
