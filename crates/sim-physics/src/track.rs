//! A racing surface as data: straights and constant-radius turns, each with its banking, laid end to end the way
//! TORCS and Speed Dreams describe tracks (`docs/research/driving-physics.md` §5). The world stays an ordinary
//! plane; the track answers where a point is along it (`s`), how far off the centreline (`offset`), and what the
//! surface does there (banking, height). Arcs are exact circles, so these answers need no spline fitting.
//!
//! Conventions: x and y in metres on the plane, headings counterclockwise from +x, `offset` positive to the left of
//! the direction of travel, `bank` positive when the road slopes down toward the left (an oval run
//! counterclockwise is banked toward its infield: positive).

use crate::fixed::{Angle, Fx, TURN, TWO_PI};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Shape {
    /// Length, mm.
    Straight(i64),
    /// Radius (mm) and angle turned (hundredths of a degree).
    Left(i64, i64),
    Right(i64, i64),
    /// A turn eased in and out by transition spirals (clothoids, the curve road and track design use): the
    /// curvature grows steadily from straight to `radius` over `spiral` mm, holds, and falls back over `spiral` mm;
    /// banking eases in and out along the spirals. Radius (mm), angle turned in all (hundredths of a degree),
    /// spiral length (mm).
    EasedLeft(i64, i64, i64),
    EasedRight(i64, i64, i64),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SegmentDef {
    pub shape: Shape,
    /// Cross-slope, hundredths of a degree (positive: the left side lower).
    #[serde(default)]
    pub bank: i64,
    /// What people call it ("Turn 1", "Backstretch"), for timing screens and errors.
    #[serde(default)]
    pub name: String,
}

/// A track as data. Lengths in mm, angles in hundredths of a degree.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackDef {
    pub name: String,
    /// Racing surface, edge to edge.
    pub width: i64,
    /// Where the first segment starts: x, y (mm) and heading. The start/finish line is here.
    #[serde(default)]
    pub start: (i64, i64, i64),
    /// Banking eases from one segment's to the next over this length, centred on the join (a real track's
    /// transition). 0: it steps.
    #[serde(default)]
    pub blend: i64,
    /// What lies beyond each edge of the racing surface (left and right of the direction of travel).
    #[serde(default)]
    pub left: SideDef,
    #[serde(default)]
    pub right: SideDef,
    pub segments: Vec<SegmentDef>,
}

/// Beyond one edge of the racing surface: a run-off `width` mm wide with its own grip (an apron, grass, gravel),
/// then, if `wall`, a wall (a SAFER barrier). No wall: the run-off goes on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SideDef {
    #[serde(default)]
    pub width: i64,
    /// × 1000 of the racing surface's grip.
    #[serde(default = "full")]
    pub grip: i64,
    #[serde(default)]
    pub wall: bool,
}

fn full() -> i64 {
    1000
}

