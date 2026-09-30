//! FEEL: the haptics palette. A pad for each kind of pulse (tap, thud, tick, rise, fall, buzz) and two sliders,
//! intensity and sharpness, so each pulse can be tuned by hand on the phone that plays it.

use super::Card;
use crate::font;
use crate::gesture::{Gesture, Px};
use crate::haptics::{Kind, Pulse};
use crate::layer::{Color, Fit, Frame, Layer, Rect, Shape};

const PADS: [(&str, Kind); 6] = [
    ("TAP", Kind::Tap),
    ("THUD", Kind::Thud),
    ("TICK", Kind::Tick),
    ("RISE", Kind::Rise),
    ("FALL", Kind::Fall),
    ("BUZZ", Kind::Buzz { ms: 300 }),
];

/// Pads in world units: 2 columns × 3 rows, from the top of the board.
fn pad(i: usize) -> (f32, f32, f32, f32) {
    let (col, row) = ((i % 2) as f32, (i / 2) as f32);
    (0.75 + col * 4.0, 14.2 - row * 3.1, 3.5, 2.6)
}

/// Sliders in world units (left, centre y, width): intensity, then sharpness.
fn slider(i: usize) -> (f32, f32, f32) {
    (1.0, 3.9 - i as f32 * 2.2, 7.0)
}

pub struct Feel {
    pub intensity: f32,
    pub sharpness: f32,
    /// Which slider the finger drags.
    dragging: Option<usize>,
    /// The last pad played, and how many ticks its flash has left.
    flash: Option<(usize, u32)>,
    played: u32,
}

impl Default for Feel {
    fn default() -> Feel {
        Feel { intensity: 0.8, sharpness: 0.5, dragging: None, flash: None, played: 0 }
    }
}

impl Feel {
    fn world(fit: &Fit, at: Px) -> (f32, f32) {
        let w = fit.world(at);
        (crate::layer::fx_f32(w.x), crate::layer::fx_f32(w.y))
    }

    fn set(&mut self, i: usize, x: f32) {
        let (left, _, width) = slider(i);
        let v = ((x - left) / width).clamp(0.0, 1.0);
        if i == 0 {
            self.intensity = v;
        } else {
            self.sharpness = v;
        }
    }
}

impl Card for Feel {
    fn name(&self) -> &'static str {
        "FEEL"
    }

    fn input(&mut self, g: &Gesture, fit: &Fit, out: &mut Vec<Pulse>) {
        match *g {
            Gesture::Down(at) => {
                let (x, y) = Feel::world(fit, at);
                if let Some(i) = (0..PADS.len()).find(|&i| {
                    let (px, top, w, h) = pad(i);
                    x >= px && x <= px + w && y <= top && y >= top - h
                }) {
                    out.push(Pulse::new(PADS[i].1, self.intensity, self.sharpness));
                    self.flash = Some((i, 12));
                    self.played += 1;
                } else if let Some(i) = (0..2).find(|&i| {
                    let (left, cy, w) = slider(i);
                    x >= left - 0.5 && x <= left + w + 0.5 && (y - cy).abs() < 0.8
                }) {
                    self.dragging = Some(i);
                    self.set(i, x);
                }
            }
            Gesture::Move { to, .. } => {
                if let Some(i) = self.dragging {
                    let (x, _) = Feel::world(fit, to);
                    self.set(i, x);
                }
            }
            Gesture::Release { .. } => self.dragging = None,
            Gesture::Tap(_) | Gesture::Swipe { .. } => {}
        }
    }

    fn step(&mut self, _sense: &crate::sensors::Sense, _out: &mut Vec<Pulse>) {
        if let Some((i, n)) = self.flash {
            self.flash = n.checked_sub(1).map(|n| (i, n));
        }
    }

    fn draw(&self, _alpha: f32, fit: &Fit, frame: &mut Frame) {
        let s = fit.scale;
        let px = s * 0.11;
        for (i, (label, _)) in PADS.iter().enumerate() {
            let (x, top, w, h) = pad(i);
            let tl = fit.px(x, top);
            let rect = Rect::new(tl.x, tl.y, w * s, h * s);
            let lit = self.flash.is_some_and(|(j, _)| j == i);
            let color = if lit { Color::hex(0xffc93c) } else { Color::hex(0x3a2f86) };
            frame.push(Layer::Pieces, 0, Shape::Box { rect, r: 0.35 * s, color });
            let fg = if lit { Color::hex(0x1b1440) } else { Color::hex(0xf2efff) };
            font::centered(frame, Layer::Pieces, 1, rect.center(), px, fg, label);
        }
        for (i, (label, v)) in [("INTENSITY", self.intensity), ("SHARPNESS", self.sharpness)].iter().enumerate() {
            let (left, cy, w) = slider(i);
            let a = fit.px(left, cy);
            let b = fit.px(left + w, cy);
            let knob = fit.px(left + w * v, cy);
            frame.push(Layer::Pieces, 0, Shape::Capsule { a, b, r: 0.12 * s, color: Color::hexa(0xffffff, 0.15) });
            frame.push(Layer::Pieces, 1, Shape::Capsule { a, b: knob, r: 0.12 * s, color: Color::hex(0x3cc8ff) });
            frame.push(Layer::Pieces, 2, Shape::circle(knob, 0.38 * s, Color::hex(0xf2efff)));
            let text = format!("{label} {v:.2}");
            font::text(frame, Layer::Pieces, 2, Px::new(a.x, a.y - 0.95 * s), px * 0.85, Color::hexa(0xf2efff, 0.7), &text);
        }
    }

    fn observe(&self) -> String {
        format!("\"intensity\":{:.2},\"sharpness\":{:.2},\"played\":{}", self.intensity, self.sharpness, self.played)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fit() -> Fit {
        Fit::new(9.0, 16.0, Rect::new(0.0, 0.0, 900.0, 1600.0))
    }

    #[test]
    fn a_pad_plays_its_pulse_at_the_sliders_values() {
        let (f, mut c, mut out) = (fit(), Feel::default(), Vec::new());
        let (x, top, w, h) = pad(1);
        c.input(&Gesture::Down(f.px(x + w / 2.0, top - h / 2.0)), &f, &mut out);
        assert_eq!(out, vec![Pulse::new(Kind::Thud, 0.8, 0.5)]);
    }

    #[test]
    fn a_slider_follows_the_finger_and_clamps() {
        let (f, mut c, mut out) = (fit(), Feel::default(), Vec::new());
        let (left, cy, w) = slider(1);
        c.input(&Gesture::Down(f.px(left + w * 0.25, cy)), &f, &mut out);
        assert!((c.sharpness - 0.25).abs() < 0.01);
        c.input(&Gesture::Move { from: f.px(left, cy), to: f.px(left + w * 2.0, cy) }, &f, &mut out);
        assert_eq!(c.sharpness, 1.0);
        assert!(out.is_empty(), "a slider plays nothing");
    }
}
