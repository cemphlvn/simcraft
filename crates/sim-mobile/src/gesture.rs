//! Gestures: one finger's down, moves and up, turned into what a casual game reacts to. Pure (times are given, no
//! clock, no window), so every rule here is tested without a phone.
//!
//! Every move is also reported as a stroke (`Move { from, to }`): a game tests it against its shapes to catch or
//! cut a rope by swiping across it. A second finger is ignored until the first lifts.

/// A point on the screen, in physical pixels, y down.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Px {
    pub x: f32,
    pub y: f32,
}

impl Px {
    pub const fn new(x: f32, y: f32) -> Px {
        Px { x, y }
    }

    pub fn dist(self, o: Px) -> f32 {
        ((self.x - o.x).powi(2) + (self.y - o.y).powi(2)).sqrt()
    }
}

/// The four directions a swipe can go (screen directions: `Up` is towards the top of the screen).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gesture {
    /// A finger touched the screen.
    Down(Px),
    /// It moved: the stroke since the last report.
    Move { from: Px, to: Px },
    /// It lifted soon, without moving much.
    Tap(Px),
    /// It lifted after a fast straight stroke.
    Swipe { from: Px, to: Px, dir: Dir },
    /// It lifted (always, after `Tap` or `Swipe` if either): where, and how fast it was going (pixels a second).
    Release { at: Px, velocity: Px },
}

/// How a recognizer decides; sizes in physical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tuning {
    /// Movement under this is still a tap (a thumb wobbles).
    pub slop: f32,
    /// A tap is shorter than this (ms).
    pub tap_ms: u64,
    /// A swipe is faster than this (pixels a second, over the whole stroke).
    pub swipe_speed: f32,
}

impl Tuning {
    /// For a screen of `scale` physical pixels per point (iOS) or per dp (Android).
    pub fn for_scale(scale: f32) -> Tuning {
        Tuning { slop: 10.0 * scale, tap_ms: 250, swipe_speed: 600.0 * scale }
    }
}

#[derive(Clone, Copy, Debug)]
struct Touch {
    id: u64,
    start: Px,
    t0: u64,
    last: Px,
    t_last: u64,
    moved: bool,
    /// Pixels a second, smoothed over recent moves.
    velocity: Px,
}

/// Turns touches into [`Gesture`]s.
#[derive(Clone, Debug)]
pub struct Recognizer {
    pub tuning: Tuning,
    touch: Option<Touch>,
}

impl Recognizer {
    pub fn new(tuning: Tuning) -> Recognizer {
        Recognizer { tuning, touch: None }
    }

    /// Finger `id` touched at `at`, `t` ms into the app.
    pub fn down(&mut self, id: u64, at: Px, t: u64) -> Vec<Gesture> {
        if self.touch.is_some() {
            return Vec::new();
        }
        self.touch = Some(Touch { id, start: at, t0: t, last: at, t_last: t, moved: false, velocity: Px::default() });
        vec![Gesture::Down(at)]
    }

    pub fn moved(&mut self, id: u64, at: Px, t: u64) -> Vec<Gesture> {
        let slop = self.tuning.slop;
        let Some(k) = self.touch.as_mut().filter(|k| k.id == id) else { return Vec::new() };
        if at == k.last {
            return Vec::new();
        }
        let dt = (t.saturating_sub(k.t_last)).max(1) as f32 / 1000.0;
        let now = Px::new((at.x - k.last.x) / dt, (at.y - k.last.y) / dt);
        // Recent moves count most: a flick's speed is its end, not its average.
        k.velocity = Px::new(k.velocity.x * 0.3 + now.x * 0.7, k.velocity.y * 0.3 + now.y * 0.7);
        let from = k.last;
        k.last = at;
        k.t_last = t;
        k.moved |= at.dist(k.start) > slop;
        vec![Gesture::Move { from, to: at }]
    }

