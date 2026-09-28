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
            e.bus().publish(Msg::Act {
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
        (DEV.replace(r#"On(NearestIn("project", "Blocked")"#, r#"On(NearestIn("project", "Blokked")"#), "state 'Blokked' does not exist for 'project'"),
        (DEV.replace(r#"initial: "Warmup","#, r#"initial: "Warm","#), "initial 'Warm' is not one of its states"),
        (DEV.replace(r#""Vacation": 'v'"#, r#""Vacashun": 'v'"#), "glyph for unknown state 'Vacashun'"),
        (DEV.replace(r#"(from: "Stuck", back: true,"#, r#"(from: "Stuck", to: "Work", back: true,"#), "exactly one of `to` and `back: true`"),
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
    e.bus().publish(Msg::Start { game, seed, source_hash, hash });
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
