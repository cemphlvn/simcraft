//! Driving, from the driver's seat (`games/<name>/drive.ron`): free-moving cars on a track that is data
//! (`sim_physics::TrackDef`), seen from a cockpit that moves like a head in a race car. Any game whose cars carry
//! `px`, `py` (mm) and `yaw` (65536 a turn, counterclockwise from +x) can use it; the look is the game's
//! (`drive.ron`), the geometry comes from the track file.
//!
//! What sells it, each piece measured (`feel_probe`): a fixed field of view (sims keep it true to the screen); the
//! head on a spring-damper that leans against the g-forces a little; the horizon tilting with the banking, half
//! of it taken back by the neck (the vestibular reflex); vibration that grows with speed and bumps fixed to places on
//! the track; the eyes going into the turn a few degrees; body roll and pitch from the car when it has them, else
//! from the g-forces. Composing is pure (cars + time → a frame), so any frame can be tested without a GPU.
//!
//! Render space: X = the track's x, Y = up, Z = the track's y (metres; see `scene`).

pub mod audio;
pub mod car;
pub mod carmodel;
pub mod geom;
pub mod photos;
pub mod scene;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sim_core::{Entity, EntityId, World};
use sim_physics::{Track, TrackDef};
use sim_render::feel::{CameraFeel, Spring};
use sim_rules::Game;

use crate::gpu::{Gpu, Mesh, World3};
use crate::math::{Eye, V3};
use crate::stage::{BLOB, Quad, WHITE, Wrap};
use car::{CarLook, Dash, Parts};
use carmodel::{CAR_MODEL, CarModel, Livery, ModelLook};
use geom::{Frame, MIRROR, rect, rgb, text};
use photos::Photos;
use scene::{Centre, Ground, Scene, centre, fl, fx};

pub const G: f32 = 9.81;
/// Where the static scene is kept on the GPU (`Gpu::keep`).
pub const SCENE_SLOT: u32 = 70;

// ---------------------------------------------------------------- the view as data

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Drive {
    /// The kind every car is (each entity of it is drawn on the track).
    pub cars: String,
    /// Which car you drive: the first whose prop equals the value (else the first car).
    #[serde(default)]
    pub you: Option<(String, i64)>,
    /// The track file (a `sim_physics::TrackDef`, relative to the game's folder) and where its origin (the start
    /// line) sits in the world (mm), for a game that does not name its track itself (`track:` in game.ron wins).
    #[serde(default)]
    pub track: Option<String>,
    #[serde(default)]
    pub origin: (i64, i64),
    #[serde(default)]
    pub props: Props,
    #[serde(default)]
    pub cockpit: Cockpit,
    #[serde(default)]
    pub chase: Chase,
    #[serde(default)]
    pub top: Top,
    #[serde(default)]
    pub look: Look,
    #[serde(default)]
    pub controls: Option<Controls>,
    #[serde(default)]
    pub sound: Sound,
    #[serde(default)]
    /// The game's own driver for your car (`O`, `--auto`).
    pub autopilot: Option<Autopilot>,
    /// Your line drawn faintly on the road ahead from outside (chase, top-down), when the car steers by line.
    #[serde(default)]
    pub line_marker: Option<LineMarker>,
    /// Speed on the dash: "mph" or "kmh".
    #[serde(default = "mph")]
    pub units: String,
    /// The spotter's calls and the running order at the top of the screen.
    #[serde(default = "yes")]
    pub spotter: bool,
    /// The window's size (logical pixels) and shots' (twice it), unless `--size` says otherwise.
    #[serde(default)]
    pub screen: Option<(u32, u32)>,
    /// The game's folder (pictures, the car's model and recordings are found from it).
    #[serde(skip)]
    pub dir: PathBuf,
}

fn mph() -> String {
    "mph".into()
}

fn yes() -> bool {
    true
}

/// The props the view reads, by name: the car's own words for them. A prop the car does not have is derived (speed,
/// g-forces, body motion, laps, the running order) or left out (rpm, gear).
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Props {
    pub speed: String,
    pub rpm: String,
    pub gear: String,
    pub steer: String,
    pub throttle: String,
    pub brake: String,
    /// What the driver feels in the car's frame, mm/s² (forward; to the left, as yaw turns): the tyres' push, so
    /// on a banked turn at its neutral speed, nothing sideways.
    pub g_long: String,
    pub g_lat: String,
    /// Body motion on the suspension, 65536 a turn (nose up; right side down).
    pub pitch: String,
    pub roll: String,
    pub lap: String,
    /// The lap you are on, the time into it, the last and the best (ms).
    pub lap_time_ms: String,
    pub last_lap_ms: String,
    pub best_lap_ms: String,
    pub position: String,
    /// The number painted on the car (else its id).
    pub number: String,
    /// A knock from contact this tick (N·s): the head jolts.
    pub impact: String,
}

impl Default for Props {
    fn default() -> Self {
        let s = |v: &str| v.to_string();
        Props {
            speed: s("speed"),
            rpm: s("rpm"),
            gear: s("gear"),
            steer: s("steer"),
            throttle: s("throttle"),
            brake: s("brake"),
            g_long: s("g_long"),
            g_lat: s("g_lat"),
            pitch: s("pitch"),
            roll: s("roll"),
            lap: s("lap"),
            lap_time_ms: s("lap_time_ms"),
            last_lap_ms: s("last_lap_ms"),
            best_lap_ms: s("best_lap_ms"),
            position: s("position"),
            number: s("number"),
            impact: s("impact"),
        }
    }
}

/// The driver's seat and what the head does.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Cockpit {
    /// The driver's eye in the car's frame: right, up, forward of the middle of the car on the ground (m). A Cup
    /// car's driver sits left of centre.
    pub eye: (f32, f32, f32),
    /// Vertical field of view, degrees, fixed (no zoom with speed). The true one for a screen `h` cm high seen from
    /// `d` cm away is 2·atan(h / 2d); 50° suits a desk monitor used a little wide.
    pub fov: f32,
    pub head: Head,
    pub body: Body,
    pub wheel: WheelLook,
    pub mirror: Option<MirrorLook>,
    /// Shift lights: start lighting at, flash at (rpm).
    pub shift_lights: (f32, f32),
    /// The roll cage's paint.
    pub cage: (u8, u8, u8),
    pub net: bool,
    /// On the dash's photograph (`look.textures.dash`): where its screen is (u0, v0, u1, v1: fractions of the
    /// picture), and its row of ten LEDs (first u, v, last u). The display and the shift lights are drawn there.
    pub display: (f32, f32, f32, f32),
    pub lights: (f32, f32, f32),
    /// How high the dash photograph's top edge stands (m, car frame): high enough that its screen shows through the
    /// wheel, above the hub.
    pub dash_top: f32,
}

impl Default for Cockpit {
    fn default() -> Self {
        Cockpit {
            eye: (-0.33, 1.02, -0.25),
            fov: 50.0,
            head: Head::default(),
            body: Body::default(),
            wheel: WheelLook::default(),
            mirror: Some(MirrorLook::default()),
            shift_lights: (7800.0, 9200.0),
            cage: (190, 192, 190),
            net: true,
            display: (0.303, 0.104, 0.698, 0.39),
            lights: (0.356, 0.073, 0.64),
            dash_top: 0.925,
        }
    }
}

/// The head in a race seat: a spring-damper, small (a Cup seat and a HANS hold it), leaning against the g-forces.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Head {
    pub stiffness: f32,
    pub damping: f32,
    /// Metres the head moves per g: sideways, down, back.
    pub lateral: f32,
    pub vertical: f32,
    pub longitudinal: f32,
    /// Degrees the view nods per g of braking or acceleration.
    pub pitch: f32,
    /// How much of the car's roll the neck takes back (0 = the horizon tilts with the car, 1 = locked level).
    pub level: f32,
    /// How fast the neck levels (spring stiffness).
    pub level_stiffness: f32,
    /// Degrees the eyes go into a turn per degree a second of yaw, at most `look_max`.
    pub look: f32,
    pub look_max: f32,
    pub look_stiffness: f32,
    /// Buzz from the road at 80 m/s (m, grows with the square of speed), and bumps fixed to the track (m).
    pub vibration: f32,
    pub bumps: f32,
    /// How hard contact throws the head: m/s per N·s of `impact`.
    pub jolt: f32,
}

impl Default for Head {
    fn default() -> Self {
        Head {
            stiffness: 140.0,
            damping: 0.7,
            lateral: 0.014,
            vertical: 0.008,
            longitudinal: 0.012,
            pitch: 0.8,
            level: 0.5,
            level_stiffness: 30.0,
            look: 0.28,
            look_max: 9.0,
            look_stiffness: 10.0,
            vibration: 0.0012,
            bumps: 0.004,
            jolt: 0.00012,
        }
    }
}

/// Body motion when the car does not say (`pitch`, `roll` props): from the g-forces, on a spring.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Body {
    /// Degrees per g (sideways: toward the outside; lengthwise: nose down braking).
    pub roll: f32,
    pub pitch: f32,
    pub stiffness: f32,
    pub damping: f32,
}

