//! simcraft-check: a game's bugs, made to show themselves. Loads the game (with every panel it has), its stage or
//! track (every image, every animation channel, every button's action), plays it headless for a while, and reports
//! what looks wrong, one line each: `error` (it is broken), `warn` (it runs, but probably not as meant).
//!
//!   simcraft-check games/lanes [--ticks 300]
//!
//! Fast enough to run on every save (the Claude hook in `.claude/hooks/check.sh` does). Exit code 1 on errors.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use sim_core::{Engine, Loaded, Running};
use sim_gpu::stage::Button;
use sim_rules::Game;

struct Report {
    errors: Vec<String>,
    warns: Vec<String>,
    /// The cost profile: what the rules did per entity-tick, the hottest rules (always printed, for the reader).
    notes: Vec<String>,
}

/// Work budgets per entity-tick: above them a game is flagged (time is noise; these counts are exact).
const EVALS_PER_ENTITY_TICK: f64 = 25.0;
const QUERIES_PER_ENTITY_TICK: f64 = 10.0;

impl Report {
    fn error(&mut self, m: String) {
        self.errors.push(m);
    }
    fn warn(&mut self, m: String) {
        self.warns.push(m);
    }
}

fn boot(dir: &Path, panel: &Path) -> Result<Engine<Running, Game>, String> {
    let src = std::fs::read_to_string(panel).map_err(|e| format!("{}: {e}", panel.display()))?;
    let (world, game) = Game::load_panel(dir, &src)?;
    Ok(Engine::<Loaded, _>::new(world, game).validate().map_err(|e| e.join("\n  "))?.start())
}

/// Buttons name declared actions with the right args: checked against the game, not trusted.
fn check_buttons(what: &str, buttons: &[Button], engine: &Engine<Running, Game>, r: &mut Report) {
    let game = engine.rules();
    for b in buttons {
        let Some(action) = game.def.actions.iter().find(|a| a.name == b.action) else {
            r.error(format!("{what}: button '{}' names no declared action (game.ron `actions`)", b.action));
            continue;
        };
        let want: Vec<&String> = action.args.iter().collect();
        let have: Vec<&String> = b.args.keys().collect();
        if want != have {
            r.error(format!("{what}: button '{}' passes args {have:?}, the action takes {want:?}", b.action));
        }
        if !game.def.kinds.contains_key(&b.on) {
            r.error(format!("{what}: button '{}' acts for kind '{}', which the game does not have", b.action, b.on));
        }
        for (name, a) in &b.anims {
            let bad = a.unknown_channels();
            if !bad.is_empty() {
                r.error(format!("{what}: button '{}' animation '{name}' has unknown channels {bad:?}", b.action));
            }
        }
    }
}

