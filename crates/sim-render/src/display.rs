//! The diorama as a display list, shared by every backend.
//!
//! `compose` turns the world into what to draw: sky colours, backdrop strips (cooked once per season and warmth
//! band), the world's cross-section (one picture, changes only when the world does), and sprite draws. The CPU
//! backend (`rasterize`, used by the terminal) and the GPU backend (`sim-gpu`) draw the same list, so a game looks
//! the same everywhere and the GPU only receives what changed.

use std::collections::BTreeMap;

use sim_core::{Entity, EntityId};

use crate::canvas::Rgb;
use crate::diorama::{DioramaProps, Layer};
use crate::image::Image;
use crate::pixel::{Pixmap, Season, mix, noise};
use crate::scene::Scene;

/// Width of a cooked backdrop strip in pixels: wider than any view plus the furthest a layer scrolls.
pub const STRIP: usize = 2048;
/// Soil drawn beyond each edge of the world, so the camera never shows a gap.
pub const MARGIN: i64 = 512;

/// A backdrop layer cooked into a strip: pixels from x = 0 of the layer's own scroll space.
#[derive(Clone, Debug)]
pub struct Strip {
    pub image: Image,
    /// Screen y of the strip's top row.
    pub top: i64,
    /// % of the camera's movement the layer follows.
    pub speed: i64,
}

/// One sprite to draw, in world pixels (not relative to the camera).
#[derive(Clone, Debug, PartialEq)]
pub struct SpriteDraw {
    pub sprite: String,
    pub frame: usize,
    /// Left edge, world pixels (fractional between ticks).
    pub x: f32,
    /// Bottom row (the floor it stands on), fractional between ticks.
    pub floor: f32,
    pub flip: bool,
    pub shade: u32,
    pub selected: bool,
}

pub struct DisplayList {
    pub season: Season,
    pub sky: (Rgb, Rgb),
    /// y of the ground line; the picture is `height` pixels tall.
    pub ground: i64,
    pub height: i64,
    /// Width of the world in pixels (the camera clamps to it).
    pub world_px: i64,
    /// Cache key of the strips: they change only with the season and a band of warmth.
    pub strips_key: u64,
    /// Cache key of the section: changes only when the cut plane's terrain, tint band or specks change.
    pub section_key: u64,
    pub sprites: Vec<SpriteDraw>,
    /// World-space specks above the ground (x, y, colour, alpha %).
    pub specks: Vec<(i64, i64, Rgb, u32)>,
}

/// Where the camera looks (left edge, world pixels) for a view `w` pixels wide.
pub fn camera(world_px: i64, target: i64, w: i64) -> i64 {
    if world_px > w { (target - w / 2).clamp(0, world_px - w) } else { -((w - world_px) / 2) }
}

fn season_of(scene: &Scene, p: &DioramaProps) -> (Season, i64) {
    let env = (!p.season_env.is_empty()).then(|| scene.world.of_kind(&p.season_env).next()).flatten();
    let season = env.map_or(Season::Summer, |e| Season::from_state(&e.state));
    let warmth = env.and_then(|e| e.props.get("warmth").copied()).unwrap_or(60);
    (season, warmth)
}

fn fnv(h: &mut u64, v: u64) {
    *h = (*h ^ v).wrapping_mul(0x0100_0000_01b3);
}

