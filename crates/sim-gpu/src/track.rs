//! First person along a track (`games/<name>/track.ron`): the world's x is lanes, its y is the road ahead, and the
//! camera rides with the followed entity. Composing is pure (world + camera state + time → a frame description), so
//! any frame can be tested or recorded without a window.
//!
//! Space: X across the road (0 = its middle, one lane = 1), Y up (0 = the road), Z forward (world row + 0.5).

use std::collections::BTreeMap;

use serde::Deserialize;
use sim_core::{Entity, EntityId, World};
use sim_render::anim::{Anim, Ease};
use sim_render::feel::{CameraFeel, Spring, Tween};
use sim_rules::Game;

use crate::fx::{Fx, FxState, jitter, overlay};
use crate::gpu::{Mesh, Vert3};
use crate::math::{Eye, V3};
use crate::stage::{Button, ButtonState, Quad, Sizes, WHITE, Wrap};

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Track {
    pub assets: Vec<String>,
    /// The kind the camera rides with.
    pub follow: String,
    #[serde(default)]
    pub sky: Option<String>,
    /// A band of scenery at the horizon (mesas, a skyline), drifting a little as the camera moves across.
    #[serde(default)]
    pub skyline: Option<Skyline>,
    pub road: Road,
    pub camera: Cam,
    /// Other camera views (`c` cycles: the main `camera`, then these). The camera moves between them on springs.
    #[serde(default)]
    pub views: Vec<View>,
    /// Rows drawn ahead of the camera (fog hides the end).
    #[serde(default = "sixty")]
    pub view: f32,
    /// Fog colour (sRGB) and where it starts and is total (world units ahead).
    pub fog: ((u8, u8, u8), f32, f32),
    pub kinds: BTreeMap<String, Prop>,
    #[serde(default)]
    pub scenery: Vec<Scenery>,
    /// The car's hood (or a cockpit) along the bottom of the screen.
    #[serde(default)]
    pub hood: Option<Hood>,
    /// Camera effects by game event (override or add to the built-in ones, see `fx`).
    #[serde(default)]
    pub fx: BTreeMap<String, Anim>,
    #[serde(default)]
    pub buttons: Vec<Button>,
    /// The move between positions: the game's core mechanic, tuned here.
    #[serde(default)]
    pub switch: Option<Switch>,
    /// Quantities of the followed entity shown as rows of icons (nitro canisters, coins).
    #[serde(default)]
    pub meters: Vec<Meter>,
    /// Quantities shown as a bar (fuel).
    #[serde(default)]
    pub bars: Vec<Bar>,
    /// A state of the followed entity during which it blinks (invulnerable after a hit).
    #[serde(default)]
    pub blink: Option<String>,
}