impl Default for SideDef {
    fn default() -> SideDef {
        SideDef { width: 0, grip: 1000, wall: false }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Straight,
    /// A clothoid-eased turn: `sign` 1 left, -1 right; its points are sampled in the segment's `pts`.
    Eased {
        radius: Fx,
        sign: i64,
        spiral: Fx,
    },
    /// `sign` 1 turns left, -1 right; `a0` points from the centre to the segment's start.
    Arc {
        radius: Fx,
        sign: i64,
        cx: Fx,
        cy: Fx,
        a0: Angle,
        turn: Angle,
    },
}

#[derive(Clone, Debug, PartialEq)]
struct Seg {
    s0: Fx,
    len: Fx,
    x0: Fx,
    y0: Fx,
    h0: Angle,
    kind: Kind,
    bank: Angle,
    /// An eased turn's centreline, every `STEP` metres from its start (empty for other shapes).
    pts: Vec<(Fx, Fx)>,
}

/// Sampling step along an eased turn (m): the chord between samples strays 0.2 mm from a 190 m curve.
const STEP: Fx = Fx::HALF;

/// The heading an eased turn has turned through `t` metres in (radians, before its sign).
fn eased_turned(radius: Fx, spiral: Fx, len: Fx, t: Fx) -> Fx {
    let t = t.clamp(Fx::ZERO, len);
    if spiral == Fx::ZERO {
        return t / radius;
    }
    let in_spiral = |u: Fx| u * u / (spiral * radius * 2);
    if t < spiral {
        in_spiral(t)
    } else if t <= len - spiral {
        in_spiral(spiral) + (t - spiral) / radius
    } else {
        // All it turns, (len − spiral)/radius, less what the last spiral has still to turn.
        (len - spiral) / radius - in_spiral(len - t)
    }
}

/// The curvature of an eased turn `t` metres in (1/m, before its sign).
fn eased_curvature(radius: Fx, spiral: Fx, len: Fx, t: Fx) -> Fx {
    if spiral == Fx::ZERO {
        return Fx::ONE / radius;
    }
    let ramp = |u: Fx| (u / spiral).clamp(Fx::ZERO, Fx::ONE) / radius;
    ramp(t).min(ramp(len - t))
}

/// Where a point is on the track.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Place {
    /// Distance along the centreline from the start line, `0..length`.
    pub s: Fx,
    /// Signed distance from the centreline, positive to the left.
    pub offset: Fx,
    /// Which segment (a hint for the next question about the same car).
    pub seg: usize,
}

/// The track surface at a place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pose {
    pub x: Fx,
    pub y: Fx,
    /// Height above the centreline's plane, from the banking.
    pub z: Fx,
    /// Direction of travel along the centreline here.
    pub heading: Angle,
    pub bank: Angle,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub name: String,
    pub width: Fx,
    pub length: Fx,
    /// Offsets of the walls (left positive, right negative), if there are walls.
    pub wall_left: Option<Fx>,
    pub wall_right: Option<Fx>,
    left: SideDef,
    right: SideDef,
    blend: Fx,
    segs: Vec<Seg>,
    names: Vec<String>,
}

/// Arc length of `a` on a circle of radius `r`, at full precision (no rounding through radians).
fn arc(r: Fx, a: Angle) -> Fx {
    Fx(((r.0 as i128 * a.0 as i128 * TWO_PI.0 as i128) >> (32 + 16)) as i64)
}

/// The angle an arc of length `t` spans on radius `r`.
fn arc_angle(t: Fx, r: Fx) -> Angle {
    Angle(((t.0 as i128 * TURN as i128 * 65536) / (r.0 as i128 * TWO_PI.0 as i128)) as i64)
}