/// Builds the display list. `facing` remembers which way each entity last moved (the caller keeps it).
#[allow(clippy::too_many_arguments)]
pub fn compose(
    scene: &Scene,
    p: &DioramaProps,
    frame: u64,
    selected: Option<EntityId>,
    facing: &mut BTreeMap<EntityId, (i64, bool)>,
    tween: Option<&crate::feel::Tween>,
    bob: i64,
) -> DisplayList {
    let world = scene.world;
    let t = p.tile;
    let ground = p.sky;
    let height = p.sky + (world.depth - 1).max(0) * t;
    let (season, warmth) = season_of(scene, p);
    let sky = season.sky(warmth);

    let mut strips_key = 0xcbf2_9ce4_8422_2325;
    fnv(&mut strips_key, season as u64);
    fnv(&mut strips_key, (warmth / 10) as u64);

    // The section changes with the terrain on the cut plane, the tint band of each voxel, and the specks.
    let mut section_key = strips_key;
    for z in 1..world.depth {
        for x in 0..world.width {
            fnv(&mut section_key, world.is_terrain(x, p.plane, z) as u64);
            if !p.tint.is_empty()
                && let Some(v) = world.field(&p.tint, x, p.plane, z)
            {
                fnv(&mut section_key, (v * 20 / p.tint_max.max(1)) as u64);
            }
        }
    }

    let mut specks = Vec::new();
    if !p.specks.is_empty() {
        for vx in 0..world.width {
            let strongest = (0..world.height).filter_map(|yy| world.field(&p.specks, vx, yy, 0)).max().unwrap_or(0);
            if strongest <= 10 {
                continue;
            }
            for wx in vx * t..(vx + 1) * t {
                if noise(wx, frame as i64 / 4, 3).is_multiple_of(5) {
                    let rise = (noise(wx, 0, 4) % 4) as i64;
                    specks.push((wx, ground - 3 - rise, Rgb(170, 240, 170), (strongest.min(120) as u32) * 60 / 120));
                }
            }
        }
    }

    // Entities, back to front: surface things by their row (behind the plane first), then underground.
    let mut ents: Vec<&Entity> = world.entities().values().filter(|e| scene.visible(e)).collect();
    ents.sort_by_key(|e| (e.z > 0, e.y, e.id));
    let mut sprites = Vec::new();
    for e in ents {
        let look = scene.assets.look(e, scene.game.state_label(e));
        let Some(name) = look.sprite else { continue };
        let Some(sprite) = scene.assets.sprites.get(&name) else { continue };
        if e.z > 0 && (e.y - p.plane).abs() > 3 {
            continue; // underground and far from the cut: hidden in the soil
        }
        let (sw, _) = sprite.size();
        // Where the sprite stands for a voxel position; between ticks, lerp the two.
        let place = |x: i64, y: i64, z: i64| -> (f32, f32) {
            let depth_off = if z == 0 { (y - p.plane) / 2 } else { 0 };
            let floor = if z == 0 { ground - 1 + depth_off.min(0) - depth_off.max(0) / 2 } else { ground + z * t - 2 };
            ((x * t + (t - sw as i64) / 2) as f32, floor as f32)
        };
        let now = (e.x, e.y, e.z);
        let (mut x, mut floor) = place(e.x, e.y, e.z);
        let mut moving = false;
        if let Some(tw) = tween
            && let Some(&before) = tw.prev.get(&e.id)
            && before != now
            && (before.0 - now.0).abs() <= 2
            && (before.2 - now.2).abs() <= 2
        {
            let a = tw.alpha.clamp(0.0, 1.0);
            let (x0, f0) = place(before.0, before.1, before.2);
            x = x0 + (x - x0) * a;
            floor = f0 + (floor - f0) * a;
            moving = true;
        }
        if moving && bob > 0 && (frame / 4 + e.id).is_multiple_of(2) {
            floor -= bob as f32;
        }
        let entry = facing.entry(e.id).or_insert((e.x, false));
        if e.x != entry.0 {
            entry.1 = e.x < entry.0;
            entry.0 = e.x;
        }
        let shade = if e.z == 0 { (100 - (p.plane - e.y).max(0) * 4).clamp(60, 100) as u32 } else { 100 };
        sprites.push(SpriteDraw {
            frame: (frame * sprite.fps as u64 / 30 + e.id) as usize,
            x,
            floor,
            flip: entry.1,
            shade,
            selected: selected == Some(e.id),
            sprite: name,
        });
    }
    DisplayList { season, sky, ground, height, world_px: world.width * t, strips_key, section_key, sprites, specks }
}

/// The backdrop strips for the list's season (cook once per `strips_key`).
pub fn cook_strips(scene: &Scene, p: &DioramaProps, list: &DisplayList) -> Result<Vec<Strip>, String> {
    const KEY: Rgb = Rgb(255, 0, 255);
    let horizon = list.sky.1;
    let mut out = Vec::new();
    for l in &p.layers {
        let (pm, top) = match l.backdrop()? {
            Some(b) => {
                let mut pm = Pixmap::new(STRIP, list.ground.max(1) as usize, KEY);
                b.draw(&mut pm, 0, list.ground, horizon, l.haze, list.season);
                (pm, 0)
            }
            None => {
                let img = scene.assets.loaded.get(&l.image).ok_or_else(|| format!("image '{}' not loaded", l.image))?;
                let mut pm = Pixmap::new(STRIP, img.h, KEY);
                for sx in 0..STRIP {
                    let ix = sx % img.w;
                    for iy in 0..img.h {
                        let px = img.get(ix, iy);
                        if px[3] > 0 {
                            let c = recolor_image(list.season, Rgb(px[0], px[1], px[2]), horizon, l, ix, iy);
                            pm.set(sx as i64, iy as i64, c);
                        }
                    }
                }
                (pm, list.ground - l.height - img.h as i64)
            }
        };
        let image =
            Image { w: pm.w, h: pm.h, px: pm.px.iter().map(|c| if *c == KEY { [0, 0, 0, 0] } else { [c.0, c.1, c.2, 255] }).collect() };
        out.push(Strip { image, top, speed: l.speed });
    }
    Ok(out)
}

