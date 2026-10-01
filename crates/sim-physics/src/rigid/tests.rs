//! Properties of the solver, each one also a number an eval can track (`scenes`).

use super::scenes::{self, ball, cans, ground, measure, pyramid, stack};
use super::*;

const DT: f32 = 1.0 / 60.0;

fn bits(w: &World) -> Vec<u32> {
    w.iter().flat_map(|(_, b)| [b.pos.x, b.pos.y, b.pos.z, b.rot.x, b.rot.y, b.rot.z, b.rot.w].map(f32::to_bits)).collect()
}

fn top(w: &World) -> &Body {
    w.iter().filter(|(_, b)| !b.fixed).map(|(_, b)| b).max_by(|a, b| a.pos.y.total_cmp(&b.pos.y)).unwrap()
}

#[test]
fn a_box_falls_like_a_parabola() {
    let mut w = World::new();
    let id = w.add(BodyDef::new(Shape::Box { half: V3::splat(0.2) }, V3::new(0.0, 10.0, 0.0)));
    for _ in 0..60 {
        w.step(DT);
    }
    let y = w.get(id).unwrap().pos.y;
    let exact = 10.0 - 0.5 * 9.81;
    // Semi-implicit Euler over 240 substeps lands slightly low: about g·t·h/2.
    assert!((y - exact).abs() < 0.03, "y = {y}, closed form {exact}");
    assert!((w.get(id).unwrap().vel.y + 9.81).abs() < 1e-3);
}

#[test]
fn a_box_comes_to_rest_on_the_ground_without_sinking() {
    let mut w = World::new();
    ground(&mut w);
    let id = w.add(BodyDef::new(Shape::Box { half: V3::splat(0.2) }, V3::new(0.0, 1.0, 0.0)));
    let m = measure(&mut w, 180, DT);
    let b = w.get(id).unwrap();
    assert!(m.max_penetration <= 2.0 * w.tuning.linear_slop + 0.02, "{m:?}");
    assert!((b.pos.y - 0.2).abs() < 2.0 * w.tuning.linear_slop, "rests at {}", b.pos.y);
    assert!(b.asleep, "asleep after 3 s: {m:?}");
    assert!(m.time_to_sleep < 2.5, "{m:?}");
}

#[test]
fn a_tower_spawned_asleep_stays_bit_identical() {
    let mut w = World::new();
    ground(&mut w);
    stack(&mut w, 10, true);
    let before = bits(&w);
    for _ in 0..600 {
        w.step(DT);
    }
    assert_eq!(bits(&w), before);
    assert_eq!(w.stats().awake, 0);
}

#[test]
fn an_awake_stack_of_ten_stands() {
    let mut w = scenes::scene("stack10").unwrap();
    let settle = measure(&mut w, 60, DT);
    let y1 = top(&w).pos.y;
    let m = measure(&mut w, 540, DT);
    // It settles by the sag of soft contacts (about a millimetre a contact), then never creeps.
    assert!(settle.final_drift_top < 0.02, "{settle:?}");
    assert!((top(&w).pos.y - y1).abs() < 0.001, "creeps after settling: {m:?}");
    assert!(m.time_to_sleep < 10.0, "falls asleep: {m:?}");
    assert!(m.max_penetration < 0.02, "{m:?}");
}

#[test]
fn a_pyramid_stands() {
    let mut w = World::new();
    ground(&mut w);
    pyramid(&mut w, 6, false);
    let m = measure(&mut w, 600, DT);
    assert!(m.final_drift_top < 0.015, "{m:?}");
    assert!(m.time_to_sleep < 10.0, "{m:?}");
}

#[test]
fn cans_stack() {
    let mut w = World::new();
    ground(&mut w);
    cans(&mut w, 5, false);
    let m = measure(&mut w, 600, DT);
    assert!(m.final_drift_top < 0.015, "{m:?}");
    assert!(m.time_to_sleep < 10.0, "{m:?}");
}

