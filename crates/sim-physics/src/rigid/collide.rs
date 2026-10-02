//! Narrowphase: where two bodies touch, or will within `margin` (speculative contacts). Hull against hull is the
//! separating axis test of Gregorius (GDC 2013): the faces of both, and the edge pairs whose arcs cross on the Gauss
//! map (the only edge pairs that can separate). The face with the least overlap becomes the reference face and the
//! other hull's most opposed face is clipped against its sides; an edge pair gives one point. Face axes are preferred
//! unless an edge pair separates clearly more, so a box resting on a face never flickers to an edge contact.
//! Spheres meet hulls at the exact closest point.

use super::hull::Hull;
use super::math::{Quat, V3};

/// Up to four points, each a pair of surface points (on `a`, on `b`) and their separation along `normal`.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Contact {
    /// From `a` to `b`.
    pub normal: V3,
    pub points: [(V3, V3, f32); 4],
    pub count: usize,
}

impl Contact {
    fn push(&mut self, pa: V3, pb: V3, sep: f32) {
        if self.count < 4 {
            self.points[self.count] = (pa, pb, sep);
            self.count += 1;
        }
    }
}

/// A hull placed in the world.
pub(crate) struct Placed<'a> {
    pub hull: &'a Hull,
    pub pos: V3,
    pub rot: Quat,
    /// Its vertices and face normals in the world.
    pub verts: &'a [V3],
    pub normals: &'a [V3],
}

impl Placed<'_> {
    fn normal(&self, f: usize) -> V3 {
        self.normals[f]
    }

    fn offset(&self, f: usize) -> f32 {
        self.normal(f).dot(self.verts[self.hull.loops[self.hull.faces[f].start as usize] as usize])
    }
}

/// A hull's vertices and face normals in the world (into `out`), turned once per pair rather than per test.
pub(crate) fn place(hull: &Hull, pos: V3, rot: Quat, out: &mut Placement) {
    out.verts.clear();
    out.verts.extend(hull.verts.iter().map(|&v| pos + rot.rotate(v)));
    out.normals.clear();
    out.normals.extend(hull.faces.iter().map(|f| rot.rotate(f.normal)));
}

/// Reused buffers for a placed hull.
#[derive(Clone, Default)]
pub(crate) struct Placement {
    pub verts: Vec<V3>,
    pub normals: Vec<V3>,
}

pub(crate) fn sphere_sphere(ca: V3, ra: f32, cb: V3, rb: f32, margin: f32) -> Option<Contact> {
    let d = cb - ca;
    let dist = d.length();
    let sep = dist - ra - rb;
    if sep > margin {
        return None;
    }
    let n = if dist > 1e-6 { d * (1.0 / dist) } else { V3::Y };
    let mut c = Contact { normal: n, ..Contact::default() };
    c.push(ca + n * ra, cb - n * rb, sep);
    Some(c)
}

/// A hull `a` against a sphere `b`.
pub(crate) fn hull_sphere(a: &Placed, cb: V3, rb: f32, margin: f32) -> Option<Contact> {
    let h = a.hull;
    let local = a.rot.unrotate(cb - a.pos);
    let (mut best_f, mut best_s) = (0, f32::MIN);
    for (i, f) in h.faces.iter().enumerate() {
        let s = f.normal.dot(local) - f.offset;
        if s > best_s {
            best_s = s;
            best_f = i;
        }
    }
    if best_s - rb > margin {
        return None;
    }
    let (q, n, dist) = if best_s <= 0.0 {
        // Inside: out through the nearest face.
        let n = h.faces[best_f].normal;
        (local - n * best_s, n, best_s)
    } else {
        // Outside: the closest point lies on a face the centre is in front of.
        let mut best: Option<(V3, f32)> = None;
        for (i, f) in h.faces.iter().enumerate() {
            let s = f.normal.dot(local) - f.offset;
            if s <= 0.0 {
                continue;
            }
            let q = closest_on_face(h, i, local - f.normal * s);
            let d = (local - q).length();
            if best.is_none_or(|(_, bd)| d < bd) {
                best = Some((q, d));
            }
        }
        let (q, d) = best?;
        let n = if d > 1e-6 { (local - q) * (1.0 / d) } else { h.faces[best_f].normal };
        (q, n, d)
    };
    if dist - rb > margin {
        return None;
    }
    let nw = a.rot.rotate(n);
    let qa = a.pos + a.rot.rotate(q);
    let mut c = Contact { normal: nw, ..Contact::default() };
    c.push(qa, cb - nw * rb, dist - rb);
    Some(c)
}

