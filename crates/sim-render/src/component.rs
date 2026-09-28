//! Components: reusable widgets, registered by name, configured by props, styled by the theme.
//!
//! A component draws into a rectangle through a `Ctx` (canvas, scene, the node's style, UI state). Props come from
//! view.ron as a RON value and are read into the component's own props struct.

use std::collections::{BTreeMap, VecDeque};

use serde::Deserialize;
use serde::de::DeserializeOwned;
use sim_core::EntityId;

use crate::canvas::{Canvas, Cell, Rect, Rgb};
use crate::projection::Projection;
use crate::scene::Scene;
use crate::style::Style;

/// Everything a frame shows beyond the world itself.
pub struct Ui {
    pub tick: u64,
    pub speed: f32,
    pub paused: bool,
    pub outcome: Option<String>,
    pub selected: Option<EntityId>,
    pub history: BTreeMap<String, VecDeque<i64>>,
    pub events: VecDeque<String>,
    pub fps: f32,
    /// Live projections of the view's `World` components (keys orbit and change level).
    pub worlds: Vec<Projection>,
    /// Where each `World` was drawn last frame (slot, rect): clicks are routed by it.
    pub hits: std::cell::RefCell<Vec<(usize, Rect)>>,
    /// True pixels (kitty graphics protocol) instead of half-blocks.
    pub graphics: bool,
    /// Size of one terminal cell in screen pixels (for square pixels).
    pub cell_px: (u16, u16),
    /// Frames drawn so far (animation clock, independent of the simulation).
    pub frame: u64,
    /// Pixel images to show this frame: (where, picture). The host sends them after the cells.
    pub images: std::cell::RefCell<Vec<(Rect, crate::pixel::Pixmap)>>,
    /// Which way each entity faces (last x, facing left).
    pub facing: std::cell::RefCell<std::collections::BTreeMap<EntityId, (i64, bool)>>,
    /// Cooked diorama parts: backdrop strips and the world's section, with the key they were cooked for.
    pub strips: std::cell::RefCell<Option<(u64, Vec<crate::display::Strip>)>>,
    pub section: std::cell::RefCell<Option<(u64, crate::image::Image)>>,
}

impl Ui {
    pub fn new(worlds: Vec<Projection>) -> Ui {
        Ui {
            tick: 0,
            speed: 10.0,
            paused: false,
            outcome: None,
            selected: None,
            history: BTreeMap::new(),
            events: VecDeque::new(),
            fps: 0.0,
            worlds,
            hits: Default::default(),
            graphics: false,
            cell_px: (8, 16),
            frame: 0,
            images: Default::default(),
            facing: Default::default(),
            strips: Default::default(),
            section: Default::default(),
        }
    }

    /// The `World` slot under a screen cell.
    pub fn hit(&self, x: u16, y: u16) -> Option<usize> {
        self.hits.borrow().iter().rev().find(|(_, r)| x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h).map(|(s, _)| *s)
    }
}

pub struct Ctx<'a, 'c> {
    pub canvas: &'c mut Canvas,
    pub scene: &'c Scene<'a>,
    pub style: Style,
    pub ui: &'c Ui,
}

impl Ctx<'_, '_> {
    pub fn text(&mut self, r: Rect, dx: u16, dy: u16, s: &str, token: &str) {
        let c = self.style.color(token);
        self.canvas.text(r, dx, dy, s, c);
    }

    /// A titled border in the theme's style; returns the inside.
    pub fn panel(&mut self, r: Rect, title: &str) -> Rect {
        let Some([hz, vt, tl, tr, bl, br]) = self.style.border.chars() else { return r };
        if r.w < 2 || r.h < 2 {
            return Rect::new(r.x, r.y, 0, 0);
        }
        let line = self.style.color("border");
        let c = &mut *self.canvas;
        for x in r.x + 1..r.x + r.w - 1 {
            c.glyph(x, r.y, hz, line);
            c.glyph(x, r.y + r.h - 1, hz, line);
        }
        for y in r.y + 1..r.y + r.h - 1 {
            c.glyph(r.x, y, vt, line);
            c.glyph(r.x + r.w - 1, y, vt, line);
        }
        c.glyph(r.x, r.y, tl, line);
        c.glyph(r.x + r.w - 1, r.y, tr, line);
        c.glyph(r.x, r.y + r.h - 1, bl, line);
        c.glyph(r.x + r.w - 1, r.y + r.h - 1, br, line);
        self.text(r, 2, 0, &format!(" {title} "), "accent");
        r.inner(1)
    }
}

