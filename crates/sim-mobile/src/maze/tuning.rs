//! MAZE's tuning as data (`games/maze/maze.ron`): embedded in the player, and read by the autopilot evals.

use serde::Deserialize;

/// The tuning the player ships with.
pub const EMBEDDED: &str = include_str!("../../../../games/maze/maze.ron");

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Tuning {
    pub physics: Physics,
    pub marble: Marble,
    pub board: Board,
    pub feel: Feel,
    pub camera: CameraTuning,
    pub levels: Vec<Level>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Physics {
    pub gravity: f32,
    pub substeps: u32,
    pub max_tilt: f32,
    pub tilt_lag: f32,
    pub drag_full: f32,
    pub rolling_resistance: f32,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Marble {
    pub radius: f32,
    pub density: f32,
    pub friction: f32,
    pub restitution: f32,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Board {
    pub cell: f32,
    pub wall_height: f32,
    pub capture: f32,
    pub hole_radius: f32,
    pub friction: f32,
    pub restitution: f32,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Feel {
    pub knock_min: f32,
    pub knock_full: f32,
    pub respawn: f32,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct CameraTuning {
    pub fov: f32,
    pub margin: f32,
    pub lean: f32,
    pub tilt_shown: f32,
}

/// A cell of a level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cell {
    Wall,
    Floor,
    Hole,
    Start,
    Goal,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Level {
    pub name: String,
    pub rows: Vec<String>,
}

impl Level {
    /// Columns and rows.
    pub fn size(&self) -> (usize, usize) {
        (self.rows.iter().map(|r| r.chars().count()).max().unwrap_or(0), self.rows.len())
    }

    /// The cell at column `c`, row `r` (outside the rows, or a short row's end: wall).
    pub fn at(&self, c: usize, r: usize) -> Cell {
        match self.rows.get(r).and_then(|row| row.chars().nth(c)) {
            Some('.') => Cell::Floor,
            Some('o') => Cell::Hole,
            Some('S') => Cell::Start,
            Some('G') => Cell::Goal,
            _ => Cell::Wall,
        }
    }

    /// Every cell of a kind, in reading order.
    pub fn find(&self, kind: Cell) -> Vec<(usize, usize)> {
        let (w, h) = self.size();
        (0..h).flat_map(|r| (0..w).map(move |c| (c, r))).filter(|&(c, r)| self.at(c, r) == kind).collect()
    }
}

impl Tuning {
    /// The shipped tuning's text.
    pub fn embedded_src() -> &'static str {
        EMBEDDED
    }

    pub fn embedded() -> Tuning {
        Tuning::parse(EMBEDDED).expect("games/maze/maze.ron")
    }

    /// Reads a tuning, and refuses a level the game cannot play (no start, no goal, a stray character) with what
    /// is wrong and where.
    pub fn parse(src: &str) -> Result<Tuning, String> {
        let t: Tuning = ron::from_str(src).map_err(|e| format!("maze.ron: {e}"))?;
        if t.levels.is_empty() {
            return Err("maze.ron: no levels".into());
        }
        for (i, l) in t.levels.iter().enumerate() {
            let at = format!("maze.ron: level {} ({})", i + 1, l.name);
            for (r, row) in l.rows.iter().enumerate() {
                if let Some(ch) = row.chars().find(|c| !"#.oSG".contains(*c)) {
                    return Err(format!("{at}, row {}: '{ch}' is not one of # . o S G", r + 1));
                }
            }
            if l.find(Cell::Start).len() != 1 {
                return Err(format!("{at}: needs exactly one S"));
            }
            if l.find(Cell::Goal).is_empty() {
                return Err(format!("{at}: needs a G"));
            }
        }
        Ok(t)
    }
}
