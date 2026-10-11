use super::pilot::{Pilot, corners, route};
use super::*;

const LEVEL: Sense = Sense { gravity: None };

fn run(m: &mut Maze, ticks: u32) -> Vec<Pulse> {
    let mut out = Vec::new();
    for _ in 0..ticks {
        m.step(&LEVEL, &mut out);
    }
    out
}

#[test]
fn every_level_parses_and_has_a_way_through() {
    let t = Tuning::embedded();
    for l in &t.levels {
        let path = route(l).unwrap_or_else(|| panic!("{}: no way from S to G", l.name));
        assert!(path.len() > 3, "{}: the goal is not next to the start", l.name);
    }
}

#[test]
fn a_level_with_a_stray_character_is_refused_where_it_is() {
    let src = Tuning::embedded_src().replacen("#S......#", "#S..x...#", 1);
    let e = Tuning::parse(&src).unwrap_err();
    assert!(e.contains("level 1") && e.contains("row 2") && e.contains("'x'"), "{e}");
}

#[test]
fn walls_are_covered_by_few_boxes_and_exactly() {
    let t = Tuning::embedded();
    for l in &t.levels {
        let boxes = wall_boxes(l);
        let (w, h) = l.size();
        let mut count = vec![vec![0; w]; h];
        for &(c0, r0, c1, r1) in &boxes {
            for row in count.iter_mut().take(r1 + 1).skip(r0) {
                for n in row.iter_mut().take(c1 + 1).skip(c0) {
                    *n += 1;
                }
            }
        }
        for (r, row) in count.iter().enumerate() {
            for (c, &n) in row.iter().enumerate() {
                let want = i32::from(l.at(c, r) == tuning::Cell::Wall);
                assert_eq!(n, want, "{}: cell ({c}, {r})", l.name);
            }
        }
        let cells = l.find(tuning::Cell::Wall).len();
        assert!(boxes.len() * 3 < cells, "{}: {} boxes for {} wall cells", l.name, boxes.len(), cells);
    }
}

#[test]
fn a_level_board_keeps_the_marble_still() {
    let mut m = Maze::default();
    run(&mut m, 120);
    let moved = m.marble_pos() - m.start;
    assert!(V3::new(moved.x, 0.0, moved.z).length() < 0.002, "it drifted {moved:?}");
    assert!((m.marble_pos().y - m.t.marble.radius).abs() < 0.01, "it rests on the floor");
}

#[test]
fn tilting_right_rolls_it_right_and_up_rolls_it_up() {
    for (tilt, dir) in [((0.2, 0.0), V3::X), ((0.0, 0.2), V3::new(0.0, 0.0, -1.0))] {
        let mut m = Maze { pilot: Some(tilt), ..Maze::default() };
        run(&mut m, 30);
        let moved = (m.marble_pos() - m.start).dot(dir);
        assert!(moved > 0.05, "{tilt:?}: moved {moved}");
    }
}

#[test]
fn the_tilt_is_capped_at_the_board_maximum() {
    let mut m = Maze { pilot: Some((5.0, 5.0)), ..Maze::default() };
    run(&mut m, 60);
    let max = m.t.physics.max_tilt.to_radians().sin();
    let len = (m.tilt.0 * m.tilt.0 + m.tilt.1 * m.tilt.1).sqrt();
    assert!(len <= max + 1e-4, "{len} > {max}");
}

#[test]
fn the_phone_held_at_any_angle_is_level_until_it_moves() {
    let mut m = Maze::default();
    let mut out = Vec::new();
    // Held at 40 degrees towards the face: that is how it starts, so the board is level.
    let held = Sense { gravity: Some([0, -643, -766]) };
    for _ in 0..60 {
        m.step(&held, &mut out);
    }
    assert!(m.tilt.0.abs() < 1e-4 && m.tilt.1.abs() < 1e-4);
    // Turned 10 degrees to the right: the board tips right and the marble follows.
    let turned = Sense { gravity: Some([174, -633, -754]) };
    for _ in 0..60 {
        m.step(&turned, &mut out);
    }
    assert!(m.tilt.0 > 0.15, "tilt {:?}", m.tilt);
    assert!(m.marble_pos().x > m.start.x + 0.05);
}

#[test]
fn a_dragged_finger_tilts_the_board_without_a_sensor() {
    let mut m = Maze::default();
    let mut out = Vec::new();
    let fit = Fit::new(9.0, 16.0, Rect::new(0.0, 0.0, 1179.0, 2556.0));
    let at = Px::new(600.0, 1500.0);
    m.input(&Gesture::Down(at), &fit, &mut out);
    m.input(&Gesture::Move { from: at, to: Px::new(600.0 + 120.0 * m.scale, 1500.0) }, &fit, &mut out);
    run(&mut m, 30);
    assert!(m.tilt.0 > 0.1, "tilt {:?}", m.tilt);
    m.input(&Gesture::Release { at, velocity: Px::default() }, &fit, &mut out);
    run(&mut m, 60);
    assert!(m.tilt.0.abs() < 0.01, "let go, it levels: {:?}", m.tilt);
}

