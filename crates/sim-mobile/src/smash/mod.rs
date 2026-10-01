//! SMASH: pull the slingshot, let go, knock the tower off its pedestal. A stack-and-topple game (Smash Fest's
//! family, `docs/research/mobile-types.md`) on the rigid-body solver (`sim_physics::rigid`), drawn in 3D.
//!
//! The core is tuned first, levels later: one test tower, unlimited stones, and every number that shapes the feel in
//! `games/smash/smash.ron`. What happens is written to an event log; the evals (`simcraft-smash eval`) read it, the
//! haptics and the camera react to it. Game time can slow (hit-stop, slow motion); the camera, the band and the
//! haptics run on real time, so a frozen frame still shakes.

pub mod camera;
#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub mod eval;
pub mod fx;
pub mod look;
#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub mod precision;
pub mod sling;
pub mod tuning;

use sim_physics::rigid::{BodyDef, BodyId, Material, Quat, Shape, V3, World};

use crate::gesture::{Gesture, Px};
use crate::haptics::{Kind as Haptic, Pulse};
use crate::layer::{Fit, Frame, Rect};
use crate::playground::{Card, Layout, TICK_RATE};
use crate::sensors::Sense;
use camera::{Pose, Rig};
use fx::{Chips, Floor, Rng};
use sling::{Aim, Band, Preview};
use tuning::{MaterialTuning, Tuning};

/// Collision layers: the static world, the tower and stones, debris (touches only the static world).
pub const STATIC: u32 = 1;
pub const SOLID: u32 = 2;
pub const DEBRIS: u32 = 4;

/// What a body is, as the game sees it (its `user` tag in the solver).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Ground = 0,
    Pedestal = 1,
    Wood = 2,
    Stone = 3,
    Can = 4,
    Glass = 5,
    Shard = 6,
    Ball = 7,
}

impl Kind {
    pub fn of_user(user: u32) -> Kind {
        match user {
            0 => Kind::Ground,
            1 => Kind::Pedestal,
            2 => Kind::Wood,
            3 => Kind::Stone,
            4 => Kind::Can,
            5 => Kind::Glass,
            6 => Kind::Shard,
            _ => Kind::Ball,
        }
    }

    /// A piece of the tower (what has to leave the table).
    pub fn is_piece(self) -> bool {
        matches!(self, Kind::Wood | Kind::Stone | Kind::Can | Kind::Glass)
    }
}

/// What happened, in order (ticks are real ticks since the tower was built).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// A stone left the pouch: the aim, where the preview said it would first touch something, and where it started.
    Shot {
        tick: u64,
        aim: Aim,
        predicted: Option<V3>,
        from: V3,
        vel: V3,
    },
    /// A stone's first touch: where its centre was, where it touched, and how hard.
    StoneHit {
        tick: u64,
        at: V3,
        contact: V3,
        normal: V3,
        impulse: f32,
        on: Kind,
    },
    Broke {
        tick: u64,
        at: V3,
    },
    /// A piece left the table.
    Cleared {
        tick: u64,
        kind: Kind,
    },
    HitStop {
        tick: u64,
    },
    SlowMo {
        tick: u64,
    },
    /// Every piece is off the table.
    TableClear {
        tick: u64,
    },
    /// The stones ran out with pieces still on the table.
    OutOfStones {
        tick: u64,
    },
    Pulse {
        tick: u64,
        kind: Haptic,
        intensity: f32,
    },
}

#[derive(Clone, Copy, Debug)]
pub struct Piece {
    pub id: BodyId,
    pub kind: Kind,
    pub cleared: bool,
    /// Seconds alive (shards fade after a while).
    pub age: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Stone {
    pub id: BodyId,
    pub age: f32,
    /// Where it first hit, if it has.
    pub hit: Option<V3>,
}

/// A button on the HUD.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Button {
    pub rect: Rect,
    pub label: &'static str,
}

