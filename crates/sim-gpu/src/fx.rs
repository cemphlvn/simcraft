//! Camera effects driven by game events: a dash kicks the field of view out and streaks the screen, a jump lifts the
//! camera on an arc, a landing bumps it, a crash shakes it. Each effect is an `Anim` over camera channels, started
//! when the game emits the event; overlapping effects add up. Games override or add effects in data (`fx:`);
//! the defaults below are simcraft's built-in feel.
//!
//! Channels: `fov` (degrees added), `shake` (world units of jitter), `lift` (camera height added), `roll` (degrees),
//! `pitch` (degrees, looking down is positive), `streaks` (0..1 speed lines), `flash` (0..1 white), `vignette`
//! (0..1 extra darkening at the edges), `hurt` (0..1 red at the edges: damage).

use std::collections::BTreeMap;

use sim_render::anim::{Anim, Ease, Key};

use crate::stage::{Quad, WHITE, Wrap};

pub const CHANNELS: [&str; 9] = ["fov", "shake", "lift", "roll", "pitch", "streaks", "flash", "vignette", "hurt"];

/// The camera's offsets at one instant (sums over the running effects).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Fx {
    pub fov: f32,
    pub shake: f32,
    pub lift: f32,
    pub roll: f32,
    pub pitch: f32,
    pub streaks: f32,
    pub flash: f32,
    pub vignette: f32,
    pub hurt: f32,
}

/// A key as written in code: (time, value, ease).
type CodeKey = (f32, f32, Ease);

fn track(keys: &[CodeKey]) -> Vec<Key> {
    keys.iter().map(|&(t, v, e)| Key::Eased(t, v, e)).collect()
}

fn anim(tracks: &[(&str, &[CodeKey])]) -> Anim {
    Anim { tracks: tracks.iter().map(|(n, k)| (n.to_string(), track(k))).collect(), looping: false }
}

use Ease::{BackOut, BounceOut, ElasticOut, Linear, QuadIn, QuadOut, SineInOut};

