//! The Mobile Capability Playground: where the mobile core's mechanics are tuned before a game uses them
//! (`docs/architecture.md`, Mobile core). One card per capability, switched by tabs at the bottom of the safe area;
//! a workbench background (a grid in world units, the safe area's corners, a fading trail behind every finger) so
//! distances, speeds and what the phone registered can be read by eye.

mod feel;
mod game;
mod rope;
mod tilt;
mod touch;

use std::collections::BTreeMap;

use crate::font;
use crate::gesture::{Gesture, Px};
use crate::haptics::Pulse;
use crate::layer::{Anchor, Color, Fit, Frame, Insets, Layer, Rect, Shape, place};
use crate::sensors::Sense;

/// The board every card uses: 9 × 16 world units, portrait, y up.
pub const BOARD: (f32, f32) = (9.0, 16.0);
/// Simulation ticks a second.
pub const TICK_RATE: u32 = 60;

/// A raw touch's phase (every finger, not only the one gestures follow).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Down,
    Move,
    Up,
}

/// A capability on a card. Every method but `name` and `draw` has a do-nothing default.
pub trait Card {
    /// Its tab's label.
    fn name(&self) -> &'static str;
    /// Every finger, as it happens.
    fn touch(&mut self, _id: u64, _phase: Phase, _at: Px, _fit: &Fit, _out: &mut Vec<Pulse>) {}
    /// The first finger's gestures.
    fn input(&mut self, _g: &Gesture, _fit: &Fit, _out: &mut Vec<Pulse>) {}
    /// One simulation tick, with what the sensors read.
    fn step(&mut self, _sense: &Sense, _out: &mut Vec<Pulse>) {}
    /// The card drawn `alpha` of the way from the last tick to this one.
    fn draw(&self, alpha: f32, fit: &Fit, frame: &mut Frame);
    /// Its live values for the stats line (JSON fields, without braces).
    fn observe(&self) -> String {
        String::new()
    }
    /// A card that is a whole game takes the whole screen: no workbench, no header, no tab bar (it draws its own
    /// way back, [`Card::wants_lab`]).
    fn fullscreen(&self) -> bool {
        false
    }
    /// Where everything is on the screen, before every touch and draw (a fullscreen card places its own HUD).
    fn layout(&mut self, _layout: &Layout) {}
    /// What it shows in 3D, drawn under the 2D layers.
    fn scene(&self, _alpha: f32, _layout: &Layout) -> Option<crate::draw3d::Scene3> {
        None
    }
    /// A fullscreen card asks to go back to the playground's cards (its own button was tapped).
    fn wants_lab(&mut self) -> bool {
        false
    }
}

/// Where everything is on a screen of a given size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub safe: Rect,
    pub header: Rect,
    pub tabs: Rect,
    pub fit: Fit,
    /// Screen pixels per font pixel.
    pub px: f32,
    /// The whole screen (pixels) and pixels per point.
    pub screen: (f32, f32),
    pub scale: f32,
}

impl Layout {
    /// For a screen of `w` × `h` pixels at `scale` pixels per point.
    pub fn new(w: f32, h: f32, scale: f32) -> Layout {
        let safe = Insets::phone(scale).safe(w, h);
        let m = 12.0 * scale;
        let header = place(Anchor::Top, (safe.w - 2.0 * m, 28.0 * scale), (0.0, m * 0.5), safe);
        let tabs = place(Anchor::Bottom, (safe.w - 2.0 * m, 52.0 * scale), (0.0, m * 0.5), safe);
        let top = header.y + header.h + m;
        let area = Rect::new(safe.x + m, top, safe.w - 2.0 * m, tabs.y - m - top);
        Layout { safe, header, tabs, fit: Fit::new(BOARD.0, BOARD.1, area), px: 2.2 * scale, screen: (w, h), scale }
    }

    /// The tab under `at`, if any.
    pub fn tab_at(&self, at: Px, n: usize) -> Option<usize> {
        let t = self.tabs;
        let inside = at.x >= t.x && at.x < t.x + t.w && at.y >= t.y && at.y < t.y + t.h;
        (inside && n > 0).then(|| (((at.x - t.x) / t.w * n as f32) as usize).min(n - 1))
    }
}