/// The second autumn hue falls only on foliage (green pixels), in small scattered clumps, like turning leaves.
/// Rock and sky-coloured pixels keep the plain seasonal tint.
fn recolor_image(season: Season, c: Rgb, horizon: Rgb, l: &Layer, x: usize, y: usize) -> Rgb {
    let foliage = c.1 > c.0 && c.1 > c.2;
    let clump = foliage && noise((x / 2) as i64, (y / 2) as i64, 31).is_multiple_of(5);
    season.recolor(mix(c, horizon, l.haze), clump)
}

/// The world's cross-section along the cut plane, with the grass line on top. Covers world x from `-MARGIN` to
/// `world_px + MARGIN`; row 0 is `ground - 3`.
pub fn cook_section(scene: &Scene, p: &DioramaProps, list: &DisplayList) -> Image {
    let world = scene.world;
    let t = p.tile;
    let (ground, h) = (list.ground, list.height);
    let season = list.season;
    let x0 = -MARGIN;
    let w = (list.world_px + 2 * MARGIN) as usize;
    let rows = (h - (ground - 3)) as usize;
    let mut px = vec![[0u8, 0, 0, 0]; w * rows];
    let mut put = |wx: i64, y: i64, c: Rgb| {
        let (col, row) = (wx - x0, y - (ground - 3));
        if col >= 0 && row >= 0 && (col as usize) < w && (row as usize) < rows {
            px[row as usize * w + col as usize] = [c.0, c.1, c.2, 255];
        }
    };
    let texture = (!p.soil.is_empty()).then(|| scene.assets.loaded.get(&p.soil)).flatten();
    for wx in x0..x0 + w as i64 {
        let vx = wx.div_euclid(t);
        let inside = (0..world.width).contains(&vx);
        let tuft = (noise(wx, 1, 5) % 4) as i64;
        let grass = season.recolor(Rgb(72, 150, 64), noise(wx, 2, 5).is_multiple_of(5));
        for dy in 0..=tuft.min(2) {
            put(wx, ground - 1 - dy, grass.shade(if dy == 0 { 90 } else { 110 }));
        }
        if season == Season::Winter {
            put(wx, ground - 1 - tuft.min(2), Rgb(236, 240, 248));
        }
        for y in ground..h {
            let depth = y - ground;
            let level = 1 + depth / t;
            let soil_here = !inside || world.is_terrain(vx, p.plane, level);
            let mut c = if let (true, Some(tex)) = (soil_here, texture) {
                let q = tex.get(wx.rem_euclid(tex.w as i64) as usize, depth.rem_euclid(tex.h as i64) as usize);
                mix(Rgb(q[0], q[1], q[2]), Rgb(40, 28, 22), (depth * 60 / (h - ground).max(1)) as u32)
            } else if soil_here {
                let base = mix(Rgb(122, 86, 56), Rgb(70, 48, 36), (depth * 100 / (h - ground).max(1)) as u32);
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
                let iy = depth.rem_euclid(t);
                let above_soil = world.is_terrain(vx, p.plane, level - 1) || level == 1;
                if iy == t - 1 {
                    Rgb(88, 62, 44)
                } else if iy == 0 && above_soil {
                    Rgb(20, 14, 12)
                } else {
                    Rgb(34, 24, 22)
                }
            };
            if !p.tint.is_empty()
                && inside
                && let Some(v) = world.field(&p.tint, vx, p.plane, level)
            {
                let heat = (v * 100 / p.tint_max.max(1)).clamp(0, 100) as u32;
                let tint = if heat >= 50 { Rgb(210, 90, 50) } else { Rgb(80, 120, 210) };
                c = mix(c, tint, (heat as i64 - 50).unsigned_abs() as u32 * 30 / 50);
            }
            put(wx, y, c);
        }
    }
    Image { w, h: rows, px }
}

/// Snow flakes in screen space for a view `w` wide (moves with the frame counter, never with the simulation).
pub fn snow(w: i64, ground: i64, frame: u64) -> Vec<(i64, i64)> {
    (0..w / 6)
        .map(|i| {
            let n = noise(i, 0, 99);
            let x = ((n % w as u64) as i64 + frame as i64 / 3 * ((n / 7 % 3) as i64 - 1)).rem_euclid(w);
            let y = ((n / 11) as i64 + frame as i64 / 2).rem_euclid(ground.max(1));
            (x, y)
        })
        .collect()
}

/// Screen x of the sun for a view `w` wide.
pub fn sun_x(w: i64, cam: i64) -> i64 {
    w * 3 / 4 - cam / 12
}

pub fn sun_color(season: Season) -> Rgb {
    if season == Season::Winter { Rgb(235, 235, 225) } else { Rgb(255, 236, 170) }
}