/// simcraft's built-in effects, by event name.
pub fn default_fx(event: &str) -> Option<Anim> {
    Some(match event {
        // A dash: the view widens with a punch and settles, speed lines rush past, a small jolt and a flash.
        "dashed" => anim(&[
            ("fov", &[(0.0, 0.0, Linear), (0.12, 22.0, BackOut), (0.7, 14.0, SineInOut), (1.1, 0.0, QuadIn)]),
            ("streaks", &[(0.0, 0.0, Linear), (0.08, 1.0, QuadOut), (0.75, 0.8, Linear), (1.1, 0.0, QuadIn)]),
            ("shake", &[(0.0, 0.05, Linear), (0.35, 0.0, QuadOut)]),
            ("pitch", &[(0.0, 0.0, Linear), (0.1, -2.0, QuadOut), (0.6, 0.0, SineInOut)]),
            ("flash", &[(0.0, 0.18, Linear), (0.25, 0.0, QuadOut)]),
            ("vignette", &[(0.0, 0.0, Linear), (0.15, 0.5, QuadOut), (1.1, 0.0, QuadIn)]),
        ]),
        // A jump: the camera rises on an arc (up fast, down with gravity) and tips up a little.
        "jumped" => anim(&[
            ("lift", &[(0.0, 0.0, Linear), (0.28, 0.75, QuadOut), (0.58, 0.0, QuadIn)]),
            ("pitch", &[(0.0, 0.0, Linear), (0.2, -4.0, QuadOut), (0.58, 2.0, QuadIn)]),
        ]),
        // Touchdown: a short dip and a jolt.
        "landed" => anim(&[
            ("lift", &[(0.0, -0.09, Linear), (0.3, 0.0, BackOut)]),
            ("pitch", &[(0.0, 3.0, Linear), (0.3, 0.0, BackOut)]),
            ("shake", &[(0.0, 0.05, Linear), (0.25, 0.0, QuadOut)]),
        ]),
        // A hit: the car bounces through it. A hop, a wobble that rings down, a squeeze of the view that springs
        // back, red at the edges. Cartoonish on purpose: a hit costs a life, not the fun.
        "crashed" => anim(&[
            ("lift", &[(0.0, 0.0, Linear), (0.1, 0.16, QuadOut), (0.45, 0.0, BounceOut)]),
            (
                "roll",
                &[
                    (0.0, 0.0, Linear),
                    (0.07, 8.0, QuadOut),
                    (0.19, -6.0, SineInOut),
                    (0.31, 3.5, SineInOut),
                    (0.45, -1.5, SineInOut),
                    (0.6, 0.0, SineInOut),
                ],
            ),
            ("fov", &[(0.0, -9.0, Linear), (0.7, 0.0, ElasticOut)]),
            ("shake", &[(0.0, 0.07, Linear), (0.3, 0.0, QuadOut)]),
            ("hurt", &[(0.0, 0.0, Linear), (0.05, 0.75, QuadOut), (0.7, 0.0, QuadIn)]),
            ("flash", &[(0.0, 0.12, Linear), (0.15, 0.0, QuadOut)]),
        ]),
        // Braking: the nose dips.
        "braked" => anim(&[
            ("pitch", &[(0.0, 0.0, Linear), (0.15, 3.0, QuadOut), (0.9, 0.0, SineInOut)]),
            ("fov", &[(0.0, 0.0, Linear), (0.2, -5.0, QuadOut), (1.0, 0.0, SineInOut)]),
        ]),
        // Oil: a lurch.
        "skidded" => anim(&[
            ("roll", &[(0.0, 0.0, Linear), (0.12, 7.0, QuadOut), (0.5, 0.0, BackOut)]),
            ("shake", &[(0.0, 0.04, Linear), (0.4, 0.0, QuadOut)]),
        ]),
        // Good things: a soft flash.
        "refueled" | "stamped" | "nitro" => anim(&[("flash", &[(0.0, 0.25, Linear), (0.4, 0.0, QuadOut)])]),
        "coin" => anim(&[("flash", &[(0.0, 0.08, Linear), (0.2, 0.0, QuadOut)])]),
        _ => return None,
    })
}

/// The effects running now.
#[derive(Clone, Debug, Default)]
pub struct FxState {
    /// (effect, start time, strength): an effect may be scheduled ahead and scaled (a longer switch, a bigger punch).
    running: Vec<(Anim, f32, f32)>,
}

impl FxState {
    /// The game emitted `event` at time `now`: start its effect (the game's own, else the built-in one).
    pub fn trigger(&mut self, overrides: &BTreeMap<String, Anim>, event: &str, now: f32) {
        if let Some(a) = overrides.get(event).cloned().or_else(|| default_fx(event)) {
            self.running.push((a, now, 1.0));
        }
    }

    /// Plays `anim` from `at` (may be in the future) with every channel scaled by `strength`.
    pub fn play(&mut self, anim: &Anim, at: f32, strength: f32) {
        self.running.push((anim.clone(), at, strength));
    }

    pub fn sample(&mut self, now: f32) -> Fx {
        self.running.retain(|(a, at, _)| !a.done(now - at));
        let mut fx = Fx::default();
        for (a, at, k) in &self.running {
            if now < *at {
                continue;
            }
            let t = now - at;
            let c = |n: &str| a.channel(n, t).unwrap_or(0.0) * k;
            fx.fov += c("fov");
            fx.shake += c("shake");
            fx.lift += c("lift");
            fx.roll += c("roll");
            fx.pitch += c("pitch");
            fx.streaks += c("streaks");
            fx.flash += c("flash");
            fx.vignette += c("vignette");
            fx.hurt += c("hurt");
        }
        fx
    }
}

