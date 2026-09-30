//! The stage: a game's HD 2.5D scene as data (`games/<name>/stage.ron`), and `compose`, which turns the world and
//! a camera into a list of textured quads. No GPU here: the quads are what any backend draws, and what tests read.
//!
//! Space: x runs along the world (1 = one cell), y runs down (0 = the ground line, 1 = one level underground),
//! depth runs into the screen (1 = the plane cut open; the far mountains sit at 40). A point at depth d moves
//! 1/d as fast as the camera: parallax is perspective, not a per-layer speed. World rows behind the plane stand a
//! little deeper (`row_depth`), so the surface reads as a strip of land, not a line.

use std::collections::BTreeMap;

use serde::Deserialize;
use sim_core::{Entity, World};
use sim_render::anim::{Anim, Player, Pose};
use sim_render::feel::Tween;
use sim_render::pixel::Season;
use sim_rules::Game;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stage {
    /// Asset packs with the images (`assets/<pack>.ron`, as for views).
    pub assets: Vec<String>,
    /// World cells across the screen at depth 1 (the zoom of the close camera).
    #[serde(default = "twelve")]
    pub cells_across: f32,
    /// Where the ground line sits when the camera looks at the surface (fraction of the screen height).
    #[serde(default = "horizon")]
    pub horizon: f32,
    /// The world row cut open (as the Diorama's `plane`).
    #[serde(default)]
    pub plane: i64,
    /// Depth added per world row behind the plane.
    #[serde(default = "row_depth")]
    pub row_depth: f32,
    /// Environment whose state is the season ("" = always summer).
    #[serde(default)]
    pub season_env: String,
    /// The kind the camera follows first.
    #[serde(default)]
    pub follow: String,
    /// Screen-filling sky (an image stretched over the whole view, drifting slowly), drawn first.
    #[serde(default)]
    pub sky: Option<String>,
    pub layers: Vec<Layer>,
    pub soil: Soil,
    pub kinds: BTreeMap<String, KindArt>,
    /// Title cards shown when the season changes: season name → image.
    #[serde(default)]
    pub season_cards: BTreeMap<String, String>,
    /// Darken the edges of the frame (0 = none, 100 = strong).
    #[serde(default)]
    pub vignette: u32,
    /// Quantities made visible: a prop drawn as a heap of items that grows, shrinks and spoils.
    #[serde(default)]
    pub piles: Vec<Pile>,
    /// What the player can do: buttons bound to the game's declared actions.
    #[serde(default)]
    pub buttons: Vec<Button>,
}

/// A prop of every entity of a kind, drawn as a heap: one item per `per` units, at most `max`. Items pop in when the
/// prop grows, the top ones go when it shrinks (eaten, spent), and the bottom ones turn when some of it spoils.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pile {
    pub kind: String,
    pub prop: String,
    pub item: String,
    #[serde(default = "one_i")]
    pub per: i64,
    #[serde(default = "forty")]
    pub max: usize,
    /// Height of one item in world units.
    pub size: f32,
    /// Where the heap stands, from the entity's foot (world units).
    #[serde(default)]
    pub offset: (f32, f32),
    /// Another prop counted in the same heap, drawn with another item at the bottom (spoiled food).
    #[serde(default)]
    pub spoil: Option<Spoil>,
    /// "enter", "leave" and "idle" (defaults: a pop, a shrink, stillness).
    #[serde(default)]
    pub anims: BTreeMap<String, Anim>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spoil {
    pub prop: String,
    pub item: String,
}

fn one_i() -> i64 {
    1
}
fn forty() -> usize {
    40
}

/// A button: a declared game action (`action`, for the first entity of kind `on`) behind an image, a key, a place on
/// the screen. Enabled exactly when the game would accept the action now (same checks as any agent).
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Button {
    pub action: String,
    pub on: String,
    pub icon: String,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub args: BTreeMap<String, i64>,
    /// Centre, as fractions of the screen (0,0 top left).
    pub at: (f32, f32),
    /// Diameter as a fraction of the screen height.
    #[serde(default = "button_size")]
    pub size: f32,
    /// "idle" (loops), "hover" (held while the pointer is over it), "press", "denied" (defaults provided).
    #[serde(default)]
    pub anims: BTreeMap<String, Anim>,
    /// Not drawn and not clickable: only its key works (a phone game whose moves are swipes, played on a keyboard).
    #[serde(default)]
    pub hidden: bool,
    /// When its key is let go: this action (and args) too (hold to crouch, hold to walk).
    #[serde(default)]
    pub release: Option<(String, BTreeMap<String, i64>)>,
}

fn button_size() -> f32 {
    0.11
}

fn twelve() -> f32 {
    12.0
}
fn horizon() -> f32 {
    0.55
}
fn row_depth() -> f32 {
    0.06
}
fn one() -> f32 {
    1.0
}

/// A band of scenery at one depth, tiled along x.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layer {
    pub image: String,
    /// 1 = the plane; larger = farther (moves slower, looks smaller); < 1 = in front of the plane.
    pub depth: f32,
    /// Height of the band in world units at its depth (its screen size shrinks with depth).
    pub height: f32,
    /// How far above the ground line its bottom sits (world units at its depth; negative = sinks below).
    #[serde(default)]
    pub lift: f32,
    /// Drawn in front of the sprites (grass the ants walk behind).
    #[serde(default)]
    pub front: bool,
    #[serde(default = "one")]
    pub opacity: f32,
    /// Out-of-focus blur (mip bias): 0 = sharp, 2 = soft.
    #[serde(default)]
    pub blur: f32,
    /// Atmospheric haze: % blended towards the horizon colour.
    #[serde(default)]
    pub haze: u32,
    /// Repeat by mirroring (art that is not seamless); default: plain repeat (import with `--tile`).
    #[serde(default)]
    pub mirror: bool,
    /// Horizontal offset in world units (so two layers of one image do not line up).
    #[serde(default)]
    pub offset: f32,
}

