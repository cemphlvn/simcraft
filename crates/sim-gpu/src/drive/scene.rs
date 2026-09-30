//! The track as a place: the racing surface from the TrackDef (its banking, its width), and around it what a
//! NASCAR oval has on race day: a flatter apron with its lines, a SAFER barrier on a concrete wall, the catch fence,
//! pit road behind its wall, grandstands along the frontstretch, a mowed infield, the land beyond in haze.
//! Built once (it does not change) and kept on the GPU.
//!
//! Render space: X = the track's x (m), Y = up, Z = the track's y (m). The track's plane is the plane of its
//! centreline; the banking lifts the outside of a turn and drops the inside (`sim_physics::Track::pose`).

use sim_physics::{Angle, Fx, TURN, Track};

use super::Look;
use super::geom::{ASPHALT, Builder, CONCRETE, CROWD, FENCE, GRASS, hash01, rgb, rgba, shade};
use crate::gpu::Mesh;
use crate::math::V3;
use crate::stage::{WHITE, Wrap};

pub fn fx(v: f32) -> Fx {
    Fx((v as f64 * 65536.0).round() as i64)
}

pub fn fl(v: Fx) -> f32 {
    (v.0 as f64 / 65536.0) as f32
}

pub fn rad(a: Angle) -> f32 {
    (a.signed().0 as f64 * std::f64::consts::TAU / TURN as f64) as f32
}

/// The centreline at `s`: x, y (m), heading and banking (radians).
#[derive(Clone, Copy, Debug)]
pub struct Centre {
    pub s: f32,
    pub x: f32,
    pub y: f32,
    pub heading: f32,
    pub bank: f32,
}

pub fn centre(track: &Track, s: f32) -> Centre {
    let p = track.pose(fx(s), Fx::ZERO);
    Centre { s, x: fl(p.x), y: fl(p.y), heading: rad(p.heading), bank: rad(p.bank) }
}

/// The ground around the track, as numbers: how high it is at any (s, offset), so cars sit on it and the scene is
/// built from it. Offsets are positive to the left of the direction of travel (the infield of a counterclockwise
/// oval).
#[derive(Clone, Debug)]
pub struct Ground {
    /// Half the racing surface's width.
    pub half: f32,
    pub apron: f32,
    /// tan of the apron's steepest slope.
    pub apron_tan: f32,
    /// The infield's level (below the lowest apron edge).
    pub infield: f32,
    pub length: f32,
    pub pit: Option<(f32, f32)>,
    pub wall: f32,
}

/// How far the grass falls from the apron's edge to the infield (m across).
const SLOPE: f32 = 26.0;
/// Pit road: from the apron's edge, a grass strip, the pit wall, the road, to its far edge.
const PIT_WALL: (f32, f32) = (1.0, 1.6);
const PIT_END: f32 = 14.0;

impl Ground {
    /// Distance along the track from the start line, in (-length/2, length/2].
    pub fn rel(&self, s: f32) -> f32 {
        let s = s.rem_euclid(self.length);
        if s > self.length / 2.0 { s - self.length } else { s }
    }

    /// How much of pit road is here: 1 inside its range, 0 outside, easing over 30 m at either end.
    pub fn pit_k(&self, s: f32) -> f32 {
        let Some((from, to)) = self.pit else { return 0.0 };
        let r = self.rel(s);
        ((r - from) / 30.0).min((to - r) / 30.0).clamp(0.0, 1.0)
    }

    /// Height of the ground at `offset` across the centreline `c`.
    pub fn height(&self, c: &Centre, o: f32) -> f32 {
        let tb = c.bank.tan();
        if o <= self.half {
            return -o.max(-self.half) * tb;
        }
        let edge = -self.half * tb;
        let at = self.apron_tan.min(tb);
        if o <= self.half + self.apron {
            return edge - (o - self.half) * at;
        }
        let a = self.half + self.apron;
        let za = edge - self.apron * at;
        let d = o - a;
        let smooth = |t: f32| {
            let t = t.clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        };
        let grass = za + (self.infield - za) * smooth(d / SLOPE);
        let k = self.pit_k(c.s);
        if k <= 0.0 {
            return grass;
        }
        let pit_level = za - PIT_END * 0.012;
        let pit =
            if d <= PIT_END { za - d * 0.012 } else { pit_level + (self.infield - pit_level) * smooth((d - PIT_END) / (SLOPE - PIT_END)) };
        grass + (pit - grass) * k
    }

