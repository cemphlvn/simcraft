//! Constraints: points joined by rods, ropes and struts, solved with Verlet integration and a fixed number of
//! relaxation passes (Jakobsen, *Advanced Character Physics*, 2001).
//!
//! A point keeps its position and its previous one; its velocity is their difference, so a correction that moves a
//! point also changes its speed, and stretch left in a pulled rope becomes speed when it is let go (a slingshot).
//! Each point has an inverse weight: a correction is split by it, so a heavy load barely moves and a pinned point
//! (inverse weight 0) never does. A link is a [`Link::Distance`] rod (pushes and pulls), a [`Link::Max`] rope
//! (slack allowed, no stretch) or a [`Link::Min`] strut (no shortening), as in Cut the Rope's source
//! (`docs/research/legendary-mobile-games.md`). Everything is [`Fx`]: the same input gives the same points on every
//! platform. Architecture: `docs/architecture.md`, Constraints.

use std::ops::{Add, Sub};

use crate::fixed::Fx;

/// A point or a direction in the plane.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct V2 {
    pub x: Fx,
    pub y: Fx,
}

impl V2 {
    pub const ZERO: V2 = V2 { x: Fx::ZERO, y: Fx::ZERO };

    pub const fn new(x: Fx, y: Fx) -> V2 {
        V2 { x, y }
    }

    /// A point in whole units.
    pub const fn int(x: i64, y: i64) -> V2 {
        V2 { x: Fx::int(x), y: Fx::int(y) }
    }

    pub fn scale(self, k: Fx) -> V2 {
        V2::new(self.x * k, self.y * k)
    }

    pub fn dot(self, o: V2) -> Fx {
        self.x * o.x + self.y * o.y
    }

    /// The z of the cross product: positive when `o` turns left of `self`.
    pub fn cross(self, o: V2) -> Fx {
        self.x * o.y - self.y * o.x
    }

    pub fn length(self) -> Fx {
        self.dot(self).sqrt()
    }
}

impl Add for V2 {
    type Output = V2;
    fn add(self, o: V2) -> V2 {
        V2::new(self.x + o.x, self.y + o.y)
    }
}

impl Sub for V2 {
    type Output = V2;
    fn sub(self, o: V2) -> V2 {
        V2::new(self.x - o.x, self.y - o.y)
    }
}

/// What a link between two points allows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Link {
    /// A rod: always its rest length (pushes and pulls).
    Distance,
    /// A rope: never longer than its rest length, free to go slack.
    Max,
    /// A strut: never shorter than its rest length.
    Min,
}

/// A point: where it is, where it was a step ago, and how easily it moves (inverse weight; 0 = pinned).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Point {
    pub pos: V2,
    pub prev: V2,
    pub inv: Fx,
}

impl Point {
    /// Its velocity: how far it moved in the last step.
    pub fn velocity(&self) -> V2 {
        self.pos - self.prev
    }
}

/// A constraint between points `a` and `b`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Constraint {
    pub a: usize,
    pub b: usize,
    pub rest: Fx,
    pub link: Link,
}

/// Points and constraints, stepped together.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct System {
    pub points: Vec<Point>,
    pub constraints: Vec<Constraint>,
    /// Added to every free point's velocity each step (units per step²).
    pub gravity: V2,
    /// The share of its velocity a point keeps each step (1 = none lost).
    pub damping: Fx,
    /// Relaxation passes per step: more makes links stiffer (and costs more).
    pub passes: u32,
}

impl Default for System {
    fn default() -> System {
        System { points: Vec::new(), constraints: Vec::new(), gravity: V2::ZERO, damping: Fx::ratio(99, 100), passes: 30 }
    }
}

impl System {
    /// Adds a point at rest; `inv` is its inverse weight (0 = pinned). Returns its index.
    pub fn point(&mut self, pos: V2, inv: Fx) -> usize {
        self.points.push(Point { pos, prev: pos, inv });
        self.points.len() - 1
    }

