//! Animation as data: keyframe tracks with easing, sampled as a pure function of time.
//!
//! An `Anim` is a set of tracks, one per channel (`scale`, `x`, `y`, `rot`, `alpha`, `bright`). Each track is a list
//! of keys `(time, value)` or `(time, value, ease)`: the ease shapes the move *into* that key. Sampling needs only
//! the time since the animation started, so any frame can be computed (and tested) on its own, at any frame rate,
//! and the simulation never sees it: animations are display, never state.
//!
//! ```ron
//! "press": (tracks: { "scale": [(0.0, 1.0), (0.06, 0.88, "quad_out"), (0.3, 1.0, "back_out")] }),
//! "idle":  (tracks: { "rot": [(0.0, -2.0), (1.2, 2.0, "sine_in_out"), (2.4, -2.0, "sine_in_out")] }, loop: true),
//! ```

use std::collections::BTreeMap;

use serde::Deserialize;

/// Easing curves: how a value travels from one key to the next (`t` 0..1 → progress, may overshoot).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Ease {
    #[default]
    Linear,
    /// Jump at the key (no in-between).
    Step,
    QuadIn,
    QuadOut,
    QuadInOut,
    CubicOut,
    SineInOut,
    /// Overshoots a little, then settles (a pop).
    BackOut,
    /// Springs past the target and rings down.
    ElasticOut,
    /// Lands and bounces.
    BounceOut,
}

impl Ease {
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Ease::Linear => t,
            Ease::Step => {
                if t >= 1.0 {
                    1.0
                } else {
                    0.0
                }
            }
            Ease::QuadIn => t * t,
            Ease::QuadOut => 1.0 - (1.0 - t) * (1.0 - t),
            Ease::QuadInOut => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
                }
            }
            Ease::CubicOut => 1.0 - (1.0 - t).powi(3),
            Ease::SineInOut => -((std::f32::consts::PI * t).cos() - 1.0) / 2.0,
            Ease::BackOut => {
                let (c1, c3) = (1.70158, 2.70158);
                1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2)
            }
            Ease::ElasticOut => {
                if t == 0.0 || t == 1.0 {
                    t
                } else {
                    2f32.powf(-10.0 * t) * ((t * 10.0 - 0.75) * (2.0 * std::f32::consts::PI / 3.0)).sin() + 1.0
                }
            }
            Ease::BounceOut => {
                let (n, d) = (7.5625, 2.75);
                if t < 1.0 / d {
                    n * t * t
                } else if t < 2.0 / d {
                    let t = t - 1.5 / d;
                    n * t * t + 0.75
                } else if t < 2.5 / d {
                    let t = t - 2.25 / d;
                    n * t * t + 0.9375
                } else {
                    let t = t - 2.625 / d;
                    n * t * t + 0.984375
                }
            }
        }
    }
}

/// One key: at `time` seconds the channel is `value`, arriving with `ease`.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum Key {
    Plain(f32, f32),
    Eased(f32, f32, Ease),
}

impl Key {
    fn parts(self) -> (f32, f32, Ease) {
        match self {
            Key::Plain(t, v) => (t, v, Ease::Linear),
            Key::Eased(t, v, e) => (t, v, e),
        }
    }
}

/// The channels an animation can drive, and their resting values.
pub const CHANNELS: [(&str, f32); 6] = [("scale", 1.0), ("x", 0.0), ("y", 0.0), ("rot", 0.0), ("alpha", 1.0), ("bright", 1.0)];

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Anim {
    pub tracks: BTreeMap<String, Vec<Key>>,
    /// Start over at the end (idle loops); otherwise the last keys hold.
    #[serde(default, rename = "loop")]
    pub looping: bool,
}

/// Where an animation leaves a thing at one instant. Offsets are in the thing's own size (x 0.5 = half its width);
/// `rot` in degrees; `bright` multiplies its colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub scale: f32,
    pub x: f32,
    pub y: f32,
    pub rot: f32,
    pub alpha: f32,
    pub bright: f32,
}

impl Default for Pose {
    fn default() -> Self {
        Pose { scale: 1.0, x: 0.0, y: 0.0, rot: 0.0, alpha: 1.0, bright: 1.0 }
    }
}

impl Pose {
    /// Two poses on top of each other (an idle loop under a hover, a press on top): scales and alphas multiply,
    /// offsets and rotations add.
    pub fn then(self, o: Pose) -> Pose {
        Pose {
            scale: self.scale * o.scale,
            x: self.x + o.x,
            y: self.y + o.y,
            rot: self.rot + o.rot,
            alpha: self.alpha * o.alpha,
            bright: self.bright * o.bright,
        }
    }
}

impl Anim {
    /// Length in seconds: the last key of any track.
    pub fn duration(&self) -> f32 {
        self.tracks.values().filter_map(|k| k.last()).map(|k| k.parts().0).fold(0.0, f32::max)
    }

    /// Has it played out (a one-shot past its last key)?
    pub fn done(&self, t: f32) -> bool {
        !self.looping && t >= self.duration()
    }

    /// The pose `t` seconds after the start.
    pub fn sample(&self, t: f32) -> Pose {
        let d = self.duration();
        let t = if self.looping && d > 0.0 { t.rem_euclid(d) } else { t.max(0.0) };
        let mut p = Pose::default();
        for (name, keys) in &self.tracks {
            let v = sample_track(keys, t);
            match name.as_str() {
                "scale" => p.scale = v,
                "x" => p.x = v,
                "y" => p.y = v,
                "rot" => p.rot = v,
                "alpha" => p.alpha = v,
                "bright" => p.bright = v,
                _ => {}
            }
        }
        p
    }

