use sim_core::{Engine, Loaded, Running};
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
    assert!(errs.iter().any(|e| e.contains("no rule or action named 'predaton'")), "{errs:?}");
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
    let args = [("dx".to_string(), dx), ("dy".to_string(), 0)].into();
    let group = e.rules().act(e.world(), None, wolf.id, "move", &args).expect("move is a declared action");
    e.queue(group);
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

// --- mercy dungeon ---

const DUNGEON: &str = include_str!("../../../games/mercy_dungeon/game.ron");
const DUNGEON_PANEL: &str = include_str!("../../../games/mercy_dungeon/engine.toml");

fn hero(e: &Engine<Running, Game>) -> sim_core::Entity {
    e.world().entities().values().find(|x| x.kind == "hero").cloned().expect("a hero")
}

fn no_args() -> std::collections::BTreeMap<String, i64> {
    Default::default()
}

#[test]
fn layout_sets_world_and_places_entities() {
    let e = boot(DUNGEON, DUNGEON_PANEL).expect("valid");
    let w = e.world();
    assert_eq!((w.width, w.height), (22, 10));
    let h = hero(&e);
    assert_eq!((h.x, h.y), (1, 1));
    assert_eq!(w.count("ghost"), 3);
}

#[test]
fn walls_block_solid_movement() {
    let mut e = boot(DUNGEON, DUNGEON_PANEL).expect("valid");
    let h = hero(&e);
    let args = [("dx".to_string(), -1), ("dy".to_string(), 0)].into();
    let g = e.rules().act(e.world(), None, h.id, "move", &args).expect("move");
    e.queue(g);
    e.tick();
    assert_eq!(e.world().get(h.id).map(|x| (x.x, x.y)), Some((1, 1)), "wall at x=0");
}

#[test]
fn action_refused_when_condition_fails() {
    let e = boot(DUNGEON, DUNGEON_PANEL).expect("valid");
    let err = e.rules().act(e.world(), None, hero(&e).id, "fight", &no_args()).expect_err("no ghost adjacent");
    assert!(err.starts_with("refused"), "{err}");
}

#[test]
fn action_errors_are_specific() {
    let e = boot(DUNGEON, DUNGEON_PANEL).expect("valid");
    let id = hero(&e).id;
    let g = e.rules();
    assert!(g.act(e.world(), None, id, "dance", &no_args()).unwrap_err().contains("unknown action"));
    assert!(g.act(e.world(), None, id, "move", &no_args()).unwrap_err().contains("takes args"));
    assert!(g.act(e.world(), None, 1, "move", &no_args()).unwrap_err().contains("not controllable"));
}

#[test]
fn operator_can_switch_off_an_action() {
    let panel = DUNGEON_PANEL.replace("[params]", "[switches]\nspare = false\n\n[params]");
    let e = boot(DUNGEON, &panel).expect("valid");
    let err = e.rules().act(e.world(), None, hero(&e).id, "spare", &no_args()).unwrap_err();
    assert!(err.contains("switched off"), "{err}");
}

#[test]
fn end_condition_finishes_the_game() {
    let empty = DUNGEON.replace(".....g.....", "...........").replace(".g.", "...").replace("#...g", "#....");
    let mut e = boot(&empty, DUNGEON_PANEL).expect("valid");
    assert_eq!(e.outcome(), Some("win"));
    let r = e.tick();
    assert_eq!((r.tick, r.outcome.as_deref()), (0, Some("win")), "a finished game does not tick");
}

