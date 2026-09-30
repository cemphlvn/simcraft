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
use super::photos::{Found, Photos};
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

/// How much of the groove's rubber is on the surface at offset `o` (0..1): a band across the racing line.
pub fn rubber(look: &Look, g: &Ground, bank: f32, o: f32) -> f32 {
    let d = (o - groove_centre(look, g, bank)) / 2.6;
    (-0.5 * d * d).exp()
}

/// A surface's triangles: on the game's photograph when it has one (lit white, the photo's own scale), else on the
/// picture the view makes (tinted by the look's colour, at `scale` m a tile).
pub struct Surf {
    pub b: Builder,
    pub photo: Option<Found>,
    pub scale: f32,
}

impl Surf {
    pub fn new(photos: &Photos, key: &str, fallback: &str, wrap: Wrap, sun: V3, scale: f32) -> Surf {
        match photos.get(key) {
            Some(f) => Surf { b: Builder::new(&f.texture, wrap, sun), photo: Some(f.clone()), scale: f.size },
            None => Surf { b: Builder::new(fallback, wrap, sun), photo: None, scale },
        }
    }

    /// The colour to draw with: white on a photograph (it has its own), else the look's.
    pub fn tint(&self, look: [f32; 3]) -> [f32; 3] {
        if self.photo.is_some() { [1.0; 3] } else { look }
    }

    /// Texture coordinates for a strip along a wall: `s` along the track, `y` up (m). A photograph keeps its
    /// proportions (its height is `size`), the view's own picture its old scale.
    pub fn strip_uv(&self, s: f32, y: f32) -> [f32; 2] {
        match &self.photo {
            Some(f) => [s / (f.size * f.aspect()), -y / f.size],
            None => [s / 4.0, y * 0.3],
        }
    }
}

