//! A vehicle simulator whose cars are data (`docs/plans/physics-and-vehicles.md`,
//! `docs/research/driving-physics.md` §6).
//!
//! [`VehicleDef`] is a car as a designer or a spec sheet describes it: whole numbers in the units a spec sheet
//! uses (kg, mm, Nm, rpm, thousandths). [`Params`] is the same car converted once into SI fixed point. [`Fleet`]
//! steps many cars at once, stored as parallel arrays (data-oriented design: "where there is one, there are
//! many"), in named stages that a benchmark can time one by one (`docs/research/building-a-physics-engine.md` §3).
//!
//! Layer 1 of the model (`driving-physics.md` §2): a kinematic bicycle. The car turns along the arc its steering
//! and wheelbase draw, and it cannot slide yet. Its longitudinal force is a stand-in for the drivetrain (layer 3):
//! first-gear torque, then the engine's peak power, both capped by the driven tyres' grip.

use crate::fixed::{Angle, Fx, curve};
use serde::{Deserialize, Serialize};

/// Standard gravity, m/s².
pub const G: Fx = Fx::ratio(980_665, 100_000);
/// Air density at sea level, kg/m³.
pub const AIR: Fx = Fx::ratio(1_225, 1_000);

/// Which wheels the engine drives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Drive {
    Fwd,
    Rwd,
    Awd,
}

/// The engine: its torque at each RPM, straight from a dyno sheet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineDef {
    pub idle: i64,
    pub redline: i64,
    /// (rpm, Nm), rpm ascending.
    pub torque_curve: Vec<(i64, i64)>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TyreDef {
    /// Peak grip coefficient × 1000 (1000: a road tyre on dry tarmac; wet ~750, gravel ~500).
    pub friction: i64,
}

/// A car as data. Every number is a whole number in spec-sheet units; comments give where each comes from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VehicleDef {
    /// Kerb weight, kg.
    pub mass: i64,
    /// mm.
    pub wheelbase: i64,
    /// Share of the weight on the front axle at rest, %.
    pub weight_front: i64,
    /// Height of the centre of gravity, mm (load transfer, layer 2).
    pub cg_height: i64,
    /// mm.
    pub track_width: i64,
    /// Loaded tyre radius, mm.
    pub wheel_radius: i64,
    /// The most a front wheel turns each way, hundredths of a degree.
    pub steer_lock: i64,
    pub engine: EngineDef,
    /// Gear ratios × 1000, first gear first.
    pub gears: Vec<i64>,
    /// × 1000.
    pub final_drive: i64,
    /// Engine to wheels, × 1000.
    #[serde(default = "efficiency")]
    pub efficiency: i64,
    pub drive: Drive,
    /// Braking torque of all four brakes at full pedal, Nm.
    pub brake_torque: i64,
    pub tyre: TyreDef,
    /// Drag coefficient × 1000 (a hatchback ~320).
    #[serde(default)]
    pub drag: i64,
    /// Frontal area, m² × 1000.
    #[serde(default)]
    pub frontal_area: i64,
    /// Rolling resistance, × 1000 of the weight (~15 on tarmac).
    #[serde(default = "rolling")]
    pub rolling: i64,
}

fn efficiency() -> i64 {
    900
}

fn rolling() -> i64 {
    15
}

impl VehicleDef {
    /// Everything wrong with this car, all at once (empty: it can drive).
    pub fn problems(&self) -> Vec<String> {
        let mut p = Vec::new();
        let mut positive = |name: &str, v: i64| {
            if v <= 0 {
                p.push(format!("{name} must be above 0 (is {v})"));
            }
        };
        positive("mass", self.mass);
        positive("wheelbase", self.wheelbase);
        positive("track_width", self.track_width);
        positive("wheel_radius", self.wheel_radius);
        positive("steer_lock", self.steer_lock);
        positive("final_drive", self.final_drive);
        positive("efficiency", self.efficiency);
        positive("tyre.friction", self.tyre.friction);
        positive("engine.redline", self.engine.redline);
        if !(1..100).contains(&self.weight_front) {
            p.push(format!("weight_front is a share of the weight, 1..99 % (is {})", self.weight_front));
        }
        if self.steer_lock >= 9000 {
            p.push(format!("steer_lock is per wheel and under 90° (is {} hundredths)", self.steer_lock));
        }
        if self.gears.is_empty() || self.gears.iter().any(|&g| g <= 0) {
            p.push(format!("gears needs at least one ratio, each above 0 (is {:?})", self.gears));
        }
        let c = &self.engine.torque_curve;
        if c.is_empty() {
            p.push("engine.torque_curve needs at least one (rpm, Nm) point".into());
        }
        if c.windows(2).any(|w| w[1].0 <= w[0].0) {
            p.push(format!("engine.torque_curve rpm must rise point by point ({c:?})"));
        }
        if self.drag < 0 || self.frontal_area < 0 || self.rolling < 0 || self.brake_torque < 0 {
            p.push("drag, frontal_area, rolling and brake_torque cannot be negative".into());
        }
        p
    }
}

