//! SMASH's tuning as data (`games/smash/smash.ron`): embedded in the player, read from disk by the eval tool, and
//! changed one field at a time by `--set path=value` (a probe).

use serde::Deserialize;

/// The tuning the player ships with.
pub const EMBEDDED: &str = include_str!("../../../../games/smash/smash.ron");

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Tuning {
    pub physics: Physics,
    pub sling: Sling,
    pub stone: Stone,
    pub camera: CameraTuning,
    pub feel: Feel,
    pub pieces: Pieces,
    pub levels: Vec<Level>,
    pub pedestal: Pedestal,
    pub materials: Materials,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Physics {
    pub gravity: f32,
    pub substeps: u32,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Sling {
    pub at: (f32, f32, f32),
    pub pull_screen: f32,
    pub min_speed: f32,
    pub max_speed: f32,
    /// The lowest aim (m above the pedestal's top), the highest (m above the tower's top), and how far across the
    /// tower a full screen's width of sideways finger travel aims (m).
    pub aim_low: f32,
    pub aim_over: f32,
    pub aim_width: f32,
    pub pouch_travel: f32,
    /// A release shoots the aim from this long before the finger lifted (s): a lifting finger rolls a few pixels.
    pub release_lock: f32,
    pub band_hz: f32,
    pub band_damping: f32,
    pub reload: f32,
    pub ticks: u32,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Stone {
    pub radius: f32,
    pub density: f32,
    pub restitution: f32,
    pub friction: f32,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct CameraTuning {
    pub eye: (f32, f32, f32),
    pub look: (f32, f32, f32),
    pub fov: f32,
    pub pull_fov: f32,
    pub pull_drop: f32,
    pub lean: f32,
    /// Tilt parallax: how far the eye slides per g of tilt away from how the phone is usually held (m).
    pub tilt: f32,
    pub follow: f32,
    pub follow_fov: f32,
    pub hz: f32,
    pub damping: f32,
    pub settle: f32,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Feel {
    pub hitstop: f32,
    pub hitstop_impulse: f32,
    pub slowmo_scale: f32,
    pub slowmo_time: f32,
    pub slowmo_impulse: f32,
    pub shake_per_impulse: f32,
    pub shake_decay: f32,
    pub shake_angle: f32,
    pub shake_move: f32,
    pub chips_per_impulse: f32,
    pub chips_max: u32,
    pub chip_life: f32,
    pub flash: f32,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Pieces {
    pub cell: (f32, f32, f32),
}

/// One level: a tower as rows of pieces (from the bottom) and the stones to knock it off with.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Level {
    pub name: String,
    pub stones: u32,
    pub depth: u32,
    pub rows: Vec<String>,
}

/// The pieces a row may hold.
pub const PIECES: &str = "wscg=#.";

impl Level {
    /// Half its width and half its depth (m).
    pub fn half_extent(&self, cell: (f32, f32, f32)) -> (f32, f32) {
        let w = self.rows.iter().map(|r| r.chars().count()).max().unwrap_or(0) as f32;
        (w * cell.0 / 2.0, self.depth.max(1) as f32 * cell.2 / 2.0)
    }

    /// How tall it stands (m).
    pub fn height(&self, cell: (f32, f32, f32)) -> f32 {
        self.rows.len() as f32 * cell.1
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Pedestal {
    pub radius: f32,
    pub top: f32,
    pub thickness: f32,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
pub struct MaterialTuning {
    pub density: f32,
    pub friction: f32,
    pub restitution: f32,
    pub colour: u32,
    /// A hit that changes its speed by more than this (m/s) breaks it; 0 = unbreakable.
    pub break_dv: f32,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Materials {
    pub wood: MaterialTuning,
    pub stone: MaterialTuning,
    pub can: MaterialTuning,
    pub glass: MaterialTuning,
}

impl Tuning {
    pub fn embedded() -> Tuning {
        Tuning::parse(EMBEDDED, &[]).expect("games/smash/smash.ron parses")
    }

    /// What cannot work as written (the checker and the edit hook report these where they are).
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        let s = &self.sling;
        if s.min_speed <= 0.0 || s.max_speed < s.min_speed {
            out.push(format!("sling: speeds must rise from min_speed to max_speed (got {} → {})", s.min_speed, s.max_speed));
        }
        if !(0.05..=0.9).contains(&s.pull_screen) {
            out.push(format!("sling.pull_screen = {}: a full pull must fit on the screen (0.05..0.9)", s.pull_screen));
        }
        if self.physics.substeps == 0 {
            out.push("physics.substeps = 0: the solver needs at least one".into());
        }
        if self.levels.is_empty() {
            out.push("levels is empty: nothing to play".into());
        }
        for (l, lv) in self.levels.iter().enumerate() {
            let at = format!("levels[{l}] ({})", lv.name);
            if lv.rows.is_empty() || lv.rows.iter().all(|r| r.chars().all(|c| c == '.')) {
                out.push(format!("{at}: no pieces: nothing to knock off"));
            }
            if lv.stones == 0 {
                out.push(format!("{at}: stones = 0: it can't be played"));
            }
            for (i, r) in lv.rows.iter().enumerate() {
                if let Some(c) = r.chars().find(|c| !PIECES.contains(*c)) {
                    out.push(format!(
                        "{at}.rows[{i}]: unknown piece {c:?} (w wood, s stone, c can, g glass, = wood beam, # stone beam, . empty)"
                    ));
                }
            }
            let (hw, hd) = lv.half_extent(self.pieces.cell);
            if hw > 2.5 || hd > 1.5 {
                out.push(format!("{at}: {:.1} m wide: wider than the camera shows (at most 5 m)", hw * 2.0));
            }
        }
        out
    }

    /// Parses `src`, then applies each `path=value` (e.g. `sling.max_speed=28`): a probe changes one field and
    /// nothing else. An unknown path is an error, never silently ignored.
    pub fn parse(src: &str, sets: &[String]) -> Result<Tuning, String> {
        let mut v: ron::Value = ron::from_str(src).map_err(|e| format!("smash.ron: {e}"))?;
        for s in sets {
            let (path, value) = s.split_once('=').ok_or_else(|| format!("--set {s}: expected path=value"))?;
            let new: ron::Value = ron::from_str(value.trim()).map_err(|e| format!("--set {s}: {e}"))?;
            let slot = path.trim().split('.').try_fold(&mut v, |at, key| match at {
                ron::Value::Map(m) => m.get_mut(&ron::Value::String(key.to_string())).ok_or_else(|| format!("--set {s}: no field `{key}`")),
                _ => Err(format!("--set {s}: `{key}` is not inside a structure")),
            })?;
            *slot = new;
        }
        // Integers written where a float is meant (`max_speed=28`) are read as floats.
        v.into_rust::<Tuning>().map_err(|e| format!("smash.ron: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_tuning_parses() {
        let t = Tuning::embedded();
        assert!(t.sling.max_speed > t.sling.min_speed);
        assert!(!t.levels.is_empty());
        assert!(t.problems().is_empty(), "{:?}", t.problems());
    }

    #[test]
    fn a_probe_changes_one_field_and_a_typo_is_an_error() {
        let t = Tuning::parse(EMBEDDED, &["sling.max_speed=28.5".into()]).unwrap();
        let base = Tuning::embedded();
        assert_eq!(t.sling.max_speed, 28.5);
        assert_eq!(t.camera, base.camera);
        assert!(Tuning::parse(EMBEDDED, &["sling.max_sped=28".into()]).unwrap_err().contains("max_sped"));
    }
}
