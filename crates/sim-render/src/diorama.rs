//! The Diorama: a side-view "ant farm" in pixel art. Sky and parallax backdrops behind, the surface on the ground
//! line, the soil cut open along one row of the world (`plane`) below it, tunnels and chambers as hollows, the soil
//! tinted by a temperature field, sprites for every entity. Surface entities behind the plane stand a little higher
//! and dimmer (2.5D depth); the camera follows the selected entity and backdrops scroll slower (parallax).

use serde::Deserialize;
use sim_core::Entity;

use crate::canvas::{Cell, Rect, Rgb};
use crate::component::{Component, Ctx, props};
use crate::pixel::{Backdrop, Pixmap, Season, mix, noise};

/// A backdrop layer as written in a view: `(kind: "hills", color: (96, 122, 150), height: 30, detail: 2, seed: 11,
/// speed: 15, haze: 45)`. `detail` is roughness for hills, spacing for trees, clouds per 100 pixels for clouds.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layer {
    pub kind: String,
    pub color: (u8, u8, u8),
    #[serde(default)]
    pub height: i64,
    #[serde(default)]
    pub detail: i64,
    #[serde(default)]
    pub seed: u64,
    /// % of the camera's movement this layer follows: far layers are slow.
    pub speed: i64,
    /// % blended towards the horizon colour (atmospheric distance).
    #[serde(default)]
    pub haze: u32,
}