    /// Links `a` and `b` at their current distance. Returns the constraint's index.
    pub fn link(&mut self, a: usize, b: usize, link: Link) -> usize {
        let rest = (self.points[b].pos - self.points[a].pos).length();
        self.constraints.push(Constraint { a, b, rest, link });
        self.constraints.len() - 1
    }

    /// A rope of `segments` links from `from` to `to`, its points evenly spaced on the straight line, each of
    /// inverse weight `inv`. Returns its points in order; pin the ends yourself ([`System::pin`]).
    pub fn rope(&mut self, from: V2, to: V2, segments: usize, inv: Fx, link: Link) -> Vec<usize> {
        let n = segments.max(1) as i64;
        let step = to - from;
        let ids: Vec<usize> = (0..=n).map(|i| self.point(from + V2::new(step.x * i / n, step.y * i / n), inv)).collect();
        for w in ids.windows(2) {
            self.link(w[0], w[1], link);
        }
        ids
    }

    /// Pins point `i` where it is: nothing moves it until it is given a weight again.
    pub fn pin(&mut self, i: usize) {
        let p = &mut self.points[i];
        p.inv = Fx::ZERO;
        p.prev = p.pos;
    }

    /// Frees point `i` with inverse weight `inv`, keeping its last step's velocity.
    pub fn release(&mut self, i: usize, inv: Fx) {
        self.points[i].inv = inv;
    }

    /// Moves a pinned point (an anchor, a finger) to `at`. Its velocity becomes this move, so a point released
    /// right after keeps the finger's speed.
    pub fn drag(&mut self, i: usize, at: V2) {
        let p = &mut self.points[i];
        p.prev = p.pos;
        p.pos = at;
    }

    /// Removes constraint `c` (a cut). The others keep their order and so their indices shift down by one.
    pub fn cut(&mut self, c: usize) {
        self.constraints.remove(c);
    }

    /// One step: free points move by their velocity (damped) plus gravity, then every constraint is relaxed
    /// `passes` times, in order.
    pub fn step(&mut self) {
        for p in &mut self.points {
            if p.inv == Fx::ZERO {
                p.prev = p.pos;
                continue;
            }
            let v = p.velocity().scale(self.damping);
            p.prev = p.pos;
            p.pos = (p.pos + v) + self.gravity;
        }
        for _ in 0..self.passes {
            for c in 0..self.constraints.len() {
                self.satisfy(self.constraints[c]);
            }
        }
    }

    fn satisfy(&mut self, c: Constraint) {
        let (pa, pb) = (self.points[c.a], self.points[c.b]);
        let w = pa.inv + pb.inv;
        if w == Fx::ZERO {
            return;
        }
        let d = pb.pos - pa.pos;
        let len = d.length();
        let act = match c.link {
            Link::Distance => len != c.rest,
            Link::Max => len > c.rest,
            Link::Min => len < c.rest,
        };
        if !act || len == Fx::ZERO {
            return;
        }
        // Move each end along the link by its share of the error: `a` towards `b` when too long, away when short.
        // Multiplied before divided: one rounding instead of two.
        let (err, den) = (len - c.rest, len * w);
        let fix = V2::new(d.x * err / den, d.y * err / den);
        self.points[c.a].pos = pa.pos + fix.scale(pa.inv);
        self.points[c.b].pos = pb.pos - fix.scale(pb.inv);
    }

    /// How far constraint `c` is past its rest length (0 when it holds, or when a rope is slack).
    pub fn stretch(&self, c: usize) -> Fx {
        let k = self.constraints[c];
        let len = (self.points[k.b].pos - self.points[k.a].pos).length();
        match k.link {
            Link::Min => Fx::ZERO,
            _ => (len - k.rest).max(Fx::ZERO),
        }
    }

    /// The largest stretch of any link, as a share of its rest length: how taut the system is (for sound and
    /// haptics). 0 when everything holds.
    pub fn tension(&self) -> Fx {
        (0..self.constraints.len())
            .filter(|&c| self.constraints[c].rest > Fx::ZERO)
            .map(|c| self.stretch(c) / self.constraints[c].rest)
            .max()
            .unwrap_or(Fx::ZERO)
    }

