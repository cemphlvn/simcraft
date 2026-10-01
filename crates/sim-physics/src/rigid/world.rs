//! The world: bodies in slots, and the step (see the module docs for its stages).

use super::collide::{self, Clip, Contact, Placed, Placement};
use super::hull::Hull;
use super::math::{M3, Quat, V3};
use super::{Body, BodyDef, BodyId, Impact, Shape, Stats};

/// How the solver behaves, in metres. Box2D v3's defaults (`docs/research/smash-physics.md` §2) except contact
/// stiffness: 30 Hz lets a box under nine others sink 5 mm a contact (overlap ≈ load · g / ω²), so a struck tower
/// visibly squashes. 60 Hz with 6 substeps sags 1.3 mm and keeps a third below the stability cap (a quarter of the
/// substep rate; at 4 substeps 60 Hz sits on the cap and a stack of ten rocks forever).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tuning {
    /// Stiffness of a contact (Hz); capped at a quarter of the substep rate. Contacts with a fixed body use twice this
    /// (up to the same cap).
    pub contact_hertz: f32,
    /// Damping of a contact (ratio; 10 is heavily overdamped: no bounce from pushing out).
    pub damping_ratio: f32,
    /// The fastest a contact pushes overlapping bodies apart (m/s): deep overlaps resolve gently, never explode.
    pub push_max: f32,
    /// Overlap the solver tolerates (m).
    pub linear_slop: f32,
    /// Contacts are made this far before touching (m), so a resting body never falls in a step.
    pub speculative: f32,
    /// A body this slow for this long (s) may sleep; its island sleeps when all of it may.
    pub sleep_time: f32,
    pub sleep_lin: f32,
    pub sleep_ang: f32,
    /// Bounce only above this closing speed (m/s): resting contacts never bounce.
    pub restitution_threshold: f32,
    /// Velocity lost per second, as a rate (1/s).
    pub linear_damping: f32,
    pub angular_damping: f32,
    /// Rolling resistance (m): how hard a rolling sphere or prism is slowed per unit of contact force.
    pub rolling_resistance: f32,
    /// Warm starting reuses last step's impulse for a contact point this close to where it was (m).
    pub recycle: f32,
}