#[test]
fn it_without_target_is_rejected() {
    let game = DUNGEON.replace(
        r#"(name: "ghost_dust","#,
        r#"(name: "bad", for: "hero", then: [ On(It, [ Add("hp", "1") ]) ]),
        (name: "ghost_dust","#,
    );
    let errs = boot(&game, DUNGEON_PANEL).err().expect("must fail");
    assert!(errs.iter().any(|e| e.contains("uses It but has no target")), "{errs:?}");
}

#[test]
fn layout_char_not_in_legend_is_rejected() {
    let game = DUNGEON.replace("#H.......#", "#H...?...#");
    let errs = boot(&game, DUNGEON_PANEL).err().expect("must fail");
    assert!(errs.iter().any(|e| e.contains("'?' is not in the legend")), "{errs:?}");
}

#[test]
fn panel_world_must_match_layout() {
    let panel = DUNGEON_PANEL.replace("[params]", "[world]\nwidth = 30\nheight = 10\n\n[params]");
    let errs = boot(DUNGEON, &panel).err().expect("must fail");
    assert!(errs.iter().any(|e| e.contains("does not match game.ron layout")), "{errs:?}");
}

// --- market ---

const MARKET: &str = include_str!("../../../games/market/game.ron");
const MARKET_PANEL: &str = include_str!("../../../games/market/engine.toml");

fn village(e: &Engine<Running, Game>, owner: i64) -> sim_core::Entity {
    let vs = e.world().entities().values();
    vs.filter(|x| x.kind == "village").find(|x| x.props["owner"] == owner).cloned().expect("village")
}

fn n(v: i64) -> std::collections::BTreeMap<String, i64> {
    [("n".to_string(), v)].into()
}

#[test]
fn legend_props_set_owners() {
    let e = boot(MARKET, MARKET_PANEL).expect("valid");
    assert_eq!((village(&e, 1).x, village(&e, 2).x), (2, 15));
}

#[test]
fn need_prevents_double_spend() {
    let rich = MARKET.replace(r#""gold": 30"#, r#""gold": 300"#);
    let mut e = boot(&rich, MARKET_PANEL).expect("valid");
    let a = village(&e, 1).id;
    for _ in 0..2 {
        let g = e.rules().act(e.world(), Some("alice"), a, "buy_stone", &n(8)).expect("stock 10 >= 8 at request time");
        e.queue(g);
    }
    let r = e.tick();
    assert_eq!(r.events.iter().filter(|ev| ev.name == "short").count(), 1, "{:?}", r.events);
    let market = e.world().entities().values().find(|x| x.kind == "market").expect("market");
    assert!(market.props["stone_stock"] >= 0, "stock went negative: {:?}", market.props);
    assert_eq!(e.world().get(a).unwrap().props["stone"], 8);
}

#[test]
fn seats_enforce_ownership() {
    let e = boot(MARKET, MARKET_PANEL).expect("valid");
    let (a, w, g) = (village(&e, 1).id, e.world(), e.rules());
    assert!(g.act(w, None, a, "build", &no_args()).unwrap_err().contains("send \"as\""));
    assert!(g.act(w, Some("bob"), a, "build", &no_args()).unwrap_err().contains("not yours"));
    assert!(g.act(w, Some("eve"), a, "build", &no_args()).unwrap_err().contains("unknown seat"));
}

#[test]
fn scores_are_per_seat() {
    let e = boot(MARKET, MARKET_PANEL).expect("valid");
    let scores = e.rules().scores(e.world());
    assert_eq!(scores, [("alice".to_string(), 30), ("bob".to_string(), 30)].into());
}

#[test]
fn legend_prop_typo_is_rejected() {
    let game = MARKET.replace(r#"("village", { "owner": 1 })"#, r#"("village", { "ownr": 1 })"#);
    let errs = boot(&game, MARKET_PANEL).err().expect("must fail");
    assert!(errs.iter().any(|e| e.contains("has no prop 'ownr'")), "{errs:?}");
}

#[test]
fn seats_need_an_owner_prop() {
    let panel = PANEL.replace("observe_radius = 5", "observe_radius = 5\nseats = { a = 1 }");
    let errs = boot(GAME, &panel).err().expect("must fail");
    assert!(errs.iter().any(|e| e.contains("needs an 'owner' prop")), "{errs:?}");
}

// --- multicore ---

#[test]
fn thread_count_does_not_change_the_world() {
    let run_with = |threads: usize| {
        let panel = FIRE_PANEL.replace("max_ticks = 2000", &format!("max_ticks = 2000\nthreads = {threads}"));
        let mut e = boot(FIRE, &panel).expect("valid");
        (0..150).map(|_| e.tick().hash).collect::<Vec<_>>()
    };
    let one = run_with(1);
    assert_eq!(one, run_with(4));
    assert_eq!(one, run_with(0));
}