/// The ground cut open: soil texture below the ground line, hollows where the plane has no terrain.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Soil {
    pub image: String,
    /// World units one repeat of the soil texture covers.
    #[serde(default = "one")]
    pub scale: f32,
    /// Texture of the tunnel walls (hollows).
    pub hollow: String,
    /// How much of the lower levels darkens (0..100 at the bottom).
    #[serde(default)]
    pub darken: u32,
}

/// How a kind looks: an image per state (the first matching state selector wins, deepest first), size, motion.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KindArt {
    /// Frames (images) of the default look; one image = no animation.
    pub frames: Vec<String>,
    #[serde(default)]
    pub states: BTreeMap<String, Vec<String>>,
    /// Height in world units.
    pub height: f32,
    #[serde(default = "eight")]
    pub fps: f32,
    /// World units a moving sprite bobs.
    #[serde(default)]
    pub bob: f32,
    /// Sinks this far into the ground (a mound's base, a bush's roots).
    #[serde(default)]
    pub sink: f32,
    /// The picture faces left (flip it to face right).
    #[serde(default)]
    pub faces_left: bool,
}

fn eight() -> f32 {
    8.0
}

/// One textured rectangle in screen pixels. `uv` may run past 0..1 on a repeating texture.
#[derive(Clone, Debug, PartialEq)]
pub struct Quad {
    pub image: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub uv: [f32; 4],
    /// Colour multiplied in at the top and the bottom edge (rgb, opacity).
    pub top: [f32; 4],
    pub bottom: [f32; 4],
    /// Mip bias (blur) and desaturation (0..1).
    pub blur: f32,
    pub desat: f32,
    pub wrap: Wrap,
    /// Degrees, clockwise, about the centre.
    pub rot: f32,
}

/// How a texture continues past 0..1: clamped (sprites), repeated (seamless textures), mirrored (painted
/// scenery that does not tile: every other copy is flipped, so no seam shows).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wrap {
    Clamp,
    /// Both ways (seamless textures: soil).
    Repeat,
    /// Along x only (scenery bands: the top edge must not wrap into the bottom one).
    RepeatX,
    MirrorX,
}

/// Name of the built-in 1×1 white texture (solid colours and gradients).
pub const WHITE: &str = "__white";
/// Name of the built-in soft round blob (hollows, snow, vignette).
pub const BLOB: &str = "__blob";

pub struct Camera {
    /// World point at the centre of the screen (x) and at the horizon line (y).
    pub x: f32,
    pub y: f32,
    /// Cells across the screen at depth 1.
    pub across: f32,
}

/// Image sizes the composer needs (for aspect ratios).
pub type Sizes = BTreeMap<String, (u32, u32)>;

fn rgba(c: sim_render::Rgb, a: f32) -> [f32; 4] {
    [c.0 as f32 / 255.0, c.1 as f32 / 255.0, c.2 as f32 / 255.0, a]
}

fn lerp(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t, a[3] + (b[3] - a[3]) * t]
}

/// The season's colour grade: multiply and desaturation.
pub fn grade(season: Season) -> ([f32; 4], f32) {
    match season {
        Season::Summer => ([1.0, 1.0, 1.0, 1.0], 0.0),
        Season::Autumn => ([1.08, 0.92, 0.72, 1.0], 0.15),
        Season::Winter => ([0.92, 0.96, 1.08, 1.0], 0.55),
    }
}

pub fn season_of(world: &World, env: &str) -> (Season, i64) {
    let e = (!env.is_empty()).then(|| world.of_kind(env).next()).flatten();
    (e.map_or(Season::Summer, |e| Season::from_state(&e.state)), e.and_then(|e| e.props.get("warmth").copied()).unwrap_or(60))
}

pub struct Composer<'a> {
    pub stage: &'a Stage,
    pub game: &'a Game,
    pub sizes: &'a Sizes,
    /// Screen size in pixels.
    pub w: f32,
    pub h: f32,
}