pub fn build(track: &Track, look: &Look, photos: &Photos) -> Scene {
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
    let mut road = Surf::new(photos, "asphalt", ASPHALT, Wrap::Repeat, sun, 3.0);
    let mut apron = Surf::new(photos, "apron", ASPHALT, Wrap::Repeat, sun, 3.0);
    let mut pit_road = Surf::new(photos, "pit", CONCRETE, Wrap::Repeat, sun, 3.0);
    let mut grass = Surf::new(photos, "grass", GRASS, Wrap::Repeat, sun, 7.0);
    let mut concrete = Surf::new(photos, "concrete", CONCRETE, Wrap::Repeat, sun, 4.0);
    // Painted lines: plain paint, whatever the surface under them.
    let mut paint = Builder::new(WHITE, Wrap::Clamp, sun);
    // The rubbered groove over the asphalt: the same triangles again (so the same depths: no flicker), the photo
    // streaked along the track, fading out across the racing line.
    let mut groove_b = photos.get("groove").map(|f| (Builder::new(&f.texture, Wrap::Repeat, sun), f.size));
    let marked = |s: f32| marks.iter().any(|&(at, w)| at == 0.0 && s >= at && s <= at + w);
    let stall = |s: f32| marks.iter().skip(1).any(|&(at, w)| s >= at && s <= at + w);
    let line = |c: (u8, u8, u8)| rgb(c).map(|v| v * 0.8);
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
            // Patches of older and newer paving (subtler over a photograph, which has its own).
            let amp = if road.photo.is_some() { 0.04 } else { 0.1 };
            let patch = 1.0 - amp * 0.6 + amp * hash01((s_mid / 9.0) as i64, (o0 / 3.0).floor() as i64, 5);
            // Per corner: base colour × sunlight (× the groove on the racing surface; lighter over a photographed
            // groove, which brings its own rubber).
            let rubber_k = if groove_b.is_some() { 0.35 } else { 1.0 };
            let corner = |k: usize, base: [f32; 3], groove_on: bool| {
                let gk = if groove_on { 1.0 - (1.0 - groove(look, &g, banks[k], os[k])) * rubber_k } else { 1.0 };
                rgba(base, shade(ns[k], sun) * gk, 1.0)
            };
            let on_line = marked(s_mid) && o0 < g.half + g.apron && o0 >= -g.half;
            let asp = road.tint(rgb(look.asphalt)).map(|v| v * patch);
            let painted = match band {
                _ if on_line => Some(line(look.white)),
                Band::White => Some(line(look.white)),
                Band::Yellow => Some(line(look.yellow)),
                Band::PitWhite if pit => Some(line(look.white)),
                Band::PitYellow if pit => Some(line(look.yellow)),
                Band::Stall if pit && stall(s_mid) => Some(line(look.white)),
                _ => None,
            };
            if let Some(col) = painted {
                let c: [[f32; 4]; 4] = std::array::from_fn(|k| corner(k, col, false));
                paint.quad_c(p, [[0.0; 2]; 4], c);
                continue;
            }
            let (surf, base, groove_on): (&mut Surf, [f32; 3], bool) = match band {
                Band::Road => (&mut road, asp, true),
                Band::Seam => (&mut road, asp.map(|v| v * 0.78), true),
                Band::Apron => {
                    let c = apron.tint(rgb(look.apron)).map(|v| v * patch);
                    (&mut apron, c, false)
                }
                Band::Pit if pit => {
                    let c = pit_road.tint(rgb(look.concrete));
                    (&mut pit_road, c, false)
                }
                Band::Stall if pit => {
                    let c = pit_road.tint(rgb(look.concrete)).map(|v| v * 0.93);
                    (&mut pit_road, c, false)
                }
                _ => {
                    let c = grass.tint(rgb(look.grass));
                    (&mut grass, c, false)
                }
            };
            let sc = surf.scale;
            let uv: [[f32; 2]; 4] =
                if band == Band::Grass || !pit && matches!(band, Band::Pit | Band::PitWhite | Band::PitYellow | Band::Stall) {
                    std::array::from_fn(|k| [p[k].0 / sc, p[k].2 / sc])
                } else {
                    std::array::from_fn(|k| [os[k] / sc, ss[k] / sc])
                };
            let c: [[f32; 4]; 4] = std::array::from_fn(|k| corner(k, base, groove_on));
            surf.b.quad_c(p, uv, c);
            if let (true, Some((gb, gs))) = (groove_on, groove_b.as_mut()) {
                let strength = (look.groove * 3.3).min(1.0);
                let a: [f32; 4] = std::array::from_fn(|k| strength * rubber(look, &g, banks[k], os[k]));
                if a.iter().cloned().fold(0.0, f32::max) > 0.03 {
                    let c: [[f32; 4]; 4] = std::array::from_fn(|k| {
                        let l = shade(ns[k], sun) * patch;
                        [l, l, l, a[k]]
                    });
                    let uv: [[f32; 2]; 4] = std::array::from_fn(|k| [ss[k] / *gs, os[k] / *gs]);
                    gb.quad_c(p, uv, c);
                }
            }
        }
    }
    let mut white = Builder::new(WHITE, Wrap::Clamp, sun);
    let mut crowd = Surf::new(photos, "crowd", CROWD, Wrap::Repeat, sun, 16.0);
    let mut fence = Builder::new(FENCE, Wrap::Repeat, sun);
    let mut safer = photos.get("safer").map(|f| (Builder::new(&f.texture, Wrap::Repeat, sun), f.clone()));
    let mut suites = photos.get("suites").map(|f| (Builder::new(&f.texture, Wrap::Repeat, sun), f.clone()));
    let mut trees = photos.get("trees").map(|f| (Builder::new(&f.texture, Wrap::Repeat, sun), f.clone()));
    let mut signs: Vec<Builder> = Vec::new();
    walls(&g, look, &centres, &r.s, &mut white, &mut concrete, &mut fence, safer.as_mut());
    pit_wall(&g, look, &centres, &r.s, &mut concrete);
    outside(&g, look, &centres, &mut grass, &mut white, trees.as_mut());
    infield(track, &g, look, &r, &mut grass, &mut white);
    stands(track, &g, look, &mut crowd, &mut white, suites.as_mut());
    if let Some(f) = photos.get("logo") {
        signs.push(logos(track, &g, look, f, sun));
    }
    if let Some(f) = photos.get("banner") {
        signs.push(banner(track, &g, look, f, &mut white, sun));
    }
    if let Some(f) = photos.get("sponsors") {
        signs.push(boards(&g, look, &centres, &r.s, f, sun));
    }
    let mut meshes: Vec<Mesh> = vec![road.b.mesh(), apron.b.mesh(), pit_road.b.mesh()];
    meshes.extend(groove_b.map(|(b, _)| b.mesh()));
    meshes.extend([paint.mesh(), concrete.b.mesh(), grass.b.mesh(), white.mesh(), crowd.b.mesh()]);
    meshes.extend(safer.map(|(b, _)| b.mesh()).into_iter().chain(suites.map(|(b, _)| b.mesh())));
    meshes.extend(signs.into_iter().map(Builder::mesh));
    // See-through last: the tree line, then the fence.
    meshes.extend(trees.map(|(b, _)| b.mesh()));
    meshes.push(fence.mesh());
    meshes.retain(|m| !m.verts.is_empty() || m.image == FENCE);
    let triangles = meshes.iter().map(|m| m.verts.len() / 3).sum();
    Scene { ground: g, meshes, sun, triangles }
}