/// The CPU backend: draws the list into a `w`-pixel-wide picture with the camera at `cam`.
pub fn rasterize(scene: &Scene, list: &DisplayList, strips: &[Strip], section: &Image, w: usize, cam: i64, frame: u64) -> Pixmap {
    let (sky_top, horizon) = list.sky;
    let ground = list.ground;
    let mut pm = Pixmap::new(w, list.height as usize, sky_top);
    for y in 0..ground.max(1) {
        let c = mix(sky_top, horizon, (y * 100 / ground.max(1)) as u32);
        for x in 0..w as i64 {
            pm.set(x, y, c);
        }
    }
    let (sx0, sun) = (sun_x(w as i64, cam), sun_color(list.season));
    for dy in -3i64..=3 {
        for dx in -3i64..=3 {
            if dx * dx + dy * dy <= 10 {
                pm.set(sx0 + dx, 8 + dy, sun);
            } else if dx * dx + dy * dy <= 14 {
                pm.blend(sx0 + dx, 8 + dy, sun, 40);
            }
        }
    }
    for s in strips {
        let lcam = cam * s.speed / 100;
        for sx in 0..w as i64 {
            let ix = (sx + lcam).rem_euclid(s.image.w as i64) as usize;
            for iy in 0..s.image.h {
                let q = s.image.get(ix, iy);
                if q[3] > 0 {
                    pm.set(sx, s.top + iy as i64, Rgb(q[0], q[1], q[2]));
                }
            }
        }
    }
    if list.season == Season::Winter {
        for (x, y) in snow(w as i64, ground, frame) {
            pm.blend(x, y, Rgb(245, 248, 255), 85);
        }
    }
    let top = ground - 3;
    for sx in 0..w as i64 {
        let col = sx + cam + MARGIN;
        if col < 0 || col >= section.w as i64 {
            continue;
        }
        for row in 0..section.h {
            let q = section.get(col as usize, row);
            if q[3] > 0 {
                pm.set(sx, top + row as i64, Rgb(q[0], q[1], q[2]));
            }
        }
    }
    for &(x, y, c, a) in &list.specks {
        pm.blend(x - cam, y, c, a);
    }
    for d in &list.sprites {
        let Some(sprite) = scene.assets.sprites.get(&d.sprite) else { continue };
        let (x, floor) = (d.x.round() as i64, d.floor.round() as i64);
        sprite.blit(&mut pm, &scene.assets.palette, d.frame, x - cam, floor, d.flip, d.shade);
        if d.selected {
            let (sw, sh) = sprite.size();
            let (mx, my) = (x - cam + sw as i64 / 2, floor - sh as i64 - 2);
            for (dx, dy) in [(0, 0), (-1, -1), (1, -1)] {
                pm.set(mx + dx, my + dy, Rgb(255, 255, 255));
            }
        }
    }
    pm
}

/// Sprite atlas: every frame of every sprite in one RGBA image, with each frame's rectangle.
pub struct Atlas {
    pub image: Image,
    /// (sprite, frame) → (x, y, w, h) in the atlas.
    pub rects: BTreeMap<(String, usize), (u32, u32, u32, u32)>,
}

/// Packs all sprite frames in rows (shelf packing), plus a 1×1 white pixel at the origin for solid quads.
pub fn build_atlas(assets: &crate::style::Assets) -> Atlas {
    let width = 512usize;
    /// (sprite, frame), x, y, pixels, w, h
    type Placed = ((String, usize), usize, usize, Vec<[u8; 4]>, usize, usize);
    let mut placed: Vec<Placed> = Vec::new();
    let (mut x, mut y, mut row_h) = (2usize, 0usize, 1usize);
    for (name, sprite) in &assets.sprites {
        let mut sprite = sprite.clone();
        if sprite.baked.is_empty() {
            sprite.bake(&assets.palette, &assets.loaded);
        }
        for (i, frame) in sprite.baked.iter().enumerate() {
            let (w, h) = (frame.w, frame.h);
            if x + w + 1 > width {
                x = 0;
                y += row_h + 1;
                row_h = 1;
            }
            placed.push(((name.clone(), i), x, y, frame.px.clone(), w, h));
            x += w + 1;
            row_h = row_h.max(h);
        }
    }
    let height = (y + row_h + 1).next_power_of_two().max(4);
    let mut img = Image { w: width, h: height, px: vec![[0; 4]; width * height] };
    img.px[0] = [255, 255, 255, 255];
    let mut rects = BTreeMap::new();
    for (key, x, y, px, w, h) in placed {
        for r in 0..h {
            for c in 0..w {
                img.px[(y + r) * width + x + c] = px[r * w + c];
            }
        }
        rects.insert(key, (x as u32, y as u32, w as u32, h as u32));
    }
    Atlas { image: img, rects }
}