impl Layer {
    fn backdrop(&self) -> Result<Backdrop, String> {
        Ok(match self.kind.as_str() {
            "hills" => Backdrop::Hills(self.color, self.height, self.detail.max(1), self.seed),
            "trees" => Backdrop::Trees(self.color, self.height, self.detail.max(4), self.seed),
            "clouds" => Backdrop::Clouds(self.color, self.height, self.detail.max(1), self.seed),
            k => return Err(format!("backdrop kind '{k}': use \"hills\", \"trees\" or \"clouds\"")),
        })
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DioramaProps {
    pub title: String,
    /// The world row cut open (y).
    pub plane: i64,
    /// Pixels per voxel.
    pub tile: i64,
    /// Pixels of sky above the ground line.
    pub sky: i64,
    /// Environment whose state and `warmth` colour the sky and the seasons ("" = none).
    pub season_env: String,
    /// Field tinting the soil (e.g. temperature, 0..`tint_max`); "" = none.
    pub tint: String,
    pub tint_max: i64,
    /// Field shown as faint specks on the surface (e.g. scent); "" = none.
    pub specks: String,
    pub layers: Vec<Layer>,
}

impl Default for DioramaProps {
    fn default() -> Self {
        DioramaProps {
            title: "diorama".into(),
            plane: 0,
            tile: 8,
            sky: 40,
            season_env: String::new(),
            tint: String::new(),
            tint_max: 100,
            specks: String::new(),
            layers: Vec::new(),
        }
    }
}

pub struct Diorama;

impl Component for Diorama {
    fn draw(&self, ctx: &mut Ctx, v: &ron::Value, _: Option<usize>, r: Rect) -> Result<(), String> {
        let p: DioramaProps = props(v)?;
        let inner = ctx.panel(r, &format!("{} · {}", p.title, if ctx.ui.graphics { "pixels" } else { "half-blocks" }));
        if inner.w < 4 || inner.h < 2 {
            return Ok(());
        }
        let world = ctx.scene.world;
        let art_h = (p.sky + (world.depth - 1).max(0) * p.tile) as usize;
        // Choose the art width so the picture fills the panel with square pixels.
        let (cw, ch) = (ctx.ui.cell_px.0.max(1) as usize, ctx.ui.cell_px.1.max(1) as usize);
        let (scale, art_w) = if ctx.ui.graphics {
            let s = ((inner.h as usize * ch) / art_h).max(1);
            (s, (inner.w as usize * cw) / s)
        } else {
            (1, inner.w as usize * art_h / (inner.h as usize * 2).max(1))
        };
        let layers = p.layers.iter().map(|l| l.backdrop().map(|b| (b, l.speed, l.haze))).collect::<Result<Vec<_>, _>>()?;
        let art = paint(ctx, &p, &layers, art_w.max(8), art_h);
        if ctx.ui.graphics {
            let bg = ctx.style.color("bg");
            ctx.canvas.fill(inner, Cell { ch: ' ', fg: bg, bg });
            ctx.ui.images.borrow_mut().push((inner, art.scaled(scale)));
        } else {
            art.to_cells(ctx.canvas, inner);
        }
        Ok(())
    }
}

fn paint(ctx: &Ctx, p: &DioramaProps, layers: &[(Backdrop, i64, u32)], w: usize, h: usize) -> Pixmap {
    let scene = ctx.scene;
    let world = scene.world;
    let t = p.tile;
    let ground = p.sky;
    let env = (!p.season_env.is_empty()).then(|| world.of_kind(&p.season_env).next()).flatten();
    let season = env.map_or(Season::Summer, |e| Season::from_state(&e.state));
    let warmth = env.and_then(|e| e.props.get("warmth").copied()).unwrap_or(60);
    let (sky_top, horizon) = season.sky(warmth);

    // Camera: centre on the selected entity, clamped to the world when it is wider than the view.
    let world_px = world.width * t;
    let target = ctx
        .ui
        .selected
        .and_then(|id| world.get(id))
        .map_or(world_px / 2, |e| e.x * t + t / 2);
    let cam = if world_px > w as i64 { (target - w as i64 / 2).clamp(0, world_px - w as i64) } else { -((w as i64 - world_px) / 2) };

    let mut pm = Pixmap::new(w, h, sky_top);
    // Sky gradient.
    for y in 0..ground.max(1) {
        let c = mix(sky_top, horizon, (y * 100 / ground.max(1)) as u32);
        for x in 0..w as i64 {
            pm.set(x, y, c);
        }
    }
    // Sun or moon-pale disc, drifting slowly with the camera (a very far layer).
    let sun_x = w as i64 * 3 / 4 - cam / 12;
    let sun = if season == Season::Winter { Rgb(235, 235, 225) } else { Rgb(255, 236, 170) };
    for dy in -3i64..=3 {
        for dx in -3i64..=3 {
            if dx * dx + dy * dy <= 10 {
                pm.set(sun_x + dx, 8 + dy, sun);
            } else if dx * dx + dy * dy <= 14 {
                pm.blend(sun_x + dx, 8 + dy, sun, 40);
            }
        }
    }
    // Parallax backdrops, far to near.
    for (b, speed, haze) in layers {
        b.draw(&mut pm, cam * speed / 100, ground, horizon, *haze, season);
    }
    // Falling snow in winter (from the frame counter: moves while you watch, never touches the simulation).
    if season == Season::Winter {
        for i in 0..(w as i64 / 6) {
            let n = noise(i, 0, 99);
            let x = ((n % w as u64) as i64 + ctx.ui.frame as i64 / 3 * ((n / 7 % 3) as i64 - 1)).rem_euclid(w as i64);
            let y = ((n / 11) as i64 + ctx.ui.frame as i64 / 2).rem_euclid(ground.max(1));
            pm.blend(x, y, Rgb(245, 248, 255), 85);
        }
    }

    // Ground and soil, column by column.
    for sx in 0..w as i64 {
        let wx = sx + cam;
        let vx = wx.div_euclid(t);
        let inside = (0..world.width).contains(&vx);
        // Grass line with tufts.
        let tuft = (noise(wx, 1, 5) % 4) as i64;
        let grass = season.recolor(Rgb(72, 150, 64), noise(wx, 2, 5).is_multiple_of(5));
        for dy in 0..=tuft.min(2) {
            pm.set(sx, ground - 1 - dy, grass.shade(if dy == 0 { 90 } else { 110 }));
        }
        if season == Season::Winter {
            pm.set(sx, ground - 1 - tuft.min(2), Rgb(236, 240, 248));
        }
        for y in ground..h as i64 {
            let depth = y - ground;
            let level = 1 + depth / t;
            let soil_here = !inside || world.is_terrain(vx, p.plane, level);
            let base = mix(Rgb(122, 86, 56), Rgb(70, 48, 36), (depth * 100 / (h as i64 - ground).max(1)) as u32);
            let mut c = if soil_here {
                // Texture: pebbles and grains, from position (stable while scrolling).
                let n = noise(wx, y, 17);
                let mut c = base;
                if n.is_multiple_of(11) {
                    c = c.shade(125);
                } else if n.is_multiple_of(7) {
                    c = c.shade(82);
                }
                if n.is_multiple_of(97) {
                    c = Rgb(150, 145, 135);
                }
                c
            } else {
                // A hollow: dark air, a lit floor, a shadowed ceiling.
                let iy = depth.rem_euclid(t);
                let above_soil = world.is_terrain(vx, p.plane, level - 1) || level == 1;
                let c = Rgb(34, 24, 22);
                if iy == t - 1 { Rgb(88, 62, 44) } else if iy == 0 && above_soil { Rgb(20, 14, 12) } else { c }
            };
            // Temperature tint: warm soil glows a little red, cold soil turns blue.
            if !p.tint.is_empty() && inside
                && let Some(v) = world.field(&p.tint, vx, p.plane, level)
            {
                let heat = (v * 100 / p.tint_max.max(1)).clamp(0, 100) as u32;
                let tint = if heat >= 50 { Rgb(210, 90, 50) } else { Rgb(80, 120, 210) };
                c = mix(c, tint, (heat as i64 - 50).unsigned_abs() as u32 * 30 / 50);
            }
            pm.set(sx, y, c);
        }
        // Scent on the surface: faint green specks rising above the trail.
        if inside && !p.specks.is_empty() {
            let strongest = (0..world.height).filter_map(|yy| world.field(&p.specks, vx, yy, 0)).max().unwrap_or(0);
            if strongest > 10 && noise(wx, ctx.ui.frame as i64 / 4, 3).is_multiple_of(5) {
                let rise = (noise(wx, 0, 4) % 4) as i64;
                pm.blend(sx, ground - 3 - rise, Rgb(170, 240, 170), (strongest.min(120) as u32) * 60 / 120);
            }
        }
    }

    // Entities, back to front: surface things by their row (behind the plane first), then underground.
    let mut ents: Vec<&Entity> = world.entities().values().filter(|e| scene.visible(e)).collect();
    ents.sort_by_key(|e| (e.z > 0, e.y, e.id));
    let mut facing = ctx.ui.facing.borrow_mut();
    for e in ents {
        let look = scene.assets.look(e, scene.game.state_label(e));
        let Some(sprite) = look.sprite.as_ref().and_then(|s| scene.assets.sprites.get(s)) else { continue };
        let (sw, _) = sprite.size();
        let depth_off = if e.z == 0 { (e.y - p.plane) / 2 } else { 0 };
        if e.z > 0 && (e.y - p.plane).abs() > 3 {
            continue; // underground and far from the cut: hidden in the soil
        }
        let x = e.x * t + (t - sw as i64) / 2 - cam;
        let floor = if e.z == 0 { ground - 1 + depth_off.min(0) - depth_off.max(0) / 2 } else { ground + e.z * t - 2 };
        // Face the way the entity last moved.
        let entry = facing.entry(e.id).or_insert((e.x, false));
        if e.x != entry.0 {
            entry.1 = e.x < entry.0;
            entry.0 = e.x;
        }
        let shade = if e.z == 0 { (100 - (p.plane - e.y).max(0) * 4).clamp(60, 100) as u32 } else { 100 };
        let frame = (ctx.ui.frame * sprite.fps as u64 / 30 + e.id) as usize;
        sprite.blit(&mut pm, &scene.assets.palette, frame, x, floor, entry.1, shade);
        if ctx.ui.selected == Some(e.id) {
            // A small marker above the selected entity.
            let mx = x + sw as i64 / 2;
            let my = floor - sprite.size().1 as i64 - 2;
            pm.set(mx, my, Rgb(255, 255, 255));
            pm.set(mx - 1, my - 1, Rgb(255, 255, 255));
            pm.set(mx + 1, my - 1, Rgb(255, 255, 255));
        }
    }
    pm
}
