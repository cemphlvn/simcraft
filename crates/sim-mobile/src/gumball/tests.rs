use super::*;

const STILL: Sense = Sense { gravity: None };

fn run(g: &mut Gumball, ticks: u32) -> Vec<Pulse> {
    let mut out = Vec::new();
    for _ in 0..ticks {
        g.step(&STILL, &mut out);
    }
    out
}

#[test]
fn the_shipped_tuning_reads_and_a_broken_one_is_refused() {
    let t = Tuning::embedded();
    assert_eq!(t.balls.points.len(), t.balls.sizes);
    let src = tuning::EMBEDDED.replace("growth: 1.26", "growth: 1.6");
    let e = Tuning::parse(&src).unwrap_err();
    assert!(e.contains("does not fit the jar"), "{e}");
}

#[test]
fn two_of_a_size_merge_into_the_next_and_score_it() {
    let mut g = Gumball::default();
    let r = g.radius(2);
    g.add_ball(2, -r - 0.05, r, V3::new(1.0, 0.0, 0.0));
    g.add_ball(2, r + 0.05, r, V3::new(-1.0, 0.0, 0.0));
    let out = run(&mut g, 30);
    assert_eq!(g.balls.len(), 1, "one ball left");
    assert_eq!(g.balls[0].size, 3);
    assert_eq!(g.score, g.t.balls.points[3]);
    assert!(out.iter().any(|p| p.kind == Haptic::Thud), "a merge is felt");
}

#[test]
fn different_sizes_only_touch() {
    let mut g = Gumball::default();
    g.add_ball(1, -0.3, g.radius(1), V3::new(1.0, 0.0, 0.0));
    g.add_ball(2, 0.3, g.radius(2), V3::new(-1.0, 0.0, 0.0));
    run(&mut g, 60);
    assert_eq!(g.balls.len(), 2);
    assert_eq!(g.score, 0);
}

#[test]
fn two_of_the_largest_vanish_for_the_bonus() {
    let mut g = Gumball::default();
    let top = g.t.balls.sizes - 1;
    let r = g.radius(top);
    g.add_ball(top, -r, r, V3::new(0.5, 0.0, 0.0));
    g.add_ball(top, r, r * 3.0, V3::ZERO);
    run(&mut g, 90);
    assert!(g.balls.is_empty());
    assert_eq!(g.score, g.t.balls.bonus);
    assert!(g.log.iter().any(|e| matches!(e, Event::Vanish { .. })));
}

#[test]
fn a_three_way_touch_merges_one_pair_and_leaves_the_third() {
    let mut g = Gumball::default();
    let r = g.radius(0);
    for x in [-2.0 * r, 0.0, 2.0 * r] {
        g.add_ball(0, x, r, V3::ZERO);
    }
    run(&mut g, 1);
    assert_eq!(g.merges, 1);
    assert_eq!(g.balls.iter().filter(|b| b.size == 0).count(), 1);
}

#[test]
fn a_merged_ball_eases_its_neighbours_aside_within_half_a_second() {
    let mut g = Gumball::default();
    // A row of small balls on the floor, two of size 3 in the middle: their merge grows into the row.
    let r0 = g.radius(0);
    for k in 0..4 {
        let x = 0.55 + k as f32 * 2.0 * (r0 + 0.002);
        g.add_ball(k % 2 + 1, x, g.radius(k % 2 + 1), V3::ZERO);
        g.add_ball(k % 2 + 1, -x, g.radius(k % 2 + 1), V3::ZERO);
    }
    run(&mut g, 60);
    let r = g.radius(3);
    g.add_ball(3, -r, r, V3::new(0.6, 0.0, 0.0));
    g.add_ball(3, r, r, V3::new(-0.6, 0.0, 0.0));
    run(&mut g, 5);
    assert!(g.balls.iter().any(|b| b.size == 4), "merged");
    run(&mut g, 30);
    assert!(overlap(&g, 0.0) < 0.02, "still {} m inside", overlap(&g, 0.0));
}