    /// A point on (or `lift` above) the ground, in render space.
    pub fn point(&self, c: &Centre, o: f32, lift: f32) -> V3 {
        let (sn, cs) = c.heading.sin_cos();
        V3(c.x - o * sn, self.height(c, o) + lift, c.y + o * cs)
    }
}

/// The ground's shape from the track and the look.
pub fn ground(track: &Track, look: &Look) -> Ground {
    let half = fl(track.width) / 2.0;
    let length = fl(track.length);
    let mut g = Ground {
        half,
        apron: look.apron_width,
        apron_tan: look.apron_bank.to_radians().tan(),
        infield: 0.0,
        length,
        pit: look.pit_road,
        wall: look.wall_height,
    };
    // The infield lies a little below the lowest point of the apron all the way round.
    let n = (length / 5.0) as usize;
    let lowest = (0..n).map(|i| g.height(&centre(track, i as f32 * length / n as f32), half + look.apron_width)).fold(0.0, f32::min);
    g.infield = lowest - 0.8;
    g
}

/// What a strip of the cross-section is.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Band {
    Road,
    Seam,
    White,
    Yellow,
    Apron,
    Grass,
    /// Pit road's surface (concrete where there is pit road, grass elsewhere).
    Pit,
    PitWhite,
    PitYellow,
    /// Pit stalls (their transverse lines come from the samples along the track).
    Stall,
}

/// The cross-section: offsets (outside → inside) and the band from each to the next.
fn section(g: &Ground) -> Vec<(f32, Band)> {
    let (hw, a) = (g.half, g.half + g.apron);
    let mut e: Vec<(f32, Band)> = Vec::new();
    let mut o = -hw;
    while o < hw - 0.01 {
        e.push((o, Band::Road));
        o += 1.0;
    }
    // Paving joints where the lanes of the paver met.
    for seam in [-5.4, -1.8, 1.8, 5.4] {
        if seam > -hw + 0.5 && seam < hw - 0.5 {
            e.push((seam - 0.04, Band::Seam));
            e.push((seam + 0.04, Band::Road));
        }
    }
    e.extend([(hw, Band::White), (hw + 0.2, Band::Apron), (hw + 0.35, Band::Yellow), (hw + 0.47, Band::Apron)]);
    e.extend([(hw + 0.6, Band::Yellow), (hw + 0.72, Band::Apron)]);
    let mut o = hw + 2.0;
    while o < a - 0.5 {
        e.push((o, Band::Apron));
        o += 1.5;
    }
    e.extend([(a, Band::Grass), (a + PIT_WALL.0, Band::Grass), (a + PIT_WALL.1, Band::Pit), (a + 1.9, Band::PitWhite)]);
    e.extend([(a + 2.05, Band::Pit), (a + 4.8, Band::Pit), (a + 7.6, Band::PitYellow), (a + 7.75, Band::Stall)]);
    e.extend([(a + 10.7, Band::Stall), (a + 13.6, Band::Grass), (a + PIT_END, Band::Grass), (a + 18.0, Band::Grass)]);
    e.extend([(a + 22.0, Band::Grass), (a + SLOPE, Band::Grass)]);
    e.sort_by(|x, y| x.0.total_cmp(&y.0));
    e
}

/// Where along the track the grid rows are: every ~2 m, plus the edges of painted marks across the track (the
/// start/finish line, pit stalls), so a mark is its own strip of the mesh instead of a decal that could flicker.
fn samples(g: &Ground, look: &Look) -> (Vec<f32>, Vec<(f32, f32)>) {
    let n = (g.length / 2.0).ceil() as usize;
    let mut s: Vec<f32> = (0..n).map(|i| i as f32 * g.length / n as f32).collect();
    let mut marks = vec![(0.0, look.line_width)];
    if let Some((from, to)) = g.pit {
        let mut r = from + 20.0;
        while r < to - 20.0 {
            marks.push((r.rem_euclid(g.length), 0.12));
            r += 11.0;
        }
    }
    for &(m, w) in &marks {
        s.retain(|v| (v - m).abs() > 0.08 && (v - (m + w)).abs() > 0.08);
        s.push(m);
        s.push((m + w).rem_euclid(g.length));
    }
    s.sort_by(f32::total_cmp);
    s.dedup_by(|a, b| (*a - *b).abs() < 1e-4);
    (s, marks)
}

/// The rows of the road grid: at each sample along the track, the cross-section's points (render space).
pub struct Rows {
    pub s: Vec<f32>,
    pub pts: Vec<Vec<V3>>,
    pub offsets: Vec<f32>,
}

