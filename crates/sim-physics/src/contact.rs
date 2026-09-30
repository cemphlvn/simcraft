//! Contact: cars against walls and against each other, once a tick after the step.
//!
//! Each car is a box (its `half` length and width) turned by its heading. Two boxes touch when no separating axis
//! exists; in 2D a box has two face normals, so four candidate axes decide it (the separating axis theorem;
//! Gregorius, GDC 2013). The axis of least overlap gives the contact normal, the deepest corner the contact point.
//! The response is a sequential impulse (Catto): positions pushed apart by the overlap (split by mass), then one
//! impulse along the normal (restitution) and one along the surface (Coulomb friction, at most μ times the normal
//! one), both through the contact point, so an off-centre hit turns the car. Walls are the same with an immovable
//! partner. Linear momentum is conserved between cars and energy never grows (`contact_conserves_momentum...`).

use crate::fixed::Fx;
use crate::track::Track;
use crate::vehicle::Fleet;

/// How bouncy a hit is (0: dead, 1: elastic), and how much a scrape grips. Stock cars against each other and
/// against a SAFER barrier: little bounce, a lot of scrape.
pub const RESTITUTION: Fx = Fx::ratio(2, 10);
pub const FRICTION: Fx = Fx::ratio(5, 10);
/// Overlap left alone (m): pushing out every last millimetre makes resting contact jitter (Catto's slop).
const SLOP: Fx = Fx::ratio(1, 100);
/// Corners this close to the deepest one count as touching too (m).
const MANIFOLD: Fx = Fx::ratio(2, 100);

type V = (Fx, Fx);

fn dot(a: V, b: V) -> Fx {
    a.0 * b.0 + a.1 * b.1
}

fn cross(a: V, b: V) -> Fx {
    a.0 * b.1 - a.1 * b.0
}

fn sub(a: V, b: V) -> V {
    (a.0 - b.0, a.1 - b.1)
}

fn scale(a: V, k: Fx) -> V {
    (a.0 * k, a.1 * k)
}

/// A car as the contact solver sees it: world frame.
#[derive(Clone, Copy, Debug)]
struct Body {
    c: V,
    fwd: V,
    left: V,
    half: V,
    v: V,
    w: Fx,
    /// Mass (kg) and yaw inertia (kg·m²): divided by, never inverted (1/1542 in Q16 keeps two digits).
    m: Fx,
    i: Fx,
}

/// Inverse masses are carried per tonne (1000/m): a 1542 kg car is 0.648, precise in Q16, where 1/m is not.
const TONNE: Fx = Fx::int(1000);

impl Body {
    fn of(f: &Fleet, i: usize) -> Body {
        let yaw = f.yaw[i];
        let (co, si) = (yaw.cos(), yaw.sin());
        let (fwd, left) = ((co, si), (-si, co));
        let p = &f.params[i];
        Body {
            c: (f.x[i], f.y[i]),
            fwd,
            left,
            half: f.half[i],
            v: (f.vx[i] * co - f.vy[i] * si, f.vx[i] * si + f.vy[i] * co),
            w: f.yaw_rate[i],
            m: p.mass,
            i: p.iz,
        }
    }

    fn store(&self, f: &mut Fleet, i: usize) {
        (f.x[i], f.y[i]) = self.c;
        f.vx[i] = dot(self.v, self.fwd);
        f.vy[i] = dot(self.v, self.left);
        f.yaw_rate[i] = self.w;
    }

    fn corners(&self) -> [V; 4] {
        let (a, b) = (scale(self.fwd, self.half.0), scale(self.left, self.half.1));
        [
            (self.c.0 + a.0 + b.0, self.c.1 + a.1 + b.1),
            (self.c.0 + a.0 - b.0, self.c.1 + a.1 - b.1),
            (self.c.0 - a.0 + b.0, self.c.1 - a.1 + b.1),
            (self.c.0 - a.0 - b.0, self.c.1 - a.1 - b.1),
        ]
    }

    /// Half the box's extent along `axis`.
    fn reach(&self, axis: V) -> Fx {
        self.half.0 * dot(self.fwd, axis).abs() + self.half.1 * dot(self.left, axis).abs()
    }

    /// Velocity of the point `r` (from the centre) on this body.
    fn at(&self, r: V) -> V {
        (self.v.0 - self.w * r.1, self.v.1 + self.w * r.0)
    }

    fn push(&mut self, r: V, j: V) {
        self.v = (self.v.0 + j.0 / self.m, self.v.1 + j.1 / self.m);
        self.w += cross(r, j) / self.i;
    }

    /// How hard this body is to move at `r` along `dir`, per tonne: 1000/m + (r × dir)²·1000/I.
    fn give(&self, r: V, dir: V) -> Fx {
        let c = cross(r, dir);
        TONNE / self.m + c * c * TONNE / self.i
    }
}