impl Composer<'_> {
    /// Pixels per world unit at depth 1.
    pub fn unit(&self, cam: &Camera) -> f32 {
        self.w / cam.across.max(0.5)
    }

    /// Screen position of a world point at a depth.
    pub fn project(&self, cam: &Camera, x: f32, y: f32, depth: f32) -> (f32, f32) {
        let s = self.unit(cam) / depth.max(0.05);
        (self.w / 2.0 + (x - cam.x) * s, self.h * self.stage.horizon + (y - cam.y) * s)
    }

    fn aspect(&self, image: &str) -> f32 {
        self.sizes.get(image).map_or(1.0, |(w, h)| *w as f32 / (*h).max(1) as f32)
    }

    /// Everything to draw this frame, back to front.
    pub fn compose(&self, world: &World, cam: &Camera, tween: Option<&Tween>, time: f32) -> Vec<Quad> {
        self.compose_with(world, cam, tween, time, None)
    }

    /// `compose`, with the heaps (`piles`) the renderer remembers from frame to frame.
    pub fn compose_with(&self, world: &World, cam: &Camera, tween: Option<&Tween>, time: f32, piles: Option<&PileMemory>) -> Vec<Quad> {
        let st = self.stage;
        let (season, warmth) = season_of(world, &st.season_env);
        let (tint, desat) = grade(season);
        let (sky_top, horizon) = season.sky(warmth);
        let (sky_top, horizon) = (rgba(sky_top, 1.0), rgba(horizon, 1.0));
        let mut out = Vec::new();
        let plain = |image: &str, x: f32, y: f32, w: f32, h: f32, c: [f32; 4]| Quad {
            image: image.into(),
            x,
            y,
            w,
            h,
            uv: [0.0, 0.0, 1.0, 1.0],
            top: c,
            bottom: c,
            blur: 0.0,
            desat: 0.0,
            wrap: Wrap::Clamp,
            rot: 0.0,
        };

        // Sky: the gradient behind everything, then the painted sky (drifting very slowly), graded by the season.
        out.push(Quad { top: sky_top, bottom: horizon, ..plain(WHITE, 0.0, 0.0, self.w, self.h, sky_top) });
        if let Some(sky) = &st.sky {
            // Scaled to cover the sky, never repeated (a mirrored cloud would show); it drifts inside the picture.
            let (_, gy) = self.project(cam, 0.0, 0.0, 1.0);
            let (iw, ih) = self.sizes.get(sky).map_or((1.0, 1.0), |(w, h)| (*w as f32, *h as f32));
            let scale = (self.w / iw).max(gy.max(1.0) / ih) * 1.15;
            let span = (self.w / (iw * scale)).min(1.0);
            let drift = 0.5 + 0.5 * (time * 0.01 + cam.x * 0.004).sin();
            let u0 = (1.0 - span) * drift;
            let v1 = (gy / (ih * scale)).min(1.0);
            out.push(Quad { uv: [u0, 1.0 - v1, u0 + span, 1.0], desat, ..plain(sky, 0.0, 0.0, self.w, gy, tint) });
        }

        // Scenery behind the plane, far to near.
        let mut layers: Vec<&Layer> = st.layers.iter().collect();
        layers.sort_by(|a, b| b.depth.total_cmp(&a.depth));
        // Scenery stands on the ground line: horizontal parallax comes from depth, but vertically every band rides
        // with the ground, or a camera looking down into the nest would sink the mountains behind the soil.
        let (_, ground_y) = self.project(cam, 0.0, 0.0, 1.0);
        let underground = (cam.y / 1.5).clamp(0.0, 1.0);
        let layer_quad = |l: &Layer| -> Quad {
            let s = self.unit(cam) / l.depth;
            let bottom = ground_y - l.lift * s;
            let top = bottom - l.height * s;
            let world_w = l.height * self.aspect(&l.image);
            // The world x at the screen's left and right edges, at this depth.
            let x0 = cam.x - self.w / 2.0 / s + l.offset;
            let x1 = cam.x + self.w / 2.0 / s + l.offset;
            let c = lerp(tint, horizon, l.haze as f32 / 100.0);
            // Foreground cover belongs to the surface: it fades as the camera goes down into the nest.
            let fade = if l.front { 1.0 - underground } else { 1.0 };
            let c = [c[0], c[1], c[2], l.opacity * fade];
            Quad {
                uv: [x0 / world_w, 0.0, x1 / world_w, 1.0],
                wrap: if l.mirror { Wrap::MirrorX } else { Wrap::RepeatX },
                blur: l.blur,
                desat,
                ..plain(&l.image, 0.0, top, self.w, bottom - top, c)
            }
        };
        for l in layers.iter().filter(|l| !l.front) {
            out.push(layer_quad(l));
        }

        // The ground cut open: soil from the ground line to the bottom of the world, hollows where there is none.
        let levels = (world.depth - 1).max(0) as f32;
        let (sx0, gy) = self.project(cam, -40.0, 0.0, 1.0);
        let (sx1, by) = self.project(cam, world.width as f32 + 40.0, levels.max(0.0) + 40.0, 1.0);
        let dark = 1.0 - st.soil.darken as f32 / 100.0;
        // The soil stays a backdrop to the tunnels: a little dimmer than the art, darker with depth.
        let top_soil = [tint[0] * 0.82, tint[1] * 0.8, tint[2] * 0.78, 1.0];
        let soil_tint =
            |y: f32| lerp(top_soil, [top_soil[0] * dark, top_soil[1] * dark, top_soil[2] * dark, 1.0], (y / levels.max(1.0)).min(1.0));
        let span = (world.width as f32 + 80.0) / st.soil.scale;
        let deep = (levels + 40.0) / st.soil.scale * self.aspect(&st.soil.image);
        out.push(Quad {
            uv: [-40.0 / st.soil.scale, 0.0, span - 40.0 / st.soil.scale, deep],
            top: soil_tint(0.0),
            bottom: soil_tint(levels + 40.0),
            wrap: Wrap::Repeat,
            desat: 0.25 + desat * 0.5,
            ..plain(&st.soil.image, sx0, gy, sx1 - sx0, by - gy, tint)
        });
        // Hollows: soft dark blobs a little larger than a cell. Towards a hollow neighbour (right, below) a blob
        // stretches over the joint, so hollows join into galleries. All shadow rings first, then all the dark
        // cores (packed, slightly moist earth), so no ring shows inside a gallery.
        let mut shapes = Vec::new();
        for z in 1..world.depth {
            for x in 0..world.width {
                if world.is_terrain(x, st.plane, z) {
                    continue;
                }
                let open = |dx: i64, dz: i64| {
                    (0..world.width).contains(&(x + dx)) && z + dz < world.depth && !world.is_terrain(x + dx, st.plane, z + dz)
                };
                shapes.push((x as f32, (z - 1) as f32, 1.0, 1.0));
                if open(1, 0) {
                    shapes.push((x as f32 + 0.5, (z - 1) as f32 + 0.12, 1.0, 0.76));
                }
                if open(0, 1) {
                    shapes.push((x as f32 + 0.15, z as f32 - 0.5, 0.7, 1.0));
                }
            }
        }
        for (grow, k) in [(0.45, [0.08, 0.05, 0.03, 0.55]), (0.22, [0.13, 0.09, 0.065, 1.0])] {
            for &(sx, sy, sw, sh) in &shapes {
                let (hx, hy) = self.project(cam, sx - grow, sy - grow * 0.5, 1.0);
                let (hx1, hy1) = self.project(cam, sx + sw + grow, sy + sh + grow * 0.5, 1.0);
                out.push(plain(BLOB, hx, hy, hx1 - hx, hy1 - hy, k));
            }
        }

        // Entities: far rows first, then nearer; underground only near the cut.
        let mut ents: Vec<(&Entity, f32, f32, f32)> = Vec::new();
        for e in world.entities().values() {
            if self.game.is_hidden(&e.kind) || !st.kinds.contains_key(&e.kind) {
                continue;
            }
            if e.z > 0 && (e.y - st.plane).abs() > 1 {
                continue;
            }
            let (x, y, z) = tween.map_or((e.x as f32, e.y as f32, e.z as f32), |t| t.at(e.id, (e.x, e.y, e.z)));
            let depth = if e.z == 0 { (1.0 + (y - st.plane as f32) * st.row_depth).max(0.5) } else { 1.0 };
            ents.push((e, x, z, depth));
        }
        ents.sort_by(|a, b| b.3.total_cmp(&a.3).then(a.0.id.cmp(&b.0.id)));
        for (e, x, z, depth) in ents {
            let art = &st.kinds[&e.kind];
            let frames = self.frames(art, e);
            let Some(first) = frames.first() else { continue };
            let moving = tween.is_some_and(|t| t.moving(e.id, (e.x, e.y, e.z)));
            // The walk cycle runs only while it moves; standing still, it holds the first pose.
            let n = if moving { ((time * art.fps) as usize + e.id as usize) % frames.len() } else { 0 };
            let image = &frames[n];
            let bob = if moving && art.bob > 0.0 { art.bob * (time * art.fps * std::f32::consts::PI).sin().abs() } else { 0.0 };
            let foot = z + art.sink - bob;
            let (cx, fy) = self.project(cam, x + 0.5, foot, depth);
            let hpx = art.height * self.unit(cam) / depth;
            let wpx = hpx * self.aspect(first);
            let left = tween.and_then(|t| t.prev.get(&e.id)).is_some_and(|p| p.0 > e.x);
            let flip = left != art.faces_left;
            // Rows behind the plane recede into the haze a little.
            let far = ((depth - 1.0) * 4.0).clamp(0.0, 0.5);
            let c = lerp(tint, horizon, far);
            // A soft contact shadow grounds the sprite (smaller while it bobs up).
            let lift = 1.0 - bob * 4.0;
            out.push(plain(BLOB, cx - wpx * 0.45 * lift, fy - hpx * 0.08, wpx * 0.9 * lift, hpx * 0.2, [0.0, 0.0, 0.0, 0.35 * lift]));
            out.push(Quad {
                uv: if flip { [1.0, 0.0, 0.0, 1.0] } else { [0.0, 0.0, 1.0, 1.0] },
                desat,
                ..plain(image, cx - wpx / 2.0, fy - hpx, wpx, hpx, [c[0], c[1], c[2], 1.0])
            });
        }

        if let Some(mem) = piles {
            out.extend(self.piles(world, cam, mem, time));
        }

        for l in layers.iter().filter(|l| l.front) {
            out.push(layer_quad(l));
        }

        // Winter: snow drifting down in screen space (moves with time, never with the simulation).
        if season == Season::Winter {
            for i in 0..140u64 {
                let n = sim_render::pixel::noise(i as i64, 0, 77);
                let speed = 20.0 + (n % 40) as f32;
                let x = ((n % 10_000) as f32 / 10_000.0 * self.w + (time * (n % 7) as f32 * 3.0)).rem_euclid(self.w);
                let y = ((n / 10_000 % 10_000) as f32 / 10_000.0 * self.h + time * speed).rem_euclid(self.h);
                let r = 2.0 + (n % 4) as f32;
                out.push(plain(BLOB, x, y, r, r, [1.0, 1.0, 1.0, 0.8]));
            }
        }
        if st.vignette > 0 {
            let a = st.vignette as f32 / 100.0;
            out.push(plain("__vignette", 0.0, 0.0, self.w, self.h, [0.0, 0.0, 0.0, a]));
        }
        out
    }

    /// The heaps: every remembered item at its place in its heap, posed by its animation.
    fn piles(&self, world: &World, cam: &Camera, mem: &PileMemory, time: f32) -> Vec<Quad> {
        let mut out = Vec::new();
        for (pi, pile) in self.stage.piles.iter().enumerate() {
            for e in world.of_kind(&pile.kind) {
                let Some(items) = mem.items.get(&(e.id, pi)) else { continue };
                if e.z > 0 && (e.y - self.stage.plane).abs() > 1 {
                    continue;
                }
                let spoiled = pile.spoil.as_ref().map_or(0, |s| (e.props.get(&s.prop).copied().unwrap_or(0) / pile.per.max(1)) as usize);
                let depth = if e.z == 0 { (1.0 + (e.y - self.stage.plane) as f32 * self.stage.row_depth).max(0.5) } else { 1.0 };
                let s = self.unit(cam) / depth;
                let hpx = pile.size * s;
                for (i, item) in items.iter().enumerate() {
                    let (dx, dy) = heap_slot(i, pile.max);
                    let image = match &pile.spoil {
                        Some(sp) if i < spoiled && item.gone.is_none() => &sp.item,
                        _ => &pile.item,
                    };
                    let aspect = self.aspect(image);
                    let mut pose = pile.anims.get("idle").map_or_else(Pose::default, |a| a.sample(time + i as f32 * 0.37));
                    pose = match item.gone {
                        Some(g) => pose.then(anim_or(&pile.anims, "leave", default_leave).sample(time - g)),
                        None => pose.then(anim_or(&pile.anims, "enter", default_enter).sample(time - item.born)),
                    };
                    if pose.alpha <= 0.0 || pose.scale <= 0.0 {
                        continue;
                    }
                    let (fx, fy) = self.project(
                        cam,
                        e.x as f32 + 0.5 + pile.offset.0 + dx * pile.size * 0.85,
                        e.z as f32 + pile.offset.1 - dy * pile.size * 0.62,
                        depth,
                    );
                    let (h, w) = (hpx * pose.scale, hpx * pose.scale * aspect);
                    let c = [pose.bright, pose.bright, pose.bright, pose.alpha];
                    let n = sim_render::pixel::noise(i as i64, e.id as i64, 5);
                    out.push(Quad {
                        image: image.clone(),
                        x: fx - w / 2.0 + pose.x * hpx,
                        y: fy - h + pose.y * hpx,
                        w,
                        h,
                        uv: if n.is_multiple_of(2) { [0.0, 0.0, 1.0, 1.0] } else { [1.0, 0.0, 0.0, 1.0] },
                        top: c,
                        bottom: c,
                        blur: 0.0,
                        desat: 0.0,
                        wrap: Wrap::Clamp,
                        rot: pose.rot + (n % 41) as f32 - 20.0,
                    });
                }
            }
        }
        out
    }

    /// The frames for an entity's current state: the deepest matching state selector, else the default.
    fn frames<'b>(&self, art: &'b KindArt, e: &Entity) -> &'b [String] {
        let label = self.game.state_label(e);
        art.states
            .iter()
            .filter(|(sel, _)| sim_state::in_label(label, sel))
            .min_by_key(|(sel, _)| sim_state::depth_in_label(label, sel))
            .map_or(&art.frames, |(_, f)| f)
    }
}

