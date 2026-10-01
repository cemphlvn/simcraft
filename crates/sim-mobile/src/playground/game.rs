//! GAME: a bundled simcraft game, played on the phone by the real engine (the same `game.ron` the evals run).
//! Built for grid puzzles whose player places and detonates (a chain-reaction puzzle): tap an
//! open cell to place a bomb, tap a placed bomb to take it back, tap the starter to set the chain off. A clear
//! board goes to the next level, a failed chain retries the same one; tap to continue.
//!
//! The game decides everything (its `place`, `pickup` and `detonate` actions, its rules, its `end`); this card only
//! turns taps into those actions, paces the ticks so a wave can be watched, and draws the grid.

use std::collections::BTreeMap;

use sim_core::{Engine, EntityId, Loaded, Running};
use sim_rules::Game;

use super::Card;
use crate::font;
use crate::games::{self, Level};
use crate::gesture::{Gesture, Px};
use crate::haptics::{Kind, Pulse};
use crate::layer::{Color, Fit, Frame, Layer, Rect, Shape, fx_f32};
use crate::sensors::Sense;

/// Playground ticks (60 a second) per game tick while a chain runs: a wave (two game ticks) takes 0.2 s.
const PERIOD: u32 = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Placing,
    Chain,
    Won,
    Lost,
}

struct Play {
    engine: Engine<Running, Game>,
    hand: EntityId,
    starter: EntityId,
    phase: Phase,
    /// Playground ticks until the next game tick.
    wait: u32,
    /// Where every entity was before the last game tick (for sliding between ticks).
    prev: BTreeMap<EntityId, (i64, i64)>,
    /// Explosions being drawn: cell and age in playground ticks.
    flashes: Vec<(i64, i64, u32)>,
    chain_ticks: u32,
}

pub struct GameCard {
    levels: Vec<Level>,
    index: usize,
    play: Option<Play>,
    error: Option<String>,
}

impl Default for GameCard {
    fn default() -> GameCard {
        GameCard::new()
    }
}

impl GameCard {
    pub fn new() -> GameCard {
        let levels = games::root().map(|d| games::levels(&d)).unwrap_or_default();
        let mut card = GameCard { levels, index: 0, play: None, error: None };
        card.load();
        card
    }