/// Sequential impulses (Catto): `ITERATIONS` rounds of a normal impulse toward the bounce the closing speed asked
/// for, then friction along the surface, each accumulated and clamped (the normal never pulls, friction never
/// exceeds μ times it), so a scrape at a corner cannot push the corner back into what it hit. `b` None is a wall.
/// Returns the size of the total impulse (N·s).
fn resolve(a: &mut Body, ra: V, mut b: Option<(&mut Body, V)>, n: V) -> Fx {
    let t = (-n.1, n.0);
    let rel = |a: &Body, b: &Option<(&mut Body, V)>| {
        let va = a.at(ra);
        match b {
            Some((b, rb)) => sub(b.at(*rb), va),
            None => (-va.0, -va.1),
        }
    };
    let give = |dir: V, a: &Body, b: &Option<(&mut Body, V)>| a.give(ra, dir) + b.as_ref().map_or(Fx::ZERO, |(b, rb)| b.give(*rb, dir));
    let apply = |a: &mut Body, b: &mut Option<(&mut Body, V)>, j: V| {
        a.push(ra, (-j.0, -j.1));
        if let Some((b, rb)) = b {
            b.push(*rb, j);
        }
    };
    let closing = dot(rel(a, &b), n);
    if closing >= Fx::ZERO {
        return Fx::ZERO; // already separating
    }
    let bounce = -(RESTITUTION * closing);
    let (kn, kt) = (give(n, a, &b), give(t, a, &b));
    let (mut pn, mut pt) = (Fx::ZERO, Fx::ZERO);
    for _ in 0..ITERATIONS {
        let dn = (bounce - dot(rel(a, &b), n)) * TONNE / kn;
        let new = (pn + dn).max(Fx::ZERO);
        apply(a, &mut b, scale(n, new - pn));
        pn = new;
        let dt = -(dot(rel(a, &b), t)) * TONNE / kt;
        let new = (pt + dt).clamp(-(FRICTION * pn), FRICTION * pn);
        apply(a, &mut b, scale(t, new - pt));
        pt = new;
    }
    (pn * pn + pt * pt).sqrt()
}

/// Rounds of the contact solver per contact.
const ITERATIONS: usize = 8;

/// Keep every car inside the track's walls. `places` are the cars' places on the track (from `sit_on`).
pub fn walls(fleet: &mut Fleet, track: &Track, places: &[crate::track::Place]) {
    for (i, place) in places.iter().enumerate() {
        let heading = track.pose(place.s, Fx::ZERO).heading;
        let left = (-heading.sin(), heading.cos());
        let mut body = Body::of(fleet, i);
        let mut hit = Fx::ZERO;
        // The left wall is at a positive offset, the right at a negative one; `side` says which way is outward.
        for (wall, side) in [(track.wall_left, Fx::ONE), (track.wall_right, -Fx::ONE)] {
            let Some(wall) = wall else { continue };
            let inward = scale(left, -side);
            // How far past the wall the deepest corner is.
            let deepest = body
                .corners()
                .into_iter()
                .map(|corner| ((place.offset + dot(sub(corner, body.c), left) - wall) * side, corner))
                .max_by_key(|&(past, _)| past);
            let Some((past, corner)) = deepest else { continue };
            if past <= Fx::ZERO {
                continue;
            }
            // The contact point relative to the centre (unchanged by the push), then out of the wall.
            let r = sub(corner, body.c);
            body.c = (body.c.0 + inward.0 * past, body.c.1 + inward.1 * past);
            // The wall is the immovable partner; the normal from the car toward it is outward.
            hit += resolve(&mut body, r, None, scale(inward, -Fx::ONE));
        }
        body.store(fleet, i);
        fleet.impact[i] += hit;
    }
}

/// Separate every pair of overlapping cars and exchange the impulses of the hit.
pub fn cars(fleet: &mut Fleet) {
    let n = fleet.len();
    // Sweep and prune along x: sorted by the left edge of each car's bounding circle.
    let radius: Vec<Fx> = (0..n).map(|i| (fleet.half[i].0 * fleet.half[i].0 + fleet.half[i].1 * fleet.half[i].1).sqrt()).collect();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by_key(|&i| (fleet.x[i] - radius[i], i));
    for (k, &a) in order.iter().enumerate() {
        for &b in &order[k + 1..] {
            if fleet.x[b] - radius[b] > fleet.x[a] + radius[a] {
                break;
            }
            if (fleet.y[b] - fleet.y[a]).abs() > radius[a] + radius[b] {
                continue;
            }
            let (lo, hi) = (a.min(b), a.max(b));
            pair(fleet, lo, hi);
        }
    }
}

