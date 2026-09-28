//! 2.5D: one frame per world level, stacked by perspective states, rendered lazily.
//!
//! A perspective orders the layers (back to front), shifts each by `step` per place in the stack, fades all but the
//! focus layer. Layer frames are cached and re-rendered only when their level's content changed; an unchanged frame
//! reuses the last composite. Nothing here runs when the component is not on screen.

use std::cell::RefCell;

use serde::Deserialize;

use crate::canvas::{Cell, Rect};
use crate::component::Ctx;
use crate::scene::Scene;
use crate::style::Style;

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Perspective {
    pub name: String,
    /// Levels back to front. Unlisted levels are not drawn.
    pub order: Vec<i64>,
    /// The layer in focus (opaque, tinted). -1 (default): the front layer.
    #[serde(default = "minus_one")]
    pub focus: i64,
    /// Screen shift per place in the stack (cells): (2, -1) climbs up and to the right.
    #[serde(default = "step")]
    pub step: (i16, i16),
    /// Brightness of layers out of focus, %.
    #[serde(default = "fade")]
    pub fade: u32,
    /// Perspective to go to on click. Empty (default): the next one.
    #[serde(default)]
    pub click: String,
}

fn minus_one() -> i64 {
    -1
}

fn step() -> (i16, i16) {
    (2, -1)
}
fn fade() -> u32 {
    45
}

/// One cached layer: what its level looked like when rendered, and the frame.
#[derive(Default)]
struct LayerFrame {
    key: Option<u64>,
    cells: Vec<Option<Cell>>,
}

#[derive(Default)]
struct Cache {
    layers: Vec<LayerFrame>,
    /// (tick, perspective, rect) of the last composite, and the composite itself.
    composite_key: Option<(u64, usize, Rect)>,
    composite: Vec<(u16, u16, Cell)>,
}

/// What the last draw did (shown in the panel title, read by tests).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub redrawn: usize,
    pub visible: usize,
    pub reused: bool,
}

#[derive(Debug)]
pub struct Layered {
    pub perspectives: Vec<Perspective>,
    pub current: usize,
    pub tint: Option<String>,
    cache: RefCell<Cache>,
    stats: RefCell<Stats>,
}

impl std::fmt::Debug for Cache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Cache({} layers)", self.layers.len())
    }
}

impl Clone for Layered {
    fn clone(&self) -> Self {
        Layered::new(self.perspectives.clone(), self.tint.clone())
    }
}

impl PartialEq for Layered {
    fn eq(&self, o: &Self) -> bool {
        self.perspectives == o.perspectives && self.current == o.current && self.tint == o.tint
    }
}

impl Layered {
    pub fn new(perspectives: Vec<Perspective>, tint: Option<String>) -> Layered {
        Layered { perspectives, current: 0, tint, cache: RefCell::default(), stats: RefCell::default() }
    }

    /// Without perspectives: every level, top level in front.
    pub fn all_levels(depth: i64) -> Perspective {
        Perspective {
            name: "all levels".into(),
            order: (0..depth).rev().collect(),
            focus: -1,
            step: step(),
            fade: 70,
            click: String::new(),
        }
    }

    pub fn perspective(&self) -> &Perspective {
        &self.perspectives[self.current.min(self.perspectives.len() - 1)]
    }

    pub fn stats(&self) -> Stats {
        *self.stats.borrow()
    }

    /// The click transition: the named state, else the next one.
    pub fn click(&mut self) {
        let target = &self.perspective().click;
        let next = self.perspectives.iter().position(|p| !target.is_empty() && &p.name == target);
        self.current = next.unwrap_or((self.current + 1) % self.perspectives.len());
    }

    /// Checks names and levels (at load).
    pub fn check(&self, depth: i64) -> Result<(), String> {
        if self.perspectives.is_empty() {
            return Err("2.5D needs at least one perspective".into());
        }
        for p in &self.perspectives {
            if let Some(z) = p.order.iter().find(|z| !(0..depth).contains(*z)) {
                return Err(format!("perspective '{}': level {z} does not exist (the world has {depth})", p.name));
            }
            if p.focus >= 0 && !p.order.contains(&p.focus) {
                return Err(format!("perspective '{}': focus {} is not in its order", p.name, p.focus));
            }
            if !p.click.is_empty() && !self.perspectives.iter().any(|q| q.name == p.click) {
                return Err(format!("perspective '{}': click goes to unknown perspective '{}'", p.name, p.click));
            }
        }
        Ok(())
    }

    pub fn title(&self) -> String {
        let s = self.stats();
        let work = if s.reused { "reused".to_string() } else { format!("{}/{} layers redrawn", s.redrawn, s.visible) };
        format!("world · 2.5D · {} · click to switch · {work}", self.perspective().name)
    }

