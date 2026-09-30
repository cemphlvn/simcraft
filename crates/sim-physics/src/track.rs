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
            };
            segs.push(Seg { s0: s, len, x0: x, y0: y, h0: h, kind, bank });
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
        let half = self.blend / 2;
        let n = self.segs.len();
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
