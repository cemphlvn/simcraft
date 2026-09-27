use sim_core::{Effect, Engine, Group, Loaded, Running};
use sim_rules::Game;

const GAME: &str = include_str!("../../../games/wolf_sheep/game.ron");
const PANEL: &str = include_str!("../../../games/wolf_sheep/engine.toml");

fn boot(game: &str, panel: &str) -> Result<Engine<Running, Game>, Vec<String>> {
    let (world, g) = Game::from_strs(game, panel).map_err(|e| vec![e])?;
    Ok(Engine::<Loaded, _>::new(world, g).validate()?.start())
}

fn run(panel: &str, ticks: u64) -> (Vec<u64>, Vec<String>) {
    let mut e = boot(GAME, panel).expect("valid");
    let mut hashes = Vec::new();
    let mut events = Vec::new();
    for _ in 0..ticks {
        let r = e.tick();
        hashes.push(r.hash);
        events.extend(r.events.into_iter().map(|ev| ev.name));
    }
    (hashes, events)
}

#[test]
fn same_seed_same_trajectory() {
    let (a, _) = run(PANEL, 300);
    let (b, _) = run(PANEL, 300);
    assert_eq!(a, b, "aynı seed + aynı input → her tick aynı hash");
}

#[test]
fn different_seed_different_trajectory() {
    let (a, _) = run(PANEL, 50);
    let (b, _) = run(&PANEL.replace("seed = 42", "seed = 43"), 50);
    assert_ne!(a.last(), b.last());
}

#[test]
fn switch_off_disables_rule() {
    let (_, on) = run(PANEL, 100);
    let (_, off) = run(&PANEL.replace("predation = true", "predation = false"), 100);
    assert!(on.iter().any(|e| e == "kill"));
    assert!(!off.iter().any(|e| e == "kill"));
}

#[test]
fn unknown_switch_is_rejected() {
    let panel = PANEL.replace("[switches]", "[switches]\npredaton = false");
    let errs = boot(GAME, &panel).err().expect("must fail");
    assert!(errs.iter().any(|e| e.contains("no rule named 'predaton'")), "{errs:?}");
}

#[test]
fn undeclared_param_is_rejected() {
    let panel = PANEL.replace("[params]", "[params]\nwolf_speed = 2");
    let errs = boot(GAME, &panel).err().expect("must fail");
    assert!(errs.iter().any(|e| e.contains("'wolf_speed' is not declared")), "{errs:?}");
}

#[test]
fn unknown_panel_key_is_rejected() {
    let panel = PANEL.replace("[run]", "[run]\nturbo = true");
    assert!(boot(GAME, &panel).is_err());
}

#[test]
fn typo_in_expression_caught_by_dry_run() {
    let game = GAME.replace("me.hunger >= p.starve_at", "me.hungr >= p.starve_at");
    let errs = boot(&game, PANEL).err().expect("must fail");
    assert!(errs.iter().any(|e| e.contains("starvation")), "{errs:?}");
}

#[test]
fn unknown_state_is_rejected() {
    let game = GAME.replace(r#"state: "Hunt""#, r#"state: "Hnt""#);
    let errs = boot(&game, PANEL).err().expect("must fail");
    assert!(errs.iter().any(|e| e.contains("state 'Hnt'")), "{errs:?}");
}

#[test]
fn agent_move_overrides_rule_move() {
    let mut e = boot(GAME, PANEL).expect("valid");
    let wolf = e.world().entities().values().find(|x| x.kind == "wolf").cloned().expect("a wolf");
    let dx = if wolf.x < e.world().width / 2 { 1 } else { -1 };
    e.queue(Group { source: "agent".into(), actor: Some(wolf.id), effects: vec![Effect::Move { e: wolf.id, dx, dy: 0 }] });
    e.tick();
    let after = &e.world().get(wolf.id).unwrap();
    assert_eq!((after.x, after.y), (wolf.x + dx, wolf.y));
}

// --- regression: engine refactors must not change existing games ---

#[test]
fn golden_wolf_sheep_hash() {
    let (hashes, _) = run(PANEL, 300);
    // Recorded before the spatial-grid refactor. Change only on purpose, together with the game.
    assert_eq!(format!("{:016x}", hashes[299]), "ee9a66d10246d6f9");
}

#[test]
fn unknown_field_in_game_is_rejected() {
    let game = GAME.replace("glyph: 's',", "glyph: 's', solidd: true,");
    assert!(boot(&game, PANEL).is_err(), "a typo in game.ron must not be silently ignored");
}

// --- forest fire ---

const FIRE: &str = include_str!("../../../games/forest_fire/game.ron");
const FIRE_PANEL: &str = include_str!("../../../games/forest_fire/engine.toml");

#[test]
fn solid_kinds_fill_one_per_cell() {
    let e = boot(FIRE, FIRE_PANEL).expect("valid");
    let w = e.world();
    for y in 0..w.height {
        for x in 0..w.width {
            assert_eq!(w.at(x, y).len(), 1, "cell ({x},{y})");
        }
    }
}

#[test]
fn forest_grows_and_burns() {
    let mut e = boot(FIRE, FIRE_PANEL).expect("valid");
    let mut lightning = 0;
    for _ in 0..600 {
        lightning += e.tick().events.iter().filter(|ev| ev.name == "lightning").count();
    }
    let trees = e.world().entities().values().filter(|p| p.state == "Tree").count();
    assert!(trees > 0, "trees must grow");
    assert!(lightning > 0, "lightning must strike");
}

#[test]
fn glyph_for_unknown_state_is_rejected() {
    let game = FIRE.replace(r#""Fire": '*'"#, r#""Fier": '*'"#);
    let errs = boot(&game, FIRE_PANEL).err().expect("must fail");
    assert!(errs.iter().any(|e| e.contains("glyph for unknown state 'Fier'")), "{errs:?}");
}