pub fn rows(track: &Track, g: &Ground, look: &Look) -> Rows {
    let (s, _) = samples(g, look);
    let sec = section(g);
    let offsets: Vec<f32> = sec.iter().map(|e| e.0).collect();
    let pts = s.iter().map(|&si| {
        let c = centre(track, si);
        offsets.iter().map(|&o| g.point(&c, o, 0.0)).collect()
    });
    Rows { pts: pts.collect(), offsets, s }
}

/// The static scene: every mesh, in draw order (the see-through fence last).
pub struct Scene {
    pub ground: Ground,
    pub meshes: Vec<Mesh>,
    /// The sun (render space, towards it).
    pub sun: V3,
    pub triangles: usize,
}

pub fn sun_dir(look: &Look) -> V3 {
    let (az, el) = (look.sun.0.to_radians(), look.sun.1.to_radians());
    V3(el.cos() * az.cos(), el.sin(), el.cos() * az.sin())
}

/// Where the groove runs (offset, m): up by the wall on the straights, down toward the bottom in the turns.
pub fn groove_centre(_look: &Look, g: &Ground, bank: f32) -> f32 {
    let t = ((bank.to_degrees() - 6.0) / 14.0).clamp(0.0, 1.0);
    -g.half * 0.28 + t * g.half * 0.5
}

/// The racing groove: darker where the cars run, high by the wall on the straights and low in the turns.
pub fn groove(look: &Look, g: &Ground, bank: f32, o: f32) -> f32 {
    let t = ((bank.to_degrees() - 6.0) / 14.0).clamp(0.0, 1.0);
    let centre = groove_centre(look, g, bank);
    let d = (o - centre) / 2.6;
    let rubber = look.groove * (-0.5 * d * d).exp();
    // Marbles: bits of rubber swept up high in the turns, a lighter grey near the wall.
    let marbles = if o < centre - 4.5 && t > 0.5 { 0.06 } else { 0.0 };
    1.0 - rubber + marbles
}

