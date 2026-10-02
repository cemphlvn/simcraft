//! Convex hulls for boxes and prisms: vertices, faces (outward normal, plane offset, a counter-clockwise loop of
//! vertices seen from outside) and edges (two vertices and the two faces that meet there). Built once per body in
//! its local frame; the narrowphase turns them into the world as it needs them.

use super::Shape;
use super::math::V3;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Face {
    pub normal: V3,
    /// `dot(normal, p) = offset` on the face's plane.
    pub offset: f32,
    /// Its vertices are `loops[start..start + len]`.
    pub start: u16,
    pub len: u16,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Edge {
    pub a: u16,
    pub b: u16,
    pub left: u16,
    pub right: u16,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Hull {
    pub verts: Vec<V3>,
    pub faces: Vec<Face>,
    pub loops: Vec<u16>,
    pub edges: Vec<Edge>,
}

impl Hull {
    /// The hull of a box or a prism; `None` for a sphere.
    pub fn of(shape: &Shape) -> Option<Hull> {
        match *shape {
            Shape::Box { half } => Some(Hull::box_(half)),
            Shape::Prism { radius, half_height, sides } => Some(Hull::prism(radius, half_height, sides.max(3))),
            Shape::Sphere { .. } => None,
        }
    }

    pub fn face_verts(&self, f: usize) -> impl Iterator<Item = u16> + '_ {
        let f = self.faces[f];
        self.loops[f.start as usize..(f.start + f.len) as usize].iter().copied()
    }

    fn box_(h: V3) -> Hull {
        let verts: Vec<V3> = (0..8)
            .map(|i| {
                let s = |bit: u32| if i & bit != 0 { 1.0 } else { -1.0 };
                V3::new(h.x * s(1), h.y * s(2), h.z * s(4))
            })
            .collect();
        // Each face's loop is counter-clockwise seen from outside.
        let quads: [[u16; 4]; 6] = [
            [1, 3, 7, 5], // +x
            [0, 4, 6, 2], // -x
            [2, 6, 7, 3], // +y
            [0, 1, 5, 4], // -y
            [4, 5, 7, 6], // +z
            [0, 2, 3, 1], // -z
        ];
        Hull::from_loops(verts, &quads.iter().map(|q| q.to_vec()).collect::<Vec<_>>())
    }

    fn prism(r: f32, hh: f32, n: u8) -> Hull {
        let n = u16::from(n);
        let mut verts = Vec::with_capacity(2 * n as usize);
        for k in 0..n {
            let a = std::f32::consts::TAU * f32::from(k) / f32::from(n);
            // Round the y axis, counter-clockwise seen from +y: x = cos, z = −sin.
            let (x, z) = (r * a.cos(), -r * a.sin());
            verts.push(V3::new(x, -hh, z));
            verts.push(V3::new(x, hh, z));
        }
        let mut loops: Vec<Vec<u16>> = Vec::new();
        for k in 0..n {
            let j = (k + 1) % n;
            // Side: bottom k, bottom j, top j, top k (outward, counter-clockwise).
            loops.push(vec![2 * k, 2 * j, 2 * j + 1, 2 * k + 1]);
        }
        loops.push((0..n).map(|k| 2 * k + 1).collect()); // top, counter-clockwise from +y
        loops.push((0..n).rev().map(|k| 2 * k).collect()); // bottom, reversed
        Hull::from_loops(verts, &loops)
    }

    fn from_loops(verts: Vec<V3>, loops: &[Vec<u16>]) -> Hull {
        let mut hull = Hull { verts, ..Hull::default() };
        for l in loops {
            let p = |i: usize| hull.verts[l[i] as usize];
            // Newell's normal: robust for any planar loop.
            let mut n = V3::ZERO;
            for i in 0..l.len() {
                let (a, b) = (p(i), p((i + 1) % l.len()));
                n += V3::new((a.y - b.y) * (a.z + b.z), (a.z - b.z) * (a.x + b.x), (a.x - b.x) * (a.y + b.y));
            }
            let n = n.normalized();
            hull.faces.push(Face { normal: n, offset: n.dot(p(0)), start: hull.loops.len() as u16, len: l.len() as u16 });
            hull.loops.extend_from_slice(l);
        }
        // Edges: every directed edge a → b of a face meets its twin b → a on the neighbouring face.
        for (fi, l) in loops.iter().enumerate() {
            for i in 0..l.len() {
                let (a, b) = (l[i], l[(i + 1) % l.len()]);
                if a < b {
                    let right =
                        loops.iter().position(|m| (0..m.len()).any(|k| m[k] == b && m[(k + 1) % m.len()] == a)).expect("a closed hull")
                            as u16;
                    hull.edges.push(Edge { a, b, left: fi as u16, right });
                }
            }
        }
        hull
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hulls_are_closed_and_their_faces_face_out() {
        for shape in [Shape::Box { half: V3::new(0.5, 0.2, 0.3) }, Shape::Prism { radius: 0.3, half_height: 0.4, sides: 8 }] {
            let h = Hull::of(&shape).unwrap();
            // Euler: V − E + F = 2.
            assert_eq!(h.verts.len() as i32 - h.edges.len() as i32 + h.faces.len() as i32, 2);
            for (i, f) in h.faces.iter().enumerate() {
                assert!(f.offset > 0.0, "face {i} faces in");
                for v in &h.verts {
                    assert!(f.normal.dot(*v) <= f.offset + 1e-5, "a vertex lies outside face {i}");
                }
            }
        }
    }
}