/// How long a finger's trail lasts (ms).
const TRAIL_MS: f32 = 700.0;

pub struct Playground {
    cards: Vec<Box<dyn Card>>,
    pub active: usize,
    /// Each finger's trail: points and their age (ms). Lifted fingers' trails fade out too.
    trails: BTreeMap<u64, Vec<(Px, f32)>>,
    /// A touch that started on the tabs: its gestures do not reach the card.
    on_tabs: bool,
    /// Frames a second, as the stats last measured (shown in the header).
    pub fps: f32,
}

impl Default for Playground {
    fn default() -> Playground {
        Playground::new()
    }
}

impl Playground {
    pub fn new() -> Playground {
        Playground {
            cards: vec![
                Box::new(crate::smash::Smash::default()),
                Box::new(game::GameCard::new()),
                Box::new(rope::Rope::new()),
                Box::new(touch::Touch::default()),
                Box::new(feel::Feel::default()),
                Box::new(tilt::Tilt::new()),
            ],
            active: 0,
            trails: BTreeMap::new(),
            on_tabs: false,
            fps: 0.0,
        }
    }

    pub fn card(&self) -> &dyn Card {
        self.cards[self.active].as_ref()
    }

    /// A raw touch of finger `id` (every finger).
    pub fn touch(&mut self, id: u64, phase: Phase, at: Px, layout: &Layout, out: &mut Vec<Pulse>) {
        self.cards[self.active].layout(layout);
        let full = self.cards[self.active].fullscreen();
        if phase == Phase::Down
            && !full
            && let Some(i) = layout.tab_at(at, self.cards.len())
        {
            if i != self.active {
                self.active = i;
                out.push(Pulse::new(crate::haptics::Kind::Tick, 0.5, 0.7));
            }
            self.on_tabs = true;
            return;
        }
        if phase != Phase::Up || self.trails.contains_key(&id) {
            self.trails.entry(id).or_default().push((at, 0.0));
        }
        if !self.on_tabs {
            self.cards[self.active].touch(id, phase, at, &layout.fit, out);
        }
    }

    /// The first finger's gestures.
    pub fn input(&mut self, g: &Gesture, layout: &Layout, out: &mut Vec<Pulse>) {
        if self.on_tabs {
            if matches!(g, Gesture::Release { .. }) {
                self.on_tabs = false;
            }
            return;
        }
        self.cards[self.active].layout(layout);
        self.cards[self.active].input(g, &layout.fit, out);
        if self.cards[self.active].wants_lab() {
            self.active = (self.active + 1) % self.cards.len();
            out.push(Pulse::new(crate::haptics::Kind::Tick, 0.5, 0.7));
        }
    }

    pub fn step(&mut self, sense: &Sense, out: &mut Vec<Pulse>) {
        self.cards[self.active].step(sense, out);
    }

    /// Ages the trails by `ms` and drops what has faded.
    pub fn age(&mut self, ms: f32) {
        for t in self.trails.values_mut() {
            for p in t.iter_mut() {
                p.1 += ms;
            }
            t.retain(|p| p.1 < TRAIL_MS);
        }
        self.trails.retain(|_, t| !t.is_empty());
    }

    pub fn observe(&self) -> String {
        let card = self.card();
        let extra = card.observe();
        if extra.is_empty() { format!("\"card\":\"{}\"", card.name()) } else { format!("\"card\":\"{}\",{extra}", card.name()) }
    }

    pub fn draw(&mut self, alpha: f32, layout: &Layout, frame: &mut Frame) {
        let card = &mut self.cards[self.active];
        card.layout(layout);
        if card.fullscreen() {
            card.draw(alpha, &layout.fit, frame);
            return;
        }
        self.workbench(layout, frame);
        self.cards[self.active].draw(alpha, &layout.fit, frame);
        self.chrome(layout, frame);
    }

