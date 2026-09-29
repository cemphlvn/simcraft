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
    assert_eq!(a, b, "same seed + same input → same hash every tick");
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

// --- event bus ---

use std::sync::{Arc, Mutex};

use sim_core::{Filter, Msg};

/// Plays a short market match while recording the bus, like the host does.
fn record_market(ticks: u64) -> Vec<Msg> {
    let rec: Arc<Mutex<Vec<Msg>>> = Arc::default();
    let mut e = boot(MARKET, MARKET_PANEL).expect("valid");
    e.bus().subscribe(Filter::All, Box::new(rec.clone()));
    let (a, b) = (village(&e, 1).id, village(&e, 2).id);
    for t in 0..ticks {
        let plan = [(Some("alice"), a, "sell_wood"), (Some("bob"), b, "buy_wood")];
        for (seat, id, action) in plan.into_iter().filter(|_| t % 5 == 4) {
            let tick = e.world().tick;
            let out = e.rules().act(e.world(), seat, id, action, &n(1));
            e.bus().publish(&Msg::Act {
                tick,
                seat: seat.map(String::from),
                entity: id,
                action: action.into(),
                args: n(1),
                ok: out.is_ok(),
                error: out.as_ref().err().cloned(),
            });
            if let Ok(g) = out {
                e.queue(g);
            }
        }
        e.tick();
    }
    rec.lock().unwrap().clone()
}

#[test]
fn bus_log_replays_to_the_same_hashes() {
    let log = record_market(40);
    assert!(log.iter().any(|m| matches!(m, Msg::Act { ok: true, .. })), "the match must contain accepted acts");
    let mut e = boot(MARKET, MARKET_PANEL).expect("valid");
    let r = sim_rules::replay(&mut e, &log).expect("replay");
    assert_eq!(r.ticks, 40);
}

#[test]
fn tampered_log_is_detected() {
    let mut log = record_market(40);
    let act = log.iter_mut().find_map(|m| match m {
        Msg::Act { ok: true, args, .. } => Some(args),
        _ => None,
    });
    act.expect("an accepted act").insert("n".into(), 2);
    let mut e = boot(MARKET, MARKET_PANEL).expect("valid");
    let err = sim_rules::replay(&mut e, &log).expect_err("must diverge");
    assert!(err.contains("diverged"), "{err}");
}

#[test]
fn bus_filter_delivers_only_named_messages() {
    let kills: Arc<Mutex<Vec<Msg>>> = Arc::default();
    let mut e = boot(GAME, PANEL).expect("valid");
    e.bus().subscribe(Filter::only(["kill"]), Box::new(kills.clone()));
    for _ in 0..100 {
        e.tick();
    }
    let got = kills.lock().unwrap();
    assert!(!got.is_empty());
    assert!(got.iter().all(|m| matches!(m, Msg::Event(ev) if ev.name == "kill")));
}

// --- gamedev (state charts) ---

const DEV: &str = include_str!("../../../games/gamedev/game.ron");
const DEV_PANEL: &str = include_str!("../../../games/gamedev/engine.toml");

fn dev_run(panel: &str, ticks: u64) -> (Engine<Running, Game>, Vec<String>) {
    let mut e = boot(DEV, panel).expect("valid");
    let mut events = Vec::new();
    for _ in 0..ticks {
        events.extend(e.tick().events.into_iter().map(|ev| ev.name));
    }
    (e, events)
}

fn states_of(e: &Engine<Running, Game>, kind: &str) -> Vec<String> {
    e.world().entities().values().filter(|x| x.kind == kind).map(|x| e.rules().state_label(x).to_string()).collect()
}

#[test]
fn gamedev_layers_start_side_by_side() {
    let e = boot(DEV, DEV_PANEL).expect("valid");
    // Born at tick 0 (midnight) in `initial`: Awake, whose Work picks its first option.
    assert!(states_of(&e, "dev").iter().all(|s| s == "Life.Awake.Work.Commute|Mood.Motivated"), "{:?}", states_of(&e, "dev"));
}

#[test]
fn gamedev_walls_interrupt_and_refactors_resume() {
    let (e, events) = dev_run(DEV_PANEL, 24 * 20);
    let has = |n: &str| events.iter().any(|x| x == n);
    assert!(has("wall") && has("feature") && has("ship"), "every project hits a wall, the engine grows, games ship");
    assert!(!events.iter().any(|x| x.starts_with("error")), "{events:?}");
    let engine = e.world().entities().values().find(|x| x.kind == "engine").expect("engine");
    assert!(engine.props["features"] > 0);
}