pub fn build(track: &Track, look: &Look) -> Scene {
    let g = ground(track, look);
    let sun = sun_dir(look);
    let r = rows(track, &g, look);
    let sec = section(&g);
    let (_, marks) = samples(&g, look);
    let n = r.s.len();
    let m = sec.len();
    let centres: Vec<Centre> = r.s.iter().map(|&s| centre(track, s)).collect();
    // Vertex normals from the grid (across × along), pointing up.
    let normal = |i: usize, j: usize| {
        let a = r.pts[(i + 1) % n][j] - r.pts[(i + n - 1) % n][j];
        let b = r.pts[i][(j + 1).min(m - 1)] - r.pts[i][j.saturating_sub(1)];
        let nn = a.cross(b).norm();
        if nn.1 < 0.0 { nn.scale(-1.0) } else { nn }
    };
    let mut asphalt = Builder::new(ASPHALT, Wrap::Repeat, sun);
    let mut concrete = Builder::new(CONCRETE, Wrap::Repeat, sun);
    let mut grass = Builder::new(GRASS, Wrap::Repeat, sun);
    let marked = |s: f32| marks.iter().any(|&(at, w)| at == 0.0 && s >= at && s <= at + w);
    let stall = |s: f32| marks.iter().skip(1).any(|&(at, w)| s >= at && s <= at + w);
    for i in 0..n {
        let i1 = (i + 1) % n;
        let s_mid = if i1 == 0 { (r.s[i] + g.length) / 2.0 } else { (r.s[i] + r.s[i1]) / 2.0 };
        let pit = g.pit_k(s_mid) > 0.5;
        for j in 0..m - 1 {
            let (o0, o1, band) = (sec[j].0, sec[j + 1].0, sec[j].1);
            let p = [r.pts[i][j], r.pts[i1][j], r.pts[i1][j + 1], r.pts[i][j + 1]];
            let ns = [normal(i, j), normal(i1, j), normal(i1, j + 1), normal(i, j + 1)];
            let os = [o0, o0, o1, o1];
            let banks = [centres[i].bank, centres[i1].bank, centres[i1].bank, centres[i].bank];
            let ss = [r.s[i], if i1 == 0 { g.length } else { r.s[i1] }, if i1 == 0 { g.length } else { r.s[i1] }, r.s[i]];
            let patch = 0.94 + 0.1 * hash01((s_mid / 9.0) as i64, (o0 / 3.0).floor() as i64, 5);
            // Per corner: base colour × sunlight (× the groove on the racing surface).
            let corner = |k: usize, base: [f32; 3], groove_on: bool| {
                let gk = if groove_on { groove(look, &g, banks[k], os[k]) } else { 1.0 };
                rgba(base, shade(ns[k], sun) * gk, 1.0)
            };
            let road_uv: [[f32; 2]; 4] = std::array::from_fn(|k| [os[k] / 3.0, ss[k] / 3.0]);
            let world_uv: [[f32; 2]; 4] = std::array::from_fn(|k| [p[k].0 / 7.0, p[k].2 / 7.0]);
            let on_line = marked(s_mid) && o0 < g.half + g.apron && o0 >= -g.half;
            let asp = rgb(look.asphalt).map(|v| v * patch);
            let (target, base, groove_on): (&mut Builder, [f32; 3], bool) = match band {
                _ if on_line => (&mut asphalt, rgb(look.white), false),
                Band::Road => (&mut asphalt, asp, true),
                Band::Seam => (&mut asphalt, asp.map(|v| v * 0.78), true),
                Band::White => (&mut asphalt, rgb(look.white), false),
                Band::Yellow => (&mut asphalt, rgb(look.yellow), false),
                Band::Apron => (&mut asphalt, rgb(look.apron).map(|v| v * patch), false),
                Band::Pit if pit => (&mut concrete, rgb(look.concrete), false),
                Band::PitWhite if pit => (&mut concrete, rgb(look.white), false),
                Band::PitYellow if pit => (&mut concrete, rgb(look.yellow), false),
                Band::Stall if pit && stall(s_mid) => (&mut concrete, rgb(look.white), false),
                Band::Stall if pit => (&mut concrete, rgb(look.concrete).map(|v| v * 0.93), false),
                _ => (&mut grass, rgb(look.grass), false),
            };
            let uv = if target.image == GRASS { world_uv } else { road_uv };
            let c: [[f32; 4]; 4] = std::array::from_fn(|k| corner(k, base, groove_on));
            target.quad_c(p, uv, c);
        }
    }
    let mut white = Builder::new(WHITE, Wrap::Clamp, sun);
    let mut crowd = Builder::new(CROWD, Wrap::Repeat, sun);
    let mut fence = Builder::new(FENCE, Wrap::Repeat, sun);
    walls(&g, look, &centres, &r.s, &mut white, &mut concrete, &mut fence);
    pit_wall(&g, look, &centres, &r.s, &mut concrete);
    outside(&g, look, &centres, &mut grass, &mut white);
    infield(track, &g, look, &r, &mut grass, &mut white);
    stands(track, &g, look, &mut crowd, &mut white);
    let meshes: Vec<Mesh> = vec![asphalt.mesh(), concrete.mesh(), grass.mesh(), white.mesh(), crowd.mesh(), fence.mesh()];
    let triangles = meshes.iter().map(|m| m.verts.len() / 3).sum();
    Scene { ground: g, meshes, sun, triangles }
}

/// Outward (to the right of travel, horizontal) at a centre.
fn outward(c: &Centre) -> V3 {
    let (sn, cs) = c.heading.sin_cos();
    V3(sn, 0.0, -cs)
}