    pub fn draw(&self, ctx: &mut Ctx, r: Rect) {
        let scene = ctx.scene;
        let (w, h, d) = (scene.world.width, scene.world.height, scene.world.depth);
        let p = self.perspective().clone();
        let focus = if p.focus >= 0 { p.focus } else { *p.order.last().unwrap_or(&0) };
        let mut cache = self.cache.borrow_mut();

        let key = (scene.world.tick, self.current, r);
        if cache.composite_key == Some(key) {
            for &(x, y, cell) in &cache.composite {
                ctx.canvas.put(x, y, cell);
            }
            *self.stats.borrow_mut() = Stats { redrawn: 0, visible: p.order.len(), reused: true };
            return;
        }

        if cache.layers.len() != d as usize {
            cache.layers = (0..d).map(|_| LayerFrame::default()).collect();
        }
        let mut redrawn = 0;
        for &z in &p.order {
            let tint = (z == focus).then_some(self.tint.as_deref()).flatten();
            let k = level_key(scene, z, tint, z == focus);
            let frame = &mut cache.layers[z as usize];
            if frame.key != Some(k) {
                frame.cells = render_level(scene, &ctx.style, z, tint, z == focus);
                frame.key = Some(k);
                redrawn += 1;
            }
        }

        // Composite back to front; centre the stack in the panel.
        let n = p.order.len() as i64;
        let (sx, sy) = (p.step.0 as i64, p.step.1 as i64);
        let span_x = w + (sx.abs() * (n - 1));
        let span_y = h + (sy.abs() * (n - 1));
        let ox = r.x as i64 + ((r.w as i64 - span_x) / 2).max(0) + if sx < 0 { -sx * (n - 1) } else { 0 };
        let oy = r.y as i64 + ((r.h as i64 - span_y) / 2).max(0) + if sy < 0 { -sy * (n - 1) } else { 0 };
        let mut out = Vec::new();
        for (i, &z) in p.order.iter().enumerate() {
            let frame = &cache.layers[z as usize];
            let (lx, ly) = (ox + sx * i as i64, oy + sy * i as i64);
            for y in 0..h {
                for x in 0..w {
                    let Some(mut cell) = frame.cells[(y * w + x) as usize] else { continue };
                    let (cx, cy) = (lx + x, ly + y);
                    if cx < r.x as i64 || cy < r.y as i64 || cx >= (r.x + r.w) as i64 || cy >= (r.y + r.h) as i64 {
                        continue;
                    }
                    if z != focus {
                        cell.fg = cell.fg.shade(p.fade);
                        cell.bg = cell.bg.shade(p.fade);
                    }
                    ctx.canvas.put(cx as u16, cy as u16, cell);
                    out.push((cx as u16, cy as u16, cell));
                }
            }
        }
        cache.composite = out;
        cache.composite_key = Some(key);
        *self.stats.borrow_mut() = Stats { redrawn, visible: p.order.len(), reused: false };
    }
}

/// A fingerprint of everything a level's frame shows: visible entities (id, place, glyph), terrain, the tint field.
fn level_key(scene: &Scene, z: i64, tint: Option<&str>, focus: bool) -> u64 {
    let w = scene.world;
    let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ (focus as u64);
    let mut mix = |v: u64| h = (h ^ v).wrapping_mul(0x0100_0000_01b3);
    for e in w.entities().values().filter(|e| e.z == z && scene.visible(e)) {
        mix(e.id);
        mix(e.x as u64);
        mix(e.y as u64);
        mix(scene.glyph(e) as u64);
    }
    for y in 0..w.height {
        for x in 0..w.width {
            mix(w.is_terrain(x, y, z) as u64);
            if let Some(v) = tint.and_then(|t| w.field(t, x, y, z)) {
                mix(v as u64);
            }
        }
    }
    h
}

/// One level as a frame. The focus layer and the top level are opaque; other layers show only their open spaces
/// (tunnels, chambers) and entities, so the soil of one layer never hides the layers behind it.
fn render_level(scene: &Scene, style: &Style, z: i64, tint: Option<&str>, focus: bool) -> Vec<Option<Cell>> {
    let w = scene.world;
    let mut out = Vec::with_capacity((w.width * w.height) as usize);
    for y in 0..w.height {
        for x in 0..w.width {
            let terrain = scene.terrain(x, y, z);
            let cell = match scene.top(x, y, z) {
                Some(e) => Some(Cell { ch: scene.glyph(e), fg: scene.color(e), bg: scene.ground(style, x, y, z, tint) }),
                None if terrain && focus => Some(Cell { ch: '░', fg: style.color("soil_glyph"), bg: scene.ground(style, x, y, z, tint) }),
                None if terrain => None,
                None if z == 0 || focus => Some(Cell { ch: ' ', fg: style.color("dim"), bg: scene.ground(style, x, y, z, tint) }),
                // A tunnel seen through the soil: a faint outline.
                None => Some(Cell { ch: '·', fg: style.color("soil_glyph"), bg: style.color("air") }),
            };
            out.push(cell);
        }
    }
    out
}
