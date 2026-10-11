//! GUMBALL: a merge-drop game (Suika's family, `docs/research/mobile-types.md`). Drag to aim, let go to drop a
//! gumball into a glass jar; two of the same size that touch merge into the next size up, and two of the largest
//! vanish for a bonus. The game ends when the pile stays over the rim.
//!
//! It runs on the rigid-body solver (`sim_physics::rigid`) with every ball held in one plane (z = 0) after each
//! step, so it plays as a flat game and is drawn in 3D. Merging is the game's rule, not the solver's: after a step,
//! two balls of one size closer than their radii merge, each ball at most once a tick, nearest pairs first.

pub mod look;
pub mod pilot;
pub mod tuning;

use sim_physics::rigid::{BodyDef, BodyId, Material, Shape, V3, World};

use crate::gesture::{Gesture, Px};
use crate::haptics::{Kind as Haptic, Pulse};
use crate::layer::{Fit, Frame, Rect};
use crate::playground::{Card, Layout, TICK_RATE};
use crate::sensors::Sense;
use crate::smash::fx::Rng;
use tuning::Tuning;

/// What a body is (its `user` tag in the solver): the jar, or a ball of size `BALL + size`.
pub const JAR: u32 = 0;
pub const BALL: u32 = 100;

/// What happened, in order (ticks since the game started).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    Drop {
        tick: u64,
        x: f32,
        size: usize,
    },
    /// Two balls made one of `size`.
    Merge {
        tick: u64,
        size: usize,
        at: V3,
    },
    /// Two of the largest vanished.
    Vanish {
        tick: u64,
    },
    /// The pile stayed over the rim.
    Over {
        tick: u64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ball {
    pub id: BodyId,
    pub size: usize,
    /// Seconds it has spent over the rim without moving fast (a ball just dropped is falling, not over).
    pub over: f32,
    /// Seconds since it came into the jar.
    pub age: f32,
}

/// A button on the HUD.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Button {
    pub rect: Rect,
    pub label: &'static str,
}

pub struct Gumball {
    pub t: Tuning,
    pub world: World,
    pub balls: Vec<Ball>,
    rng: Rng,
    /// The ball in the hand, and the one after it.
    pub next: usize,
    pub after: usize,
    /// Where the hand is over the jar (m), and whether a finger is aiming.
    pub hand_x: f32,
    pub aiming: bool,
    /// Seconds until the next ball is in the hand.
    pub reload: f32,
    /// The autopilot's drop, when it plays (the evals): it takes the place of a finger.
    pub pilot: Option<f32>,
    pub score: u32,
    pub best: u32,
    pub merges: u32,
    pub drops: u32,
    /// The largest size made so far.
    pub biggest: usize,
    pub lost: Option<u64>,
    pub tick: u64,
    pub log: Vec<Event>,
    pub screen: (f32, f32),
    pub scale: f32,
    pub safe: Rect,
    want_next_card: bool,
    pub step_us: f32,
}

impl Default for Gumball {
    fn default() -> Gumball {
        Gumball::new(Tuning::embedded())
    }
}

/// An sRGB hex colour in linear light.
pub fn lin(rgb: u32) -> [f32; 3] {
    let c = crate::layer::Color::hex(rgb).0;
    [c[0], c[1], c[2]]
}

impl Gumball {
    pub fn new(t: Tuning) -> Gumball {
        let mut g = Gumball {
            rng: Rng::new(t.feel.seed),
            t,
            world: World::new(),
            balls: Vec::new(),
            next: 0,
            after: 0,
            hand_x: 0.0,
            aiming: false,
            reload: 0.0,
            pilot: None,
            score: 0,
            best: 0,
            merges: 0,
            drops: 0,
            biggest: 0,
            lost: None,
            tick: 0,
            log: Vec::new(),
            screen: (1179.0, 2556.0),
            scale: 3.0,
            safe: Rect::new(0.0, 0.0, 1179.0, 2556.0),
            want_next_card: false,
            step_us: 0.0,
        };
        g.build();
        g
    }

    /// An empty jar, a fresh score, the dice from the start.
    pub fn build(&mut self) {
        let t = self.t.clone();
        let mut w = World::new();
        w.gravity = V3::new(0.0, -t.physics.gravity, 0.0);
        w.substeps = t.physics.substeps;
        let glass = Material { friction: t.jar.friction, restitution: t.jar.restitution, density: 1000.0 };
        let fixed = |half, pos| BodyDef { fixed: true, material: glass, user: JAR, ..BodyDef::new(Shape::Box { half }, pos) };
        let (hw, wall) = (t.jar.width / 2.0, t.jar.wall);
        // Deep enough in z that no ball can slip past an edge; the balls are held in z = 0 anyway.
        let depth = 2.0;
        w.add(fixed(V3::new(hw + 2.0 * wall, 0.25, depth), V3::new(0.0, -0.25, 0.0)));
        // The walls rise well over the rim, so a ball dropped at the edge never falls outside.
        let tall = t.jar.height + 2.0;
        for side in [-1.0, 1.0] {
            w.add(fixed(V3::new(wall / 2.0, tall / 2.0, depth), V3::new(side * (hw + wall / 2.0), tall / 2.0, 0.0)));
        }
        self.world = w;
        self.balls.clear();
        self.rng = Rng::new(t.feel.seed);
        self.next = self.roll();
        self.after = self.roll();
        self.hand_x = 0.0;
        self.aiming = false;
        self.reload = 0.0;
        self.score = 0;
        self.merges = 0;
        self.drops = 0;
        self.biggest = 0;
        self.lost = None;
        self.tick = 0;
        self.log.clear();
    }

