//! A vehicle simulator whose cars are data (`docs/plans/physics-and-vehicles.md`,
//! `docs/research/driving-physics.md` §6).
//!
//! [`VehicleDef`] is a car as a designer or a spec sheet describes it: whole numbers in the units a spec sheet
//! uses (kg, mm, Nm, rpm, thousandths). [`Params`] is the same car converted once into SI fixed point. [`Fleet`]
//! steps many cars at once, stored as parallel arrays (data-oriented design: "where there is one, there are
//! many"), in named stages that a benchmark can time one by one (`docs/research/building-a-physics-engine.md` §3).
//!
//! Layer 2 of the model (`driving-physics.md` §2): a dynamic bicycle. Each axle's tyres make a lateral force from
//! their slip angle, linear up to the grip limit and then sliding (a friction circle shared with braking and
//! driving), under a load that shifts with braking, acceleration, downforce and banking. Below walking pace, where
//! slip angles mean nothing, it blends into the kinematic bicycle (layer 1).
//!
//! Layer 3, the drivetrain: the engine turns at the wheels' speed through the gear it is in (a slipping clutch
//! holds it near its torque peak when pulling away), makes the torque its dyno curve gives at that rpm (none at the
//! limiter; engine braking off the throttle), multiplied by the gear and final drive. A sequential gearbox shifts
//! by itself (automatic) or on request, cutting the torque for a moment each shift.
//!
//! The car moves on a plane; a banked road enters as its slope (`bank`, `bank_dir`), which the caller reads from
//! the track under each car: gravity pulls it downslope, and the slope presses the tyres harder into a banked
//! turn (N = (m·g + F_lat·sin θ) / cos θ). Sub-steps: the whole step runs `substeps` times a tick (8 by default,
//! 480 Hz), so stiff tyres at low speed stay stable.

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
    /// Cornering stiffness: lateral force per degree of slip, per mille of the tyre's load (~165: the 16–17 % rule
    /// of thumb, `driving-physics.md` §6.3).
    #[serde(default = "cornering")]
    pub cornering: i64,
    /// Grip once sliding, × 1000 of the peak (a tyre past its limit gives a little back: ~850).
    #[serde(default = "slide")]
    pub slide: i64,
}

fn cornering() -> i64 {
    165
}

fn slide() -> i64 {
    850
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
    /// Downforce: lift coefficient × area, m² × 1000 (0: a road car; a stock car ~1500).
    #[serde(default)]
    pub downforce: i64,
    /// Share of the downforce on the front axle, %.
    #[serde(default = "half")]
    pub aero_front: i64,
    /// Share of the braking on the front axle, %.
    #[serde(default = "brake_front")]
    pub brake_front: i64,
    /// Yaw moment of inertia, kg·m² (0: derive it, mass × (0.45 × wheelbase)², §6.3).
    #[serde(default)]
    pub yaw_inertia: i64,
    /// Physics steps per game tick (8 at 60 Hz: 480 Hz).
    #[serde(default = "substeps")]
    pub substeps: i64,
}

fn half() -> i64 {
    50
}

fn brake_front() -> i64 {
    60
}

fn substeps() -> i64 {
    8
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
        if self.drag < 0 || self.frontal_area < 0 || self.rolling < 0 || self.brake_torque < 0 || self.downforce < 0 {
            p.push("drag, frontal_area, rolling, downforce and brake_torque cannot be negative".into());
        }
        for (name, v) in [("aero_front", self.aero_front), ("brake_front", self.brake_front)] {
            if !(0..=100).contains(&v) {
                p.push(format!("{name} is a share, 0..100 % (is {v})"));
            }
        }
        if !(1..=64).contains(&self.substeps) {
            p.push(format!("substeps must be 1..64 (is {})", self.substeps));
        }
        if self.tyre.cornering <= 0 || !(1..=1000).contains(&self.tyre.slide) {
            p.push("tyre.cornering must be above 0 and tyre.slide 1..1000".into());
        }
        p
    }
}

/// A car in SI fixed point (metres, seconds, kg, newtons, radians), derived once from its [`VehicleDef`].
#[derive(Clone, Debug, PartialEq)]
pub struct Params {
    pub mass: Fx,
    /// Yaw moment of inertia, kg·m².
    pub iz: Fx,
    pub wheelbase: Fx,
    /// Centre of gravity to the front and rear axles.
    pub cg_to_front: Fx,
    pub cg_to_rear: Fx,
    pub cg_height: Fx,
    /// Static share of the weight on the front axle.
    pub weight_front: Fx,
    pub steer_lock: Angle,
    /// Force at the wheels in first gear at peak torque, N.
    pub drive_max: Fx,
    /// The engine's peak power at the wheels, W.
    pub power_max: Fx,
    /// Share of the drive on the front axle (FWD 1, RWD 0, AWD ½).
    pub drive_front: Fx,
    /// Full-pedal braking force at the tyres (before grip), N.
    pub brake_max: Fx,
    pub brake_front: Fx,
    /// Peak grip coefficient, and the share left once sliding.
    pub grip: Fx,
    pub slide: Fx,
    /// Cornering stiffness per newton of load, per radian.
    pub cornering: Fx,
    /// ½ρ·Cd·A and ½ρ·Cl·A: drag and downforce are these × v².
    pub drag_k: Fx,
    pub down_k: Fx,
    pub aero_front: Fx,
    /// N.
    pub rolling: Fx,
    pub substeps: i64,
    // The drivetrain: each gear's ratio times the final drive (first gear first), the torque curve (rpm, Nm),
    // idle and redline (rpm), where the torque peaks (a slipping clutch holds the engine there pulling away), the
    // engine braking at the redline (Nm), the efficiency to the wheels and the tyre radius (m).
    pub ratios: Vec<Fx>,
    pub torque: Vec<(i64, i64)>,
    pub idle: Fx,
    pub redline: Fx,
    pub launch: Fx,
    pub engine_brake: Fx,
    pub efficiency: Fx,
    pub radius: Fx,
}