#[test]
fn gamedev_hacking_culture_breeds_debt() {
    let (e, events) = dev_run(&DEV_PANEL.replace("care = 6", "care = 0"), 24 * 20);
    let engine = e.world().entities().values().find(|x| x.kind == "engine").expect("engine");
    assert!(events.iter().any(|x| x == "hack"));
    assert_eq!(engine.props["features"], 0, "nobody refactors");
    assert!(engine.props["debt"] > 0);
}

#[test]
fn golden_gamedev_hash() {
    let (e, _) = dev_run(DEV_PANEL, 24 * 30);
    // Recorded when game 4 was written. Change only on purpose, together with the game.
    assert_eq!(format!("{:016x}", e.world().hash()), GAMEDEV_GOLDEN);
}
const GAMEDEV_GOLDEN: &str = "aaf82a4a1c4a2ba7";

#[test]
fn state_rules_bind_by_inheritance() {
    // `awake_drain` lives in Awake: it runs deep inside Awake, never while Asleep.
    let (e, _) = dev_run(DEV_PANEL, 3);
    let asleep = e.world().entities().values().filter(|x| x.kind == "dev").all(|x| x.state.starts_with("Life.Asleep"));
    assert!(asleep, "midnight: everyone goes to sleep");
    let before: Vec<i64> = e.world().entities().values().filter(|x| x.kind == "dev").map(|x| x.props["energy"]).collect();
    let (e2, _) = dev_run(DEV_PANEL, 4);
    let after: Vec<i64> = e2.world().entities().values().filter(|x| x.kind == "dev").map(|x| x.props["energy"]).collect();
    assert!(after.iter().zip(&before).all(|(a, b)| a >= b), "no drain while asleep");
}

#[test]
fn switching_off_a_machine_rule_turns_it_off_at_every_mount() {
    let panel = DEV_PANEL.replace("[agent]", "[switches]\nconcentrate = false\n\n[agent]");
    let (e, _) = dev_run(&panel, 24 * 3);
    assert!(e.world().entities().values().filter(|x| x.kind == "dev").all(|x| !x.state.contains("Flow")));
}