pub struct Smash {
    pub t: Tuning,
    pub world: World,
    pub pieces: Vec<Piece>,
    pub stones: Vec<Stone>,
    pub chips: Chips,
    pub rng: Rng,
    pub rig: Rig,
    pub band: Band,
    /// The finger: where it went down and where it is, while aiming.
    pub drag: Option<(Px, Px)>,
    pub aim: Aim,
    /// Recent aims and the tick of each: a release shoots the one from `release_lock` ago (a lifting finger rolls).
    aims: std::collections::VecDeque<(u64, Aim)>,
    pub preview: Preview,
    /// A stone is in the pouch.
    pub loaded: bool,
    reload: f32,
    /// Seconds left of hit-stop, and of slow motion.
    hitstop: f32,
    slowmo: f32,
    slowmo_used: bool,
    /// The flash on screen now (0..1) and where the camera looks after a hit.
    pub flash: f32,
    focus: Option<V3>,
    since_hit: f32,
    /// How far the camera is pushed in towards what it is interested in (0..1, eased), and the last point of interest.
    push: f32,
    last_interest: Option<V3>,
    /// The pull step last felt (a tick in the hand each step up).
    pull_step: u32,
    pulse_cooldown: f32,
    /// The hardest hit of the current shot (N·s).
    shot_peak: f32,
    pub tick: u64,
    pub shots: u32,
    pub won: Option<u64>,
    /// The level being played (an index into the tuning's levels), its stones left, when they ran out with pieces
    /// still on the table, how many pieces it started with, and how wide its pedestal is.
    pub level: usize,
    pub stones_left: u32,
    pub lost: Option<u64>,
    /// Seconds since the last stone left the pouch.
    since_shot: f32,
    pub total: usize,
    pub pedestal_radius: f32,
    pub log: Vec<Event>,
    /// The screen (pixels) and its scale, as last laid out.
    pub screen: (f32, f32),
    pub scale: f32,
    pub safe: Rect,
    want_lab: bool,
    /// The phone's tilt (gravity in g, x and z) and a baseline that follows it slowly: the camera slides by their
    /// difference, so how the phone happens to be held doesn't matter, only how it moves.
    tilt: (f32, f32),
    tilt_base: Option<(f32, f32)>,
    /// Microseconds the last solver step took (the stats line).
    pub step_us: f32,
    pub fps: f32,
}

impl Default for Smash {
    fn default() -> Smash {
        Smash::new(Tuning::embedded())
    }
}

fn v3(t: (f32, f32, f32)) -> V3 {
    V3::new(t.0, t.1, t.2)
}

/// An sRGB hex colour in linear light.
pub fn lin(rgb: u32) -> [f32; 3] {
    let c = crate::layer::Color::hex(rgb).0;
    [c[0], c[1], c[2]]
}

impl Smash {
    pub fn new(t: Tuning) -> Smash {
        let pose = Smash::aim_pose(&t, Aim::default());
        let mut s = Smash {
            world: World::new(),
            pieces: Vec::new(),
            stones: Vec::new(),
            chips: Chips::default(),
            rng: Rng::new(1),
            rig: Rig::new(pose),
            band: Band::default(),
            drag: None,
            aim: Aim::default(),
            aims: std::collections::VecDeque::new(),
            preview: Preview::default(),
            loaded: true,
            reload: 0.0,
            hitstop: 0.0,
            slowmo: 0.0,
            slowmo_used: false,
            flash: 0.0,
            focus: None,
            since_hit: 0.0,
            push: 0.0,
            last_interest: None,
            pull_step: 0,
            pulse_cooldown: 0.0,
            shot_peak: 0.0,
            tick: 0,
            shots: 0,
            won: None,
            level: 0,
            stones_left: 0,
            lost: None,
            since_shot: 0.0,
            total: 0,
            pedestal_radius: 1.5,
            log: Vec::new(),
            screen: (1179.0, 2556.0),
            scale: 3.0,
            safe: Rect::new(0.0, 0.0, 1179.0, 2556.0),
            want_lab: false,
            tilt: (0.0, 0.0),
            tilt_base: None,
            step_us: 0.0,
            fps: 0.0,
            t,
        };
        s.build();
        s
    }

    /// SMASH at level `name` (the evals keep playing the same tower whatever comes before it).
    pub fn at_level(t: Tuning, name: &str) -> Smash {
        let mut s = Smash::new(t);
        if let Some(i) = s.t.levels.iter().position(|l| l.name == name) {
            s.level = i;
            s.build();
        }
        s
    }

    pub fn level(&self) -> &tuning::Level {
        &self.t.levels[self.level.min(self.t.levels.len() - 1)]
    }

    /// Goes to level `i` (wrapping round) and builds it.
    pub fn go(&mut self, i: isize) {
        let n = self.t.levels.len() as isize;
        self.level = i.rem_euclid(n) as usize;
        self.build();
    }

    /// Stars for a cleared level: one, plus one for each stone left over (up to three).
    pub fn stars(&self) -> u32 {
        1 + self.stones_left.min(2)
    }

