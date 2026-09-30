//! Driving a track. [`Plan`] is the fastest speed a car can carry at every metre of a line round the track: a
//! quasi-steady-state lap simulation, the standard lap-time estimate of vehicle dynamics (each point's cornering
//! limit from grip, banking and downforce, then a forward pass for how fast the car can accelerate out of each
//! corner and a backward pass for how late it can brake into the next). [`Pilot`] follows a plan with the real
//! dynamics: pure pursuit for the steering (Coulter 1992: aim at a point a little ahead on the line) and a
//! proportional controller for the pedals. Racing AI and the lap-time benchmark stand on both.

use crate::fixed::{Angle, Fx};
use crate::track::{Place, Track};
use crate::vehicle::{Fleet, G, Params};

/// The line a car drives and the speed it can carry along it.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    /// The line: this far left of the centreline, all the way round.
    pub offset: Fx,
    /// Metres between samples along the centreline.
    pub step: Fx,
    /// The speed at each sample, m/s.
    pub speed: Vec<Fx>,
    /// What the plan predicts for a flying lap, seconds.
    pub lap_time: Fx,
}

/// The fastest steady speed through a turn of curvature `k` (1/m along the line) on a slope `bank` toward the
/// inside of the turn. The tyres must supply what the slope does not (in the slope's plane, cos θ·(m·v²·k −
/// m·g·tan θ)), shared between the axles as the weight is (a steady turn: each axle holds its own mass); each
/// axle can supply μ times its load, which the turn itself raises and the downforce raises in its own proportion.
/// The weaker axle sets the limit (the front: understeer; the rear: oversteer), as it does in the dynamics. The
/// driven axle also pushes against drag to hold the speed, from the same grip (the friction circle): miss that
/// and a rear-driven car is planned into power oversteer (measured: it spun in the first turn).
fn corner_limit(p: &Params, k: Fx, bank: Angle, top: Fx) -> Fx {
    if k == Fx::ZERO {
        return top;
    }
    let (sb, cb) = (bank.sin(), bank.cos());
    let fits = |v: Fx| {
        let need = cb * (p.mass * v * v * k.abs() - p.mass * G * sb / cb);
        let pressed = (p.mass * G + need * sb) / cb;
        let down = p.down_k * v * v;
        let front = pressed * p.weight_front + down * p.aero_front;
        let rear = pressed * (Fx::ONE - p.weight_front) + down * (Fx::ONE - p.aero_front);
        let hold = p.drag_k * v * v + p.rolling;
        let fits_axle = |lat: Fx, load: Fx, push: Fx| lat * lat + push * push <= (p.grip * load) * (p.grip * load);
        fits_axle(need * p.weight_front, front, hold * p.drive_front)
            && fits_axle(need * (Fx::ONE - p.weight_front), rear, hold * (Fx::ONE - p.drive_front))
    };
    let (mut lo, mut hi) = (Fx::ZERO, top);
    if fits(hi) {
        return hi;
    }
    for _ in 0..40 {
        let mid = (lo + hi) / 2;
        if fits(mid) { lo = mid } else { hi = mid }
    }
    lo
}

/// Flat out on a straight: where the engine's power meets drag and rolling resistance.
fn top_speed(p: &Params) -> Fx {
    let (mut lo, mut hi) = (Fx::ZERO, Fx::int(200));
    for _ in 0..40 {
        let mid = (lo + hi) / 2;
        if (p.rolling + p.drag_k * mid * mid) * mid < p.power_max { lo = mid } else { hi = mid }
    }
    lo
}