    pub fn up(&mut self, id: u64, at: Px, t: u64) -> Vec<Gesture> {
        let mut out = self.moved(id, at, t);
        let Some(k) = self.touch.take_if(|k| k.id == id) else { return out };
        let ms = t.saturating_sub(k.t0);
        if !k.moved && ms < self.tuning.tap_ms {
            out.push(Gesture::Tap(k.start));
        } else if k.moved {
            let (dx, dy) = (at.x - k.start.x, at.y - k.start.y);
            let speed = at.dist(k.start) / (ms.max(1) as f32 / 1000.0);
            if speed > self.tuning.swipe_speed {
                let dir = if dx.abs() > dy.abs() {
                    if dx > 0.0 { Dir::Right } else { Dir::Left }
                } else if dy > 0.0 {
                    Dir::Down
                } else {
                    Dir::Up
                };
                out.push(Gesture::Swipe { from: k.start, to: at, dir });
            }
        }
        // A finger that stopped before lifting has no speed left.
        let velocity = if t.saturating_sub(k.t_last) > 60 { Px::default() } else { k.velocity };
        out.push(Gesture::Release { at, velocity });
        out
    }

    /// The system took the touch (a notification, a call): no tap, no swipe, but a release where it was.
    pub fn cancel(&mut self, id: u64) -> Vec<Gesture> {
        match self.touch.take_if(|k| k.id == id) {
            Some(k) => vec![Gesture::Release { at: k.last, velocity: Px::default() }],
            None => Vec::new(),
        }
    }

    pub fn held(&self) -> bool {
        self.touch.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec() -> Recognizer {
        Recognizer::new(Tuning::for_scale(1.0))
    }

    #[test]
    fn a_short_still_touch_is_a_tap() {
        let mut r = rec();
        r.down(1, Px::new(100.0, 100.0), 0);
        let g = r.up(1, Px::new(103.0, 101.0), 120);
        assert!(matches!(g[..], [Gesture::Move { .. }, Gesture::Tap(p), Gesture::Release { .. }] if p == Px::new(100.0, 100.0)), "{g:?}");
    }

    #[test]
    fn a_long_still_touch_is_not_a_tap() {
        let mut r = rec();
        r.down(1, Px::new(100.0, 100.0), 0);
        let g = r.up(1, Px::new(100.0, 100.0), 900);
        assert!(matches!(g[..], [Gesture::Release { .. }]), "{g:?}");
    }

    #[test]
    fn a_fast_stroke_is_a_swipe_in_its_main_direction() {
        let mut r = rec();
        r.down(1, Px::new(100.0, 500.0), 0);
        r.moved(1, Px::new(110.0, 400.0), 50);
        let g = r.up(1, Px::new(120.0, 300.0), 100);
        assert!(g.iter().any(|g| matches!(g, Gesture::Swipe { dir: Dir::Up, .. })), "{g:?}");
        let Some(Gesture::Release { velocity, .. }) = g.last() else { panic!("{g:?}") };
        assert!(velocity.y < -1000.0, "a flick upwards keeps its speed: {velocity:?}");
    }

    #[test]
    fn a_slow_drag_is_neither_tap_nor_swipe_and_ends_still() {
        let mut r = rec();
        r.down(1, Px::new(0.0, 0.0), 0);
        for i in 1..=10 {
            r.moved(1, Px::new(i as f32 * 10.0, 0.0), i * 100);
        }
        let g = r.up(1, Px::new(100.0, 0.0), 1400);
        assert!(matches!(g[..], [Gesture::Release { velocity, .. }] if velocity == Px::default()), "{g:?}");
    }

    #[test]
    fn every_move_is_a_stroke_from_the_last_point() {
        let mut r = rec();
        r.down(1, Px::new(0.0, 0.0), 0);
        assert_eq!(r.moved(1, Px::new(5.0, 5.0), 16), vec![Gesture::Move { from: Px::new(0.0, 0.0), to: Px::new(5.0, 5.0) }]);
        assert_eq!(r.moved(1, Px::new(9.0, 5.0), 32), vec![Gesture::Move { from: Px::new(5.0, 5.0), to: Px::new(9.0, 5.0) }]);
    }

    #[test]
    fn a_second_finger_is_ignored_while_the_first_is_down() {
        let mut r = rec();
        r.down(1, Px::new(0.0, 0.0), 0);
        assert!(r.down(2, Px::new(50.0, 50.0), 10).is_empty());
        assert!(r.moved(2, Px::new(60.0, 50.0), 20).is_empty());
        assert!(r.up(2, Px::new(60.0, 50.0), 30).is_empty());
        assert!(r.held());
    }

    #[test]
    fn a_cancelled_touch_releases_without_a_tap() {
        let mut r = rec();
        r.down(1, Px::new(10.0, 10.0), 0);
        assert_eq!(r.cancel(1), vec![Gesture::Release { at: Px::new(10.0, 10.0), velocity: Px::default() }]);
        assert!(!r.held());
    }
}