/// A smooth jitter in -1..1 (sum of incommensurate sines: no pops, never repeats visibly).
pub fn jitter(t: f32, seed: f32) -> f32 {
    ((t * 37.0 + seed).sin() * 0.5 + (t * 23.3 + seed * 2.1).sin() * 0.3 + (t * 61.7 + seed * 0.7).sin() * 0.2).clamp(-1.0, 1.0)
}

/// Speed lines, a flash and extra vignette over the frame.
pub fn overlay(fx: &Fx, w: f32, h: f32, time: f32) -> Vec<Quad> {
    let mut out = Vec::new();
    let quad = |image: &str, x: f32, y: f32, qw: f32, qh: f32, c: [f32; 4], rot: f32| Quad {
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
        rot,
    };
    let s = fx.streaks.clamp(0.0, 1.0);
    if s > 0.01 {
        let (cx, cy) = (w / 2.0, h * 0.46);
        let reach = (w * w + h * h).sqrt() / 2.0;
        for i in 0..70u64 {
            let n = sim_render::pixel::noise(i as i64, 0, 41);
            let angle = (n % 3600) as f32 / 3600.0 * std::f32::consts::TAU;
            // Each line travels outward and wraps; faster lines are longer.
            let speed = 1.6 + (n / 3600 % 100) as f32 / 40.0;
            let r = ((time * speed + (n / 360_000 % 1000) as f32 / 1000.0).fract() * 0.75 + 0.25) * reach;
            let len = reach * (0.08 + 0.12 * (r / reach)) * s;
            let (dx, dy) = (angle.cos(), angle.sin());
            let (mx, my) = (cx + dx * r, cy + dy * r);
            let thick = 1.5 + (n % 3) as f32;
            let a = s * 0.55 * (r / reach);
            out.push(quad(WHITE, mx - len / 2.0, my - thick / 2.0, len, thick, [1.0, 1.0, 1.0, a], angle.to_degrees()));
        }
    }
    if fx.vignette > 0.01 {
        out.push(quad("__vignette", 0.0, 0.0, w, h, [0.0, 0.0, 0.0, fx.vignette.clamp(0.0, 1.0)], 0.0));
    }
    if fx.hurt > 0.01 {
        out.push(quad("__vignette", 0.0, 0.0, w, h, [0.85, 0.05, 0.05, (fx.hurt * 1.2).clamp(0.0, 1.0)], 0.0));
    }
    if fx.flash > 0.01 {
        out.push(quad(WHITE, 0.0, 0.0, w, h, [1.0, 0.97, 0.9, fx.flash.clamp(0.0, 1.0)], 0.0));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dash_kicks_the_view_out_and_settles() {
        let mut st = FxState::default();
        st.trigger(&BTreeMap::new(), "dashed", 10.0);
        let peak = st.sample(10.15);
        assert!(peak.fov > 15.0 && peak.streaks > 0.9, "{peak:?}");
        assert_eq!(st.sample(20.0), Fx::default(), "effects end and are dropped");
    }

    #[test]
    fn effects_add_up_and_games_override_them() {
        let mut st = FxState::default();
        st.trigger(&BTreeMap::new(), "crashed", 0.0);
        st.trigger(&BTreeMap::new(), "landed", 0.0);
        let both = st.sample(0.0);
        assert!((both.shake - 0.12).abs() < 1e-4, "shakes add: {}", both.shake);
        let mine: BTreeMap<String, Anim> =
            [("dashed".to_string(), ron::from_str(r#"(tracks: { "fov": [(0.0, 5.0), (1.0, 5.0)] })"#).unwrap())].into_iter().collect();
        let mut st = FxState::default();
        st.trigger(&mine, "dashed", 0.0);
        assert_eq!(st.sample(0.5).fov, 5.0);
        assert_eq!(st.sample(0.5).streaks, 0.0, "an override replaces the built-in effect entirely");
        st.trigger(&mine, "no_such_event", 0.0);
    }
}