    pub fn material(&self, k: Kind) -> MaterialTuning {
        let m = &self.t.materials;
        match k {
            Kind::Stone => m.stone,
            Kind::Can => m.can,
            Kind::Glass | Kind::Shard => m.glass,
            _ => m.wood,
        }
    }

    /// The ground, the pedestal and the tower (asleep: it stands perfectly still until hit).
    pub fn build(&mut self) {
        let t = self.t.clone();
        let mut w = World::new();
        w.gravity = V3::new(0.0, -t.physics.gravity, 0.0);
        w.substeps = t.physics.substeps;
        let fixed = |shape, pos, kind: Kind| BodyDef { fixed: true, user: kind as u32, layer: STATIC, ..BodyDef::new(shape, pos) };
        w.add(fixed(Shape::Box { half: V3::new(60.0, 0.5, 60.0) }, V3::new(0.0, -0.5, 0.0), Kind::Ground));
        let p = &t.pedestal;
        let cell = t.pieces.cell;
        let (cx, cy, cz) = cell;
        let level = self.level().clone();
        let (hw, hd) = level.half_extent(cell);
        // As wide as the tower needs, with a hand's width to spare.
        self.pedestal_radius = p.radius.max((hw * hw + hd * hd).sqrt() + 0.2);
        w.add(fixed(
            Shape::Prism { radius: self.pedestal_radius, half_height: p.thickness / 2.0, sides: 24 },
            V3::new(0.0, p.top - p.thickness / 2.0, 0.0),
            Kind::Pedestal,
        ));
        let post_h = (p.top - p.thickness) / 2.0;
        w.add(fixed(Shape::Prism { radius: 0.16, half_height: post_h, sides: 12 }, V3::new(0.0, post_h, 0.0), Kind::Pedestal));
        self.pieces.clear();
        let depth = level.depth.max(1);
        for (row, line) in level.rows.iter().enumerate() {
            let chars: Vec<char> = line.chars().collect();
            let n = chars.len() as f32;
            // A hair of air between layers, so nothing starts inside anything.
            let y = p.top + cy * (row as f32 + 0.5) + 0.0005 * (row as f32 + 1.0);
            let mut col = 0;
            while col < chars.len() {
                let ch = chars[col];
                // A run of beam cells is one long piece.
                let run = if ch == '=' || ch == '#' { chars[col..].iter().take_while(|&&c| c == ch).count() } else { 1 };
                let kind = match ch {
                    'w' | '=' => Kind::Wood,
                    's' | '#' => Kind::Stone,
                    'c' => Kind::Can,
                    'g' => Kind::Glass,
                    _ => {
                        col += 1;
                        continue;
                    }
                };
                let x = (col as f32 + (run as f32 - 1.0) / 2.0 - (n - 1.0) / 2.0) * cx;
                for d in 0..depth {
                    let z = (d as f32 - (depth as f32 - 1.0) / 2.0) * cz;
                    let shape = match kind {
                        Kind::Can | Kind::Glass => Shape::Prism { radius: cx.min(cz) * 0.46, half_height: cy / 2.0, sides: 8 },
                        _ => Shape::Box { half: V3::new(cx * (run as f32 * 0.5 - 0.01), cy / 2.0, cz * 0.49) },
                    };
                    let m = self.material(kind);
                    let id = w.add(BodyDef {
                        material: Material { friction: m.friction, restitution: m.restitution, density: m.density },
                        asleep: true,
                        user: kind as u32,
                        layer: SOLID,
                        ..BodyDef::new(shape, V3::new(x, y, z))
                    });
                    self.pieces.push(Piece { id, kind, cleared: false, age: 0.0 });
                }
                col += run;
            }
        }
        self.total = self.pieces.len();
        self.stones_left = level.stones;
        self.lost = None;
        self.world = w;
        self.stones.clear();
        self.chips.list.clear();
        self.rng = Rng::new(1);
        self.loaded = true;
        self.reload = 0.0;
        self.hitstop = 0.0;
        self.slowmo = 0.0;
        self.slowmo_used = false;
        self.flash = 0.0;
        self.focus = None;
        self.push = 0.0;
        self.last_interest = None;
        self.won = None;
        self.drag = None;
        self.aim = Aim::default();
        self.band = Band::default();
        self.tick = 0;
        self.shots = 0;
        self.log.clear();
        let pose = Smash::aim_pose(&self.t, Aim::default());
        self.rig.snap(pose);
    }