/// A car in SI fixed point (metres, seconds, kg, newtons, radians), derived once from its [`VehicleDef`].
#[derive(Clone, Debug, PartialEq)]
pub struct Params {
    pub mass: Fx,
    pub wheelbase: Fx,
    /// Centre of gravity to the rear axle.
    pub cg_to_rear: Fx,
    pub steer_lock: Angle,
    /// Force at the wheels in first gear at peak torque, N.
    pub drive_max: Fx,
    /// The engine's peak power at the wheels, W.
    pub power_max: Fx,
    /// Grip of the driven wheels, N.
    pub traction: Fx,
    /// Brakes or grip, whichever gives first, N.
    pub brake_max: Fx,
    /// ½ρ·Cd·A, so drag = this × v².
    pub drag_k: Fx,
    /// N.
    pub rolling: Fx,
}

impl Params {
    pub fn new(d: &VehicleDef) -> Params {
        let mass = Fx::int(d.mass);
        let wheelbase = Fx::ratio(d.wheelbase, 1000);
        let radius = Fx::ratio(d.wheel_radius, 1000);
        let eff = Fx::ratio(d.efficiency, 1000);
        let weight = mass * G;
        let grip = Fx::ratio(d.tyre.friction, 1000);
        let peak_torque = d.engine.torque_curve.iter().map(|&(_, t)| t).max().unwrap_or(0);
        let first = d.gears.first().copied().unwrap_or(1000);
        // Peak power: torque × angular speed, sampled every 50 rpm along the curve (it may peak between points).
        let top = d.engine.redline.max(1);
        let power_max = (0..=top / 50)
            .map(|i| {
                let rpm = i * 50;
                Fx::int(curve(&d.engine.torque_curve, rpm)) * Fx::int(rpm) * crate::fixed::TWO_PI / 60
            })
            .max()
            .unwrap_or(Fx::ZERO)
            * eff;
        let driven_share = match d.drive {
            Drive::Fwd => Fx::ratio(d.weight_front, 100),
            Drive::Rwd => Fx::ratio(100 - d.weight_front, 100),
            Drive::Awd => Fx::ONE,
        };
        Params {
            mass,
            wheelbase,
            cg_to_rear: wheelbase * Fx::ratio(d.weight_front, 100),
            steer_lock: Angle::centidegrees(d.steer_lock),
            drive_max: Fx::int(peak_torque) * Fx::ratio(first, 1000) * Fx::ratio(d.final_drive, 1000) * eff / radius,
            power_max,
            traction: weight * driven_share * grip,
            brake_max: (Fx::int(d.brake_torque) / radius).min(weight * grip),
            drag_k: AIR * Fx::ratio(d.drag, 1000) * Fx::ratio(d.frontal_area, 1000) / 2,
            rolling: weight * Fx::ratio(d.rolling, 1000),
        }
    }
}

/// Below this speed the power limit would divide by almost nothing; first gear's force caps it anyway (m/s).
const CRAWL: Fx = Fx::ratio(1, 2);

/// Many cars, stepped together. One index per car across every array.
#[derive(Clone, Debug, Default)]
pub struct Fleet {
    pub params: Vec<Params>,
    // State: position of the centre of gravity (m), heading, speed along the heading (m/s).
    pub x: Vec<Fx>,
    pub y: Vec<Fx>,
    pub yaw: Vec<Angle>,
    pub speed: Vec<Fx>,
    // Controls, written by whoever drives (a player, a rule): throttle and brake 0..1, steer -1 (right)..1 (left).
    pub throttle: Vec<Fx>,
    pub brake: Vec<Fx>,
    pub steer: Vec<Fx>,
    // Stage outputs, kept for the next stage and for anyone who asks (a HUD, an eval).
    /// Net force along the heading, N.
    pub force: Vec<Fx>,
    /// Turning rate, rad/s.
    pub yaw_rate: Vec<Fx>,
}

impl Fleet {
    pub fn len(&self) -> usize {
        self.params.len()
    }

    pub fn is_empty(&self) -> bool {
        self.params.is_empty()
    }