impl Params {
    /// Engine rpm per m/s of road speed in gear `g` (1-based).
    pub fn rpm_per_speed(&self, g: i64) -> Fx {
        let ratio = self.ratios[(g.clamp(1, self.ratios.len() as i64) - 1) as usize];
        ratio * 60 / (self.radius * crate::fixed::TWO_PI)
    }

    /// Force at the wheels in gear `g` at `rpm` with the throttle at `throttle` (0..1), N; negative is engine
    /// braking.
    pub fn wheel_force(&self, g: i64, rpm: Fx, throttle: Fx) -> Fx {
        let ratio = self.ratios[(g.clamp(1, self.ratios.len() as i64) - 1) as usize];
        let torque = if rpm >= self.redline {
            Fx::ZERO
        } else {
            Fx::int(curve(&self.torque, rpm.floor())) * throttle - self.engine_brake * (Fx::ONE - throttle) * rpm / self.redline
        };
        torque * ratio * self.efficiency / self.radius
    }

    pub fn new(d: &VehicleDef) -> Params {
        let mass = Fx::int(d.mass);
        let wheelbase = Fx::ratio(d.wheelbase, 1000);
        let radius = Fx::ratio(d.wheel_radius, 1000);
        let eff = Fx::ratio(d.efficiency, 1000);
        let weight = mass * G;
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
        let weight_front = Fx::ratio(d.weight_front, 100);
        let reach = wheelbase * Fx::ratio(45, 100);
        Params {
            mass,
            iz: if d.yaw_inertia > 0 { Fx::int(d.yaw_inertia) } else { mass * reach * reach },
            wheelbase,
            // The weight on the front is the share of the wheelbase behind the centre of gravity.
            cg_to_front: wheelbase * (Fx::ONE - weight_front),
            cg_to_rear: wheelbase * weight_front,
            cg_height: Fx::ratio(d.cg_height, 1000),
            weight_front,
            steer_lock: Angle::centidegrees(d.steer_lock),
            drive_max: Fx::int(peak_torque) * Fx::ratio(first, 1000) * Fx::ratio(d.final_drive, 1000) * eff / radius,
            power_max,
            drive_front: match d.drive {
                Drive::Fwd => Fx::ONE,
                Drive::Rwd => Fx::ZERO,
                Drive::Awd => Fx::HALF,
            },
            brake_max: Fx::int(d.brake_torque) / radius,
            brake_front: Fx::ratio(d.brake_front, 100),
            grip: Fx::ratio(d.tyre.friction, 1000),
            slide: Fx::ratio(d.tyre.slide, 1000),
            // Per mille per degree → per unit per radian: × 180/π / 1000.
            cornering: Fx::ratio(d.tyre.cornering * 180, 1000) / crate::fixed::TWO_PI * 2,
            drag_k: AIR * Fx::ratio(d.drag, 1000) * Fx::ratio(d.frontal_area, 1000) / 2,
            down_k: AIR * Fx::ratio(d.downforce, 1000) / 2,
            aero_front: Fx::ratio(d.aero_front, 100),
            rolling: weight * Fx::ratio(d.rolling, 1000),
            substeps: d.substeps.clamp(1, 64),
            ratios: d.gears.iter().map(|&g| Fx::ratio(g, 1000) * Fx::ratio(d.final_drive, 1000)).collect(),
            torque: d.engine.torque_curve.clone(),
            idle: Fx::int(d.engine.idle),
            redline: Fx::int(d.engine.redline),
            launch: Fx::int(d.engine.torque_curve.iter().max_by_key(|&&(_, t)| t).map_or(d.engine.idle, |&(r, _)| r)),
            engine_brake: Fx::int(peak_torque) / 10,
            efficiency: eff,
            radius,
        }
    }
}

/// The share of an axle's remaining grip the driver aids let the engine or brakes use.
const AID_MARGIN: Fx = Fx::ratio(85, 100);

/// Slip angles lose their meaning at walking pace: below `SLOW` the car turns like the kinematic bicycle, above
/// `FAST` like the dynamic one, and in between it blends (m/s).
const SLOW: Fx = Fx::int(2);
const FAST: Fx = Fx::int(5);

