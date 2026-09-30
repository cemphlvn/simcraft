//! Named benchmark scenes for the physics layer, each timed stage by stage (docs/research/building-a-physics-engine.md
//! §3.1, §3.3). `tools/perf.py physics` runs it and keeps the history.
//!
//!     simcraft-physics-bench <scene> <cars> <ticks>
//!
//! Prints one JSON line: nanoseconds per car per tick for each stage, and the fleet's hash at the end (an
//! optimisation must not change it). Scenes:
//! - `brake`: every car stops hard from 30 m/s, again and again (grip-limited braking);
//! - `skidpad`: every car holds a steady circle (the turning geometry, trig every tick);
//! - `mixed`: every car gets new random throttle, brake and steering each tick (all paths, no pattern to predict).

use sim_physics::{Angle, Fleet, Fx, Params, VehicleDef};
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
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
                    if fleet.speed[c] == Fx::ZERO {
                        fleet.speed[c] = Fx::int(30);
                    }
                    fleet.brake[c] = Fx::ONE;
                }
                "skidpad" => {
                    fleet.speed[c] = Fx::int(12);
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
