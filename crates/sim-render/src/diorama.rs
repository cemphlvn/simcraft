//! The Diorama: a side-view "ant farm" in pixel art. Sky and parallax backdrops behind, the surface on the ground
//! line, the soil cut open along one row of the world (`plane`) below it, tunnels and chambers as hollows, the soil
//! tinted by a temperature field, sprites for every entity. Surface entities behind the plane stand a little higher
//! and dimmer (2.5D depth); the camera follows the selected entity and backdrops scroll slower (parallax).

use serde::Deserialize;

use crate::canvas::{Cell, Rect};
use crate::component::{Component, Ctx, props};
use crate::display;
use crate::pixel::Backdrop;

/// A backdrop layer as written in a view: `(kind: "hills", color: (96, 122, 150), height: 30, detail: 2, seed: 11,
/// speed: 15, haze: 45)`. `detail` is roughness for hills, spacing for trees, clouds per 100 pixels for clouds.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layer {
    pub kind: String,
    #[serde(default)]
    pub color: (u8, u8, u8),
    /// For `kind: "image"`: an image from the asset packs, tiled across and standing on the ground (`height` lifts it).
    #[serde(default)]
    pub image: String,
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
    pub fn backdrop(&self) -> Result<Option<Backdrop>, String> {
        Ok(Some(match self.kind.as_str() {
            "image" => return Ok(None),
            "hills" => Backdrop::Hills(self.color, self.height, self.detail.max(1), self.seed),
            "trees" => Backdrop::Trees(self.color, self.height, self.detail.max(4), self.seed),
            "clouds" => Backdrop::Clouds(self.color, self.height, self.detail.max(1), self.seed),
            k => return Err(format!("backdrop kind '{k}': use \"image\", \"hills\", \"trees\" or \"clouds\"")),
        }))
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
    /// Soil texture: an image from the asset packs, tiled; "" = procedural soil.
    pub soil: String,
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
            soil: String::new(),
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
        for l in p.layers.iter().filter(|l| l.kind == "image") {
            if !ctx.scene.assets.loaded.contains_key(&l.image) {
                return Err(format!("image '{}' is not in the asset packs' `images`", l.image));
            }
        }
        if !p.soil.is_empty() && !ctx.scene.assets.loaded.contains_key(&p.soil) {
            return Err(format!("soil image '{}' is not in the asset packs' `images`", p.soil));
        }
        let scene = ctx.scene;
        let list = display::compose(scene, &p, ctx.ui.frame, ctx.ui.selected, &mut ctx.ui.facing.borrow_mut());
        // Cooked once per season band / world change, reused every frame.
        let mut strips = ctx.ui.strips.borrow_mut();
        if strips.as_ref().is_none_or(|(k, _)| *k != list.strips_key) {
            *strips = Some((list.strips_key, display::cook_strips(scene, &p, &list)?));
        }
        let mut section = ctx.ui.section.borrow_mut();
        if section.as_ref().is_none_or(|(k, _)| *k != list.section_key) {
            *section = Some((list.section_key, display::cook_section(scene, &p, &list)));
        }
        let target = ctx.ui.selected.and_then(|id| scene.world.get(id)).map_or(list.world_px / 2, |e| e.x * p.tile + p.tile / 2);
        let w = art_w.max(8) as i64;
        let cam = display::camera(list.world_px, target, w);
        let (strips, section) = (&strips.as_ref().expect("cooked").1, &section.as_ref().expect("cooked").1);
        let art = display::rasterize(scene, &list, strips, section, w as usize, cam, ctx.ui.frame);
        let _ = art_h;
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

