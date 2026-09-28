//! Themes (CSS-like tokens and classes) and asset libraries (each kind's look, separate from its rules).

use std::collections::BTreeMap;

use serde::Deserialize;
use sim_core::Entity;

use crate::canvas::Rgb;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
pub enum Border {
    #[default]
    Rounded,
    Square,
    Double,
    None,
}

impl Border {
    /// (horizontal, vertical, top-left, top-right, bottom-left, bottom-right)
    pub fn chars(self) -> Option<[char; 6]> {
        match self {
            Border::Rounded => Some(['─', '│', '╭', '╮', '╰', '╯']),
            Border::Square => Some(['─', '│', '┌', '┐', '└', '┘']),
            Border::Double => Some(['═', '║', '╔', '╗', '╚', '╝']),
            Border::None => None,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename = "Theme", deny_unknown_fields)]
pub struct Theme {
    #[serde(default)]
    pub border: Border,
    #[serde(default)]
    pub colors: BTreeMap<String, (u8, u8, u8)>,
    #[serde(default)]
    pub classes: BTreeMap<String, BTreeMap<String, (u8, u8, u8)>>,
}

impl Theme {
    pub fn builtin(name: &str) -> Option<Theme> {
        let src = match name {
            "dark" => include_str!("../themes/dark.ron"),
            "light" => include_str!("../themes/light.ron"),
            _ => return None,
        };
        ron::from_str(src).ok()
    }

    /// `over` wins: a game's theme.ron only needs the tokens it changes.
    pub fn merged(mut self, over: Theme) -> Theme {
        self.colors.extend(over.colors);
        self.classes.extend(over.classes);
        if over.border != Border::default() {
            self.border = over.border;
        }
        self
    }
}

/// The theme as seen by one node: its tokens with the node's class applied.
#[derive(Clone, Debug)]
pub struct Style {
    pub border: Border,
    colors: BTreeMap<String, Rgb>,
}

impl Style {
    pub fn new(theme: &Theme) -> Style {
        Style { border: theme.border, colors: theme.colors.iter().map(|(k, (r, g, b))| (k.clone(), Rgb(*r, *g, *b))).collect() }
    }

    /// A token's colour. Unknown tokens are visible (magenta) rather than silently wrong.
    pub fn color(&self, token: &str) -> Rgb {
        self.colors.get(token).copied().unwrap_or(Rgb(255, 0, 255))
    }

    pub fn with_class(&self, theme: &Theme, class: Option<&str>) -> Style {
        let mut s = self.clone();
        if let Some(over) = class.and_then(|c| theme.classes.get(c)) {
            s.colors.extend(over.iter().map(|(k, (r, g, b))| (k.clone(), Rgb(*r, *g, *b))));
        }
        s
    }
}

/// How one kind (or one of its states) looks.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Look {
    #[serde(default)]
    pub glyph: Option<char>,
    #[serde(default)]
    pub color: Option<(u8, u8, u8)>,
    /// Colour in 3D views (else `color`).
    #[serde(default)]
    pub voxel: Option<(u8, u8, u8)>,
    /// Pixel-art sprite (from the pack's `sprites`).
    #[serde(default)]
    pub sprite: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KindLook {
    #[serde(default)]
    pub glyph: Option<char>,
    #[serde(default)]
    pub color: Option<(u8, u8, u8)>,
    #[serde(default)]
    pub voxel: Option<(u8, u8, u8)>,
    #[serde(default)]
    pub sprite: Option<String>,
    /// State selector (as in rules: "Carry", "Active.Nurse") → look. The most specific match wins.
    #[serde(default)]
    pub states: BTreeMap<String, Look>,
}

/// An asset pack: looks for kinds, reusable across games.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename = "Assets", deny_unknown_fields)]
pub struct Assets {
    #[serde(default)]
    pub kinds: BTreeMap<String, KindLook>,
    /// Palette characters for sprites.
    #[serde(default)]
    pub palette: BTreeMap<char, (u8, u8, u8)>,
    #[serde(default)]
    pub sprites: BTreeMap<String, crate::pixel::Sprite>,
    /// Images by name → PNG path relative to the pack file (backdrops, textures).
    #[serde(default)]
    pub images: BTreeMap<String, String>,
    /// The images, loaded (`load_images`).
    #[serde(skip)]
    pub loaded: BTreeMap<String, crate::image::Image>,
}

impl KindLook {
    pub fn base(&self) -> Look {
        Look { glyph: self.glyph, color: self.color, voxel: self.voxel, sprite: self.sprite.clone() }
    }
}

impl Assets {
    /// Later packs win, kind by kind.
    pub fn merged(mut self, over: Assets) -> Assets {
        self.kinds.extend(over.kinds);
        self.palette.extend(over.palette);
        self.sprites.extend(over.sprites);
        self.images.extend(over.images);
        self.loaded.extend(over.loaded);
        self
    }

    /// Loads every image the pack names, relative to `dir` (the pack file's folder).
    pub fn load_images(&mut self, dir: &std::path::Path) -> Result<(), String> {
        for (name, rel) in &self.images {
            self.loaded.insert(name.clone(), crate::image::Image::load(&dir.join(rel))?);
        }
        Ok(())
    }

    /// The look for an entity in its current state: kind base, then the most specific matching state.
    pub fn look(&self, e: &Entity, label: &str) -> Look {
        let Some(k) = self.kinds.get(&e.kind) else { return Look::default() };
        let mut out = k.base();
        let best = k
            .states
            .iter()
            .filter(|(sel, _)| sim_state::in_label(label, sel))
            .min_by_key(|(sel, _)| sim_state::depth_in_label(label, sel));
        if let Some((_, l)) = best {
            out.glyph = l.glyph.or(out.glyph);
            out.color = l.color.or(out.color);
            out.voxel = l.voxel.or(out.voxel).or(l.color);
            out.sprite = l.sprite.clone().or(out.sprite);
        }
        out
    }
}