/// Outward (to the right of travel, horizontal) at a centre.
fn outward(c: &Centre) -> V3 {
    let (sn, cs) = c.heading.sin_cos();
    V3(sn, 0.0, -cs)
}

/// The SAFER barrier on the concrete wall, and the catch fence on top, around the whole outside.
#[allow(clippy::too_many_arguments)]
fn walls(
    g: &Ground,
    look: &Look,
    cs: &[Centre],
    s: &[f32],
    white: &mut Builder,
    concrete: &mut Surf,
    fence: &mut Builder,
    mut safer_photo: Option<&mut (Builder, Found)>,
) {
    let n = cs.len();
    let safer = rgb(look.safer);
    let wall = concrete.tint(rgb(look.concrete));
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
        match safer_photo.as_deref_mut() {
            // The photograph of the barrier's face, as high as the face (its tubes and scuffs are in it).
            Some((sb_, f)) => {
                let along = f.size * f.aspect();
                let uv = [[sa / along, 1.0], [sb / along, 1.0], [sb / along, 0.0], [sa / along, 0.0]];
                sb_.quad([at(a, 0.0, 0.0), at(b, 0.0, 0.0), at(b, 0.0, 1.0), at(a, 0.0, 1.0)], uv, [0.92 + 0.08 * scuff; 3], 1.0);
            }
            None => {
                for (lo, hi, col) in bands {
                    let col = if lo < 0.5 { col.map(|v| v * scuff) } else { col };
                    white.quad([at(a, 0.0, lo), at(b, 0.0, lo), at(b, 0.0, hi), at(a, 0.0, hi)], [[0.0; 2]; 4], col, 1.0);
                }
            }
        }
        white.quad([at(a, 0.0, 1.0), at(b, 0.0, 1.0), at(b, 0.5, 1.0), at(a, 0.5, 1.0)], [[0.0; 2]; 4], safer.map(|v| v * 0.9), 1.0);
        let top = g.wall;
        let uv =
            |ya: f32, yb: f32| [concrete.strip_uv(sa, ya), concrete.strip_uv(sb, ya), concrete.strip_uv(sb, yb), concrete.strip_uv(sa, yb)];
        let (u_face, u_top, u_back) = (uv(1.0, top), uv(top, top + 0.3), uv(top, 0.0));
        concrete.b.quad([at(a, 0.5, 1.0), at(b, 0.5, 1.0), at(b, 0.5, top), at(a, 0.5, top)], u_face, wall, 1.0);
        concrete.b.quad([at(a, 0.5, top), at(b, 0.5, top), at(b, 0.8, top), at(a, 0.8, top)], u_top, wall, 1.0);
        let (ba, bb) = (at(a, 0.8, top), at(b, 0.8, top));
        concrete.b.quad(
            [ba, bb, V3(bb.0, outer_h(g, b, 0.8) - 0.2, bb.2), V3(ba.0, outer_h(g, a, 0.8) - 0.2, ba.2)],
            u_back,
            wall.map(|v| v * 0.8),
            1.0,
        );
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
fn pit_wall(g: &Ground, look: &Look, cs: &[Centre], s: &[f32], concrete: &mut Surf) {
    if g.pit.is_none() {
        return;
    }
    let n = cs.len();
    let a = g.half + g.apron;
    let wall = concrete.tint(rgb(look.pit_wall));
    for i in 0..n {
        let i1 = (i + 1) % n;
        let (c0, c1) = (&cs[i], &cs[i1]);
        let (sa, sb) = (s[i], if i1 == 0 { g.length } else { s[i1] });
        if g.pit_k((sa + sb) / 2.0) < 0.5 {
            continue;
        }
        let h = 1.1;
        let p = |c: &Centre, o: f32, up: f32| g.point(c, o, up);
        let (o0, o1) = (a + PIT_WALL.0, a + PIT_WALL.1);
        let (uv_face, uv_top) = match &concrete.photo {
            Some(_) => {
                let u = |ya: f32, yb: f32| {
                    [concrete.strip_uv(sa, ya), concrete.strip_uv(sb, ya), concrete.strip_uv(sb, yb), concrete.strip_uv(sa, yb)]
                };
                (u(0.0, h), u(h, h + 0.6))
            }
            None => ([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]], [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]),
        };
        concrete.b.quad([p(c0, o0, 0.0), p(c1, o0, 0.0), p(c1, o0, h), p(c0, o0, h)], uv_face, wall, 1.0);
        concrete.b.quad([p(c0, o0, h), p(c1, o0, h), p(c1, o1, h), p(c0, o1, h)], uv_top, wall.map(|v| v * 1.05), 1.0);
        concrete.b.quad([p(c0, o1, h), p(c1, o1, h), p(c1, o1, 0.0), p(c0, o1, 0.0)], uv_face, wall.map(|v| v * 0.9), 1.0);
    }
}