/// The SAFER barrier on the concrete wall, and the catch fence on top, around the whole outside.
fn walls(g: &Ground, look: &Look, cs: &[Centre], s: &[f32], white: &mut Builder, concrete: &mut Builder, fence: &mut Builder) {
    let n = cs.len();
    let safer = rgb(look.safer);
    let wall = rgb(look.concrete);
    let dark = [0.16, 0.16, 0.17];
    let fh = look.fence_height;
    let at = |c: &Centre, back: f32, up: f32| {
        let base = g.point(c, -g.half, 0.0);
        base + outward(c).scale(back) + V3(0.0, up, 0.0)
    };
    // The barrier's face: its steel tubes as bands (dark rubber scuffs low down), then the concrete above.
    let bands: [(f32, f32, [f32; 3]); 8] = [
        (0.0, 0.1, dark),
        (0.1, 0.3, safer),
        (0.3, 0.36, safer.map(|v| v * 0.7)),
        (0.36, 0.56, safer),
        (0.56, 0.62, safer.map(|v| v * 0.7)),
        (0.62, 0.82, safer),
        (0.82, 0.88, safer.map(|v| v * 0.7)),
        (0.88, 1.0, safer),
    ];
    for i in 0..n {
        let i1 = (i + 1) % n;
        let (a, b) = (&cs[i], &cs[i1]);
        let (sa, sb) = (s[i], if i1 == 0 { g.length } else { s[i1] });
        let scuff = 0.85 + 0.15 * hash01((sa / 6.0) as i64, 0, 7);
        for (lo, hi, col) in bands {
            let col = if lo < 0.5 { col.map(|v| v * scuff) } else { col };
            white.quad([at(a, 0.0, lo), at(b, 0.0, lo), at(b, 0.0, hi), at(a, 0.0, hi)], [[0.0; 2]; 4], col, 1.0);
        }
        white.quad([at(a, 0.0, 1.0), at(b, 0.0, 1.0), at(b, 0.5, 1.0), at(a, 0.5, 1.0)], [[0.0; 2]; 4], safer.map(|v| v * 0.9), 1.0);
        let top = g.wall;
        let uv = [[sa / 4.0, 0.0], [sb / 4.0, 0.0], [sb / 4.0, 0.3], [sa / 4.0, 0.3]];
        concrete.quad([at(a, 0.5, 1.0), at(b, 0.5, 1.0), at(b, 0.5, top), at(a, 0.5, top)], uv, wall, 1.0);
        concrete.quad([at(a, 0.5, top), at(b, 0.5, top), at(b, 0.8, top), at(a, 0.8, top)], uv, wall, 1.0);
        let (ga, gb) = (g.point(a, -g.half - 0.8, 0.0).1, g.point(b, -g.half - 0.8, 0.0).1);
        let (ba, bb) = (at(a, 0.8, top), at(b, 0.8, top));
        concrete.quad(
            [ba, bb, V3(bb.0, outer_h(g, b, 0.8) - 0.2, bb.2), V3(ba.0, outer_h(g, a, 0.8) - 0.2, ba.2)],
            uv,
            wall.map(|v| v * 0.8),
            1.0,
        );
        let _ = (ga, gb);
        // The catch fence: straight up, then leaning in over the track.
        let (f0a, f0b) = (at(a, 0.65, top), at(b, 0.65, top));
        let (f1a, f1b) = (at(a, 0.65, top + fh - 1.3), at(b, 0.65, top + fh - 1.3));
        let (f2a, f2b) = (at(a, -0.4, top + fh), at(b, -0.4, top + fh));
        let v = |h: f32| h / 1.2;
        let grey = [0.9, 0.9, 0.9];
        fence.quad(
            [f0a, f0b, f1b, f1a],
            [[sa / 1.2, v(0.0)], [sb / 1.2, v(0.0)], [sb / 1.2, v(fh - 1.3)], [sa / 1.2, v(fh - 1.3)]],
            grey,
            1.0,
        );
        fence.quad(
            [f1a, f1b, f2b, f2a],
            [[sa / 1.2, v(fh - 1.3)], [sb / 1.2, v(fh - 1.3)], [sb / 1.2, v(fh)], [sa / 1.2, v(fh)]],
            grey,
            1.0,
        );
    }
    // Fence posts every 4 m along the wall.
    let post = rgb((110, 112, 115));
    let id = super::geom::Frame { o: V3(0.0, 0.0, 0.0), r: V3(1.0, 0.0, 0.0), u: V3(0.0, 1.0, 0.0), f: V3(0.0, 0.0, 1.0) };
    let mut k = 0;
    for i in 0..n {
        if s[i] < k as f32 * 4.0 {
            continue;
        }
        k = (s[i] / 4.0).floor() as i64 + 1;
        let c = &cs[i];
        let top = g.wall;
        let (p0, p1, p2) = (at(c, 0.7, top), at(c, 0.7, top + fh - 1.3), at(c, -0.35, top + fh));
        white.tube(&id, p0, p1, 0.06, 5, post);
        white.tube(&id, p1, p2, 0.05, 5, post);
    }
}

/// Height of the ground just behind the wall (`back` m outside the racing surface's edge).
fn outer_h(g: &Ground, c: &Centre, back: f32) -> f32 {
    let _ = back;
    let edge = g.height(c, -g.half);
    (edge + g.wall - 1.0).max(0.3)
}

