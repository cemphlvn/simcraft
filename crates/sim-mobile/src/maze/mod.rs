//! MAZE: tilt the phone to roll a steel marble through a wooden labyrinth into the goal, past the holes. A tilt
//! game (`docs/research/mobile-types.md`) on the rigid-body solver (`sim_physics::rigid`), drawn in 3D.
//!
//! The board never moves in the simulation: tilting it is the same as turning gravity the other way, so the floor
//! and the walls stay fixed bodies and the solver only ever moves the marble. The camera turns with the tilt, so on
//! screen it is the board that tips. A hole is a spot on one flat floor (no seams for the marble to bump on): a
//! marble whose centre reaches it stops touching the floor and drops.
//!
//! Tilt comes from the phone's gravity sensor, relative to how the phone was held when the level started (a tap
//! re-centres it); without a sensor a dragged finger tilts the board. The evals tilt it with the autopilot
//! ([`pilot`]), through the same input.

pub mod look;
pub mod pilot;
pub mod tuning;

use sim_physics::rigid::{BodyDef, BodyId, Material, Quat, Shape, V3, World};

use crate::gesture::{Gesture, Px};
use crate::haptics::{Kind as Haptic, Pulse};
use crate::layer::{Fit, Frame, Rect};
use crate::playground::{Card, Layout, TICK_RATE};
use crate::sensors::Sense;
use tuning::{Cell, Tuning};

/// Collision layers: the floor, the walls, the marble.
pub const FLOOR: u32 = 1;
pub const WALL: u32 = 2;
pub const MARBLE: u32 = 4;

/// What a body is (its `user` tag in the solver).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Floor = 0,
    Wall = 1,
    Marble = 2,
}

/// What happened, in order (ticks since the level was built).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// The marble knocked a wall this fast (m/s).
    Knock { tick: u64, speed: f32 },
    /// It dropped into a hole (column, row).
    Fell { tick: u64, cell: (usize, usize) },
    /// It dropped into the goal.
    Won { tick: u64 },
}

/// A hole in the floor: its centre on the floor, its cell, and whether it is the goal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hole {
    pub at: V3,
    pub cell: (usize, usize),
    pub goal: bool,
}

/// A button on the HUD.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Button {
    pub rect: Rect,
    pub label: &'static str,
}

pub struct Maze {
    pub t: Tuning,
    pub level: usize,
    pub world: World,
    pub marble: Option<BodyId>,
    pub holes: Vec<Hole>,
    /// Where the marble starts.
    pub start: V3,
    /// The hole the marble is dropping into.
    pub dropping: Option<usize>,
    /// Seconds until the marble is back at the start, after a fall.
    respawn: Option<f32>,
    /// The board's tilt: how strongly it slopes towards screen right and towards screen top (sines of the angles).
    pub tilt: (f32, f32),
    /// What the hand asks for, before the board's lag.
    pub want: (f32, f32),
    /// The phone's gravity (g, screen x and y) when the level started or was re-centred.
    base: Option<(f32, f32)>,
    /// The phone has a motion sensor (it has given a reading).
    pub sensor: bool,
    /// A finger tilting the board: where it went down and where it is.
    pub drag: Option<(Px, Px)>,
    /// The autopilot's tilt, when it plays (the evals); it takes the place of the hand.
    pub pilot: Option<(f32, f32)>,
    /// Ticks played on this level (the clock; it stops when the level is won).
    pub tick: u64,
    /// Ticks since the level was built, never stopping (animations), and its value when the level was won.
    pub age: u64,
    pub won_age: u64,
    pub falls: u32,
    /// The clock when the marble dropped into the goal.
    pub won: Option<u64>,
    pub log: Vec<Event>,
    pub screen: (f32, f32),
    pub scale: f32,
    pub safe: Rect,
    want_next_card: bool,
    pub step_us: f32,
}

impl Default for Maze {
    fn default() -> Maze {
        Maze::new(Tuning::embedded())
    }
}

/// An sRGB hex colour in linear light.
pub fn lin(rgb: u32) -> [f32; 3] {
    let c = crate::layer::Color::hex(rgb).0;
    [c[0], c[1], c[2]]
}