/// Outside the wall: the ground falling away from the wall, the land beyond, and a tree line in the haze.
fn outside(g: &Ground, look: &Look, cs: &[Centre], grass: &mut Surf, white: &mut Builder, trees: Option<&mut (Builder, Found)>) {
    let n = cs.len();
    let far_land = rgb(look.ground);
    let green = grass.tint(rgb(look.grass));
    let sc = grass.scale;
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
            let uv: [[f32; 2]; 4] = std::array::from_fn(|k| [p[k].0 / sc, p[k].2 / sc]);
            grass.b.quad(p, uv, green.map(|v| v * 0.92), 1.0);
        }
    }
    // The land beyond, coarse (every 8th row), from just under the near ground out into the haze.
    let rings = [29.0, 120.0, look.trees.max(150.0), 700.0, 2400.0];
    let coarse: Vec<&Centre> = cs.iter().step_by(8).collect();
    let k = coarse.len();
    let mut trees = trees;
    let mut along = 0.0f32;
    for i in 0..k {
        let (a, b) = (coarse[i], coarse[(i + 1) % k]);
        for w in rings.windows(2) {
            let y = -0.03;
            let p = [pt(a, w[0], y), pt(b, w[0], y), pt(b, w[1], y), pt(a, w[1], y)];
            let uv: [[f32; 2]; 4] = std::array::from_fn(|q| [p[q].0 / sc, p[q].2 / sc]);
            let tint = if w[0] >= 700.0 && grass.photo.is_none() { far_land } else { green.map(|v| v * 0.85) };
            grass.b.quad(p, uv, tint, 1.0);
        }
        if look.trees <= 0.0 {
            continue;
        }
        let (ta, tb) = (pt(a, look.trees, 0.0), pt(b, look.trees, 0.0));
        match trees.as_deref_mut() {
            // The photographed tree line, its height the picture's `size`, seamless along the ring.
            Some((tr, f)) => {
                let len = ((tb - ta).dot(tb - ta)).sqrt();
                let per = f.size * f.aspect();
                let (u0, u1) = (along / per, (along + len) / per);
                along += len;
                let hh = f.size;
                let lit = [0.9 + 0.2 * hash01(i as i64, 2, 9); 3];
                tr.glow(
                    [V3(ta.0, hh, ta.2), V3(tb.0, hh, tb.2), tb, ta],
                    [[u0, 0.0], [u1, 0.0], [u1, 1.0], [u0, 1.0]],
                    [lit[0], lit[1], lit[2], 1.0],
                );
            }
            // Trees: a ragged dark band facing the track.
            None => {
                let (ha, hb) = (14.0 + 9.0 * hash01(i as i64, 1, 9), 14.0 + 9.0 * hash01(((i + 1) % k) as i64, 1, 9));
                let col = [0.2, 0.3, 0.16].map(|v| v * (0.85 + 0.3 * hash01(i as i64, 2, 9)));
                white.quad([ta, tb, V3(tb.0, hb, tb.2), V3(ta.0, ha, ta.2)], [[0.0; 2]; 4], col, 1.0);
            }
        }
    }
}

