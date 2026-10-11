//! GUMBALL's tuning as data (`games/gumball/gumball.ron`): embedded in the player, and read by the autopilot evals.

use serde::Deserialize;

/// The tuning the player ships with.
pub const EMBEDDED: &str = include_str!("../../../../games/gumball/gumball.ron");

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Tuning {
    pub physics: Physics,
    pub jar: Jar,
    pub balls: Balls,
    pub feel: Feel,
    pub camera: CameraTuning,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Physics {
    pub gravity: f32,
    pub substeps: u32,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Jar {
    pub width: f32,
    pub height: f32,
    pub wall: f32,
    pub rim: f32,
    pub over_time: f32,
    pub friction: f32,
    pub restitution: f32,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Balls {
    pub density: f32,
    pub density_falloff: f32,
    pub friction: f32,
    pub restitution: f32,
    pub smallest: f32,
    pub growth: f32,
    pub sizes: usize,
    pub drop_sizes: usize,
    pub points: Vec<u32>,
    pub bonus: u32,
    pub colours: Vec<u32>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Feel {
    pub reload: f32,
    pub drop_above: f32,
    pub merge_pop: f32,
    pub seed: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct CameraTuning {
    pub fov: f32,
    pub margin: f32,
}

impl Tuning {
    pub fn embedded() -> Tuning {
        Tuning::parse(EMBEDDED).expect("games/gumball/gumball.ron")
    }

    /// Reads a tuning, and refuses one the game cannot play, saying what is wrong.
    pub fn parse(src: &str) -> Result<Tuning, String> {
        let t: Tuning = ron::from_str(src).map_err(|e| format!("gumball.ron: {e}"))?;
        let b = &t.balls;
        if b.sizes < 2 || b.drop_sizes == 0 || b.drop_sizes > b.sizes {
            return Err(format!("gumball.ron: balls.drop_sizes ({}) must be 1 to sizes ({})", b.drop_sizes, b.sizes));
        }
        if b.points.len() != b.sizes || b.colours.len() != b.sizes {
            return Err(format!(
                "gumball.ron: balls.points ({}) and balls.colours ({}) need one entry per size ({})",
                b.points.len(),
                b.colours.len(),
                b.sizes
            ));
        }
        let largest = b.smallest * b.growth.powi(b.sizes as i32 - 1);
        if 2.0 * largest >= t.jar.width {
            return Err(format!("gumball.ron: the largest ball ({:.2} m across) does not fit the jar ({} m)", 2.0 * largest, t.jar.width));
        }
        if t.jar.rim >= t.jar.height {
            return Err(format!("gumball.ron: jar.rim ({}) must be under jar.height ({})", t.jar.rim, t.jar.height));
        }
        Ok(t)
    }

    /// The radius of a ball of `size` (0 is the smallest).
    pub fn radius(&self, size: usize) -> f32 {
        self.balls.smallest * self.balls.growth.powi(size as i32)
    }
}