/// Two cars: the separating axis test, then the response.
fn pair(fleet: &mut Fleet, ia: usize, ib: usize) {
    let (mut a, mut b) = (Body::of(fleet, ia), Body::of(fleet, ib));
    let d = sub(b.c, a.c);
    // The four face normals; the one with the least overlap is the contact normal (from a toward b).
    let mut best: Option<(Fx, V, bool)> = None;
    for (axis, of_a) in [(a.fwd, true), (a.left, true), (b.fwd, false), (b.left, false)] {
        let overlap = a.reach(axis) + b.reach(axis) - dot(d, axis).abs();
        if overlap <= Fx::ZERO {
            return; // a separating axis: no contact
        }
        if best.is_none_or(|(o, _, _)| overlap < o) {
            let n = if dot(d, axis) < Fx::ZERO { (-axis.0, -axis.1) } else { axis };
            best = Some((overlap, n, of_a));
        }
    }
    let Some((overlap, n, of_a)) = best else { return };
    // The contact point: the corners of the other box that reach deepest across the face, averaged over those
    // within a couple of centimetres of the deepest (a flush hit touches along an edge: one corner alone would
    // push off-centre and invent a spin; this is the two-point manifold of a box engine, reduced to its middle).
    let (others, sign) = if of_a { (b.corners(), Fx::ONE) } else { (a.corners(), -Fx::ONE) };
    let depth = |p: V| -dot(p, n) * sign;
    let deepest = others.iter().map(|&p| depth(p)).max().unwrap_or(Fx::ZERO);
    let near: Vec<V> = others.into_iter().filter(|&p| depth(p) >= deepest - MANIFOLD).collect();
    let count = near.len().max(1) as i64;
    let point = near.iter().fold((Fx::ZERO, Fx::ZERO), |acc, p| (acc.0 + p.0, acc.1 + p.1));
    let point = (point.0 / count, point.1 / count);
    // Push apart, split by mass (the lighter car moves more).
    let push = (overlap - SLOP).max(Fx::ZERO);
    let (ga, gb) = (TONNE / a.m, TONNE / b.m);
    let (sa, sb) = (ga / (ga + gb), gb / (ga + gb));
    a.c = sub(a.c, scale(n, push * sa));
    b.c = (b.c.0 + n.0 * push * sb, b.c.1 + n.1 * push * sb);
    let (ra, rb) = (sub(point, a.c), sub(point, b.c));
    let j = resolve(&mut a, ra, Some((&mut b, rb)), n);
    a.store(fleet, ia);
    b.store(fleet, ib);
    fleet.impact[ia] += j;
    fleet.impact[ib] += j;
}