/// The pit wall: a low concrete wall with a painted top between the apron's grass and pit road.
fn pit_wall(g: &Ground, look: &Look, cs: &[Centre], s: &[f32], concrete: &mut Builder) {
    if g.pit.is_none() {
        return;
    }
    let n = cs.len();
    let a = g.half + g.apron;
    let wall = rgb(look.pit_wall);
    for i in 0..n {
        let i1 = (i + 1) % n;
        let (c0, c1) = (&cs[i], &cs[i1]);
        let s_mid = (s[i] + if i1 == 0 { g.length } else { s[i1] }) / 2.0;
        if g.pit_k(s_mid) < 0.5 {
            continue;
        }
        let h = 1.1;
        let p = |c: &Centre, o: f32, up: f32| g.point(c, o, up);
        let (o0, o1) = (a + PIT_WALL.0, a + PIT_WALL.1);
        let uv = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        concrete.quad([p(c0, o0, 0.0), p(c1, o0, 0.0), p(c1, o0, h), p(c0, o0, h)], uv, wall, 1.0);
        concrete.quad([p(c0, o0, h), p(c1, o0, h), p(c1, o1, h), p(c0, o1, h)], uv, wall.map(|v| v * 1.05), 1.0);
        concrete.quad([p(c0, o1, h), p(c1, o1, h), p(c1, o1, 0.0), p(c0, o1, 0.0)], uv, wall.map(|v| v * 0.9), 1.0);
    }
}

/// Outside the wall: the ground falling away from the wall, the land beyond, and a tree line in the haze.
fn outside(g: &Ground, look: &Look, cs: &[Centre], grass: &mut Builder, white: &mut Builder) {
    let n = cs.len();
    let far_land = rgb(look.ground);
    let steps = [0.8, 4.0, 10.0, 18.0, 30.0];
    let h = |c: &Centre, back: f32| {
        let near = outer_h(g, c, 0.8) - 0.2;
        let t = ((back - 0.8) / 29.2).clamp(0.0, 1.0);
        near + (0.0 - near) * t * t * (3.0 - 2.0 * t)
    };
    let pt = |c: &Centre, back: f32, y: f32| {
        let base = g.point(c, -g.half, 0.0);
        let q = base + outward(c).scale(back);
        V3(q.0, y, q.2)
    };
    for i in 0..n {
        let i1 = (i + 1) % n;
        for w in steps.windows(2) {
            let (b0, b1) = (w[0], w[1]);
            let p = [
                pt(&cs[i], b0, h(&cs[i], b0)),
                pt(&cs[i1], b0, h(&cs[i1], b0)),
                pt(&cs[i1], b1, h(&cs[i1], b1)),
                pt(&cs[i], b1, h(&cs[i], b1)),
            ];
            let uv: [[f32; 2]; 4] = std::array::from_fn(|k| [p[k].0 / 7.0, p[k].2 / 7.0]);
            grass.quad(p, uv, rgb(look.grass).map(|v| v * 0.92), 1.0);
        }
    }
    // The land beyond, coarse (every 8th row), from just under the near ground out into the haze.
    let rings = [29.0, 120.0, look.trees.max(150.0), 700.0, 2400.0];
    let coarse: Vec<&Centre> = cs.iter().step_by(8).collect();
    let k = coarse.len();
    for i in 0..k {
        let (a, b) = (coarse[i], coarse[(i + 1) % k]);
        for w in rings.windows(2) {
            let y = -0.03;
            let p = [pt(a, w[0], y), pt(b, w[0], y), pt(b, w[1], y), pt(a, w[1], y)];
            let uv: [[f32; 2]; 4] = std::array::from_fn(|q| [p[q].0 / 7.0, p[q].2 / 7.0]);
            let tint = if w[0] >= 700.0 { far_land } else { rgb(look.grass).map(|v| v * 0.85) };
            grass.quad(p, uv, tint, 1.0);
        }
        // Trees: a ragged dark band facing the track.
        if look.trees > 0.0 {
            let (ha, hb) = (14.0 + 9.0 * hash01(i as i64, 1, 9), 14.0 + 9.0 * hash01(((i + 1) % k) as i64, 1, 9));
            let (ta, tb) = (pt(a, look.trees, 0.0), pt(b, look.trees, 0.0));
            let col = [0.2, 0.3, 0.16].map(|v| v * (0.85 + 0.3 * hash01(i as i64, 2, 9)));
            white.quad([ta, tb, V3(tb.0, hb, tb.2), V3(ta.0, ha, ta.2)], [[0.0; 2]; 4], col, 1.0);
        }
    }
}