    /// What a pull can aim at on this tower.
    pub fn reach(&self) -> sling::Reach {
        let t = &self.t;
        let top = t.pedestal.top + self.level().height(t.pieces.cell);
        sling::Reach { low: t.pedestal.top + t.sling.aim_low, high: top + t.sling.aim_over, plane_z: 0.0, gravity: t.physics.gravity }
    }

    /// The pose for aiming with `aim`: leaning into the pull, and sliding with the aim while still looking at the
    /// tower, so the tower stays put under the finger and the beach behind it glides past (parallax round the
    /// target; when the look point slid too, the aim mark drifted 20 px after the finger stopped: EVALS step 011).
    pub fn aim_pose(t: &Tuning, aim: Aim) -> Pose {
        let c = &t.camera;
        // Degrees the aim turns from straight ahead (seen from the slingshot).
        let yaw_deg = (-aim.target.x).atan2(t.sling.at.2 - aim.target.z).to_degrees();
        let p = aim.power;
        let mut eye = v3(c.eye);
        let look = v3(c.look);
        eye.x -= c.lean * yaw_deg;
        eye.y -= c.pull_drop * p;
        Pose { eye, look, fov: c.fov + c.pull_fov * p }
    }

    /// What the camera is interested in now: the flying stone, then where it hit (the same point at the moment
    /// of the hit, so interest never jumps).
    fn interest(&self) -> Option<V3> {
        let flying = self.stones.last().filter(|s| s.hit.is_none() && s.age < 1.6).and_then(|s| self.world.get(s.id)).map(|b| b.pos);
        flying.or_else(|| self.focus.filter(|_| self.since_hit < self.t.camera.settle))
    }

    /// Where the camera wants to be now: the aim pose, pushed in towards the interest by `push` (which eases
    /// between 0 and 1, so the pose glides instead of switching).
    pub fn target_pose(&self) -> Pose {
        let t = &self.t;
        let c = &t.camera;
        let base = Smash::aim_pose(t, self.aim);
        let Some(at) = self.last_interest else { return base };
        let k = c.follow * self.push;
        let near = Pose { eye: at + V3::new(0.0, 1.0, 4.0), look: at, fov: c.fov + c.follow_fov };
        let mut p = base.lerp(near, k);
        p.look = base.look.lerp(at, (k * 1.4).min(1.0));
        p
    }

    fn pulse(&mut self, kind: Haptic, intensity: f32, sharpness: f32, out: &mut Vec<Pulse>) {
        out.push(Pulse::new(kind, intensity, sharpness));
        self.log.push(Event::Pulse { tick: self.tick, kind, intensity });
    }

    /// The finger went down at `at`.
    pub fn press(&mut self, at: Px, out: &mut Vec<Pulse>) {
        for b in look::buttons(self) {
            let r = b.rect;
            if at.x >= r.x && at.x <= r.x + r.w && at.y >= r.y && at.y <= r.y + r.h {
                match b.label {
                    "RESET" => self.build(),
                    "-" => self.go(self.level as isize - 1),
                    "+" => self.go(self.level as isize + 1),
                    _ => self.want_lab = true,
                }
                self.pulse(Haptic::Tap, 0.5, 0.8, out);
                return;
            }
        }
        if self.loaded {
            self.drag = Some((at, at));
            self.pull_step = 0;
            self.pulse(Haptic::Tap, 0.35, 0.6, out);
        }
    }

    /// The finger moved to `at`.
    pub fn pull(&mut self, at: Px, out: &mut Vec<Pulse>) {
        let Some((start, _)) = self.drag else { return };
        self.drag = Some((start, at));
        self.aim = Aim::from_drag(start, at, self.screen.0, self.screen.1, &self.t.sling, &self.reach());
        self.aims.push_back((self.tick, self.aim));
        while self.aims.len() > 64 {
            self.aims.pop_front();
        }
        let n = self.t.sling.ticks.max(1);
        let step = (self.aim.power * n as f32).floor() as u32;
        if step > self.pull_step {
            // Rising ticks, and a firmer click at the full pull: the band can't stretch further.
            if step >= n {
                self.pulse(Haptic::Thud, 0.55, 0.9, out);
            } else {
                self.pulse(Haptic::Tick, 0.15 + 0.5 * step as f32 / n as f32, 0.85, out);
            }
        }
        self.pull_step = step;
    }