impl Track {
    /// Lay the segments end to end. A track must close on itself: everything wrong is listed at once.
    pub fn new(d: &TrackDef) -> Result<Track, Vec<String>> {
        let mut problems = Vec::new();
        if d.width <= 0 {
            problems.push(format!("width must be above 0 (is {})", d.width));
        }
        if d.segments.is_empty() {
            problems.push("a track needs segments".into());
        }
        let mm = |v: i64| Fx::ratio(v, 1000);
        let (mut x, mut y, mut h) = (mm(d.start.0), mm(d.start.1), Angle::centidegrees(d.start.2));
        let mut s = Fx::ZERO;
        let mut segs = Vec::new();
        for (i, sd) in d.segments.iter().enumerate() {
            let label = if sd.name.is_empty() { format!("segment {i}") } else { format!("segment {i} ({})", sd.name) };
            let bank = Angle::centidegrees(sd.bank);
            if sd.bank.abs() >= 4500 {
                problems.push(format!("{label}: bank {}° is steeper than any track (under 45°)", sd.bank / 100));
            }
            let (len, kind) = match sd.shape {
                Shape::Straight(l) => {
                    if l <= 0 {
                        problems.push(format!("{label}: a straight needs a length above 0 (is {l})"));
                    }
                    (mm(l), Kind::Straight)
                }
                Shape::Left(r, a) | Shape::Right(r, a) => {
                    if r <= 0 || a <= 0 || a > 36000 {
                        problems.push(format!("{label}: a turn needs a radius above 0 and 0..360° (is {r} mm, {a})"));
                    }
                    let sign = if matches!(sd.shape, Shape::Left(..)) { 1 } else { -1 };
                    let radius = mm(r.max(1));
                    let turn = Angle::centidegrees(a);
                    // The centre is `radius` to the left (or right) of the start.
                    let (cx, cy) = (x - radius * h.sin() * sign, y + radius * h.cos() * sign);
                    let a0 = h - Angle(sign * TURN / 4);
                    (arc(radius, turn), Kind::Arc { radius, sign, cx, cy, a0, turn })
                }
                Shape::EasedLeft(r, a, l) | Shape::EasedRight(r, a, l) => {
                    let sign = if matches!(sd.shape, Shape::EasedLeft(..)) { 1 } else { -1 };
                    let (radius, turn, spiral) = (mm(r.max(1)), Angle::centidegrees(a), mm(l.max(0)));
                    // Two spirals turn spiral/radius together; the arc between them turns the rest.
                    let total = turn.to_radians();
                    let arc_part = total * radius - spiral;
                    if r <= 0 || a <= 0 || a > 36000 || l < 0 || arc_part < Fx::ZERO {
                        problems.push(format!(
                            "{label}: an eased turn needs a radius above 0, 0..360°, and spirals short enough to fit                              the angle (spiral ≤ angle × radius: {} mm here; is {r} mm, {a}, {l} mm)",
                            (total * radius).to_units(1000)
                        ));
                    }
                    (spiral * 2 + arc_part.max(Fx::ZERO), Kind::Eased { radius, sign, spiral })
                }
            };
            let pts = match kind {
                Kind::Eased { radius, sign, spiral } => {
                    // Walk the curve: exact heading, midpoint rule for the position.
                    let n = (len / STEP).floor().max(1) as usize;
                    let mut pts = Vec::with_capacity(n + 2);
                    let (mut px, mut py) = (x, y);
                    pts.push((px, py));
                    let mut t = Fx::ZERO;
                    while t < len {
                        let dt = STEP.min(len - t);
                        let mid = t + dt / 2;
                        let hd = h + Angle(Angle::radians(eased_turned(radius, spiral, len, mid)).0 * sign);
                        px += dt * hd.cos();
                        py += dt * hd.sin();
                        pts.push((px, py));
                        t += dt;
                    }
                    pts
                }
                _ => Vec::new(),
            };
            segs.push(Seg { s0: s, len, x0: x, y0: y, h0: h, kind, bank, pts });
            let end = at(&segs[segs.len() - 1], len);
            (x, y, h) = (end.0, end.1, end.2);
            s += len;
        }
        let (sx, sy, sh) = (mm(d.start.0), mm(d.start.1), Angle::centidegrees(d.start.2));
        let gap = ((x - sx) * (x - sx) + (y - sy) * (y - sy)).sqrt();
        let turned = (h - sh).signed();
        if !segs.is_empty() && (gap > Fx::HALF || turned.0.abs() > Angle::centidegrees(50).0) {
            problems.push(format!(
                "the track does not close: its end is {:.2} m from the start, heading off by {:.2}°",
                gap.0 as f64 / 65536.0,
                turned.0 as f64 * 360.0 / TURN as f64
            ));
        }
        if !problems.is_empty() {
            return Err(problems);
        }
        let half = mm(d.width) / 2;
        Ok(Track {
            name: d.name.clone(),
            width: mm(d.width),
            length: s,
            wall_left: d.left.wall.then(|| half + mm(d.left.width)),
            wall_right: d.right.wall.then(|| -(half + mm(d.right.width))),
            left: d.left,
            right: d.right,
            blend: mm(d.blend),
            segs,
            names: d.segments.iter().map(|s| s.name.clone()).collect(),
        })
    }