/// The infield: the last row of grass joined across to its middle (the track is convex: one fan fills it), and a
/// few buildings behind pit road.
fn infield(track: &Track, g: &Ground, look: &Look, r: &Rows, grass: &mut Builder, white: &mut Builder) {
    let n = r.pts.len();
    let last: Vec<V3> = r.pts.iter().map(|row| *row.last().expect("a section")).collect();
    let mid = last.iter().fold(V3(0.0, 0.0, 0.0), |a, p| a + *p).scale(1.0 / n as f32);
    let mid = V3(mid.0, g.infield, mid.2);
    let col = rgba(rgb(look.grass), shade(V3(0.0, 1.0, 0.0), grass.sun), 1.0);
    for i in 0..n {
        let (a, b) = (last[i], last[(i + 1) % n]);
        let uv = |p: V3| [p.0 / 7.0, p.2 / 7.0];
        grass.vert(mid, uv(mid), col);
        grass.vert(a, uv(a), col);
        grass.vert(b, uv(b), col);
    }
    // Haulers and garages behind pit road: long boxes in a row, white roofs, their sides in team colours.
    if let Some((from, to)) = g.pit {
        let mut at = from + 15.0;
        let mut k = 0;
        while at < to - 25.0 {
            let (a, b) = (centre(track, at.rem_euclid(g.length)), centre(track, (at + 18.0).rem_euclid(g.length)));
            let (o0, o1) = (g.half + g.apron + 32.0, g.half + g.apron + 35.0);
            let col = rgb(look.car_colors[k % look.car_colors.len().max(1)]);
            let (y0, y1) = (g.infield, g.infield + 4.0);
            let p = |c: &Centre, o: f32, y: f32| {
                let q = g.point(c, o, 0.0);
                V3(q.0, y, q.2)
            };
            white.quad([p(&a, o0, y0), p(&b, o0, y0), p(&b, o0, y1), p(&a, o0, y1)], [[0.0; 2]; 4], col, 1.0);
            white.quad([p(&a, o0, y1), p(&b, o0, y1), p(&b, o1, y1), p(&a, o1, y1)], [[0.0; 2]; 4], [0.92, 0.92, 0.9], 1.0);
            white.quad([p(&a, o0, y0), p(&a, o0, y1), p(&a, o1, y1), p(&a, o1, y0)], [[0.0; 2]; 4], [0.8, 0.8, 0.8], 1.0);
            white.quad([p(&b, o0, y0), p(&b, o0, y1), p(&b, o1, y1), p(&b, o1, y0)], [[0.0; 2]; 4], [0.8, 0.8, 0.8], 1.0);
            at += 22.0;
            k += 1;
        }
    }
}