impl Default for Body {
    fn default() -> Self {
        Body { roll: 1.1, pitch: 0.7, stiffness: 90.0, damping: 0.55 }
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WheelLook {
    /// Height and distance forward of the wheel's centre (m, car frame; across it is in front of the eye).
    pub at: (f32, f32),
    pub radius: f32,
    /// Degrees the wheel leans back from upright.
    pub tilt: f32,
    /// Degrees the wheel turns at full steer (steer ±1000).
    pub lock: f32,
}

impl Default for WheelLook {
    fn default() -> Self {
        WheelLook { at: (0.8, 0.16), radius: 0.165, tilt: 23.0, lock: 270.0 }
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MirrorLook {
    /// Its middle (car frame) and size (m).
    pub at: (f32, f32, f32),
    pub size: (f32, f32),
    /// Its picture's pixels.
    pub pixels: (u32, u32),
}

impl Default for MirrorLook {
    fn default() -> Self {
        MirrorLook { at: (0.06, 1.165, 0.3), size: (0.38, 0.085), pixels: (640, 144) }
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Chase {
    pub height: f32,
    pub back: f32,
    pub fov: f32,
    /// How quickly the camera swings round behind the car.
    pub stiffness: f32,
}

impl Default for Chase {
    fn default() -> Self {
        Chase { height: 2.3, back: 8.5, fov: 55.0, stiffness: 25.0 }
    }
}

/// The top-down camera: high above the car, a little behind it, nose up the screen (the heading on a spring), and
/// rising with speed so there is always room to see the next corner coming.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Top {
    /// Height at rest, and extra height per m/s (m).
    pub height: f32,
    pub per_speed: f32,
    /// Look this far ahead of the car, as a share of the height (the car sits low on the screen).
    pub lead: f32,
    pub fov: f32,
    pub stiffness: f32,
}

impl Default for Top {
    fn default() -> Self {
        Top { height: 45.0, per_speed: 0.9, lead: 0.35, fov: 50.0, stiffness: 12.0 }
    }
}

/// How the track looks on race day.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Look {
    pub asphalt: (u8, u8, u8),
    pub apron: (u8, u8, u8),
    /// How much darker the racing groove is (0..1).
    pub groove: f32,
    pub white: (u8, u8, u8),
    pub yellow: (u8, u8, u8),
    pub grass: (u8, u8, u8),
    pub safer: (u8, u8, u8),
    pub concrete: (u8, u8, u8),
    pub pit_wall: (u8, u8, u8),
    /// The sky overhead and at the horizon (the haze, also the fog's colour), and the land far off.
    pub sky: (u8, u8, u8),
    pub horizon: (u8, u8, u8),
    pub ground: (u8, u8, u8),
    /// The sun: azimuth (degrees, counterclockwise from the track's +x) and elevation.
    pub sun: (f32, f32),
    pub sun_color: (u8, u8, u8),
    /// Aerial perspective: fog starts, and is complete this far (m).
    pub fog: (f32, f32),
    /// Light scattered toward the sun in the haze.
    pub haze: f32,
    pub apron_width: f32,
    /// The apron's banking, degrees at most.
    pub apron_bank: f32,
    /// The outside wall (SAFER barrier on concrete) and the catch fence above it (m).
    pub wall_height: f32,
    pub fence_height: f32,
    /// The start/finish line's width (m).
    pub line_width: f32,
    /// Pit road: from and to (m along the track from the start line; negative is before it). None: no pit road.
    pub pit_road: Option<(f32, f32)>,
    pub stands: Vec<Stands>,
    /// Cars' paint, in order of their number.
    pub car_colors: Vec<(u8, u8, u8)>,
    /// A tree line this far outside the wall (m; 0: none).
    pub trees: f32,
    /// Photographs for the surfaces (`photos::SURFACES`: asphalt, groove, grass, crowd, sky, ...), by name:
    /// `(file, size)`. A surface without one keeps the picture the view makes itself.
    pub textures: BTreeMap<String, photos::Photo>,
    /// Sponsor boards on the catch fence: one every this many metres (0: none), and how many panels side by side
    /// the `sponsors` picture holds (each board shows the next).
    pub boards: (f32, u32),
    /// The track's emblem (`logo`) on the infield grass: where (m along the track from the start line), and how
    /// far in from the foot of the apron's grass slope its near edge lies (m).
    pub logos: Vec<(f32, f32)>,
    /// The banner (`banner`) over the track on a gantry: where (m along the track from the start line).
    pub banner_at: f32,
    /// The cars' model (a `.glb`, fitted to the kind's footprint); none, or missing: the view's own box cars.
    pub model: Option<ModelLook>,
    /// Paint schemes, in order of the cars' numbers: `((r, g, b), (r, g, b))`, the body and the second colour
    /// (rockers, stripes, number plates). None: `car_colors`.
    pub liveries: Vec<Livery>,
}

impl Default for Look {
    fn default() -> Self {
        Look {
            asphalt: (96, 96, 100),
            apron: (122, 122, 124),
            groove: 0.3,
            white: (236, 236, 230),
            yellow: (242, 196, 36),
            grass: (82, 128, 50),
            safer: (228, 230, 232),
            concrete: (196, 194, 188),
            pit_wall: (214, 212, 206),
            sky: (62, 110, 182),
            horizon: (196, 210, 224),
            ground: (120, 132, 104),
            sun: (205.0, 26.0),
            sun_color: (255, 236, 205),
            fog: (350.0, 4200.0),
            haze: 0.6,
            apron_width: 8.0,
            apron_bank: 8.0,
            wall_height: 1.25,
            fence_height: 6.5,
            line_width: 0.9,
            pit_road: Some((-300.0, 240.0)),
            stands: vec![Stands::default()],
            car_colors: vec![(200, 30, 36), (30, 80, 190), (250, 196, 30), (20, 20, 22), (240, 240, 240), (30, 150, 70), (240, 110, 20)],
            trees: 320.0,
            textures: BTreeMap::new(),
            boards: (45.0, 6),
            logos: Vec::new(),
            banner_at: 0.0,
            model: None,
            liveries: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Stands {
    /// Along the track from the start line (m).
    pub from: f32,
    pub to: f32,
    /// How deep the seating goes back from the fence, and how much it rises per metre back.
    pub depth: f32,
    pub rise: f32,
    /// A suite tower along the top.
    pub tower: bool,
}

impl Default for Stands {
    fn default() -> Self {
        Stands { from: -280.0, to: 230.0, depth: 40.0, rise: 0.52, tower: true }
    }
}

/// Keyboard (later gamepad and wheel) to a declared action with analog args.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Controls {
    /// The declared action the axes are sent through (it lands in replays like any action).
    pub action: String,
    /// Each arg of the action: how the keys make it.
    #[serde(default)]
    pub axes: BTreeMap<String, Axis>,
    /// Keys that make one action each press: `(key: "a", action: "shift", args: {"dir": 1})`.
    #[serde(default)]
    pub buttons: Vec<KeyAction>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyAction {
    pub key: String,
    pub action: String,
    #[serde(default)]
    pub args: BTreeMap<String, i64>,
    /// This arg flips between 1 and 0 on each press.
    #[serde(default)]
    pub toggle: Option<String>,
    /// The toggle's two values (off, on) when they are not (0, 1): the first press sends `on`.
    #[serde(default)]
    pub values: Option<(i64, i64)>,
}

/// One analog value from keys: a held key ramps it up over `rise_ms`, letting go brings it back over `fall_ms`.
/// `key` makes 0..1 (a pedal); `neg`/`pos` make -1..1 (the wheel). The value sent is `scale` × it.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Axis {
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub neg: Option<String>,
    #[serde(default)]
    pub pos: Option<String>,
    #[serde(default = "rise")]
    pub rise_ms: f32,
    #[serde(default = "fall")]
    pub fall_ms: f32,
    /// Less of the axis at speed (keyboard steering): at `.0` m/s and above only `.1` of it is left.
    #[serde(default)]
    pub speed_sensitive: Option<(f32, f32)>,
    #[serde(default = "thousand")]
    pub scale: f32,
}

fn rise() -> f32 {
    200.0
}
fn fall() -> f32 {
    150.0
}
fn thousand() -> f32 {
    1000.0
}

pub use audio::Sound;

/// Your car's line on the road: the prop that holds it (mm left of the centreline), shown while `when` (a prop and
/// its value) holds, this far ahead (m), in this colour.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LineMarker {
    pub prop: String,
    #[serde(default)]
    pub when: Option<(String, i64)>,
    #[serde(default = "ahead")]
    pub ahead: f32,
    #[serde(default = "marker_color")]
    pub color: (u8, u8, u8),
}

fn ahead() -> f32 {
    70.0
}

fn marker_color() -> (u8, u8, u8) {
    (90, 200, 255)
}

/// Handing your car to the game's own driver (its autopilot, with racecraft): the action and its arg, sent
/// with 1 to hand over and 0 to take back.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Autopilot {
    pub action: String,
    pub arg: String,
}

/// Loads `drive.ron` and the track: the game's own (`track:` in game.ron), else the one drive.ron names.
pub fn load(dir: &Path, game: &Game) -> Result<(Drive, Track), String> {
    let path = dir.join("drive.ron");
    let src = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut drive: Drive = ron::Options::default()
        .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
        .from_str(&src)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    drive.dir = dir.to_path_buf();
    let unknown = photos::unknown(&drive.look.textures);
    if !unknown.is_empty() {
        let known: Vec<&str> = photos::SURFACES.iter().map(|s| s.0).collect();
        return Err(format!("drive.ron: look.textures: no surface {unknown:?} (the surfaces are {known:?})"));
    }
    let file = match (&game.def.track, &drive.track) {
        (Some(t), Some(mine)) if *mine != t.file => {
            return Err(format!("drive.ron: track '{mine}', but the game races on '{}' (drop `track` from drive.ron)", t.file));
        }
        (Some(t), _) => {
            drive.origin = t.origin;
            t.file.clone()
        }
        (None, Some(mine)) => mine.clone(),
        (None, None) => return Err("drive.ron: no track (the game has no `track:` and drive.ron names none)".into()),
    };
    let tpath = dir.join(&file);
    let tsrc = std::fs::read_to_string(&tpath).map_err(|e| format!("drive.ron: track {}: {e}", tpath.display()))?;
    let def: TrackDef = ron::from_str(&tsrc).map_err(|e| format!("{}: {e}", tpath.display()))?;
    let track = Track::new(&def).map_err(|e| format!("{}: {}", tpath.display(), e.join("; ")))?;
    if let Some(c) = &drive.controls {
        for (name, a) in &c.axes {
            if a.key.is_none() && a.neg.is_none() && a.pos.is_none() {
                return Err(format!("drive.ron: axis '{name}' has no key (key, or neg and pos)"));
            }
        }
    }
    Ok((drive, track))
}

// ---------------------------------------------------------------- cars as the view sees them

/// A car at this frame (between ticks), in the track's frame.
#[derive(Clone, Debug)]
pub struct CarView {
    pub id: EntityId,
    pub you: bool,
    /// Position (m) and heading (radians, counterclockwise from +x).
    pub x: f32,
    pub y: f32,
    pub yaw: f32,
    /// m/s (the car's prop, else from how far it moved).
    pub speed: f32,
    pub rpm: Option<f32>,
    pub gear: Option<i64>,
    /// -1..1, positive left.
    pub steer: f32,
    pub throttle: f32,
    pub brake: f32,
    /// From the car's props, when it has them.
    pub lap: Option<i64>,
    pub lap_ms: Option<i64>,
    pub last_lap_ms: Option<i64>,
    pub best_lap_ms: Option<i64>,
    pub position: Option<i64>,
    pub number: i64,
    /// Contact this tick (N·s).
    pub impact: f32,
    /// The line it steers by (m left of the centreline), when the drive view shows one (`line_marker`).
    pub line: Option<f32>,
    /// Accelerations it reports (m/s²: forward, left) and its body motion (radians: nose up, right side down).
    pub accel: Option<(f32, f32)>,
    pub body: Option<(f32, f32)>,
    /// Where it is on the track.
    pub s: f32,
    pub offset: f32,
    pub seg: usize,
}

fn prop(e: &Entity, name: &str) -> Option<i64> {
    (!name.is_empty()).then(|| e.props.get(name).copied()).flatten()
}

/// Binary angle (65536 a turn) to radians.
pub fn yaw_rad(v: i64) -> f32 {
    (v.rem_euclid(65536) as f64 * std::f64::consts::TAU / 65536.0) as f32
}

fn wrap_pi(a: f32) -> f32 {
    (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

/// The car's frame on the ground: on the surface the track has there (banking, apron), heading its way, tilted by
/// its body's pitch and roll.
pub fn seat(track: &Track, ground: &Ground, c: &CarView, body: (f32, f32)) -> Frame {
    let ce = centre(track, c.s);
    seat_at(ground, &ce, c.offset, c.yaw, body)
}

pub fn seat_at(ground: &Ground, ce: &Centre, offset: f32, yaw: f32, body: (f32, f32)) -> Frame {
    let o = ground.point(ce, offset, 0.0);
    let (sn, cs) = ce.heading.sin_cos();
    let left = V3(-sn, 0.0, cs);
    let rise = ground.height(ce, offset + 0.5) - ground.height(ce, offset - 0.5);
    let n = (V3(0.0, 1.0, 0.0) - left.scale(rise)).norm();
    let fh = V3(yaw.cos(), 0.0, yaw.sin());
    let f = (fh - n.scale(fh.dot(n))).norm();
    let r = n.cross(f).norm();
    let (pitch, roll) = body;
    // Roll: the right side down; pitch: the nose up.
    let (sr, cr) = roll.sin_cos();
    let (u1, r1) = (n.scale(cr) + r.scale(sr), r.scale(cr) - n.scale(sr));
    let (sp, cp) = pitch.sin_cos();
    let (f2, u2) = (f.scale(cp) + u1.scale(sp), u1.scale(cp) - f.scale(sp));
    Frame { o, r: r1, u: u2, f: f2 }
}

/// The roll an `Eye` looking along `f` needs so that its up is `u` (degrees).
pub fn roll_of(pos: V3, f: V3, u: V3) -> f32 {
    let (r0, u0, _) = Eye { pos, target: pos + f, roll: 0.0, fov: 50.0, near: 0.1, far: 10.0 }.basis();
    (-u.dot(r0)).atan2(u.dot(u0)).to_degrees()
}

// ---------------------------------------------------------------- the camera rig

/// Motion measured from the ticks (filtered): what the head feels when the car does not report it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Kin {
    /// m/s, rad/s, and horizontal accelerations (m/s²: forward, to the left).
    pub speed: f32,
    pub yaw_rate: f32,
    pub a_long: f32,
    pub a_lat: f32,
}

/// What the rig did this frame (the feel probe reads it).
#[derive(Clone, Copy, Debug, Default)]
pub struct RigOut {
    /// The head's offset in the car (m: right, up, forward).
    pub head: V3,
    /// Roll drawn and the roll the neck aims for (degrees), and the car's own roll against the horizon.
    pub roll: f32,
    pub roll_target: f32,
    pub car_roll: f32,
    /// What the driver feels (g: to the left, forward, up).
    pub felt: V3,
    pub look: f32,
    /// The eye without vibration (world).
    pub eye_base: V3,
}

#[derive(Clone, Debug, Default)]
pub struct Rig {
    /// 0: cockpit, 1: chase, 2: top-down.
    pub view: usize,
    head: [Spring; 3],
    pitch: Spring,
    look: Spring,
    level: Spring,
    body: [Spring; 2],
    chase_yaw: Spring,
    chase_unwrapped: Option<f32>,
    pub out: RigOut,
}

fn spring(k: f32, d: f32) -> CameraFeel {
    CameraFeel { stiffness: k, damping: d }
}

/// Specific force (what a body in the seat feels, gravity included) in the car's frame: (left, up, forward), m/s².
pub fn felt(fr: &Frame, a_long: f32, a_lat: f32) -> V3 {
    let up = V3(0.0, 1.0, 0.0);
    // The car's left axis and its horizontal part: the centripetal pull is horizontal.
    let left = fr.r.scale(-1.0);
    let lh = V3(left.0, 0.0, left.2).norm();
    let fh = V3(fr.f.0, 0.0, fr.f.2).norm();
    let a = lh.scale(a_lat) + fh.scale(a_long);
    // Specific force = acceleration − gravity (gravity points down).
    let sf = a + up.scale(G);
    V3(sf.dot(left), sf.dot(fr.u), sf.dot(fr.f))
}

/// Vertical buzz and bumps: small, faster and bigger with speed; bumps are fixed to places on the track.
pub fn vibration(h: &Head, speed: f32, s: f32, time: f32) -> f32 {
    let k = (speed / 80.0).powi(2).min(2.0);
    let tau = std::f32::consts::TAU;
    let buzz = (tau * 13.0 * time).sin() * 0.5 + (tau * 21.7 * time + 1.0).sin() * 0.3 + (tau * 34.3 * time + 2.0).sin() * 0.2;
    let bumps = (tau * s / 7.3).sin() * 0.5 + (tau * s / 13.1 + 0.7).sin() * 0.3 + (tau * s / 23.7 + 1.9).sin() * 0.2;
    h.vibration * k * buzz + h.bumps * (speed / 80.0).min(1.5) * bumps
}

/// The composer's inputs that are not the world: the drive view, the track, its ground, the screen.
pub struct Composer<'a> {
    pub drive: &'a Drive,
    pub track: &'a Track,
    pub ground: &'a Ground,
    pub photos: &'a Photos,
    /// The cars' model, when the game has one (and the GPU can draw models).
    pub car: Option<&'a CarModel>,
    pub sun: V3,
    pub w: f32,
    pub h: f32,
}

/// One frame of the drive view: 2D behind (sky), the 3D world (the scene kept on the GPU plus these meshes), 2D in
/// front (the spotter, timing); and the mirror's own picture, rendered first.
pub struct DriveFrame {
    pub back: Vec<Quad>,
    pub eye: Eye,
    pub fog: [f32; 3],
    pub fog_range: [f32; 2],
    pub sun: V3,
    pub sun_color: [f32; 3],
    pub haze: f32,
    pub meshes: Vec<Mesh>,
    /// How many of `meshes` (from the start) the mirror shows.
    pub mirror_meshes: usize,
    /// Cars drawn with the model (instances), in the frame and the mirror.
    pub models: Vec<crate::skin::ModelDraw>,
    /// The sky and the ground's light on the models (linear).
    pub ambient: ([f32; 3], [f32; 3]),
    pub front: Vec<Quad>,
    pub mirror: Option<(Eye, Vec<Quad>, (u32, u32))>,
}

impl DriveFrame {
    /// The 3D part for `eye`: the kept scene and `meshes`.
    pub fn world3<'a>(&'a self, eye: &Eye, meshes: &'a [Mesh], w: f32, h: f32) -> World3<'a> {
        let mut world = World3::new(eye.view_proj(w, h), self.fog, meshes);
        world.eye = [eye.pos.0, eye.pos.1, eye.pos.2];
        world.fog_range = self.fog_range;
        world.kept = &[SCENE_SLOT];
        world.haze = self.haze;
        world.light.sun_dir = [self.sun.0, self.sun.1, self.sun.2];
        world.models = &self.models;
        // The models are lit in linear light (the scene's vertex light is sRGB): the sun a little over white.
        world.light.sun = self.sun_color.map(|c| c.powf(2.2) * 2.4);
        world.light.sky = self.ambient.0;
        world.light.ground = self.ambient.1;
        world
    }
}

/// The sky's panorama as seen by `eye`: slices of a cylinder round the eye (its bottom on the horizon, as high as
/// the picture's proportions make it), each a quad whose picture is the azimuths it covers, so the sky stays put as
/// the view turns and tilts with the horizon. `size` is the degrees of the horizon one copy spans; the picture is
/// mirrored from one copy to the next (so 360 / size should be even, or the sky jumps where the angles wrap).
pub fn sky_photo(eye: &Eye, w: f32, h: f32, f: &photos::Found) -> Vec<Quad> {
    let (r0, u0, fwd) = eye.basis();
    let fh = V3(fwd.0, 0.0, fwd.2);
    if fh.dot(fh) < 1e-6 {
        return Vec::new();
    }
    let fh = fh.norm();
    let far = |d: V3| eye.pos + d.scale(4000.0);
    let rh = V3(0.0, 1.0, 0.0).cross(fh);
    let (Some((hx, hy)), Some((px, py))) = (eye.project(far(fh), w, h), eye.project(far(fh + rh.scale(0.3)), w, h)) else {
        return Vec::new();
    };
    let rot = (py - hy).atan2(px - hx);
    let (dx, dy) = (rot.cos(), rot.sin());
    let (ux, uy) = (dy, -dx);
    let sy = 1.0 / (eye.fov.to_radians() / 2.0).tan();
    let sx = sy / (w / h.max(1.0));
    let dir_at = |x: f32, y: f32| fwd + r0.scale((x / w * 2.0 - 1.0) / sx) + u0.scale((1.0 - y / h * 2.0) / sy);
    let az = |d: V3| d.2.atan2(d.0);
    let span = f.size.to_radians().max(0.01);
    let elev = (span / f.aspect()).min(1.5);
    let diag = (w * w + h * h).sqrt();
    let n = 32;
    let t = |k: usize| diag * (-0.75 + 1.5 * k as f32 / n as f32);
    let mut out = Vec::with_capacity(n);
    let mut a0 = az(dir_at(hx + dx * t(0), hy + dy * t(0)));
    for k in 0..n {
        let (t0, t1) = (t(k), t(k + 1));
        let (x0, y0, x1, y1) = (hx + dx * t0, hy + dy * t0, hx + dx * t1, hy + dy * t1);
        let a1 = a0 + wrap_pi(az(dir_at(x1, y1)) - a0);
        let am = (a0 + a1) / 2.0;
        let (top, hor) = (V3(elev.cos() * am.cos(), elev.sin(), elev.cos() * am.sin()), V3(am.cos(), 0.0, am.sin()));
        if let (Some(pt), Some(ph)) = (eye.project(far(top), w, h), eye.project(far(hor), w, h)) {
            let hq = ((pt.0 - ph.0) * ux + (pt.1 - ph.1) * uy).max(1.0);
            let wq = (t1 - t0) + 1.0;
            let (cx, cy) = ((x0 + x1) / 2.0 + ux * hq / 2.0, (y0 + y1) / 2.0 + uy * hq / 2.0);
            // Azimuth grows to the left on screen (render space is left-handed), the picture to the right.
            out.push(Quad {
                image: f.texture.clone(),
                x: cx - wq / 2.0,
                y: cy - hq / 2.0,
                w: wq,
                h: hq,
                uv: [-a0 / span, 0.0, -a1 / span, 1.0],
                top: [1.0; 4],
                bottom: [1.0; 4],
                blur: 0.0,
                desat: 0.0,
                wrap: Wrap::MirrorX,
                rot: rot.to_degrees(),
            });
        }
        a0 = a1;
    }
    out
}

/// The spotter's call, and everything else the HUD shows.
#[derive(Clone, Debug, Default)]
pub struct Hud {
    pub call: Option<String>,
    /// The spotter's call as a line to say: car_low, car_high, three_wide, still_there, clear_low, clear_high,
    /// clear (the host's voice plays it, `sound.voice`).
    pub voice: Option<String>,
    pub position: usize,
    pub cars: usize,
    pub lap: i64,
    pub gap_ahead: Option<f32>,
    pub gap_behind: Option<f32>,
    pub lap_time: Option<f32>,
    pub last_lap: Option<f32>,
    pub best_lap: Option<f32>,
    pub auto: bool,
    pub replay: bool,
}

fn lap_text(t: f32) -> String {
    let m = (t / 60.0).floor() as i64;
    let s = t - m as f32 * 60.0;
    if m > 0 { format!("{m}:{s:06.3}") } else { format!("{s:.3}") }
}

impl Composer<'_> {
    fn look(&self) -> &Look {
        &self.drive.look
    }

    /// The camera for `you` this frame, moving the rig's springs by `dt`.
    pub fn camera(&self, you: &CarView, kin: &Kin, rig: &mut Rig, time: f32, dt: f32) -> (Eye, Frame) {
        let c = &self.drive.cockpit;
        let h = c.head;
        // The car's own body motion, or one from the g-forces on a spring.
        let flat = seat(self.track, self.ground, you, (0.0, 0.0));
        // Felt: what the car reports (sideways and lengthwise), else from its motion; up always from its motion.
        let felt_in = |fr: &Frame| {
            let k = felt(fr, kin.a_long, kin.a_lat);
            you.accel.map_or(k, |(l, t)| V3(t, k.1, l))
        };
        let f0 = felt_in(&flat);
        let body = match you.body {
            Some(b) => b,
            None => {
                let bf = spring(c.body.stiffness, c.body.damping);
                let roll = rig.body[0].update((c.body.roll * f0.0 / G).to_radians(), dt, bf);
                let pitch = rig.body[1].update((c.body.pitch * f0.2 / G).to_radians(), dt, bf);
                (pitch, roll)
            }
        };
        let fr = seat(self.track, self.ground, you, body);
        let f = felt_in(&fr);
        let hf = spring(h.stiffness, h.damping);
        // Contact: a knock throws the head (m/s per N·s), sideways and up, then the springs bring it back.
        if you.impact > 0.0 {
            let kick = (you.impact * h.jolt).min(1.2);
            let side = if (time * 97.0).fract() < 0.5 { 1.0 } else { -1.0 };
            rig.head[0].vel += kick * side;
            rig.head[1].vel += kick * 0.4;
            rig.pitch.vel += kick * 30.0;
        }
        let head = V3(
            rig.head[0].update(h.lateral * f.0 / G, dt, hf),
            rig.head[1].update(-h.vertical * (f.1 - G) / G, dt, hf),
            rig.head[2].update(-h.longitudinal * f.2 / G, dt, hf),
        );
        let pitch = rig.pitch.update(h.pitch * f.2 / G, dt, hf);
        let look_target = (h.look * kin.yaw_rate.to_degrees()).clamp(-h.look_max, h.look_max);
        let look = rig.look.update(look_target, dt, spring(h.look_stiffness, 1.0));
        let (vib, fwd) = (vibration(&h, you.speed, you.s, time), fr.f);
        if rig.view == 1 {
            return (self.chase(you, &fr, rig, dt), fr);
        }
        if rig.view == 2 {
            return (self.top(you, &fr, rig, dt), fr);
        }
        // The head's frame: the car's, turned into the turn and nodded.
        let (sl, cl) = look.to_radians().sin_cos();
        let f1 = fwd.scale(cl) - fr.r.scale(sl);
        let (sp, cp) = pitch.to_radians().sin_cos();
        let f2 = (f1.scale(cp) + fr.u.scale(sp)).norm();
        let eye_local = V3(c.eye.0, c.eye.1, c.eye.2) + head;
        let base = fr.at(eye_local);
        let pos = base + fr.u.scale(vib);
        // The horizon: the car's roll, part of it taken back by the neck, on a spring (the reflex is not instant).
        let car_roll = roll_of(pos, f2, fr.u);
        let target = car_roll * (1.0 - h.level);
        let roll = rig.level.update(target, dt, spring(h.level_stiffness, 1.0));
        rig.out = RigOut { head, roll, roll_target: target, car_roll, felt: f.scale(1.0 / G), look, eye_base: base };
        let eye = Eye { pos, target: pos + f2, roll: roll + vib * 40.0, fov: c.fov, near: 0.03, far: 6000.0 };
        (eye, fr)
    }

    fn chase(&self, you: &CarView, fr: &Frame, rig: &mut Rig, dt: f32) -> Eye {
        let ch = &self.drive.chase;
        // Unwrap the heading so the spring never spins the long way round.
        let last = rig.chase_unwrapped.unwrap_or(you.yaw);
        let yaw = last + wrap_pi(you.yaw - last);
        rig.chase_unwrapped = Some(yaw);
        let y = rig.chase_yaw.update(yaw, dt, spring(ch.stiffness, 1.0));
        let fh = V3(y.cos(), 0.0, y.sin());
        let pos = fr.o - fh.scale(ch.back) + V3(0.0, ch.height, 0.0);
        let target = fr.o + fh.scale(4.0) + V3(0.0, 1.0, 0.0);
        rig.out.eye_base = pos;
        rig.out.roll = 0.0;
        Eye { pos, target, roll: roll_of(pos, (target - pos).norm(), fr.u) * 0.3, fov: ch.fov, near: 0.1, far: 6000.0 }
    }

    fn top(&self, you: &CarView, fr: &Frame, rig: &mut Rig, dt: f32) -> Eye {
        let t = &self.drive.top;
        let last = rig.chase_unwrapped.unwrap_or(you.yaw);
        let yaw = last + wrap_pi(you.yaw - last);
        rig.chase_unwrapped = Some(yaw);
        let y = rig.chase_yaw.update(yaw, dt, spring(t.stiffness, 1.0));
        let fh = V3(y.cos(), 0.0, y.sin());
        let h = t.height + t.per_speed * you.speed.abs();
        // Almost straight down (a hair of tilt keeps "up the screen" defined): nose up, the car low on the screen.
        let pos = fr.o + V3(0.0, h, 0.0) - fh.scale(h * 0.05);
        let target = fr.o + fh.scale(h * t.lead);
        rig.out.eye_base = pos;
        rig.out.roll = 0.0;
        Eye { pos, target, roll: 0.0, fov: t.fov, near: 0.5, far: 8000.0 }
    }

    /// The sky behind everything: a gradient that tilts with the horizon, the sun and its glare.
    pub fn sky(&self, eye: &Eye, w: f32, h: f32) -> Vec<Quad> {
        let look = self.look();
        let (_, _, f) = eye.basis();
        let fh = V3(f.0, 0.0, f.2);
        let fh = if fh.dot(fh) < 1e-6 { V3(0.0, 0.0, 1.0) } else { fh.norm() };
        let rh = V3(0.0, 1.0, 0.0).cross(fh);
        let far = |d: V3| eye.pos + d.scale(4000.0);
        let (hx, hy) = eye.project(far(fh), w, h).unwrap_or((w / 2.0, h / 2.0));
        let (px, py) = eye.project(far(fh + rh.scale(0.3)), w, h).unwrap_or((hx + 1.0, hy));
        let rot = (py - hy).atan2(px - hx);
        let diag = (w * w + h * h).sqrt();
        let (sky, hor, gnd) = (rgb(look.sky), rgb(look.horizon), rgb(look.ground));
        let mix = |a: [f32; 3], b: [f32; 3], t: f32| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t, 1.0];
        let mut out = Vec::new();
        // Bands from the horizon up: haze, a pale middle, the deep sky (a rotated stack about the horizon point).
        let bands = [(-0.12, 0.0, mix(hor, gnd, 0.5), mix(hor, hor, 0.0)), (0.0, 0.06, mix(hor, hor, 0.0), mix(hor, sky, 0.35))];
        let bands2 = [(0.06, 0.3, mix(hor, sky, 0.35), mix(hor, sky, 0.8)), (0.3, 1.6, mix(hor, sky, 0.8), mix(sky, sky, 0.0))];
        let below = (-1.6, -0.12, mix(gnd, gnd, 0.0), mix(hor, gnd, 0.5));
        for (lo, hi, bottom, top) in bands.into_iter().chain(bands2).chain([below]) {
            let (lo, hi) = (lo * diag, hi * diag);
            let mid = -(lo + hi) / 2.0; // screen y is down: up from the horizon is negative
            let (cx, cy) = (hx - mid * rot.sin(), hy + mid * rot.cos());
            let (qw, qh) = (diag * 2.5, hi - lo + 1.0);
            out.push(Quad {
                image: WHITE.into(),
                x: cx - qw / 2.0,
                y: cy - qh / 2.0,
                w: qw,
                h: qh,
                uv: [0.0, 0.0, 1.0, 1.0],
                top,
                bottom,
                blur: 0.0,
                desat: 0.0,
                wrap: Wrap::Clamp,
                rot: rot.to_degrees(),
            });
        }
        // The sky's photograph over the gradient, from the horizon up.
        if let Some(f) = self.photos.get("sky") {
            out.extend(sky_photo(eye, w, h, f));
        }
        // The sun: a glow, a halo, a hot core; and a veil of glare when you drive into it.
        let sun = self.sun;
        let sc = rgb(look.sun_color);
        if let Some((sx, sy)) = eye.project(far(sun), w, h)
            && f.dot(sun) > 0.0
        {
            for (size, a) in [(0.9, 0.18), (0.28, 0.35), (0.07, 0.9), (0.03, 1.0)] {
                let r = size * h / 2.0;
                out.push(Quad {
                    image: BLOB.into(),
                    x: sx - r,
                    y: sy - r,
                    w: 2.0 * r,
                    h: 2.0 * r,
                    uv: [0.0, 0.0, 1.0, 1.0],
                    top: [sc[0], sc[1], sc[2], a],
                    bottom: [sc[0], sc[1], sc[2], a],
                    blur: 0.0,
                    desat: 0.0,
                    wrap: Wrap::Clamp,
                    rot: 0.0,
                });
            }
        }
        out
    }

    /// Glare over everything when looking toward a low sun (a front quad).
    pub fn glare(&self, eye: &Eye, w: f32, h: f32) -> Option<Quad> {
        let (_, _, f) = eye.basis();
        let k = f.dot(self.sun).max(0.0).powi(12) * 0.22;
        (k > 0.01).then(|| {
            let sc = rgb(self.look().sun_color);
            Quad {
                image: WHITE.into(),
                x: 0.0,
                y: 0.0,
                w,
                h,
                uv: [0.0, 0.0, 1.0, 1.0],
                top: [sc[0], sc[1], sc[2], k],
                bottom: [sc[0], sc[1], sc[2], k * 0.4],
                blur: 0.0,
                desat: 0.0,
                wrap: Wrap::Clamp,
                rot: 0.0,
            }
        })
    }

    /// A car's paint: its livery (by number), else two of the look's colours.
    fn livery(&self, c: &CarView) -> Livery {
        let look = self.look();
        let i = c.number.unsigned_abs() as usize;
        if !look.liveries.is_empty() {
            return look.liveries[i % look.liveries.len()];
        }
        let colors = &look.car_colors;
        let pick = |k: usize| colors.get(k % colors.len().max(1)).copied();
        Livery(pick(i).unwrap_or((200, 25, 25)), pick(i + 3).unwrap_or((255, 255, 255)))
    }

    fn car_look(&self, c: &CarView) -> CarLook {
        let l = self.livery(c);
        CarLook { paint: rgb(l.0), accent: rgb(l.1), number: c.number }
    }

    /// The whole frame.
    pub fn compose(&self, cars: &[CarView], kin: &Kin, hud: &Hud, rig: &mut Rig, time: f32, dt: f32) -> DriveFrame {
        let look = self.look();
        let you = cars.iter().find(|c| c.you).or_else(|| cars.first());
        let (eye, you_frame) = match you {
            Some(y) => {
                let (e, f) = self.camera(y, kin, rig, time, dt);
                (e, Some(f))
            }
            None => (Eye { pos: V3(0.0, 30.0, -60.0), target: V3(0.0, 0.0, 0.0), roll: 0.0, fov: 50.0, near: 0.1, far: 6000.0 }, None),
        };
        let mut parts = Parts::new(self.sun, self.photos);
        let mut instances = Vec::new();
        for c in cars {
            let mine = c.you && you_frame.is_some();
            let fr = if mine { you_frame.expect("checked") } else { seat(self.track, self.ground, c, c.body.unwrap_or((0.0, 0.0))) };
            let cl = self.car_look(c);
            if mine && rig.view == 0 {
                car::body(&mut parts.bodies, &mut parts.text, &fr, &cl, c.steer, true);
                let dash = self.dash(c, hud, time);
                car::cockpit(&mut parts, &fr, &self.drive.cockpit, &cl, &dash);
            } else {
                // Far cars are skipped (they would be a pixel in the haze).
                let d = fr.o - eye.pos;
                if d.dot(d) > 1500.0 * 1500.0 {
                    continue;
                }
                car::shadow(&mut parts.shadows, &fr);
                match self.car {
                    // The model: one instance; the number as decals while it can be read (and before depth
                    // precision runs out for a decal a centimetre off the body).
                    Some(m) => {
                        let livery = self.livery(c);
                        instances.push(m.instance(&fr, &livery));
                        if d.dot(d) < 150.0 * 150.0 {
                            m.decals(&mut parts.bodies, &fr, c.number, &livery);
                        }
                    }
                    None => car::body(&mut parts.bodies, &mut parts.text, &fr, &cl, c.steer, false),
                }
            }
        }
        // From outside, the line you steer by: a faint ribbon on the road ahead of you.
        if let (Some(m), Some(y), true) = (&self.drive.line_marker, you, rig.view != 0)
            && let Some(line) = y.line
        {
            let col = rgb(m.color);
            let n = 28;
            for k in 0..n {
                let (s0, s1) = (y.s + 3.0 + m.ahead * k as f32 / n as f32, y.s + 3.0 + m.ahead * (k + 1) as f32 / n as f32);
                let (c0, c1) = (centre(self.track, s0), centre(self.track, s1));
                let p = |c: &Centre, o: f32| self.ground.point(c, o, 0.05);
                let fade = |t: f32| 0.45 * (1.0 - t).min(t * 4.0).min(1.0);
                let (a0, a1) = (fade(k as f32 / n as f32), fade((k + 1) as f32 / n as f32));
                let w = 0.35;
                parts.solid.quad_c(
                    [p(&c0, line - w), p(&c1, line - w), p(&c1, line + w), p(&c0, line + w)],
                    [[0.0; 2]; 4],
                    [
                        [col[0], col[1], col[2], a0],
                        [col[0], col[1], col[2], a1],
                        [col[0], col[1], col[2], a1],
                        [col[0], col[1], col[2], a0],
                    ],
                );
            }
        }
        let (meshes, mirror_meshes) = parts.meshes();
        let models = if instances.is_empty() { Vec::new() } else { vec![crate::skin::ModelDraw { model: CAR_MODEL.into(), instances }] };
        let (w, h) = (self.w, self.h);
        let mut back = self.sky(&eye, w, h);
        let mirror = match (&self.drive.cockpit.mirror, you_frame, rig.view) {
            (Some(m), Some(fr), 0) => {
                let ce = &self.drive.cockpit.eye;
                let e = car::mirror_eye(&fr, m, V3(ce.0, ce.1, ce.2) + rig.out.head);
                let (mw, mh) = m.pixels;
                Some((e, self.sky(&e, mw as f32, mh as f32), (mw, mh)))
            }
            _ => None,
        };
        let mut front = Vec::new();
        front.extend(self.glare(&eye, w, h));
        self.hud(&mut front, you, hud, rig.view, time);
        back.shrink_to_fit();
        DriveFrame {
            back,
            eye,
            fog: rgb(look.horizon),
            fog_range: [look.fog.0, (look.fog.1 - look.fog.0).max(1.0)],
            sun: self.sun,
            sun_color: rgb(look.sun_color),
            haze: look.haze,
            meshes,
            mirror_meshes,
            models,
            ambient: (rgb(look.sky).map(|c| c.powf(2.2) * 0.9), rgb(look.ground).map(|c| c.powf(2.2) * 0.6)),
            front,
            mirror,
        }
    }

    fn dash(&self, c: &CarView, hud: &Hud, time: f32) -> Dash {
        let (shown, unit) = if self.drive.units == "kmh" { (c.speed * 3.6, "KMH") } else { (c.speed * 2.236_936, "MPH") };
        Dash {
            speed: c.speed,
            rpm: c.rpm.unwrap_or(0.0),
            gear: c.gear.unwrap_or(0),
            steer: c.steer,
            position: hud.position,
            cars: hud.cars,
            lap: hud.lap,
            time,
            speed_shown: shown,
            unit,
        }
    }

    /// The HUD: sparse, as a spotter and a timing screen would tell you.
    fn hud(&self, out: &mut Vec<Quad>, you: Option<&CarView>, hud: &Hud, view: usize, time: f32) {
        let (w, h) = (self.w, self.h);
        let s = (h / 900.0).max(0.4);
        let white = [0.96, 0.96, 0.96, 0.95];
        let grey = [0.75, 0.75, 0.75, 0.9];
        if self.drive.spotter {
            let panel = [0.0, 0.0, 0.0, 0.45];
            rect(out, 16.0 * s, 16.0 * s, 250.0 * s, 92.0 * s, panel);
            text(out, &format!("P{}/{}", hud.position, hud.cars), 28.0 * s, 26.0 * s, 30.0 * s, white);
            text(out, &format!("LAP {}", hud.lap.max(0)), 150.0 * s, 32.0 * s, 18.0 * s, grey);
            if let Some(g) = hud.gap_ahead {
                text(out, &format!("-{g:.3}"), 28.0 * s, 66.0 * s, 16.0 * s, [0.5, 0.9, 0.5, 0.95]);
            }
            if let Some(g) = hud.gap_behind {
                text(out, &format!("+{g:.3}"), 140.0 * s, 66.0 * s, 16.0 * s, [0.95, 0.6, 0.5, 0.95]);
            }
            rect(out, w - 266.0 * s, 16.0 * s, 250.0 * s, 92.0 * s, panel);
            let x = w - 254.0 * s;
            text(out, &hud.lap_time.map_or_else(|| "--.---".into(), lap_text), x, 24.0 * s, 26.0 * s, white);
            text(out, &format!("LAST {}", hud.last_lap.map_or_else(|| "--.---".into(), lap_text)), x, 60.0 * s, 14.0 * s, grey);
            text(
                out,
                &format!("BEST {}", hud.best_lap.map_or_else(|| "--.---".into(), lap_text)),
                x,
                82.0 * s,
                14.0 * s,
                [0.8, 0.55, 0.95, 0.95],
            );
            if let Some(call) = &hud.call {
                let size = 30.0 * s;
                let tw = geom::char_w(size) * call.len() as f32;
                let col = if call == "CLEAR" { [0.5, 1.0, 0.5, 0.95] } else { [1.0, 0.85, 0.2, 0.98] };
                rect(out, w / 2.0 - tw / 2.0 - 14.0 * s, 20.0 * s, tw + 28.0 * s, size + 20.0 * s, [0.0, 0.0, 0.0, 0.5]);
                text(out, call, w / 2.0 - tw / 2.0, 30.0 * s, size, col);
            }
        }
        // From outside there is no dash: speed, gear and revs at the bottom.
        if let (1, Some(c)) = (view, you) {
            let d = self.dash(c, hud, time);
            let size = 40.0 * s;
            let line = format!("{:>3} {}  G{}  {:>5}", d.speed_shown.round() as i64, d.unit, d.gear, (d.rpm / 10.0).round() as i64 * 10);
            let tw = geom::char_w(size) * line.len() as f32;
            rect(out, w / 2.0 - tw / 2.0 - 16.0 * s, h - 80.0 * s, tw + 32.0 * s, 64.0 * s, [0.0, 0.0, 0.0, 0.45]);
            text(out, &line, w / 2.0 - tw / 2.0, h - 68.0 * s, size, white);
        }
        if hud.auto {
            text(out, "AUTO", 16.0 * s, h - 40.0 * s, 20.0 * s, [0.6, 0.8, 1.0, 0.9]);
        }
        if hud.replay {
            text(out, "REPLAY", w - 140.0 * s, h - 40.0 * s, 20.0 * s, [1.0, 0.3, 0.25, 0.9]);
        }
    }
}

// ---------------------------------------------------------------- analog controls

/// Keys held → analog values: each axis ramps toward what its keys ask, at its own rates.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Axes {
    pub values: BTreeMap<String, f32>,
}

impl Axes {
    /// One frame: `held` key names, `speed` of your car (m/s) for speed-sensitive axes.
    pub fn update(&mut self, c: &Controls, held: &BTreeSet<String>, dt: f32) {
        for (name, a) in &c.axes {
            let on = |k: &Option<String>| k.as_ref().is_some_and(|k| held.contains(k));
            let target = if a.key.is_some() { if on(&a.key) { 1.0 } else { 0.0 } } else { (on(&a.pos) as i32 - on(&a.neg) as i32) as f32 };
            let v = self.values.entry(name.clone()).or_insert(0.0);
            // Toward a bigger value of the same sign: the rise rate; back toward zero or across it: the fall rate.
            let rising = target != 0.0 && target.signum() == v.signum() || *v == 0.0;
            let ms = if rising && target.abs() > v.abs() { a.rise_ms } else { a.fall_ms };
            let step = if ms <= 0.0 { 2.0 } else { dt * 1000.0 / ms };
            *v += (target - *v).clamp(-step, step);
        }
    }