/// The force one axle's tyres make sideways: linear in the slip angle up to the grip the load and the longitudinal
/// force leave (the friction circle), then falling to the sliding grip over as much slip again.
fn tyre(slip: Fx, load: Fx, long: Fx, p: &Params, grip: Fx) -> Fx {
    if load <= Fx::ZERO {
        return Fx::ZERO;
    }
    let peak = grip * load;
    let avail = (peak * peak - long * long).max(Fx::ZERO).sqrt();
    let stiff = p.cornering * load;
    let linear = -(stiff * slip);
    if linear.abs() <= avail {
        return linear;
    }
    let at_peak = avail / stiff;
    let past = (slip.abs() - at_peak).min(at_peak);
    let f = avail - avail * (Fx::ONE - p.slide) * past / at_peak.max(Fx(1));
    if slip > Fx::ZERO { -f } else { f }
}

/// Many cars, stepped together. One index per car across every array.
#[derive(Clone, Debug, Default)]
pub struct Fleet {
    pub params: Vec<Params>,
    /// Physics steps a tick (the most any car asks for).
    pub substeps: i64,
    // State: the centre of gravity's position (m), heading, and velocity in the car's own frame (forward, left),
    // and how fast it turns (rad/s).
    pub x: Vec<Fx>,
    pub y: Vec<Fx>,
    pub yaw: Vec<Angle>,
    pub vx: Vec<Fx>,
    pub vy: Vec<Fx>,
    pub yaw_rate: Vec<Fx>,
    // Controls, written by whoever drives (a player, a rule): throttle and brake 0..1, steer -1 (right)..1 (left).
    pub throttle: Vec<Fx>,
    pub brake: Vec<Fx>,
    pub steer: Vec<Fx>,
    // Driver aids (keyboard players, the autopilot): traction control caps the drive, ABS the brakes, so each axle
    // keeps a share of its grip for turning (the friction circle: a tyre spending all its grip on pushing or
    // braking has none left to steer with, and a car that loses the rear that way spins).
    pub traction_control: Vec<bool>,
    pub abs: Vec<bool>,
    // The gearbox: the gear engaged (1-based), a shift asked for (+1 up, -1 down; taken and cleared), whether the
    // driver shifts (else it shifts itself), and how many physics steps of the torque cut a shift has left.
    pub gear: Vec<i64>,
    /// Reverse engaged (through first gear's ratio, backwards).
    pub reverse: Vec<bool>,
    /// For a driver that needs memory (the autopilot's recovery): ticks stalled (≥ 0), or reversing (< 0).
    pub stuck: Vec<i64>,
    pub shift: Vec<i64>,
    pub manual: Vec<bool>,
    pub shift_cut: Vec<i64>,
    /// Engine speed, rpm (an output: the dash, the sound).
    pub rpm: Vec<Fx>,
    /// The body as a box for contact: half its length and half its width, m (a caller that knows the car's
    /// footprint sets it; by default ~1.75 × and ~0.72 × the wheelbase).
    pub half: Vec<(Fx, Fx)>,
    /// Impulse the car took from contact (walls, other cars) in the last `contact` pass, N·s: an output for the
    /// game (damage, a penalty) and the camera (a jolt).
    pub impact: Vec<Fx>,
    // The surface under each car, written by whoever knows the track: its slope, the direction it falls, and a
    // grip factor (1 on the racing surface; grass, wet, marbles less).
    pub bank: Vec<Angle>,
    pub bank_dir: Vec<Angle>,
    pub surface: Vec<Fx>,
    // Outputs, for the next step and for anyone who asks (a HUD, the camera, an eval): accelerations in the car's
    // frame (m/s²; sideways, what the driver feels: the tyres' push), slip angles (rad), axle loads (N).
    pub accel_long: Vec<Fx>,
    pub accel_lat: Vec<Fx>,
    pub slip_front: Vec<Fx>,
    pub slip_rear: Vec<Fx>,
    pub load_front: Vec<Fx>,
    pub load_rear: Vec<Fx>,
    /// The tyres' sideways force last step, N (the slope's extra load depends on it; carry it between ticks).
    pub tyre_lat: Vec<Fx>,
    // Stage scratch: net force (car frame) and yaw moment, from `forces` to `integrate`.
    fx: Vec<Fx>,
    fy: Vec<Fx>,
    mz: Vec<Fx>,
}

impl Fleet {
    pub fn len(&self) -> usize {
        self.params.len()
    }

    pub fn is_empty(&self) -> bool {
        self.params.is_empty()
    }

    /// Forward speed, m/s.
    pub fn speed(&self, i: usize) -> Fx {
        self.vx[i]
    }