/// Clear the impact outputs before a tick's contact passes.
pub fn clear(fleet: &mut Fleet) {
    fleet.impact.fill(Fx::ZERO);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixed::Angle;
    use crate::vehicle::{Params, VehicleDef};

    fn car() -> Params {
        let d: VehicleDef = ron::from_str(include_str!("../../../assets/vehicles/stock_car.ron")).unwrap();
        Params::new(&d)
    }

    fn f(x: Fx) -> f64 {
        x.0 as f64 / 65536.0
    }

    fn momentum(fl: &Fleet) -> (f64, f64) {
        (0..fl.len()).fold((0.0, 0.0), |(px, py), i| {
            let b = Body::of(fl, i);
            let m = f(fl.params[i].mass);
            (px + m * f(b.v.0), py + m * f(b.v.1))
        })
    }

    fn energy(fl: &Fleet) -> f64 {
        (0..fl.len())
            .map(|i| {
                let b = Body::of(fl, i);
                let (m, iz) = (f(fl.params[i].mass), f(fl.params[i].iz));
                0.5 * m * (f(b.v.0).powi(2) + f(b.v.1).powi(2)) + 0.5 * iz * f(b.w).powi(2)
            })
            .sum()
    }

    #[test]
    fn apart_is_apart() {
        let mut fl = Fleet::default();
        fl.add(car(), Fx::ZERO, Fx::ZERO, Angle::ZERO);
        fl.add(car(), Fx::int(6), Fx::ZERO, Angle::ZERO);
        fl.vx[0] = Fx::int(10);
        let before = (fl.x.clone(), fl.vx.clone());
        cars(&mut fl);
        assert_eq!((fl.x.clone(), fl.vx.clone()), before, "5 m cars 6 m apart do not touch");
    }

    #[test]
    fn a_rear_end_hit_passes_speed_forward_and_conserves_momentum() {
        let mut fl = Fleet::default();
        fl.add(car(), Fx::ZERO, Fx::ZERO, Angle::ZERO);
        fl.add(car(), Fx::ratio(48, 10), Fx::ZERO, Angle::ZERO);
        fl.vx[0] = Fx::int(20);
        fl.vx[1] = Fx::int(15);
        let (p0, e0) = (momentum(&fl), energy(&fl));
        cars(&mut fl);
        let (p1, e1) = (momentum(&fl), energy(&fl));
        assert!((p1.0 - p0.0).abs() < 1.0 && (p1.1 - p0.1).abs() < 1.0, "momentum {p0:?} → {p1:?}");
        assert!(e1 <= e0 + 1.0, "energy grew {e0} → {e1}");
        // Equal masses, restitution e: the closing speed 5 m/s leaves at e·5 apart.
        let (va, vb) = (f(fl.vx[0]), f(fl.vx[1]));
        assert!((vb - va - 0.2 * 5.0).abs() < 0.05, "after: {va} and {vb}");
        let length = 2.0 * f(fl.half[0].0);
        let gap = f(fl.x[1]) - f(fl.x[0]);
        assert!(gap >= length - 0.011, "pushed apart to within the slop: {gap} vs length {length}");
    }

    #[test]
    fn random_hits_conserve_momentum_and_never_make_energy() {
        // Property test (research §3.7): any two cars, any overlap, any velocities.
        let mut seed = 99u64;
        let mut next = |lo: i64, hi: i64| {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            lo + ((seed >> 33) as i64).rem_euclid(hi - lo + 1)
        };
        let mut touched = 0;
        for _ in 0..2000 {
            let mut fl = Fleet::default();
            fl.add(car(), Fx::ZERO, Fx::ZERO, Angle(next(0, 1 << 30) << 2));
            fl.add(car(), Fx::ratio(next(-5000, 5000), 1000), Fx::ratio(next(-2500, 2500), 1000), Angle(next(0, 1 << 30) << 2));
            for i in 0..2 {
                fl.vx[i] = Fx::ratio(next(0, 60_000), 1000);
                fl.vy[i] = Fx::ratio(next(-3000, 3000), 1000);
                fl.yaw_rate[i] = Fx::ratio(next(-1000, 1000), 1000);
            }
            let (p0, e0) = (momentum(&fl), energy(&fl));
            cars(&mut fl);
            if fl.impact[0] > Fx::ZERO {
                touched += 1;
            }
            let (p1, e1) = (momentum(&fl), energy(&fl));
            let scale = 1.0 + p0.0.abs() + p0.1.abs();
            assert!((p1.0 - p0.0).abs() < 1e-3 * scale && (p1.1 - p0.1).abs() < 1e-3 * scale, "momentum {p0:?} → {p1:?}");
            assert!(e1 <= e0 * 1.0001 + 1.0, "energy grew {e0} → {e1}");
        }
        assert!(touched > 200, "only {touched} of 2000 cases touched: the test must exercise contact");
    }

    #[test]
    fn the_wall_turns_a_car_back_and_scrapes_it() {
        use crate::track::{SegmentDef, Shape, SideDef, TrackDef};
        // A straight track along +x, 10 m wide, walls on both sides; a car at 30 m/s angled 15° into the right one.
        let def = TrackDef {
            name: "strip".into(),
            width: 10000,
            start: (0, 0, 0),
            blend: 0,
            left: SideDef { width: 0, grip: 1000, wall: true },
            right: SideDef { width: 0, grip: 1000, wall: true },
            segments: vec![
                SegmentDef { shape: Shape::Straight(1_000_000), bank: 0, name: String::new() },
                SegmentDef { shape: Shape::Left(50_000, 18000), bank: 0, name: String::new() },
                SegmentDef { shape: Shape::Straight(1_000_000), bank: 0, name: String::new() },
                SegmentDef { shape: Shape::Left(50_000, 18000), bank: 0, name: String::new() },
            ],
        };
        let track = Track::new(&def).unwrap();
        let mut fl = Fleet::default();
        fl.add(car(), Fx::int(100), -Fx::ratio(42, 10), -Angle::centidegrees(1500));
        fl.vx[0] = Fx::int(30);
        let place = track.locate(fl.x[0], fl.y[0], None);
        walls(&mut fl, &track, &[place]);
        let b = Body::of(&fl, 0);
        let corners_inside = b.corners().iter().all(|c| f(c.1) >= -5.0 - 0.01);
        assert!(corners_inside, "pushed back inside the wall: {:?}", b.corners());
        // The corner that hit (the front right, lowest y) no longer moves into the wall (+y is inward).
        let hit = b.corners().into_iter().min_by_key(|c| c.1).unwrap();
        let vy = f(b.at(sub(hit, b.c)).1);
        assert!(vy >= -0.01, "the corner still moves into the wall at {vy} m/s");
        assert!(f(b.w) > 0.0, "the hit turns the car away from the wall (left): {}", f(b.w));
        assert!(f(b.v.0) < 30.0 * (15f64).to_radians().cos(), "the scrape slowed it along the wall: {}", f(b.v.0));
        assert!(fl.impact[0] > Fx::ZERO);
    }
}