    pub fn segments(&self) -> usize {
        self.segs.len()
    }

    pub fn segment_name(&self, i: usize) -> &str {
        &self.names[i]
    }

    /// Distance `s` brought into `0..length` (laps wrap).
    pub fn wrap(&self, s: Fx) -> Fx {
        Fx(s.0.rem_euclid(self.length.0))
    }

    fn seg_at(&self, s: Fx) -> usize {
        self.segs.partition_point(|g| g.s0 <= s).saturating_sub(1)
    }

    /// The banking at `s`, eased across each join over `blend`.
    pub fn bank(&self, s: Fx) -> Angle {
        let s = self.wrap(s);
        let i = self.seg_at(s);
        let g = &self.segs[i];
        let n = self.segs.len();
        // An eased turn banks along its spirals (from the segment before, to the one after).
        if let Kind::Eased { spiral, .. } = g.kind
            && spiral > Fx::ZERO
        {
            let t = s - g.s0;
            let lerp = |a: Angle, b: Angle, f: Fx| a + Angle((((b - a).0 as i128 * f.0 as i128) >> 16) as i64);
            if t < spiral {
                return lerp(self.segs[(i + n - 1) % n].bank, g.bank, t / spiral);
            }
            if t > g.len - spiral {
                return lerp(g.bank, self.segs[(i + 1) % n].bank, (t - (g.len - spiral)) / spiral);
            }
            return g.bank;
        }
        let half = self.blend / 2;
        let (from, to, into) = if s - g.s0 < half {
            (self.segs[(i + n - 1) % n].bank, g.bank, s - g.s0 + half)
        } else if g.s0 + g.len - s < half {
            (g.bank, self.segs[(i + 1) % n].bank, s - (g.s0 + g.len) + half)
        } else {
            return g.bank;
        };
        from + Angle(((to - from).0 as i128 * into.0 as i128 / self.blend.0.max(1) as i128) as i64)
    }

    /// The grip at `offset` from the centreline, as a share of the racing surface's: 1 on it, the run-off's beyond.
    pub fn grip(&self, offset: Fx) -> Fx {
        let half = self.width / 2;
        if offset > half {
            Fx::ratio(self.left.grip, 1000)
        } else if offset < -half {
            Fx::ratio(self.right.grip, 1000)
        } else {
            Fx::ONE
        }
    }

    /// How sharply the centreline turns at `s`: 1 / radius, positive turning left, 0 on a straight.
    pub fn curvature(&self, s: Fx) -> Fx {
        match self.segs[self.seg_at(self.wrap(s))].kind {
            Kind::Straight => Fx::ZERO,
            Kind::Arc { radius, sign, .. } => Fx::ONE / radius * sign,
            Kind::Eased { radius, sign, spiral } => {
                let g = &self.segs[self.seg_at(self.wrap(s))];
                eased_curvature(radius, spiral, g.len, self.wrap(s) - g.s0) * sign
            }
        }
    }

    /// The surface at `s` along the centreline, `offset` to its left.
    pub fn pose(&self, s: Fx, offset: Fx) -> Pose {
        let s = self.wrap(s);
        let g = &self.segs[self.seg_at(s)];
        let (x, y, heading) = at(g, s - g.s0);
        let bank = self.bank(s);
        // Left of the direction of travel is (-sin, cos).
        Pose { x: x - offset * heading.sin(), y: y + offset * heading.cos(), z: -offset * bank.sin() / bank.cos(), heading, bank }
    }

