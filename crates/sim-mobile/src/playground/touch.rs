//! TOUCH: every finger the screen registers, with its id; two fingers hold a rope's ends between them (stretch it,
//! swing it, let one go). Shows what multi-touch gives a game beyond one finger's gestures.

use std::collections::BTreeMap;

use sim_physics::fixed::Fx;
use sim_physics::verlet::{Link, System, V2};

use super::{Card, Phase};
use crate::font;
use crate::gesture::Px;
use crate::haptics::{Kind, Pulse};
use crate::layer::{Color, Fit, Frame, Layer, Shape, fx_f32};
use crate::sensors::Sense;

const SEGMENTS: usize = 14;

#[derive(Default)]
pub struct Touch {
    /// Fingers down now, by id: where each is (world units).
    fingers: BTreeMap<u64, V2>,
    /// The most fingers down at once so far.
    most: usize,
    /// The rope between the first two fingers, and which fingers hold its ends.
    rope: Option<(System, Vec<usize>, [u64; 2])>,
}

impl Touch {
    fn make_rope(&mut self, out: &mut Vec<Pulse>) {
        let mut ids = self.fingers.keys().copied();
        let (Some(a), Some(b)) = (ids.next(), ids.next()) else { return };
        let (pa, pb) = (self.fingers[&a], self.fingers[&b]);
        let mut sys = System { gravity: V2::new(Fx::ZERO, -Fx::ratio(1, 180)), passes: 20, ..System::default() };
        let chain = sys.rope(pa, pb, SEGMENTS, Fx::ONE, Link::Max);
        // A little longer than the gap, so it hangs between the thumbs.
        for c in &mut sys.constraints {
            c.rest = c.rest * Fx::ratio(12, 10);
        }
        sys.pin(chain[0]);
        sys.pin(chain[SEGMENTS]);
        self.rope = Some((sys, chain, [a, b]));
        out.push(Pulse::new(Kind::Tap, 0.6, 0.6));
    }
}

impl Card for Touch {
    fn name(&self) -> &'static str {
        "TOUCH"
    }

    fn touch(&mut self, id: u64, phase: Phase, at: Px, fit: &Fit, out: &mut Vec<Pulse>) {
        match phase {
            Phase::Down | Phase::Move => {
                self.fingers.insert(id, fit.world(at));
                self.most = self.most.max(self.fingers.len());
                if self.rope.is_none() && self.fingers.len() >= 2 {
                    self.make_rope(out);
                }
            }
            Phase::Up => {
                self.fingers.remove(&id);
                // A released end falls free; with both gone the rope goes.
                if let Some((sys, chain, holders)) = &mut self.rope {
                    for (k, h) in holders.iter().enumerate() {
                        if *h == id {
                            sys.release(chain[if k == 0 { 0 } else { SEGMENTS }], Fx::ONE);
                        }
                    }
                    if holders.iter().all(|h| !self.fingers.contains_key(h)) {
                        self.rope = None;
                    }
                }
            }
        }
    }

    fn step(&mut self, _sense: &Sense, _out: &mut Vec<Pulse>) {
        if let Some((sys, chain, holders)) = &mut self.rope {
            for (k, h) in holders.iter().enumerate() {
                if let Some(&at) = self.fingers.get(h) {
                    sys.drag(chain[if k == 0 { 0 } else { SEGMENTS }], at);
                }
            }
            sys.step();
        }
    }

    fn draw(&self, alpha: f32, fit: &Fit, frame: &mut Frame) {
        let s = fit.scale;
        if let Some((sys, chain, _)) = &self.rope {
            let at = |i: usize| {
                let p = sys.points[i];
                let (x0, y0, x1, y1) = (fx_f32(p.prev.x), fx_f32(p.prev.y), fx_f32(p.pos.x), fx_f32(p.pos.y));
                fit.px(x0 + (x1 - x0) * alpha, y0 + (y1 - y0) * alpha)
            };
            for w in chain.windows(2) {
                frame.push(Layer::Pieces, 1, Shape::Capsule { a: at(w[0]), b: at(w[1]), r: 0.12 * s, color: Color::hex(0xff4f7b) });
            }
        }
        let px = s * 0.09;
        for (id, &p) in &self.fingers {
            let c = fit.px_of(p);
            frame.push(Layer::Pieces, 2, Shape::circle(c, 0.75 * s, Color::hexa(0x3cc8ff, 0.25)));
            frame.push(Layer::Pieces, 3, Shape::circle(c, 0.12 * s, Color::hex(0x3cc8ff)));
            // Its id, above the finger (where the thumb does not hide it).
            let label = format!("{}", id % 100);
            font::centered(frame, Layer::Fx, 2, Px::new(c.x, c.y - 1.2 * s), px, Color::hex(0xf2efff), &label);
        }
        let n = format!("{} DOWN  MOST {}", self.fingers.len(), self.most);
        let b = fit.rect;
        font::centered(frame, Layer::Fx, 2, Px::new(b.x + b.w / 2.0, b.y + b.h - 0.8 * s), px, Color::hexa(0xf2efff, 0.6), &n);
    }

    fn observe(&self) -> String {
        let tension = self.rope.as_ref().map_or(0.0, |(sys, _, _)| fx_f32(sys.tension()));
        format!("\"fingers\":{},\"most\":{},\"rope\":{},\"tension\":{tension:.3}", self.fingers.len(), self.most, self.rope.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::Rect;

    fn fit() -> Fit {
        Fit::new(9.0, 16.0, Rect::new(0.0, 0.0, 900.0, 1600.0))
    }

    #[test]
    fn two_fingers_hold_a_rope_and_lifting_both_drops_it() {
        let (f, mut t, mut out) = (fit(), Touch::default(), Vec::new());
        t.touch(1, Phase::Down, f.px(2.0, 8.0), &f, &mut out);
        assert!(t.rope.is_none());
        t.touch(2, Phase::Down, f.px(7.0, 8.0), &f, &mut out);
        assert!(t.rope.is_some(), "a second finger makes the rope");
        // Pull the thumbs apart: the rope goes taut.
        for k in 0..60 {
            t.touch(2, Phase::Move, f.px(7.0 + k as f32 * 0.05, 8.0), &f, &mut out);
            t.step(&Sense::default(), &mut out);
        }
        let (sys, _, _) = t.rope.as_ref().unwrap();
        assert!(sys.tension() > Fx::ZERO);
        t.touch(1, Phase::Up, f.px(2.0, 8.0), &f, &mut out);
        assert!(t.rope.is_some(), "one end still held");
        t.touch(2, Phase::Up, f.px(9.0, 8.0), &f, &mut out);
        assert!(t.rope.is_none());
        assert_eq!(t.most, 2);
    }
}