impl Plan {
    /// Plan a lap of `track` for car `p` on the line `offset` metres left of the centreline, sampled every metre.
    pub fn new(track: &Track, p: &Params, offset: Fx) -> Plan {
        let step = Fx::ONE;
        let n = (track.length / step).floor().max(1) as usize;
        let top = top_speed(p);
        let at = |i: usize| step * i as i64;
        // The line's own curvature and length per sample: a turn's inside line is tighter and shorter.
        let (curv, len): (Vec<Fx>, Vec<Fx>) = (0..n)
            .map(|i| {
                let k = track.curvature(at(i));
                let r = if k == Fx::ZERO { Fx::ZERO } else { Fx::ONE / k - offset };
                let kl = if r == Fx::ZERO { Fx::ZERO } else { Fx::ONE / r };
                (kl, step * (Fx::ONE - k * offset))
            })
            .unzip();
        let mut speed: Vec<Fx> = (0..n)
            .map(|i| {
                let bank = track.bank(at(i));
                // A slope helps when it falls toward the inside of the turn (a left turn's inside is the left).
                let toward = if curv[i] >= Fx::ZERO { bank } else { -bank };
                corner_limit(p, curv[i], toward, top)
            })
            .collect();
        // Twice round, so the passes carry over the start line.
        for _ in 0..2 {
            // Forward: accelerate out of each corner (power, or first gear's force, less drag).
            for j in 0..n {
                let (i, next) = (j, (j + 1) % n);
                let v = speed[i];
                // What the driven axle's grip leaves after holding the line, shared with the engine's push.
                let down = p.down_k * v * v;
                let (share, aero) = if p.drive_front > Fx::HALF {
                    (p.weight_front, p.aero_front)
                } else {
                    (Fx::ONE - p.weight_front, Fx::ONE - p.aero_front)
                };
                let axle = p.grip * (p.mass * G * share + down * aero);
                let lateral = p.mass * v * v * curv[i].abs() * share;
                let traction = (axle * axle - lateral * lateral).max(Fx::ZERO).sqrt();
                let engine = p.drive_max.min(p.power_max / v.max(Fx::ONE)).min(traction);
                let push = engine - p.drag_k * v * v - p.rolling;
                let reach = (v * v + push / p.mass * len[i] * 2).max(Fx::ZERO).sqrt();
                speed[next] = speed[next].min(reach);
            }
            // Backward: brake into each corner (the grip the turn leaves, plus drag).
            for j in (0..n).rev() {
                let (i, next) = (j, (j + 1) % n);
                let v = speed[next];
                let down = p.down_k * v * v;
                let grip = p.grip * (p.mass * G + down);
                let lateral = p.mass * v * v * curv[next].abs();
                let brake = (grip * grip - lateral * lateral).max(Fx::ZERO).sqrt() + p.drag_k * v * v;
                let reach = (v * v + brake / p.mass * len[i] * 2).sqrt();
                speed[i] = speed[i].min(reach);
            }
        }
        let lap_time = (0..n).fold(Fx::ZERO, |t, i| {
            let avg = (speed[i] + speed[(i + 1) % n]) / 2;
            t + len[i] / avg.max(Fx::ratio(1, 10))
        });
        Plan { offset, step, speed, lap_time }
    }

    /// The planned speed at `s` (m along the centreline), between samples by interpolation.
    pub fn speed_at(&self, track: &Track, s: Fx) -> Fx {
        let s = track.wrap(s);
        let n = self.speed.len();
        let i = (s / self.step).floor() as usize % n;
        let frac = s - self.step * i as i64;
        let (a, b) = (self.speed[i], self.speed[(i + 1) % n]);
        a + (b - a) * frac / self.step
    }
}

/// Seat car `i` on the track under it: the slope, the direction it falls, and the grip there (the racing surface,
/// or a run-off beyond its edges). Returns where the car is.
pub fn sit_on(fleet: &mut Fleet, i: usize, track: &Track, hint: Option<usize>) -> Place {
    let place = track.locate(fleet.x[i], fleet.y[i], hint);
    let pose = track.pose(place.s, place.offset);
    fleet.bank[i] = Angle(pose.bank.0.abs());
    // A positive bank falls to the left of the direction of travel.
    fleet.bank_dir[i] = pose.heading + if pose.bank.0 >= 0 { Angle::QUARTER } else { -Angle::QUARTER };
    fleet.surface[i] = track.grip(place.offset);
    place
}

/// The nearest car ahead of one the pilot drives: how far ahead along the track (m), how fast it goes (m/s) and
/// where it is across the track (m left of the centreline).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ahead {
    pub gap: Fx,
    pub speed: Fx,
    pub offset: Fx,
}

/// Look this far ahead for a car to pass (m); a car within `DODGE_WIDTH` across the track is in the way, and the
/// pass goes that far to its side, keeping `DODGE_EDGE` from the edge of the track.
const LOOK: Fx = Fx::int(80);
const DODGE_WIDTH: Fx = Fx::ratio(35, 10);
const DODGE_EDGE: Fx = Fx::int(3);
/// Pace off the planned line (a pass), as a share of the plan's speed.
const PASS_PACE: Fx = Fx::ratio(97, 100);
/// Recovery: ticks stalled before backing up, and ticks backing up.
const STALL: i64 = 90;
const BACK: i64 = 60;
/// Following: a gap of 0.6 s of travel plus 8 m, closed at 0.5 m/s per metre of error.
const FOLLOW_TIME: Fx = Fx::ratio(6, 10);
const FOLLOW_MIN: Fx = Fx::int(8);
const FOLLOW_GAIN: Fx = Fx::HALF;