#[test]
fn balls_stay_flat_and_inside_the_glass() {
    let mut g = Gumball { pilot: Some(1.4), ..Gumball::default() };
    run(&mut g, 60 * 20);
    for b in &g.balls {
        let p = g.pos(b);
        assert_eq!(p.z, 0.0);
        assert!(p.x.abs() + g.radius(b.size) <= g.t.jar.width / 2.0 + 0.01, "{p:?}");
        assert!(p.y > 0.0);
    }
}

#[test]
fn the_hand_waits_to_reload_and_keeps_the_ball_inside() {
    let mut g = Gumball::default();
    let mut out = Vec::new();
    g.hand_x = 99.0;
    assert!(g.drop_ball(&mut out));
    assert!(!g.drop_ball(&mut out), "still reloading");
    let p = g.pos(&g.balls[0]);
    assert!(p.x + g.radius(g.balls[0].size) <= g.t.jar.width / 2.0);
    run(&mut g, 60);
    assert!(g.drop_ball(&mut out));
}

#[test]
fn the_same_seed_gives_the_same_balls() {
    let sizes = |g: &mut Gumball| {
        let mut out = Vec::new();
        (0..20)
            .map(|_| {
                let s = g.next;
                g.reload = 0.0;
                g.drop_ball(&mut out);
                s
            })
            .collect::<Vec<_>>()
    };
    let (a, b) = (sizes(&mut Gumball::default()), sizes(&mut Gumball::default()));
    assert_eq!(a, b);
    assert!(a.iter().all(|&s| s < Tuning::embedded().balls.drop_sizes));
    assert!(a.iter().any(|&s| s != a[0]), "not one size only");
}

#[test]
fn a_pile_over_the_rim_ends_the_game_and_a_tap_starts_again() {
    let mut g = Gumball::default();
    // A column of the largest-but-one, taller than the rim.
    let s = g.t.balls.sizes - 3;
    let r = g.radius(s);
    let mut y = r;
    let mut i = 0;
    while y < g.t.jar.rim + r {
        g.add_ball(s + (i % 2), 0.0, y, V3::ZERO);
        y += 2.0 * g.radius(s + 1) + 0.01;
        i += 1;
    }
    let out = run(&mut g, 60 * 6);
    assert!(g.lost.is_some(), "jar full");
    assert!(out.iter().any(|p| matches!(p.kind, Haptic::Buzz { .. })));
    let fit = Fit::new(9.0, 16.0, Rect::new(0.0, 0.0, 1179.0, 2556.0));
    let mut out = Vec::new();
    let at = Px::new(600.0, 1500.0);
    g.input(&Gesture::Down(at), &fit, &mut out);
    g.input(&Gesture::Release { at, velocity: Px::default() }, &fit, &mut out);
    assert!(g.lost.is_none() && g.balls.is_empty(), "a new jar");
}

#[test]
fn a_finger_aims_where_it_is_over_the_jar() {
    let mut g = Gumball::default();
    let cam = look::camera(&g);
    let (px, _, _) = cam.project(V3::new(0.8, g.drop_y(), 0.0), g.screen.0, g.screen.1);
    assert!((g.x_at(px) - 0.8).abs() < 0.01, "{}", g.x_at(px));
    let fit = Fit::new(9.0, 16.0, Rect::new(0.0, 0.0, 1179.0, 2556.0));
    let mut out = Vec::new();
    let at = Px::new(px, g.screen.1 * 0.6);
    g.input(&Gesture::Down(at), &fit, &mut out);
    g.input(&Gesture::Release { at, velocity: Px::default() }, &fit, &mut out);
    assert_eq!(g.drops, 1);
    assert!((g.pos(&g.balls[0]).x - 0.8).abs() < 0.02);
}

/// The deepest two balls overlap (m), counting only balls older than `age` s and, with `resting`, only pairs that
/// are both nearly still: a merge makes its ball inside its neighbours on purpose (the solver eases them apart over
/// the next moments), and a falling ball squashes briefly into the one it lands on.
fn overlap_of(g: &Gumball, age: f32, resting: bool) -> f32 {
    let still = |b: &Ball| !resting || g.world.get(b.id).is_some_and(|x| x.vel.length() < 0.05);
    let mut deepest: f32 = 0.0;
    for (i, a) in g.balls.iter().enumerate() {
        for b in &g.balls[i + 1..] {
            if a.age > age && b.age > age && still(a) && still(b) {
                let d = (g.pos(a) - g.pos(b)).length();
                deepest = deepest.max(g.radius(a.size) + g.radius(b.size) - d);
            }
        }
    }
    deepest
}