    /// The finger lifted: shoot, or put the stone back if it was barely pulled.
    pub fn release(&mut self, out: &mut Vec<Pulse>) {
        if self.drag.take().is_none() {
            return;
        }
        // The aim from `release_lock` ago (the arc the player was looking at), not the roll of the lifting finger.
        let lock = (self.t.sling.release_lock * TICK_RATE as f32).round() as u64;
        let aim = self.aims.iter().rev().find(|(t, _)| t + lock <= self.tick).map_or(self.aim, |&(_, a)| a);
        self.aims.clear();
        self.aim = Aim::default();
        if aim.power < sling::MIN_POWER {
            return;
        }
        let t = &self.t;
        let (from, vel) = (aim.from, aim.vel);
        let predicted = self.preview.hit.map(|(c, n)| c - n * self.t.stone.radius);
        let st = &t.stone;
        let id = self.world.add(BodyDef {
            vel,
            material: Material { friction: st.friction, restitution: st.restitution, density: st.density },
            user: Kind::Ball as u32,
            layer: SOLID,
            ..BodyDef::new(Shape::Sphere { radius: st.radius }, from)
        });
        self.stones.push(Stone { id, age: 0.0, hit: None });
        let rest = v3(t.sling.at);
        self.band = Band { off: from - rest, vel: vel * 0.35 };
        self.loaded = false;
        self.reload = t.sling.reload;
        self.stones_left = self.stones_left.saturating_sub(1);
        self.since_shot = 0.0;
        self.slowmo_used = false;
        self.shot_peak = 0.0;
        self.shots += 1;
        self.log.push(Event::Shot { tick: self.tick, aim, predicted, from, vel });
        self.pulse(Haptic::Thud, 0.45 + 0.5 * aim.power, 0.55, out);
    }

    fn floor(&self) -> Floor {
        Floor { radius: self.pedestal_radius, top: self.t.pedestal.top }
    }

    /// What the motion sensor reads this tick.
    pub fn sense(&mut self, sense: &Sense) {
        let Some([x, _, z]) = sense.gravity else { return };
        let now = (x as f32 / 1000.0, z as f32 / 1000.0);
        let base = self.tilt_base.get_or_insert(now);
        // The baseline catches up over about two seconds.
        let k = 1.0 - (-1.0 / (2.0 * TICK_RATE as f32)).exp();
        base.0 += (now.0 - base.0) * k;
        base.1 += (now.1 - base.1) * k;
        self.tilt = (now.0 - base.0, now.1 - base.1);
    }

    /// One real tick.
    pub fn tick(&mut self, out: &mut Vec<Pulse>) {
        let dt = 1.0 / TICK_RATE as f32;
        self.tick += 1;
        let f = self.t.feel.clone();
        let scale = if self.hitstop > 0.0 {
            self.hitstop -= dt;
            if self.hitstop <= 0.0 && self.slowmo > 0.0 {
                self.log.push(Event::SlowMo { tick: self.tick });
            }
            0.0
        } else if self.slowmo > 0.0 {
            self.slowmo -= dt;
            // Eases back to full speed over its time.
            let k = 1.0 - (self.slowmo / f.slowmo_time).clamp(0.0, 1.0);
            f.slowmo_scale + (1.0 - f.slowmo_scale) * k * k
        } else {
            1.0
        };
        let gdt = dt * scale;
        let started = std::time::Instant::now();
        self.world.step(gdt);
        self.step_us = started.elapsed().as_secs_f32() * 1e6;
        if gdt > 0.0 {
            self.impacts(out);
        }
        self.chips.step(gdt, self.t.physics.gravity, self.floor());
        for s in &mut self.stones {
            s.age += gdt;
        }
        // Stones that came to rest or fell far are taken away (one shot's stone stays a while as debris).
        let world = &mut self.world;
        self.stones.retain(|s| {
            let keep = s.age < 6.0 && world.get(s.id).is_some_and(|b| b.pos.y > -2.0);
            if !keep {
                world.remove(s.id);
            }
            keep
        });
        self.update_pieces(gdt, out);
        let (hz, z) = (self.t.sling.band_hz, self.t.sling.band_damping);
        self.band.step(dt, hz, z);
        if !self.loaded {
            self.reload -= dt;
            if self.reload <= 0.0 && self.won.is_none() && self.stones_left > 0 {
                self.loaded = true;
                self.band = Band::default();
                self.pulse(Haptic::Tap, 0.25, 0.4, out);
            }
        }
        self.since_hit += dt;
        self.flash = (self.flash - dt * 4.0).max(0.0);
        self.pulse_cooldown -= dt;
        let c = self.t.camera.clone();
        let (tx, tz) = self.tilt;
        let tilt = V3::new(tx * c.tilt, -tz * c.tilt * 0.6, 0.0);
        let interest = if self.drag.is_some() { None } else { self.interest() };
        if interest.is_some() {
            self.last_interest = interest;
        }
        let want = if interest.is_some() { 1.0 } else { 0.0 };
        self.push += (want - self.push) * (1.0 - (-dt * 5.0).exp());
        let mut target = self.target_pose();
        // Tilt parallax: the eye slides, the look stays (the tower holds still, the beach moves).
        target.eye += tilt;
        self.rig.step(target, dt, c.hz, c.damping, f.shake_decay, f.shake_angle, f.shake_move);
        if self.drag.is_some() {
            self.preview = sling::preview(
                &self.world,
                self.aim.from,
                self.aim.vel,
                self.t.stone.radius,
                self.t.physics.gravity,
                TICK_RATE as f32,
                self.t.physics.substeps,
                1.4,
            );
        } else {
            self.preview = Preview::default();
        }
        // Out of stones: once the last one has done its work (everything still, or a long while), the level is lost.
        self.since_shot += dt;
        if self.stones_left == 0
            && self.won.is_none()
            && self.lost.is_none()
            && !self.loaded
            && ((self.since_shot > 1.0 && self.world.stats().awake == 0) || self.since_shot > 7.0)
        {
            self.lost = Some(self.tick);
            self.log.push(Event::OutOfStones { tick: self.tick });
            self.pulse(Haptic::Fall, 0.6, 0.3, out);
        }
        let banner = (2.8 * TICK_RATE as f32) as u64;
        if self.won.is_some_and(|w| self.tick - w > banner) {
            self.go(self.level as isize + 1);
        } else if self.lost.is_some_and(|l| self.tick - l > banner) {
            self.build();
        }
    }