/// The nearest car ahead of each car along the track, within `LOOK`: from each car's place and speed.
pub fn ahead_of(track: &Track, places: &[Place], speeds: &[Fx]) -> Vec<Option<Ahead>> {
    (0..places.len())
        .map(|i| {
            (0..places.len())
                .filter(|&j| j != i)
                .filter_map(|j| {
                    let gap = track.wrap(places[j].s - places[i].s);
                    (gap > Fx::ZERO && gap < LOOK).then_some(Ahead { gap, speed: speeds[j], offset: places[j].offset })
                })
                .min_by_key(|a| (a.gap, a.offset))
        })
        .collect()
}

/// An autopilot for one car.
#[derive(Clone, Debug, PartialEq)]
pub struct Pilot {
    /// Aim this many seconds of travel ahead on the line (never closer than `reach_min` metres). 0.4 s: at 80 m/s
    /// on the oval, 1 s ran 19 m wide of the line, 0.4 s 4 m (games/race/LAPS.md).
    pub reach_time: Fx,
    pub reach_min: Fx,
    /// Drive at this share of the plan's speed (1: the limit; below it, a margin for traffic and mistakes).
    pub pace: Fx,
    /// Pedal per m/s of speed error.
    pub gain: Fx,
    /// Stanley cross-track gain, 1/s. Off by default: at racing speed a degree of steering asks for ~4 g, and
    /// Stanley's direct angle term (built for slow robot cars) swung the car into a spin (measured); pure
    /// pursuit's aim point already pulls it back to the line.
    pub cross: Fx,
}

impl Default for Pilot {
    fn default() -> Pilot {
        Pilot { reach_time: Fx::ratio(4, 10), reach_min: Fx::int(8), pace: Fx::ONE, gain: Fx::ONE, cross: Fx::ZERO }
    }
}