/// The point of face `f` nearest `p` (a point on the face's plane, local frame).
fn closest_on_face(h: &Hull, f: usize, p: V3) -> V3 {
    let n = h.faces[f].normal;
    let vs: Vec<u16> = h.face_verts(f).collect();
    let mut inside = true;
    let mut best = (p, f32::MAX);
    for i in 0..vs.len() {
        let (a, b) = (h.verts[vs[i] as usize], h.verts[vs[(i + 1) % vs.len()] as usize]);
        if (b - a).cross(n).dot(p - a) > 0.0 {
            inside = false;
        }
        let q = closest_on_segment(a, b, p);
        let d = (q - p).length_sq();
        if d < best.1 {
            best = (q, d);
        }
    }
    if inside { p } else { best.0 }
}

fn closest_on_segment(a: V3, b: V3, p: V3) -> V3 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-12)).clamp(0.0, 1.0);
    a + ab * t
}

/// Closest points of segments `p1 q1` and `p2 q2` (Ericson, Real-Time Collision Detection §5.1.9).
fn closest_segments(p1: V3, q1: V3, p2: V3, q2: V3) -> (V3, V3) {
    let (d1, d2, r) = (q1 - p1, q2 - p2, p1 - p2);
    let (a, e, f) = (d1.length_sq(), d2.length_sq(), d2.dot(r));
    let c = d1.dot(r);
    let b = d1.dot(d2);
    let denom = a * e - b * b;
    let mut s = if denom > 1e-12 { ((b * f - c * e) / denom).clamp(0.0, 1.0) } else { 0.0 };
    let mut t = (b * s + f) / e.max(1e-12);
    if t < 0.0 {
        t = 0.0;
        s = (-c / a.max(1e-12)).clamp(0.0, 1.0);
    } else if t > 1.0 {
        t = 1.0;
        s = ((b - c) / a.max(1e-12)).clamp(0.0, 1.0);
    }
    (p1 + d1 * s, p2 + d2 * t)
}

/// The most separating face of `a` against `b`: (separation, face).
fn face_query(a: &Placed, b: &Placed) -> (f32, usize) {
    let mut best = (f32::MIN, 0);
    for f in 0..a.hull.faces.len() {
        let n = a.normal(f);
        let d = a.offset(f);
        let s = b.verts.iter().map(|v| n.dot(*v)).fold(f32::MAX, f32::min) - d;
        if s > best.0 {
            best = (s, f);
        }
    }
    best
}

/// Whether two edges' arcs on the Gauss map cross (Gregorius): only then can their cross product separate.
fn minkowski_face(a: V3, b: V3, c: V3, d: V3) -> bool {
    let bxa = b.cross(a);
    let dxc = d.cross(c);
    let cba = c.dot(bxa);
    let dba = d.dot(bxa);
    let adc = a.dot(dxc);
    let bdc = b.dot(dxc);
    cba * dba < 0.0 && adc * bdc < 0.0 && cba * bdc > 0.0
}

/// The most separating edge pair: (separation, edge of a, edge of b, axis from a to b).
fn edge_query(a: &Placed, b: &Placed) -> (f32, usize, usize, V3) {
    let mut best = (f32::MIN, 0, 0, V3::ZERO);
    for (i, ea) in a.hull.edges.iter().enumerate() {
        let (pa, qa) = (a.verts[ea.a as usize], a.verts[ea.b as usize]);
        let (u1, u2) = (a.normal(ea.left as usize), a.normal(ea.right as usize));
        for (j, eb) in b.hull.edges.iter().enumerate() {
            let (v1, v2) = (b.normal(eb.left as usize), b.normal(eb.right as usize));
            if !minkowski_face(u1, u2, -v1, -v2) {
                continue;
            }
            let (pb, qb) = (b.verts[eb.a as usize], b.verts[eb.b as usize]);
            let (e1, e2) = (qa - pa, qb - pb);
            let x = e1.cross(e2);
            let l = x.length();
            // Parallel edges: their face axes already cover them.
            if l < 0.005 * (e1.length() * e2.length()) {
                continue;
            }
            let mut n = x * (1.0 / l);
            if n.dot(pa - a.pos) < 0.0 {
                n = -n;
            }
            let s = n.dot(pb - pa);
            if s > best.0 {
                best = (s, i, j, n);
            }
        }
    }
    best
}

/// Hull against hull.
pub(crate) fn hull_hull(a: &Placed, b: &Placed, margin: f32, slop: f32, scratch: &mut Clip) -> Option<Contact> {
    let fa = face_query(a, b);
    if fa.0 > margin {
        return None;
    }
    let fb = face_query(b, a);
    if fb.0 > margin {
        return None;
    }
    let ee = edge_query(a, b);
    if ee.0 > margin {
        return None;
    }
    // Prefer faces, and a's face over b's, unless the other separates clearly more (Gregorius' tolerances).
    const REL: f32 = 0.95;
    let abs = 0.5 * slop;
    let face_b = fb.0 > REL * fa.0 + abs;
    let face_best = fa.0.max(fb.0);
    if ee.1 < a.hull.edges.len() && ee.0 > REL * face_best + abs && ee.3 != V3::ZERO {
        let (ea, eb) = (a.hull.edges[ee.1], b.hull.edges[ee.2]);
        let (ca, cb) = closest_segments(a.verts[ea.a as usize], a.verts[ea.b as usize], b.verts[eb.a as usize], b.verts[eb.b as usize]);
        let mut c = Contact { normal: ee.3, ..Contact::default() };
        c.push(ca, cb, (cb - ca).dot(ee.3));
        return Some(c);
    }
    let c = if face_b { face_contact(b, fb.1, a, margin, scratch, true) } else { face_contact(a, fa.1, b, margin, scratch, false) };
    (c.count > 0).then_some(c)
}