    fn kind_of(&self, id: BodyId) -> Kind {
        self.world.get(id).map_or(Kind::Ground, |b| Kind::of_user(b.user))
    }

    fn impacts(&mut self, out: &mut Vec<Pulse>) {
        let f = self.t.feel.clone();
        let impacts = self.world.impacts().to_vec();
        let mut total = 0.0;
        let mut strongest: Option<(f32, Kind)> = None;
        let mut breaks: Vec<(BodyId, V3)> = Vec::new();
        for imp in &impacts {
            let (ka, kb) = (self.kind_of(imp.a), self.kind_of(imp.b));
            // The stone's first touch.
            for (sid, other) in [(imp.a, kb), (imp.b, ka)] {
                if let Some(i) = self.stones.iter().position(|s| s.id == sid && s.hit.is_none()) {
                    let at = self.world.get(sid).map_or(imp.point, |b| b.pos);
                    self.stones[i].hit = Some(at);
                    self.focus = Some(imp.point);
                    self.since_hit = 0.0;
                    self.log.push(Event::StoneHit {
                        tick: self.tick,
                        at,
                        contact: imp.point,
                        normal: imp.normal,
                        impulse: imp.impulse,
                        on: other,
                    });
                    if imp.impulse > f.hitstop_impulse {
                        self.hitstop = f.hitstop;
                        self.flash = f.flash;
                        self.log.push(Event::HitStop { tick: self.tick });
                        let n = -imp.normal * if sid == imp.a { 1.0 } else { -1.0 };
                        self.chips.sparks(&mut self.rng, imp.point, n, 10, 7.0);
                    }
                }
            }
            if imp.impulse < 1.5 {
                continue;
            }
            let hit = if ka == Kind::Ball || ka == Kind::Ground || ka == Kind::Pedestal { kb } else { ka };
            if ka.is_piece() || kb.is_piece() {
                total += imp.impulse;
                if imp.impulse > 20.0 {
                    self.since_hit = 0.0;
                }
            }
            if strongest.is_none_or(|(s, _)| imp.impulse > s) {
                strongest = Some((imp.impulse, hit));
            }
            // Chips of what was hit; dust where something lands on the sand.
            let ground = ka == Kind::Ground || kb == Kind::Ground;
            let n = ((imp.impulse * f.chips_per_impulse) as u32).min(f.chips_max);
            if n > 0 {
                let color = if ground { lin(0xe8c88a) } else { lin(self.material(hit).colour) };
                let up = if imp.normal.y.abs() > 0.5 { V3::Y } else { -imp.normal };
                self.chips.burst(&mut self.rng, imp.point, up, n, 2.0 + (imp.impulse * 0.02).min(3.0), 0.035, color, f.chip_life);
            }
            for (id, k) in [(imp.a, ka), (imp.b, kb)] {
                let m = self.material(k);
                if k == Kind::Glass && m.break_dv > 0.0 {
                    let dv = imp.impulse * self.world.get(id).map_or(0.0, |b| b.inv_mass);
                    if dv > m.break_dv && !breaks.iter().any(|b| b.0 == id) {
                        breaks.push((id, imp.point));
                    }
                }
            }
        }
        self.rig.add_trauma((total * f.shake_per_impulse).min(0.6));
        if total > f.slowmo_impulse && !self.slowmo_used {
            self.slowmo = f.slowmo_time;
            self.slowmo_used = true;
            if self.hitstop <= 0.0 {
                self.log.push(Event::SlowMo { tick: self.tick });
            }
        }
        // Haptics mark the big moments: one channel (a pulse at most every 0.1 s), and a tumble only buzzes when it
        // is at least a quarter as hard as the hardest hit of this shot.
        if let Some((imp, _)) = strongest {
            self.shot_peak = self.shot_peak.max(imp);
        }
        if let Some((imp, k)) = strongest
            && imp > 12.0_f32.max(self.shot_peak * 0.25)
            && self.pulse_cooldown <= 0.0
        {
            let sharp = match k {
                Kind::Stone => 0.25,
                Kind::Glass | Kind::Can => 0.85,
                _ => 0.5,
            };
            self.pulse(Haptic::Thud, (imp / 160.0).clamp(0.2, 1.0), sharp, out);
            self.pulse_cooldown = 0.1;
        }
        for (id, at) in breaks {
            self.shatter(id, at, out);
        }
    }