    /// Any channel by name at `t` (for things other than a pose: a camera's `fov`, `shake`, `streaks`...); `None` when
    /// the animation has no such track.
    pub fn channel(&self, name: &str, t: f32) -> Option<f32> {
        let d = self.duration();
        let t = if self.looping && d > 0.0 { t.rem_euclid(d) } else { t.max(0.0) };
        self.tracks.get(name).map(|keys| sample_track(keys, t))
    }

    /// Unknown channel names (a typo would silently do nothing): for load-time checks.
    pub fn unknown_channels(&self) -> Vec<String> {
        self.tracks.keys().filter(|k| !CHANNELS.iter().any(|(c, _)| c == k)).cloned().collect()
    }
}

fn sample_track(keys: &[Key], t: f32) -> f32 {
    let Some(first) = keys.first() else { return 0.0 };
    let (t0, v0, _) = first.parts();
    if t <= t0 {
        return v0;
    }
    for w in keys.windows(2) {
        let ((ta, va, _), (tb, vb, ease)) = (w[0].parts(), w[1].parts());
        if t <= tb {
            let u = if tb > ta { (t - ta) / (tb - ta) } else { 1.0 };
            return va + (vb - va) * ease.apply(u);
        }
    }
    keys.last().map_or(0.0, |k| k.parts().1)
}

/// Named animations with one playing on top of a looping base: what a button or an item carries.
#[derive(Clone, Debug, Default)]
pub struct Player {
    /// The one-shot playing now and when it started.
    pub current: Option<(String, f32)>,
}

impl Player {
    pub fn play(&mut self, name: &str, now: f32) {
        self.current = Some((name.to_string(), now));
    }

    /// The looping base (e.g. "idle") with the current one-shot on top, if it is still running.
    pub fn pose(&mut self, anims: &BTreeMap<String, Anim>, base: &str, now: f32) -> Pose {
        let mut p = anims.get(base).map_or_else(Pose::default, |a| a.sample(now));
        if let Some((name, at)) = &self.current {
            match anims.get(name) {
                Some(a) if !a.done(now - at) => p = p.then(a.sample(now - at)),
                _ => self.current = None,
            }
        }
        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anim(src: &str) -> Anim {
        ron::from_str(src).expect("parses")
    }

    #[test]
    fn eases_start_at_zero_end_at_one_and_back_overshoots() {
        for e in [
            Ease::Linear,
            Ease::QuadIn,
            Ease::QuadOut,
            Ease::QuadInOut,
            Ease::CubicOut,
            Ease::SineInOut,
            Ease::BackOut,
            Ease::ElasticOut,
            Ease::BounceOut,
        ] {
            assert!(e.apply(0.0).abs() < 1e-4 && (e.apply(1.0) - 1.0).abs() < 1e-4, "{e:?}");
        }
        assert!((0..100).any(|i| Ease::BackOut.apply(i as f32 / 100.0) > 1.0), "a pop overshoots");
        assert_eq!(Ease::Step.apply(0.99), 0.0);
    }

    #[test]
    fn a_track_holds_before_and_after_its_keys_and_eases_between() {
        let a = anim(r#"(tracks: { "scale": [(0.1, 0.0), (0.3, 1.0, quad_out)] })"#);
        assert_eq!(a.sample(0.0).scale, 0.0);
        assert_eq!(a.sample(5.0).scale, 1.0);
        let mid = a.sample(0.2).scale;
        assert!(mid > 0.5 && mid < 1.0, "quad_out is past halfway at the midpoint: {mid}");
        assert!(a.done(0.31) && !a.done(0.29));
    }

    #[test]
    fn loops_wrap_and_poses_stack() {
        let idle = anim(r#"(tracks: { "rot": [(0.0, -2.0), (1.0, 2.0), (2.0, -2.0)] }, loop: true)"#);
        assert_eq!(idle.sample(0.5), idle.sample(2.5));
        assert!(!idle.done(100.0));
        let p = Pose { scale: 2.0, rot: 1.0, ..Pose::default() }.then(Pose { scale: 0.5, rot: 3.0, ..Pose::default() });
        assert_eq!((p.scale, p.rot), (1.0, 4.0));
    }

    #[test]
    fn a_player_runs_a_one_shot_over_the_base_then_drops_it() {
        let anims: BTreeMap<String, Anim> = [
            ("idle".to_string(), anim(r#"(tracks: { "y": [(0.0, 0.0), (1.0, 0.1), (2.0, 0.0)] }, loop: true)"#)),
            ("press".to_string(), anim(r#"(tracks: { "scale": [(0.0, 1.0), (0.1, 0.8), (0.3, 1.0)] })"#)),
        ]
        .into_iter()
        .collect();
        let mut pl = Player::default();
        pl.play("press", 10.0);
        assert!((pl.pose(&anims, "idle", 10.1).scale - 0.8).abs() < 1e-4);
        assert_eq!(pl.pose(&anims, "idle", 11.0).scale, 1.0);
        assert!(pl.current.is_none(), "a finished one-shot is dropped");
        assert_eq!(anim(r#"(tracks: { "scael": [(0.0, 1.0)] })"#).unknown_channels(), vec!["scael".to_string()]);
    }
}