    fn load(&mut self) {
        self.play = None;
        let Some(level) = self.levels.get(self.index) else { return };
        match start(level) {
            Ok(p) => {
                self.play = Some(p);
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }

    /// The cell size and the grid's top-left corner, in board units (the board is 9 × 16, y up).
    fn geometry(&self) -> Option<(f32, f32, f32, i64, i64)> {
        let p = self.play.as_ref()?;
        let w = p.engine.world();
        // The level without its border (one cell around it, the hand's row on top).
        let (lw, lh) = (w.width - 2, w.height - 2);
        let cell = (8.6 / lw as f32).min(12.0 / lh as f32);
        let left = (9.0 - cell * lw as f32) / 2.0;
        let top = 8.0 + cell * lh as f32 / 2.0;
        Some((cell, left, top, lw, lh))
    }

    fn cell_at(&self, fit: &Fit, at: Px) -> Option<(i64, i64)> {
        let (cell, left, top, lw, lh) = self.geometry()?;
        let w = fit.world(at);
        let (bx, by) = (fx_f32(w.x), fx_f32(w.y));
        let cx = ((bx - left) / cell).floor() as i64 + 1;
        let cy = ((top - by) / cell).floor() as i64 + 1;
        (cx >= 1 && cx <= lw && cy >= 1 && cy <= lh).then_some((cx, cy))
    }

    fn act(&mut self, id: EntityId, action: &str, args: &BTreeMap<String, i64>) -> bool {
        let Some(p) = self.play.as_mut() else { return false };
        match p.engine.rules().act(p.engine.world(), None, id, action, args) {
            Ok(group) => {
                p.engine.queue(group);
                true
            }
            Err(_) => false,
        }
    }
}

fn start(level: &Level) -> Result<Play, String> {
    let (world, game) = Game::from_strs(&level.game_ron, &level.engine_toml)?;
    let mut engine = Engine::<Loaded, _>::new(world, game).validate().map_err(|e| e.join("; "))?.start();
    engine.hash_every_tick(false);
    let ents = engine.world().entities();
    let hand = ents.values().find(|e| e.kind == "hand").map(|e| e.id).ok_or("the level has no hand")?;
    let starter = ents
        .values()
        .find(|e| e.kind == "bomb" && e.props.get("starter") == Some(&1))
        .map(|e| e.id)
        .ok_or("the level has no starter bomb")?;
    Ok(Play { engine, hand, starter, phase: Phase::Placing, wait: 0, prev: BTreeMap::new(), flashes: Vec::new(), chain_ticks: 0 })
}

impl Play {
    /// One game tick; what happened becomes pulses and flashes.
    fn tick(&mut self, out: &mut Vec<Pulse>) {
        self.prev = self.engine.world().entities().values().map(|e| (e.id, (e.x, e.y))).collect();
        let report = self.engine.tick();
        for e in &report.events {
            match e.name.as_str() {
                "boom" => {
                    if let Some(&(x, y)) = self.prev.get(&e.entity) {
                        self.flashes.push((x, y, 0));
                    }
                    out.push(Pulse::new(Kind::Thud, 0.75, 0.35));
                }
                "brick_broken" => out.push(Pulse::new(Kind::Tick, 0.55, 0.9)),
                _ => {}
            }
        }
        if self.phase == Phase::Chain {
            self.chain_ticks += 1;
            if report.outcome.as_deref() == Some("win") {
                self.phase = Phase::Won;
                out.push(Pulse::new(Kind::Rise, 0.8, 0.6));
            } else if self.chain_ticks > 1
                && !self.engine.world().entities().values().any(|e| e.kind == "bomb" && (e.state == "Lit" || e.state == "Boom"))
            {
                self.phase = Phase::Lost;
                out.push(Pulse::new(Kind::Fall, 0.6, 0.3));
            }
        }
    }

    fn budget(&self) -> i64 {
        self.engine.world().get(self.hand).and_then(|h| h.props.get("budget").copied()).unwrap_or(0)
    }
}

impl Card for GameCard {
    fn name(&self) -> &'static str {
        "GAME"
    }

    fn input(&mut self, g: &Gesture, fit: &Fit, out: &mut Vec<Pulse>) {
        let Gesture::Tap(at) = *g else { return };
        let Some(phase) = self.play.as_ref().map(|p| p.phase) else { return };
        match phase {
            Phase::Won => {
                self.index = (self.index + 1) % self.levels.len().max(1);
                self.load();
                out.push(Pulse::new(Kind::Tap, 0.5, 0.6));
            }
            Phase::Lost => {
                self.load();
                out.push(Pulse::new(Kind::Tap, 0.5, 0.6));
            }
            Phase::Chain => {}
            Phase::Placing => {
                let Some((cx, cy)) = self.cell_at(fit, at) else { return };
                let p = self.play.as_ref().expect("a level");
                let here = p
                    .engine
                    .world()
                    .entities()
                    .values()
                    .find(|e| e.x == cx && e.y == cy && e.kind != "edge")
                    .map(|e| (e.id, e.kind.clone()));
                let (hand, starter) = (p.hand, p.starter);
                let ok = match here {
                    Some((id, _)) if id == starter => {
                        let ok = self.act(starter, "detonate", &BTreeMap::new());
                        if ok && let Some(p) = self.play.as_mut() {
                            p.phase = Phase::Chain;
                            p.wait = 0;
                            out.push(Pulse::new(Kind::Thud, 0.95, 0.3));
                        }
                        return;
                    }
                    Some((id, kind)) if kind == "bomb" => self.act(id, "pickup", &BTreeMap::new()),
                    Some(_) => false,
                    None => self.act(hand, "place", &BTreeMap::from([("x".to_string(), cx), ("y".to_string(), cy)])),
                };
                if ok {
                    if let Some(p) = self.play.as_mut() {
                        p.tick(out);
                    }
                    out.push(Pulse::new(Kind::Tap, 0.6, 0.7));
                }
            }
        }
    }

    fn step(&mut self, _sense: &Sense, out: &mut Vec<Pulse>) {
        let Some(p) = self.play.as_mut() else { return };
        for f in &mut p.flashes {
            f.2 += 1;
        }
        p.flashes.retain(|f| f.2 < 24);
        if p.phase == Phase::Chain {
            if p.wait == 0 {
                p.tick(out);
                p.wait = PERIOD;
            }
            p.wait -= 1;
        }
    }

    fn draw(&self, _alpha: f32, fit: &Fit, frame: &mut Frame) {
        let s = fit.scale;
        let b = fit.rect;
        let px = s * 0.05;
        let dim = Color::hexa(0xf2efff, 0.65);
        let Some(p) = self.play.as_ref() else {
            let msg = self.error.clone().unwrap_or_else(|| "NO GAMES BUNDLED".into());
            font::centered(frame, Layer::Fx, 2, b.center(), px, dim, &msg.to_uppercase());
            return;
        };
        let Some((cell, left, top, lw, lh)) = self.geometry() else { return };
        let corner = |cx: f32, cy: f32| fit.px(left + (cx - 1.0) * cell, top - (cy - 1.0) * cell);
        let tl = corner(1.0, 1.0);
        let area = Rect::new(tl.x, tl.y, lw as f32 * cell * s, lh as f32 * cell * s);
        frame.push(Layer::Board, 2, Shape::Box { rect: area, r: 0.2 * s, color: Color::hex(0x2c2370) });
        // Sliding between game ticks.
        let t = if p.phase == Phase::Chain { 1.0 - p.wait as f32 / PERIOD as f32 } else { 1.0 };
        let (c, gap) = (cell * s, cell * s * 0.06);
        for e in p.engine.world().entities().values() {
            let (x, y) = match p.prev.get(&e.id) {
                Some(&(px0, py0)) if p.phase == Phase::Chain => (px0 as f32 + (e.x - px0) as f32 * t, py0 as f32 + (e.y - py0) as f32 * t),
                _ => (e.x as f32, e.y as f32),
            };
            let at = corner(x, y);
            let rect = Rect::new(at.x + gap, at.y + gap, c - 2.0 * gap, c - 2.0 * gap);
            match e.kind.as_str() {
                "wall" => frame.push(Layer::Pieces, 0, Shape::Box { rect, r: c * 0.12, color: Color::hex(0x463b9c) }),
                "brick" => {
                    frame.push(Layer::Pieces, 0, Shape::Box { rect, r: c * 0.14, color: Color::hex(0xff8a3d) });
                    let hi = Rect::new(rect.x + c * 0.1, rect.y + c * 0.08, rect.w - c * 0.2, c * 0.14);
                    frame.push(Layer::Pieces, 1, Shape::Box { rect: hi, r: c * 0.07, color: Color::hexa(0xffd2a8, 0.7) });
                }
                "bomb" => {
                    let centre = rect.center();
                    let (color, r) = match (e.id == p.starter, e.state.as_str()) {
                        (_, "Lit") => (Color::hex(0xff4f7b), c * 0.36),
                        (_, "Boom") => (Color::hex(0xffffff), c * 0.42),
                        (true, _) => (Color::hex(0xffc93c), c * 0.36),
                        _ => (Color::hex(0x3cc8ff), c * 0.33),
                    };
                    frame.push(Layer::Pieces, 2, Shape::circle(centre, r, color));
                    frame.push(
                        Layer::Pieces,
                        3,
                        Shape::circle(Px::new(centre.x - r * 0.3, centre.y - r * 0.35), r * 0.28, Color::hexa(0xffffff, 0.6)),
                    );
                    if e.id == p.starter && p.phase == Phase::Placing {
                        frame.push(Layer::Pieces, 1, Shape::circle(centre, r * 1.35, Color::hexa(0xffc93c, 0.25)));
                    }
                }
                _ => {}
            }
        }
        for &(x, y, age) in &p.flashes {
            let k = age as f32 / 24.0;
            let centre = Rect::new(corner(x as f32, y as f32).x, corner(x as f32, y as f32).y, c, c).center();
            frame.push(Layer::Fx, 1, Shape::circle(centre, c * (0.5 + 1.2 * k), Color::hexa(0xffe08a, 0.55 * (1.0 - k))));
        }
        // What the player needs: which level, bombs left, what a tap does now.
        let name = self.levels.get(self.index).map_or("", |l| l.name.as_str());
        let head = format!("{}  {}/{}", name.rsplit('/').next().unwrap_or(name), self.index + 1, self.levels.len());
        font::text(frame, Layer::Fx, 2, Px::new(b.x + 0.4 * s, b.y + 0.4 * s), px, dim, &head.to_uppercase());
        let (line1, line2) = match p.phase {
            Phase::Placing => (format!("BOMBS {}", p.budget()), "TAP YELLOW TO FIRE"),
            Phase::Chain => ("CHAIN".to_string(), ""),
            Phase::Won => ("CLEAR".to_string(), "TAP FOR NEXT LEVEL"),
            Phase::Lost => ("BRICKS LEFT".to_string(), "TAP TO RETRY"),
        };
        let color = match p.phase {
            Phase::Won => Color::hex(0xffc93c),
            Phase::Lost => Color::hex(0xff4f7b),
            _ => Color::hex(0xf2efff),
        };
        let mid = b.x + b.w / 2.0;
        font::centered(frame, Layer::Fx, 2, Px::new(mid, b.y + b.h - 2.2 * s), px * 1.4, color, &line1);
        font::centered(frame, Layer::Fx, 2, Px::new(mid, b.y + b.h - 1.2 * s), px, dim, line2);
    }

    fn observe(&self) -> String {
        let Some(p) = self.play.as_ref() else { return "\"level\":null".into() };
        let bricks = p.engine.world().entities().values().filter(|e| e.kind == "brick").count();
        let name = self.levels.get(self.index).map_or("", |l| l.name.as_str());
        format!("\"level\":\"{name}\",\"phase\":\"{:?}\",\"bombs\":{},\"bricks\":{bricks}", p.phase, p.budget())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GAME: &str = include_str!("../../tests/data/chain/game.ron");
    const ENGINE: &str = include_str!("../../tests/data/chain/engine.toml");

    fn card() -> GameCard {
        let mut c = GameCard {
            levels: vec![Level { name: "t/one".into(), game_ron: GAME.into(), engine_toml: ENGINE.into() }],
            index: 0,
            play: None,
            error: None,
        };
        c.load();
        assert!(c.error.is_none(), "{:?}", c.error);
        c
    }

    fn fit() -> Fit {
        Fit::new(9.0, 16.0, Rect::new(0.0, 0.0, 900.0, 1600.0))
    }

    /// The centre of a cell on the screen.
    fn tap(c: &GameCard, cx: i64, cy: i64) -> Gesture {
        let (cell, left, top, _, _) = c.geometry().unwrap();
        Gesture::Tap(fit().px(left + (cx as f32 - 0.5) * cell, top - (cy as f32 - 0.5) * cell))
    }

    #[test]
    fn taps_place_take_back_and_detonate_through_the_game_actions() {
        let mut c = card();
        let mut out = Vec::new();
        let budget = c.play.as_ref().unwrap().budget();
        c.input(&tap(&c, 3, 1), &fit(), &mut out);
        assert_eq!(c.play.as_ref().unwrap().budget(), budget - 1, "placed");
        c.input(&tap(&c, 3, 1), &fit(), &mut out);
        assert_eq!(c.play.as_ref().unwrap().budget(), budget, "taken back");
        let (sx, sy) = {
            let p = c.play.as_ref().unwrap();
            let e = p.engine.world().get(p.starter).unwrap();
            (e.x, e.y)
        };
        c.input(&tap(&c, sx, sy), &fit(), &mut out);
        assert_eq!(c.play.as_ref().unwrap().phase, Phase::Chain);
        for _ in 0..600 {
            c.step(&Sense::default(), &mut out);
        }
        // The starter alone cannot clear this level: the chain ends with bricks left.
        assert_eq!(c.play.as_ref().unwrap().phase, Phase::Lost);
        assert!(out.iter().any(|p| p.kind == Kind::Thud));
    }

    #[test]
    fn the_known_solution_wins() {
        let mut c = card();
        let mut out = Vec::new();
        // A bomb next to the starter carries the chain to the brick.
        c.input(&tap(&c, 2, 1), &fit(), &mut out);
        let (sx, sy) = {
            let p = c.play.as_ref().unwrap();
            let e = p.engine.world().get(p.starter).unwrap();
            (e.x, e.y)
        };
        c.input(&tap(&c, sx, sy), &fit(), &mut out);
        for _ in 0..600 {
            c.step(&Sense::default(), &mut out);
        }
        assert_eq!(c.play.as_ref().unwrap().phase, Phase::Won);
    }
}