    /// Where the point (x, y) is: the nearest segment's `s` and `offset`. `hint`, the segment of the last answer
    /// for the same car, is checked first (a car moves little in a tick).
    pub fn locate(&self, x: Fx, y: Fx, hint: Option<usize>) -> Place {
        let n = self.segs.len();
        if let Some(h) = hint.filter(|&h| h < n) {
            for i in [h, (h + 1) % n, (h + n - 1) % n] {
                if let Some(p) = self.project(i, x, y)
                    && p.offset.abs() <= self.width
                {
                    return p;
                }
            }
        }
        let mut best: Option<Place> = None;
        for i in 0..n {
            if let Some(p) = self.project(i, x, y)
                && best.is_none_or(|b| p.offset.abs() < b.offset.abs())
            {
                best = Some(p);
            }
        }
        // Outside every segment's reach (a corner of the infield between two joins): the nearest join.
        best.unwrap_or_else(|| {
            let (i, _) = (0..n)
                .map(|i| {
                    let g = &self.segs[i];
                    (i, (g.x0 - x) * (g.x0 - x) + (g.y0 - y) * (g.y0 - y))
                })
                .min_by_key(|&(_, d)| d)
                .unwrap_or((0, Fx::ZERO));
            let g = &self.segs[i];
            let side = (x - g.x0) * -g.h0.sin() + (y - g.y0) * g.h0.cos();
            Place { s: g.s0, offset: side, seg: i }
        })
    }

    /// The point's projection onto segment `i`, if it falls within the segment's length.
    fn project(&self, i: usize, x: Fx, y: Fx) -> Option<Place> {
        let g = &self.segs[i];
        let (t, offset) = match g.kind {
            Kind::Straight => {
                let (dx, dy) = (x - g.x0, y - g.y0);
                let (c, s) = (g.h0.cos(), g.h0.sin());
                (dx * c + dy * s, dy * c - dx * s)
            }
            Kind::Arc { radius, sign, cx, cy, a0, turn } => {
                let (dx, dy) = (x - cx, y - cy);
                // Measured from the arc's middle, so a point a hair before its start (rounding, on a join) is a
                // small negative angle, not almost a full turn.
                let half = Angle(turn.0 / 2);
                let from_mid = (Angle((Angle::atan2(dy, dx) - a0).0 * sign) - half).signed();
                if from_mid.0.abs() > half.0 + JOIN.0 {
                    return None;
                }
                let phi = Angle((from_mid + half).0.clamp(0, turn.0));
                let r = (dx * dx + dy * dy).sqrt();
                (arc(radius, phi), (radius - r) * sign)
            }
            Kind::Eased { .. } => {
                // The nearest sample (coarse every 8 m, then fine), then the chord to its better neighbour.
                let d2 = |k: usize| {
                    let (px, py) = g.pts[k];
                    (px - x) * (px - x) + (py - y) * (py - y)
                };
                let last = g.pts.len() - 1;
                let coarse = (0..=last).step_by(16).chain([last]).min_by_key(|&k| d2(k)).unwrap_or(0);
                let k = (coarse.saturating_sub(16)..=(coarse + 16).min(last)).min_by_key(|&k| d2(k)).unwrap_or(0);
                let (a, b) = if k == last || (k > 0 && d2(k - 1) < d2(k + 1)) { (k - 1, k) } else { (k, k + 1) };
                let ((ax, ay), (bx, by)) = (g.pts[a], g.pts[b]);
                let (cx, cy) = (bx - ax, by - ay);
                let len2 = (cx * cx + cy * cy).max(Fx(1));
                let f = ((x - ax) * cx + (y - ay) * cy) / len2;
                let t = STEP * a as i64 + STEP * f;
                // Left of the chord is positive.
                let chord = len2.sqrt();
                (t, ((x - ax) * cy * -Fx::ONE + (y - ay) * cx) / chord)
            }
        };
        let tol = Fx::ratio(1, 1000);
        (t >= -tol && t <= g.len + tol).then(|| Place { s: self.wrap(g.s0 + t.clamp(Fx::ZERO, g.len)), offset, seg: i })
    }
}

/// How far past its ends a segment still answers for a point (rounding on a join): a millionth of a turn.
const JOIN: Angle = Angle(TURN / 1_000_000);

