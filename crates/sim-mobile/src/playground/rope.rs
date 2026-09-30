//! ROPE: a rope between two pegs with a bead on it. Catch the rope by swiping across it (or touch it), pull, let
//! go: the stretch becomes speed. Tension ticks in the hand as it rises.
//!
//! The simulation runs in [`Fx`] at a fixed tick; the finger enters it as integers (`Fit::world`), and drawing
//! interpolates between ticks with the points' previous positions, so the frame rate never changes a result.

use sim_physics::fixed::Fx;
use sim_physics::verlet::{Link, System, V2};

use super::Card;
use crate::gesture::{Gesture, Px};
use crate::haptics::{Kind, Pulse};
use crate::layer::{Color, Fit, Frame, Layer, Rect, Shape, fx_f32};
use crate::sensors::Sense;

const SEGMENTS: usize = 16;
/// A finger this close (world units) to a rope point grabs it without a swipe.
const REACH: Fx = Fx::ratio(3, 5);

pub struct Rope {
    pub sys: System,
    rope: Vec<usize>,
    bead: usize,
    /// The point the finger holds, and its inverse weight before it was caught.
    held: Option<(usize, Fx)>,
    /// Where the finger is (world units), applied at the next tick.
    finger: Option<V2>,
    /// Tension in tenths, as last reported (a tick of haptics each time it rises a step).
    tension_step: i64,
}

impl Default for Rope {
    fn default() -> Rope {
        Rope::new()
    }
}

impl Rope {
    pub fn new() -> Rope {
        let mut sys = System { gravity: V2::new(Fx::ZERO, -Fx::ratio(1, 180)), passes: 24, ..System::default() };
        let rope = sys.rope(V2::int(1, 11), V2::int(8, 11), SEGMENTS, Fx::ONE, Link::Max);
        sys.pin(rope[0]);
        sys.pin(rope[SEGMENTS]);
        // Longer than the gap between the pegs, so it sags.
        for c in &mut sys.constraints {
            c.rest = c.rest * Fx::ratio(13, 10);
        }
        // The bead in the middle weighs four times a rope point.
        let bead = rope[SEGMENTS / 2];
        sys.points[bead].inv = Fx::ratio(1, 4);
        Rope { sys, rope, bead, held: None, finger: None, tension_step: 0 }
    }

    fn catch(&mut self, i: usize, at: V2, out: &mut Vec<Pulse>) {
        let inv = self.sys.points[i].inv;
        self.sys.pin(i);
        self.held = Some((i, inv));
        self.finger = Some(at);
        out.push(Pulse::new(Kind::Tap, 0.7, 0.8));
    }

    fn nearest(&self, w: V2) -> Option<usize> {
        self.rope.iter().copied().filter(|&i| self.sys.points[i].inv > Fx::ZERO).min_by_key(|&i| (self.sys.points[i].pos - w).length())
    }

}