fn overlap(g: &Gumball, age: f32) -> f32 {
    overlap_of(g, age, false)
}

/// What a whole jar measured: the deepest overlap of resting balls, and how many ticks some pair of settled balls
/// was squashed more than 2 cm (a landing, a pop).
struct Jar {
    g: Gumball,
    resting: f32,
    squashed: u32,
}

/// One whole jar with the dice seeded `seed`, played by `choose` (or at random), to the end or `max_drops`.
fn play(t: &Tuning, seed: u64, choose: &mut dyn FnMut(&Gumball) -> f32, max_drops: u32) -> Jar {
    let mut t = t.clone();
    t.feel.seed = seed;
    let mut g = Gumball::new(t);
    let mut out = Vec::new();
    let (mut resting, mut squashed): (f32, u32) = (0.0, 0);
    while g.lost.is_none() && g.drops < max_drops {
        g.pilot = (g.reload <= 0.0).then(|| choose(&g));
        g.step(&STILL, &mut out);
        g.pilot = None;
        resting = resting.max(overlap_of(&g, 0.5, true));
        squashed += u32::from(overlap(&g, 0.5) > 0.02);
    }
    // Let the last drop settle.
    run(&mut g, 120);
    Jar { g, resting, squashed }
}

/// The eval: the autopilot plays five jars (five seeds), and a player who drops at random plays the same five, for
/// scale. One jar is one chaotic run; five give a mean and a worst. The table goes to EVALS.md
/// (`cargo test -p sim-mobile --release gumball_eval -- --nocapture`).
#[test]
fn gumball_eval_five_jars() {
    let t = Tuning::embedded();
    let w = t.jar.width / 2.0;
    let seeds = [1, 2, 3, 4, 5];
    let mut rng = Rng::new(3);
    let smart: Vec<Jar> = seeds.iter().map(|&s| play(&t, s, &mut pilot::choose, 400)).collect();
    let random: Vec<Jar> = seeds.iter().map(|&s| play(&t, s, &mut |_| rng.signed() * w, 400)).collect();
    println!("| player (5 jars) | drops | merges | score | biggest size | resting overlap, worst (mm) | squashed ticks |");
    println!("|---|---|---|---|---|---|---|");
    let mean = |js: &[Jar], f: &dyn Fn(&Jar) -> f32| js.iter().map(f).sum::<f32>() / js.len() as f32;
    for (name, js) in [("autopilot", &smart), ("random", &random)] {
        println!(
            "| {name} | {:.0} | {:.0} | {:.0} | {:.1} (max {}) | {:.1} | {:.0} |",
            mean(js, &|j| j.g.drops as f32),
            mean(js, &|j| j.g.merges as f32),
            mean(js, &|j| j.g.score as f32),
            mean(js, &|j| j.g.biggest as f32 + 1.0),
            js.iter().map(|j| j.g.biggest + 1).max().unwrap_or(0),
            js.iter().map(|j| j.resting).fold(0.0, f32::max) * 1000.0,
            mean(js, &|j| j.squashed as f32),
        );
    }
    let score = |js: &[Jar]| mean(js, &|j| j.g.score as f32);
    assert!(score(&smart) > score(&random), "looking before dropping beats dropping at random");
    assert!(smart.iter().all(|j| j.g.biggest >= 6), "the autopilot reaches size 7 in every jar");
    for j in smart.iter().chain(&random) {
        // About 2 % of a mid-size ball's radius: not visible on a phone.
        assert!(j.resting < 0.03, "resting balls sink {} m into each other", j.resting);
        for b in &j.g.balls {
            let p = j.g.pos(b);
            assert!(p.x.abs() + j.g.radius(b.size) <= j.g.t.jar.width / 2.0 + 0.02, "a ball left the jar: {p:?}");
        }
    }
}