/// A reusable widget.
pub trait Component: Send + Sync {
    /// `props` is the node's RON value (Unit when none). `slot` is the node's index among `World` components.
    fn draw(&self, ctx: &mut Ctx, props: &ron::Value, slot: Option<usize>, r: Rect) -> Result<(), String>;
}

/// Reads a component's props (missing props → the struct's defaults).
pub fn props<T: DeserializeOwned + Default>(v: &ron::Value) -> Result<T, String> {
    match v {
        ron::Value::Unit => Ok(T::default()),
        v => v.clone().into_rust().map_err(|e| e.to_string()),
    }
}

/// Components by name. Built-ins are registered; add your own with `register`.
pub struct Registry {
    items: BTreeMap<String, Box<dyn Component>>,
}

impl Default for Registry {
    fn default() -> Self {
        let mut r = Registry { items: BTreeMap::new() };
        r.register("Title", Title);
        r.register("World", World);
        r.register("Env", Env);
        r.register("Inspector", Inspector);
        r.register("Series", Series);
        r.register("Counts", Counts);
        r.register("Legend", Legend);
        r.register("Events", Events);
        r.register("Help", Help);
        r.register("Text", Text);
        r.register("Diorama", crate::diorama::Diorama);
        r
    }
}

impl Registry {
    pub fn register(&mut self, name: &str, c: impl Component + 'static) {
        self.items.insert(name.to_string(), Box::new(c));
    }

    pub fn get(&self, name: &str) -> Option<&dyn Component> {
        self.items.get(name).map(|b| b.as_ref())
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.items.keys().map(String::as_str)
    }
}

// ------------------------------------------------------------------ built-in components

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TitleProps {
    pub text: Option<String>,
}

pub struct Title;
impl Component for Title {
    fn draw(&self, ctx: &mut Ctx, v: &ron::Value, _: Option<usize>, r: Rect) -> Result<(), String> {
        let p: TitleProps = props(v)?;
        let ui = ctx.ui;
        let name = p.text.unwrap_or_else(|| ctx.scene.game.def.name.clone());
        let state = if ui.paused { "PAUSED".to_string() } else { format!("{:.0} ticks/s", ui.speed) };
        let end = ui.outcome.as_ref().map(|o| format!("   END: {o}")).unwrap_or_default();
        let (fg, bg) = (ctx.style.color("title_text"), ctx.style.color("title_bg"));
        ctx.canvas.fill(r, Cell { ch: ' ', fg, bg });
        ctx.canvas.text(r, 1, 0, &format!("{name}   tick {}   [{state}]   {:.0} fps{end}", ui.tick, ui.fps), fg);
        Ok(())
    }
}