    /// A new car at rest on flat ground; returns its index.
    pub fn add(&mut self, p: Params, x: Fx, y: Fx, yaw: Angle) -> usize {
        self.substeps = self.substeps.max(p.substeps);
        self.params.push(p);
        for v in [&mut self.x, &mut self.y] {
            v.push(Fx::ZERO);
        }
        let i = self.len() - 1;
        (self.x[i], self.y[i]) = (x, y);
        self.yaw.push(yaw);
        self.traction_control.push(false);
        self.abs.push(false);
        self.gear.push(1);
        self.reverse.push(false);
        self.stuck.push(0);
        self.shift.push(0);
        self.manual.push(false);
        self.shift_cut.push(0);
        let idle = self.params[i].idle;
        self.rpm.push(idle);
        let wb = self.params[i].wheelbase;
        self.half.push((wb * Fx::ratio(875, 1000), wb * Fx::ratio(36, 100)));
        self.impact.push(Fx::ZERO);
        self.bank.push(Angle::ZERO);
        self.bank_dir.push(Angle::ZERO);
        self.surface.push(Fx::ONE);
        for v in [
            &mut self.vx,
            &mut self.vy,
            &mut self.yaw_rate,
            &mut self.throttle,
            &mut self.brake,
            &mut self.steer,
            &mut self.accel_long,
            &mut self.accel_lat,
            &mut self.slip_front,
            &mut self.slip_rear,
            &mut self.load_front,
            &mut self.load_rear,
            &mut self.fx,
            &mut self.fy,
            &mut self.mz,
            &mut self.tyre_lat,
        ] {
            v.push(Fx::ZERO);
        }
        i
    }

    /// One game tick of `1/rate` seconds, in `substeps` physics steps.
    pub fn step(&mut self, rate: i64) {
        let n = self.substeps.max(1);
        for _ in 0..n {
            self.forces();
            self.integrate(rate * n);
        }
    }

    /// Stage 1: loads, tyre forces, and the net force and yaw moment on each car.
    pub fn forces(&mut self) {
        for i in 0..self.len() {
            let p = &self.params[i];
            let (vx, vy, r) = (self.vx[i], self.vy[i], self.yaw_rate[i]);
            let speed2 = vx * vx + vy * vy;
            let delta = p.steer_lock.times(self.steer[i]);
            let (sd, cd) = (delta.sin(), delta.cos());
            let (sb, cb) = (self.bank[i].sin(), self.bank[i].cos());
            // Downslope, seen from the car: its angle from the car's heading.
            let rel = self.bank_dir[i] - self.yaw[i];
            let (sr, cr) = (rel.sin(), rel.cos());

            // Loads: the slope presses harder when the tyres push toward its low side, braking and accelerating
            // move weight between the axles, and downforce adds to both.
            let pressed = (p.mass * G + self.tyre_lat[i] * sr * sb) / cb;
            let shift = p.mass * self.accel_long[i] * p.cg_height / p.wheelbase;
            let down = p.down_k * speed2;
            let nf = (pressed * p.weight_front - shift + down * p.aero_front).max(Fx::ZERO);
            let nr = (pressed * (Fx::ONE - p.weight_front) + shift + down * (Fx::ONE - p.aero_front)).max(Fx::ZERO);
            self.load_front[i] = nf;
            self.load_rear[i] = nr;
            let grip = p.grip * self.surface[i];

            // Driving and braking, each axle capped by its grip.
            // The engine through the gearbox (nothing while a shift cuts the torque).
            let drive = if self.shift_cut[i] > 0 {
                Fx::ZERO
            } else if self.reverse[i] {
                -p.wheel_force(1, self.rpm[i], self.throttle[i]).max(Fx::ZERO)
            } else {
                p.wheel_force(self.gear[i], self.rpm[i], self.throttle[i])
            };
            // Brakes and rolling resistance oppose the motion, whichever way it goes.
            let moving = Fx::int(vx.signum());
            let brake = p.brake_max * self.brake[i] * moving;
            let cap = |f: Fx, n: Fx| f.clamp(-(grip * n), grip * n);
            // The aids: what the axle's grip leaves after the turn it is holding (last step's sideways force,
            // shared as the weight is), less a margin.
            let aid = |f: Fx, n: Fx, share: Fx, on: bool| {
                if !on {
                    return f;
                }
                let lat = self.tyre_lat[i] * share;
                let left = ((grip * n) * (grip * n) - lat * lat).max(Fx::ZERO).sqrt() * AID_MARGIN;
                f.clamp(-left, left)
            };
            let (tc, abs) = (self.traction_control[i], self.abs[i]);
            let (wf, wr) = (p.weight_front, Fx::ONE - p.weight_front);
            let fxf = cap(aid(drive * p.drive_front, nf, wf, tc) - aid(brake * p.brake_front, nf, wf, abs), nf);
            let fxr = cap(aid(drive * (Fx::ONE - p.drive_front), nr, wr, tc) - aid(brake * (Fx::ONE - p.brake_front), nr, wr, abs), nr);

            // Slip angles, small-angle form (velocity ratios, no atan: research §3).
            let ve = vx.max(Fx::ONE);
            let af = (vy + p.cg_to_front * r) / ve - delta.to_radians();
            let ar = (vy - p.cg_to_rear * r) / ve;
            self.slip_front[i] = af;
            self.slip_rear[i] = ar;
            // At walking pace the kinematic constraint turns the car (see `integrate`); slip forces blend in with the
            // same weight, or the two would disagree and push the car along (energy from nowhere).
            let w = ((vx - SLOW) / (FAST - SLOW)).clamp(Fx::ZERO, Fx::ONE);
            let fyf = tyre(af, nf, fxf, p, grip) * w;
            let fyr = tyre(ar, nr, fxr, p, grip) * w;

            // Air and rolling resistance oppose the motion; gravity pulls down the slope (m·g·tan θ on the plane,
            // the tyres' own sideways force spread over the slope by 1/cos θ).
            let speed = speed2.sqrt();
            let rolling = p.rolling * moving;
            let fall = p.mass * G * sb / cb;
            let lat = fyf * cd + fxf * sd + fyr;
            self.tyre_lat[i] = lat;
            self.fx[i] = fxr + fxf * cd - fyf * sd - p.drag_k * speed * vx - rolling + fall * cr;
            self.fy[i] = lat / cb - p.drag_k * speed * vy + fall * sr;
            self.mz[i] = p.cg_to_front * (fyf * cd + fxf * sd) - p.cg_to_rear * fyr;
        }
    }