    /// A glass piece breaks: shards (real bodies, so they bounce and scatter the tower) and a spray of chips.
    fn shatter(&mut self, id: BodyId, at: V3, out: &mut Vec<Pulse>) {
        let Some(b) = self.world.get(id).copied() else { return };
        self.world.remove(id);
        if let Some(p) = self.pieces.iter_mut().find(|p| p.id == id) {
            p.cleared = true;
            self.log.push(Event::Cleared { tick: self.tick, kind: Kind::Glass });
        }
        self.pieces.retain(|p| p.id != id);
        let m = self.material(Kind::Glass);
        let (r, hh) = match b.shape {
            Shape::Prism { radius, half_height, .. } => (radius, half_height),
            _ => (0.15, 0.2),
        };
        // Six plates of the jar's wall, standing where the wall was: tangent at 0.6 of its radius, in two rings
        // (alternating), so no two overlap and none reaches outside the jar. Thick enough (5 cm) that a fast landing
        // can't sink one into the ground, and debris: they never touch the tower (EVALS step 004).
        for k in 0..6 {
            let a = k as f32 * std::f32::consts::TAU / 6.0 + self.rng.signed() * 0.1;
            let off = V3::new(a.cos() * r * 0.6, (if k % 2 == 0 { 0.5 } else { -0.5 }) * hh, -a.sin() * r * 0.6);
            let out_v = (off * (1.0 / off.length().max(1e-3))) * (1.5 + self.rng.unit() * 1.5);
            let half = V3::new(r * 0.18, hh * 0.4, r * 0.3);
            let at = b.pos + b.rot.rotate(off);
            let sid = self.world.add(BodyDef {
                rot: b.rot * Quat::axis_angle(V3::Y, a),
                vel: b.vel + out_v + V3::Y * 0.8,
                ang: V3::new(self.rng.signed(), self.rng.signed(), self.rng.signed()) * 8.0,
                material: Material { friction: m.friction, restitution: 0.15, density: m.density },
                user: Kind::Shard as u32,
                // Debris: the ground and the pedestal, never the tower (a light shard under a heavy block sinks in).
                layer: DEBRIS,
                mask: STATIC,
                ..BodyDef::new(Shape::Box { half }, at)
            });
            self.pieces.push(Piece { id: sid, kind: Kind::Shard, cleared: true, age: 0.0 });
        }
        let col = lin(m.colour);
        self.chips.burst(&mut self.rng, at, V3::Y, 16, 3.5, 0.03, col, 0.9);
        self.chips.sparks(&mut self.rng, at, V3::Y, 6, 5.0);
        self.log.push(Event::Broke { tick: self.tick, at });
        if self.pulse_cooldown <= 0.0 {
            self.pulse(Haptic::Tap, 0.8, 1.0, out);
            self.pulse_cooldown = 0.1;
        }
    }

