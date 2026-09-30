//! TILT: a chain hanging from a peg under the phone's real gravity. Tilt the phone and it swings the way a chain
//! in your hand would; lay it flat and it floats. Gravity enters the simulation in thousandths of g (whole
//! numbers), so a replay carries the hand's motion. Without a motion sensor (the desktop) it hangs straight down.

use sim_physics::fixed::Fx;
use sim_physics::verlet::{Link, System, V2};

use super::Card;
use crate::font;
use crate::gesture::{Gesture, Px};
use crate::haptics::Pulse;
use crate::layer::{Color, Fit, Frame, Layer, Shape, fx_f32};
use crate::sensors::Sense;

const SEGMENTS: usize = 12;
/// Units per tick² at 1 g: the same pull as the other cards' ropes.
const G: i64 = 180;

pub struct Tilt {
    sys: System,
    chain: Vec<usize>,
    /// The last gravity read (thousandths of g), or none without a sensor.
    gravity: Option<[i32; 3]>,
}

impl Tilt {
    pub fn new() -> Tilt {
        let mut sys = System { gravity: V2::new(Fx::ZERO, -Fx::ratio(1, G)), passes: 20, ..System::default() };
        let chain = sys.rope(
            V2::int(4, 12) + V2::new(Fx::HALF, Fx::ZERO),
            V2::int(4, 5) + V2::new(Fx::HALF, Fx::ZERO),
            SEGMENTS,
            Fx::ONE,
            Link::Max,
        );
        sys.pin(chain[0]);
        // A heavy weight at the end.
        sys.points[chain[SEGMENTS]].inv = Fx::ratio(1, 5);
        Tilt { sys, chain, gravity: None }
    }
}

impl Card for Tilt {
    fn name(&self) -> &'static str {
        "TILT"
    }

    fn input(&mut self, g: &Gesture, _fit: &Fit, _out: &mut Vec<Pulse>) {
        // A tap gives the weight a kick, to see how it settles.
        if let Gesture::Tap(_) = g {
            let end = self.chain[SEGMENTS];
            let p = &mut self.sys.points[end];
            p.prev = p.pos - V2::new(Fx::ratio(3, 10), Fx::ZERO);
        }
    }

    fn step(&mut self, sense: &Sense, _out: &mut Vec<Pulse>) {
        self.gravity = sense.gravity;
        if let Some([x, y, _]) = sense.gravity {
            // The phone's x is the screen's right, its y the screen's top: the world's axes.
            self.sys.gravity = V2::new(Fx::int(i64::from(x)) / (1000 * G), Fx::int(i64::from(y)) / (1000 * G));
        }
        self.sys.step();
    }

    fn draw(&self, alpha: f32, fit: &Fit, frame: &mut Frame) {
        let s = fit.scale;
        let at = |i: usize| {
            let p = self.sys.points[i];
            let (x0, y0, x1, y1) = (fx_f32(p.prev.x), fx_f32(p.prev.y), fx_f32(p.pos.x), fx_f32(p.pos.y));
            fit.px(x0 + (x1 - x0) * alpha, y0 + (y1 - y0) * alpha)
        };
        for w in self.chain.windows(2) {
            frame.push(Layer::Pieces, 1, Shape::Capsule { a: at(w[0]), b: at(w[1]), r: 0.1 * s, color: Color::hex(0xc9c2ff) });
        }
        for &i in &self.chain[1..SEGMENTS] {
            frame.push(Layer::Pieces, 2, Shape::circle(at(i), 0.13 * s, Color::hex(0x8f7cff)));
        }
        frame.push(Layer::Pieces, 3, Shape::circle(at(self.chain[0]), 0.4 * s, Color::hex(0xffc93c)));
        frame.push(Layer::Pieces, 3, Shape::circle(at(self.chain[SEGMENTS]), 0.6 * s, Color::hex(0x3cc8ff)));
        // Gravity as an arrow from the board's centre, 2 units long at 1 g.
        let c = fit.px(4.5, 8.0);
        let px = s * 0.1;
        match self.gravity {
            Some([x, y, z]) => {
                let tip = fit.px(4.5 + x as f32 / 500.0, 8.0 + y as f32 / 500.0);
                frame.push(Layer::Fx, 1, Shape::Capsule { a: c, b: tip, r: 0.05 * s, color: Color::hexa(0xffc93c, 0.8) });
                frame.push(Layer::Fx, 1, Shape::circle(tip, 0.12 * s, Color::hexa(0xffc93c, 0.8)));
                let text = format!("G {x:+} {y:+} {z:+}");
                let b = fit.rect;
                font::centered(frame, Layer::Fx, 2, Px::new(b.x + b.w / 2.0, b.y + b.h - 0.8 * s), px, Color::hexa(0xf2efff, 0.6), &text);
            }
            None => {
                let b = fit.rect;
                font::centered(
                    frame,
                    Layer::Fx,
                    2,
                    Px::new(b.x + b.w / 2.0, b.y + b.h - 0.8 * s),
                    px,
                    Color::hexa(0xf2efff, 0.6),
                    "NO MOTION SENSOR",
                );
            }
        }
    }

    fn observe(&self) -> String {
        match self.gravity {
            Some([x, y, z]) => format!("\"gravity\":[{x},{y},{z}]"),
            None => "\"gravity\":null".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chain_swings_the_way_the_phone_tilts() {
        let mut t = Tilt::new();
        let mut out = Vec::new();
        // Tilted to the right: gravity points to the screen's right and down.
        for _ in 0..900 {
            t.step(&Sense { gravity: Some([707, -707, 0]) }, &mut out);
        }
        let end = t.sys.points[t.chain[SEGMENTS]].pos;
        let peg = t.sys.points[t.chain[0]].pos;
        assert!(end.x > peg.x + Fx::int(3), "the weight hangs to the right: {end:?}");
        assert!(end.y < peg.y - Fx::int(3), "and below: {end:?}");
        assert_eq!(t.observe(), "\"gravity\":[707,-707,0]");
    }
}