    fn roll(&mut self) -> usize {
        let n = self.t.balls.drop_sizes;
        ((self.rng.unit() * n as f32) as usize).min(n - 1)
    }

    pub fn radius(&self, size: usize) -> f32 {
        self.t.radius(size)
    }

    /// The height a ball is dropped from (its centre).
    pub fn drop_y(&self) -> f32 {
        self.t.jar.rim + self.t.feel.drop_above + self.radius(self.t.balls.drop_sizes - 1)
    }

    /// The hand's x, kept where the ball in it fits inside the glass.
    pub fn clamp_x(&self, x: f32) -> f32 {
        let room = (self.t.jar.width / 2.0 - self.radius(self.next) - 0.005).max(0.0);
        x.clamp(-room, room)
    }

    /// Puts a ball of `size` in the jar at (`x`, `y`): a new drop, a merge, or a test.
    pub fn add_ball(&mut self, size: usize, x: f32, y: f32, vel: V3) -> BodyId {
        let b = &self.t.balls;
        let density = b.density * (b.smallest / self.radius(size)).powf(b.density_falloff);
        let id = self.world.add(BodyDef {
            material: Material { friction: b.friction, restitution: b.restitution, density },
            user: BALL + size as u32,
            vel,
            ..BodyDef::new(Shape::Sphere { radius: self.radius(size) }, V3::new(x, y, 0.0))
        });
        self.balls.push(Ball { id, size, over: 0.0, age: 0.0 });
        self.biggest = self.biggest.max(size);
        id
    }

    /// Lets go of the ball in the hand, if one is there.
    pub fn drop_ball(&mut self, out: &mut Vec<Pulse>) -> bool {
        if self.lost.is_some() || self.reload > 0.0 {
            return false;
        }
        let x = self.clamp_x(self.hand_x);
        let size = self.next;
        self.add_ball(size, x, self.drop_y(), V3::ZERO);
        self.log.push(Event::Drop { tick: self.tick, x, size });
        self.drops += 1;
        self.next = self.after;
        self.after = self.roll();
        self.reload = self.t.feel.reload;
        self.wake_all();
        out.push(Pulse::new(Haptic::Tick, 0.45, 0.8));
        true
    }

    fn wake_all(&mut self) {
        for b in &self.balls {
            self.world.wake(b.id);
        }
    }

    pub fn pos(&self, b: &Ball) -> V3 {
        self.world.get(b.id).map_or(V3::ZERO, |body| body.pos)
    }

    /// One tick: the hand reloads, the jar settles, equal balls merge, the rim is watched.
    pub fn tick(&mut self, out: &mut Vec<Pulse>) {
        let dt = 1.0 / TICK_RATE as f32;
        self.reload = (self.reload - dt).max(0.0);
        if let Some(x) = self.pilot
            && self.reload <= 0.0
            && self.lost.is_none()
        {
            self.hand_x = x;
            self.drop_ball(out);
        }
        let started = web_time::Instant::now();
        self.world.step(dt);
        self.step_us = started.elapsed().as_secs_f32() * 1e6;
        self.flatten();
        self.merge(out);
        self.watch_rim(out);
        self.tick += 1;
    }

    /// Holds every ball in the plane z = 0, turning only about z: a flat game in a 3D solver.
    fn flatten(&mut self) {
        for b in &self.balls {
            if let Some(body) = self.world.get_mut(b.id) {
                body.pos.z = 0.0;
                body.vel.z = 0.0;
                body.ang.x = 0.0;
                body.ang.y = 0.0;
            }
        }
    }