    /// The action's args: each axis scaled (and limited by speed where asked), as integers.
    pub fn args(&self, c: &Controls, speed: f32) -> BTreeMap<String, i64> {
        c.axes
            .iter()
            .map(|(name, a)| {
                let v = self.values.get(name).copied().unwrap_or(0.0);
                let lim = a.speed_sensitive.map_or(1.0, |(at, keep)| 1.0 - (1.0 - keep) * (speed / at.max(1.0)).clamp(0.0, 1.0));
                (name.clone(), (v * lim * a.scale).round() as i64)
            })
            .collect()
    }
}

// ---------------------------------------------------------------- playing it

/// One accepted input: when, whose, what (a run is its seed and these).
#[derive(Clone, Debug, PartialEq, serde::Serialize, Deserialize)]
pub struct DrivePress {
    pub tick: u64,
    pub entity: EntityId,
    pub action: String,
    #[serde(default)]
    pub args: BTreeMap<String, i64>,
}

pub fn save_run(path: &Path, log: &[DrivePress]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text: String = log.iter().map(|p| serde_json::to_string(p).expect("serialisable") + "\n").collect();
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn load_run(path: &Path) -> Result<Vec<DrivePress>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    text.lines().filter(|l| !l.trim().is_empty()).map(|l| serde_json::from_str(l).map_err(|e| format!("{}: {e}", path.display()))).collect()
}

/// Per car, what the view keeps between ticks: where it was, how far round it has gone, its laps.
#[derive(Clone, Debug, Default)]
struct Track1 {
    prev: Option<(i64, i64, i64)>,
    hint: Option<usize>,
    progress: f32,
    last_s: Option<f32>,
    laps: i64,
    lap_start: Option<f32>,
    last_lap: Option<f32>,
    best_lap: Option<f32>,
    sent: BTreeMap<String, i64>,
}

/// The spotter: who is alongside, and for how long.
#[derive(Clone, Debug, Default)]
struct Spotter {
    alongside_since: Option<f32>,
    clear_at: Option<f32>,
    /// Which side the last car alongside was on ("low", "high", or both: "").
    side: &'static str,
}

/// A game in the driver's seat: the engine, the rig, the controls, the log. What a window, a shot, a recording and
/// the feel probe share.
pub struct DrivePlay {
    pub engine: sim_core::Engine<sim_core::Running, Game>,
    pub drive: Drive,
    pub track: Track,
    pub scene: Scene,
    /// The game's photographs (found when the view starts, decoded when uploaded).
    pub photos: Photos,
    /// The cars' model, fitted (none: box cars).
    pub car_model: Option<CarModel>,
    pub rig: Rig,
    pub time: f32,
    /// How far into the current tick the frame is (0..1).
    pub alpha: f32,
    pub kin: Kin,
    cars: BTreeMap<EntityId, Track1>,
    pub held: BTreeSet<String>,
    pub axes: Axes,
    pub log: Vec<DrivePress>,
    pub script: Option<Vec<DrivePress>>,
    /// The autopilot drives your car too.
    pub auto_you: bool,
    auto_sent: Option<bool>,
    /// The car ridden on board, if not yours.
    pub watch: Option<EntityId>,
    /// Toggle keys that are on.
    toggled: BTreeMap<String, bool>,
    spotter: Spotter,
    /// What the HUD showed on the last frame (the host's sound follows it: the spotter's voice).
    pub last_hud: Hud,
    pub refused: Option<(String, String, f32)>,
    /// Seconds a tick lasts.
    tick_dt: f32,
}

impl DrivePlay {
    pub fn new(mut engine: sim_core::Engine<sim_core::Running, Game>, drive: Drive, track: Track) -> DrivePlay {
        engine.hash_every_tick(false);
        let photos = Photos::resolve(&drive.dir, &drive.look.textures);
        let scene = scene::build(&track, &drive.look, &photos);
        let rate = engine.rules().cfg.run.tick_rate.max(1) as f32;
        let car_model = match load_car_model(&drive, engine.rules()) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("drive.ron: {e} (box cars instead)");
                None
            }
        };
        let mut p = DrivePlay {
            engine,
            drive,
            track,
            scene,
            photos,
            car_model,
            rig: Rig::default(),
            time: 0.0,
            alpha: 1.0,
            kin: Kin::default(),
            cars: BTreeMap::new(),
            held: BTreeSet::new(),
            axes: Axes::default(),
            log: Vec::new(),
            script: None,
            auto_you: false,
            auto_sent: None,
            watch: None,
            toggled: BTreeMap::new(),
            spotter: Spotter::default(),
            last_hud: Hud::default(),
            refused: None,
            tick_dt: 1.0 / rate,
        };
        p.observe();
        p
    }

    pub fn world(&self) -> &World {
        self.engine.world()
    }

    /// The car on screen: the one you watch (Tab), else yours.
    pub fn you(&self) -> Option<EntityId> {
        self.watch.filter(|id| self.world().get(*id).is_some()).or_else(|| self.driver())
    }

    /// Your car: the one the keys drive.
    pub fn driver(&self) -> Option<EntityId> {
        let w = self.world();
        let mut cars = w.of_kind(&self.drive.cars);
        match &self.drive.you {
            Some((p, v)) => w.of_kind(&self.drive.cars).find(|e| e.props.get(p) == Some(v)).or_else(|| cars.next()).map(|e| e.id),
            None => cars.next().map(|e| e.id),
        }
    }

    fn to_track(&self, px: i64, py: i64) -> (f32, f32) {
        ((px - self.drive.origin.0) as f32 / 1000.0, (py - self.drive.origin.1) as f32 / 1000.0)
    }

    /// Every car as drawn this frame (between the last two ticks).
    pub fn cars(&self) -> Vec<CarView> {
        let me = self.you();
        let pr = &self.drive.props;
        let a = self.alpha.clamp(0.0, 1.0);
        self.world()
            .of_kind(&self.drive.cars)
            .filter_map(|e| {
                let now = (prop(e, "px")?, prop(e, "py")?, prop(e, "yaw").unwrap_or(0));
                let st = self.cars.get(&e.id);
                let prev = st.and_then(|s| s.prev).filter(|p| (p.0 - now.0).abs() < 20_000 && (p.1 - now.1).abs() < 20_000).unwrap_or(now);
                let (x0, y0) = self.to_track(prev.0, prev.1);
                let (x1, y1) = self.to_track(now.0, now.1);
                let (x, y) = (x0 + (x1 - x0) * a, y0 + (y1 - y0) * a);
                let (ya, yb) = (yaw_rad(prev.2), yaw_rad(now.2));
                let yaw = ya + wrap_pi(yb - ya) * a;
                let place = self.track.locate(fx(x), fx(y), st.and_then(|s| s.hint));
                let moved = ((x1 - x0).powi(2) + (y1 - y0).powi(2)).sqrt() / self.tick_dt;
                let n = |p: &str| prop(e, p);
                let f = |p: &str, k: f32| prop(e, p).map(|v| v as f32 * k);
                Some(CarView {
                    id: e.id,
                    you: Some(e.id) == me,
                    x,
                    y,
                    yaw,
                    speed: f(&pr.speed, 0.001).unwrap_or(moved),
                    rpm: f(&pr.rpm, 1.0),
                    gear: n(&pr.gear),
                    steer: f(&pr.steer, 0.001).unwrap_or(0.0),
                    throttle: f(&pr.throttle, 0.001).unwrap_or(0.0),
                    brake: f(&pr.brake, 0.001).unwrap_or(0.0),
                    lap: n(&pr.lap),
                    lap_ms: n(&pr.lap_time_ms),
                    last_lap_ms: n(&pr.last_lap_ms),
                    best_lap_ms: n(&pr.best_lap_ms),
                    position: n(&pr.position),
                    number: n(&pr.number).unwrap_or(e.id as i64),
                    impact: f(&pr.impact, 1.0).unwrap_or(0.0),
                    line: self.drive.line_marker.as_ref().and_then(|m| {
                        let on = m.when.as_ref().is_none_or(|(p, v)| prop(e, p) == Some(*v));
                        on.then(|| f(&m.prop, 0.001)).flatten()
                    }),
                    accel: match (f(&pr.g_long, 0.001), f(&pr.g_lat, 0.001)) {
                        (Some(l), Some(t)) => Some((l, t)),
                        _ => None,
                    },
                    body: match (prop(e, &pr.pitch), prop(e, &pr.roll)) {
                        (Some(p), Some(r)) => Some((wrap_pi(yaw_rad(p)), wrap_pi(yaw_rad(r)))),
                        _ => None,
                    },
                    s: fl(place.s),
                    offset: fl(place.offset),
                    seg: place.seg,
                })
            })
            .collect()
    }

    /// After a tick: where every car is on the track, its progress and laps, and your car's motion.
    fn observe(&mut self) {
        let len = fl(self.track.length);
        let now = self.time;
        let dt = self.tick_dt;
        let me = self.you();
        let ents: Vec<(EntityId, i64, i64, i64, Option<i64>)> = self
            .world()
            .of_kind(&self.drive.cars)
            .filter_map(|e| Some((e.id, prop(e, "px")?, prop(e, "py")?, prop(e, "yaw").unwrap_or(0), prop(e, &self.drive.props.speed))))
            .collect();
        for (id, px, py, yaw, speed) in ents {
            let (x, y) = self.to_track(px, py);
            let st = self.cars.entry(id).or_default();
            let place = self.track.locate(fx(x), fx(y), st.hint);
            st.hint = Some(place.seg);
            let s = fl(place.s);
            match st.last_s {
                None => {
                    // Before the line at the start (on the grid): negative progress.
                    st.progress = if s > len / 2.0 { s - len } else { s };
                }
                Some(ls) => {
                    let mut d = s - ls;
                    if d > len / 2.0 {
                        d -= len;
                    } else if d < -len / 2.0 {
                        d += len;
                    }
                    let before = st.progress;
                    st.progress += d;
                    // A lap: the progress passed a multiple of the length (timed to the fraction of the tick).
                    if (st.progress / len).floor() > (before / len).floor() && st.progress > 0.0 {
                        let line = (st.progress / len).floor() * len;
                        let frac = if d.abs() > 1e-6 { (line - before) / d } else { 1.0 };
                        let at = now - dt + frac * dt;
                        if let Some(start) = st.lap_start {
                            let t = at - start;
                            st.last_lap = Some(t);
                            st.best_lap = Some(st.best_lap.map_or(t, |b: f32| b.min(t)));
                        }
                        st.lap_start = Some(at);
                        st.laps += 1;
                    }
                }
            }
            st.last_s = Some(s);
            if Some(id) == me {
                let prev = st.prev;
                if let Some((ppx, ppy, pyaw)) = prev {
                    let moved = (((px - ppx) as f32).powi(2) + ((py - ppy) as f32).powi(2)).sqrt() / 1000.0 / dt;
                    let v = speed.map_or(moved, |s| s as f32 / 1000.0);
                    let yr = wrap_pi(yaw_rad(yaw) - yaw_rad(pyaw)) / dt;
                    let k = 1.0 - (-dt / 0.08).exp();
                    let kin = &mut self.kin;
                    let a_long = (v - kin.speed) / dt;
                    kin.a_long += (a_long.clamp(-60.0, 60.0) - kin.a_long) * k;
                    kin.yaw_rate += (yr - kin.yaw_rate) * k;
                    kin.speed = v;
                    kin.a_lat += (v * kin.yaw_rate - kin.a_lat) * k;
                }
            }
        }
    }

    fn remember(&mut self) {
        let snap: Vec<(EntityId, (i64, i64, i64))> = self
            .world()
            .of_kind(&self.drive.cars)
            .filter_map(|e| Some((e.id, (prop(e, "px")?, prop(e, "py")?, prop(e, "yaw").unwrap_or(0)))))
            .collect();
        for (id, p) in snap {
            self.cars.entry(id).or_default().prev = Some(p);
        }
    }

    /// Asks the game to take `action` for `id`; queued for the next tick and logged, or refused (kept for the title).
    fn send(&mut self, id: EntityId, action: &str, args: BTreeMap<String, i64>) -> bool {
        match self.engine.rules().act(self.world(), None, id, action, &args) {
            Ok(group) => {
                self.engine.queue(group);
                self.log.push(DrivePress { tick: self.world().tick, entity: id, action: action.into(), args });
                true
            }
            Err(why) => {
                if Some(id) == self.driver() {
                    self.refused = Some((action.into(), why, self.time));
                }
                false
            }
        }
    }

    /// Inputs for the tick about to run: the autopilot switched on or off when asked, else yours from the axes;
    /// only what changed is sent.
    fn inputs(&mut self) {
        let tick = self.world().tick;
        if let Some(script) = &self.script {
            let due: Vec<DrivePress> = script.iter().filter(|p| p.tick == tick).cloned().collect();
            for p in due {
                self.send(p.entity, &p.action, p.args);
            }
            return;
        }
        let Some(id) = self.driver() else { return };
        if let Some(ap) = self.drive.autopilot.clone()
            && self.auto_sent != Some(self.auto_you)
        {
            let args = BTreeMap::from([(ap.arg.clone(), self.auto_you as i64)]);
            if self.send(id, &ap.action, args) {
                self.auto_sent = Some(self.auto_you);
                // Taking the wheel back: the pedals and the wheel are sent again as they are now.
                self.cars.entry(id).or_default().sent.clear();
            }
        }
        if self.auto_you {
            return;
        }
        if let Some(c) = self.drive.controls.clone() {
            let speed = self.cars().iter().find(|v| v.id == id).map_or(0.0, |v| v.speed);
            let args = self.axes.args(&c, speed);
            if self.cars.get(&id).is_none_or(|s| s.sent != args) && self.send(id, &c.action, args.clone()) {
                self.cars.entry(id).or_default().sent = args;
            }
        }
    }

    /// A button's key: its action for your car.
    pub fn key(&mut self, key: &str) -> bool {
        let Some(c) = &self.drive.controls else { return false };
        let Some(b) = c.buttons.iter().find(|b| b.key == key).cloned() else { return false };
        if self.script.is_some() {
            return true;
        }
        if let Some(id) = self.driver() {
            let mut args = b.args.clone();
            // A toggle flips its arg between 1 and 0 each press (reverse in, reverse out).
            if let Some(t) = &b.toggle {
                let on = !self.toggled.get(key).copied().unwrap_or(false);
                let (off_v, on_v) = b.values.unwrap_or((0, 1));
                args.insert(t.clone(), if on { on_v } else { off_v });
                if self.send(id, &b.action, args) {
                    self.toggled.insert(key.to_string(), on);
                }
            } else {
                self.send(id, &b.action, args);
            }
        }
        true
    }

    /// A new run on `engine` (the same game started again), keeping the view and the scene.
    pub fn restart(&mut self, mut engine: sim_core::Engine<sim_core::Running, Game>) {
        engine.hash_every_tick(false);
        self.engine = engine;
        self.rig = Rig { view: self.rig.view, ..Rig::default() };
        self.time = 0.0;
        self.alpha = 1.0;
        self.kin = Kin::default();
        self.cars.clear();
        self.axes = Axes::default();
        self.log.clear();
        self.script = None;
        self.spotter = Spotter::default();
        self.refused = None;
        self.auto_sent = None;
        self.toggled.clear();
        self.observe();
    }

    /// Runs `n` ticks.
    pub fn step(&mut self, n: u32) {
        for _ in 0..n {
            if self.engine.outcome().is_some() || self.world().tick >= self.engine.rules().cfg.run.max_ticks {
                return;
            }
            self.inputs();
            self.remember();
            self.engine.tick();
            self.observe();
        }
    }

    /// The frame for a `w × h` target; `dt` since the last one.
    pub fn frame(&mut self, w: f32, h: f32, dt: f32) -> DriveFrame {
        if let Some(c) = self.drive.controls.clone() {
            self.axes.update(&c, &self.held, dt);
        }
        let cars = self.cars();
        let hud = self.hud(&cars);
        self.last_hud = hud.clone();
        let composer = Composer {
            drive: &self.drive,
            track: &self.track,
            ground: &self.scene.ground,
            photos: &self.photos,
            car: self.car_model.as_ref(),
            sun: self.scene.sun,
            w,
            h,
        };
        composer.compose(&cars, &self.kin, &hud, &mut self.rig, self.time, dt)
    }

    /// The running order, the gaps, the laps and the spotter's call.
    fn hud(&mut self, cars: &[CarView]) -> Hud {
        let len = fl(self.track.length);
        let Some(you) = cars.iter().find(|c| c.you) else { return Hud::default() };
        let progress = |c: &CarView| match (c.lap, self.cars.get(&c.id)) {
            (Some(l), _) if l > 0 => (l - 1) as f32 * len + c.s,
            (_, Some(st)) => st.progress,
            _ => c.s,
        };
        let mut order: Vec<(f32, &CarView)> = cars.iter().map(|c| (progress(c), c)).collect();
        order.sort_by(|a, b| b.0.total_cmp(&a.0));
        let idx = order.iter().position(|(_, c)| c.you).unwrap_or(0);
        let position = you.position.map_or(idx + 1, |p| p.max(1) as usize);
        let mine = order[idx].0;
        let speed = you.speed.max(5.0);
        let gap_ahead = idx.checked_sub(1).map(|i| (order[i].0 - mine) / speed);
        let gap_behind = order.get(idx + 1).map(|(p, _)| (mine - p) / speed);
        let st = self.cars.get(&you.id).cloned().unwrap_or_default();
        let lap = you.lap.unwrap_or(st.laps);
        let lap_time = match you.lap_ms {
            Some(ms) if lap > 0 => Some(ms as f32 / 1000.0),
            Some(_) => None,
            None => st.lap_start.map(|s| self.time - s),
        };
        let last_lap = you.last_lap_ms.filter(|m| *m > 0).map(|m| m as f32 / 1000.0).or(st.last_lap);
        let best_lap = you.best_lap_ms.filter(|m| *m > 0).map(|m| m as f32 / 1000.0).or(st.best_lap);
        // Alongside: overlapping lengthwise, within three lanes sideways. Low is the inside (left, positive offset).
        let (mut low, mut high) = (false, false);
        for c in cars.iter().filter(|c| !c.you) {
            let mut ds = c.s - you.s;
            if ds > len / 2.0 {
                ds -= len;
            } else if ds < -len / 2.0 {
                ds += len;
            }
            let dof = c.offset - you.offset;
            if ds.abs() < 5.2 && dof.abs() < 6.0 && dof.abs() > 0.8 {
                if dof > 0.0 {
                    low = true;
                } else {
                    high = true;
                }
            }
        }
        let now = self.time;
        let sp = &mut self.spotter;
        let (call, voice) = if low || high {
            let since = *sp.alongside_since.get_or_insert(now);
            sp.clear_at = None;
            sp.side = if low && high {
                ""
            } else if low {
                "low"
            } else {
                "high"
            };
            let (c, v) = if low && high {
                ("THREE WIDE", "three_wide")
            } else if now - since > 2.5 {
                ("STILL THERE", "still_there")
            } else if low {
                ("CAR LOW", "car_low")
            } else {
                ("CAR HIGH", "car_high")
            };
            (Some(c.to_string()), Some(v.to_string()))
        } else {
            if sp.alongside_since.take().is_some() {
                sp.clear_at = Some(now);
            }
            let side = sp.side;
            let on = sp.clear_at.filter(|t| now - t < 1.2).is_some();
            (on.then(|| "CLEAR".to_string()), on.then(|| if side.is_empty() { "clear".to_string() } else { format!("clear_{side}") }))
        };
        Hud {
            call,
            voice,
            position,
            cars: cars.len(),
            lap,
            gap_ahead,
            gap_behind,
            lap_time,
            last_lap,
            best_lap,
            auto: self.auto_you,
            replay: self.script.is_some(),
        }
    }

    /// Ride on board the next car (Tab), round to your own.
    pub fn watch_next(&mut self) {
        let ids: Vec<EntityId> = self.world().of_kind(&self.drive.cars).map(|e| e.id).collect();
        let now = self.you();
        let next = ids.iter().position(|id| Some(*id) == now).map_or(0, |i| (i + 1) % ids.len().max(1));
        self.watch = ids.get(next).copied().filter(|id| Some(*id) != self.driver());
        // Another car: its motion is measured afresh (the head does not carry the last car's g over).
        self.kin = Kin::default();
        self.rig = Rig { view: self.rig.view, ..Rig::default() };
    }

    pub fn cycle_view(&mut self) {
        self.rig.view = (self.rig.view + 1) % 3;
    }

    pub fn title(&self, speed: f32, paused: bool) -> String {
        let you = self.cars().into_iter().find(|c| c.you);
        let what =
            you.map_or_else(|| "no car".to_string(), |c| format!("{:.0} km/h, s {:.0} m, offset {:.1} m", c.speed * 3.6, c.s, c.offset));
        let refused = match &self.refused {
            Some((a, why, at)) if self.time - at < 3.0 => format!(" — {a}: {why}"),
            _ => String::new(),
        };
        format!(
            "{} — {} — {what} — {}{refused} — C: camera, arrows: drive",
            self.engine.rules().def.name,
            self.track.name,
            if paused { "paused".to_string() } else { format!("{speed:.0} ticks/s") }
        )
    }

    /// Draws a frame into `target`: the mirror's picture first, then the view.
    pub fn render(&self, gpu: &mut Gpu, target: &wgpu::TextureView, w: u32, h: u32, f: &DriveFrame) {
        gpu.keep(SCENE_SLOT, 1, &self.scene.meshes);
        if let Some((eye, back, (mw, mh))) = &f.mirror {
            let world = f.world3(eye, &f.meshes[..f.mirror_meshes], *mw as f32, *mh as f32);
            gpu.render_into(MIRROR, *mw, *mh, back, world);
        }
        let world = f.world3(&f.eye, &f.meshes, w as f32, h as f32);
        gpu.render(target, w, h, &f.back, Some(world), &f.front);
    }

    /// A frame as a picture.
    pub fn shot(&self, gpu: &mut Gpu, w: u32, h: u32, f: &DriveFrame) -> sim_render::image::Image {
        gpu.keep(SCENE_SLOT, 1, &self.scene.meshes);
        if let Some((eye, back, (mw, mh))) = &f.mirror {
            let world = f.world3(eye, &f.meshes[..f.mirror_meshes], *mw as f32, *mh as f32);
            gpu.render_into(MIRROR, *mw, *mh, back, world);
        }
        let world = f.world3(&f.eye, &f.meshes, w as f32, h as f32);
        gpu.shot_scene(w, h, &f.back, Some(world), &f.front)
    }
}

