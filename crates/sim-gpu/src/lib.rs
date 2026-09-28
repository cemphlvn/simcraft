//! sim-gpu: the HD 2.5D renderer. `stage` turns the world into quads (testable, no GPU); `gpu` draws them with
//! wgpu (Metal, Vulkan, DX12, WebGPU/WebGL2) and can read a frame back as a picture.

pub mod fx;
pub mod gpu;
pub mod math;
pub mod stage;
pub mod track;

use std::path::{Path, PathBuf};

use sim_render::Assets;

pub use stage::{Camera, Composer, Stage};

/// `games/<name>/<file>.ron`, else `<ancestor>/assets/<file>.ron` (as views find asset packs).
pub fn find(dir: &Path, sub: &str, name: &str) -> Option<PathBuf> {
    std::iter::once(dir.join(format!("{name}.ron")))
        .chain(dir.ancestors().map(|a| a.join(sub).join(format!("{name}.ron"))))
        .find(|p| p.exists())
}

/// Loads a stage and the images of its asset packs.
pub fn load_stage(dir: &Path, file: Option<&Path>) -> Result<(Stage, Assets), String> {
    let path = file.map_or_else(|| dir.join("stage.ron"), Path::to_path_buf);
    let src = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let stage: Stage = ron::Options::default()
        .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
        .from_str(&src)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let mut assets = Assets::default();
    for pack in &stage.assets {
        let p = find(dir, "assets", pack).ok_or_else(|| format!("asset pack '{pack}' not found (assets/{pack}.ron)"))?;
        let src = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
        let mut a: Assets = ron::from_str(&src).map_err(|e| format!("{}: {e}", p.display()))?;
        a.load_images(p.parent().unwrap_or_else(|| Path::new(".")))?;
        assets = assets.merged(a);
    }
    let mut missing: Vec<String> = Vec::new();
    let mut need = |n: &str, what: &str| {
        if !assets.loaded.contains_key(n) {
            missing.push(format!("{what}: no image '{n}'"));
        }
    };
    stage.sky.iter().for_each(|s| need(s, "sky"));
    stage.layers.iter().for_each(|l| need(&l.image, "layer"));
    need(&stage.soil.image, "soil");
    need(&stage.soil.hollow, "soil hollow");
    for (k, art) in &stage.kinds {
        art.frames.iter().chain(art.states.values().flatten()).for_each(|f| need(f, &format!("kind '{k}'")));
    }
    stage.season_cards.values().for_each(|c| need(c, "season card"));
    match missing.first() {
        Some(m) => Err(m.clone()),
        None => Ok((stage, assets)),
    }
}

/// Loads a track (`track.ron`) and the images of its asset packs.
pub fn load_track(dir: &Path) -> Result<(track::Track, Assets), String> {
    let path = dir.join("track.ron");
    let src = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let track: track::Track = ron::Options::default()
        .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
        .from_str(&src)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let mut assets = Assets::default();
    for pack in &track.assets {
        let p = find(dir, "assets", pack).ok_or_else(|| format!("asset pack '{pack}' not found (assets/{pack}.ron)"))?;
        let src = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
        let mut a: Assets = ron::from_str(&src).map_err(|e| format!("{}: {e}", p.display()))?;
        a.load_images(p.parent().unwrap_or_else(|| Path::new(".")))?;
        assets = assets.merged(a);
    }
    let mut names: Vec<&String> = vec![&track.road.image, &track.road.shoulder];
    names.extend(track.sky.iter());
    names.extend(track.skyline.iter().map(|s| &s.image));
    names.extend(track.hood.iter().map(|h| &h.image));
    names.extend(track.kinds.values().flat_map(|k| k.frames.iter().chain(k.states.values().flatten())));
    names.extend(track.scenery.iter().flat_map(|s| s.images.iter()));
    names.extend(track.buttons.iter().map(|b| &b.icon));
    names.extend(track.meters.iter().map(|m| &m.icon));
    if let Some(n) = names.into_iter().find(|n| !assets.loaded.contains_key(*n) && !n.starts_with("__")) {
        return Err(format!("track: no image '{n}' in its asset packs"));
    }
    for (event, a) in &track.fx {
        let bad: Vec<&String> = a.tracks.keys().filter(|k| !fx::CHANNELS.contains(&k.as_str())).collect();
        if !bad.is_empty() {
            return Err(format!("track fx '{event}': unknown channel {bad:?} (channels: {:?})", fx::CHANNELS));
        }
    }
    Ok((track, assets))
}