/// A roam view names your kind, three actions that take `dx`, `dy`, `dz`, kinds, a smell field and a carried prop:
/// each checked against the game.
fn check_roam(roam: &sim_gpu::roam::Roam, engine: &Engine<Running, Game>, r: &mut Report) {
    let game = engine.rules();
    match game.def.kinds.get(&roam.you) {
        None => r.error(format!("roam.ron: you are '{}', which the game does not have", roam.you)),
        Some(k) => {
            if !game.cfg.agent.controllable.contains(&roam.you) {
                r.error(format!("roam.ron: '{}' is not controllable (engine.toml [agent] controllable)", roam.you));
            }
            if let Some(p) = &roam.actions.carrying
                && !k.props.contains_key(p)
            {
                r.error(format!("roam.ron: carrying reads '{}.{p}', which has no such prop", roam.you));
            }
        }
    }
    let a = &roam.actions;
    for name in [&a.crawl, &a.dig, &a.drop] {
        match game.def.actions.iter().find(|x| &x.name == name) {
            None => r.error(format!("roam.ron: action '{name}' is not declared (game.ron `actions`)")),
            Some(x) if x.args != ["dx", "dy", "dz"] => {
                r.error(format!("roam.ron: action '{name}' takes {:?}; the view passes [\"dx\", \"dy\", \"dz\"]", x.args))
            }
            _ => {}
        }
    }
    for k in roam.kinds.keys().filter(|k| !game.def.kinds.contains_key(*k)) {
        r.error(format!("roam.ron: draws kind '{k}', which the game does not have"));
    }
    // Models: the contract between the modelling tool's file and the game, checked both ways.
    for (kind, body) in &roam.kinds {
        let Some(spec) = &body.model else { continue };
        let states = game.states_of(kind);
        for (sel, _) in spec.clips.iter().filter(|(sel, _)| sel != "*") {
            if !states.iter().any(|s| sim_state::in_label(s, sel)) {
                r.error(format!("roam.ron: kind '{kind}' plays a clip in state '{sel}', which its machine does not have ({states:?})"));
            }
        }
        if let Some(c) = &body.carries_in
            && !states.iter().any(|s| sim_state::in_label(s, c))
            && !game.def.kinds.get(kind).is_some_and(|k| k.props.contains_key(c))
        {
            r.error(format!("roam.ron: kind '{kind}' carries in '{c}', which is neither its state nor its prop"));
        }
        let wanted: Vec<&String> = spec.clips.iter().map(|(_, c)| c).chain(spec.still.iter()).collect();
        for file in spec.files() {
            let Some(m) = roam.models.get(file) else { continue };
            for clip in wanted.iter().filter(|c| !m.clips.contains_key(c.as_str())) {
                r.error(format!("roam.ron: {file} has no clip '{clip}' (it has {:?})", m.clips.keys().collect::<Vec<_>>()));
            }
            if let Some(socket) = spec.carry.as_ref().filter(|s| !m.nodes.contains_key(*s)) {
                r.error(format!("roam.ron: {file} has no socket node '{socket}'"));
            }
            for mat in spec.textures.keys().filter(|t| !m.materials.iter().any(|x| &x.name == *t)) {
                r.error(format!("roam.ron: {file} has no material '{mat}' to re-skin"));
            }
        }
    }
    if let Some(s) = &roam.smell
        && !game.def.fields.contains_key(&s.field)
    {
        r.error(format!("roam.ron: smell reads field '{}', which the game does not declare", s.field));
    }
    if game.def.terrain.is_none() {
        r.error("roam.ron: the game has no `terrain` to walk on".into());
    }
    let f = roam.feel;
    if f.run < f.walk || f.height < f.eye || f.radius >= 0.5 {
        r.warn(format!(
            "roam.ron: feel: run {} < walk {}, eye {} above the body's height {}, or radius {} too wide for one voxel",
            f.run, f.walk, f.eye, f.height, f.radius
        ));
    }
}