impl Default for Tuning {
    fn default() -> Tuning {
        let slop = 0.005;
        Tuning {
            contact_hertz: 60.0,
            damping_ratio: 10.0,
            push_max: 3.0,
            linear_slop: slop,
            speculative: 4.0 * slop,
            sleep_time: 0.5,
            sleep_lin: 0.05,
            sleep_ang: 0.05,
            restitution_threshold: 1.0,
            linear_damping: 0.0,
            angular_damping: 0.05,
            rolling_resistance: 0.02,
            recycle: 10.0 * slop,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Point {
    /// Anchors in each body's local frame (for matching across steps).
    local_a: V3,
    local_b: V3,
    /// World offsets from each centre at the step's start (fixed during the step).
    ra: V3,
    rb: V3,
    /// Separation at the start minus the anchors' offset along the normal: current separation is
    /// `dot(Δ, n) + adjusted` with Δ the anchors' movement.
    adjusted: f32,
    sep: f32,
    normal_mass: f32,
    tangent_mass: [f32; 2],
    /// Closing speed at the start (negative when closing), for restitution.
    rel_vel: f32,
    normal_impulse: f32,
    /// World tangent impulse, carried across steps by projection onto the new tangents.
    tangent_impulse: V3,
    max_normal: f32,
    /// Normal impulse applied over the whole step (for impacts).
    total: f32,
}

#[derive(Clone, Debug, Default)]
struct Manifold {
    a: u32,
    b: u32,
    normal: V3,
    t1: V3,
    t2: V3,
    points: [Point; 4],
    count: usize,
    friction: f32,
    restitution: f32,
    rolling: f32,
    rolling_impulse: V3,
    rolling_mass: M3,
    /// Contact softness (hertz used for this manifold).
    hertz: f32,
}

impl Manifold {
    fn key(&self) -> (u32, u32) {
        (self.a, self.b)
    }
}

#[derive(Clone, Debug)]
struct Slot {
    body: Body,
    alive: bool,
    generation: u32,
    hull: Option<Hull>,
    sleep_timer: f32,
}

/// Per-body solver state for one step, by slot.
#[derive(Clone, Copy, Debug, Default)]
struct State {
    v: V3,
    w: V3,
    inv_m: f32,
    inv_i: M3,
    x0: V3,
    q0: Quat,
    /// Movement since the step began.
    dp: V3,
    dq: Quat,
}

impl Default for M3 {
    fn default() -> M3 {
        M3::ZERO
    }
}

#[derive(Clone)]
pub struct World {
    pub gravity: V3,
    pub substeps: u32,
    pub tuning: Tuning,
    slots: Vec<Slot>,
    free: Vec<u32>,
    manifolds: Vec<Manifold>,
    old: Vec<Manifold>,
    impacts: Vec<Impact>,
    stats: Stats,
    // Reused buffers.
    aabbs: Vec<(V3, V3)>,
    order: Vec<u32>,
    pairs: Vec<(u32, u32)>,
    states: Vec<State>,
    verts_a: Placement,
    verts_b: Placement,
    clip: Clip,
    parent: Vec<u32>,
}

impl Default for World {
    fn default() -> World {
        World::new()
    }
}

/// A body's inverse inertia about its local axes, from its shape and mass.
fn inertia(shape: &Shape, m: f32) -> V3 {
    let i = match *shape {
        Shape::Box { half: h } => V3::new(h.y * h.y + h.z * h.z, h.x * h.x + h.z * h.z, h.x * h.x + h.y * h.y) * (m / 3.0),
        Shape::Sphere { radius } => V3::splat(0.4 * m * radius * radius),
        Shape::Prism { radius, half_height, .. } => {
            let side = m / 12.0 * (3.0 * radius * radius + 4.0 * half_height * half_height);
            V3::new(side, 0.5 * m * radius * radius, side)
        }
    };
    V3::new(1.0 / i.x, 1.0 / i.y, 1.0 / i.z)
}

/// The radius of the ball that holds a shape (for bounds and rolling).
fn reach(shape: &Shape) -> V3 {
    match *shape {
        Shape::Box { half } => half,
        Shape::Sphere { radius } => V3::splat(radius),
        Shape::Prism { radius, half_height, .. } => V3::new(radius, half_height, radius),
    }
}

fn rolls(shape: &Shape) -> bool {
    matches!(shape, Shape::Sphere { .. } | Shape::Prism { .. })
}

/// The inverse of a symmetric 3 × 3 matrix (zero if it has none).
fn inverse(m: &M3) -> M3 {
    let [a, b, c] = m.c;
    let r0 = b.cross(c);
    let r1 = c.cross(a);
    let r2 = a.cross(b);
    let det = a.dot(r0);
    if det.abs() < 1e-20 {
        return M3::ZERO;
    }
    let k = 1.0 / det;
    M3 { c: [r0 * k, r1 * k, r2 * k] }.transpose()
}

impl World {
    pub fn new() -> World {
        World {
            gravity: V3::new(0.0, -9.81, 0.0),
            substeps: 6,
            tuning: Tuning::default(),
            slots: Vec::new(),
            free: Vec::new(),
            manifolds: Vec::new(),
            old: Vec::new(),
            impacts: Vec::new(),
            stats: Stats::default(),
            aabbs: Vec::new(),
            order: Vec::new(),
            pairs: Vec::new(),
            states: Vec::new(),
            verts_a: Placement::default(),
            verts_b: Placement::default(),
            clip: Clip::default(),
            parent: Vec::new(),
        }
    }

    pub fn add(&mut self, def: BodyDef) -> BodyId {
        let fixed = def.fixed;
        let m = def.shape.volume() * def.material.density;
        let (inv_mass, inv_inertia) = if fixed || m <= 0.0 { (0.0, V3::ZERO) } else { (1.0 / m, inertia(&def.shape, m)) };
        let asleep = def.asleep && !fixed;
        let body = Body {
            shape: def.shape,
            pos: def.pos,
            rot: def.rot.normalized(),
            vel: if asleep || fixed { V3::ZERO } else { def.vel },
            ang: if asleep || fixed { V3::ZERO } else { def.ang },
            prev_pos: def.pos,
            prev_rot: def.rot.normalized(),
            material: def.material,
            inv_mass,
            inv_inertia,
            fixed,
            asleep,
            user: def.user,
            layer: def.layer,
            mask: def.mask,
        };
        let hull = Hull::of(&def.shape);
        match self.free.pop() {
            Some(i) => {
                let s = &mut self.slots[i as usize];
                s.body = body;
                s.alive = true;
                s.hull = hull;
                s.sleep_timer = 0.0;
                BodyId { index: i, generation: s.generation }
            }
            None => {
                self.slots.push(Slot { body, alive: true, generation: 0, hull, sleep_timer: 0.0 });
                BodyId { index: self.slots.len() as u32 - 1, generation: 0 }
            }
        }
    }

    pub fn remove(&mut self, id: BodyId) {
        if self.get(id).is_none() {
            return;
        }
        let i = id.index;
        // Whatever it touched must notice it is gone, sleeping or not (sleeping pairs keep no contacts).
        let (lo, hi) = self.bounds(i as usize, 0.0);
        let near: Vec<u32> = (0..self.slots.len() as u32)
            .filter(|&j| j != i && self.slots[j as usize].alive)
            .filter(|&j| {
                let (l, h) = self.bounds(j as usize, 0.0);
                l.x <= hi.x && h.x >= lo.x && l.y <= hi.y && h.y >= lo.y && l.z <= hi.z && h.z >= lo.z
            })
            .collect();
        let s = &mut self.slots[i as usize];
        s.alive = false;
        s.generation += 1;
        s.hull = None;
        self.free.push(i);
        self.manifolds.retain(|m| m.a != i && m.b != i);
        self.old.retain(|m| m.a != i && m.b != i);
        for j in near {
            self.wake_slot(j);
        }
    }

    pub fn get(&self, id: BodyId) -> Option<&Body> {
        self.slots.get(id.index as usize).filter(|s| s.alive && s.generation == id.generation).map(|s| &s.body)
    }

    pub fn get_mut(&mut self, id: BodyId) -> Option<&mut Body> {
        self.slots.get_mut(id.index as usize).filter(|s| s.alive && s.generation == id.generation).map(|s| &mut s.body)
    }

    pub fn iter(&self) -> impl Iterator<Item = (BodyId, &Body)> {
        self.slots.iter().enumerate().filter(|(_, s)| s.alive).map(|(i, s)| (BodyId { index: i as u32, generation: s.generation }, &s.body))
    }

    /// Wakes the body and every sleeping body touching it, and theirs (its island).
    pub fn wake(&mut self, id: BodyId) {
        if self.get(id).is_some() {
            self.wake_slot(id.index);
        }
    }

    pub fn impacts(&self) -> &[Impact] {
        &self.impacts
    }

    pub fn stats(&self) -> Stats {
        self.stats
    }

    fn id(&self, i: u32) -> BodyId {
        BodyId { index: i, generation: self.slots[i as usize].generation }
    }

    fn bounds(&self, i: usize, dt: f32) -> (V3, V3) {
        let b = &self.slots[i].body;
        let r = reach(&b.shape);
        let m = b.rot.mat();
        // The box's extent in the world: |R| · half.
        let ext = V3::new(
            m.c[0].x.abs() * r.x + m.c[1].x.abs() * r.y + m.c[2].x.abs() * r.z,
            m.c[0].y.abs() * r.x + m.c[1].y.abs() * r.y + m.c[2].y.abs() * r.z,
            m.c[0].z.abs() * r.x + m.c[1].z.abs() * r.y + m.c[2].z.abs() * r.z,
        );
        let grow = V3::splat(self.tuning.speculative);
        let (lo, hi) = (b.pos - ext - grow, b.pos + ext + grow);
        // Swept: where it can get to in this step.
        let d = b.vel * dt;
        (lo.min(lo + d), hi.max(hi + d))
    }

    /// Wakes slot `i` and floods through sleeping bodies whose bounds touch an awake one's.
    fn wake_slot(&mut self, i: u32) {
        let s = &self.slots[i as usize];
        if !s.alive || s.body.fixed || !s.body.asleep {
            return;
        }
        let mut stack = vec![i];
        self.slots[i as usize].body.asleep = false;
        self.slots[i as usize].sleep_timer = 0.0;
        while let Some(k) = stack.pop() {
            let (lo, hi) = self.bounds(k as usize, 0.0);
            let near = |(l, h): (V3, V3)| l.x <= hi.x && h.x >= lo.x && l.y <= hi.y && h.y >= lo.y && l.z <= hi.z && h.z >= lo.z;
            let me = self.slots[k as usize].body;
            for j in 0..self.slots.len() {
                let s = &self.slots[j];
                if s.alive && s.body.asleep && !s.body.fixed && s.body.collides(&me) && near(self.bounds(j, 0.0)) {
                    self.slots[j].body.asleep = false;
                    self.slots[j].sleep_timer = 0.0;
                    stack.push(j as u32);
                }
            }
        }
    }

    /// Sweep and prune on x: candidate pairs, lower slot first, in a fixed order.
    fn broadphase(&mut self, dt: f32) {
        self.aabbs.clear();
        for i in 0..self.slots.len() {
            let b = if self.slots[i].alive { self.bounds(i, dt) } else { (V3::ZERO, V3::ZERO) };
            self.aabbs.push(b);
        }
        self.order.clear();
        self.order.extend((0..self.slots.len() as u32).filter(|&i| self.slots[i as usize].alive));
        let aabbs = &self.aabbs;
        self.order.sort_by(|&a, &b| aabbs[a as usize].0.x.total_cmp(&aabbs[b as usize].0.x).then(a.cmp(&b)));
        self.pairs.clear();
        for (k, &i) in self.order.iter().enumerate() {
            let (lo, hi) = self.aabbs[i as usize];
            let bi = &self.slots[i as usize].body;
            let i_moves = !bi.fixed && !bi.asleep;
            for &j in &self.order[k + 1..] {
                let (l, h) = self.aabbs[j as usize];
                if l.x > hi.x {
                    break;
                }
                if l.y > hi.y || h.y < lo.y || l.z > hi.z || h.z < lo.z {
                    continue;
                }
                let bj = &self.slots[j as usize].body;
                if !i_moves && (bj.fixed || bj.asleep) || !bi.collides(bj) {
                    continue;
                }
                self.pairs.push((i.min(j), i.max(j)));
            }
        }
        self.pairs.sort_unstable();
    }

    /// A speculative contact on a sphere further away than the static margin is kept only if the sphere, moved by
    /// the pair's relative velocity for this step, really reaches the other body. Without this check a fast stone
    /// grazing past a corner is stopped by a contact that the linear model made up (a ghost collision, Catto): the
    /// stone flew 13 cm over the cans and hit them anyway, and the preview's arc lied (EVALS step 009).
    fn sweep_reaches(&mut self, a: u32, b: u32, c: &Contact, dt: f32) -> bool {
        let sep = (0..c.count).map(|k| c.points[k].2).fold(f32::MAX, f32::min);
        if sep <= self.tuning.speculative {
            return true;
        }
        let (ba, bb) = (self.slots[a as usize].body, self.slots[b as usize].body);
        let (sphere, other, radius, rel) = match (ba.shape, bb.shape) {
            (Shape::Sphere { radius }, _) => (a, b, radius, ba.vel - bb.vel),
            (_, Shape::Sphere { radius }) => (b, a, radius, bb.vel - ba.vel),
            _ => return true,
        };
        let reach = rel.length() * dt;
        if reach <= 1e-6 {
            return false;
        }
        let dir = rel.normalized();
        let from = self.slots[sphere as usize].body.pos;
        let o = &self.slots[other as usize];
        let ob = o.body;
        let mut t = 0.0f32;
        for _ in 0..32 {
            let p = from + dir * t;
            let d = match (&o.hull, ob.shape) {
                (_, Shape::Sphere { radius: rb }) => (p - ob.pos).length() - rb,
                (Some(h), _) => {
                    collide::place(h, ob.pos, ob.rot, &mut self.verts_a);
                    let placed = Placed { hull: h, pos: ob.pos, rot: ob.rot, verts: &self.verts_a.verts, normals: &self.verts_a.normals };
                    match collide::hull_sphere(&placed, p, 0.0, f32::MAX) {
                        Some(c) => c.points[0].2,
                        None => return true,
                    }
                }
                _ => return true,
            };
            let clear = d - radius;
            if clear <= self.tuning.speculative {
                return true;
            }
            t += clear;
            if t > reach {
                return false;
            }
        }
        true
    }

    fn collide(&mut self, a: u32, b: u32, margin: f32) -> Option<Contact> {
        let (sa, sb) = (&self.slots[a as usize], &self.slots[b as usize]);
        let (ba, bb) = (&sa.body, &sb.body);
        match (ba.shape, bb.shape) {
            (Shape::Sphere { radius: ra }, Shape::Sphere { radius: rb }) => collide::sphere_sphere(ba.pos, ra, bb.pos, rb, margin),
            (_, Shape::Sphere { radius }) => {
                collide::place(sa.hull.as_ref()?, ba.pos, ba.rot, &mut self.verts_a);
                let p = Placed {
                    hull: sa.hull.as_ref()?,
                    pos: ba.pos,
                    rot: ba.rot,
                    verts: &self.verts_a.verts,
                    normals: &self.verts_a.normals,
                };
                collide::hull_sphere(&p, bb.pos, radius, margin)
            }
            (Shape::Sphere { radius }, _) => {
                collide::place(sb.hull.as_ref()?, bb.pos, bb.rot, &mut self.verts_b);
                let p = Placed {
                    hull: sb.hull.as_ref()?,
                    pos: bb.pos,
                    rot: bb.rot,
                    verts: &self.verts_b.verts,
                    normals: &self.verts_b.normals,
                };
                let mut c = collide::hull_sphere(&p, ba.pos, radius, margin)?;
                c.normal = -c.normal;
                for k in 0..c.count {
                    let (pa, pb, s) = c.points[k];
                    c.points[k] = (pb, pa, s);
                }
                Some(c)
            }
            _ => {
                let (ha, hb) = (sa.hull.as_ref()?, sb.hull.as_ref()?);
                collide::place(ha, ba.pos, ba.rot, &mut self.verts_a);
                collide::place(hb, bb.pos, bb.rot, &mut self.verts_b);
                let pa = Placed { hull: ha, pos: ba.pos, rot: ba.rot, verts: &self.verts_a.verts, normals: &self.verts_a.normals };
                let pb = Placed { hull: hb, pos: bb.pos, rot: bb.rot, verts: &self.verts_b.verts, normals: &self.verts_b.normals };
                collide::hull_hull(&pa, &pb, margin, self.tuning.linear_slop, &mut self.clip)
            }
        }
    }

    /// Contacts for every candidate pair, warm-started from last step's matching points.
    fn narrowphase(&mut self, dt: f32) {
        std::mem::swap(&mut self.manifolds, &mut self.old);
        self.manifolds.clear();
        let pairs = std::mem::take(&mut self.pairs);
        for &(a, b) in &pairs {
            let (ba, bb) = (self.slots[a as usize].body, self.slots[b as usize].body);
            let margin = self.tuning.speculative + (ba.vel - bb.vel).length() * dt;
            let Some(c) = self.collide(a, b, margin) else { continue };
            if !self.sweep_reaches(a, b, &c, dt) {
                continue;
            }
            if c.count == 0 {
                continue;
            }
            let (ma, mb) = (ba.material, bb.material);
            let mut m = Manifold {
                a,
                b,
                normal: c.normal,
                count: c.count,
                friction: (ma.friction * mb.friction).max(0.0).sqrt(),
                restitution: ma.restitution.max(mb.restitution),
                rolling: if rolls(&ba.shape) || rolls(&bb.shape) { self.tuning.rolling_resistance } else { 0.0 },
                ..Manifold::default()
            };
            let old = self.old.binary_search_by_key(&(a, b), Manifold::key).ok().map(|k| &self.old[k]);
            for k in 0..c.count {
                let (pa, pb, sep) = c.points[k];
                let mid = (pa + pb) * 0.5;
                let p = &mut m.points[k];
                p.local_a = ba.rot.unrotate(mid - ba.pos);
                p.local_b = bb.rot.unrotate(mid - bb.pos);
                p.sep = sep;
                if let Some(o) = old.filter(|o| o.normal.dot(c.normal) > 0.95) {
                    let r2 = self.tuning.recycle * self.tuning.recycle;
                    if let Some(op) = o.points[..o.count].iter().find(|op| (op.local_a - p.local_a).length_sq() < r2) {
                        p.normal_impulse = op.normal_impulse;
                        p.tangent_impulse = op.tangent_impulse;
                    }
                }
            }
            if let Some(o) = old {
                m.rolling_impulse = o.rolling_impulse;
            }
            self.manifolds.push(m);
        }
        self.pairs = pairs;
    }

    pub fn step(&mut self, dt: f32) {
        for s in self.slots.iter_mut().filter(|s| s.alive) {
            s.body.prev_pos = s.body.pos;
            s.body.prev_rot = s.body.rot;
        }
        self.impacts.clear();
        if dt <= 0.0 {
            return;
        }
        self.broadphase(dt);
        self.narrowphase(dt);
        // A moving body about to touch a sleeping one wakes its island before the solve, so the hit itself is
        // solved this step (a sleeping body would otherwise be a wall).
        let mut woke = false;
        for k in 0..self.manifolds.len() {
            let m = &self.manifolds[k];
            let (ba, bb) = (&self.slots[m.a as usize].body, &self.slots[m.b as usize].body);
            let (awake_a, awake_b) = (!ba.fixed && !ba.asleep, !bb.fixed && !bb.asleep);
            if awake_a == awake_b || ba.fixed || bb.fixed {
                continue;
            }
            let closing = (bb.vel - ba.vel).dot(m.normal).min(0.0);
            let reach = self.tuning.linear_slop - closing * dt;
            if m.points[..m.count].iter().any(|p| p.sep < reach) {
                let sleeper = if awake_a { m.b } else { m.a };
                self.wake_slot(sleeper);
                woke = true;
            }
        }
        if woke {
            // Newly woken bodies need their pairs with each other too.
            self.broadphase(dt);
            let keep = std::mem::take(&mut self.old);
            self.narrowphase(dt);
            self.old = keep;
        }
        self.solve(dt);
        self.islands(dt);
        self.report();
    }

    fn solve(&mut self, dt: f32) {
        let n = self.slots.len();
        self.states.clear();
        self.states.resize(n, State::default());
        for (i, s) in self.slots.iter().enumerate() {
            let b = &s.body;
            let st = &mut self.states[i];
            st.x0 = b.pos;
            st.q0 = b.rot;
            st.dq = Quat::IDENTITY;
            if s.alive && !b.fixed && !b.asleep {
                st.v = b.vel;
                st.w = b.ang;
                st.inv_m = b.inv_mass;
                st.inv_i = M3::sandwich(&b.rot.mat(), b.inv_inertia);
            }
        }
        let h = dt / self.substeps.max(1) as f32;
        let inv_h = 1.0 / h;
        let t = self.tuning;
        // Prepare.
        for m in &mut self.manifolds {
            let (sa, sb) = (&self.states[m.a as usize], &self.states[m.b as usize]);
            let (ba, bb) = (&self.slots[m.a as usize].body, &self.slots[m.b as usize].body);
            let n = m.normal;
            let (t1, t2) = n.basis();
            m.t1 = t1;
            m.t2 = t2;
            let one_fixed = sa.inv_m == 0.0 || sb.inv_m == 0.0;
            m.hertz = (if one_fixed { 2.0 * t.contact_hertz } else { t.contact_hertz }).min(0.25 * inv_h);
            let k = |r_a: V3, r_b: V3, d: V3| {
                let ka = sa.inv_m + sa.inv_i.mul_v(r_a.cross(d)).cross(r_a).dot(d);
                let kb = sb.inv_m + sb.inv_i.mul_v(r_b.cross(d)).cross(r_b).dot(d);
                let k = ka + kb;
                if k > 0.0 { 1.0 / k } else { 0.0 }
            };
            for p in &mut m.points[..m.count] {
                p.ra = ba.rot.rotate(p.local_a);
                p.rb = bb.rot.rotate(p.local_b);
                p.adjusted = p.sep - (p.rb - p.ra).dot(n);
                p.normal_mass = k(p.ra, p.rb, n);
                p.tangent_mass = [k(p.ra, p.rb, t1), k(p.ra, p.rb, t2)];
                let dv = sb.v + sb.w.cross(p.rb) - sa.v - sa.w.cross(p.ra);
                p.rel_vel = dv.dot(n);
                p.max_normal = 0.0;
                p.total = 0.0;
                // Keep only the part of last step's friction that lies in the new tangent plane.
                p.tangent_impulse = p.tangent_impulse - n * p.tangent_impulse.dot(n);
            }
            m.rolling_mass = if m.rolling > 0.0 {
                inverse(&M3 { c: [sa.inv_i.c[0] + sb.inv_i.c[0], sa.inv_i.c[1] + sb.inv_i.c[1], sa.inv_i.c[2] + sb.inv_i.c[2]] })
            } else {
                M3::ZERO
            };
        }
        let lin_damp = 1.0 / (1.0 + h * t.linear_damping);
        let ang_damp = 1.0 / (1.0 + h * t.angular_damping);
        let g = self.gravity;
        let mut max_pen = (0.0f32, 0u32, 0u32);
        let substeps = self.substeps.max(1);
        for sub in 0..substeps {
            // Integrate velocities.
            for st in self.states.iter_mut().filter(|s| s.inv_m > 0.0) {
                st.v = (st.v + g * h) * lin_damp;
                st.w = st.w * ang_damp;
            }
            // Warm start.
            for m in &self.manifolds {
                let (a, b) = (m.a as usize, m.b as usize);
                for p in &m.points[..m.count] {
                    let imp = m.normal * p.normal_impulse + p.tangent_impulse;
                    apply(&mut self.states, a, b, p.ra, p.rb, imp);
                }
                if m.rolling > 0.0 {
                    apply_angular(&mut self.states, a, b, m.rolling_impulse);
                }
            }
            solve_contacts(&mut self.manifolds, &mut self.states, &t, inv_h, true, None);
            // Integrate positions.
            for st in self.states.iter_mut().filter(|s| s.inv_m > 0.0) {
                st.dp += st.v * h;
                st.dq = st.dq.integrate(st.w, h);
            }
            let last = sub + 1 == substeps;
            solve_contacts(&mut self.manifolds, &mut self.states, &t, inv_h, false, if last { Some(&mut max_pen) } else { None });
            for m in &mut self.manifolds {
                for p in &mut m.points[..m.count] {
                    p.total += p.normal_impulse;
                }
            }
        }
        // Restitution: bounce what was closing fast, now that the contact has been solved.
        for m in &mut self.manifolds {
            if m.restitution == 0.0 {
                continue;
            }
            let (a, b) = (m.a as usize, m.b as usize);
            for p in &mut m.points[..m.count] {
                if p.rel_vel > -t.restitution_threshold || p.max_normal == 0.0 {
                    continue;
                }
                let (sa, sb) = (&self.states[a], &self.states[b]);
                let vn = (sb.v + sb.w.cross(p.rb) - sa.v - sa.w.cross(p.ra)).dot(m.normal);
                let imp = -p.normal_mass * (vn + m.restitution * p.rel_vel);
                let new = (p.normal_impulse + imp).max(0.0);
                let d = new - p.normal_impulse;
                p.normal_impulse = new;
                p.max_normal = p.max_normal.max(d);
                p.total += d;
                apply(&mut self.states, a, b, p.ra, p.rb, m.normal * d);
            }
        }
        // Write back.
        for (i, s) in self.slots.iter_mut().enumerate() {
            let st = &self.states[i];
            if st.inv_m > 0.0 {
                s.body.vel = st.v;
                s.body.ang = st.w;
                s.body.pos = st.x0 + st.dp;
                s.body.rot = (st.dq * st.q0).normalized();
            }
        }
        self.stats.max_penetration = max_pen.0;
        let user = |i: u32| self.slots[i as usize].body.user;
        self.stats.deepest = if max_pen.0 > 0.0 { Some((user(max_pen.1), user(max_pen.2))) } else { None };
    }

    /// Union-find over touching awake bodies; an island whose bodies have all been slow long enough sleeps.
    fn islands(&mut self, dt: f32) {
        let t = self.tuning;
        for s in self.slots.iter_mut().filter(|s| s.alive && !s.body.fixed && !s.body.asleep) {
            let slow = s.body.vel.length_sq() < t.sleep_lin * t.sleep_lin && s.body.ang.length_sq() < t.sleep_ang * t.sleep_ang;
            s.sleep_timer = if slow { s.sleep_timer + dt } else { 0.0 };
        }
        let n = self.slots.len();
        self.parent.clear();
        self.parent.extend(0..n as u32);
        fn find(p: &mut [u32], mut i: u32) -> u32 {
            while p[i as usize] != i {
                p[i as usize] = p[p[i as usize] as usize];
                i = p[i as usize];
            }
            i
        }
        for m in &self.manifolds {
            let (ba, bb) = (&self.slots[m.a as usize].body, &self.slots[m.b as usize].body);
            if ba.fixed || bb.fixed || !m.points[..m.count].iter().any(|p| p.sep < t.linear_slop) {
                continue;
            }
            // A sleeping partner joins too: the island sleeps only if all of it may (it is not slow if just hit).
            let (ra, rb) = (find(&mut self.parent, m.a), find(&mut self.parent, m.b));
            if ra != rb {
                self.parent[ra.max(rb) as usize] = ra.min(rb);
            }
        }
        // The slowest-to-settle body of each island decides.
        let mut least = vec![f32::MAX; n];
        for i in 0..n {
            let s = &self.slots[i];
            if !s.alive || s.body.fixed {
                continue;
            }
            let r = find(&mut self.parent, i as u32) as usize;
            let timer = if s.body.asleep { f32::MAX } else { s.sleep_timer };
            least[r] = least[r].min(timer);
        }
        for i in 0..n {
            let s = &self.slots[i];
            if !s.alive || s.body.fixed || s.body.asleep {
                continue;
            }
            let r = find(&mut self.parent, i as u32) as usize;
            if least[r] >= t.sleep_time {
                let s = &mut self.slots[i];
                s.body.asleep = true;
                s.body.vel = V3::ZERO;
                s.body.ang = V3::ZERO;
            }
        }
    }

    fn report(&mut self) {
        let old = std::mem::take(&mut self.old);
        for m in &self.manifolds {
            let total: f32 = m.points[..m.count].iter().map(|p| p.total).sum();
            if total <= 0.0 {
                continue;
            }
            let (mut deepest, mut speed) = (0, 0.0f32);
            for k in 0..m.count {
                if m.points[k].sep < m.points[deepest].sep {
                    deepest = k;
                }
                speed = speed.max(-m.points[k].rel_vel);
            }
            let p = &m.points[deepest];
            let point = self.slots[m.a as usize].body.prev_pos + p.ra;
            let new = match old.binary_search_by_key(&(m.a, m.b), Manifold::key) {
                Ok(k) => old[k].points[..old[k].count].iter().all(|p| p.total <= 0.0),
                Err(_) => true,
            };
            self.impacts.push(Impact { a: self.id(m.a), b: self.id(m.b), point, normal: m.normal, impulse: total, speed, new });
        }
        self.old = old;
        let alive = self.slots.iter().filter(|s| s.alive);
        self.stats.bodies = alive.clone().count() as u32;
        self.stats.awake = alive.filter(|s| !s.body.fixed && !s.body.asleep).count() as u32;
        self.stats.pairs = self.pairs.len() as u32;
        self.stats.manifolds = self.manifolds.len() as u32;
        self.stats.points = self.manifolds.iter().map(|m| m.count as u32).sum();
    }

    /// The first body a ray hits within `max` (m): its handle, the distance and the surface normal there.
    pub fn raycast(&self, from: V3, dir: V3, max: f32) -> Option<(BodyId, f32, V3)> {
        let dir = dir.normalized();
        let mut best: Option<(BodyId, f32, V3)> = None;
        for (i, s) in self.slots.iter().enumerate() {
            if !s.alive {
                continue;
            }
            let b = &s.body;
            let hit = match (&s.hull, b.shape) {
                (_, Shape::Sphere { radius }) => {
                    let oc = from - b.pos;
                    let bq = oc.dot(dir);
                    let c = oc.length_sq() - radius * radius;
                    let disc = bq * bq - c;
                    (disc >= 0.0).then(|| -bq - disc.sqrt()).filter(|&t| t >= 0.0).map(|t| (t, (from + dir * t - b.pos).normalized()))
                }
                (Some(h), _) => {
                    let (o, d) = (b.rot.unrotate(from - b.pos), b.rot.unrotate(dir));
                    let (mut enter, mut exit, mut normal) = (0.0f32, max, V3::ZERO);
                    let mut ok = true;
                    for f in &h.faces {
                        let (num, den) = (f.offset - f.normal.dot(o), f.normal.dot(d));
                        if den.abs() < 1e-9 {
                            if num < 0.0 {
                                ok = false;
                                break;
                            }
                            continue;
                        }
                        let t = num / den;
                        if den < 0.0 {
                            if t > enter {
                                enter = t;
                                normal = f.normal;
                            }
                        } else {
                            exit = exit.min(t);
                        }
                        if enter > exit {
                            ok = false;
                            break;
                        }
                    }
                    (ok && normal != V3::ZERO).then(|| (enter, b.rot.rotate(normal)))
                }
                _ => None,
            };
            if let Some((t, nrm)) = hit
                && t <= max
                && best.is_none_or(|(_, bt, _)| t < bt)
            {
                best = Some((self.id(i as u32), t, nrm));
            }
        }
        best
    }

    /// The first body a sphere of `radius` moving from `from` along `dir` touches within `max`: (the body, the
    /// distance travelled, the surface's normal there). Only bodies a body of `layer` / `mask` would collide with.
    /// Conservative advancement: step by the clearance to the nearest body until it is (almost) zero, so the answer
    /// is the sphere's own, not a few rays' (rays let a grazing stone slip past a can: EVALS step 009).
    pub fn sphere_cast(&self, from: V3, dir: V3, radius: f32, max: f32, layer: u32, mask: u32) -> Option<(BodyId, f32, V3)> {
        let dir = dir.normalized();
        let end = from + dir * max;
        let (lo, hi) = (from.min(end) - V3::splat(radius), from.max(end) + V3::splat(radius));
        let mut place = Placement::default();
        let candidates: Vec<usize> = (0..self.slots.len())
            .filter(|&i| {
                let s = &self.slots[i];
                if !s.alive || s.body.layer & mask == 0 || layer & s.body.mask == 0 {
                    return false;
                }
                let (l, h) = self.bounds(i, 0.0);
                l.x <= hi.x && h.x >= lo.x && l.y <= hi.y && h.y >= lo.y && l.z <= hi.z && h.z >= lo.z
            })
            .collect();
        if candidates.is_empty() {
            return None;
        }
        let mut t = 0.0f32;
        for _ in 0..96 {
            let p = from + dir * t;
            let mut best: Option<(usize, f32, V3)> = None;
            for &i in &candidates {
                let s = &self.slots[i];
                let b = &s.body;
                let near = match (&s.hull, b.shape) {
                    (_, Shape::Sphere { radius: rb }) => {
                        let d = p - b.pos;
                        let l = d.length();
                        Some((l - rb, if l > 1e-6 { d * (1.0 / l) } else { V3::Y }))
                    }
                    (Some(h), _) => {
                        collide::place(h, b.pos, b.rot, &mut place);
                        let placed = Placed { hull: h, pos: b.pos, rot: b.rot, verts: &place.verts, normals: &place.normals };
                        collide::hull_sphere(&placed, p, 0.0, f32::MAX).map(|c| (c.points[0].2, c.normal))
                    }
                    _ => None,
                };
                if let Some((d, n)) = near
                    && best.is_none_or(|(_, bd, _)| d < bd)
                {
                    best = Some((i, d, n));
                }
            }
            let (i, d, n) = best?;
            let clear = d - radius;
            if clear <= 1e-3 {
                return Some((self.id(i as u32), t, n));
            }
            t += clear;
            if t > max {
                return None;
            }
        }
        None
    }
}

fn apply(states: &mut [State], a: usize, b: usize, ra: V3, rb: V3, imp: V3) {
    let sa = &mut states[a];
    sa.v -= imp * sa.inv_m;
    sa.w -= sa.inv_i.mul_v(ra.cross(imp));
    let sb = &mut states[b];
    sb.v += imp * sb.inv_m;
    sb.w += sb.inv_i.mul_v(rb.cross(imp));
}

fn apply_angular(states: &mut [State], a: usize, b: usize, imp: V3) {
    let sa = &mut states[a];
    sa.w -= sa.inv_i.mul_v(imp);
    let sb = &mut states[b];
    sb.w += sb.inv_i.mul_v(imp);
}

/// One pass over every contact: non-penetration (soft when `use_bias`), then friction, then rolling resistance.
fn solve_contacts(
    manifolds: &mut [Manifold],
    states: &mut [State],
    t: &Tuning,
    inv_h: f32,
    use_bias: bool,
    mut max_pen: Option<&mut (f32, u32, u32)>,
) {
    let h = 1.0 / inv_h;
    for m in manifolds.iter_mut() {
        let (a, b) = (m.a as usize, m.b as usize);
        let n = m.normal;
        let omega = std::f32::consts::TAU * m.hertz;
        let a1 = 2.0 * t.damping_ratio + h * omega;
        let a2 = h * omega * a1;
        let a3 = 1.0 / (1.0 + a2);
        let (bias_rate, mass_scale, impulse_scale) = (omega / a1, a2 * a3, a3);
        let mut total_normal = 0.0;
        // Where the anchors' bodies have got to since the step began (fixed during this pass for a contact's points).
        let (dpa, dqa, dpb, dqb) = (states[a].dp, states[a].dq, states[b].dp, states[b].dq);
        for p in &mut m.points[..m.count] {
            let pa = dpa + dqa.rotate(p.ra);
            let pb = dpb + dqb.rotate(p.rb);
            let s = (pb - pa).dot(n) + p.adjusted;
            if let Some(mp) = max_pen.as_deref_mut()
                && -s > mp.0
            {
                *mp = (-s, m.a, m.b);
            }
            let (bias, ms, is) = if s > 0.0 {
                // Speculative: may close exactly the gap in this substep.
                (s * inv_h, 1.0, 0.0)
            } else if use_bias {
                ((bias_rate * s).max(-t.push_max), mass_scale, impulse_scale)
            } else {
                (0.0, 1.0, 0.0)
            };
            let (sa, sb) = (&states[a], &states[b]);
            let dv = sb.v + sb.w.cross(p.rb) - sa.v - sa.w.cross(p.ra);
            let vn = dv.dot(n);
            let imp = -p.normal_mass * ms * (vn + bias) - is * p.normal_impulse;
            let new = (p.normal_impulse + imp).max(0.0);
            let d = new - p.normal_impulse;
            p.normal_impulse = new;
            p.max_normal = p.max_normal.max(d);
            total_normal += new;
            apply(states, a, b, p.ra, p.rb, n * d);
        }
        for p in &mut m.points[..m.count] {
            let (sa, sb) = (&states[a], &states[b]);
            let dv = sb.v + sb.w.cross(p.rb) - sa.v - sa.w.cross(p.ra);
            let l1 = -p.tangent_mass[0] * dv.dot(m.t1);
            let l2 = -p.tangent_mass[1] * dv.dot(m.t2);
            let old = p.tangent_impulse;
            let mut new = old + m.t1 * l1 + m.t2 * l2;
            let max = m.friction * p.normal_impulse;
            let len = new.length();
            if len > max {
                new = new * (max / len.max(1e-12));
            }
            p.tangent_impulse = new;
            apply(states, a, b, p.ra, p.rb, new - old);
        }
        if m.rolling > 0.0 {
            let dw = states[b].w - states[a].w;
            let old = m.rolling_impulse;
            let mut new = old - m.rolling_mass.mul_v(dw);
            let max = m.rolling * total_normal;
            let len = new.length();
            if len > max {
                new = new * (max / len.max(1e-12));
            }
            m.rolling_impulse = new;
            apply_angular(states, a, b, new - old);
        }
    }
}