    /// A new car at rest; returns its index.
    pub fn add(&mut self, p: Params, x: Fx, y: Fx, yaw: Angle) -> usize {
        self.params.push(p);
        self.x.push(x);
        self.y.push(y);
        self.yaw.push(yaw);
        self.speed.push(Fx::ZERO);
        self.throttle.push(Fx::ZERO);
        self.brake.push(Fx::ZERO);
        self.steer.push(Fx::ZERO);
        self.force.push(Fx::ZERO);
        self.yaw_rate.push(Fx::ZERO);
        self.len() - 1
    }

    /// One step of `1/rate` seconds.
    pub fn step(&mut self, rate: i64) {
        self.forces();
        self.integrate(rate);
    }

    /// Stage 1: the net force along each car's heading.
    pub fn forces(&mut self) {
        for i in 0..self.len() {
            let p = &self.params[i];
            let v = self.speed[i];
            let push = p.drive_max.min(p.power_max / v.max(CRAWL)).min(p.traction) * self.throttle[i];
            let brake = p.brake_max * self.brake[i];
            let resist = p.rolling + p.drag_k * v * v;
            self.force[i] = push - if v > Fx::ZERO { brake + resist } else { Fx::ZERO };
        }
    }

    /// Stage 2: speed from force, then heading and position along the arc (kinematic bicycle).
    pub fn integrate(&mut self, rate: i64) {
        for i in 0..self.len() {
            let p = &self.params[i];
            // Semi-implicit Euler: the new speed moves the car. Brakes and drag stop a car; they never reverse it.
            let v = (self.speed[i] + self.force[i] / p.mass / rate).max(Fx::ZERO);
            self.speed[i] = v;
            let delta = p.steer_lock.times(self.steer[i]);
            let tan = delta.sin() / delta.cos();
            let rate_rad = v * tan / p.wheelbase;
            self.yaw_rate[i] = rate_rad;
            // The centre of gravity moves at the slip angle β off the heading; small-angle form (research §3).
            let beta = Angle::radians(p.cg_to_rear * tan / p.wheelbase);
            let dir = self.yaw[i] + beta;
            self.x[i] += v * dir.cos() / rate;
            self.y[i] += v * dir.sin() / rate;
            self.yaw[i] = (self.yaw[i] + Angle(Angle::radians(rate_rad).0 / rate)).wrapped();
        }
    }

    /// A fingerprint of every car's state: two runs agree bit for bit or they differ here (FNV-1a).
    pub fn hash(&self) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for i in 0..self.len() {
            for v in [self.x[i].0, self.y[i].0, self.yaw[i].0, self.speed[i].0] {
                for b in v.to_le_bytes() {
                    h = (h ^ b as u64).wrapping_mul(0x0100_0000_01b3);
                }
            }
        }
        h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hatchback() -> VehicleDef {
        ron::from_str(include_str!("../../../assets/vehicles/hatchback.ron")).expect("hatchback.ron")
    }

    fn f(x: Fx) -> f64 {
        x.0 as f64 / 65536.0
    }

    /// A car on a frictionless plane with no air: nothing slows it but its brakes.
    fn ideal() -> Params {
        let mut d = hatchback();
        d.drag = 0;
        d.rolling = 0;
        Params::new(&d)
    }

    #[test]
    fn the_hatchback_file_is_a_valid_car() {
        let d = hatchback();
        assert_eq!(d.problems(), Vec::<String>::new());
        let p = Params::new(&d);
        // ~100 kW at the wheels from a 1.4-litre-turbo-shaped curve; grip-limited in first gear, like a real FWD.
        assert!((90_000.0..130_000.0).contains(&f(p.power_max)), "{}", f(p.power_max));
        assert!(p.drive_max > p.traction);
    }

    #[test]
    fn a_broken_car_says_everything_wrong_at_once() {
        let mut d = hatchback();
        d.mass = 0;
        d.weight_front = 120;
        d.gears.clear();
        d.engine.torque_curve = vec![(3000, 200), (2000, 250)];
        let p = d.problems();
        assert_eq!(p.len(), 4, "{p:#?}");
        let unknown = ron::from_str::<VehicleDef>("(mass: 1, wheel_base: 2)");
        assert!(unknown.is_err(), "a misspelt field is an error, not a default");
    }

