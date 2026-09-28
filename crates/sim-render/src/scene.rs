//! What the renderer knows about a game: the world plus the designer's glyphs. Read-only.

use sim_core::{Entity, World};
use sim_rules::Game;

use crate::canvas::Rgb;
use crate::style::{Assets, Style};

pub struct Scene<'a> {
    pub world: &'a World,
    pub game: &'a Game,
    pub assets: &'a Assets,
}

impl Scene<'_> {
    pub fn visible(&self, e: &Entity) -> bool {
        !self.game.is_hidden(&e.kind)
    }

    /// The entity drawn in a voxel: the last visible one (newest on top).
    pub fn top(&self, x: i64, y: i64, z: i64) -> Option<&Entity> {
        self.world.at3(x, y, z).iter().rev().map(|id| &self.world.entities()[id]).find(|e| self.visible(e))
    }

    /// The asset pack's glyph for this state, else the game's own glyph.
    pub fn glyph(&self, e: &Entity) -> char {
        self.assets.look(e, self.game.state_label(e)).glyph.unwrap_or_else(|| self.game.glyph_of(e))
    }

    /// The asset pack's colour, else a stable colour per kind.
    pub fn color(&self, e: &Entity) -> Rgb {
        self.assets.look(e, self.game.state_label(e)).color.map_or_else(|| Rgb::of(&e.kind), |(r, g, b)| Rgb(r, g, b))
    }

    /// Colour in 3D views.
    pub fn voxel(&self, e: &Entity) -> Rgb {
        let look = self.assets.look(e, self.game.state_label(e));
        look.voxel.or(look.color).map_or_else(|| Rgb::of(&e.kind), |(r, g, b)| Rgb(r, g, b))
    }

    pub fn terrain(&self, x: i64, y: i64, z: i64) -> bool {
        self.world.is_terrain(x, y, z)
    }

    /// Background of an empty voxel: the theme's soil or air; tinted by a field if asked.
    pub fn ground(&self, style: &Style, x: i64, y: i64, z: i64, tint: Option<&str>) -> Rgb {
        let base = if self.terrain(x, y, z) { style.color("soil").shade(75) } else { style.color("air") };
        match tint.and_then(|f| self.world.field(f, x, y, z)) {
            Some(v) => heat(v, base),
            None => base,
        }
    }
}

/// A field value 0..100 as a cold-to-warm tint over `base`.
pub fn heat(v: i64, base: Rgb) -> Rgb {
    let t = v.clamp(0, 100) as u32;
    let warm = Rgb((40 + t * 2) as u8, (30 + t / 2) as u8, (90u32.saturating_sub(t)) as u8);
    Rgb(
        ((base.0 as u32 + warm.0 as u32) / 2) as u8,
        ((base.1 as u32 + warm.1 as u32) / 2) as u8,
        ((base.2 as u32 + warm.2 as u32) / 2) as u8,
    )
}