    /// Stage 2: velocities from forces (semi-implicit Euler), then heading and position.
    pub fn integrate(&mut self, rate: i64) {
        for i in 0..self.len() {
            let p = &self.params[i];
            let (vx, vy, r) = (self.vx[i], self.vy[i], self.yaw_rate[i]);
            let ax = self.fx[i] / p.mass + vy * r;
            let ay = self.fy[i] / p.mass - vx * r;
            let mut nvx = vx + ax / rate;
            let mut nvy = vy + ay / rate;
            let mut nr = r + self.mz[i] / p.iz / rate;
            // Brakes, drag and slopes stop a car; they never turn its motion round. Only the engine in reverse backs
            // it up (and forward gears drive it forward from rest).
            let crossed = (vx > Fx::ZERO && nvx < Fx::ZERO) || (vx < Fx::ZERO && nvx > Fx::ZERO);
            let wrong_way = vx == Fx::ZERO && ((nvx < Fx::ZERO) != self.reverse[i]) && nvx != Fx::ZERO;
            if crossed || wrong_way {
                (nvx, nvy, nr) = (Fx::ZERO, Fx::ZERO, Fx::ZERO);
            }
            // At walking pace, the kinematic bicycle: the car goes where its wheels point.
            let delta = p.steer_lock.times(self.steer[i]);
            let kin_r = nvx * delta.sin() / delta.cos() / p.wheelbase;
            let w = ((nvx - SLOW) / (FAST - SLOW)).clamp(Fx::ZERO, Fx::ONE);
            nr = kin_r + (nr - kin_r) * w;
            nvy = kin_r * p.cg_to_rear + (nvy - kin_r * p.cg_to_rear) * w;
            self.accel_long[i] = (nvx - vx) * rate - nvy * nr;
            // What the driver feels sideways is what the tyres push, not the slope's pull (on a banked turn at its
            // neutral speed, nothing).
            self.accel_lat[i] = self.tyre_lat[i] / p.mass;
            (self.vx[i], self.vy[i], self.yaw_rate[i]) = (nvx, nvy, nr);
            let (c, s) = (self.yaw[i].cos(), self.yaw[i].sin());
            self.x[i] += (nvx * c - nvy * s) / rate;
            self.y[i] += (nvx * s + nvy * c) / rate;
            self.yaw[i] = (self.yaw[i] + Angle(Angle::radians(nr).0 / rate)).wrapped();
            self.gearbox(i, rate);
        }
    }

    /// The gearbox and the engine's speed, after the wheels have moved.
    fn gearbox(&mut self, i: usize, rate: i64) {
        let p = &self.params[i];
        let top = p.ratios.len() as i64;
        let cut = (rate / 20).max(1); // a sequential shift: ~50 ms without torque
        self.shift_cut[i] = (self.shift_cut[i] - 1).max(0);
        let at = |g: i64| self.vx[i] * p.rpm_per_speed(g);
        // The optimal shift point: up when the next gear pulls at least as hard at the wheels (or at the limiter),
        // down when the gear below pulls a clear 10 % harder and has room under the redline (kickdown); the margin
        // keeps it from hunting between two gears.
        let pull = |g: i64| p.wheel_force(g, at(g), Fx::ONE);
        let g = self.gear[i];
        let wanted = if self.reverse[i] {
            1 - g // reverse runs through first
        } else if self.manual[i] {
            std::mem::take(&mut self.shift[i]).signum()
        } else if g < top && (at(g) >= p.redline * Fx::ratio(97, 100) || (at(g + 1) > p.idle && pull(g + 1) >= pull(g))) {
            1
        } else if g > 1 && at(g - 1) < p.redline * Fx::ratio(90, 100) && pull(g - 1) > pull(g) * Fx::ratio(110, 100) {
            -1
        } else {
            0
        };
        if wanted != 0 && (1..=top).contains(&(g + wanted)) && self.shift_cut[i] == 0 {
            self.gear[i] = g + wanted;
            self.shift_cut[i] = cut;
        }
        // Coupled to the wheels, or, pulling away, held up by the slipping clutch toward the torque peak.
        let coupled = if self.reverse[i] { -at(1) } else { at(self.gear[i]) };
        let slipping = p.idle + (p.launch - p.idle) * self.throttle[i];
        self.rpm[i] = coupled.max(slipping).max(p.idle);
    }