/// Grandstands outside the wall: rows of seats full of people rising away from the track, a suite tower on top.
fn stands(track: &Track, g: &Ground, look: &Look, crowd: &mut Builder, white: &mut Builder) {
    for st in &look.stands {
        let len = st.to - st.from;
        if len <= 0.0 {
            continue;
        }
        let n = (len / 3.0).ceil() as usize;
        let cs: Vec<Centre> = (0..=n).map(|i| centre(track, (st.from + len * i as f32 / n as f32).rem_euclid(g.length))).collect();
        let front = 4.0;
        let pt = |c: &Centre, back: f32, y: f32| {
            let base = g.point(c, -g.half, 0.0);
            let q = base + outward(c).scale(back);
            V3(q.0, y, q.2)
        };
        let base_h = |c: &Centre| g.height(c, -g.half) + g.wall + 1.2;
        let rows = 5;
        for i in 0..n {
            let (a, b) = (&cs[i], &cs[i + 1]);
            let (sa, sb) = (st.from + len * i as f32 / n as f32, st.from + len * (i + 1) as f32 / n as f32);
            for k in 0..rows {
                let (d0, d1) = (st.depth * k as f32 / rows as f32, st.depth * (k + 1) as f32 / rows as f32);
                let y = |c: &Centre, d: f32| base_h(c) + d * st.rise;
                let p =
                    [pt(a, front + d0, y(a, d0)), pt(b, front + d0, y(b, d0)), pt(b, front + d1, y(b, d1)), pt(a, front + d1, y(a, d1))];
                let uv = [[sa / 16.0, d0 / 12.8], [sb / 16.0, d0 / 12.8], [sb / 16.0, d1 / 12.8], [sa / 16.0, d1 / 12.8]];
                crowd.quad(p, uv, [1.0, 1.0, 1.0], 1.0);
            }
            let dark = [0.3, 0.3, 0.32];
            // The face under the first row, and the back wall above the last.
            let (fa, fb) = (pt(a, front, base_h(a)), pt(b, front, base_h(b)));
            white.quad([V3(fa.0, 0.0, fa.2), V3(fb.0, 0.0, fb.2), fb, fa], [[0.0; 2]; 4], dark, 1.0);
            let top = |c: &Centre| base_h(c) + st.depth * st.rise;
            let (ba, bb) = (pt(a, front + st.depth, top(a)), pt(b, front + st.depth, top(b)));
            white.quad([ba, bb, V3(bb.0, bb.1 + 3.5, bb.2), V3(ba.0, ba.1 + 3.5, ba.2)], [[0.0; 2]; 4], [0.55, 0.55, 0.58], 1.0);
            // The suite tower along the middle of the stand: glass bands between white floors.
            let mid = (i as f32 + 0.5) / n as f32;
            if st.tower && (0.25..0.75).contains(&mid) {
                let floors = [(3.5, 4.3, [0.9, 0.9, 0.88]), (4.3, 6.3, [0.18, 0.24, 0.3]), (6.3, 7.1, [0.9, 0.9, 0.88])];
                let floors2 = [(7.1, 9.1, [0.18, 0.24, 0.3]), (9.1, 10.0, [0.9, 0.9, 0.88])];
                for (lo, hi, col) in floors.into_iter().chain(floors2) {
                    let (qa, qb) = (pt(a, front + st.depth - 2.0, top(a) + lo), pt(b, front + st.depth - 2.0, top(b) + lo));
                    white.quad([qa, qb, V3(qb.0, qb.1 + hi - lo, qb.2), V3(qa.0, qa.1 + hi - lo, qa.2)], [[0.0; 2]; 4], col, 1.0);
                }
                let (ra, rb) = (pt(a, front + st.depth - 2.0, top(a) + 10.0), pt(b, front + st.depth - 2.0, top(b) + 10.0));
                let (ra2, rb2) = (pt(a, front + st.depth + 6.0, top(a) + 10.0), pt(b, front + st.depth + 6.0, top(b) + 10.0));
                white.quad([ra, rb, rb2, ra2], [[0.0; 2]; 4], [0.85, 0.85, 0.85], 1.0);
            }
        }
        // The stand's two ends.
        for (c, sign) in [(&cs[0], 1.0f32), (&cs[n], -1.0)] {
            let _ = sign;
            let (p0, p1) = (pt(c, front, base_h(c)), pt(c, front + st.depth, base_h(c) + st.depth * st.rise + 3.5));
            let q0 = V3(p0.0, 0.0, p0.2);
            let q1 = V3(p1.0, 0.0, p1.2);
            white.quad([q0, q1, p1, p0], [[0.0; 2]; 4], [0.5, 0.5, 0.52], 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_physics::TrackDef;

    pub fn charlotte() -> Track {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../games/race/tracks/charlotte.ron");
        let def: TrackDef = ron::from_str(&std::fs::read_to_string(path).expect("the track file")).expect("a TrackDef");
        Track::new(&def).expect("it closes")
    }

    #[test]
    fn the_road_mesh_closes_on_itself_with_no_gaps() {
        let t = charlotte();
        let look = Look::default();
        let g = ground(&t, &look);
        let r = rows(&t, &g, &look);
        // Consecutive rows are close (no gaps along the track), and the last row meets the first.
        for i in 0..r.pts.len() {
            let (a, b) = (&r.pts[i], &r.pts[(i + 1) % r.pts.len()]);
            let d = a[0] - b[0];
            assert!(d.dot(d).sqrt() < 2.6, "row {i} at s {} is {} m from the next", r.s[i], d.dot(d).sqrt());
        }
        // Across a row, neighbouring points are joined without steps: the road is one surface.
        for row in &r.pts {
            for w in row.windows(2) {
                assert!((w[1].1 - w[0].1).abs() < 3.0, "a cliff across the section: {:?}", w);
            }
        }
        // The racing surface's edges sit where the track says they are: 24° banking lifts the outside.
        // Mid turns 1-2 (the track starts just past the dogleg; the turn runs from ~37 m to ~773 m).
        let turn = r.s.iter().position(|&s| s > 400.0).unwrap();
        let outside = r.pts[turn][0].1;
        assert!(outside > 3.0 && outside < 4.5, "outside edge of a 24° turn is {outside} m up");
    }

    #[test]
    fn the_scene_is_built_with_its_parts_and_the_fence_is_drawn_last() {
        let t = charlotte();
        let s = build(&t, &Look::default());
        assert_eq!(s.meshes.last().unwrap().image, FENCE, "see-through last");
        assert!(s.triangles > 50_000 && s.triangles < 600_000, "{} triangles", s.triangles);
        assert!(s.meshes.iter().any(|m| m.image == CROWD && !m.verts.is_empty()), "grandstands");
        assert!(s.ground.infield < -4.0, "the infield lies below the banked apron: {}", s.ground.infield);
    }
}