/// Scratch polygons for clipping, reused between calls.
#[derive(Clone, Default)]
pub(crate) struct Clip {
    a: Vec<V3>,
    b: Vec<V3>,
}

/// Clips the face of `inc` most opposed to reference face `f` of `r` against `f`'s sides. `flip`: `r` is body b.
fn face_contact(r: &Placed, f: usize, inc: &Placed, margin: f32, clip: &mut Clip, flip: bool) -> Contact {
    let n = r.normal(f);
    let d = r.offset(f);
    let mut fi = 0;
    let mut least = f32::MAX;
    for k in 0..inc.hull.faces.len() {
        let dn = inc.normal(k).dot(n);
        if dn < least {
            least = dn;
            fi = k;
        }
    }
    clip.a.clear();
    clip.a.extend(inc.hull.face_verts(fi).map(|v| inc.verts[v as usize]));
    let rv: Vec<V3> = r.hull.face_verts(f).map(|v| r.verts[v as usize]).collect();
    for i in 0..rv.len() {
        let (p, q) = (rv[i], rv[(i + 1) % rv.len()]);
        let side = (q - p).cross(n).normalized();
        let off = side.dot(p);
        clip.b.clear();
        let poly = &clip.a;
        for k in 0..poly.len() {
            let (s, e) = (poly[k], poly[(k + 1) % poly.len()]);
            let (ds, de) = (side.dot(s) - off, side.dot(e) - off);
            if ds <= 0.0 {
                clip.b.push(s);
            }
            if (ds <= 0.0) != (de <= 0.0) {
                clip.b.push(s + (e - s) * (ds / (ds - de)));
            }
        }
        std::mem::swap(&mut clip.a, &mut clip.b);
        if clip.a.is_empty() {
            break;
        }
    }
    // Points below the reference plane (within the margin), at most four kept.
    let mut pts: Vec<(V3, f32)> = clip.a.iter().map(|&p| (p, n.dot(p) - d)).filter(|&(_, s)| s <= margin).collect();
    reduce(&mut pts, n);
    let mut c = Contact { normal: if flip { -n } else { n }, ..Contact::default() };
    for (p, s) in pts {
        let on_ref = p - n * s;
        if flip {
            c.push(p, on_ref, s);
        } else {
            c.push(on_ref, p, s);
        }
    }
    c
}

/// Keeps at most four points spanning the largest area: the deepest, the farthest from it, the one making the
/// largest triangle, and the one farthest outside that triangle.
fn reduce(pts: &mut Vec<(V3, f32)>, n: V3) {
    if pts.len() <= 4 {
        return;
    }
    let mut keep: Vec<(V3, f32)> = Vec::with_capacity(4);
    let deepest = (0..pts.len()).min_by(|&i, &j| pts[i].1.total_cmp(&pts[j].1)).unwrap_or(0);
    keep.push(pts[deepest]);
    let p0 = keep[0].0;
    let far = (0..pts.len()).max_by(|&i, &j| (pts[i].0 - p0).length_sq().total_cmp(&(pts[j].0 - p0).length_sq())).unwrap_or(0);
    keep.push(pts[far]);
    let p1 = keep[1].0;
    let area = |p: V3| (p1 - p0).cross(p - p0).dot(n);
    let third = (0..pts.len()).max_by(|&i, &j| area(pts[i].0).abs().total_cmp(&area(pts[j].0).abs())).unwrap_or(0);
    keep.push(pts[third]);
    let p2 = keep[2].0;
    // Orient the triangle counter-clockwise about n, then take the point most outside any of its edges.
    let (a, b, c) = if area(p2) >= 0.0 { (p0, p1, p2) } else { (p0, p2, p1) };
    let outside = |p: V3| {
        let e = |u: V3, v: V3| (v - u).cross(p - u).dot(n);
        e(a, b).min(e(b, c)).min(e(c, a))
    };
    let fourth = (0..pts.len()).min_by(|&i, &j| outside(pts[i].0).total_cmp(&outside(pts[j].0))).unwrap_or(0);
    if outside(pts[fourth].0) < 0.0 {
        keep.push(pts[fourth]);
    }
    *pts = keep;
}