    /// A fingerprint of every car's state: two runs agree bit for bit or they differ here (FNV-1a).
    pub fn hash(&self) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for i in 0..self.len() {
            for v in [self.x[i].0, self.y[i].0, self.yaw[i].0, self.vx[i].0, self.vy[i].0, self.yaw_rate[i].0] {
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

    fn deg(a: Angle) -> f64 {
        a.0 as f64 * 360.0 / crate::fixed::TURN as f64
    }

    /// No air and no rolling resistance: nothing changes the car's speed but its brakes and throttle.
    fn ideal() -> Params {
        let mut d = hatchback();
        d.drag = 0;
        d.rolling = 0;
        let mut p = Params::new(&d);
        p.engine_brake = Fx::ZERO;
        p
    }

    fn g() -> f64 {
        f(G)
    }

    #[test]
    fn the_hatchback_file_is_a_valid_car() {
        let d = hatchback();
        assert_eq!(d.problems(), Vec::<String>::new());
        let p = Params::new(&d);
        // ~100 kW at the wheels from a 1.4-litre-turbo-shaped curve; grip-limited in first gear, like a real FWD.
        assert!((90_000.0..130_000.0).contains(&f(p.power_max)), "{}", f(p.power_max));
        assert!(p.drive_max > p.mass * G * p.weight_front * p.grip);
        // The derived yaw inertia: mass × (0.45 × wheelbase)².
        assert!((f(p.iz) - 1300.0 * (0.45f64 * 2.64).powi(2)).abs() < 1.0, "{}", f(p.iz));
    }

    #[test]
    fn a_broken_car_says_everything_wrong_at_once() {
        let mut d = hatchback();
        d.mass = 0;
        d.weight_front = 120;
        d.gears.clear();
        d.engine.torque_curve = vec![(3000, 200), (2000, 250)];
        d.substeps = 0;
        let p = d.problems();
        assert_eq!(p.len(), 5, "{p:#?}");
        let unknown = ron::from_str::<VehicleDef>("(mass: 1, wheel_base: 2)");
        assert!(unknown.is_err(), "a misspelt field is an error, not a default");
    }

    /// Drive in a steady circle and report (radius, lateral acceleration) once it has settled.
    fn circle(p: Params, speed: f64, steer: Fx, ticks: usize) -> (Fleet, f64) {
        let mut fleet = Fleet::default();
        let i = fleet.add(p, Fx::ZERO, Fx::ZERO, Angle::ZERO);
        fleet.vx[i] = Fx((speed * 65536.0) as i64);
        fleet.steer[i] = steer;
        // Cruise control: a steered tyre slows the car (its sideways force leans back), so hold the speed.
        let hold = fleet.vx[i];
        for _ in 0..ticks {
            fleet.step(60);
            fleet.vx[i] = hold;
        }
        let r = f(fleet.vx[i]) / f(fleet.yaw_rate[i]);
        (fleet, r)
    }

    #[test]
    fn at_walking_pace_the_car_follows_its_wheels() {
        // Below 2 m/s it is the kinematic bicycle: radius √(b² + (L / tan δ)²) at the centre of gravity.
        let p = ideal();
        let (l, b) = (f(p.wheelbase), f(p.cg_to_rear));
        let delta = (hatchback().steer_lock as f64 / 100.0).to_radians() * 0.5;
        let (fleet, _) = circle(p, 1.5, Fx::HALF, 60 * 30);
        let expected = (b * b + (l / delta.tan()).powi(2)).sqrt();
        let got = f(fleet.vx[0]) / f(fleet.yaw_rate[0]) * (1.0 + (b / (l / delta.tan())).powi(2)).sqrt();
        assert!((got - expected).abs() < 0.01 * expected, "radius {got} vs {expected}");
    }

    #[test]
    fn a_gentle_turn_obeys_the_linear_bicycle_model() {
        // Steady state of the linear model: yaw rate r = v·δ / (L + K·v²), K = m·(b/Cf − a/Cr) / L, with each
        // axle's stiffness from its load. Downforce weighted to the rear (30 % front vs 61 % of the weight) makes
        // the rear stiffer at speed: the car understeers, K > 0.
        let mut p = ideal();
        p.down_k = AIR * Fx::ratio(1500, 1000) / 2;
        p.aero_front = Fx::ratio(30, 100);
        let v = 25.0;
        let down = f(p.down_k) * v * v;
        let (m, l, a, b) = (f(p.mass), f(p.wheelbase), f(p.cg_to_front), f(p.cg_to_rear));
        let wf = f(p.weight_front);
        let cf = f(p.cornering) * (m * g() * wf + down * 0.3);
        let cr = f(p.cornering) * (m * g() * (1.0 - wf) + down * 0.7);
        let k = m * (b / cf - a / cr) / l;
        assert!(k > 0.0, "understeer gradient {k}");
        let steer = Fx::ratio(1, 30);
        let delta = f(p.steer_lock.times(steer).to_radians());
        let (fleet, _) = circle(p, v, steer, 60 * 20);
        let expected = v * delta / (l + k * v * v);
        let got = f(fleet.yaw_rate[0]);
        assert!((got - expected).abs() < 0.02 * expected, "yaw rate {got} vs {expected} (neutral {})", v * delta / l);
    }

    #[test]
    fn the_limit_on_a_flat_skidpad_is_mu_g() {
        // Wind the steering in slowly at 20 m/s: the most lateral acceleration it ever holds is the grip, μ·g.
        let p = ideal();
        let mu = f(p.grip);
        let mut fleet = Fleet::default();
        fleet.add(p, Fx::ZERO, Fx::ZERO, Angle::ZERO);
        fleet.vx[0] = Fx::int(20);
        let mut best = 0f64;
        for t in 0..60 * 20 {
            fleet.steer[0] = Fx::ratio(t, 60 * 20 * 4);
            fleet.step(60);
            best = best.max(f(fleet.accel_lat[0]));
            fleet.vx[0] = Fx::int(20);
        }
        assert!(best <= mu * g() * 1.01 && best >= mu * g() * 0.9, "held {best} m/s² vs μg {}", mu * g());
    }

    #[test]
    fn on_a_banked_turn_at_its_neutral_speed_the_tyres_rest() {
        // v² = g·R·tan θ: the slope alone holds the car on the circle. It tracks the radius its wheels point at,
        // its tyres push nothing sideways, and they carry m·g / cos θ.
        let p = ideal();
        let (m, l) = (f(p.mass), f(p.wheelbase));
        let (radius, bank) = (200.0, Angle::centidegrees(2400));
        let v = (g() * radius * (deg(bank)).to_radians().tan()).sqrt();
        let lock = f(p.steer_lock.to_radians());
        let steer = Fx(((l / radius).atan() / lock * 65536.0) as i64);
        let mut fleet = Fleet::default();
        fleet.add(p, Fx((radius * 65536.0) as i64), Fx::ZERO, Angle::QUARTER);
        // Already in the turn (a standing start would settle onto a circle about another centre).
        let b = f(fleet.params[0].cg_to_rear);
        fleet.vx[0] = Fx((v * 65536.0) as i64);
        fleet.yaw_rate[0] = Fx((v / radius * 65536.0) as i64);
        fleet.vy[0] = Fx((b * v / radius * 65536.0) as i64);
        fleet.steer[0] = steer;
        fleet.bank[0] = bank;
        let mut worst = 0f64;
        for _ in 0..60 * 30 {
            // Downslope: toward the centre of the circle, wherever the car is.
            fleet.bank_dir[0] = Angle::atan2(-fleet.y[0], -fleet.x[0]);
            fleet.step(60);
            fleet.vx[0] = Fx((v * 65536.0) as i64);
            let r = (f(fleet.x[0]).powi(2) + f(fleet.y[0]).powi(2)).sqrt();
            worst = worst.max((r - radius).abs());
        }
        assert!(worst < 0.02 * radius, "strayed {worst} m from the {radius} m circle");
        let load = f(fleet.load_front[0]) + f(fleet.load_rear[0]);
        let expected = m * g() / deg(bank).to_radians().cos();
        assert!((load - expected).abs() < 0.01 * expected, "tyres carry {load} N vs {expected}");
    }

    #[test]
    fn braking_stops_the_car_where_the_grip_says() {
        // Grip-limited braking with 60 % bias: at best μ·g (both axles at their limit), a little less when one
        // axle's share outruns its load. Straight, and it stays straight.
        let p = ideal();
        let mu = f(p.grip);
        let mut fleet = Fleet::default();
        fleet.add(p, Fx::ZERO, Fx::ZERO, Angle::ZERO);
        fleet.vx[0] = Fx::int(30);
        fleet.brake[0] = Fx::ONE;
        let mut ticks = 0;
        while fleet.vx[0] > Fx::ZERO {
            fleet.step(60);
            ticks += 1;
            assert!(ticks < 60 * 20, "never stopped");
        }
        let best = 900.0 / (2.0 * mu * g());
        let d = f(fleet.x[0]);
        assert!(d >= best * 0.99 && d <= best / 0.85, "stopped after {d} m, best possible {best}");
        assert_eq!(fleet.y[0], Fx::ZERO, "a straight stop stays straight");
    }

    #[test]
    fn top_speed_is_where_the_engine_meets_drag() {
        // Flat out: the speed where the best gear's wheel force (the torque curve at that gear's rpm) meets drag and
        // rolling resistance. The best gear need not be the top one (a tall sixth is an overdrive).
        let mut fleet = Fleet::default();
        let p = Params::new(&hatchback());
        let gears = p.ratios.len() as i64;
        let (k, roll) = (f(p.drag_k), f(p.rolling));
        let force = |g: i64, v: f64| f(p.wheel_force(g, Fx((v * f(p.rpm_per_speed(g)) * 65536.0) as i64), Fx::ONE));
        let best = |v: f64| (1..=gears).max_by(|&a, &b| force(a, v).total_cmp(&force(b, v))).unwrap_or(1);
        let push = |v: f64| force(best(v), v);
        let (mut lo, mut hi) = (0.0, 200.0);
        for _ in 0..100 {
            let v: f64 = (lo + hi) / 2.0;
            if push(v) > roll + k * v * v { lo = v } else { hi = v }
        }
        fleet.add(p.clone(), Fx::ZERO, Fx::ZERO, Angle::ZERO);
        fleet.throttle[0] = Fx::ONE;
        for _ in 0..60 * 240 {
            fleet.step(60);
        }
        assert_eq!(fleet.gear[0], best(lo), "flat out ends in the gear that pulls hardest there");
        assert!((f(fleet.vx[0]) - lo).abs() < 0.02 * lo, "top speed {} m/s vs {lo}", f(fleet.vx[0]));
    }

    #[test]
    fn the_gearbox_climbs_through_every_gear_without_passing_the_redline() {
        // Full throttle from rest: first to top, each upshift where it gains drive, the engine never past the redline.
        let p = Params::new(&hatchback());
        let (red, gears) = (f(p.redline), p.ratios.len() as i64);
        let mut fleet = Fleet::default();
        fleet.add(p, Fx::ZERO, Fx::ZERO, Angle::ZERO);
        fleet.throttle[0] = Fx::ONE;
        let (mut last, mut shifts, mut worst) = (1, Vec::new(), 0f64);
        for _ in 0..60 * 60 {
            fleet.step(60);
            worst = worst.max(f(fleet.rpm[0]));
            if fleet.gear[0] != last {
                shifts.push((last, fleet.gear[0], f(fleet.vx[0])));
                last = fleet.gear[0];
            }
        }
        // It ends in the gear that pulls hardest at the speed it reached.
        let p = &fleet.params[0];
        let v = f(fleet.vx[0]);
        let force = |g: i64| f(p.wheel_force(g, Fx((v * f(p.rpm_per_speed(g)) * 65536.0) as i64), Fx::ONE));
        let best = (1..=gears).max_by(|&a, &b| force(a).total_cmp(&force(b))).unwrap_or(1);
        assert_eq!(last, best, "{shifts:?}");
        assert!(shifts.iter().all(|&(a, b, _)| b == a + 1), "only upshifts, one at a time: {shifts:?}");
        assert!(worst <= red * 1.001, "the engine reached {worst} rpm (redline {red})");
        // Each upshift where the next gear pulls at least as hard, or at the limiter: never one that loses drive.
        for &(from, to, v) in &shifts {
            let pull = |g: i64| f(p.wheel_force(g, Fx(((v * f(p.rpm_per_speed(g))) * 65536.0) as i64), Fx::ONE));
            let rpm = v * f(p.rpm_per_speed(from));
            assert!(pull(to) >= pull(from) * 0.97 || rpm >= red * 0.96, "{from}→{to} at {rpm:.0} rpm lost drive");
        }
    }

    #[test]
    fn off_the_throttle_the_engine_brakes() {
        // Coasting in gear slows the car faster than drag and rolling resistance alone would.
        let coast = |engine: bool| {
            let mut p = Params::new(&hatchback());
            if !engine {
                p.engine_brake = Fx::ZERO;
            }
            let mut fleet = Fleet::default();
            fleet.add(p, Fx::ZERO, Fx::ZERO, Angle::ZERO);
            fleet.vx[0] = Fx::int(25);
            fleet.gear[0] = 3;
            fleet.manual[0] = true;
            for _ in 0..60 * 3 {
                fleet.step(60);
            }
            f(fleet.vx[0])
        };
        let (with, without) = (coast(true), coast(false));
        assert!(with < without - 0.2, "engine braking: {with} m/s vs {without} m/s coasting free");
    }

    #[test]
    fn a_manual_shift_happens_once_per_request() {
        let mut fleet = Fleet::default();
        fleet.add(Params::new(&hatchback()), Fx::ZERO, Fx::ZERO, Angle::ZERO);
        fleet.manual[0] = true;
        fleet.vx[0] = Fx::int(20);
        fleet.shift[0] = 1;
        fleet.step(60);
        assert_eq!(fleet.gear[0], 2);
        fleet.step(60);
        assert_eq!(fleet.gear[0], 2, "a request is taken once, not held");
        fleet.shift[0] = -1;
        fleet.step(60);
        fleet.shift[0] = -1;
        fleet.step(60);
        assert_eq!(fleet.gear[0], 1, "down twice from second: first, and no lower");
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
        assert_eq!(h, 0x0cfa_64f5_5ebb_cbe8, "fleet hash {h:#x}");
    }
}