/// The centreline point `t` into segment `g`: x, y, heading.
fn at(g: &Seg, t: Fx) -> (Fx, Fx, Angle) {
    match g.kind {
        Kind::Straight => (g.x0 + t * g.h0.cos(), g.y0 + t * g.h0.sin(), g.h0),
        Kind::Arc { radius, sign, cx, cy, a0, .. } => {
            let phi = arc_angle(t, radius);
            let a = a0 + Angle(phi.0 * sign);
            (cx + radius * a.cos(), cy + radius * a.sin(), g.h0 + Angle(phi.0 * sign))
        }
        Kind::Eased { radius, sign, spiral } => {
            let t = t.clamp(Fx::ZERO, g.len);
            let k = ((t / STEP).floor().max(0) as usize).min(g.pts.len() - 2);
            let f = (t - STEP * k as i64) / STEP;
            let ((ax, ay), (bx, by)) = (g.pts[k], g.pts[k + 1]);
            let heading = g.h0 + Angle(Angle::radians(eased_turned(radius, spiral, g.len, t)).0 * sign);
            (ax + (bx - ax) * f, ay + (by - ay) * f, heading)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(x: Fx) -> f64 {
        x.0 as f64 / 65536.0
    }

    /// A 1 km oval: two 250 m straights, two 180° turns of radius 250/π·… chosen to close exactly.
    fn oval() -> TrackDef {
        TrackDef {
            name: "test oval".into(),
            width: 15000,
            start: (0, 0, 0),
            blend: 40000,
            left: SideDef::default(),
            right: SideDef { width: 0, grip: 1000, wall: true },
            segments: vec![
                SegmentDef { shape: Shape::Straight(250_000), bank: 500, name: "Front".into() },
                SegmentDef { shape: Shape::Left(100_000, 18000), bank: 2400, name: "Turns 1-2".into() },
                SegmentDef { shape: Shape::Straight(250_000), bank: 500, name: "Back".into() },
                SegmentDef { shape: Shape::Left(100_000, 18000), bank: 2400, name: "Turns 3-4".into() },
            ],
        }
    }

    #[test]
    fn an_oval_closes_and_measures_its_length() {
        let t = Track::new(&oval()).expect("closes");
        let expected = 500.0 + 2.0 * std::f64::consts::PI * 100.0;
        assert!((f(t.length) - expected).abs() < 0.01, "{} m vs {expected}", f(t.length));
        let far = t.pose(Fx::int(250) + arc(Fx::int(100), Angle::QUARTER), Fx::ZERO);
        assert!((f(far.x) - 350.0).abs() < 0.01 && (f(far.y) - 100.0).abs() < 0.01, "{far:?}");
    }

    #[test]
    fn a_track_that_does_not_close_says_how_far_off_it_is() {
        let mut d = oval();
        d.segments[2].shape = Shape::Straight(240_000);
        d.segments[1].bank = 5000;
        let e = Track::new(&d).unwrap_err();
        assert_eq!(e.len(), 2, "{e:#?}");
        assert!(e[1].contains("10.00 m"), "{e:#?}");
        assert!(e[0].contains("Turns 1-2"), "{e:#?}");
    }

    #[test]
    fn locate_undoes_pose_everywhere() {
        let t = Track::new(&oval()).unwrap();
        let mut hint = None;
        for i in 0..400 {
            let s = Fx(t.length.0 * i / 400 + 12345);
            for off in [-7, -2, 0, 3, 7] {
                let p = t.pose(s, Fx::int(off));
                let at = t.locate(p.x, p.y, hint);
                hint = Some(at.seg);
                let ds = f((at.s - s).abs()).min(f(t.length) - f((at.s - s).abs()));
                assert!(ds < 0.005 && (f(at.offset) - off as f64).abs() < 0.005, "s {} off {off}: {at:?}", f(s));
            }
        }
    }

    /// The same oval with its turns eased by 60 m spirals.
    fn eased_oval() -> TrackDef {
        let mut d = oval();
        d.blend = 0;
        d.segments[1].shape = Shape::EasedLeft(100_000, 18000, 60_000);
        d.segments[3].shape = Shape::EasedLeft(100_000, 18000, 60_000);
        // An eased half turn of radius R with spirals L reaches further out; straights shortened to close: the
        // generator of a real track solves this; here the two halves are symmetric, so it closes as it is.
        d
    }

    #[test]
    fn eased_turns_close_and_have_no_kinks() {
        let t = Track::new(&eased_oval()).expect("closes");
        let mut last_k = f(t.curvature(Fx::ZERO));
        let mut last_h = t.pose(Fx::ZERO, Fx::ZERO).heading;
        let mut worst = (0f64, 0f64);
        for i in 1..(f(t.length) * 4.0) as i64 {
            let s = Fx::ratio(i, 4);
            let (k, h) = (f(t.curvature(s)), t.pose(s, Fx::ZERO).heading);
            // Curvature changes by at most (1/R)/spiral per metre (no steps); heading by at most κ·ds.
            worst.0 = worst.0.max((k - last_k).abs());
            worst.1 = worst.1.max(((h - last_h).signed().0 as f64 / TURN as f64 * 360.0).abs());
            (last_k, last_h) = (k, h);
        }
        // (plus one Q16 step of rounding: curvature is 1/65536 fine)
        assert!(worst.0 <= 0.25 / 100.0 / 60.0 + 1.0 / 65536.0, "curvature jumps by {} per quarter metre", worst.0);
        assert!(worst.1 <= (0.25f64 / 100.0).to_degrees() + 0.002, "heading jumps by {}° per quarter metre", worst.1);
    }

    #[test]
    fn locate_undoes_pose_on_eased_turns() {
        let t = Track::new(&eased_oval()).unwrap();
        for i in 0..800 {
            let s = Fx(t.length.0 * i / 800 + 777);
            for off in [-7, 0, 5] {
                let p = t.pose(s, Fx::int(off));
                let at = t.locate(p.x, p.y, None);
                let ds = f((at.s - s).abs()).min(f(t.length) - f((at.s - s).abs()));
                assert!(ds < 0.02 && (f(at.offset) - off as f64).abs() < 0.02, "s {} off {off}: {at:?}", f(s));
            }
        }
    }

    #[test]
    fn eased_banking_rises_along_the_spiral() {
        let t = Track::new(&eased_oval()).unwrap();
        let deg = |a: Angle| a.0 as f64 * 360.0 / TURN as f64;
        assert!((deg(t.bank(Fx::int(250))) - 5.0).abs() < 0.01, "the straight's own bank up to the turn");
        assert!((deg(t.bank(Fx::int(280))) - 14.5).abs() < 0.01, "halfway up the spiral: halfway up the bank");
        assert!((deg(t.bank(Fx::int(320))) - 24.0).abs() < 0.01, "full bank on the arc");
    }

    #[test]
    fn banking_eases_across_joins_and_tilts_the_surface() {
        let t = Track::new(&oval()).unwrap();
        let deg = |a: Angle| a.0 as f64 * 360.0 / TURN as f64;
        assert!((deg(t.bank(Fx::int(100))) - 5.0).abs() < 0.001);
        assert!((deg(t.bank(Fx::int(250))) - 14.5).abs() < 0.001, "halfway through the blend");
        assert!((deg(t.bank(Fx::int(300))) - 24.0).abs() < 0.001);
        // The inside of a banked turn is lower: 7 m left of the centreline at 24°, 7·tan 24° ≈ 3.12 m down.
        let p = t.pose(Fx::int(400), Fx::int(7));
        assert!((f(p.z) + 7.0 * 24f64.to_radians().tan()).abs() < 0.01, "{}", f(p.z));
    }
}