    #[test]
    fn a_steady_turn_draws_the_circle_the_geometry_predicts() {
        let mut fleet = Fleet::default();
        let p = ideal();
        let (l, b) = (f(p.wheelbase), f(p.cg_to_rear));
        let i = fleet.add(p, Fx::ZERO, Fx::ZERO, Angle::ZERO);
        fleet.speed[i] = Fx::int(10);
        fleet.steer[i] = Fx::ratio(1, 4);
        // Expected: the rear axle turns on radius L / tan δ, the centre of gravity on √(b² + R²).
        let delta = (hatchback().steer_lock as f64 / 100.0).to_radians() * 0.25;
        let r = (b * b + (l / delta.tan()).powi(2)).sqrt();
        let ticks = (std::f64::consts::TAU * r / 10.0 * 60.0).round() as usize;
        let (mut lo, mut hi) = (f64::MAX, f64::MIN);
        let mut far = 0f64;
        for _ in 0..ticks {
            fleet.step(60);
            let (x, y) = (f(fleet.x[i]), f(fleet.y[i]));
            far = far.max((x * x + y * y).sqrt());
            lo = lo.min(y);
            hi = hi.max(y);
        }
        // A full lap brings it home, and the circle's diameter is 2R, both within 1 %.
        let home = (f(fleet.x[i]).powi(2) + f(fleet.y[i]).powi(2)).sqrt();
        assert!(home < 0.01 * r * 2.0 * std::f64::consts::PI, "{home} m from the start after a lap");
        assert!(((hi - lo) - 2.0 * r).abs() < 0.01 * 2.0 * r, "diameter {} vs {}", hi - lo, 2.0 * r);
        assert!((far - 2.0 * r).abs() < 0.01 * 2.0 * r, "farthest point {far} vs {}", 2.0 * r);
    }

    #[test]
    fn braking_stops_the_car_where_the_grip_says() {
        let mut fleet = Fleet::default();
        let p = ideal();
        let (decel, v0) = (f(p.brake_max) / f(p.mass), 30.0);
        let i = fleet.add(p, Fx::ZERO, Fx::ZERO, Angle::ZERO);
        fleet.speed[i] = Fx::int(30);
        fleet.brake[i] = Fx::ONE;
        let mut ticks = 0;
        while fleet.speed[i] > Fx::ZERO {
            fleet.step(60);
            ticks += 1;
            assert!(ticks < 60 * 20, "never stopped");
        }
        // v²/2a and v/a, within 1 % (one tick of 1/60 s is ~2 % of a 3 s stop: compare the distance tightly,
        // the time to a tick).
        let d = v0 * v0 / (2.0 * decel);
        assert!((f(fleet.x[i]) - d).abs() < 0.01 * d, "stopped after {} m, expected {d}", f(fleet.x[i]));
        assert!(((ticks as f64 / 60.0) - v0 / decel).abs() <= 1.0 / 60.0 + 1e-9, "{ticks} ticks");
        assert_eq!(fleet.y[i], Fx::ZERO, "a straight stop stays straight");
    }

    #[test]
    fn top_speed_is_where_power_meets_drag() {
        let mut fleet = Fleet::default();
        let p = Params::new(&hatchback());
        let (pw, k, roll) = (f(p.power_max), f(p.drag_k), f(p.rolling));
        let i = fleet.add(p, Fx::ZERO, Fx::ZERO, Angle::ZERO);
        fleet.throttle[i] = Fx::ONE;
        for _ in 0..60 * 240 {
            fleet.step(60);
        }
        // Solve P = (roll + k·v²)·v for v, by bisection.
        let (mut lo, mut hi) = (0.0, 200.0);
        for _ in 0..100 {
            let v: f64 = (lo + hi) / 2.0;
            if (roll + k * v * v) * v < pw { lo = v } else { hi = v }
        }
        assert!((f(fleet.speed[i]) - lo).abs() < 0.02 * lo, "top speed {} m/s vs {lo}", f(fleet.speed[i]));
    }

    #[test]
    fn the_same_drive_gives_the_same_world() {
        let run = || {
            let mut fleet = Fleet::default();
            let p = Params::new(&hatchback());
            for c in 0..50 {
                fleet.add(p.clone(), Fx::int(c * 5), Fx::ZERO, Angle::turns(c, 50));
            }
            let mut seed = 7u64;
            for t in 0..600 {
                for c in 0..fleet.len() {
                    seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
                    let r = (seed >> 33) as i64;
                    fleet.throttle[c] = Fx::ratio(r % 1001, 1000);
                    fleet.brake[c] = if t % 120 > 100 { Fx::ONE } else { Fx::ZERO };
                    fleet.steer[c] = Fx::ratio(r % 2001 - 1000, 1000);
                }
                fleet.step(60);
            }
            fleet.hash()
        };
        let h = run();
        assert_eq!(h, run());
        // The recorded fingerprint: a change here means every car drives differently. Say why before updating it.
        assert_eq!(h, 0x87fb_7d23_703f_4240, "fleet hash {h:#x}");
    }
}