/// Uploads the pictures the drive view makes for itself.
pub fn upload_textures(gpu: &mut Gpu) {
    for (name, img) in geom::textures() {
        gpu.upload(&name, &img, 1024);
    }
}

impl DrivePlay {
    /// Everything the view draws with onto the GPU: its own pictures, the game's photographs, the cars' model (a
    /// GPU without storage buffers draws box cars instead).
    pub fn upload(&mut self, gpu: &mut Gpu) {
        upload_textures(gpu);
        self.photos.upload(gpu);
        match &self.car_model {
            Some(m) if gpu.models_supported() => gpu.upload_model(CAR_MODEL, &m.model, &BTreeMap::new()),
            Some(_) => self.car_model = None,
            None => {}
        }
    }
}

/// The cars' model from `look.model`, fitted to the kind's footprint (its `motion` size: mm in a 1 m cell world).
pub fn load_car_model(drive: &Drive, game: &Game) -> Result<Option<CarModel>, String> {
    let Some(m) = &drive.look.model else { return Ok(None) };
    let path = photos::find(&drive.dir, &m.file).ok_or_else(|| format!("look.model: {} not found", m.file))?;
    let size = game
        .def
        .kinds
        .get(&drive.cars)
        .and_then(|k| k.motion.as_ref())
        .map_or((2.0, 4.9), |mo| (mo.size.0 as f32 / 1000.0, mo.size.1 as f32 / 1000.0));
    CarModel::load(&path, m, size).map(Some)
}