    /// The length of a chain of points (a rope from [`System::rope`]), measured along it.
    pub fn length(&self, chain: &[usize]) -> Fx {
        chain.windows(2).fold(Fx::ZERO, |sum, w| sum + (self.points[w[1]].pos - self.points[w[0]].pos).length())
    }

    /// The first constraint a stroke from `from` to `to` crosses, and where along the stroke (0 at `from`, 1 at
    /// `to`): how a finger swiped across a rope catches or cuts it.
    pub fn crossed(&self, from: V2, to: V2) -> Option<(usize, Fx)> {
        (0..self.constraints.len())
            .filter_map(|c| {
                let k = self.constraints[c];
                crossing(from, to, self.points[k.a].pos, self.points[k.b].pos).map(|t| (c, t))
            })
            .min_by_key(|&(c, t)| (t, c))
    }
}

/// Where segment `a0`–`a1` crosses segment `b0`–`b1`, as a share of the way along `a` (0 to 1); `None` when they
/// do not cross or are parallel.
pub fn crossing(a0: V2, a1: V2, b0: V2, b1: V2) -> Option<Fx> {
    let (r, s) = (a1 - a0, b1 - b0);
    let den = r.cross(s);
    if den == Fx::ZERO {
        return None;
    }
    let q = b0 - a0;
    let t = q.cross(s) / den;
    let u = q.cross(r) / den;
    let within = |x: Fx| x >= Fx::ZERO && x <= Fx::ONE;
    (within(t) && within(u)).then_some(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A rope of 10 links hanging from a pin at the origin, falling (y down is negative).
    fn hanging(link: Link) -> (System, Vec<usize>) {
        let mut s = System { gravity: V2::new(Fx::ZERO, -Fx::ratio(1, 100)), ..System::default() };
        let rope = s.rope(V2::int(0, 0), V2::int(10, 0), 10, Fx::ONE, link);
        s.pin(rope[0]);
        (s, rope)
    }

    #[test]
    fn a_pinned_point_never_moves() {
        let (mut s, rope) = hanging(Link::Max);
        for _ in 0..500 {
            s.step();
        }
        assert_eq!(s.points[rope[0]].pos, V2::int(0, 0));
    }

    #[test]
    fn a_hanging_rope_settles_below_its_pin_at_its_length() {
        let (mut s, rope) = hanging(Link::Max);
        for _ in 0..3000 {
            s.step();
        }
        let end = s.points[*rope.last().unwrap()];
        // Straight down, 10 units, give or take the stretch gravity leaves after the passes.
        assert!(end.pos.x.abs() < Fx::ratio(1, 10), "end drifted sideways: {:?}", end.pos);
        assert!(end.pos.y < -Fx::int(9) && end.pos.y > -Fx::ratio(105, 10), "end at {:?}", end.pos);
        assert!(end.velocity().length() < Fx::ratio(1, 100), "still moving: {:?}", end.velocity());
    }

    #[test]
    fn a_rope_goes_slack_and_a_rod_does_not() {
        for (link, pushed) in [(Link::Max, false), (Link::Distance, true)] {
            let mut s = System::default();
            let a = s.point(V2::int(0, 0), Fx::ZERO);
            let b = s.point(V2::int(10, 0), Fx::ONE);
            s.link(a, b, link);
            s.points[b].pos = V2::int(5, 0);
            s.points[b].prev = V2::int(5, 0);
            s.step();
            let x = s.points[b].pos.x;
            assert_eq!(x > Fx::int(5), pushed, "{link:?}: b at {x:?}");
        }
    }

    #[test]
    fn a_strut_resists_only_shortening() {
        let mut s = System::default();
        let a = s.point(V2::int(0, 0), Fx::ZERO);
        let b = s.point(V2::int(10, 0), Fx::ONE);
        s.link(a, b, Link::Min);
        s.drag(a, V2::int(0, 0));
        s.points[b].pos = V2::int(20, 0);
        s.points[b].prev = V2::int(20, 0);
        s.step();
        assert_eq!(s.points[b].pos, V2::int(20, 0), "a strut let a longer link alone");
        s.points[b].pos = V2::int(4, 0);
        s.points[b].prev = V2::int(4, 0);
        s.step();
        assert!(s.points[b].pos.x > Fx::int(9), "a strut let itself shorten: {:?}", s.points[b].pos);
    }

    #[test]
    fn a_correction_is_split_by_weight() {
        let mut s = System { passes: 1, ..System::default() };
        let light = s.point(V2::int(0, 0), Fx::int(4));
        let heavy = s.point(V2::int(10, 0), Fx::ONE);
        s.link(light, heavy, Link::Max);
        s.points[heavy].pos = V2::int(20, 0);
        s.points[heavy].prev = V2::int(20, 0);
        s.step();
        let moved_light = s.points[light].pos.x;
        let moved_heavy = Fx::int(20) - s.points[heavy].pos.x;
        // Stretch 10 split 4:1: the light end moves 8, the heavy one 2.
        assert_eq!(moved_light, Fx::int(8));
        assert_eq!(moved_heavy, Fx::int(2));
    }

    #[test]
    fn stretch_left_in_a_pulled_rope_becomes_speed_when_it_is_let_go() {
        // A slingshot: a band of 8 links between two pins 8 apart, its middle pulled down 6 by a finger.
        let mut s = System { passes: 4, ..System::default() };
        let band = s.rope(V2::int(-4, 0), V2::int(4, 0), 8, Fx::ONE, Link::Max);
        s.pin(band[0]);
        s.pin(band[8]);
        let mid = band[4];
        s.pin(mid);
        for k in 1..=6 {
            s.drag(mid, V2::int(0, -k));
            s.step();
        }
        assert!(s.tension() > Fx::ZERO, "a pulled band is taut");
        s.drag(mid, V2::int(0, -6));
        s.step();
        s.release(mid, Fx::ONE);
        s.step();
        s.step();
        let v = s.points[mid].velocity();
        assert!(v.y > Fx::ratio(1, 2), "let go, the middle flies back up: {v:?}");
    }

    #[test]
    fn the_same_start_gives_the_same_bits() {
        let run = || {
            let (mut s, rope) = hanging(Link::Distance);
            s.points[rope[10]].pos = V2::new(Fx::ratio(107, 10), Fx::ratio(33, 10));
            for _ in 0..777 {
                s.step();
            }
            s
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn a_cut_rope_drops_its_load() {
        let (mut s, rope) = hanging(Link::Max);
        for _ in 0..2000 {
            s.step();
        }
        let before = s.points[rope[10]].pos.y;
        s.cut(4);
        for _ in 0..100 {
            s.step();
        }
        assert!(s.points[rope[10]].pos.y < before - Fx::int(10), "the load kept hanging after the cut");
        assert!(s.points[rope[4]].pos.y > before, "the part above the cut fell too");
    }

    #[test]
    fn a_swipe_across_finds_the_link_it_crosses() {
        let mut s = System::default();
        let rope = s.rope(V2::int(0, 0), V2::int(10, 0), 10, Fx::ONE, Link::Max);
        // A downward stroke at x = 3.5 crosses link 3 (points 3–4) halfway down the stroke.
        let hit = s.crossed(V2::new(Fx::ratio(35, 10), Fx::int(1)), V2::new(Fx::ratio(35, 10), -Fx::int(1)));
        assert_eq!(hit, Some((3, Fx::HALF)));
        assert_eq!(s.crossed(V2::int(0, 5), V2::int(10, 5)), None, "a stroke above the rope misses it");
        assert_eq!(rope.len(), 11);
    }

    #[test]
    fn crossing_is_exact_on_both_segments() {
        // apelann's Unity slingshot computed the second segment's share with x twice; this checks both.
        let t = crossing(V2::int(0, 0), V2::int(4, 4), V2::int(0, 4), V2::int(4, 0));
        assert_eq!(t, Some(Fx::HALF));
        assert_eq!(crossing(V2::int(0, 0), V2::int(1, 1), V2::int(3, 0), V2::int(4, -5)), None);
        assert_eq!(crossing(V2::int(0, 0), V2::int(1, 0), V2::int(0, 1), V2::int(1, 1)), None, "parallel");
    }
}