    fn update_pieces(&mut self, gdt: f32, out: &mut Vec<Pulse>) {
        let top = self.t.pedestal.top;
        let mut gone = Vec::new();
        for p in &mut self.pieces {
            p.age += gdt;
            let Some(b) = self.world.get(p.id) else { continue };
            if !p.cleared && p.kind.is_piece() && b.pos.y < top - 0.35 {
                p.cleared = true;
                self.log.push(Event::Cleared { tick: self.tick, kind: p.kind });
            }
            if p.kind == Kind::Shard && p.age > 3.5 {
                gone.push(p.id);
            }
        }
        for id in gone {
            self.world.remove(id);
            self.pieces.retain(|p| p.id != id);
        }
        if self.won.is_none() && self.left() == 0 {
            self.won = Some(self.tick);
            self.log.push(Event::TableClear { tick: self.tick });
            self.pulse(Haptic::Rise, 0.9, 0.5, out);
        }
    }

    /// Tower pieces still on the table.
    pub fn left(&self) -> usize {
        self.pieces.iter().filter(|p| p.kind.is_piece() && !p.cleared).count()
    }

    /// Tower pieces the level started with.
    pub fn total(&self) -> usize {
        self.total
    }
}

impl Card for Smash {
    fn name(&self) -> &'static str {
        "SMASH"
    }

    fn fullscreen(&self) -> bool {
        true
    }

    fn layout(&mut self, l: &Layout) {
        self.screen = l.screen;
        self.scale = l.scale;
        self.safe = l.safe;
    }

    fn input(&mut self, g: &Gesture, _fit: &Fit, out: &mut Vec<Pulse>) {
        match *g {
            Gesture::Down(at) => self.press(at, out),
            Gesture::Move { to, .. } => self.pull(to, out),
            Gesture::Release { .. } => self.release(out),
            Gesture::Tap(_) | Gesture::Swipe { .. } => {}
        }
    }

    fn step(&mut self, sense: &Sense, out: &mut Vec<Pulse>) {
        self.sense(sense);
        self.tick(out);
    }

    fn draw(&self, alpha: f32, _fit: &Fit, frame: &mut Frame) {
        look::hud(self, alpha, frame);
    }

    fn scene(&self, alpha: f32, _layout: &Layout) -> Option<crate::draw3d::Scene3> {
        Some(look::scene(self, alpha))
    }

    fn wants_lab(&mut self) -> bool {
        std::mem::take(&mut self.want_lab)
    }

    fn observe(&self) -> String {
        let s = self.world.stats();
        format!(
            "\"awake\":{},\"bodies\":{},\"left\":{},\"shots\":{},\"step_us\":{:.0},\"pen_mm\":{:.1}",
            s.awake,
            s.bodies,
            self.left(),
            self.shots,
            self.step_us,
            s.max_penetration * 1000.0
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tilting_the_phone_moves_the_beach_not_the_tower() {
        let mut g = Smash::default();
        let mut out = Vec::new();
        let held = Sense { gravity: Some([0, -800, -600]) };
        for _ in 0..30 {
            g.step(&held, &mut out);
        }
        let (w, h) = (1179.0, 2556.0);
        let tower = V3::new(0.0, g.t.pedestal.top + 1.0, 0.0);
        let palm = V3::new(-11.0, 7.0, -20.0);
        let cam0 = g.rig.camera(1.0);
        // Tilted 15° to the right, held there for half a second.
        let tilted = Sense { gravity: Some([260, -770, -600]) };
        for _ in 0..30 {
            g.step(&tilted, &mut out);
        }
        let cam1 = g.rig.camera(1.0);
        assert!((cam1.eye - cam0.eye).length() > 0.1, "the eye slides");
        let dx = |c: &crate::draw3d::Camera, p| c.project(p, w, h).0;
        let tower_moved = (dx(&cam1, tower) - dx(&cam0, tower)).abs();
        let palm_moved = (dx(&cam1, palm) - dx(&cam0, palm)).abs();
        assert!(palm_moved > 4.0 * tower_moved, "the beach moves far more than the tower: {palm_moved} vs {tower_moved}");
        // Held tilted, the baseline catches up and the camera comes home.
        for _ in 0..600 {
            g.step(&tilted, &mut out);
        }
        assert!((g.rig.camera(1.0).eye - cam0.eye).length() < 0.02, "how it is held doesn't matter, only how it moves");
    }
}