/// A title card over the scene, fading in and out (`age` seconds since it appeared, `life` seconds in all),
/// over a soft dark cloud so it reads on a bright sky.
pub fn card(image: &str, sizes: &Sizes, w: f32, h: f32, age: f32, life: f32) -> Vec<Quad> {
    if age < 0.0 || age > life {
        return Vec::new();
    }
    let fade = 0.6_f32;
    let a = (age / fade).min(1.0).min((life - age) / fade).clamp(0.0, 1.0);
    let (iw, ih) = sizes.get(image).copied().unwrap_or((16, 9));
    let cw = (w * 0.42).min(h * 0.42 * iw as f32 / ih.max(1) as f32);
    let ch = cw * ih as f32 / iw.max(1) as f32;
    let rise = (1.0 - a) * h * 0.02;
    let (x, y) = ((w - cw) / 2.0, (h - ch) / 2.0 + rise);
    let quad = |image: &str, x: f32, y: f32, qw: f32, qh: f32, c: [f32; 4]| Quad {
        image: image.into(),
        x,
        y,
        w: qw,
        h: qh,
        uv: [0.0, 0.0, 1.0, 1.0],
        top: c,
        bottom: c,
        blur: 0.0,
        desat: 0.0,
        wrap: Wrap::Clamp,
        rot: 0.0,
    };
    vec![
        quad(BLOB, x - cw * 0.25, y - ch * 0.3, cw * 1.5, ch * 1.6, [0.0, 0.0, 0.0, 0.45 * a]),
        quad(image, x, y, cw, ch, [1.0, 1.0, 1.0, a]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn colony() -> (World, Game, Stage, Sizes) {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../games/colony3d");
        let (world, game) = Game::load(&dir, None).expect("loads");
        let (stage, assets) = crate::load_stage(&dir, None).expect("every image the stage names exists");
        let sizes = assets.loaded.iter().map(|(n, i)| (n.clone(), (i.w as u32, i.h as u32))).collect();
        (world, game, stage, sizes)
    }

    #[test]
    fn parallax_is_perspective_far_things_move_slower() {
        let (_, game, stage, sizes) = colony();
        let c = Composer { stage: &stage, game: &game, sizes: &sizes, w: 1600.0, h: 900.0 };
        let at = |x: f32, depth: f32| c.project(&Camera { x, y: 0.0, across: 8.0 }, 5.0, 0.0, depth).0;
        let near = at(0.0, 1.0) - at(1.0, 1.0);
        let far = at(0.0, 4.0) - at(1.0, 4.0);
        assert!((near - 4.0 * far).abs() < 0.01, "a camera step moves depth 4 a quarter as far: {near} vs {far}");
    }

    #[test]
    fn scenery_stands_on_the_ground_line_wherever_the_camera_looks() {
        let (world, game, stage, sizes) = colony();
        let c = Composer { stage: &stage, game: &game, sizes: &sizes, w: 1600.0, h: 900.0 };
        for y in [0.0, 2.5] {
            let cam = Camera { x: 10.0, y, across: 8.0 };
            let (_, ground) = c.project(&cam, 0.0, 0.0, 1.0);
            let q = c.compose(&world, &cam, None, 0.0);
            let meadow = q.iter().find(|q| q.image == "meadow").expect("drawn");
            let lift = stage.layers.iter().find(|l| l.image == "meadow").unwrap().lift;
            let bottom = meadow.y + meadow.h;
            assert!((bottom - (ground - lift * c.unit(&cam) / 4.0)).abs() < 0.5, "camera y {y}: {bottom} vs ground {ground}");
        }
    }

    #[test]
    fn the_foreground_fades_when_the_camera_goes_underground() {
        let (world, game, stage, sizes) = colony();
        let c = Composer { stage: &stage, game: &game, sizes: &sizes, w: 1600.0, h: 900.0 };
        let front = |y: f32| {
            let q = c.compose(&world, &Camera { x: 10.0, y, across: 8.0 }, None, 0.0);
            // The front copy is the blurred one (the turf on the cut uses the same picture, sharp).
            q.iter().filter(|q| q.image == "grass_front" && q.blur > 1.0).map(|q| q.top[3]).fold(0.0f32, f32::max)
        };
        assert!(front(0.0) > 0.9);
        assert!(front(3.0) < 0.01);
    }

    #[test]
    fn only_the_cut_shows_underground() {
        let (world, game, stage, sizes) = colony();
        let c = Composer { stage: &stage, game: &game, sizes: &sizes, w: 1600.0, h: 900.0 };
        let q = c.compose(&world, &Camera { x: 16.0, y: 2.0, across: 40.0 }, None, 0.0);
        let sprites = q.iter().filter(|q| stage.kinds.values().any(|k| k.frames.contains(&q.image))).count();
        let shown = world
            .entities()
            .values()
            .filter(|e| stage.kinds.contains_key(&e.kind) && !game.is_hidden(&e.kind))
            .filter(|e| e.z == 0 || (e.y - stage.plane).abs() <= 1)
            .count();
        assert!(sprites <= shown && sprites > 0, "{sprites} sprites, {shown} entities on the surface or the cut");
    }

    #[test]
    fn a_heap_grows_from_the_middle_and_rows_rise() {
        let xs: Vec<f32> = (0..5).map(|i| heap_slot(i, 40).0.round()).collect();
        assert_eq!(xs, vec![0.0, -1.0, 1.0, -2.0, 2.0], "centre first, then outwards");
        assert_eq!(heap_slot(0, 40).1, 0.0);
        assert!(heap_slot(39, 40).1 > 3.0, "the last items are high up the heap");
    }

    #[test]
    fn heaps_remember_arrivals_and_departures() {
        let (mut world, _, stage, _) = colony();
        let nest = world.of_kind("nest").next().unwrap().id;
        let key = (nest, 0);
        let mut mem = PileMemory::default();
        let set = |w: &mut World, food: i64, spoiled: i64| {
            let p = w.props_mut(nest).unwrap();
            p.insert("food".into(), food);
            p.insert("spoiled".into(), spoiled);
        };
        set(&mut world, 5, 0);
        mem.update(&stage, &world, 10.0);
        assert!(mem.items[&key].iter().all(|it| it.born < 0.0), "what is there at the start does not pop in");
        set(&mut world, 8, 0);
        mem.update(&stage, &world, 11.0);
        assert_eq!(mem.items[&key].iter().filter(|it| it.born >= 11.0).count(), 3, "a delivery arrives on top");
        set(&mut world, 4, 2);
        mem.update(&stage, &world, 12.0);
        let items = &mem.items[&key];
        assert_eq!(items.iter().filter(|it| it.gone.is_none()).count(), 6, "food + spoiled share one heap");
        assert_eq!(items.iter().filter(|it| it.gone == Some(12.0)).count(), 2, "the top ones leave");
        mem.update(&stage, &world, 13.0);
        assert_eq!(mem.items[&key].len(), 6, "left items are dropped once their leave animation is over");
    }

    #[test]
    fn buttons_hit_by_circle_and_look_disabled_when_the_game_would_refuse() {
        let (_, _, stage, _) = colony();
        let b = &stage.buttons[0];
        let (cx, cy, r) = b.circle(1600.0, 900.0);
        assert!(b.hit(1600.0, 900.0, cx + r * 0.7, cy) && !b.hit(1600.0, 900.0, cx + r * 1.1, cy));
        let mut on = ButtonState { enabled: true, ..ButtonState::default() };
        let mut off = ButtonState::default();
        let (a_on, a_off) = (b.quads(&mut on, 1600.0, 900.0, 0.0)[1].top[3], b.quads(&mut off, 1600.0, 900.0, 0.0)[1].top[3]);
        assert!(a_on > 0.95 && a_off < 0.6, "{a_on} {a_off}");
        off.player.play("press", 1.0);
        let pressed = b.quads(&mut off, 1600.0, 900.0, 1.05);
        assert!(pressed[1].w < b.quads(&mut on, 1600.0, 900.0, 1.05)[1].w, "a press squashes it");
    }

    #[test]
    fn a_title_card_fades_in_and_out() {
        let sizes: Sizes = [("c".to_string(), (1600, 900))].into_iter().collect();
        let alpha = |age: f32| card("c", &sizes, 1600.0, 900.0, age, 3.0).last().map_or(0.0, |q| q.top[3]);
        assert_eq!(alpha(-0.1), 0.0);
        assert!(alpha(0.1) < 0.3);
        assert_eq!(alpha(1.5), 1.0);
        assert!(alpha(2.9) < 0.3);
        assert_eq!(alpha(3.1), 0.0);
    }
}

fn anim_or(anims: &BTreeMap<String, Anim>, name: &str, default: fn() -> Anim) -> Anim {
    anims.get(name).cloned().unwrap_or_else(default)
}

/// A key as written in code: (time, value, ease).
type CodeKey = (f32, f32, sim_render::anim::Ease);

fn keys(k: &[CodeKey]) -> Vec<sim_render::anim::Key> {
    k.iter().map(|&(t, v, e)| sim_render::anim::Key::Eased(t, v, e)).collect()
}

fn anim(tracks: &[(&str, &[CodeKey])], looping: bool) -> Anim {
    Anim { tracks: tracks.iter().map(|(n, k)| (n.to_string(), keys(k))).collect(), looping }
}

use sim_render::anim::Ease::{BackOut, BounceOut, Linear, QuadIn, QuadOut, SineInOut};

/// An item arriving: dropped onto the heap, landing with a bounce, a small pop.
pub fn default_enter() -> Anim {
    anim(
        &[
            ("y", &[(0.0, -2.5, Linear), (0.45, 0.0, BounceOut)]),
            ("scale", &[(0.0, 0.4, Linear), (0.3, 1.1, BackOut), (0.45, 1.0, QuadOut)]),
        ],
        false,
    )
}

/// An item leaving (eaten, spent): it lifts a little and shrinks away.
pub fn default_leave() -> Anim {
    anim(
        &[
            ("scale", &[(0.0, 1.0, Linear), (0.35, 0.0, QuadIn)]),
            ("y", &[(0.0, 0.0, Linear), (0.35, -0.6, QuadOut)]),
            ("alpha", &[(0.0, 1.0, Linear), (0.35, 0.0, QuadIn)]),
        ],
        false,
    )
}

pub fn default_button(name: &str) -> Option<Anim> {
    Some(match name {
        "idle" => anim(&[("scale", &[(0.0, 1.0, Linear), (1.2, 1.03, SineInOut), (2.4, 1.0, SineInOut)])], true),
        "hover" => {
            anim(&[("scale", &[(0.0, 1.0, Linear), (0.18, 1.1, BackOut)]), ("bright", &[(0.0, 1.0, Linear), (0.18, 1.12, QuadOut)])], false)
        }
        "press" => anim(
            &[
                ("scale", &[(0.0, 1.0, Linear), (0.07, 0.84, QuadOut), (0.3, 1.06, BackOut), (0.42, 1.0, QuadOut)]),
                ("rot", &[(0.0, 0.0, Linear), (0.07, -6.0, QuadOut), (0.42, 0.0, BackOut)]),
            ],
            false,
        ),
        "denied" => anim(
            &[
                (
                    "x",
                    &[
                        (0.0, 0.0, Linear),
                        (0.06, -0.08, QuadOut),
                        (0.14, 0.08, SineInOut),
                        (0.22, -0.05, SineInOut),
                        (0.3, 0.03, SineInOut),
                        (0.38, 0.0, QuadOut),
                    ],
                ),
                ("bright", &[(0.0, 0.7, Linear), (0.38, 1.0, QuadOut)]),
            ],
            false,
        ),
        _ => return None,
    })
}

/// Where item `i` of a heap sits: rows fill from the bottom, each one item narrower, centred; (column offset in
/// items, row).
pub fn heap_slot(i: usize, max: usize) -> (f32, f32) {
    let base = ((((8 * max.max(1) + 1) as f32).sqrt() - 1.0) / 2.0).ceil().max(1.0) as usize;
    let (mut row, mut left) = (0usize, i);
    let mut width = base;
    while left >= width && width > 1 {
        left -= width;
        row += 1;
        width -= 1;
    }
    // Within a row, fill from the middle outwards (0, +1, -1, +2, ...), so a small heap is a mound, not a line; odd
    // rows shift half an item, into the gaps of the row below.
    let k = left as f32;
    let from_middle = if left % 2 == 0 { k / 2.0 } else { -(k + 1.0) / 2.0 };
    let jitter = (sim_render::pixel::noise(i as i64, 0, 11) % 100) as f32 / 100.0 * 0.3 - 0.15;
    (from_middle + if row % 2 == 1 { 0.5 } else { 0.0 } + jitter, row as f32)
}

/// One item of a heap, as the renderer remembers it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Item {
    pub born: f32,
    pub gone: Option<f32>,
}

/// The heaps between frames: which items exist, when each arrived, and which are on their way out.
#[derive(Clone, Debug, Default)]
pub struct PileMemory {
    pub items: BTreeMap<(sim_core::EntityId, usize), Vec<Item>>,
    started: bool,
}

impl PileMemory {
    /// Brings the heaps up to the world at time `now`: new units arrive on top, missing ones leave from the top.
    /// The first call takes the world as it is (no pops for what was already there).
    pub fn update(&mut self, stage: &Stage, world: &World, now: f32) {
        let first = !self.started;
        self.started = true;
        for (pi, pile) in stage.piles.iter().enumerate() {
            for e in world.of_kind(&pile.kind) {
                let units = e.props.get(&pile.prop).copied().unwrap_or(0)
                    + pile.spoil.as_ref().map_or(0, |s| e.props.get(&s.prop).copied().unwrap_or(0));
                let want = ((units.max(0) / pile.per.max(1)) as usize).min(pile.max);
                let items = self.items.entry((e.id, pi)).or_default();
                let leave = anim_or(&pile.anims, "leave", default_leave);
                items.retain(|it| it.gone.is_none_or(|g| !leave.done(now - g)));
                let alive = items.iter().filter(|it| it.gone.is_none()).count();
                if want > alive {
                    let born = if first { now - 100.0 } else { now };
                    // Staggered a little, so a big delivery pours in instead of appearing at once.
                    items.extend((0..want - alive).map(|k| Item { born: born + k as f32 * 0.05, gone: None }));
                } else if want < alive {
                    let mut drop = alive - want;
                    for it in items.iter_mut().rev() {
                        if drop == 0 {
                            break;
                        }
                        if it.gone.is_none() {
                            it.gone = Some(now);
                            drop -= 1;
                        }
                    }
                }
                // Items still leaving sit on top of the heap until they are gone: keep live ones first.
                items.sort_by_key(|it| it.gone.is_some());
            }
        }
    }
}

/// What a button is doing this frame (the renderer keeps it).
#[derive(Clone, Debug, Default)]
pub struct ButtonState {
    pub player: Player,
    pub hover_since: Option<f32>,
    pub enabled: bool,
    /// How lit it looks, 0..1: follows `enabled` over a quarter second instead of snapping.
    pub lit: f32,
    last: Option<f32>,
}

impl Button {
    fn anim(&self, name: &str) -> Option<Anim> {
        self.anims.get(name).cloned().or_else(|| default_button(name))
    }

    /// Centre and radius in pixels on a `w × h` screen.
    pub fn circle(&self, w: f32, h: f32) -> (f32, f32, f32) {
        (self.at.0 * w, self.at.1 * h, self.size * h / 2.0)
    }

    pub fn hit(&self, w: f32, h: f32, x: f32, y: f32) -> bool {
        if self.hidden {
            return false;
        }
        let (cx, cy, r) = self.circle(w, h);
        (x - cx).powi(2) + (y - cy).powi(2) <= r * r
    }

    /// The button's pose now: idle loop, the hover on top while hovered, then any one-shot (press, denied).
    pub fn pose(&self, st: &mut ButtonState, now: f32) -> Pose {
        let mut p = self.anim("idle").map_or_else(Pose::default, |a| a.sample(now));
        if let (Some(since), Some(h)) = (st.hover_since, self.anim("hover")) {
            p = p.then(h.sample(now - since));
        }
        if let Some((name, at)) = st.player.current.clone() {
            match self.anim(&name) {
                Some(a) if !a.done(now - at) => p = p.then(a.sample(now - at)),
                _ => st.player.current = None,
            }
        }
        p
    }

    /// The button's quads: a soft shadow, then the icon, posed; greyed and faded when the action is not possible.
    pub fn quads(&self, st: &mut ButtonState, w: f32, h: f32, now: f32) -> Vec<Quad> {
        if self.hidden {
            return Vec::new();
        }
        let pose = self.pose(st, now);
        // A press always shows at full colour (the action may already be in effect and so unavailable); otherwise the
        // look eases towards enabled / disabled.
        let pressing = st.player.current.as_ref().is_some_and(|(n, _)| n == "press");
        let target = if st.enabled || pressing { 1.0 } else { 0.0 };
        // The first frame has no previous one: dt = 1 s, so it starts at its target.
        let dt = st.last.map_or(1.0, |l| (now - l).max(0.0));
        st.last = Some(now);
        st.lit += (target - st.lit) * (dt * 12.0).min(1.0);
        let (cx, cy, r) = self.circle(w, h);
        let d = 2.0 * r * pose.scale;
        let (x, y) = (cx + pose.x * 2.0 * r - d / 2.0, cy + pose.y * 2.0 * r - d / 2.0);
        let a = pose.alpha * (0.55 + 0.45 * st.lit);
        let b = pose.bright * (0.8 + 0.2 * st.lit);
        let quad = |image: &str, x: f32, y: f32, qw: f32, qh: f32, c: [f32; 4], desat: f32, rot: f32| Quad {
            image: image.into(),
            x,
            y,
            w: qw,
            h: qh,
            uv: [0.0, 0.0, 1.0, 1.0],
            top: c,
            bottom: c,
            blur: 0.0,
            desat,
            wrap: Wrap::Clamp,
            rot,
        };
        vec![
            quad(BLOB, x - d * 0.08, y + d * 0.1, d * 1.16, d * 1.08, [0.0, 0.0, 0.0, 0.4 * a], 0.0, 0.0),
            quad(&self.icon, x, y, d, d, [b, b, b, a], 0.85 * (1.0 - st.lit), pose.rot),
        ]
    }
}