#[test]
fn a_fast_sphere_never_tunnels() {
    let mut w = World::new();
    ground(&mut w);
    let wall = w.add(BodyDef::new(Shape::Box { half: V3::new(0.5, 0.5, 0.15) }, V3::new(0.0, 0.5, 0.0)));
    let b = ball(&mut w, 0.0, 0.5, 2.0, 40.0);
    let mut touched = false;
    for _ in 0..60 {
        w.step(DT);
        touched |= w.impacts().iter().any(|i| (i.a == wall && i.b == b) || (i.a == b && i.b == wall));
        let z = w.get(b).unwrap().pos.z;
        assert!(touched || z > 0.15, "passed through the wall untouched (z = {z})");
    }
    assert!(touched, "the ball hit the wall");
}

#[test]
fn a_hit_wakes_and_topples_the_tower() {
    let mut w = World::new();
    ground(&mut w);
    stack(&mut w, 10, true);
    let b = ball(&mut w, 0.0, 2.0, 3.0, 25.0);
    let mut woke_on_hit = false;
    for _ in 0..30 {
        w.step(DT);
        if let Some(hit) = w.impacts().iter().find(|i| i.a == b || i.b == b) {
            let other = if hit.a == b { hit.b } else { hit.a };
            let ob = w.get(other).unwrap();
            woke_on_hit = !ob.asleep && ob.vel.length() > 0.5;
            assert!(hit.speed > 20.0, "closing at {}", hit.speed);
            assert!(hit.new);
            break;
        }
    }
    assert!(woke_on_hit, "the struck box moves on the step it is hit");
    for _ in 0..240 {
        w.step(DT);
    }
    let fallen = w.iter().filter(|(_, b)| !b.fixed && b.shape != Shape::Sphere { radius: 0.15 } && b.pos.y < 3.0).count();
    assert!(top(&w).pos.y < 3.6, "the top is down");
    assert!(fallen >= 9, "{fallen}");
}

#[test]
fn same_inputs_same_result() {
    let run = || {
        let mut w = scenes::scene("shot").unwrap();
        for _ in 0..600 {
            w.step(DT);
        }
        bits(&w)
    };
    assert_eq!(run(), run());
}

#[test]
fn removing_a_body_drops_its_contacts_and_handle() {
    let mut w = World::new();
    ground(&mut w);
    let a = w.add(BodyDef::new(Shape::Box { half: V3::splat(0.2) }, V3::new(0.0, 0.2, 0.0)));
    let top = w.add(BodyDef::new(Shape::Box { half: V3::splat(0.2) }, V3::new(0.0, 0.601, 0.0)));
    for _ in 0..120 {
        w.step(DT);
    }
    w.remove(a);
    assert!(w.get(a).is_none());
    let c = w.add(BodyDef::new(Shape::Sphere { radius: 0.1 }, V3::new(5.0, 0.1, 0.0)));
    assert_eq!(c.index, a.index, "the slot is reused");
    assert!(w.get(a).is_none(), "the old handle never finds the new body");
    assert!(w.get(c).is_some());
    for _ in 0..120 {
        w.step(DT);
    }
    let y = w.get(top).unwrap().pos.y;
    assert!((y - 0.2).abs() < 0.02, "the box it held fell to the ground: {y}");
}