impl Maze {
    pub fn new(t: Tuning) -> Maze {
        let mut m = Maze {
            t,
            level: 0,
            world: World::new(),
            marble: None,
            holes: Vec::new(),
            start: V3::ZERO,
            dropping: None,
            respawn: None,
            tilt: (0.0, 0.0),
            want: (0.0, 0.0),
            base: None,
            sensor: false,
            drag: None,
            pilot: None,
            tick: 0,
            age: 0,
            won_age: 0,
            falls: 0,
            won: None,
            log: Vec::new(),
            screen: (1179.0, 2556.0),
            scale: 3.0,
            safe: Rect::new(0.0, 0.0, 1179.0, 2556.0),
            want_next_card: false,
            step_us: 0.0,
        };
        m.build();
        m
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

    /// The centre of cell (`c`, `r`) on the floor: the board is centred on the origin, rows run towards +z.
    pub fn cell_at(&self, c: usize, r: usize) -> V3 {
        let (w, h) = self.level().size();
        let s = self.t.board.cell;
        V3::new((c as f32 - (w as f32 - 1.0) / 2.0) * s, 0.0, (r as f32 - (h as f32 - 1.0) / 2.0) * s)
    }

    /// The board's half width and half depth (m).
    pub fn half_extent(&self) -> (f32, f32) {
        let (w, h) = self.level().size();
        (w as f32 * self.t.board.cell / 2.0, h as f32 * self.t.board.cell / 2.0)
    }

    /// One flat floor, the walls as few boxes as cover them, the holes, and the marble at the start.
    pub fn build(&mut self) {
        let t = self.t.clone();
        let level = self.level().clone();
        let mut w = World::new();
        w.gravity = V3::new(0.0, -t.physics.gravity, 0.0);
        w.substeps = t.physics.substeps;
        // The marble is the only thing that moves: it must never fall asleep while the board is level, or the next
        // tilt would find it frozen.
        w.tuning.sleep_time = f32::INFINITY;
        w.tuning.rolling_resistance = t.physics.rolling_resistance;
        let wood = Material { friction: t.board.friction, restitution: t.board.restitution, density: 600.0 };
        let fixed =
            |shape, pos, kind: Kind, layer| BodyDef { fixed: true, material: wood, user: kind as u32, layer, ..BodyDef::new(shape, pos) };
        let (hw, hd) = self.half_extent();
        w.add(fixed(Shape::Box { half: V3::new(hw, 0.1, hd) }, V3::new(0.0, -0.1, 0.0), Kind::Floor, FLOOR));
        let s = t.board.cell;
        let hh = t.board.wall_height / 2.0;
        for (c0, r0, c1, r1) in wall_boxes(&level) {
            let a = self.cell_at(c0, r0);
            let b = self.cell_at(c1, r1);
            let half = V3::new((c1 - c0 + 1) as f32 * s / 2.0, hh, (r1 - r0 + 1) as f32 * s / 2.0);
            w.add(fixed(Shape::Box { half }, V3::new((a.x + b.x) / 2.0, hh, (a.z + b.z) / 2.0), Kind::Wall, WALL));
        }
        self.holes = level
            .find(Cell::Hole)
            .into_iter()
            .map(|cell| Hole { at: self.cell_at(cell.0, cell.1), cell, goal: false })
            .chain(level.find(Cell::Goal).into_iter().map(|cell| Hole { at: self.cell_at(cell.0, cell.1), cell, goal: true }))
            .collect();
        let (sc, sr) = level.find(Cell::Start)[0];
        self.start = self.cell_at(sc, sr);
        self.world = w;
        self.marble = None;
        self.spawn();
        self.dropping = None;
        self.respawn = None;
        self.tilt = (0.0, 0.0);
        self.want = (0.0, 0.0);
        self.base = None;
        self.drag = None;
        self.tick = 0;
        self.age = 0;
        self.won_age = 0;
        self.falls = 0;
        self.won = None;
        self.log.clear();
    }

    fn spawn(&mut self) {
        let m = &self.t.marble;
        let id = self.world.add(BodyDef {
            material: Material { friction: m.friction, restitution: m.restitution, density: m.density },
            user: Kind::Marble as u32,
            layer: MARBLE,
            mask: FLOOR | WALL,
            ..BodyDef::new(Shape::Sphere { radius: m.radius }, self.start + V3::new(0.0, m.radius + 0.002, 0.0))
        });
        self.marble = Some(id);
        self.dropping = None;
    }

    /// The board's rotation for a tilt: it slopes down towards screen right by `x` and towards screen top by `y`
    /// (sines of the angles).
    pub fn board_rot(x: f32, y: f32) -> Quat {
        // About z, a positive angle raises +x; about x, a positive angle raises −z (the screen's top).
        Quat::axis_angle(V3::X, -y.clamp(-1.0, 1.0).asin()) * Quat::axis_angle(V3::Z, -x.clamp(-1.0, 1.0).asin())
    }

    /// Gravity in the board's frame for its current tilt.
    pub fn gravity(&self) -> V3 {
        Maze::board_rot(self.tilt.0, self.tilt.1).unrotate(V3::new(0.0, -self.t.physics.gravity, 0.0))
    }

    /// The marble's centre (where it is now, or where it will start).
    pub fn marble_pos(&self) -> V3 {
        self.marble.and_then(|id| self.world.get(id)).map_or(self.start, |b| b.pos)
    }

    pub fn marble_vel(&self) -> V3 {
        self.marble.and_then(|id| self.world.get(id)).map_or(V3::ZERO, |b| b.vel)
    }

    /// What the hand asks for: the autopilot, else a dragging finger, else the phone's tilt from how it was held.
    fn sense(&mut self, sense: &Sense) {
        let max = self.t.physics.max_tilt.to_radians().sin();
        if let Some([x, y, _]) = sense.gravity {
            self.sensor = true;
            let g = (x as f32 / 1000.0, y as f32 / 1000.0);
            let base = *self.base.get_or_insert(g);
            self.want = (g.0 - base.0, g.1 - base.1);
        }
        if let Some((from, to)) = self.drag.filter(|_| !self.sensor) {
            let full = self.t.physics.drag_full * self.scale;
            // Screen y grows downwards; a finger dragged up tips the board's top down.
            self.want = ((to.x - from.x) / full * max, (from.y - to.y) / full * max);
        } else if !self.sensor {
            self.want = (0.0, 0.0);
        }
        if let Some(p) = self.pilot {
            self.want = p;
        }
        let len = (self.want.0 * self.want.0 + self.want.1 * self.want.1).sqrt();
        if len > max {
            self.want = (self.want.0 * max / len, self.want.1 * max / len);
        }
    }

    /// One tick: the board follows the hand, gravity follows the board, the marble rolls, drops, knocks.
    pub fn tick(&mut self, out: &mut Vec<Pulse>) {
        let dt = 1.0 / TICK_RATE as f32;
        let k = 1.0 - (-dt / self.t.physics.tilt_lag.max(1e-3)).exp();
        self.tilt.0 += (self.want.0 - self.tilt.0) * k;
        self.tilt.1 += (self.want.1 - self.tilt.1) * k;
        self.world.gravity = self.gravity();
        if let Some(id) = self.marble {
            self.world.wake(id);
            let p = self.marble_pos();
            if self.dropping.is_none()
                && let Some(i) = self.holes.iter().position(|h| {
                    let (dx, dz) = (p.x - h.at.x, p.z - h.at.z);
                    dx * dx + dz * dz < self.t.board.capture * self.t.board.capture
                })
            {
                // Off the floor: from here on only the walls hold it, and the hole swallows it.
                self.dropping = Some(i);
                if let Some(b) = self.world.get_mut(id) {
                    b.mask = WALL;
                }
            }
        }
        let started = std::time::Instant::now();
        self.world.step(dt);
        self.step_us = started.elapsed().as_secs_f32() * 1e6;
        self.knocks(out);
        if let Some(id) = self.marble {
            // Below the floor: in a hole, or off the board altogether.
            if self.marble_pos().y < -0.5 {
                self.world.remove(id);
                self.marble = None;
                match self.dropping.map(|i| self.holes[i]) {
                    Some(h) if h.goal => {
                        self.won = Some(self.tick);
                        self.won_age = self.age;
                        self.log.push(Event::Won { tick: self.tick });
                        out.push(Pulse::new(Haptic::Rise, 1.0, 0.6));
                        out.push(Pulse::new(Haptic::Tap, 1.0, 1.0));
                    }
                    h => {
                        self.falls += 1;
                        self.log.push(Event::Fell { tick: self.tick, cell: h.map_or((0, 0), |h| h.cell) });
                        out.push(Pulse::new(Haptic::Fall, 0.9, 0.3));
                        self.respawn = Some(self.t.feel.respawn);
                    }
                }
            }
        }
        if let Some(r) = self.respawn.as_mut() {
            *r -= dt;
            if *r <= 0.0 {
                self.respawn = None;
                self.spawn();
                out.push(Pulse::new(Haptic::Tap, 0.5, 0.8));
            }
        }
        if self.won.is_none() {
            self.tick += 1;
        }
        self.age += 1;
    }

    /// A marble knocking a wall is felt, harder for a faster knock; rolling along one is not.
    fn knocks(&mut self, out: &mut Vec<Pulse>) {
        let Some(id) = self.marble else { return };
        let f = &self.t.feel;
        let mut hardest: f32 = 0.0;
        for i in self.world.impacts() {
            let other = if i.a == id {
                i.b
            } else if i.b == id {
                i.a
            } else {
                continue;
            };
            let wall = self.world.get(other).is_some_and(|b| b.user == Kind::Wall as u32);
            if wall && i.new && i.speed >= f.knock_min {
                hardest = hardest.max(i.speed);
            }
        }
        if hardest > 0.0 {
            self.log.push(Event::Knock { tick: self.tick, speed: hardest });
            let k = ((hardest - f.knock_min) / (f.knock_full - f.knock_min)).clamp(0.0, 1.0);
            out.push(Pulse::new(Haptic::Thud, 0.25 + 0.75 * k, 0.4 + 0.5 * k));
        }
    }

    /// Seconds played on this level.
    pub fn seconds(&self) -> f32 {
        self.tick as f32 / TICK_RATE as f32
    }

    /// Stars for a cleared level: three without a fall, two for one or two, one after that.
    pub fn stars(&self) -> u32 {
        match self.falls {
            0 => 3,
            1 | 2 => 2,
            _ => 1,
        }
    }

    fn press(&mut self, at: Px, out: &mut Vec<Pulse>) -> bool {
        let inside = |r: Rect| at.x >= r.x && at.x <= r.x + r.w && at.y >= r.y && at.y <= r.y + r.h;
        let Some(b) = look::buttons(self).into_iter().find(|b| inside(b.rect)) else { return false };
        out.push(Pulse::new(Haptic::Tap, 0.6, 0.9));
        match b.label {
            "RESET" => self.build(),
            "-" => self.go(self.level as isize - 1),
            "+" => self.go(self.level as isize + 1),
            _ => self.want_next_card = true,
        }
        true
    }
}

/// The walls as rectangles of cells (first column, first row, last column, last row), greedily: as wide as a run
/// goes, then as deep as the same run repeats. Fewer, larger boxes give the marble fewer edges to catch on.
pub fn wall_boxes(level: &tuning::Level) -> Vec<(usize, usize, usize, usize)> {
    let (w, h) = level.size();
    let mut used = vec![vec![false; w]; h];
    let mut out = Vec::new();
    for r in 0..h {
        for c in 0..w {
            if used[r][c] || level.at(c, r) != Cell::Wall {
                continue;
            }
            let mut c1 = c;
            while c1 + 1 < w && !used[r][c1 + 1] && level.at(c1 + 1, r) == Cell::Wall {
                c1 += 1;
            }
            let mut r1 = r;
            while r1 + 1 < h && (c..=c1).all(|x| !used[r1 + 1][x] && level.at(x, r1 + 1) == Cell::Wall) {
                r1 += 1;
            }
            for row in used.iter_mut().take(r1 + 1).skip(r) {
                for u in row.iter_mut().take(c1 + 1).skip(c) {
                    *u = true;
                }
            }
            out.push((c, r, c1, r1));
        }
    }
    out
}

impl Card for Maze {
    fn name(&self) -> &'static str {
        "MAZE"
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
            Gesture::Down(at) => {
                if !self.press(at, out) {
                    self.drag = Some((at, at));
                }
            }
            Gesture::Move { to, .. } => {
                if let Some(d) = self.drag.as_mut() {
                    d.1 = to;
                }
            }
            Gesture::Tap(_) => {
                if self.won.is_some() {
                    self.go(self.level as isize + 1);
                } else if self.sensor {
                    // However the phone is held now is level.
                    self.base = None;
                    out.push(Pulse::new(Haptic::Tick, 0.5, 0.7));
                }
            }
            Gesture::Release { .. } => self.drag = None,
            Gesture::Swipe { .. } => {}
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
        std::mem::take(&mut self.want_next_card)
    }

    fn observe(&self) -> String {
        let s = self.world.stats();
        format!(
            "\"level\":{},\"falls\":{},\"tilt\":[{:.3},{:.3}],\"step_us\":{:.0},\"pen_mm\":{:.1}",
            self.level,
            self.falls,
            self.tilt.0,
            self.tilt.1,
            self.step_us,
            s.max_penetration * 1000.0
        )
    }
}

#[cfg(test)]
mod tests;