    /// The active card's 3D scene, if it has one.
    pub fn scene(&self, alpha: f32, layout: &Layout) -> Option<crate::draw3d::Scene3> {
        self.cards[self.active].scene(alpha, layout)
    }

    /// The background: the board as a grid in world units, the safe area's corners, finger trails.
    fn workbench(&self, layout: &Layout, frame: &mut Frame) {
        let fit = &layout.fit;
        let b = fit.rect;
        let s = fit.scale;
        frame.push(Layer::Board, 0, Shape::Box { rect: b, r: 0.35 * s, color: Color::hex(0x221a52) });
        let line = (s * 0.025).max(1.0);
        for x in 1..BOARD.0 as i32 {
            let major = x % 4 == 0;
            let c = Color::hexa(0xffffff, if major { 0.11 } else { 0.045 });
            let p = fit.px(x as f32, 0.0);
            frame.push(Layer::Board, 1, Shape::Box { rect: Rect::new(p.x - line / 2.0, b.y, line, b.h), r: 0.0, color: c });
        }
        for y in 1..BOARD.1 as i32 {
            let major = y % 4 == 0;
            let c = Color::hexa(0xffffff, if major { 0.11 } else { 0.045 });
            let p = fit.px(0.0, y as f32);
            frame.push(Layer::Board, 1, Shape::Box { rect: Rect::new(b.x, p.y - line / 2.0, b.w, line), r: 0.0, color: c });
        }
        // The safe area's corners: where no notch, island or home bar covers the screen.
        let sa = layout.safe;
        let (len, th) = (18.0 * layout.px / 2.2, line * 1.5);
        let mark = Color::hexa(0x8f7cff, 0.55);
        for (x, y, dx, dy) in
            [(sa.x, sa.y, 1.0, 1.0), (sa.x + sa.w, sa.y, -1.0, 1.0), (sa.x, sa.y + sa.h, 1.0, -1.0), (sa.x + sa.w, sa.y + sa.h, -1.0, -1.0)]
        {
            let hx = if dx > 0.0 { x } else { x - len };
            let vy = if dy > 0.0 { y } else { y - len };
            frame.push(Layer::Fx, 0, Shape::Box { rect: Rect::new(hx, if dy > 0.0 { y } else { y - th }, len, th), r: 0.0, color: mark });
            frame.push(Layer::Fx, 0, Shape::Box { rect: Rect::new(if dx > 0.0 { x } else { x - th }, vy, th, len), r: 0.0, color: mark });
        }
        // Finger trails, fading with age.
        for t in self.trails.values() {
            for w in t.windows(2) {
                let fade = 1.0 - w[1].1 / TRAIL_MS;
                frame.push(
                    Layer::Fx,
                    1,
                    Shape::Capsule {
                        a: w[0].0,
                        b: w[1].0,
                        r: 3.0 * layout.px / 2.2 * fade + 0.5,
                        color: Color::hexa(0x9be7ff, 0.45 * fade),
                    },
                );
            }
        }
    }