impl Pilot {
    /// Set car `i`'s steering and pedals to follow `plan`; `place` is where the car is (from [`sit_on`]).
    pub fn drive(&self, fleet: &mut Fleet, i: usize, track: &Track, plan: &Plan, place: Place, ahead: Option<Ahead>) {
        let v = fleet.vx[i];
        let p = &fleet.params[i];
        // Racecraft: a car ahead on this line is passed on the side with more room, and if the car cannot get
        // alongside, it follows at a speed-dependent distance (the Intelligent Driver Model's gap idea).
        // Recovering from a spin or a wall: stalled for `STALL` ticks with somewhere to go, back up for `BACK`
        // ticks steering the other way, then drive on.
        let stalled = v.abs() < Fx::ONE;
        if fleet.stuck[i] < 0 {
            fleet.stuck[i] += 1;
        } else if stalled {
            fleet.stuck[i] += 1;
            if fleet.stuck[i] >= STALL {
                fleet.stuck[i] = -BACK;
            }
        } else {
            fleet.stuck[i] = 0;
        }
        let mut backing = fleet.stuck[i] < 0;
        let edge = track.width / 2 - DODGE_EDGE;
        let (line, follow) = match ahead {
            Some(a) if (a.offset - plan.offset).abs() < DODGE_WIDTH && a.gap < LOOK => {
                let inside = (a.offset + DODGE_WIDTH).min(edge);
                let outside = (a.offset - DODGE_WIDTH).max(-edge);
                // The side nearer where this car already is, if there is room on it.
                let pick = if (place.offset - inside).abs() <= (place.offset - outside).abs() { inside } else { outside };
                let alongside = (place.offset - a.offset).abs() >= DODGE_WIDTH - Fx::HALF;
                let safe = a.speed + (a.gap - v * FOLLOW_TIME - FOLLOW_MIN) * FOLLOW_GAIN;
                (pick, if alongside { None } else { Some(safe.max(Fx::ZERO)) })
            }
            _ => (plan.offset, None),
        };
        // Pure pursuit: the arc through a point on the line ahead, curvature 2·sin α / distance, α measured from
        // the car's heading (measuring from the direction of travel feeds back: turning left, the car slides a
        // little right of its heading, the error grows, it steers more; measured, it spun in 3 s). The front
        // wheels turn by atan(wheelbase × curvature), small-angle.
        let reach = (v * self.reach_time).max(self.reach_min);
        let aim = track.pose(place.s + reach, line);
        let (dx, dy) = (aim.x - fleet.x[i], aim.y - fleet.y[i]);
        let alpha = (Angle::atan2(dy, dx) - fleet.yaw[i]).signed();
        // Facing away from the line (after a spin), pure pursuit's sin α is near zero: turn at full lock instead.
        let behind = alpha.0.abs() > Angle::QUARTER.0;
        // Backing out ends once the nose points within 60° of the line (it can drive on), or when time runs out.
        if backing && alpha.0.abs() < Angle::turns(1, 6).0 {
            fleet.stuck[i] = 0;
            backing = false;
        }
        fleet.reverse[i] = backing;
        // Still rolling the wrong way (just out of reverse, or pushed): stop first, then drive.
        let rolling_back = !backing && v < -Fx::HALF;
        let dist = (dx * dx + dy * dy).sqrt().max(Fx::ONE);
        // Pursuit: the arc's curvature, turned into a wheel angle by geometry (δ = L·κ). Plus the understeer the
        // steady turn itself needs, fed forward from the line's own curvature: K·v²·κ_line, K the understeer
        // gradient from each axle's cornering stiffness at its load now (the steady-state inverse of the tested
        // bicycle model). Only the steady turn gets it: multiplying the corrections too raised the loop gain up
        // to 5× at speed.
        let curve = alpha.sin() * 2 / dist;
        let (cf, cr) = (p.cornering * fleet.load_front[i], p.cornering * fleet.load_rear[i]);
        let gradient_v2 = if cf > Fx::ZERO && cr > Fx::ZERO {
            let lean = p.cg_to_rear * cr - p.cg_to_front * cf;
            (p.mass * v * v / p.wheelbase * lean / cf / cr).clamp(-p.wheelbase / 2, p.wheelbase * 4)
        } else {
            Fx::ZERO
        };
        let k_track = track.curvature(place.s + reach / 2);
        let k_line = k_track / (Fx::ONE - k_track * line).max(Fx::HALF);
        let pursue = curve * p.wheelbase + k_line * gradient_v2;
        // Stanley's cross-track term (Thrun et al. 2006): steer back toward the line in proportion to how far
        // off it the car is, gentler the faster it goes.
        let off = place.offset - plan.offset;
        let back = -(Angle::atan2(off * self.cross, v.max(Fx::ONE)).signed().to_radians());
        let wheel = if behind { Fx::int(alpha.0.signum()) } else { (pursue + back) / p.steer_lock.to_radians() };
        // Backing up, the steering works the other way round.
        let wheel = if backing { -Fx::int(alpha.0.signum()) } else { wheel };
        fleet.steer[i] = wheel.clamp(-Fx::ONE, Fx::ONE);
        // Pedals: feed forward the force that holds the planned speed (drag, rolling, and the plan's own
        // acceleration), then correct gently toward it; a driver, not a switch (full throttle at any deficit
        // spun the car: 5.9 kN of push on rear tyres already near their limit).
        let ahead = place.s + v * Fx::ratio(2, 10);
        let want = plan.speed_at(track, ahead) * self.pace;
        let want = follow.map_or(want, |safe| want.min(safe));
        // Off the planned line (passing), a little margin: the plan's limit is for its own line.
        let want = if line == plan.offset { want } else { want * PASS_PACE };
        let slope = (plan.speed_at(track, ahead + Fx::int(5)) - plan.speed_at(track, ahead)) / 5;
        let hold = p.drag_k * v * v + p.rolling + p.mass * slope * v;
        let engine = p.drive_max.min(p.power_max / v.max(Fx::ONE)).max(Fx::ONE);
        let error = want - v;
        let pedal = hold / engine + error * self.gain;
        // Lift as the driven tyres near their limit (clamped first: a big deficit must not outvote the lift) (what a driver feels through the seat, and what traction
        // control does): full throttle below 70 % of the slip where grip peaks, none at the peak.
        let driven = if p.drive_front > Fx::HALF { fleet.slip_front[i] } else { fleet.slip_rear[i] };
        let peak = p.grip / p.cornering;
        let used = driven.abs() / peak;
        let lift = ((Fx::ONE - used) * 10 / 3).clamp(Fx::ZERO, Fx::ONE);
        // Gently in reverse: 670 hp backwards across a track is its own accident (measured: 8 m/s).
        fleet.throttle[i] = if backing {
            Fx::ratio(3, 10)
        } else if rolling_back {
            Fx::ZERO
        } else {
            pedal.clamp(Fx::ZERO, Fx::ONE) * lift
        };
        // The pilot drives with the aids on (a racing driver's feet do what they do).
        fleet.traction_control[i] = true;
        fleet.abs[i] = true;
        fleet.brake[i] = if backing {
            Fx::ZERO
        } else if rolling_back {
            Fx::ONE
        } else {
            (-(error + Fx::ONE) * self.gain).clamp(Fx::ZERO, Fx::ONE)
        };
    }
}