fn sixty() -> f32 {
    60.0
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Skyline {
    pub image: String,
    /// Height as a fraction of the screen.
    pub height: f32,
    /// How much it drifts per unit the camera moves across (0 = fixed at infinity).
    #[serde(default)]
    pub drift: f32,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Road {
    pub image: String,
    pub lanes: u32,
    /// World units one repeat of the road texture covers.
    #[serde(default = "two")]
    pub repeat: f32,
    /// The ground beside the road, and how far it reaches on each side.
    pub shoulder: String,
    #[serde(default = "forty")]
    pub shoulder_width: f32,
    #[serde(default = "four")]
    pub shoulder_repeat: f32,
    /// Lane markings (sRGB); dashed between lanes, solid at the edges.
    #[serde(default = "paint")]
    pub paint: (u8, u8, u8),
}

fn two() -> f32 {
    2.0
}
fn forty() -> f32 {
    40.0
}
fn four() -> f32 {
    4.0
}
fn paint() -> (u8, u8, u8) {
    (235, 232, 220)
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cam {
    /// Eye height above the road.
    pub height: f32,
    /// How far behind the followed entity's cell the eye sits.
    #[serde(default)]
    pub back: f32,
    /// Vertical field of view, degrees.
    pub fov: f32,
    /// Spring that carries the eye across lanes.
    #[serde(default = "lane_spring")]
    pub lane: CameraFeel,
    /// Degrees of bank per lane-per-second of sideways speed, at most `max_bank`; the roll follows on a spring.
    #[serde(default = "bank")]
    pub bank: f32,
    #[serde(default = "max_bank")]
    pub max_bank: f32,
    /// Road vibration: world units of jitter at full speed.
    #[serde(default)]
    pub rumble: f32,
    /// Degrees the view looks down.
    #[serde(default)]
    pub pitch: f32,
}

/// How the followed entity moves between positions. The game decides *where* (a prop, e.g. `lane_to`, set by
/// the player's action) and how fast it gets there in cells; this decides how the move *feels*: where the positions
/// lie, how long a switch takes, its curve, how the camera banks, hops and punches, scaled by the lanes crossed.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Switch {
    /// The followed entity's prop that holds the position it is heading to (a lane index).
    pub prop: String,
    /// World X of each position (across the road); empty = the lane centres.
    #[serde(default)]
    pub positions: Vec<f32>,
    /// Seconds: a switch takes `base + per_lane × lanes crossed`.
    pub base: f32,
    pub per_lane: f32,
    /// The curve of the move (e.g. `back_out`: overshoot and settle; `cubic_out`: fast then soft).
    pub ease: Ease,
    /// Camera lift at the middle of the move, per lane crossed (a small hop on a long switch).
    #[serde(default)]
    pub hop: f32,
    /// Effect when a switch starts and when it arrives (`fx` channels), scaled by the lanes crossed.
    #[serde(default)]
    pub start: Anim,
    #[serde(default)]
    pub arrive: Anim,
}

/// A switch in progress.
#[derive(Clone, Copy, Debug)]
pub struct SwitchRun {
    pub from: f32,
    pub to: f32,
    pub start: f32,
    pub dur: f32,
    pub lanes: f32,
    pub ease: Ease,
}

impl SwitchRun {
    pub fn at(&self, now: f32) -> (f32, f32) {
        let u = ((now - self.start) / self.dur.max(1e-3)).clamp(0.0, 1.0);
        (self.from + (self.to - self.from) * self.ease.apply(u), u)
    }
}

/// Another way to look at the road: a chase camera, a low bumper camera...
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct View {
    pub name: String,
    pub height: f32,
    pub back: f32,
    pub fov: f32,
    #[serde(default)]
    pub pitch: f32,
    /// Show the hood (a view from the driver's seat).
    #[serde(default)]
    pub hood: bool,
    /// Draw the followed entity with this image and height (a view from outside).
    #[serde(default)]
    pub body: Option<(String, f32)>,
}

fn lane_spring() -> CameraFeel {
    CameraFeel { stiffness: 90.0, damping: 0.9 }
}
fn bank() -> f32 {
    1.2
}
fn max_bank() -> f32 {
    9.0
}

/// How a kind looks on the track: frames per state (as on the stage), size, and whether it stands or lies flat.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prop {
    pub frames: Vec<String>,
    #[serde(default)]
    pub states: BTreeMap<String, Vec<String>>,
    /// Standing: height (width from the picture). Flat: length along the road.
    pub height: f32,
    /// Hovers this far above the road (a coin).
    #[serde(default)]
    pub lift: f32,
    /// Lies on the road (oil, a painted pad) instead of standing.
    #[serde(default)]
    pub flat: bool,
    /// Turns about its upright axis (a coin), turns per second.
    #[serde(default)]
    pub spin: f32,
    /// Bobs up and down (world units), for pickups.
    #[serde(default)]
    pub bob: f32,
    #[serde(default = "eight")]
    pub fps: f32,
    /// Not drawn when it is the followed entity (the car the camera sits in).
    #[serde(default)]
    pub hide_followed: bool,
    /// When one disappears near the camera (smashed, picked up), it keeps playing this: channels `x` (world units
    /// sideways, away from the followed entity), `y` (up), `rot` (degrees), `scale`, `alpha`.
    #[serde(default)]
    pub gone: Option<Anim>,
    /// How much of the followed entity's speed a gone one is carried along with (a smashed cone flies ahead).
    #[serde(default)]
    pub push: f32,
}

fn eight() -> f32 {
    8.0
}

/// Roadside decoration: nothing to do with the rules, everything to do with speed (things rushing past).
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenery {
    pub images: Vec<String>,
    pub height: (f32, f32),
    /// One every this many rows on average, each side.
    pub every: f32,
    /// Distance from the road's edge, from .. to.
    pub side: (f32, f32),
    #[serde(default)]
    pub seed: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hood {
    pub image: String,
    /// Height as a fraction of the screen.
    pub height: f32,
    /// How much of the camera's shake and lift it follows (0 = fixed to the screen, 1 = rides with the view).
    #[serde(default = "half")]
    pub follow: f32,
}

fn half() -> f32 {
    0.5
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Meter {
    pub prop: String,
    pub icon: String,
    pub max: i64,
    /// Left end, fractions of the screen; icons run to the right.
    pub at: (f32, f32),
    /// Icon height as a fraction of the screen height.
    pub size: f32,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bar {
    pub prop: String,
    pub max: i64,
    pub at: (f32, f32),
    /// Width and height as fractions of the screen.
    pub size: (f32, f32),
    pub color: (u8, u8, u8),
    /// Below this fraction the bar pulses (low fuel).
    #[serde(default)]
    pub warn: f32,
}

/// What the camera remembers between frames.
#[derive(Clone, Debug, Default)]
pub struct Rig {
    pub lane: Spring,
    /// The chosen view (0 = the main camera), and the springs that carry the camera to it: height, back, fov,
    /// pitch, hood (0..1), body (0..1).
    pub view: usize,
    pub blend: [Spring; 6],
    pub fx: FxState,
    /// Where the followed entity was last seen (it may be gone at the end).
    pub last: Option<(f32, f32)>,
    /// The position it was last heading to (the switch prop), the switch in progress, and the last drawn X.
    pub heading: Option<i64>,
    pub switch: Option<SwitchRun>,
    pub drawn_x: Option<f32>,
    /// The camera's bank, eased (degrees).
    pub bank: Spring,
    /// Things seen last frame (for the ones that vanish) and the vanished ones still animating.
    pub seen: BTreeMap<EntityId, (String, f32, f32)>,
    pub ghosts: Vec<Ghost>,
    /// The followed entity's forward speed (world units a second), smoothed.
    pub speed: f32,
    /// Per meter: when each icon appeared, icons being lost (slot, when), when it last shook.
    pub meters: Vec<MeterState>,
}

/// Something that vanished, still animating where it was.
#[derive(Clone, Debug)]
pub struct Ghost {
    pub kind: String,
    pub image: String,
    pub x: f32,
    pub z: f32,
    pub since: f32,
    pub dir: f32,
    pub speed: f32,
}

#[derive(Clone, Debug, Default)]
pub struct MeterState {
    pub born: Vec<f32>,
    pub lost: Vec<(usize, f32)>,
    pub shook: Option<f32>,
}

/// A frame of the track: 2D behind, the 3D world, 2D in front.
pub struct Frame {
    pub back: Vec<Quad>,
    pub eye: Eye,
    pub fog: [f32; 3],
    pub meshes: Vec<Mesh>,
    /// Skinned, instanced models (roam views).
    pub models: Vec<crate::skin::ModelDraw>,
    pub front: Vec<Quad>,
}

fn rgb(c: (u8, u8, u8)) -> [f32; 3] {
    [c.0 as f32 / 255.0, c.1 as f32 / 255.0, c.2 as f32 / 255.0]
}

fn quad2(image: &str, x: f32, y: f32, w: f32, h: f32, c: [f32; 4]) -> Quad {
    Quad {
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
    }
}

/// Two triangles for a quad given four corners (a b c d, around) with their texture coordinates.
fn tri2(v: &mut Vec<Vert3>, p: [V3; 4], uv: [[f32; 2]; 4], color: [f32; 4], fog: [f32; 4]) {
    for i in [0usize, 1, 2, 0, 2, 3] {
        v.push(Vert3 { pos: [p[i].0, p[i].1, p[i].2], uv: uv[i], color, fog: fog[i] });
    }
}

pub struct TrackComposer<'a> {
    pub track: &'a Track,
    pub game: &'a Game,
    pub sizes: &'a Sizes,
    pub w: f32,
    pub h: f32,
}

impl TrackComposer<'_> {
    fn aspect(&self, image: &str) -> f32 {
        self.sizes.get(image).map_or(1.0, |(w, h)| *w as f32 / (*h).max(1) as f32)
    }

    /// The world X of a lane (fractional between ticks): the switch's positions if given, else lane centres.
    pub fn lane_x(&self, x: f32) -> f32 {
        match self.track.switch.as_ref().filter(|s| !s.positions.is_empty()) {
            Some(s) => {
                let p = &s.positions;
                let i = (x.floor().max(0.0) as usize).min(p.len() - 1);
                let j = (i + 1).min(p.len() - 1);
                p[i] + (p[j] - p[i]) * (x - i as f32).clamp(0.0, 1.0)
            }
            None => x + 0.5 - self.track.road.lanes as f32 / 2.0,
        }
    }

    fn fog_at(&self, dist: f32) -> f32 {
        let (_, start, end) = self.track.fog;
        ((dist - start) / (end - start).max(0.01)).clamp(0.0, 1.0)
    }

    fn frames<'b>(&self, p: &'b Prop, e: &Entity) -> &'b [String] {
        let label = self.game.state_label(e);
        p.states
            .iter()
            .filter(|(sel, _)| sim_state::in_label(label, sel))
            .min_by_key(|(sel, _)| sim_state::depth_in_label(label, sel))
            .map_or(&p.frames, |(_, f)| f)
    }

    /// The followed entity's position (x across, z along), between ticks.
    pub fn followed(&self, world: &World, tween: Option<&Tween>) -> Option<(EntityId, f32, f32)> {
        let e = world.of_kind(&self.track.follow).next()?;
        let (x, y, _) = tween.map_or((e.x as f32, e.y as f32, 0.0), |t| t.at(e.id, (e.x, e.y, e.z)));
        Some((e.id, self.lane_x(x), y + 0.5))
    }

    /// Every view: the main camera first, then `views`.
    pub fn views(&self) -> Vec<View> {
        let c = &self.track.camera;
        let main = View {
            name: "driver".into(),
            height: c.height,
            back: c.back,
            fov: c.fov,
            pitch: c.pitch,
            hood: self.track.hood.is_some(),
            body: None,
        };
        std::iter::once(main).chain(self.track.views.iter().cloned()).collect()
    }

    /// Composes the frame. `rig` carries the springs and the running effects; `dt` moves them.
    pub fn compose(&self, world: &World, tween: Option<&Tween>, rig: &mut Rig, time: f32, dt: f32) -> Frame {
        let tr = self.track;
        let views = self.views();
        let view = &views[rig.view % views.len()];
        let glide = CameraFeel { stiffness: 14.0, damping: 1.0 };
        let targets =
            [view.height, view.back, view.fov, view.pitch, if view.hood { 1.0 } else { 0.0 }, if view.body.is_some() { 1.0 } else { 0.0 }];
        let mut cur = [0.0f32; 6];
        for i in 0..6 {
            cur[i] = rig.blend[i].update(targets[i], dt, glide);
        }
        let [cam_h, cam_back, cam_fov, cam_pitch, hood_k, body_k] = cur;
        // The body shown from outside: the chosen view's, else the last view that had one (while it fades out).
        let body = view.body.clone().or_else(|| views.iter().find_map(|v| v.body.clone()));
        let followed = self.followed(world, tween);
        if let Some((_, x, z)) = followed {
            if let Some((_, pz)) = rig.last
                && dt > 0.0
            {
                let v = ((z - pz) / dt).clamp(0.0, 80.0);
                rig.speed += (v - rig.speed) * (dt * 6.0).min(1.0);
            }
            rig.last = Some((x, z));
        }
        let (fx_target, cz) = rig.last.unwrap_or((0.0, 0.0));
        // The move between positions: when the heading changes, a switch runs from wherever the camera is now.
        let mut hop = 0.0;
        let heading =
            world.of_kind(&tr.follow).next().zip(tr.switch.as_ref()).and_then(|(e, s)| e.props.get(&s.prop).copied()).filter(|h| *h >= 0);
        if let (Some(sw), Some(h)) = (&tr.switch, heading)
            && rig.heading != Some(h)
        {
            if let Some(prev) = rig.heading {
                let lanes = (h - prev).abs().max(1) as f32;
                let from = rig.drawn_x.unwrap_or(fx_target);
                let dur = sw.base + sw.per_lane * lanes;
                rig.switch = Some(SwitchRun { from, to: self.lane_x(h as f32), start: time, dur, lanes, ease: sw.ease });
                rig.fx.play(&sw.start, time, lanes);
                rig.fx.play(&sw.arrive, time + dur, lanes);
            }
            rig.heading = Some(h);
        }
        // Between switches the camera holds the position it is headed to (the simulation converges on it cell by
        // cell); without a switch it follows the entity's cell.
        let fx_target = heading.map_or(fx_target, |h| self.lane_x(h as f32));
        let before = rig.drawn_x.unwrap_or(fx_target);
        let cx = match rig.switch {
            Some(run) if time - run.start < run.dur => {
                let (x, u) = run.at(time);
                hop = tr.switch.as_ref().map_or(0.0, |s| s.hop * run.lanes * (u * std::f32::consts::PI).sin());
                rig.lane.pos = x;
                rig.lane.vel = 0.0;
                x
            }
            _ => {
                rig.switch = None;
                rig.lane.update(fx_target, dt, tr.camera.lane)
            }
        };
        rig.drawn_x = Some(cx);
        let side_speed = if dt > 0.0 { (cx - before) / dt } else { 0.0 };
        let lean = (-side_speed * tr.camera.bank).clamp(-tr.camera.max_bank, tr.camera.max_bank);
        let bank = rig.bank.update(lean, dt, CameraFeel { stiffness: 70.0, damping: 0.75 });
        let mut fx: Fx = rig.fx.sample(time);
        fx.lift += hop;
        let rumble = tr.camera.rumble + fx.shake;
        let (jx, jy) = (jitter(time, 1.3) * rumble, jitter(time, 7.9) * rumble * 0.6);
        let eye_y = cam_h + fx.lift + jy;
        let eye_z = cz - cam_back;
        let pitch = (cam_pitch + fx.pitch).to_radians();
        let eye = Eye {
            pos: V3(cx + jx, eye_y, eye_z),
            target: V3(cx + jx * 0.5, eye_y - pitch.tan() * 10.0, eye_z + 10.0),
            roll: bank + fx.roll,
            fov: (cam_fov + fx.fov).clamp(20.0, 140.0),
            near: 0.05,
            far: tr.view + 20.0,
        };
        let fogc = rgb(tr.fog.0);

        // Behind: the sky gradient (fog colour at the horizon), the painted sky, the skyline on the horizon.
        let mut back = Vec::new();
        let horizon = eye.project(V3(eye.pos.0, eye.pos.1, eye.pos.2 + 10_000.0), self.w, self.h).map_or(self.h / 2.0, |p| p.1);
        let top = [fogc[0] * 0.55, fogc[1] * 0.6, fogc[2] * 0.9, 1.0];
        back.push(Quad { top, bottom: [fogc[0], fogc[1], fogc[2], 1.0], ..quad2(WHITE, 0.0, 0.0, self.w, self.h, top) });
        if let Some(sky) = &tr.sky {
            let hgt = horizon.max(1.0) + self.h * 0.06;
            let span = (self.w / (hgt * self.aspect(sky))).min(1.0);
            let u0 = (1.0 - span) * (0.5 + 0.5 * (cx * 0.01).sin());
            back.push(Quad { uv: [u0, 0.0, u0 + span, 1.0], ..quad2(sky, 0.0, 0.0, self.w, hgt, [1.0; 4]) });
        }
        if let Some(sl) = &tr.skyline {
            let hh = sl.height * self.h;
            let ww = hh * self.aspect(&sl.image);
            let u0 = cx * sl.drift / ww * self.w * 0.01 + time * 0.0;
            let span = self.w / ww;
            back.push(Quad {
                uv: [u0, 0.0, u0 + span, 1.0],
                wrap: Wrap::RepeatX,
                ..quad2(&sl.image, 0.0, horizon - hh * 0.97, self.w, hh, [1.0, 1.0, 1.0, 1.0])
            });
        }

        // The 3D world.
        let mut meshes: Vec<Mesh> = Vec::new();
        let z0 = cz - 3.0;
        let z1 = cz + tr.view;
        let half = tr.road.lanes as f32 / 2.0;
        let fog = |z: f32| self.fog_at(z - eye_z);
        let white = [1.0f32; 4];
        // Ground beside the road, then the road, then its paint (all flat, near-to-far fog).
        let mut ground = Vec::new();
        let sw = tr.road.shoulder_width;
        let rep = tr.road.shoulder_repeat;
        for (xa, xb) in [(-half - sw, -half), (half, half + sw)] {
            tri2(
                &mut ground,
                [V3(xa, 0.0, z0), V3(xb, 0.0, z0), V3(xb, 0.0, z1), V3(xa, 0.0, z1)],
                [[xa / rep, z0 / rep], [xb / rep, z0 / rep], [xb / rep, z1 / rep], [xa / rep, z1 / rep]],
                white,
                [fog(z0), fog(z0), fog(z1), fog(z1)],
            );
        }
        meshes.push(Mesh { image: tr.road.shoulder.clone(), wrap: Wrap::Repeat, verts: ground });
        let mut road = Vec::new();
        let r = tr.road.repeat;
        tri2(
            &mut road,
            [V3(-half, 0.0, z0), V3(half, 0.0, z0), V3(half, 0.0, z1), V3(-half, 0.0, z1)],
            [[0.0, z0 / r], [tr.road.lanes as f32 / r, z0 / r], [tr.road.lanes as f32 / r, z1 / r], [0.0, z1 / r]],
            white,
            [fog(z0), fog(z0), fog(z1), fog(z1)],
        );
        meshes.push(Mesh { image: tr.road.image.clone(), wrap: Wrap::Repeat, verts: road });
        let mut paint = Vec::new();
        let pc = rgb(tr.road.paint);
        let pc = [pc[0], pc[1], pc[2], 0.9];
        let line = |v: &mut Vec<Vert3>, x: f32, za: f32, zb: f32, wdt: f32| {
            tri2(
                v,
                [V3(x - wdt, 0.004, za), V3(x + wdt, 0.004, za), V3(x + wdt, 0.004, zb), V3(x - wdt, 0.004, zb)],
                [[0.0, 0.0]; 4],
                pc,
                [fog(za), fog(za), fog(zb), fog(zb)],
            );
        };
        for edge in [-half + 0.08, half - 0.08] {
            line(&mut paint, edge, z0, z1, 0.035);
        }
        for l in 1..tr.road.lanes {
            let x = -half + l as f32;
            let mut z = z0.floor() - z0.floor().rem_euclid(2.0);
            while z < z1 {
                line(&mut paint, x, z, z + 1.0, 0.03);
                z += 2.0;
            }
        }
        meshes.push(Mesh { image: WHITE.into(), wrap: Wrap::Clamp, verts: paint });

        // Things on the road and beside it, far to near (blending needs back to front).
        struct Thing {
            z: f32,
            mesh: Mesh,
        }
        let mut things: Vec<Thing> = Vec::new();
        let follow_id = followed.map(|f| f.0);
        let mut flats: Vec<Mesh> = Vec::new();
        for e in world.entities().values() {
            let Some(p) = tr.kinds.get(&e.kind) else { continue };
            if self.game.is_hidden(&e.kind) || (p.hide_followed && Some(e.id) == follow_id) {
                continue;
            }
            let (x, y, _) = tween.map_or((e.x as f32, e.y as f32, 0.0), |t| t.at(e.id, (e.x, e.y, e.z)));
            let (x, z) = (self.lane_x(x), y + 0.5);
            if z < z0 || z > z1 {
                continue;
            }
            let frames = self.frames(p, e);
            let Some(first) = frames.first() else { continue };
            let n = ((time * p.fps) as usize + e.id as usize) % frames.len();
            let image = &frames[n];
            let f = fog(z);
            let mut v = Vec::new();
            if p.flat {
                let wd = p.height * self.aspect(first) / 2.0;
                let ln = p.height / 2.0;
                tri2(
                    &mut v,
                    [V3(x - wd, 0.006, z - ln), V3(x + wd, 0.006, z - ln), V3(x + wd, 0.006, z + ln), V3(x - wd, 0.006, z + ln)],
                    [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
                    white,
                    [f; 4],
                );
                flats.push(Mesh { image: image.clone(), wrap: Wrap::Clamp, verts: v });
            } else {
                let spin = if p.spin > 0.0 { (time * p.spin * std::f32::consts::TAU + e.id as f32).cos() } else { 1.0 };
                let wd = p.height * self.aspect(first) / 2.0 * spin.abs().max(0.08);
                let lift = p.lift + p.bob * (time * 2.5 + e.id as f32).sin();
                let (u0, u1) = if spin < 0.0 { (1.0, 0.0) } else { (0.0, 1.0) };
                // A soft shadow on the road under anything standing.
                let mut sh = Vec::new();
                let sr = wd.max(0.15) * 0.9;
                tri2(
                    &mut sh,
                    [
                        V3(x - sr, 0.005, z - sr * 0.5),
                        V3(x + sr, 0.005, z - sr * 0.5),
                        V3(x + sr, 0.005, z + sr * 0.5),
                        V3(x - sr, 0.005, z + sr * 0.5),
                    ],
                    [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
                    [0.0, 0.0, 0.0, 0.35 * (1.0 - f)],
                    [f; 4],
                );
                flats.push(Mesh { image: crate::stage::BLOB.into(), wrap: Wrap::Clamp, verts: sh });
                tri2(
                    &mut v,
                    [V3(x - wd, lift, z), V3(x + wd, lift, z), V3(x + wd, lift + p.height, z), V3(x - wd, lift + p.height, z)],
                    [[u0, 1.0], [u1, 1.0], [u1, 0.0], [u0, 0.0]],
                    white,
                    [f; 4],
                );
                things.push(Thing { z, mesh: Mesh { image: image.clone(), wrap: Wrap::Clamp, verts: v } });
            }
        }
        for s in &tr.scenery {
            let start = (z0 / s.every).floor() as i64;
            let end = (z1 / s.every).ceil() as i64;
            for k in start..=end {
                for side in [-1.0f32, 1.0] {
                    let n = sim_render::pixel::noise(k, side as i64, s.seed);
                    let z = (k as f32 + (n % 1000) as f32 / 1000.0) * s.every;
                    if z < z0 || z > z1 {
                        continue;
                    }
                    let d = s.side.0 + (n / 1000 % 1000) as f32 / 1000.0 * (s.side.1 - s.side.0);
                    let x = side * (half + d);
                    let image = &s.images[(n / 1_000_000 % s.images.len() as u64) as usize];
                    let hgt = s.height.0 + (n / 7 % 1000) as f32 / 1000.0 * (s.height.1 - s.height.0);
                    let wd = hgt * self.aspect(image) / 2.0;
                    let f = fog(z);
                    let flip = n.is_multiple_of(2);
                    let (u0, u1) = if flip { (1.0, 0.0) } else { (0.0, 1.0) };
                    let mut v = Vec::new();
                    tri2(
                        &mut v,
                        [V3(x - wd, 0.0, z), V3(x + wd, 0.0, z), V3(x + wd, hgt, z), V3(x - wd, hgt, z)],
                        [[u0, 1.0], [u1, 1.0], [u1, 0.0], [u0, 0.0]],
                        white,
                        [f; 4],
                    );
                    things.push(Thing { z, mesh: Mesh { image: image.clone(), wrap: Wrap::Clamp, verts: v } });
                }
            }
        }
        // Vanished things near the road ahead become ghosts that play their `gone` animation.
        let mut now_seen: BTreeMap<EntityId, (String, f32, f32)> = BTreeMap::new();
        for e in world.entities().values() {
            if let Some(p) = tr.kinds.get(&e.kind)
                && p.gone.is_some()
            {
                let (x, z) = (self.lane_x(e.x as f32), e.y as f32 + 0.5);
                if z >= z0 && z <= z1 {
                    now_seen.insert(e.id, (e.kind.clone(), x, z));
                }
            }
        }
        for (id, (kind, x, z)) in std::mem::take(&mut rig.seen) {
            if !now_seen.contains_key(&id) && world.get(id).is_none() {
                let image = tr.kinds[&kind].frames.first().cloned().unwrap_or_default();
                let dir = if (x - cx).abs() > 0.2 {
                    (x - cx).signum()
                } else if id % 2 == 0 {
                    1.0
                } else {
                    -1.0
                };
                rig.ghosts.push(Ghost { kind, image, x, z, since: time, dir, speed: rig.speed });
            }
        }
        rig.seen = now_seen;
        rig.ghosts.retain(|g| tr.kinds.get(&g.kind).and_then(|p| p.gone.as_ref()).is_some_and(|a| !a.done(time - g.since)));
        for g in &rig.ghosts {
            let p = &tr.kinds[&g.kind];
            let t = time - g.since;
            let pose = p.gone.as_ref().expect("kept only with one").sample(t);
            let z = g.z + g.speed * p.push * t;
            let x = g.x + g.dir * pose.x;
            let hgt = p.height * pose.scale;
            let wd = hgt * self.aspect(&g.image) / 2.0;
            let (cy, rot) = (p.lift + pose.y + hgt / 2.0, (pose.rot * g.dir).to_radians());
            let (sn, cs) = rot.sin_cos();
            let corner = |dx: f32, dy: f32| V3(x + dx * cs - dy * sn, (cy + dx * sn + dy * cs).max(0.0), z);
            let f = fog(z);
            let mut v = Vec::new();
            tri2(
                &mut v,
                [corner(-wd, -hgt / 2.0), corner(wd, -hgt / 2.0), corner(wd, hgt / 2.0), corner(-wd, hgt / 2.0)],
                [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
                [1.0, 1.0, 1.0, pose.alpha.clamp(0.0, 1.0)],
                [f; 4],
            );
            things.push(Thing { z, mesh: Mesh { image: g.image.clone(), wrap: Wrap::Clamp, verts: v } });
        }

        if let (Some((image, height)), Some((_, _, z))) = (&body, followed)
            && body_k > 0.01
        {
            // It rides the switch curve, like the camera.
            let x = cx;
            // Seen from outside, the followed entity rides in its lane, on the jump arc, leaning into lane changes.
            let wd = height * self.aspect(image) / 2.0;
            let lift = fx.lift;
            let mut v = Vec::new();
            let lean = (bank * 0.012).clamp(-0.12, 0.12);
            let blinking = tr
                .blink
                .as_ref()
                .is_some_and(|b| world.of_kind(&tr.follow).next().is_some_and(|e| sim_state::in_label(self.game.state_label(e), b)));
            let a = body_k * if blinking { 0.35 + 0.65 * (time * 22.0).sin().abs() } else { 1.0 };
            tri2(
                &mut v,
                [V3(x - wd, lift, z), V3(x + wd, lift, z), V3(x + wd + lean, lift + height, z), V3(x - wd + lean, lift + height, z)],
                [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
                [1.0, 1.0, 1.0, a],
                [0.0; 4],
            );
            let mut sh = Vec::new();
            let sr = wd * 1.05;
            tri2(
                &mut sh,
                [V3(x - sr, 0.005, z - 0.35), V3(x + sr, 0.005, z - 0.35), V3(x + sr, 0.005, z + 0.35), V3(x - sr, 0.005, z + 0.35)],
                [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
                [0.0, 0.0, 0.0, 0.45 * body_k / (1.0 + lift * 2.0)],
                [0.0; 4],
            );
            flats.push(Mesh { image: crate::stage::BLOB.into(), wrap: Wrap::Clamp, verts: sh });
            things.push(Thing { z, mesh: Mesh { image: image.clone(), wrap: Wrap::Clamp, verts: v } });
        }
        meshes.extend(flats);
        things.sort_by(|a, b| b.z.total_cmp(&a.z));
        meshes.extend(things.into_iter().map(|t| t.mesh));

        // In front: the hood (it rides with part of the camera's motion), effects, meters, buttons.
        let mut front = Vec::new();
        if let Some(hood) = tr.hood.as_ref().filter(|_| hood_k > 0.01) {
            // At least as wide as the screen (a little wider, so its edges never show when it sways).
            let a = self.aspect(&hood.image);
            let hh = (hood.height * self.h).max(self.w * 1.12 / a);
            let ww = hh * a;
            // Sliding down out of view as the camera leaves the driver's seat.
            let away = (1.0 - hood_k) * hh * 1.2;
            let k = hood.follow;
            let dy = (-fx.lift * 0.35 + jy * 3.0) * self.h * 0.2 * k;
            let dx = jx * self.w * 0.05 * k;
            front.push(Quad {
                rot: -(bank + fx.roll) * 0.5 * k,
                ..quad2(&hood.image, (self.w - ww) / 2.0 + dx, self.h - hh + dy + hh * 0.04 + away, ww, hh, [1.0; 4])
            });
        }
        front.extend(overlay(&fx, self.w, self.h, time));
        let me = world.of_kind(&tr.follow).next();
        let prop = |name: &str| me.and_then(|e| e.props.get(name).copied()).unwrap_or(0);
        rig.meters.resize(tr.meters.len(), MeterState::default());
        for (m, st) in tr.meters.iter().zip(rig.meters.iter_mut()) {
            let n = prop(&m.prop).clamp(0, m.max) as usize;
            // Count changes animate: new icons pop in, lost ones burst off, the rest shake.
            let first = st.born.is_empty() && st.lost.is_empty() && st.shook.is_none();
            if n > st.born.len() {
                let t = if first { time - 10.0 } else { time };
                st.born.resize(n, t);
            } else if n < st.born.len() {
                for slot in n..st.born.len() {
                    st.lost.push((slot, time));
                }
                st.born.truncate(n);
                st.shook = Some(time);
            }
            st.lost.retain(|(_, at)| time - at < 0.6);
            let s = m.size * self.h;
            let a = self.aspect(&m.icon);
            let shake = st.shook.map_or(0.0, |at| {
                let t = time - at;
                if t < 0.4 { (t * 60.0).sin() * (0.4 - t) * s * 0.25 } else { 0.0 }
            });
            let place = |i: usize| (m.at.0 * self.w + i as f32 * s * a * 0.8, m.at.1 * self.h);
            for (i, born) in st.born.iter().enumerate() {
                let k = sim_render::anim::Ease::BackOut.apply(((time - born) / 0.3).clamp(0.0, 1.0));
                let (x, y) = place(i);
                let (w, h) = (s * a * k, s * k);
                front.push(quad2(&m.icon, x + (s * a - w) / 2.0 + shake, y + (s - h) / 2.0, w, h, [1.0; 4]));
            }
            for (slot, at) in &st.lost {
                let t = ((time - at) / 0.6).clamp(0.0, 1.0);
                let k = 1.0 + t * 1.2;
                let (x, y) = place(*slot);
                let (w, h) = (s * a * k, s * k);
                front.push(Quad {
                    rot: t * 50.0,
                    ..quad2(&m.icon, x + (s * a - w) / 2.0, y + (s - h) / 2.0 - t * s * 0.6, w, h, [1.0, 0.55, 0.5, 1.0 - t])
                });
            }
        }
        for b in &tr.bars {
            let frac = (prop(&b.prop) as f32 / b.max.max(1) as f32).clamp(0.0, 1.0);
            let (x, y, bw, bh) = (b.at.0 * self.w, b.at.1 * self.h, b.size.0 * self.w, b.size.1 * self.h);
            let c = rgb(b.color);
            let pulse = if frac < b.warn { 0.55 + 0.45 * (time * 8.0).sin().abs() } else { 1.0 };
            front.push(quad2(WHITE, x - 3.0, y - 3.0, bw + 6.0, bh + 6.0, [0.0, 0.0, 0.0, 0.45]));
            front.push(quad2(WHITE, x, y, bw * frac, bh, [c[0], c[1], c[2], pulse]));
        }
        Frame { back, eye, fog: fogc, meshes, models: Vec::new(), front }
    }
}

/// Buttons over a track frame (same buttons as the stage: actions, keys, animations).
/// Buttons in one place (Enter = refuel here, stamp there) show as one: the one that is live, else the first.
pub fn button_quads(buttons: &[Button], states: &mut [ButtonState], w: f32, h: f32, now: f32) -> Vec<Quad> {
    let shown: Vec<bool> = (0..buttons.len())
        .map(|i| {
            let same: Vec<usize> = (0..buttons.len()).filter(|&j| buttons[j].at == buttons[i].at).collect();
            let live = same.iter().copied().find(|&j| states[j].enabled || states[j].player.current.is_some());
            live.unwrap_or(same[0]) == i
        })
        .collect();
    buttons.iter().zip(states.iter_mut()).zip(shown).filter(|(_, s)| *s).flat_map(|((b, st), _)| b.quads(st, w, h, now)).collect()
}

/// A game on its track: the engine, the camera rig, the buttons. What a window, a screenshot and a recording share.
pub struct TrackPlay {
    pub engine: sim_core::Engine<sim_core::Running, Game>,
    pub track: Track,
    pub sizes: Sizes,
    pub tween: Tween,
    pub rig: Rig,
    pub buttons: Vec<ButtonState>,
    pub time: f32,
    /// The last refusal (action, why, when), for the title.
    pub refused: Option<(String, String, f32)>,
    /// Every accepted press: (tick it was made at, action, args). With the seed, the whole run.
    pub log: Vec<Press>,
    /// Replaying: the presses to make, in order (the player's keys are ignored).
    pub script: Option<Vec<Press>>,
}

/// One accepted press, as a replay needs it.
#[derive(Clone, Debug, PartialEq, serde::Serialize, Deserialize)]
pub struct Press {
    pub tick: u64,
    pub action: String,
    #[serde(default)]
    pub args: BTreeMap<String, i64>,
}

/// A run as JSON lines: one press per line (the seed and panel come from the game).
pub fn save_run(path: &std::path::Path, log: &[Press]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text: String = log.iter().map(|p| serde_json::to_string(p).expect("serialisable") + "\n").collect();
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn load_run(path: &std::path::Path) -> Result<Vec<Press>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    text.lines().filter(|l| !l.trim().is_empty()).map(|l| serde_json::from_str(l).map_err(|e| format!("{}: {e}", path.display()))).collect()
}

impl TrackPlay {
    pub fn new(engine: sim_core::Engine<sim_core::Running, Game>, track: Track, sizes: Sizes) -> TrackPlay {
        let n = track.buttons.len();
        TrackPlay {
            engine,
            track,
            sizes,
            tween: Tween::default(),
            rig: Rig::default(),
            buttons: vec![ButtonState::default(); n],
            time: 0.0,
            refused: None,
            log: Vec::new(),
            script: None,
        }
    }

    pub fn world(&self) -> &World {
        self.engine.world()
    }

    /// Runs `n` ticks; the followed entity's events start camera effects.
    pub fn step(&mut self, n: u32) {
        for _ in 0..n {
            if self.engine.outcome().is_some() || self.world().tick >= self.engine.rules().cfg.run.max_ticks {
                return;
            }
            let me = self.world().of_kind(&self.track.follow).next().map(|e| e.id);
            // A replay makes the recorded presses at their ticks (the same checks as the player's).
            if let Some(script) = &self.script {
                let tick = self.world().tick;
                let due: Vec<Press> = script.iter().filter(|p| p.tick == tick).cloned().collect();
                for p in due {
                    if let Some(id) = me
                        && let Ok(group) = self.engine.rules().act(self.world(), None, id, &p.action, &p.args)
                    {
                        self.engine.queue(group);
                        if let Some(b) = self.track.buttons.iter().position(|b| b.action == p.action && b.args == p.args) {
                            self.buttons[b].player.play("press", self.time);
                        }
                        self.log.push(p);
                    }
                }
            }
            self.tween.remember(self.engine.world());
            let report = self.engine.tick();
            for ev in report.events {
                if Some(ev.entity) == me {
                    self.rig.fx.trigger(&self.track.fx, &ev.name, self.time);
                }
            }
        }
    }

    fn allowed(&self, b: usize) -> Result<sim_core::Group, String> {
        let btn = &self.track.buttons[b];
        let id = self.world().of_kind(&btn.on).next().map(|e| e.id).ok_or_else(|| format!("no {} in the world", btn.on))?;
        self.engine.rules().act(self.world(), None, id, &btn.action, &btn.args)
    }

    /// Presses button `b`: queued for the next tick, or refused (the button shakes).
    pub fn press(&mut self, b: usize) {
        if self.script.is_some() {
            return; // a replay plays itself
        }
        match self.allowed(b) {
            Ok(group) => {
                self.engine.queue(group);
                self.buttons[b].player.play("press", self.time);
                let btn = &self.track.buttons[b];
                self.log.push(Press { tick: self.world().tick, action: btn.action.clone(), args: btn.args.clone() });
            }
            Err(why) => {
                self.buttons[b].player.play("denied", self.time);
                self.refused = Some((self.track.buttons[b].action.clone(), why, self.time));
            }
        }
    }

    /// A key: the first of its buttons the game would take now (Enter may mean refuel here and stamp there);
    /// if none, the first one is pressed and refuses. Returns whether any button has this key.
    pub fn key(&mut self, key: &str) -> bool {
        let mine: Vec<usize> =
            self.track.buttons.iter().enumerate().filter(|(_, b)| b.key.as_deref() == Some(key)).map(|(i, _)| i).collect();
        let Some(&first) = mine.first() else { return false };
        let pick = mine.iter().copied().find(|&b| self.allowed(b).is_ok()).unwrap_or(first);
        self.press(pick);
        true
    }

    /// The next camera view (`c`).
    pub fn cycle_view(&mut self) {
        self.rig.view = (self.rig.view + 1) % (1 + self.track.views.len());
    }

    pub fn hover(&mut self, w: f32, h: f32, x: f32, y: f32) {
        for (i, b) in self.track.buttons.iter().enumerate() {
            let over = b.hit(w, h, x, y);
            let st = &mut self.buttons[i];
            match (over, st.hover_since) {
                (true, None) => st.hover_since = Some(self.time),
                (false, Some(_)) => st.hover_since = None,
                _ => {}
            }
        }
    }

    pub fn click(&mut self, w: f32, h: f32, x: f32, y: f32) {
        if let Some(b) = self.track.buttons.iter().position(|b| b.hit(w, h, x, y)) {
            self.press(b);
        }
    }

    /// The frame for a `w × h` target; `dt` since the last one.
    pub fn frame(&mut self, w: f32, h: f32, dt: f32) -> Frame {
        for i in 0..self.track.buttons.len() {
            self.buttons[i].enabled = self.allowed(i).is_ok();
        }
        let composer = TrackComposer { track: &self.track, game: self.engine.rules(), sizes: &self.sizes, w, h };
        let mut f = composer.compose(self.engine.world(), Some(&self.tween), &mut self.rig, self.time, dt);
        f.front.extend(button_quads(&self.track.buttons, &mut self.buttons, w, h, self.time));
        if self.script.is_some() {
            // Replaying: a pulsing red dot at the top.
            let r = h * 0.022 * (1.0 + 0.15 * (self.time * 4.0).sin());
            let c = [0.95, 0.1, 0.08, 0.9];
            f.front.push(Quad {
                image: crate::stage::BLOB.into(),
                x: w / 2.0 - r,
                y: h * 0.04 - r,
                w: 2.0 * r,
                h: 2.0 * r,
                uv: [0.0, 0.0, 1.0, 1.0],
                top: c,
                bottom: c,
                blur: 0.0,
                desat: 0.0,
                wrap: Wrap::Clamp,
                rot: 0.0,
            });
        }
        f
    }

    pub fn title(&self, speed: f32, paused: bool) -> String {
        let w = self.world();
        let me = w.of_kind(&self.track.follow).next();
        let state = me.map_or_else(|| "—".to_string(), |e| sim_state::active_part(&e.state).to_string());
        let score = self.engine.rules().scores(w).values().sum::<i64>();
        let end = self.engine.outcome().map(|o| format!(" — {o} — R replay · N new run")).unwrap_or_default();
        let replay = if self.script.is_some() { " — REPLAY" } else { "" };
        let refused = match &self.refused {
            Some((a, why, at)) if self.time - at < 3.0 => format!(" — {a}: {why}"),
            _ => String::new(),
        };
        format!(
            "{}{replay} — {} — score {score} — {}{end}{refused}",
            self.engine.rules().def.name,
            state,
            if paused { "paused".to_string() } else { format!("{speed:.0} ticks/s") }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn lanes() -> (World, Game, Track, Sizes) {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../games/lanes");
        let (world, game) = Game::load(&dir, None).expect("loads");
        let (track, assets) = crate::load_track(&dir).expect("every image the track names exists");
        let sizes = assets.loaded.iter().map(|(n, i)| (n.clone(), (i.w as u32, i.h as u32))).collect();
        (world, game, track, sizes)
    }

    #[test]
    fn the_camera_sits_behind_the_car_and_looks_down_the_road() {
        let (world, game, track, sizes) = lanes();
        let c = TrackComposer { track: &track, game: &game, sizes: &sizes, w: 1600.0, h: 900.0 };
        let (_, x, z) = c.followed(&world, None).expect("a car");
        let f = c.compose(&world, None, &mut Rig::default(), 0.0, 0.0);
        assert!((f.eye.pos.0 - x).abs() < 0.3 && f.eye.pos.2 < z && f.eye.target.2 > z);
        // The road ahead narrows to the horizon: a lane line's near end is lower and wider apart than its far end.
        let near = f.eye.project(V3(0.5, 0.0, z + 2.0), 1600.0, 900.0).unwrap();
        let far = f.eye.project(V3(0.5, 0.0, z + 40.0), 1600.0, 900.0).unwrap();
        assert!(near.1 > far.1 && near.0 > far.0);
    }

    #[test]
    fn a_dash_widens_the_view_and_streaks_the_screen() {
        let (world, game, track, sizes) = lanes();
        let c = TrackComposer { track: &track, game: &game, sizes: &sizes, w: 1600.0, h: 900.0 };
        let mut rig = Rig::default();
        let calm = c.compose(&world, None, &mut rig, 1.0, 0.0);
        rig.fx.trigger(&track.fx, "dashed", 1.0);
        let dash = c.compose(&world, None, &mut rig, 1.15, 0.0);
        assert!(dash.eye.fov > calm.eye.fov + 10.0);
        assert!(dash.front.len() > calm.front.len() + 30, "speed lines");
    }

    #[test]
    fn a_switch_glides_overshoots_and_lands_on_the_position_with_effects_scaled_by_distance() {
        let (mut world, game, track, sizes) = lanes();
        let c = TrackComposer { track: &track, game: &game, sizes: &sizes, w: 1600.0, h: 900.0 };
        let car = world.of_kind("car").next().unwrap().id;
        let set = |w: &mut World, lane: i64| {
            w.props_mut(car).unwrap().insert("lane_to".into(), lane);
        };
        set(&mut world, 1);
        let mut rig = Rig::default();
        let dt = 1.0 / 60.0;
        let mut t = 0.0;
        for _ in 0..30 {
            c.compose(&world, None, &mut rig, t, dt);
            t += dt;
        }
        let start_x = rig.drawn_x.unwrap();
        set(&mut world, 3); // two positions over
        let (mut xs, mut fovs) = (Vec::new(), Vec::new());
        for _ in 0..40 {
            let f = c.compose(&world, None, &mut rig, t, dt);
            xs.push(rig.drawn_x.unwrap());
            fovs.push(f.eye.fov);
            t += dt;
        }
        let target = track.switch.as_ref().unwrap().positions[3];
        assert!((xs.last().unwrap() - target).abs() < 1e-3, "lands on the position: {:?}", xs.last());
        assert!(xs.iter().any(|x| *x > target + 0.01), "back_out overshoots before it settles");
        assert!((xs[0] - start_x).abs() < 1e-4 && xs[3] > start_x, "it starts from where the camera was, then moves");
        let kick = fovs.iter().cloned().fold(0.0, f32::max) - track.camera.fov;
        // One position over: the start effect is half as strong.
        set(&mut world, 2);
        let mut fovs1 = Vec::new();
        for _ in 0..40 {
            fovs1.push(c.compose(&world, None, &mut rig, t, dt).eye.fov);
            t += dt;
        }
        let kick1 = fovs1.iter().cloned().fold(0.0, f32::max) - track.camera.fov;
        assert!(kick > kick1 * 1.6, "a longer switch punches harder: {kick} vs {kick1}");
    }

    fn engine() -> sim_core::Engine<sim_core::Running, Game> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../games/lanes");
        let (world, game) = Game::load(&dir, None).expect("loads");
        sim_core::Engine::<sim_core::Loaded, _>::new(world, game).validate().expect("valid").start()
    }

    #[test]
    fn a_replay_of_a_run_is_the_same_run() {
        let (_, _, track, sizes) = lanes();
        let mut play = TrackPlay::new(engine(), track.clone(), sizes.clone());
        let keys = ["4", "1", "space", "3", "enter", "2", "1", "4"];
        for (i, k) in keys.iter().enumerate() {
            play.step(6 + i as u32);
            play.key(k);
        }
        play.step(40);
        assert!(!play.log.is_empty(), "presses were recorded");
        let (tick, hash) = (play.world().tick, play.world().hash());
        let path = std::env::temp_dir().join("simcraft-lanes-run.jsonl");
        save_run(&path, &play.log).unwrap();
        let mut again = TrackPlay::new(engine(), track, sizes);
        again.script = Some(load_run(&path).unwrap());
        again.key("1"); // a replay ignores the player
        again.step((tick - again.world().tick) as u32);
        assert_eq!((again.world().tick, again.world().hash()), (tick, hash), "same presses, same ticks, same world");
    }

    #[test]
    fn a_hit_leaves_a_ghost_and_a_lost_life_bursts_off_the_meter() {
        let (mut world, game, track, sizes) = lanes();
        let c = TrackComposer { track: &track, game: &game, sizes: &sizes, w: 1600.0, h: 900.0 };
        let mut rig = Rig::default();
        c.compose(&world, None, &mut rig, 0.0, 1.0 / 60.0);
        let car = world.of_kind("car").next().unwrap().id;
        let (cy, cx) = {
            let e = world.get(car).unwrap();
            (e.y, e.x)
        };
        let cone = world.of_kind("cone").filter(|e| e.y > cy).min_by_key(|e| e.y).map(|e| e.id).expect("a cone ahead");
        // Bring the car up to the cone, then smash it: the cone vanishes, the car loses a life.
        let (ky, kx) = {
            let e = world.get(cone).unwrap();
            (e.y, e.x)
        };
        assert!(world.leap(car, kx - cx, ky - cy - 3, 0));
        world.props_mut(car).unwrap().insert("lives".into(), 3);
        c.compose(&world, None, &mut rig, 0.5, 1.0 / 60.0);
        world.despawn(cone);
        world.props_mut(car).unwrap().insert("lives".into(), 2);
        let f = c.compose(&world, None, &mut rig, 0.6, 1.0 / 60.0);
        assert_eq!(rig.ghosts.len(), 1, "the smashed cone tumbles on");
        assert!(f.meshes.iter().any(|m| m.image == "cone"), "and is drawn");
        assert_eq!(rig.meters[0].lost.len(), 1, "one life bursts off the meter");
        c.compose(&world, None, &mut rig, 5.0, 1.0 / 60.0);
        assert!(rig.ghosts.is_empty() && rig.meters[0].lost.is_empty(), "both animations end");
    }

    #[test]
    fn nothing_behind_the_camera_or_past_the_view_is_drawn() {
        let (world, game, track, sizes) = lanes();
        let c = TrackComposer { track: &track, game: &game, sizes: &sizes, w: 1600.0, h: 900.0 };
        let f = c.compose(&world, None, &mut Rig::default(), 0.0, 0.0);
        let (_, _, z) = c.followed(&world, None).unwrap();
        for m in &f.meshes {
            for v in &m.verts {
                assert!(v.pos[2] >= z - 3.5 && v.pos[2] <= z + track.view + 0.5, "{} at {}", m.image, v.pos[2]);
            }
        }
    }
}
