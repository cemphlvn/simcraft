//! The game's own pictures for the drive view (`look.textures` in drive.ron): a photograph per surface, found next
//! to the game or in an `assets/` folder above it, decoded from PNG or JPEG. A surface without one (not named, or
//! the file missing) keeps the picture the view makes itself (`geom`), so a game needs no art and a missing file
//! only makes one surface plainer; `simcraft-check` lists which is which.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sim_render::image::Image;

/// One surface's picture: the file, and how big one copy of it is in the world (see `SURFACES` for what `size`
/// measures on each).
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Photo {
    pub file: String,
    pub size: f32,
}

/// The surfaces a picture can go on, and what `size` means there.
pub const SURFACES: [(&str, &str); 16] = [
    ("asphalt", "the racing surface, tiled: metres a tile"),
    ("groove", "the rubbered racing line over the asphalt, streaks along the track: metres a tile"),
    ("apron", "the apron, tiled: metres a tile"),
    ("pit", "pit road, tiled: metres a tile"),
    ("grass", "the infield and the land outside, tiled: metres a tile"),
    ("safer", "the SAFER barrier's face, seamless along the wall: its height (m)"),
    ("concrete", "the walls' concrete, seamless along: metres the picture is high"),
    ("crowd", "the grandstands' seating, seamless along: metres up the stand one copy covers"),
    ("suites", "the suite tower's glass front, seamless along: its height (m)"),
    ("sky", "the sky's panorama, seamless (or mirrored) around: degrees of the horizon one copy spans"),
    ("trees", "the tree line in the distance (with alpha), seamless along: its height (m)"),
    ("dash", "the dashboard panel in front of the driver: its width (m)"),
    ("wheel", "the steering wheel seen from the seat (with alpha): the picture's width (m)"),
    ("banner", "the banner over the start/finish line: its height (m)"),
    ("sponsors", "sponsor boards on the catch fence, the picture split into `boards` panels: a board's height (m)"),
    ("logo", "the track's emblem on the infield grass: its width (m)"),
];

/// A picture that was found: where, its pixels' size, the world size of a copy, and the texture it is uploaded as.
#[derive(Clone, Debug)]
pub struct Found {
    pub path: PathBuf,
    pub w: u32,
    pub h: u32,
    pub size: f32,
    pub texture: String,
}

impl Found {
    /// Width over height.
    pub fn aspect(&self) -> f32 {
        self.w as f32 / self.h.max(1) as f32
    }
}

/// The pictures of a drive view, by surface; and what was asked for but not found.
#[derive(Clone, Debug, Default)]
pub struct Photos {
    pub found: BTreeMap<String, Found>,
    pub missing: Vec<(String, String)>,
}

/// No photographs: every surface in the view's own pictures.
pub static NONE: Photos = Photos { found: BTreeMap::new(), missing: Vec::new() };

/// The file `name` next to the game (`dir`), else in an `assets/` folder in the game's folder or any above it.
pub fn find(dir: &Path, name: &str) -> Option<PathBuf> {
    std::iter::once(dir.join(name)).chain(dir.ancestors().map(|a| a.join("assets").join(name))).find(|p| p.is_file())
}

/// A PNG or JPEG as RGBA.
pub fn decode(path: &Path) -> Result<Image, String> {
    let img = image::open(path).map_err(|e| format!("{}: {e}", path.display()))?.to_rgba8();
    let (w, h) = (img.width() as usize, img.height() as usize);
    Ok(Image { w, h, px: img.into_raw().as_chunks::<4>().0.to_vec() })
}

impl Photos {
    /// Finds every named picture (reading only its size, not its pixels).
    pub fn resolve(dir: &Path, textures: &BTreeMap<String, Photo>) -> Photos {
        let mut out = Photos::default();
        for (key, p) in textures {
            match find(dir, &p.file).map(|path| (image::image_dimensions(&path), path)) {
                Some((Ok((w, h)), path)) if p.size > 0.0 => {
                    let texture = format!("__drive_photo_{key}");
                    out.found.insert(key.clone(), Found { path, w, h, size: p.size, texture });
                }
                Some((Ok(_), _)) => out.missing.push((key.clone(), format!("{}: size must be above 0", p.file))),
                Some((Err(e), path)) => out.missing.push((key.clone(), format!("{}: {e}", path.display()))),
                None => out.missing.push((key.clone(), format!("{} not found next to the game or in an assets/ folder above it", p.file))),
            }
        }
        out
    }

    pub fn get(&self, key: &str) -> Option<&Found> {
        self.found.get(key)
    }

    /// Decodes every picture (on all cores) and uploads it; a picture that fails to decode is reported and its
    /// surface is drawn plain.
    pub fn upload(&self, gpu: &mut crate::gpu::Gpu) {
        use rayon::prelude::*;
        let decoded: Vec<(&Found, Result<Image, String>)> =
            self.found.values().collect::<Vec<_>>().into_par_iter().map(|f| (f, decode(&f.path))).collect();
        for (f, img) in decoded {
            match img {
                Ok(img) => gpu.upload(&f.texture, &img, 2048),
                Err(e) => eprintln!("drive.ron: {e}"),
            }
        }
    }
}

/// Unknown surface names in `look.textures`: a typo would otherwise be a silently plain surface.
pub fn unknown(textures: &BTreeMap<String, Photo>) -> Vec<String> {
    textures.keys().filter(|k| !SURFACES.iter().any(|(s, _)| s == k)).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn race() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../games/race")
    }

    #[test]
    fn a_photo_is_found_in_the_assets_above_the_game_and_a_missing_one_is_listed() {
        let textures = BTreeMap::from([
            ("asphalt".to_string(), Photo { file: "race/asphalt.jpg".into(), size: 4.0 }),
            ("sky".to_string(), Photo { file: "race/no_such_sky.png".into(), size: 90.0 }),
        ]);
        let p = Photos::resolve(&race(), &textures);
        let a = p.get("asphalt").expect("found under assets/");
        assert_eq!((a.w, a.h), (1024, 1024));
        assert_eq!(a.texture, "__drive_photo_asphalt");
        assert!(p.get("sky").is_none());
        assert_eq!(p.missing.len(), 1, "{:?}", p.missing);
        assert_eq!(p.missing[0].0, "sky");
    }

    #[test]
    fn jpeg_and_png_both_decode_and_a_typo_in_a_surface_name_is_caught() {
        let root = race();
        let jpg = decode(&find(&root, "race/asphalt.jpg").expect("the asphalt")).expect("a JPEG");
        assert_eq!((jpg.w, jpg.h), (1024, 1024));
        let png = decode(&find(&root, "race/wheel.png").expect("the wheel")).expect("a PNG");
        assert!(png.px.iter().any(|p| p[3] == 0), "the wheel keeps its alpha");
        let textures = BTreeMap::from([("asphlat".to_string(), Photo { file: "x".into(), size: 1.0 })]);
        assert_eq!(unknown(&textures), vec!["asphlat".to_string()]);
    }
}