#[test]
fn box_box_edge_and_face_contacts() {
    let mut w = World::new();
    ground(&mut w);
    let mut d = BodyDef::new(Shape::Box { half: V3::splat(0.2) }, V3::new(0.0, 0.6, 0.0));
    d.rot = Quat::axis_angle(V3::Z, 0.785) * Quat::axis_angle(V3::X, 0.3);
    let id = w.add(d);
    for _ in 0..300 {
        w.step(DT);
    }
    let b = w.get(id).unwrap();
    // On a face: one of its axes points straight up, and its centre is half a side above the ground.
    let up = (0..3).map(|i| b.rot.mat().c[i].y.abs()).fold(0.0, f32::max);
    assert!(up > 0.999, "lies flat: {up}");
    assert!((b.pos.y - 0.2).abs() < 0.01, "{}", b.pos.y);
    // And a box on a box.
    let mut w = World::new();
    ground(&mut w);
    w.add(BodyDef::new(Shape::Box { half: V3::new(0.5, 0.2, 0.5) }, V3::new(0.0, 0.2, 0.0)));
    let mut d = BodyDef::new(Shape::Box { half: V3::splat(0.15) }, V3::new(0.05, 0.9, 0.0));
    d.rot = Quat::axis_angle(V3::new(1.0, 0.0, 1.0), 0.7);
    let id = w.add(d);
    for _ in 0..300 {
        w.step(DT);
    }
    let b = w.get(id).unwrap();
    assert!((b.pos.y - 0.55).abs() < 0.01, "rests on the slab: {}", b.pos.y);
}

#[test]
fn raycasts_find_the_nearest_surface() {
    let mut w = World::new();
    ground(&mut w);
    let id = w.add(BodyDef::new(Shape::Box { half: V3::splat(0.5) }, V3::new(0.0, 0.5, 0.0)));
    let (hit, t, n) = w.raycast(V3::new(0.0, 0.5, 5.0), V3::new(0.0, 0.0, -1.0), 20.0).unwrap();
    assert_eq!(hit, id);
    assert!((t - 4.5).abs() < 1e-4 && (n - V3::Z).length() < 1e-4);
}

#[test]
fn every_scene_runs_and_stays_above_ground() {
    for name in scenes::NAMES {
        let mut w = scenes::scene(name).unwrap();
        let m = measure(&mut w, 300, DT);
        assert_eq!(m.below_ground, 0.0, "{name}: {m:?}");
        assert!(m.to_json().starts_with("{\"steps\":300"));
    }
}

/// `cargo test -p sim-physics --release print_scene_metrics -- --ignored --nocapture`: every scene's numbers.
#[test]
#[ignore]
fn print_scene_metrics() {
    for name in scenes::NAMES {
        let mut w = scenes::scene(name).unwrap();
        println!("{name}: {}", measure(&mut w, 600, DT).to_json());
    }
    // ~60 bodies awake plus a shot: the step-time target.
    let mut w = World::new();
    ground(&mut w);
    scenes::wall(&mut w, 10, 6, false);
    ball(&mut w, 0.0, 0.8, 4.0, 25.0);
    println!("wall60+shot: {}", measure(&mut w, 600, DT).to_json());
}

#[test]
fn a_sphere_cast_catches_a_graze_that_rays_miss() {
    let mut w = World::new();
    let id = w.add(BodyDef { fixed: true, ..BodyDef::new(Shape::Box { half: V3::splat(0.2) }, V3::ZERO) });
    // A stone of radius 0.17 passing the box's corner diagonally: its centre clears the corner by 0.1, so it touches;
    // rays from its centre and four rim points (up, down, left, right) all pass by.
    let r = 0.17;
    let off = 0.2 + 0.1 / 2f32.sqrt();
    let from = V3::new(off, off, 5.0);
    let dir = V3::new(0.0, 0.0, -1.0);
    for o in [V3::ZERO, V3::X * r, -V3::X * r, V3::Y * r, -V3::Y * r] {
        assert!(w.raycast(from + o, dir, 10.0).is_none(), "the ray from {o:?} misses");
    }
    let (hit, t, n) = w.sphere_cast(from, dir, r, 10.0, 1, u32::MAX).expect("the sphere grazes the corner");
    assert_eq!(hit, id);
    assert!((t - (5.0 - 0.2 - (r * r - 0.01f32).sqrt())).abs() < 0.01, "touches when its rim meets the edge: {t}");
    assert!(n.z > 0.0 && n.x > 0.0 && n.y > 0.0, "the normal points back out of the corner: {n:?}");
    // A layer the box doesn't collide with sees nothing.
    assert!(w.sphere_cast(from, dir, r, 10.0, 1, 0).is_none());
}
