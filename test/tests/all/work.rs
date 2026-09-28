//! What the engine does, counted: expression evaluations, spatial queries, entity maps, per tick and
//! per entity, and the hottest rules. Exact counts (not time): the same on every machine and core count, so they are
//! snapshots. A change that makes a game do more work shows up here as a diff to review: efficient code reveals
//! itself. Accept an intended change with `cargo insta review` (or INSTA_UPDATE=always).

use std::fmt::Write;
use std::path::Path;

use sim_core::{Engine, Loaded, Running};
use sim_rules::Game;

const TICKS: u64 = 60;

fn boot(dir: &Path, threads: Option<usize>) -> Engine<Running, Game> {
    let mut panel = std::fs::read_to_string(dir.join("engine.toml")).expect("a panel");
    if let Some(t) = threads {
        panel = panel.replacen("[run]", &format!("[run]\nthreads = {t}"), 1);
    }
    let (world, game) = Game::load_panel(dir, &panel).expect("loads");
    Engine::<Loaded, _>::new(world, game).validate().expect("valid").start()
}

fn games() -> Vec<std::path::PathBuf> {
    let root = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../games"));
    let mut v: Vec<_> =
        std::fs::read_dir(root).unwrap().filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.join("engine.toml").exists()).collect();
    v.sort();
    v
}

/// The work profile of a game over `TICKS` ticks (after loading and validation).
fn profile(dir: &Path) -> String {
    let mut e = boot(dir, None);
    e.rules().reset_work();
    let mut entity_ticks = 0u64;
    let mut ticks = 0u64;
    for _ in 0..TICKS {
        if e.outcome().is_some() {
            break;
        }
        entity_ticks += e.world().entities().len() as u64;
        e.tick();
        ticks += 1;
    }
    let w = e.rules().work();
    let per = |n: u64| n as f64 / ticks.max(1) as f64;
    let per_e = |n: u64| n as f64 / entity_ticks.max(1) as f64;
    let mut out = String::new();
    writeln!(out, "ticks {ticks}, entities per tick {:.1}", per(entity_ticks)).unwrap();
    writeln!(out, "per tick:        evals {:>9.1}  queries {:>8.1}  maps {:>8.1}", per(w.evals), per(w.queries), per(w.maps)).unwrap();
    writeln!(out, "per entity-tick: evals {:>9.2}  queries {:>8.2}  maps {:>8.2}", per_e(w.evals), per_e(w.queries), per_e(w.maps))
        .unwrap();
    let mut rules = e.rules().rule_work();
    rules.sort_by(|a, b| b.checks.cmp(&a.checks).then(a.name.cmp(&b.name)));
    writeln!(out, "hottest rules (checks per tick, % that fired):").unwrap();
    for r in rules.iter().take(6) {
        let fired = if r.checks == 0 { 0.0 } else { 100.0 * r.fires as f64 / r.checks as f64 };
        writeln!(out, "  {:<24} {:>9.1}  {:>5.1}%", r.name, per(r.checks), fired).unwrap();
    }
    let dead: Vec<&str> = rules.iter().filter(|r| r.checks > 0 && r.fires == 0).map(|r| r.name.as_str()).collect();
    if !dead.is_empty() {
        writeln!(out, "checked but never fired: {}", dead.join(", ")).unwrap();
    }
    out
}

#[test]
fn work_profiles_of_every_game() {
    let mut settings = insta::Settings::clone_current();
    settings.set_snapshot_path(simtest::repo_root().join("test/snapshots"));
    settings.set_prepend_module_to_snapshot(false);
    settings.bind(|| {
        for dir in games() {
            let name = dir.file_name().unwrap().to_string_lossy().to_string();
            insta::assert_snapshot!(format!("work__{name}"), profile(&dir));
        }
    });
}

#[test]
fn work_counts_do_not_depend_on_the_core_count() {
    let dir = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../games/colony"));
    let run = |threads| {
        let mut e = boot(dir, Some(threads));
        assert!(e.world().entities().len() > 256, "large enough for the parallel path");
        for _ in 0..30 {
            e.tick();
        }
        (e.rules().work(), e.rules().rule_work())
    };
    assert_eq!(run(1), run(0));
}

#[test]
fn fast_paths_change_nothing() {
    // Native guards and skipped idle kinds are shortcuts: with them off, every game must give the same world on
    // every tick. A shortcut that ever disagrees with the interpreter fails here, not in a player's game.
    for dir in games() {
        let (mut fast, mut slow) = (boot(&dir, None), boot(&dir, None));
        slow.rules().set_fast_paths(false);
        for t in 0..400 {
            if fast.outcome().is_some() {
                break;
            }
            assert_eq!(fast.tick().hash, slow.tick().hash, "{} diverges at tick {t}", dir.display());
        }
    }
}