#[test]
fn a_hole_swallows_the_marble_and_it_comes_back_at_the_start() {
    let mut m = Maze::default();
    // A hole right where the marble is.
    m.holes.push(Hole { at: m.start, cell: (1, 1), goal: false });
    let mut out = Vec::new();
    while m.falls == 0 && m.age < 120 {
        m.step(&LEVEL, &mut out);
    }
    assert_eq!(m.falls, 1);
    assert!(m.log.iter().any(|e| matches!(e, Event::Fell { .. })));
    assert!(out.iter().any(|p| p.kind == Haptic::Fall), "a fall is felt");
    assert!(m.marble.is_none(), "gone for a moment");
    m.holes.pop();
    run(&mut m, 60);
    assert!(m.marble.is_some(), "it is back");
    let off = m.marble_pos() - m.start;
    assert!(V3::new(off.x, 0.0, off.z).length() < 0.02, "back at the start: {off:?}");
}

#[test]
fn the_clock_waits_for_the_first_tilt() {
    let mut m = Maze::default();
    run(&mut m, 120);
    assert_eq!(m.tick, 0, "nobody is playing yet");
    m.pilot = Some((0.1, 0.0));
    run(&mut m, 60);
    assert_eq!(m.tick, 60);
}

#[test]
fn the_goal_wins_and_the_clock_stops() {
    let mut m = Maze::default();
    m.holes.push(Hole { at: m.start, cell: (1, 1), goal: true });
    let out = run(&mut m, 120);
    let won = m.won.expect("won");
    assert_eq!(m.tick, won, "the clock stopped");
    assert_eq!(m.stars(), 3);
    assert!(out.iter().any(|p| p.kind == Haptic::Rise));
}

#[test]
fn knocking_a_wall_is_felt_and_harder_is_stronger() {
    let feel = |tilt: f32| {
        // Roll left into the border wall next to the start.
        let mut m = Maze { pilot: Some((-tilt, 0.0)), ..Maze::default() };
        let out = run(&mut m, 90);
        out.iter().filter(|p| p.kind == Haptic::Thud).map(|p| p.intensity).fold(0.0, f32::max)
    };
    let soft = feel(0.06);
    let hard = feel(0.24);
    assert!(hard > 0.0, "a knock is felt");
    assert!(hard >= soft, "{hard} < {soft}");
}

#[test]
fn corners_keep_only_the_turns_and_the_end() {
    let path = [(1, 1), (2, 1), (3, 1), (3, 2), (3, 3), (4, 3)];
    assert_eq!(corners(&path), vec![(3, 1), (3, 3), (4, 3)]);
}

/// The eval: the autopilot plays every level with the shipped tuning. Each must be won without a fall, in a
/// time a person could beat; the table goes to EVALS.md (`cargo test -p sim-mobile maze_eval -- --nocapture`).
#[test]
fn maze_eval_every_level_is_won_by_the_autopilot() {
    let t = Tuning::embedded();
    println!("| level | won (s) | falls | knocks | hardest knock (m/s) | max penetration (mm) |");
    println!("|---|---|---|---|---|---|");
    for i in 0..t.levels.len() {
        let mut m = Maze::new(t.clone());
        m.go(i as isize);
        let mut pilot = Pilot::new(&m).expect("a route");
        let mut out = Vec::new();
        let mut pen: f32 = 0.0;
        for _ in 0..60 * 90 {
            m.pilot = Some(pilot.tilt(&m));
            m.step(&LEVEL, &mut out);
            pen = pen.max(m.world.stats().max_penetration);
            if m.won.is_some() {
                break;
            }
        }
        let knocks: Vec<f32> = m.log.iter().filter_map(|e| if let Event::Knock { speed, .. } = e { Some(*speed) } else { None }).collect();
        let hardest = knocks.iter().copied().fold(0.0, f32::max);
        println!(
            "| {} {} | {} | {} | {} | {:.2} | {:.1} |",
            i + 1,
            m.level().name,
            m.won.map_or_else(|| "no".into(), |w| format!("{:.1}", w as f32 / 60.0)),
            m.falls,
            knocks.len(),
            hardest,
            pen * 1000.0
        );
        assert!(m.won.is_some(), "{}: not won in 90 s", m.level().name);
        assert_eq!(m.falls, 0, "{}: the autopilot fell", m.level().name);
        assert!(pen < 0.01, "{}: the marble sank {pen} m into the wood", m.level().name);
    }
}