#[test]
fn state_chart_mistakes_are_reported() {
    let cases = [
        (DEV.replace(r#"Interrupt("Cleanup")"#, r#"Interrupt("Cleenup")"#), "Interrupt to unknown state 'Cleenup'"),
        (DEV.replace(r#"(name: "concentrate", then"#, r#"(name: "concentrate", for: "dev", then"#), "drop `for`"),
        (DEV.replace(r#""Design":   (use: "focus""#, r#""Design":   (use: "fokus""#), "unknown machine 'fokus'"),
        (
            DEV.replace(r#"On(NearestIn("project", "Blocked")"#, r#"On(NearestIn("project", "Blokked")"#),
            "state 'Blokked' does not exist for 'project'",
        ),
        (DEV.replace(r#"initial: "Warmup","#, r#"initial: "Warm","#), "initial 'Warm' is not one of its states"),
        (DEV.replace(r#""Vacation": 'v'"#, r#""Vacashun": 'v'"#), "glyph for unknown state 'Vacashun'"),
        (
            DEV.replace(r#"(from: "Stuck", back: true,"#, r#"(from: "Stuck", to: "Work", back: true,"#),
            "exactly one of `to` and `back: true`",
        ),
    ];
    for (game, want) in &cases {
        let errs = boot(game, DEV_PANEL).err().unwrap_or_else(|| panic!("must fail: {want}"));
        assert!(errs.iter().any(|e| e.contains(want)), "want '{want}' in {errs:?}");
    }
}

#[test]
fn ambiguous_goto_needs_a_path() {
    // `Flow` is mounted four times (Design, Build, Playtest, Refactor).
    let game = DEV.replace(r#"Emit("coffee_break") ]"#, r#"Emit("coffee_break"), Goto("Flow") ]"#);
    let errs = boot(&game, DEV_PANEL).err().expect("must fail");
    assert!(errs.iter().any(|e| e.contains("'Flow' is ambiguous")), "{errs:?}");
    let game = DEV.replace(r#"Emit("coffee_break") ]"#, r#"Emit("coffee_break"), Goto("Build.Flow") ]"#);
    assert!(boot(&game, DEV_PANEL).is_ok(), "a path is unique");
}

// --- snapshot / restore ---

#[test]
fn snapshot_restores_the_exact_future() {
    for (game, panel) in [(GAME, PANEL), (DEV, DEV_PANEL), (MARKET, MARKET_PANEL)] {
        let mut a = boot(game, panel).expect("valid");
        for _ in 0..50 {
            a.tick();
        }
        // Through JSON, the way a host (Unity, Unreal, a save file) would keep it.
        let json = serde_json::to_string(&a.snapshot()).expect("serializes");
        let future: Vec<u64> = (0..50).map(|_| a.tick().hash).collect();

        let mut b = boot(game, panel).expect("valid");
        b.restore(serde_json::from_str(&json).expect("parses")).expect("restores");
        let again: Vec<u64> = (0..50).map(|_| b.tick().hash).collect();
        assert_eq!(future, again);
    }
}

#[test]
fn snapshot_keeps_queued_acts() {
    let mut a = boot(GAME, PANEL).expect("valid");
    let wolf = a.world().entities().values().find(|x| x.kind == "wolf").cloned().expect("a wolf");
    let args = [("dx".to_string(), 1), ("dy".to_string(), 0)].into();
    let g = a.rules().act(a.world(), None, wolf.id, "move", &args).expect("ok");
    a.queue(g);
    let snap = a.snapshot();
    let h1 = a.tick().hash;
    let mut b = boot(GAME, PANEL).expect("valid");
    b.restore(snap).expect("restores");
    assert_eq!(b.tick().hash, h1);
}

#[test]
fn foreign_snapshots_are_rejected() {
    let mut fire = boot(FIRE, FIRE_PANEL).expect("valid");
    let wolves = boot(GAME, PANEL).expect("valid").snapshot();
    let before = fire.world().hash();
    let errs = fire.restore(wolves).expect_err("wolf/sheep is not forest fire");
    assert!(errs.iter().any(|e| e.contains("unknown kind 'wolf'")), "{errs:?}");
    assert_eq!(fire.world().hash(), before, "a refused restore changes nothing");

    let mut dev = boot(DEV, DEV_PANEL).expect("valid");
    let mut snap = dev.snapshot();
    snap.world.entities.iter_mut().filter(|e| e.kind == "dev").for_each(|e| e.state = "Life.Partying".into());
    let errs = dev.restore(snap).expect_err("no such state");
    assert!(errs.iter().any(|e| e.contains("unknown state 'Life.Partying'")), "{errs:?}");
}

#[test]
fn a_log_with_a_restore_still_replays() {
    let log: Arc<Mutex<Vec<Msg>>> = Arc::default();
    let mut e = boot(MARKET, MARKET_PANEL).expect("valid");
    let (game, seed, source_hash, hash) = (e.rules().def.name.clone(), 1, e.rules().source_hash, e.world().hash());
    e.bus().subscribe(Filter::All, Box::new(log.clone()));
    e.bus().publish(&Msg::Start { game, seed, source_hash, hash });
    for _ in 0..10 {
        e.tick();
    }
    let save = e.snapshot();
    for _ in 0..10 {
        e.tick();
    }
    e.restore(save).expect("restores"); // "load game"
    for _ in 0..10 {
        e.tick();
    }
    let log = log.lock().unwrap().clone();
    let mut fresh = boot(MARKET, MARKET_PANEL).expect("valid");
    let r = sim_rules::replay(&mut fresh, &log).expect("replays through the restore");
    assert_eq!(r.ticks, 30);
    assert_eq!(fresh.world().hash(), e.world().hash());
}

// --- colony (stigmergy) ---

const COLONY: &str = include_str!("../../../games/colony/game.ron");
const COLONY_PANEL: &str = include_str!("../../../games/colony/engine.toml");

#[test]
fn climb_steps_up_the_gradient_and_stops_at_the_top() {
    let game = r#"#![enable(implicit_some)]
    Game(name: "hill",
        kinds: { "ground": (glyph: '.', solid: true, props: { "h": 0 }), "walker": (glyph: 'w') },
        layout: (legend: { '0': ("ground", {"h": 0}), '1': ("ground", {"h": 1}), '5': ("ground", {"h": 5}), '9': ("ground", {"h": 9}) },
                 rows: [ "0159" ]),
        rules: [ (name: "climb", for: "walker", then: [ Climb("ground", "h") ]) ])"#;
    let panel = "[run]\nseed = 1\nmax_ticks = 10\n[spawn]\nwalker = 1\n";
    let mut e = boot(game, panel).expect("valid");
    let walker = |e: &Engine<Running, Game>| e.world().entities().values().find(|x| x.kind == "walker").map(|x| x.x).unwrap();
    for _ in 0..6 {
        e.tick();
    }
    assert_eq!(walker(&e), 3, "climbs to the highest cell and stays there");
}

#[test]
fn climb_on_a_missing_prop_is_rejected() {
    let game = COLONY.replace(r#"Climb("ground", "scent")"#, r#"Climb("ground", "sent")"#);
    let errs = boot(&game, COLONY_PANEL).err().expect("must fail");
    assert!(errs.iter().any(|e| e.contains("kind 'ground' has no prop 'sent'")), "{errs:?}");
}

#[test]
fn golden_colony_hash() {
    let mut e = boot_colony(COLONY, &colony_envs()).expect("valid");
    for _ in 0..200 {
        e.tick();
    }
    // Recorded at eval step 018 (identity salts). Change only on purpose, together with the game (and its EVALS.md).
    assert_eq!(format!("{:016x}", e.world().hash()), "ad127c69b7527bbc");
}

// --- environments ---

const SEASONS: &str = include_str!("../../../envs/seasons.ron");

fn colony_envs() -> Vec<(String, String)> {
    vec![("seasons".to_string(), SEASONS.to_string())]
}

fn boot_colony(game: &str, envs: &[(String, String)]) -> Result<Engine<Running, Game>, Vec<String>> {
    let (world, g) = Game::from_parts(game, COLONY_PANEL, envs).map_err(|e| vec![e])?;
    Ok(Engine::<Loaded, _>::new(world, g).validate()?.start())
}

/// The seasons environment written by hand: what a C++ port would implement.
struct NativeSeasons;

impl sim_rules::NativeEnv for NativeSeasons {
    fn step(
        &self,
        tick: u64,
        p: &std::collections::BTreeMap<String, i64>,
        props: &std::collections::BTreeMap<String, i64>,
        state: &str,
    ) -> (std::collections::BTreeMap<String, i64>, String) {
        let (summer, autumn, winter) = (p["summer"], p["autumn"], p["winter"]);
        let year = summer + autumn + winter;
        let t = tick as i64 % year;
        let grow = summer + autumn;
        let triangle = |x: i64, len: i64| if x < 0 || x > len { 0 } else { 100 - (x * 200 / len - 100).abs() };
        let floor = if t < grow { p["ripe_floor"] } else { 0 };
        let ripeness = floor.max(triangle(t, grow));
        let warmth = triangle((tick as i64 + p["warmth_lag"]) % year, year);
        // Transitions see the start of the tick, exactly like the .ron machine.
        let next = match state {
            "Summer" if t >= summer => "Autumn",
            "Autumn" if t >= grow => "Winter",
            "Winter" if t < summer => "Summer",
            s => s,
        };
        let mut out = props.clone();
        out.insert("ripeness".into(), ripeness);
        out.insert("warmth".into(), warmth);
        (out, next.to_string())
    }
}

/// A calendar: the seasons environment and one kind that senses it. No end, so conformance runs every tick.
const CALENDAR: &str = r#"#![enable(implicit_some)]
Game(name: "calendar", environments: ["seasons"],
  kinds: { "watcher": (glyph: 'w', props: { "felt": 0 }, senses: { "warmth": "env.seasons.warmth" }) },
  rules: [ (name: "feel", for: "watcher", then: [ Set("felt", "sense.warmth") ]) ])"#;
const CALENDAR_PANEL: &str = "[run]\nseed = 1\nmax_ticks = 5000\n[world]\nwidth = 2\nheight = 1\n[spawn]\nwatcher = 1\n";

#[test]
fn a_native_environment_matches_its_ron_reference() {
    let envs = colony_envs();
    let ticks = sim_rules::conformance(CALENDAR, CALENDAR_PANEL, &envs, "seasons", Arc::new(NativeSeasons), 1500).expect("bit-identical");
    assert_eq!(ticks, 1500, "three years and more, every tick compared");
    // And inside a real game, up to its end.
    sim_rules::conformance(COLONY, COLONY_PANEL, &envs, "seasons", Arc::new(NativeSeasons), 1500).expect("bit-identical");
}

#[test]
fn a_wrong_native_environment_is_caught() {
    struct Late;
    impl sim_rules::NativeEnv for Late {
        fn step(
            &self,
            tick: u64,
            p: &std::collections::BTreeMap<String, i64>,
            props: &std::collections::BTreeMap<String, i64>,
            state: &str,
        ) -> (std::collections::BTreeMap<String, i64>, String) {
            // Off by one tick: winter starts a tick late.
            NativeSeasons.step(tick.saturating_sub(1), p, props, state)
        }
    }
    let err = sim_rules::conformance(CALENDAR, CALENDAR_PANEL, &colony_envs(), "seasons", Arc::new(Late), 1500).expect_err("must diverge");
    assert!(err.contains("tick"), "{err}");
}

#[test]
fn environments_are_checked_like_everything_else() {
    // Missing file.
    let errs = boot_colony(COLONY, &[]).err().expect("must fail");
    assert!(errs.iter().any(|e| e.contains("environment 'seasons' not found")), "{errs:?}");
    // A game reading something the environment does not provide.
    let game = COLONY.replace("env.seasons.ripeness", "env.seasons.ripness");
    let errs = boot_colony(&game, &colony_envs()).err().expect("must fail");
    assert!(errs.iter().any(|e| e.contains("ripness")), "{errs:?}");
    // A name clash.
    let game = COLONY.replace(r#""evaporation": 4,"#, r#""evaporation": 4, "summer": 1,"#);
    let errs = boot_colony(&game, &colony_envs()).err().expect("must fail");
    assert!(errs.iter().any(|e| e.contains("param 'summer' is also a game param")), "{errs:?}");
}

#[test]
fn an_environment_is_one_hidden_entity() {
    let e = boot_colony(COLONY, &colony_envs()).expect("valid");

    assert_eq!(e.world().of_kind("seasons").count(), 1);
    assert!(e.rules().is_hidden("seasons") && !e.rules().is_hidden("ant"));
    // Unplaced by the layout: it appears once, at (0, 0).
    let game = COLONY.replace(r#"'S': "seasons""#, r#"'S': "ground""#);
    let e = boot_colony(&game, &colony_envs()).expect("valid");
    let s: Vec<_> = e.world().of_kind("seasons").map(|x| (x.x, x.y)).collect();
    assert_eq!(s, vec![(0, 0)]);
}

#[test]
fn maths_helpers_are_integer_curves() {
    let game = r#"#![enable(implicit_some)]
    Game(name: "maths", kinds: { "probe": (glyph: 'p', props: { "a": 0, "b": 0, "c": 0, "d": 0, "e": 0 }) },
      rules: [ (name: "calc", for: "probe", then: [
        Set("a", "triangle(90, 360, 100)"), Set("b", "triangle(180, 360, 100)"), Set("c", "ramp(50, 100, 30)"),
        Set("d", "clamp(-5, 0, 10)"), Set("e", "pct(250, 40)") ]) ])"#;
    let mut e = boot(game, "[run]\nseed = 1\nmax_ticks = 5\n[world]\nwidth = 1\nheight = 1\n[spawn]\nprobe = 1\n").expect("valid");
    e.tick();
    let p = &e.world().of_kind("probe").next().unwrap().props;
    assert_eq!((p["a"], p["b"], p["c"], p["d"], p["e"]), (50, 100, 15, 0, 100));
}

// --- 3D worlds and fields ---

/// Three levels: air, soil, soil. A digger digs straight down; heat from the air spreads into the soil.
const DIG: &str = r##"#![enable(implicit_some)]
Game(name: "dig", perception: Direct,
  fields: { "soil": (init: 0), "temp": (init: 10, diffusion: 30, top: "100") },
  terrain: "soil",
  kinds: { "digger": (glyph: 'd') },
  layout: (legend: { 'd': "digger" }, cells: { 'x': { "soil": 1 } },
           levels: [ [ "d.." ], [ "xxx" ], [ "xxx" ] ]),
  rules: [
    (name: "dig", for: "digger", when: r#"field_at("soil", 0, 0, 1) != 0"#, then: [ SetFieldAt("soil", "0", "0", "1", "0") ]),
    (name: "down", for: "digger", then: [ Move3("0", "0", "1") ]),
  ])"##;
const DIG_PANEL: &str = "[run]\nseed = 1\nmax_ticks = 100\n";

#[test]
fn a_3d_world_has_levels_terrain_and_digging() {
    let mut e = boot(DIG, DIG_PANEL).expect("valid");
    assert_eq!(e.world().depth, 3);
    let digger = |e: &Engine<Running, Game>| e.world().of_kind("digger").next().map(|d| (d.x, d.y, d.z)).unwrap();
    assert_eq!(digger(&e), (0, 0, 0));
    assert!(e.world().is_terrain(0, 0, 1), "soil below");
    // Evaluated on the start of the tick, applied in order: the dig lands first, so the move finds an open voxel.
    e.tick();
    assert!(!e.world().is_terrain(0, 0, 1) && e.world().is_terrain(1, 0, 1), "one voxel dug, its neighbour not");
    assert_eq!(digger(&e), (0, 0, 1), "dug and moved into the hole in one tick");
    for _ in 0..4 {
        e.tick();
    }
    assert_eq!(digger(&e), (0, 0, 2), "and on down to the bottom level");
}

#[test]
fn heat_diffuses_down_through_the_levels() {
    let mut e = boot(DIG, DIG_PANEL).expect("valid");
    for _ in 0..20 {
        e.tick();
    }
    let t = |z| e.world().field("temp", 2, 0, z).unwrap();
    assert_eq!(t(0), 100, "the top level is pinned to the air");
    assert!(t(0) > t(1) && t(1) > t(2) && t(2) > 10, "warmth spreads downwards: {} {} {}", t(0), t(1), t(2));
}

#[test]
fn a_3d_world_is_deterministic_and_snapshots() {
    let run = || {
        let mut e = boot(DIG, DIG_PANEL).expect("valid");
        (0..30).map(|_| e.tick().hash).collect::<Vec<_>>()
    };
    assert_eq!(run(), run());
    let mut a = boot(DIG, DIG_PANEL).expect("valid");
    for _ in 0..5 {
        a.tick();
    }
    let json = serde_json::to_string(&a.snapshot()).expect("serializes");
    let future: Vec<u64> = (0..20).map(|_| a.tick().hash).collect();
    let mut b = boot(DIG, DIG_PANEL).expect("valid");
    b.restore(serde_json::from_str(&json).expect("parses")).expect("restores");
    assert_eq!(future, (0..20).map(|_| b.tick().hash).collect::<Vec<_>>(), "fields, levels and z survive a save");
}

#[test]
fn field_mistakes_are_reported() {
    let errs = boot(&DIG.replace(r#"SetFieldAt("soil""#, r#"SetFieldAt("soill""#), DIG_PANEL).err().expect("must fail");
    assert!(errs.iter().any(|e| e.contains("no field 'soill'")), "{errs:?}");
    let errs = boot(&DIG.replace(r#"terrain: "soil""#, r#"terrain: "rock""#), DIG_PANEL).err().expect("must fail");
    assert!(errs.iter().any(|e| e.contains("terrain 'rock' is not a declared field")), "{errs:?}");
    let errs = boot(&DIG.replace(r#"field_at("soil""#, r#"field_at("sol""#), DIG_PANEL).err().expect("must fail");
    assert!(errs.iter().any(|e| e.contains("no field 'sol'")), "{errs:?}");
}

// --- colony 3D ---

#[test]
fn golden_colony3d_hash() {
    let (world, g) = Game::load(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../games/colony3d")), None).expect("loads");
    let mut e = Engine::<Loaded, _>::new(world, g).validate().expect("valid").start();
    for _ in 0..200 {
        e.tick();
    }
    // Recorded at colony3d eval step 001 (identity salts, player actions). Change only on purpose, together with the
    // game (and its EVALS.md).
    assert_eq!(format!("{:016x}", e.world().hash()), "8f913e43070d74a7");
}

// --- tick rate ---

const PACER: &str = r#"#![enable(implicit_some)]
    Game(name: "pacer",
        kinds: { "walker": (glyph: 'w', props: { "steps": 0, "rate": 0 }) },
        rules: [ (name: "walk", for: "walker", when: "pace(3)", then: [ Add("steps", "1") ]),
                 (name: "rate", for: "walker", then: [ Set("rate", "tick_rate") ]) ])"#;

fn pacer(tick_rate: i64) -> Engine<Running, Game> {
    let panel =
        format!("[run]\nseed = 1\nmax_ticks = 1000\ntick_rate = {tick_rate}\n[world]\nwidth = 4\nheight = 1\n[spawn]\nwalker = 4\n");
    boot(PACER, &panel).expect("valid")
}

#[test]
fn pace_keeps_speed_per_second_at_any_tick_rate() {
    for rate in [10, 60] {
        let mut e = pacer(rate);
        for _ in 0..rate * 4 {
            e.tick();
        }
        for w in e.world().entities().values() {
            assert_eq!(w.props["steps"], 12, "3 steps a second for 4 seconds at {rate} ticks/s");
            assert_eq!(w.props["rate"], rate);
        }
    }
}

#[test]
fn pace_spaces_steps_evenly() {
    let mut e = pacer(60);
    let mut when: Vec<u64> = Vec::new();
    let first = *e.world().entities().keys().next().unwrap();
    let mut last = 0;
    for t in 1..=240 {
        e.tick();
        let s = e.world().get(first).unwrap().props["steps"];
        if s != last {
            when.push(t);
            last = s;
        }
    }
    let gaps: Vec<u64> = when.windows(2).map(|w| w[1] - w[0]).collect();
    assert!(gaps.iter().all(|g| *g == 20), "one step every 20 ticks at 60 ticks/s: {gaps:?}");
}

#[test]
fn tick_rate_must_be_positive() {
    let panel = "[run]\nseed = 1\nmax_ticks = 10\ntick_rate = 0\n[world]\nwidth = 4\nheight = 1\n[spawn]\nwalker = 1\n";
    let errs = boot(PACER, panel).err().expect("must fail");
    assert!(errs.iter().any(|e| e.contains("tick_rate must be at least 1")), "{errs:?}");
}

// --- brains (games/forage) ---

const FORAGE: &str = include_str!("../../../games/forage/game.ron");
const FORAGE_PANEL: &str = include_str!("../../../games/forage/engine.toml");

fn nest_food(e: &Engine<Running, Game>) -> i64 {
    e.world().entities().values().find(|x| x.kind == "nest").map(|n| n.props["food"]).unwrap()
}

#[test]
fn golden_forage_hash() {
    let mut e = boot(FORAGE, FORAGE_PANEL).expect("valid");
    for _ in 0..300 {
        e.tick();
    }
    // Recorded at forage eval step 000. Change only on purpose, together with the game (and its EVALS.md).
    assert_eq!(format!("{:016x}", e.world().hash()), "bc993f622da59a35");
}

#[test]
fn every_ant_has_its_own_brain() {
    let e = boot(FORAGE, FORAGE_PANEL).expect("valid");
    let genomes: Vec<&Vec<i8>> = e.world().of_kind("ant").map(|a| &a.genome).collect();
    assert_eq!(genomes.len(), 8);
    assert!(genomes.iter().all(|g| g.len() == 7 * 8 + 8 * 4), "(6 inputs + bias) × 8 hidden + 8 × 4 outputs, one byte each");
    assert!(genomes.windows(2).all(|w| w[0] != w[1]), "no two alike");
    assert!(e.world().of_kind("bush").all(|b| b.genome.is_empty()), "only learning kinds carry one");
}

#[test]
fn a_colony_of_brains_learns_to_forage() {
    let mut e = boot(FORAGE, FORAGE_PANEL).expect("valid");
    for _ in 0..1000 {
        e.tick();
    }
    let early = nest_food(&e);
    for _ in 1000..3000 {
        e.tick();
    }
    let before = nest_food(&e);
    for _ in 3000..4000 {
        e.tick();
    }
    let late = nest_food(&e) - before;
    // Random brains average ~10 a thousand ticks (one seed's early luck can reach 50); without heredity the late
    // rate stays near 1 (EVALS.md control). Learned brains: well above both.
    assert!(late >= 150 && late > 2 * early, "deliveries per 1000 ticks: {early} at first, {late} after 3000 ticks");
}

#[test]
fn brains_survive_save_and_load() {
    let mut a = boot(FORAGE, FORAGE_PANEL).expect("valid");
    for _ in 0..600 {
        a.tick();
    }
    let json = serde_json::to_string(&a.snapshot()).expect("serializes");
    assert!(json.contains("genome"));
    let future: Vec<u64> = (0..100).map(|_| a.tick().hash).collect();
    let mut b = boot(FORAGE, FORAGE_PANEL).expect("valid");
    b.restore(serde_json::from_str(&json).expect("parses")).expect("restores");
    assert_eq!(future, (0..100).map(|_| b.tick().hash).collect::<Vec<_>>(), "genomes are part of the saved world");
}

#[test]
fn brain_mistakes_are_reported() {
    let errs = boot(&FORAGE.replace("perception: Senses,", "perception: Direct,"), FORAGE_PANEL).err().expect("must fail");
    assert!(errs.iter().any(|e| e.contains("a brain needs `perception: Senses`")), "{errs:?}");
    let errs = boot(&FORAGE.replace(r#""load": "me.load * 100""#, r#""load": "me.lod * 100""#), FORAGE_PANEL).err().expect("must fail");
    assert!(errs.iter().any(|e| e.contains("lod")), "{errs:?}");
    let errs =
        boot(&FORAGE.replace(r#"outputs: ["north", "east", "south", "west"]"#, "outputs: []"), FORAGE_PANEL).err().expect("must fail");
    assert!(errs.iter().any(|e| e.contains("at least one output")), "{errs:?}");
}

// --- lanes (first-person positions) ---

#[test]
fn golden_lanes_hash() {
    let (world, g) = Game::load(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../games/lanes")), None).expect("loads");
    let mut e = Engine::<Loaded, _>::new(world, g).validate().expect("valid").start();
    for _ in 0..120 {
        e.tick();
    }
    // Recorded when the game was made. Change only on purpose, together with the game.
    assert_eq!(format!("{:016x}", e.world().hash()), "70ebfcf7985ff7a6");
}

#[test]
fn lanes_goto_moves_one_position_per_tick_and_refuses_where_you_already_are() {
    let (world, g) = Game::load(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../games/lanes")), None).expect("loads");
    let mut e = Engine::<Loaded, _>::new(world, g).validate().expect("valid").start();
    e.tick();
    let car = e.world().of_kind("car").next().unwrap().id;
    let x0 = e.world().get(car).unwrap().x;
    let go = |e: &Engine<Running, Game>, lane: i64| {
        let args = [("lane".to_string(), lane)].into_iter().collect();
        e.rules().act(e.world(), None, car, "goto", &args)
    };
    assert!(go(&e, x0).is_err(), "the position you are headed to is not a move");
    let group = go(&e, 3).expect("another position is");
    e.queue(group);
    let xs: Vec<i64> = (0..4)
        .map(|_| {
            e.tick();
            e.world().get(car).unwrap().x
        })
        .collect();
    assert_eq!(xs, vec![x0, x0 + 1, x0 + 2, 3].into_iter().map(|x| x.min(3)).collect::<Vec<_>>(), "one position a tick");
}

// --- mound (crawlers, fields as material, the player's reach) ---

fn mound() -> Engine<Running, Game> {
    let (world, g) = Game::load(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../games/mound")), None).expect("loads");
    Engine::<Loaded, _>::new(world, g).validate().expect("valid").start()
}

#[test]
fn golden_mound_hash() {
    let mut e = mound();
    for _ in 0..300 {
        e.tick();
    }
    // Recorded when the game was made. Change only on purpose, together with the game.
    assert_eq!(format!("{:016x}", e.world().hash()), "08a1751ef66c65b1");
}

#[test]
fn ground_starts_under_air_and_crawlers_start_on_it() {
    let e = mound();
    let w = e.world();
    assert!(!w.is_terrain(5, 5, 13) && w.is_terrain(5, 5, 14) && w.is_terrain(5, 5, 19), "`from_level: 14`: air above, earth from 14");
    assert_eq!(w.count("termite"), 120, "every termite found a place");
    for t in w.of_kind("termite").chain(w.of_kind("you")) {
        assert!(!w.is_terrain(t.x, t.y, t.z) && w.touches_terrain(t.x, t.y, t.z), "spawned on a surface: {t:?}");
    }
}

#[test]
fn crawlers_never_float_while_they_dig_climb_and_build() {
    let mut e = mound();
    for tick in 0..1500 {
        e.tick();
        let w = e.world();
        for t in w.of_kind("termite") {
            // A termite may be left inside a ball dropped where it stands; otherwise it touches terrain.
            assert!(w.is_terrain(t.x, t.y, t.z) || w.touches_terrain(t.x, t.y, t.z), "tick {tick}: floating {t:?}");
        }
    }
    let built = e.world().field_values("mud").unwrap().iter().filter(|v| **v == 2).count();
    assert!(built > 20, "they built something: {built}");
}

#[test]
fn you_dig_and_drop_exactly_where_you_reach_and_no_farther() {
    let mut e = mound();
    let me = e.world().of_kind("you").next().unwrap().clone();
    let act = |e: &Engine<Running, Game>, name: &str, d: (i64, i64, i64)| {
        let args = [("dx", d.0), ("dy", d.1), ("dz", d.2)].map(|(k, v)| (k.to_string(), v)).into_iter().collect();
        e.rules().act(e.world(), None, me.id, name, &args)
    };
    assert!(act(&e, "drop", (1, 0, 0)).is_err(), "nothing to drop yet");
    assert!(act(&e, "dig", (0, 0, 4)).is_err(), "out of reach");
    let g = act(&e, "dig", (0, 0, 2)).expect("earth two below, within reach");
    e.queue(g);
    e.tick();
    assert!(!e.world().is_terrain(me.x, me.y, me.z + 2), "SetFieldAt dug exactly two below, not one");
    assert!(e.world().is_terrain(me.x, me.y, me.z + 1), "the voxel between is untouched");
    let g = act(&e, "drop", (2, 0, 0)).expect("carrying now; two to the side is within reach and open");
    e.queue(g);
    e.tick();
    assert_eq!(e.world().field("mud", me.x + 2, me.y, me.z), Some(2), "a ball, exactly there");
    assert!(e.world().field("cement", me.x + 2, me.y, me.z).unwrap() > 0, "and it smells");
}

#[test]
fn a_clinging_kind_needs_terrain() {
    let game = r#"Game(name: "t", kinds: { "bug": (glyph: 'b', cling: true) }, rules: [])"#;
    let errs = boot(game, "[run]\nseed = 1\nmax_ticks = 10\n[world]\nwidth = 4\nheight = 4\n").err().expect("must fail");
    assert!(errs.iter().any(|e| e.contains("clings") && e.contains("terrain")), "{errs:?}");
}