    /// Equal balls that touch become one of the next size, at their midpoint, popping up a little; two of the
    /// largest vanish. Nearest pairs first, each ball once a tick.
    fn merge(&mut self, out: &mut Vec<Pulse>) {
        let mut pairs = Vec::new();
        for i in 0..self.balls.len() {
            for j in i + 1..self.balls.len() {
                let (a, b) = (self.balls[i], self.balls[j]);
                if a.size != b.size {
                    continue;
                }
                let d = (self.pos(&a) - self.pos(&b)).length();
                let touch = 2.0 * self.radius(a.size) + 0.004;
                if d < touch {
                    pairs.push((d, i, j));
                }
            }
        }
        if pairs.is_empty() {
            return;
        }
        pairs.sort_by(|p, q| p.0.total_cmp(&q.0));
        let mut used = vec![false; self.balls.len()];
        let mut made = Vec::new();
        for (_, i, j) in pairs {
            if used[i] || used[j] {
                continue;
            }
            used[i] = true;
            used[j] = true;
            let (a, b) = (self.balls[i], self.balls[j]);
            let (pa, pb) = (self.pos(&a), self.pos(&b));
            let va = self.world.get(a.id).map_or(V3::ZERO, |x| x.vel);
            let vb = self.world.get(b.id).map_or(V3::ZERO, |x| x.vel);
            made.push((a.size, (pa + pb) * 0.5, (va + vb) * 0.5));
        }
        let mut keep = Vec::with_capacity(self.balls.len());
        for (k, b) in self.balls.drain(..).enumerate() {
            if used[k] {
                self.world.remove(b.id);
            } else {
                keep.push(b);
            }
        }
        self.balls = keep;
        let largest = self.t.balls.sizes - 1;
        for (size, at, vel) in made {
            self.merges += 1;
            if size == largest {
                self.score += self.t.balls.bonus;
                self.log.push(Event::Vanish { tick: self.tick });
                out.push(Pulse::new(Haptic::Rise, 1.0, 0.7));
                continue;
            }
            let new = size + 1;
            // A bigger ball where two touched: kept inside the glass and above the floor, so the solver only has to
            // ease its neighbours aside.
            let r = self.radius(new);
            let room = self.t.jar.width / 2.0 - r - 0.002;
            let (x, y) = (at.x.clamp(-room, room), at.y.max(r + 0.002));
            self.add_ball(new, x, y, vel + V3::new(0.0, self.t.feel.merge_pop, 0.0));
            self.score += self.t.balls.points[new];
            self.log.push(Event::Merge { tick: self.tick, size: new, at });
            out.push(Pulse::new(Haptic::Thud, (0.3 + 0.07 * new as f32).min(1.0), 0.5));
        }
        self.best = self.best.max(self.score);
        self.wake_all();
    }

    /// A ball that rests over the rim long enough ends the game; a falling one is not resting.
    fn watch_rim(&mut self, out: &mut Vec<Pulse>) {
        let dt = 1.0 / TICK_RATE as f32;
        let rim = self.t.jar.rim;
        let mut over = false;
        for k in 0..self.balls.len() {
            let b = self.balls[k];
            let (p, v) = self.world.get(b.id).map_or((V3::ZERO, V3::ZERO), |x| (x.pos, x.vel));
            let ball = &mut self.balls[k];
            ball.age += dt;
            if p.y + self.t.radius(b.size) > rim && v.length() < 0.5 && ball.age > 0.6 {
                ball.over += dt;
            } else {
                ball.over = 0.0;
            }
            over |= ball.over > self.t.jar.over_time;
        }
        if over && self.lost.is_none() {
            self.lost = Some(self.tick);
            self.log.push(Event::Over { tick: self.tick });
            self.best = self.best.max(self.score);
            out.push(Pulse::new(Haptic::Buzz { ms: 300 }, 0.8, 0.4));
        }
    }

    /// The jar's x under screen x `px` (pixels), on the dropping line.
    pub fn x_at(&self, px: f32) -> f32 {
        let cam = look::camera(self);
        let y = self.drop_y();
        let hw = self.t.jar.width / 2.0;
        let (l, _, _) = cam.project(V3::new(-hw, y, 0.0), self.screen.0, self.screen.1);
        let (r, _, _) = cam.project(V3::new(hw, y, 0.0), self.screen.0, self.screen.1);
        if (r - l).abs() < 1.0 { 0.0 } else { -hw + (px - l) / (r - l) * 2.0 * hw }
    }

    fn press(&mut self, at: Px, out: &mut Vec<Pulse>) -> bool {
        let inside = |r: Rect| at.x >= r.x && at.x <= r.x + r.w && at.y >= r.y && at.y <= r.y + r.h;
        let Some(b) = look::buttons(self).into_iter().find(|b| inside(b.rect)) else { return false };
        out.push(Pulse::new(Haptic::Tap, 0.6, 0.9));
        match b.label {
            "RESET" => self.build(),
            _ => self.want_next_card = true,
        }
        true
    }
}

impl Card for Gumball {
    fn name(&self) -> &'static str {
        "GUMBALL"
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
                if !self.press(at, out) && self.lost.is_none() {
                    self.aiming = true;
                    self.hand_x = self.clamp_x(self.x_at(at.x));
                }
            }
            Gesture::Move { to, .. } => {
                if self.aiming {
                    self.hand_x = self.clamp_x(self.x_at(to.x));
                }
            }
            Gesture::Release { .. } => {
                if self.lost.is_some() {
                    // After a short look at the score, a tap starts again.
                    if self.lost.is_some_and(|t| self.tick > t + 45) {
                        self.build();
                    }
                } else if self.aiming {
                    self.drop_ball(out);
                }
                self.aiming = false;
            }
            Gesture::Tap(_) | Gesture::Swipe { .. } => {}
        }
    }

    fn step(&mut self, _sense: &Sense, out: &mut Vec<Pulse>) {
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
            "\"score\":{},\"balls\":{},\"biggest\":{},\"step_us\":{:.0},\"pen_mm\":{:.1}",
            self.score,
            self.balls.len(),
            self.biggest,
            self.step_us,
            s.max_penetration * 1000.0
        )
    }
}

#[cfg(test)]
mod tests;