/// A drive view names its cars' kind (moving, controllable), an action whose args are exactly its axes, buttons
/// and an autopilot switch that are declared actions with the args they pass. Returns what a player would press.
fn check_drive(d: &sim_gpu::drive::Drive, engine: &Engine<Running, Game>, r: &mut Report) -> Vec<(String, String, BTreeMap<String, i64>)> {
    let game = engine.rules();
    let mut presses = Vec::new();
    match game.def.kinds.get(&d.cars) {
        None => r.error(format!("drive.ron: cars are kind '{}', which the game does not have", d.cars)),
        Some(k) => {
            if k.motion.is_none() {
                r.error(format!("drive.ron: kind '{}' has no `motion` (the view reads px, py and yaw)", d.cars));
            }
            if !game.cfg.agent.controllable.contains(&d.cars) {
                r.warn(format!("drive.ron: '{}' is not controllable (engine.toml [agent]): the keys will be refused", d.cars));
            }
            if let Some((p, _)) = &d.you
                && !k.props.contains_key(p)
            {
                r.error(format!("drive.ron: you are the car whose '{p}' matches, and '{}' has no such prop", d.cars));
            }
        }
    }
    let mut want = |what: String, action: &str, args: Vec<&String>| {
        let Some(a) = game.def.actions.iter().find(|a| a.name == action) else {
            r.error(format!("drive.ron: {what} names action '{action}', which the game does not declare"));
            return;
        };
        let mut have: Vec<&String> = args;
        have.sort();
        let mut takes: Vec<&String> = a.args.iter().collect();
        takes.sort();
        if have != takes {
            r.error(format!("drive.ron: {what} passes {have:?} to '{action}', which takes {takes:?}"));
        }
    };
    if let Some(c) = &d.controls {
        want("controls".into(), &c.action, c.axes.keys().collect());
        let full: BTreeMap<String, i64> = c.axes.keys().map(|k| (k.clone(), if k.contains("brake") { 0 } else { 600 })).collect();
        presses.push((d.cars.clone(), c.action.clone(), full));
        for b in &c.buttons {
            want(format!("key '{}'", b.key), &b.action, b.args.keys().chain(b.toggle.iter()).collect());
            let mut args = b.args.clone();
            args.extend(b.toggle.iter().map(|t| (t.clone(), 1)));
            presses.push((d.cars.clone(), b.action.clone(), args));
        }
    }
    if let Some(a) = &d.autopilot {
        want("autopilot".into(), &a.action, vec![&a.arg]);
    }
    // Pictures: a named one that is not there is a mistake (the surface quietly goes plain); the rest is listed.
    let photos = sim_gpu::drive::photos::Photos::resolve(&d.dir, &d.look.textures);
    for (k, why) in &photos.missing {
        r.warn(format!("drive.ron: look.textures.{k}: {why} (drawn with the view's own picture instead)"));
    }
    let own: Vec<&str> = sim_gpu::drive::photos::SURFACES.iter().map(|s| s.0).filter(|k| photos.get(k).is_none()).collect();
    r.notes.push(format!(
        "drive photos: {} ({}); the view's own picture: {}",
        photos.found.len(),
        photos.found.keys().cloned().collect::<Vec<_>>().join(", "),
        if own.is_empty() { "none".to_string() } else { own.join(", ") }
    ));
    presses
}