// ---------------------------------------------------------------- feel, measured

/// How the drive view feels, measured on a run played headless at a fixed frame rate through the window's code.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct DriveFeel {
    /// Frames where your car was moving but the eye did not (waiting for a tick): 0 = it glides.
    pub stall: f32,
    /// Largest change of the eye's velocity between frames (m a frame), vibration left out.
    /// (99th percentile; the largest, which contact makes, beside it.)
    pub eye_jerk: f32,
    pub eye_jerk_max: f32,
    /// How long the head takes to answer a sideways g-force (ms, the lag of best correlation).
    pub head_g_lag: f32,
    /// How far the horizon drawn is from where the neck aims it (degrees, mean), and its largest tilt.
    pub horizon_error: f32,
    pub horizon_deg: f32,
    /// The head's largest excursion (mm) and the largest sideways g felt.
    pub head_max_mm: f32,
    /// Where your car ran across the track (m left of the centreline: least, most): inside the walls?
    pub offset: (f32, f32),
    pub felt_g_max: f32,
    pub top_speed: f32,
    pub lap_s: f32,
    pub tick_ms: f32,
    pub frame_ms: f32,
    pub worst_ms: f32,
    pub p99_ms: f32,
    pub over_8ms: u32,
    pub frames: u32,
}

/// Plays `frames` frames at `fps` and measures them.
pub fn feel_probe(play: &mut DrivePlay, frames: u32, fps: f32) -> DriveFeel {
    let rate = play.engine.rules().cfg.run.tick_rate as f32;
    let dt = 1.0 / fps;
    let mut clock = 0.0f32;
    let (mut tick_s, mut frame_s, mut ticks) = (0.0f64, 0.0f64, 0u32);
    let mut costs: Vec<f64> = Vec::new();
    let (mut eyes, mut lat, mut head, mut horizon, mut rolls) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let (mut stalled, mut moving, mut head_max, mut g_max, mut top) = (0u32, 0u32, 0.0f32, 0.0f32, 0.0f32);
    let mut off = (f32::MAX, f32::MIN);
    let mut played = 0;
    for _ in 0..frames {
        if play.engine.outcome().is_some() {
            break;
        }
        played += 1;
        clock += dt * rate;
        let n = clock.floor() as u32;
        clock -= n as f32;
        let t0 = std::time::Instant::now();
        play.step(n);
        tick_s += t0.elapsed().as_secs_f64();
        ticks += n;
        play.alpha = clock;
        play.time += dt;
        let t1 = std::time::Instant::now();
        let f = play.frame(1280.0, 720.0, dt);
        frame_s += t1.elapsed().as_secs_f64();
        costs.push(t0.elapsed().as_secs_f64() * 1000.0);
        let o = play.rig.out;
        let _ = f;
        eyes.push(o.eye_base);
        lat.push(o.felt.0);
        head.push(o.head.0);
        horizon.push((o.roll - o.roll_target).abs());
        rolls.push(o.roll.abs());
        head_max = head_max.max((o.head.dot(o.head)).sqrt() * 1000.0);
        g_max = g_max.max(o.felt.0.abs());
        top = top.max(play.kin.speed);
        if let Some(c) = play.cars().into_iter().find(|c| c.you) {
            off = (off.0.min(c.offset), off.1.max(c.offset));
        }
        if eyes.len() >= 2 && play.kin.speed > 1.0 {
            moving += 1;
            let d = eyes[eyes.len() - 1] - eyes[eyes.len() - 2];
            if d.dot(d).sqrt() < play.kin.speed * dt * 0.1 {
                stalled += 1;
            }
        }
    }
    // (A car placed or reset by the game jumps: steps over 10 m are not motion.)
    let len = |d: V3| d.dot(d).sqrt();
    let jerk = eyes
        .windows(3)
        .filter(|w| len(w[1] - w[0]) < 10.0 && len(w[2] - w[1]) < 10.0)
        .map(|w| len((w[2] - w[1]) - (w[1] - w[0])))
        .fold(0.0, f32::max);
    let mut jerks: Vec<f32> = eyes.windows(3).map(|w| len((w[2] - w[1]) - (w[1] - w[0]))).collect();
    jerks.sort_by(f32::total_cmp);
    let jerk_p99 = jerks.get(jerks.len() * 99 / 100).copied().unwrap_or(0.0);
    // Lag: the shift (frames) that best lines the head's sideways offset up with the sideways g felt.
    let corr = |k: usize| -> f32 {
        let n = lat.len().saturating_sub(k);
        if n < 30 {
            return f32::MIN;
        }
        let (ma, mb) = (lat[..n].iter().sum::<f32>() / n as f32, head[k..k + n].iter().sum::<f32>() / n as f32);
        let (mut num, mut da, mut db) = (0.0, 0.0, 0.0);
        for i in 0..n {
            let (a, b) = (lat[i] - ma, head[i + k] - mb);
            num += a * b;
            da += a * a;
            db += b * b;
        }
        num / (da * db).sqrt().max(1e-9)
    };
    let best = (0..(fps as usize / 2)).max_by(|a, b| corr(*a).total_cmp(&corr(*b))).unwrap_or(0);
    let me = play.you();
    let best_prop = play.cars().into_iter().find(|c| c.you).and_then(|c| c.best_lap_ms).filter(|m| *m > 0).map(|m| m as f32 / 1000.0);
    let lap = best_prop.or_else(|| me.and_then(|id| play.cars.get(&id)).and_then(|s| s.best_lap)).unwrap_or(0.0);
    let r = |v: f32| (v * 1000.0).round() / 1000.0;
    let mut sorted = costs.clone();
    sorted.sort_by(f64::total_cmp);
    DriveFeel {
        stall: r(stalled as f32 / moving.max(1) as f32),
        eye_jerk: r(jerk_p99),
        eye_jerk_max: r(jerk),
        head_g_lag: r(best as f32 * 1000.0 / fps),
        horizon_error: r(horizon.iter().sum::<f32>() / horizon.len().max(1) as f32),
        horizon_deg: r(rolls.iter().cloned().fold(0.0, f32::max)),
        head_max_mm: r(head_max),
        offset: (r(off.0), r(off.1)),
        felt_g_max: r(g_max),
        top_speed: r(top),
        lap_s: r(lap),
        tick_ms: r((tick_s * 1000.0 / ticks.max(1) as f64) as f32),
        frame_ms: r((frame_s * 1000.0 / played.max(1) as f64) as f32),
        worst_ms: r(sorted.last().copied().unwrap_or(0.0) as f32),
        p99_ms: r(sorted.get(sorted.len() * 99 / 100).copied().unwrap_or(0.0) as f32),
        over_8ms: costs.iter().filter(|c| **c > 8.0).count() as u32,
        frames: played,
    }
}

#[cfg(test)]
mod tests;