/// The world through a projection. Props: `(projection: Dim3((yaw: 30, ...)))`.
pub struct World;
impl Component for World {
    fn draw(&self, ctx: &mut Ctx, _: &ron::Value, slot: Option<usize>, r: Rect) -> Result<(), String> {
        let ui = ctx.ui;
        let i = slot.ok_or("World without a projection")?;
        let p = ui.worlds.get(i).ok_or("World without a projection")?;
        ui.hits.borrow_mut().push((i, r));
        // Draw first, then the title: a 2.5D title reports this frame's work.
        let inner = ctx.panel(r, "");
        p.draw(ctx, inner);
        let title = p.title();
        ctx.panel(r, &title);
        Ok(())
    }
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EnvProps {
    pub title: Option<String>,
    /// Bar width in cells.
    pub bar: Option<usize>,
}

pub struct Env;
impl Component for Env {
    fn draw(&self, ctx: &mut Ctx, v: &ron::Value, _: Option<usize>, r: Rect) -> Result<(), String> {
        let p: EnvProps = props(v)?;
        let inner = ctx.panel(r, p.title.as_deref().unwrap_or("environment"));
        let scene = ctx.scene;
        let mut y = 0;
        for e in scene.world.entities().values().filter(|e| scene.game.is_hidden(&e.kind)) {
            let state = scene.game.state_label(e).rsplit('.').next().unwrap_or("").to_string();
            ctx.text(inner, 0, y, &format!("{}: {state}", e.kind), "text");
            y += 1;
            for (name, v) in &e.props {
                ctx.text(inner, 1, y, &format!("{name:<9} {} {v:>3}", bar(*v, 0, 100, p.bar.unwrap_or(14))), "bar");
                y += 1;
            }
        }
        if y == 0 {
            ctx.text(inner, 0, 0, "(no environment)", "dim");
        }
        Ok(())
    }
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct InspectorProps {
    pub title: Option<String>,
    /// Show what the entity senses (default true).
    pub senses: Option<bool>,
}

pub struct Inspector;
impl Component for Inspector {
    fn draw(&self, ctx: &mut Ctx, v: &ron::Value, _: Option<usize>, r: Rect) -> Result<(), String> {
        let p: InspectorProps = props(v)?;
        let inner = ctx.panel(r, p.title.as_deref().unwrap_or("inspector (tab: next)"));
        let scene = ctx.scene;
        let Some(e) = ctx.ui.selected.and_then(|id| scene.world.get(id)) else {
            ctx.text(inner, 0, 0, "nothing selected", "dim");
            return Ok(());
        };
        let mut lines: Vec<(String, Rgb)> = vec![
            (format!("{} #{}  at ({}, {}, {})", e.kind, e.id, e.x, e.y, e.z), scene.color(e)),
            (format!("state  {}", scene.game.state_label(e)), ctx.style.color("text")),
        ];
        lines.extend(e.props.iter().map(|(k, v)| (format!("  {k:<10} {v}"), ctx.style.color("props"))));
        if p.senses.unwrap_or(true) {
            let senses = scene.game.senses_of(scene.world, e);
            if !senses.is_empty() {
                let c = ctx.style.color("senses");
                lines.push(("senses".to_string(), c));
                lines.extend(senses.into_iter().map(|(k, v)| (format!("  {k:<10} {v}"), c)));
            }
        }
        for (i, (line, col)) in lines.into_iter().take(inner.h as usize).enumerate() {
            ctx.canvas.text(inner, 0, i as u16, &line, col);
        }
        Ok(())
    }
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SeriesProps {
    pub title: Option<String>,
    /// `"nest.food"` (sum of a prop over a kind) or `"count.ant"`.
    pub names: Vec<String>,
}

pub struct Series;
impl Component for Series {
    fn draw(&self, ctx: &mut Ctx, v: &ron::Value, _: Option<usize>, r: Rect) -> Result<(), String> {
        let p: SeriesProps = props(v)?;
        let inner = ctx.panel(r, p.title.as_deref().unwrap_or("trends"));
        for (i, name) in p.names.iter().enumerate() {
            let h = ctx.ui.history.get(name).cloned().unwrap_or_default();
            let now = h.back().copied().unwrap_or(0);
            let trend = now - h.front().copied().unwrap_or(now);
            let (arrow, token) = if trend > 0 { ('▲', "trend_up") } else if trend < 0 { ('▼', "trend_down") } else { (' ', "text") };
            ctx.text(inner, 0, i as u16, &format!("{name:<16}{now:>6}"), "text");
            ctx.text(inner, 23, i as u16, &arrow.to_string(), token);
            ctx.text(inner, 25, i as u16, &spark(&h, inner.w.saturating_sub(26) as usize), "bar");
        }
        Ok(())
    }
}

pub struct Counts;
impl Component for Counts {
    fn draw(&self, ctx: &mut Ctx, v: &ron::Value, _: Option<usize>, r: Rect) -> Result<(), String> {
        let p: TitleProps = props(v)?;
        let inner = ctx.panel(r, p.text.as_deref().unwrap_or("who is doing what"));
        let scene = ctx.scene;
        let mut by: BTreeMap<&str, BTreeMap<String, i64>> = BTreeMap::new();
        for e in scene.world.entities().values().filter(|e| !scene.game.is_hidden(&e.kind)) {
            let leaf = scene.game.state_label(e).split('|').next().unwrap_or("").rsplit('.').next().unwrap_or("").to_string();
            *by.entry(e.kind.as_str()).or_default().entry(leaf).or_default() += 1;
        }
        for (y, (kind, states)) in by.into_iter().enumerate() {
            let total: i64 = states.values().sum();
            let detail: Vec<String> = states.iter().filter(|(s, _)| *s != "-").map(|(s, n)| format!("{s} {n}")).collect();
            let col = scene.assets.kinds.get(kind).and_then(|k| k.color).map_or_else(|| Rgb::of(kind), |(r, g, b)| Rgb(r, g, b));
            ctx.canvas.text(inner, 0, y as u16, &format!("{kind:<8}{total:>4}  {}", detail.join("  ")), col);
        }
        Ok(())
    }
}

/// Glyph → kind and state, from the asset packs and the game.
pub struct Legend;
impl Component for Legend {
    fn draw(&self, ctx: &mut Ctx, _: &ron::Value, _: Option<usize>, r: Rect) -> Result<(), String> {
        let scene = ctx.scene;
        let mut x = 1;
        for (kind, k) in &scene.game.def.kinds {
            if k.hidden {
                continue;
            }
            let pack = scene.assets.kinds.get(kind);
            let base_col = pack.and_then(|p| p.color).map_or_else(|| Rgb::of(kind), |(r, g, b)| Rgb(r, g, b));
            let mut items: Vec<(char, Rgb, String)> = Vec::new();
            for (state, g) in &k.glyphs {
                let look = pack.and_then(|p| p.states.get(state));
                let glyph = look.and_then(|l| l.glyph).unwrap_or(*g);
                let col = look.and_then(|l| l.color).map_or(base_col, |(r, g, b)| Rgb(r, g, b));
                items.push((glyph, col, format!("{kind} {}", state.to_lowercase())));
            }
            let base_glyph = pack.and_then(|p| p.glyph).unwrap_or(k.glyph);
            if !items.iter().any(|(g, ..)| *g == base_glyph) {
                items.push((base_glyph, base_col, kind.clone()));
            }
            for (g, col, label) in items.into_iter().filter(|(g, ..)| !g.is_whitespace()) {
                if x + label.len() as u16 + 4 > r.w {
                    return Ok(());
                }
                ctx.canvas.glyph(r.x + x, r.y, g, col);
                ctx.text(r, x + 2, 0, &label, "dim");
                x += label.len() as u16 + 4;
            }
        }
        Ok(())
    }
}

pub struct Events;
impl Component for Events {
    fn draw(&self, ctx: &mut Ctx, v: &ron::Value, _: Option<usize>, r: Rect) -> Result<(), String> {
        let p: TitleProps = props(v)?;
        let inner = ctx.panel(r, p.text.as_deref().unwrap_or("events"));
        let lines: Vec<String> = ctx.ui.events.iter().rev().take(inner.h as usize).cloned().collect();
        for (i, line) in lines.iter().enumerate() {
            ctx.text(inner, 0, i as u16, line, if i == 0 { "text" } else { "dim" });
        }
        Ok(())
    }
}

pub struct Help;
impl Component for Help {
    fn draw(&self, ctx: &mut Ctx, _: &ron::Value, _: Option<usize>, r: Rect) -> Result<(), String> {
        ctx.text(r, 1, 0, "space pause · +/- speed · s step · click/p perspective · ←→↑↓ orbit / level · [ ] cut · tab select · q quit", "dim");
        Ok(())
    }
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TextProps {
    pub text: String,
}

pub struct Text;
impl Component for Text {
    fn draw(&self, ctx: &mut Ctx, v: &ron::Value, _: Option<usize>, r: Rect) -> Result<(), String> {
        let p: TextProps = props(v)?;
        for (i, line) in p.text.lines().enumerate() {
            ctx.text(r, 1, i as u16, line, "text");
        }
        Ok(())
    }
}

pub fn bar(v: i64, lo: i64, hi: i64, width: usize) -> String {
    let filled = (((v - lo) as f32 / (hi - lo).max(1) as f32) * width as f32).round().clamp(0.0, width as f32) as usize;
    "█".repeat(filled) + "·".repeat(width - filled).as_str()
}

pub fn spark(h: &VecDeque<i64>, width: usize) -> String {
    const S: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    if h.is_empty() || width == 0 {
        return String::new();
    }
    let step = (h.len() / width).max(1);
    let vals: Vec<i64> = h.iter().step_by(step).copied().collect();
    let (lo, hi) = (*vals.iter().min().unwrap_or(&0), *vals.iter().max().unwrap_or(&0));
    vals.iter().rev().take(width).rev().map(|v| S[(((v - lo) * 7) / (hi - lo).max(1)) as usize]).collect()
}