impl Card for Rope {
    fn name(&self) -> &'static str {
        "ROPE"
    }

    /// A gesture, with the board's place on the screen. Pulses to play go to `out`.
    fn input(&mut self, g: &Gesture, fit: &Fit, out: &mut Vec<Pulse>) {
        match *g {
            Gesture::Down(at) => {
                let w = fit.world(at);
                if let Some(i) = self.nearest(w).filter(|&i| (self.sys.points[i].pos - w).length() < REACH) {
                    self.catch(i, w, out);
                }
            }
            Gesture::Move { from, to } => {
                let w = fit.world(to);
                if self.held.is_some() {
                    self.finger = Some(w);
                } else if let Some((c, _)) = self.sys.crossed(fit.world(from), w) {
                    // Swiped across a link: catch whichever of its ends is free and nearer the finger.
                    let k = self.sys.constraints[c];
                    let free: Vec<usize> = [k.a, k.b].into_iter().filter(|&i| self.sys.points[i].inv > Fx::ZERO).collect();
                    if let Some(&i) = free.iter().min_by_key(|&&i| (self.sys.points[i].pos - w).length()) {
                        self.catch(i, w, out);
                    }
                }
            }
            Gesture::Release { .. } => {
                if let Some((i, inv)) = self.held.take() {
                    self.sys.release(i, inv);
                    self.finger = None;
                    let t = fx_f32(self.sys.tension());
                    out.push(Pulse::new(Kind::Thud, 0.3 + t * 2.0, 0.4));
                }
            }
            Gesture::Tap(_) | Gesture::Swipe { .. } => {}
        }
    }

    /// One tick. A tick of haptics each time the rope's tension rises a tenth.
    fn step(&mut self, _sense: &Sense, out: &mut Vec<Pulse>) {
        if let (Some((i, _)), Some(at)) = (self.held, self.finger) {
            self.sys.drag(i, at);
        }
        self.sys.step();
        let step = (self.sys.tension() * 10).floor();
        if step > self.tension_step {
            out.push(Pulse::new(Kind::Tick, 0.2 + step as f32 * 0.1, 0.9));
        }
        self.tension_step = step;
    }

    /// What the scene adds to the stats line (JSON fields): how taut the rope is and whether a finger holds it.
    fn observe(&self) -> String {
        format!("\"tension\":{:.3},\"held\":{}", fx_f32(self.sys.tension()), self.held.is_some())
    }

    /// The scene drawn `alpha` of the way from the last tick to this one.
    fn draw(&self, alpha: f32, fit: &Fit, frame: &mut Frame) {
        let at = |i: usize| {
            let p = self.sys.points[i];
            let (x0, y0) = (fx_f32(p.prev.x), fx_f32(p.prev.y));
            let (x1, y1) = (fx_f32(p.pos.x), fx_f32(p.pos.y));
            fit.px(x0 + (x1 - x0) * alpha, y0 + (y1 - y0) * alpha)
        };
        let s = fit.scale;
        // The rope: a shadow, the cord, a highlight.
        for w in self.rope.windows(2) {
            let (a, b) = (at(w[0]), at(w[1]));
            let off = Px::new(0.06 * s, 0.1 * s);
            let (sa, sb) = (Px::new(a.x + off.x, a.y + off.y), Px::new(b.x + off.x, b.y + off.y));
            frame.push(Layer::Pieces, 0, Shape::Capsule { a: sa, b: sb, r: 0.13 * s, color: Color::hexa(0x0b0820, 0.45) });
            frame.push(Layer::Pieces, 1, Shape::Capsule { a, b, r: 0.13 * s, color: Color::hex(0xff4f7b) });
            let hi = Px::new(-0.03 * s, -0.04 * s);
            frame.push(
                Layer::Pieces,
                2,
                Shape::Capsule {
                    a: Px::new(a.x + hi.x, a.y + hi.y),
                    b: Px::new(b.x + hi.x, b.y + hi.y),
                    r: 0.04 * s,
                    color: Color::hexa(0xffc2d2, 0.8),
                },
            );
        }
        for &peg in [self.rope[0], self.rope[SEGMENTS]].iter() {
            frame.push(Layer::Pieces, 3, Shape::circle(at(peg), 0.42 * s, Color::hex(0xffc93c)));
            frame.push(Layer::Pieces, 4, Shape::circle(at(peg), 0.18 * s, Color::hex(0xfff1b8)));
        }
        let bead = at(self.bead);
        frame.push(Layer::Pieces, 3, Shape::circle(bead, 0.5 * s, Color::hex(0x3cc8ff)));
        frame.push(Layer::Pieces, 4, Shape::circle(Px::new(bead.x - 0.14 * s, bead.y - 0.16 * s), 0.16 * s, Color::hexa(0xffffff, 0.7)));
        if let Some((i, _)) = self.held {
            frame.push(Layer::Fx, 0, Shape::circle(at(i), 0.7 * s, Color::hexa(0xffffff, 0.18)));
        }
        // A tension meter along the top of the board.
        let unit = s * 0.35;
        let b = fit.rect;
        let bar = Rect::new(b.x + b.w * 0.2, b.y + unit, b.w * 0.6, unit);
        frame.push(Layer::Hud, 0, Shape::Box { rect: bar, r: unit / 2.0, color: Color::hexa(0x000000, 0.35) });
        let t = (fx_f32(self.sys.tension()) * 4.0).clamp(0.0, 1.0);
        // Gravity alone leaves a hair of stretch: the meter shows pulling, not hanging.
        if t > 0.05 {
            let fill = Rect::new(bar.x, bar.y, (bar.w * t).max(bar.h), bar.h);
            frame.push(Layer::Hud, 1, Shape::Box { rect: fill, r: unit / 2.0, color: Color::hex(0xffc93c) });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gesture::Px;
    use crate::layer::Rect;
    use crate::playground::BOARD;

    fn fit() -> Fit {
        Fit::new(BOARD.0, BOARD.1, Rect::new(0.0, 0.0, 900.0, 1600.0))
    }

    fn settle(s: &mut Rope) {
        let mut out = Vec::new();
        for _ in 0..600 {
            s.step(&Sense::default(), &mut out);
        }
    }

    #[test]
    fn the_rope_sags_between_its_pegs() {
        let mut s = Rope::new();
        settle(&mut s);
        let bead = s.sys.points[s.bead].pos;
        assert!(bead.y < Fx::int(9), "the bead hangs below the pegs: {bead:?}");
        assert_eq!(s.sys.points[s.rope[0]].pos, V2::int(1, 11));
    }

    #[test]
    fn a_swipe_across_the_rope_catches_it_and_a_release_flings_it() {
        let mut s = Rope::new();
        settle(&mut s);
        let f = fit();
        let bead = f.px_of(s.sys.points[s.bead].pos);
        let mut out = Vec::new();
        // A stroke from well below the bead up through it: the rope is caught.
        s.input(&Gesture::Move { from: Px::new(bead.x + 20.0, bead.y + 150.0), to: Px::new(bead.x + 20.0, bead.y - 40.0) }, &f, &mut out);
        assert!(s.held.is_some(), "the swipe across did not catch the rope");
        assert!(matches!(out[..], [Pulse { kind: Kind::Tap, .. }]), "{out:?}");
        // Pull it down hard, then let go.
        for k in 1..=30 {
            s.input(&Gesture::Move { from: bead, to: Px::new(bead.x, bead.y + k as f32 * 20.0) }, &f, &mut out);
            s.step(&Sense::default(), &mut out);
        }
        assert!(out.iter().any(|p| p.kind == Kind::Tick), "pulling a rope taut ticks");
        s.input(&Gesture::Release { at: bead, velocity: Px::default() }, &f, &mut out);
        let before = s.sys.points[s.bead].pos.y;
        for _ in 0..6 {
            s.step(&Sense::default(), &mut out);
        }
        assert!(s.sys.points[s.bead].pos.y > before + Fx::int(1), "let go, the rope springs back up");
        assert!(out.iter().any(|p| p.kind == Kind::Thud));
    }

    #[test]
    fn a_stroke_that_misses_catches_nothing() {
        let mut s = Rope::new();
        settle(&mut s);
        let mut out = Vec::new();
        s.input(&Gesture::Move { from: Px::new(50.0, 1500.0), to: Px::new(100.0, 1500.0) }, &fit(), &mut out);
        assert!(s.held.is_none() && out.is_empty());
    }

    #[test]
    fn the_scene_is_deterministic() {
        let run = || {
            let mut s = Rope::new();
            let f = fit();
            let mut out = Vec::new();
            s.input(&Gesture::Down(f.px(4.5, 9.0)), &f, &mut out);
            for k in 0..200 {
                s.input(&Gesture::Move { from: f.px(4.5, 9.0), to: f.px(4.5 - k as f32 * 0.01, 9.0 - k as f32 * 0.02) }, &f, &mut out);
                s.step(&Sense::default(), &mut out);
            }
            s.sys
        };
        assert_eq!(run(), run());
    }
}
