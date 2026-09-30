//! Named benchmark scenes for the physics layer, each timed stage by stage (docs/research/building-a-physics-engine.md
//! §3.1, §3.3). `tools/perf.py physics` runs it and keeps the history.
//!
//!     simcraft-physics-bench <scene> <cars> <ticks>
//!
//! Prints one JSON line: nanoseconds per car per tick for each stage, and the fleet's hash at the end (an
//! optimisation must not change it). Scenes:
//! - `brake`: every car stops hard from 30 m/s, again and again (grip-limited braking);
//! - `skidpad`: every car holds a steady circle (the turning geometry, trig every tick);
//! - `mixed`: every car gets new random throttle, brake and steering each tick (all paths, no pattern to predict);
//! - `lap <track.ron> <car.ron>`: an autopilot drives a flying lap and times it against the plan's prediction
//!   (the dogfooding benchmark: a real car on a real track has a real lap time to be compared with).

use sim_physics::{Angle, Fleet, Fx, Params, Pilot, Plan, Track, TrackDef, VehicleDef, sit_on};
use std::time::Instant;

fn read<T: serde::de::DeserializeOwned>(path: &str) -> T {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    ron::from_str(&text).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// A flying lap: start on the line at the planned speed, drive two laps, time the second.
fn lap(track_path: &str, car_path: &str) {
    let track = Track::new(&read::<TrackDef>(track_path)).unwrap_or_else(|e| panic!("{track_path}: {e:#?}"));
    let def: VehicleDef = read(car_path);
    let problems = def.problems();
    assert!(problems.is_empty(), "{car_path}: {problems:#?}");
    let p = Params::new(&def);
    let offset = Fx::ZERO;
    let plan = Plan::new(&track, &p, offset);
    let start = track.pose(Fx::ZERO, offset);
    let mut fleet = Fleet::default();
    fleet.add(p, start.x, start.y, start.heading);
    // Already on the line's curve: moving at the planned speed and turning at its rate.
    fleet.vx[0] = plan.speed[0];
    fleet.yaw_rate[0] = plan.speed[0] * track.curvature(Fx::ZERO);
    // SIMCRAFT_PILOT="reach=0.5,pace=0.98,gain=0.1,cross=0,offset=0": probe the pilot's knobs (seconds, shares).
    let mut pilot = Pilot::default();
    let mut offset = offset;
    let mut standing = false;
    let milli = |v: &str| Fx::ratio((v.parse::<f64>().expect("a number") * 1000.0).round() as i64, 1000);
    for kv in std::env::var("SIMCRAFT_PILOT").unwrap_or_default().split(',').filter(|s| !s.is_empty()) {
        let (k, v) = kv.split_once('=').expect("key=value");
        match k {
            "reach" => pilot.reach_time = milli(v),
            "pace" => pilot.pace = milli(v),
            "gain" => pilot.gain = milli(v),
            "cross" => pilot.cross = milli(v),
            "offset" => offset = milli(v),
            "standing" => standing = v == "1",
            other => panic!("unknown pilot knob {other}"),
        }
    }
    let plan = if offset == Fx::ZERO { plan } else { Plan::new(&track, &fleet.params[0], offset) };
    if standing {
        // From rest, 9 m behind the line (a grid slot).
        let grid = track.pose(-Fx::int(9), offset);
        (fleet.x[0], fleet.y[0], fleet.yaw[0]) = (grid.x, grid.y, grid.heading);
        (fleet.vx[0], fleet.yaw_rate[0]) = (Fx::ZERO, Fx::ZERO);
    }
    let (mut hint, mut last_s, mut laps, mut lap_start) = (None, Fx::ZERO, 0, 0usize);
    let (mut worst_off, mut max_g, mut min_v, mut max_v) = (0f64, 0f64, f64::MAX, 0f64);
    let f = |x: Fx| x.0 as f64 / 65536.0;
    let t0 = Instant::now();
    let mut tick = 0usize;
    let mut times = Vec::new();
    // SIMCRAFT_TRACE=1: twice a second, what the car and the pilot are doing (stderr).
    let trace = std::env::var_os("SIMCRAFT_TRACE").is_some();
    while laps < 3 && tick < 60 * 600 {
        let place = sit_on(&mut fleet, 0, &track, hint);
        hint = Some(place.seg);
        if place.s < last_s - track.length / 2 {
            laps += 1;
            times.push((tick - lap_start) as f64 / 60.0);
            lap_start = tick;
        }
        last_s = place.s;
        if laps >= 1 {
            worst_off = worst_off.max(f(place.offset - offset).abs());
            max_g = max_g.max(f(fleet.accel_lat[0]).abs() / 9.80665);
            min_v = min_v.min(f(fleet.vx[0]));
            max_v = max_v.max(f(fleet.vx[0]));
        }
        pilot.drive(&mut fleet, 0, &track, &plan, place);
        if trace && tick.is_multiple_of(30) {
            eprintln!(
                "t {:6.2}  s {:7.1}  off {:6.2}  v {:5.1} (plan {:5.1})  steer {:6.3}  thr {:4.2} brk {:4.2}  vy {:5.2}  r {:6.3}  slip f/r {:6.3} {:6.3}",
                tick as f64 / 60.0,
                f(place.s),
                f(place.offset),
                f(fleet.vx[0]),
                f(plan.speed_at(&track, place.s)),
                f(fleet.steer[0]),
                f(fleet.throttle[0]),
                f(fleet.brake[0]),
                f(fleet.vy[0]),
                f(fleet.yaw_rate[0]),
                f(fleet.slip_front[0]),
                f(fleet.slip_rear[0])
            );
        }
        fleet.step(60);
        tick += 1;
    }
    let wall = t0.elapsed().as_secs_f64();
    let lap = times.get(1).copied().unwrap_or(f64::NAN);
    println!(
        "{{\"scene\":\"lap\",\"track\":\"{}\",\"lap_s\":{lap:.3},\"plan_s\":{:.3},\"avg_mph\":{:.2},\"min_mph\":{:.1},\"max_mph\":{:.1},\"max_lat_g\":{:.2},\"worst_off_line_m\":{worst_off:.2},\"sim_x_realtime\":{:.0},\"hash\":\"{:016x}\"}}",
        track.name,
        f(plan.lap_time),
        f(track.length) / lap * 2.236_936,
        min_v * 2.236_936,
        max_v * 2.236_936,
        max_g,
        tick as f64 / 60.0 / wall,
        fleet.hash()
    );
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let [_, s, t, c] = args.as_slice()
        && s == "lap"
    {
        return lap(t, c);
    }
    let (scene, cars, ticks) = match args.as_slice() {
        [_, s, n, t] => (s.as_str(), n.parse::<usize>().expect("cars"), t.parse::<usize>().expect("ticks")),
        _ => {
            eprintln!("usage: simcraft-physics-bench brake|skidpad|mixed <cars> <ticks>");
            std::process::exit(2);
        }
    };
    let def: VehicleDef = ron::from_str(include_str!("../../../../assets/vehicles/hatchback.ron")).expect("hatchback");
    let p = Params::new(&def);
    let mut fleet = Fleet::default();
    for c in 0..cars as i64 {
        fleet.add(p.clone(), Fx::int(c % 100 * 10), Fx::int(c / 100 * 10), Angle::turns(c, 64));
    }
    let mut seed = 1u64;
    let mut controls = |fleet: &mut Fleet, t: usize| {
        for c in 0..fleet.len() {
            match scene {
                "brake" => {
                    if fleet.vx[c] == Fx::ZERO {
                        fleet.vx[c] = Fx::int(30);
                    }
                    fleet.brake[c] = Fx::ONE;
                }
                "skidpad" => {
                    fleet.vx[c] = Fx::int(12);
                    fleet.steer[c] = Fx::ratio(1, 3);
                }
                "mixed" => {
                    seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
                    let r = (seed >> 33) as i64;
                    fleet.throttle[c] = Fx::ratio(r % 1001, 1000);
                    fleet.brake[c] = if t % 120 > 100 { Fx::ONE } else { Fx::ZERO };
                    fleet.steer[c] = Fx::ratio(r % 2001 - 1000, 1000);
                }
                other => {
                    eprintln!("unknown scene {other}: brake, skidpad or mixed");
                    std::process::exit(2);
                }
            }
        }
    };
    let (mut forces, mut integrate) = (0u128, 0u128);
    for t in 0..ticks {
        controls(&mut fleet, t);
        let a = Instant::now();
        fleet.forces();
        let b = Instant::now();
        fleet.integrate(60);
        let c = Instant::now();
        forces += (b - a).as_nanos();
        integrate += (c - b).as_nanos();
    }
    let per = |ns: u128| ns as f64 / (cars * ticks) as f64;
    println!(
        "{{\"scene\":\"{scene}\",\"n\":{cars},\"ticks\":{ticks},\"stages\":{{\"forces\":{:.2},\"integrate\":{:.2}}},\"total\":{:.2},\"hash\":\"{:016x}\"}}",
        per(forces),
        per(integrate),
        per(forces + integrate),
        fleet.hash()
    );
}