fn run(dir: &Path, ticks: u32) -> Report {
    let mut r = Report { errors: Vec::new(), warns: Vec::new(), notes: Vec::new() };
    let panels: Vec<PathBuf> = ["engine.toml", "play.toml"].iter().map(|p| dir.join(p)).filter(|p| p.exists()).collect();
    if panels.is_empty() {
        r.error(format!("{}: no engine.toml", dir.display()));
        return r;
    }
    // What a player could press: the views' buttons (real actions with real args), else every declared action
    // without args. The check presses them in turn, so bugs behind actions show too.
    let mut presses: Vec<(String, String, BTreeMap<String, i64>)> = Vec::new();
    for panel in &panels {
        let name = panel.file_name().unwrap().to_string_lossy().to_string();
        let mut engine = match boot(dir, panel) {
            Ok(e) => e,
            Err(e) => {
                r.error(format!("{name}: does not load:\n  {e}"));
                continue;
            }
        };
        if name == "engine.toml" {
            if dir.join("stage.ron").exists() {
                match sim_gpu::load_stage(dir, None) {
                    Ok((stage, _)) => {
                        check_buttons("stage.ron", &stage.buttons, &engine, &mut r);
                        presses.extend(stage.buttons.iter().map(|b| (b.on.clone(), b.action.clone(), b.args.clone())));
                        for p in &stage.piles {
                            if !engine.rules().def.kinds.get(&p.kind).is_some_and(|k| k.props.contains_key(&p.prop)) {
                                r.error(format!("stage.ron: pile of '{}.{}': no such prop", p.kind, p.prop));
                            }
                        }
                    }
                    Err(e) => r.error(format!("stage.ron: {e}")),
                }
            }
            if dir.join("track.ron").exists() {
                match sim_gpu::load_track(dir) {
                    Ok((track, _)) => {
                        check_buttons("track.ron", &track.buttons, &engine, &mut r);
                        presses.extend(track.buttons.iter().map(|b| (b.on.clone(), b.action.clone(), b.args.clone())));
                        let follow = engine.rules().def.kinds.get(&track.follow);
                        if follow.is_none() {
                            r.error(format!("track.ron: follows kind '{}', which the game does not have", track.follow));
                        }
                        let has = |p: &str| follow.is_some_and(|k| k.props.contains_key(p));
                        for p in track.meters.iter().map(|m| &m.prop).chain(track.bars.iter().map(|b| &b.prop)) {
                            if !has(p) {
                                r.error(format!("track.ron: meter or bar reads '{}.{p}', which has no such prop", track.follow));
                            }
                        }
                        if let Some(s) = &track.switch {
                            if !has(&s.prop) {
                                r.error(format!("track.ron: switch reads '{}.{}', which has no such prop", track.follow, s.prop));
                            }
                            if !s.positions.is_empty() && s.positions.len() != track.road.lanes as usize {
                                r.warn(format!("track.ron: {} switch positions for {} lanes", s.positions.len(), track.road.lanes));
                            }
                        }
                        for (k, p) in &track.kinds {
                            if let Some(bad) = p.gone.as_ref().map(|a| a.unknown_channels()).filter(|b| !b.is_empty()) {
                                r.error(format!("track.ron: kind '{k}' gone animation has unknown channels {bad:?}"));
                            }
                        }
                    }
                    Err(e) => r.error(format!("track.ron: {e}")),
                }
            }
            if dir.join("drive.ron").exists() {
                match sim_gpu::drive::load(dir, engine.rules()) {
                    Ok((drive, _)) => presses.extend(check_drive(&drive, &engine, &mut r)),
                    Err(e) => r.error(format!("drive.ron: {e}")),
                }
            }
            if dir.join("roam.ron").exists() {
                match sim_gpu::load_roam(dir) {
                    Ok((roam, _)) => check_roam(&roam, &engine, &mut r),
                    Err(e) => r.error(format!("roam.ron: {e}")),
                }
            }
        }
        // Buttons a designer put on screen: those must work. Fallback presses only exercise the rules.
        let from_views = !presses.is_empty();
        if presses.is_empty() {
            let game = engine.rules();
            for a in game.def.actions.iter().filter(|a| a.args.is_empty()) {
                if let Some(kind) = game.cfg.agent.controllable.iter().find(|k| a.for_kind.as_ref().is_none_or(|f| f == *k)) {
                    presses.push((kind.clone(), a.name.clone(), BTreeMap::new()));
                }
            }
        }
        // Play it headless, pressing a button every few ticks, and look at what happened.
        let mut seen: BTreeMap<String, (u32, u64, String)> = BTreeMap::new();
        let (mut accepted, mut allowed_here, mut next) = (0u32, 0u32, 0usize);
        engine.rules().reset_work();
        let (mut entity_ticks, mut played) = (0u64, 0u64);
        for t in 0..ticks {
            if engine.outcome().is_some() {
                break;
            }
            entity_ticks += engine.world().entities().len() as u64;
            played += 1;
            if t % 5 == 4 && !presses.is_empty() {
                let (kind, action, args) = presses[next % presses.len()].clone();
                next += 1;
                // A panel that does not make the kind controllable (evals keep the player out) is not a refusal.
                if !engine.rules().cfg.agent.controllable.contains(&kind) {
                    continue;
                }
                allowed_here += 1;
                let id = engine.world().of_kind(&kind).next().map(|e| e.id);
                if let Some(Ok(group)) = id.map(|id| engine.rules().act(engine.world(), None, id, &action, &args)) {
                    engine.queue(group);
                    accepted += 1;
                }
            }
            for ev in engine.tick().events {
                let e = seen.entry(ev.name.clone()).or_insert_with(|| (0, ev.tick, ev.source.clone()));
                e.0 += 1;
            }
        }
        for (name_, (n, tick, source)) in &seen {
            if name_.starts_with("error") {
                r.error(format!("{name}: {n}× \"{name_}\" (first at tick {tick}, rule '{source}')"));
            } else if name_.starts_with("clamped") {
                r.warn(format!(
                    "{name}: {n}× {name_} (rule '{source}', first at tick {tick}): use MoveBy for a move of more than one cell"
                ));
            }
        }
        // The cost profile (only for the main panel, so the numbers are the game's own).
        if name == "engine.toml" && played > 0 {
            let w = engine.rules().work();
            let per = |n: u64| n as f64 / entity_ticks.max(1) as f64;
            r.notes.push(format!(
                "cost: {:.1} entities, per entity-tick {:.2} evals, {:.2} queries, {:.2} maps",
                entity_ticks as f64 / played as f64,
                per(w.evals),
                per(w.queries),
                per(w.maps)
            ));
            if per(w.evals) > EVALS_PER_ENTITY_TICK {
                r.warn(format!(
                    "{name}: {:.1} expression evaluations per entity-tick (budget {EVALS_PER_ENTITY_TICK}): see the hottest rules",
                    per(w.evals)
                ));
            }
            if per(w.queries) > QUERIES_PER_ENTITY_TICK {
                r.warn(format!("{name}: {:.1} spatial queries per entity-tick (budget {QUERIES_PER_ENTITY_TICK})", per(w.queries)));
            }
            let mut rules = engine.rules().rule_work();
            rules.sort_by(|a, b| b.checks.cmp(&a.checks).then(a.name.cmp(&b.name)));
            let hot: Vec<String> = rules
                .iter()
                .take(3)
                .filter(|x| x.checks > 0)
                .map(|x| {
                    format!(
                        "{} {:.0}/tick ({:.0}% fire)",
                        x.name,
                        x.checks as f64 / played as f64,
                        100.0 * x.fires as f64 / x.checks as f64
                    )
                })
                .collect();
            if !hot.is_empty() {
                r.notes.push(format!("hottest rules: {}", hot.join(", ")));
            }
            let never: Vec<&str> = rules.iter().filter(|x| x.checks > 0 && x.fires == 0).map(|x| x.name.as_str()).collect();
            if !never.is_empty() {
                r.notes.push(format!("checked but never fired in {played} ticks: {}", never.join(", ")));
            }
        }
        if from_views && allowed_here > 0 && accepted == 0 {
            r.warn(format!("{name}: pressed {} kinds of button, the game took none: are the actions' `when`s right?", presses.len()));
        }
        if let Some(o) = engine.outcome()
            && engine.world().tick < 5
        {
            r.warn(format!("{name}: ends at tick {} ({o}) with no input: is the start state right?", engine.world().tick));
        }
    }
    r
}

fn main() {
    let mut args = std::env::args().skip(1);
    let (mut dir, mut ticks) = (PathBuf::from("games/lanes"), 300u32);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--ticks" => ticks = args.next().and_then(|s| s.parse().ok()).unwrap_or(ticks),
            _ => dir = PathBuf::from(a),
        }
    }
    let r = run(&dir, ticks);
    for e in &r.errors {
        println!("error {}: {e}", dir.display());
    }
    for w in &r.warns {
        println!("warn  {}: {w}", dir.display());
    }
    if r.errors.is_empty() && r.warns.is_empty() {
        println!("ok    {}: loads, its views check out, {ticks} ticks with presses, no complaint", dir.display());
    }
    for n in &r.notes {
        println!("note  {}: {n}", dir.display());
    }
    std::process::exit(if r.errors.is_empty() { 0 } else { 1 });
}