    /// The header (title, fps) and the tabs.
    fn chrome(&self, layout: &Layout, frame: &mut Frame) {
        let px = layout.px;
        let h = layout.header;
        let white = Color::hex(0xf2efff);
        let dim = Color::hexa(0xf2efff, 0.55);
        font::text(frame, Layer::Hud, 0, Px::new(h.x, h.y + (h.h - font::height(px)) / 2.0), px, white, "PLAYGROUND");
        let fps = format!("{:.0} FPS", self.fps);
        let fw = font::width(&fps, px);
        font::text(frame, Layer::Hud, 0, Px::new(h.x + h.w - fw, h.y + (h.h - font::height(px)) / 2.0), px, dim, &fps);
        let t = layout.tabs;
        frame.push(Layer::Hud, 0, Shape::Box { rect: t, r: t.h / 2.0, color: Color::hexa(0x0e0a26, 0.85) });
        let n = self.cards.len();
        let w = t.w / n as f32;
        for (i, c) in self.cards.iter().enumerate() {
            let cell = Rect::new(t.x + i as f32 * w, t.y, w, t.h);
            let inner = Rect::new(cell.x + t.h * 0.1, cell.y + t.h * 0.1, cell.w - t.h * 0.2, cell.h - t.h * 0.2);
            let (bg, fg) = if i == self.active { (Color::hex(0xffc93c), Color::hex(0x1b1440)) } else { (Color::hexa(0xffffff, 0.0), dim) };
            if i == self.active {
                frame.push(Layer::Hud, 1, Shape::Box { rect: inner, r: inner.h / 2.0, color: bg });
            }
            font::centered(frame, Layer::Hud, 2, inner.center(), px * 0.9, fg, c.name());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> Layout {
        Layout::new(1179.0, 2556.0, 3.0)
    }

    #[test]
    fn the_board_sits_between_header_and_tabs_inside_the_safe_area() {
        let l = layout();
        let b = l.fit.rect;
        assert!(b.y >= l.header.y + l.header.h, "board under the header");
        assert!(b.y + b.h <= l.tabs.y, "board above the tabs");
        assert!(l.tabs.y + l.tabs.h <= l.safe.y + l.safe.h, "tabs above the home bar");
        assert!(l.header.y >= l.safe.y, "header below the island");
    }

    #[test]
    fn a_tap_on_a_tab_switches_the_card_and_never_reaches_it() {
        let l = layout();
        let mut p = Playground::new();
        p.active = 1;
        let mut out = Vec::new();
        let n = p.cards.len() as f32;
        let fourth = Px::new(l.tabs.x + l.tabs.w * 4.5 / n, l.tabs.y + l.tabs.h / 2.0);
        p.touch(1, Phase::Down, fourth, &l, &mut out);
        assert_eq!(p.active, 4);
        assert_eq!(p.card().name(), "FEEL");
        assert_eq!(out.len(), 1, "a tick for the switch");
        // The tap's gestures are swallowed until it lifts.
        p.input(&Gesture::Down(fourth), &l, &mut out);
        p.input(&Gesture::Release { at: fourth, velocity: Px::default() }, &l, &mut out);
        assert!(!p.on_tabs);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn a_fullscreen_game_has_no_tabs_and_its_lab_button_leads_back() {
        let l = layout();
        let mut p = Playground::new();
        assert!(p.card().fullscreen(), "the app opens on SMASH");
        let mut out = Vec::new();
        // Where a tab would be is the game's: a pull starts there.
        let tab_spot = Px::new(l.tabs.x + l.tabs.w * 0.5, l.tabs.y + l.tabs.h / 2.0);
        p.touch(1, Phase::Down, tab_spot, &l, &mut out);
        assert_eq!(p.active, 0);
        p.input(&Gesture::Release { at: tab_spot, velocity: Px::default() }, &l, &mut out);
        let mut g = crate::smash::Smash::default();
        g.layout(&l);
        let lab = crate::smash::look::buttons(&g)[1].rect.center();
        p.input(&Gesture::Down(lab), &l, &mut out);
        assert_eq!(p.active, 1, "LAB goes to the playground's cards");
        assert!(!p.card().fullscreen());
    }

    #[test]
    fn trails_fade_and_are_dropped() {
        let l = layout();
        let mut p = Playground::new();
        let mut out = Vec::new();
        let at = l.fit.px(4.0, 8.0);
        p.touch(7, Phase::Down, at, &l, &mut out);
        p.touch(7, Phase::Move, Px::new(at.x + 30.0, at.y), &l, &mut out);
        assert_eq!(p.trails[&7].len(), 2);
        p.age(TRAIL_MS + 1.0);
        assert!(p.trails.is_empty());
    }

    #[test]
    fn every_card_draws_and_reports() {
        let l = layout();
        let mut p = Playground::new();
        for i in 0..p.cards.len() {
            p.active = i;
            let mut out = Vec::new();
            p.step(&Sense { gravity: Some([300, -950, 0]) }, &mut out);
            let mut f = Frame::default();
            p.draw(0.5, &l, &mut f);
            assert!(f.sorted().len() > 20, "card {} draws", p.card().name());
            assert!(p.observe().starts_with("\"card\":"));
        }
    }
}