/// The infield: the last row of grass joined across to its middle (the track is convex: one fan fills it), and a
/// few buildings behind pit road.
fn infield(track: &Track, g: &Ground, look: &Look, r: &Rows, grass: &mut Surf, white: &mut Builder) {
    let n = r.pts.len();
    let last: Vec<V3> = r.pts.iter().map(|row| *row.last().expect("a section")).collect();
    let mid = last.iter().fold(V3(0.0, 0.0, 0.0), |a, p| a + *p).scale(1.0 / n as f32);
    let mid = V3(mid.0, g.infield, mid.2);
    let col = rgba(grass.tint(rgb(look.grass)), shade(V3(0.0, 1.0, 0.0), grass.b.sun), 1.0);
    let sc = grass.scale;
    for i in 0..n {
        let (a, b) = (last[i], last[(i + 1) % n]);
        let uv = |p: V3| [p.0 / sc, p.2 / sc];
        grass.b.vert(mid, uv(mid), col);
        grass.b.vert(a, uv(a), col);
        grass.b.vert(b, uv(b), col);
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
fn stands(track: &Track, g: &Ground, look: &Look, crowd: &mut Surf, white: &mut Builder, mut suites: Option<&mut (Builder, Found)>) {
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
        // Up the slope of the seating (m), for a photographed crowd.
        let slope = (1.0 + st.rise * st.rise).sqrt();
        for i in 0..n {
            let (a, b) = (&cs[i], &cs[i + 1]);
            let (sa, sb) = (st.from + len * i as f32 / n as f32, st.from + len * (i + 1) as f32 / n as f32);
            for k in 0..rows {
                let (d0, d1) = (st.depth * k as f32 / rows as f32, st.depth * (k + 1) as f32 / rows as f32);
                let y = |c: &Centre, d: f32| base_h(c) + d * st.rise;
                let p =
                    [pt(a, front + d0, y(a, d0)), pt(b, front + d0, y(b, d0)), pt(b, front + d1, y(b, d1)), pt(a, front + d1, y(a, d1))];
                let uv = match &crowd.photo {
                    Some(f) => {
                        let (along, up) = (f.size * f.aspect(), f.size);
                        let (v0, v1) = (-d0 * slope / up, -d1 * slope / up);
                        [[-sa / along, v0], [-sb / along, v0], [-sb / along, v1], [-sa / along, v1]]
                    }
                    None => [[sa / 16.0, d0 / 12.8], [sb / 16.0, d0 / 12.8], [sb / 16.0, d1 / 12.8], [sa / 16.0, d1 / 12.8]],
                };
                crowd.b.quad(p, uv, [1.0, 1.0, 1.0], 1.0);
            }
            let dark = [0.3, 0.3, 0.32];
            // The face under the first row, and the back wall above the last.
            let (fa, fb) = (pt(a, front, base_h(a)), pt(b, front, base_h(b)));
            white.quad([V3(fa.0, 0.0, fa.2), V3(fb.0, 0.0, fb.2), fb, fa], [[0.0; 2]; 4], dark, 1.0);
            let top = |c: &Centre| base_h(c) + st.depth * st.rise;
            let (ba, bb) = (pt(a, front + st.depth, top(a)), pt(b, front + st.depth, top(b)));
            white.quad([ba, bb, V3(bb.0, bb.1 + 3.5, bb.2), V3(ba.0, ba.1 + 3.5, ba.2)], [[0.0; 2]; 4], [0.55, 0.55, 0.58], 1.0);
            // The suite tower along the middle of the stand: glass bands between white floors (or its photograph).
            let mid = (i as f32 + 0.5) / n as f32;
            if st.tower && (0.25..0.75).contains(&mid) {
                let back = front + st.depth - 2.0;
                let roof = match suites.as_deref_mut() {
                    Some((sb_, f)) => {
                        let (hh, along) = (f.size, f.size * f.aspect());
                        let (qa, qb) = (pt(a, back, top(a) + 3.5), pt(b, back, top(b) + 3.5));
                        let up = V3(0.0, hh, 0.0);
                        sb_.quad(
                            [qa, qb, qb + up, qa + up],
                            [[-sa / along, 1.0], [-sb / along, 1.0], [-sb / along, 0.0], [-sa / along, 0.0]],
                            [1.0; 3],
                            1.0,
                        );
                        3.5 + hh
                    }
                    None => {
                        let floors = [(3.5, 4.3, [0.9, 0.9, 0.88]), (4.3, 6.3, [0.18, 0.24, 0.3]), (6.3, 7.1, [0.9, 0.9, 0.88])];
                        let floors2 = [(7.1, 9.1, [0.18, 0.24, 0.3]), (9.1, 10.0, [0.9, 0.9, 0.88])];
                        for (lo, hi, col) in floors.into_iter().chain(floors2) {
                            let (qa, qb) = (pt(a, back, top(a) + lo), pt(b, back, top(b) + lo));
                            white.quad([qa, qb, V3(qb.0, qb.1 + hi - lo, qb.2), V3(qa.0, qa.1 + hi - lo, qa.2)], [[0.0; 2]; 4], col, 1.0);
                        }
                        10.0
                    }
                };
                let (ra, rb) = (pt(a, back, top(a) + roof), pt(b, back, top(b) + roof));
                let (ra2, rb2) = (pt(a, front + st.depth + 6.0, top(a) + roof), pt(b, front + st.depth + 6.0, top(b) + roof));
                white.quad([ra, rb, rb2, ra2], [[0.0; 2]; 4], [0.85, 0.85, 0.85], 1.0);
            }
        }
        // The stand's two ends.
        for c in [&cs[0], &cs[n]] {
            let (p0, p1) = (pt(c, front, base_h(c)), pt(c, front + st.depth, base_h(c) + st.depth * st.rise + 3.5));
            let q0 = V3(p0.0, 0.0, p0.2);
            let q1 = V3(p1.0, 0.0, p1.2);
            white.quad([q0, q1, p1, p0], [[0.0; 2]; 4], [0.5, 0.5, 0.52], 1.0);
        }
    }
}

/// The track's emblem painted on the infield grass, read from the track (its top away from it).
fn logos(track: &Track, g: &Ground, look: &Look, f: &Found, sun: V3) -> Builder {
    let mut b = Builder::new(&f.texture, Wrap::Clamp, sun);
    let (w, d) = (f.size, f.size / f.aspect());
    for &(s, inset) in &look.logos {
        let c = centre(track, s.rem_euclid(g.length));
        let (sn, cs) = c.heading.sin_cos();
        let (fwd, left) = (V3(cs, 0.0, sn), V3(-sn, 0.0, cs));
        let near = g.half + g.apron + SLOPE + inset;
        let o = V3(c.x, g.infield + 0.35, c.y);
        let p = |along: f32, across: f32| o + fwd.scale(along) + left.scale(across);
        b.quad(
            [p(-w / 2.0, near + d), p(w / 2.0, near + d), p(w / 2.0, near), p(-w / 2.0, near)],
            [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            [1.0; 3],
            1.0,
        );
    }
    b
}

/// A gantry over the track at `look.banner_at`: two posts, a truss, and the banner hanging from it, facing the
/// cars coming (and readable from behind, in the mirror).
fn banner(track: &Track, g: &Ground, look: &Look, f: &Found, white: &mut Builder, sun: V3) -> Builder {
    let mut b = Builder::new(&f.texture, Wrap::Clamp, sun);
    let c = centre(track, look.banner_at.rem_euclid(g.length));
    let (sn, cs) = c.heading.sin_cos();
    let (fwd, left) = (V3(cs, 0.0, sn), V3(-sn, 0.0, cs));
    let (o_out, o_in) = (-g.half - 1.2, g.half + g.apron);
    let (h, w) = (f.size, f.size * f.aspect());
    let top = g.height(&c, -g.half).max(g.height(&c, 0.0)) + 6.0 + h;
    let at = |o: f32, y: f32| V3(c.x - o * sn, y, c.y + o * cs);
    let id = super::geom::Frame { o: V3(0.0, 0.0, 0.0), r: V3(1.0, 0.0, 0.0), u: V3(0.0, 1.0, 0.0), f: V3(0.0, 0.0, 1.0) };
    let steel = [0.55, 0.56, 0.58];
    for o in [o_out, o_in] {
        white.tube(&id, at(o, g.height(&c, o) - 0.5), at(o, top + 0.6), 0.28, 8, steel);
    }
    for dy in [0.0, 0.6] {
        for df in [-0.3, 0.3] {
            white.tube(&id, at(o_out, top + dy) + fwd.scale(df), at(o_in, top + dy) + fwd.scale(df), 0.06, 5, steel);
        }
    }
    // Centred over the racing surface: the front faces the cars coming, the back (a hair behind) reads from behind.
    let mid = at(0.0, top - 0.1);
    let corner = |side: f32, down: f32, back: f32| mid + left.scale(side * w / 2.0) - V3(0.0, down, 0.0) + fwd.scale(back);
    let (back_, front_) = (0.04, -0.04);
    b.quad(
        [corner(1.0, 0.0, front_), corner(-1.0, 0.0, front_), corner(-1.0, h, front_), corner(1.0, h, front_)],
        [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        [1.0; 3],
        1.0,
    );
    b.quad(
        [corner(-1.0, 0.0, back_), corner(1.0, 0.0, back_), corner(1.0, h, back_), corner(-1.0, h, back_)],
        [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        [1.0; 3],
        1.0,
    );
    b
}

/// Sponsor boards on the catch fence, facing the track, each the next panel of the `sponsors` picture.
fn boards(g: &Ground, look: &Look, cs: &[Centre], s: &[f32], f: &Found, sun: V3) -> Builder {
    let mut b = Builder::new(&f.texture, Wrap::Clamp, sun);
    let (every, panels) = (look.boards.0, look.boards.1.max(1) as usize);
    if every <= 0.0 {
        return b;
    }
    let (h, w) = (f.size, f.size * f.aspect() / panels as f32);
    let at = |c: &Centre, up: f32| g.point(c, -g.half, 0.0) + outward(c).scale(0.55) + V3(0.0, g.wall + up, 0.0);
    let mut next = every / 2.0;
    let mut k = 0usize;
    for i in 0..cs.len() {
        if s[i] < next || s[i] + w > g.length {
            continue;
        }
        // The board from here to `w` further on: the row whose distance along is closest.
        let j = (i..cs.len()).find(|&j| s[j] >= s[i] + w).unwrap_or(cs.len() - 1);
        let (a, e) = (&cs[i], &cs[j]);
        let (u0, u1) = (k as f32 / panels as f32, (k + 1) as f32 / panels as f32);
        // Seen from the track, the board's left is further along it.
        b.quad([at(e, 0.3 + h), at(a, 0.3 + h), at(a, 0.3), at(e, 0.3)], [[u0, 0.0], [u1, 0.0], [u1, 1.0], [u0, 1.0]], [1.0; 3], 1.0);
        k = (k + 1) % panels;
        next = s[i] + every;
    }
    b
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
        let s = build(&t, &Look::default(), &Photos::default());
        assert_eq!(s.meshes.last().unwrap().image, FENCE, "see-through last");
        assert!(s.triangles > 50_000 && s.triangles < 600_000, "{} triangles", s.triangles);
        assert!(s.meshes.iter().any(|m| m.image == CROWD && !m.verts.is_empty()), "grandstands");
        assert!(s.ground.infield < -4.0, "the infield lies below the banked apron: {}", s.ground.infield);
    }

    #[test]
    fn photographs_replace_the_views_own_pictures_surface_by_surface() {
        let t = charlotte();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../games/race");
        let photo = |f: &str, size: f32| crate::drive::photos::Photo { file: f.into(), size };
        let look = Look {
            textures: std::collections::BTreeMap::from([
                ("asphalt".to_string(), photo("race/asphalt.jpg", 4.0)),
                ("groove".to_string(), photo("race/groove.jpg", 6.0)),
                ("banner".to_string(), photo("race/banner.jpg", 4.0)),
            ]),
            ..Look::default()
        };
        let photos = Photos::resolve(&dir, &look.textures);
        let s = build(&t, &look, &photos);
        let has = |name: &str| s.meshes.iter().any(|m| m.image == name && !m.verts.is_empty());
        assert!(has("__drive_photo_asphalt") && has("__drive_photo_groove") && has("__drive_photo_banner"));
        // What has no photograph keeps the view's own picture; the see-through fence stays last.
        assert!(has(GRASS) && has(CROWD));
        assert_eq!(s.meshes.last().unwrap().image, FENCE);
        // The groove lies on the asphalt's own triangles (same depths: it cannot flicker), fading across the line.
        let groove = s.meshes.iter().find(|m| m.image == "__drive_photo_groove").unwrap();
        let road = s.meshes.iter().find(|m| m.image == "__drive_photo_asphalt").unwrap();
        let on_road = |p: [f32; 3]| road.verts.iter().any(|v| v.pos == p);
        assert!(groove.verts.iter().take(600).all(|v| on_road(v.pos)));
        let alphas: Vec<f32> = groove.verts.iter().map(|v| v.color[3]).collect();
        assert!(alphas.iter().any(|a| *a > 0.8) && alphas.iter().any(|a| *a < 0.2), "strong on the line, faint at its edges");
    }
}
