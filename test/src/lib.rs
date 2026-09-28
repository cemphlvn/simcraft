//! simtest: simcraft's test suite library.
//!
//! Game developers write **scenarios** (`test/scenarios/*.ron`): load a game, override its panel, step, act as a
//! player, expect things about the world, take snapshots. Engine developers use the same pieces from Rust.
//! Format and expressions: docs/architecture.md, "Testing".

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rhai::{Dynamic, Map};
use serde::Deserialize;
use sim_core::{Engine, Loaded, Running};
use sim_rules::Game;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename = "Scenario", deny_unknown_fields)]
pub struct Scenario {
    pub name: String,
    /// Game folder, relative to the repository root.
    pub game: String,
    #[serde(default)]
    pub seed: Option<u64>,
    #[serde(default)]
    pub params: BTreeMap<String, i64>,
    #[serde(default)]
    pub switches: BTreeMap<String, bool>,
    /// The game must fail to load, with this text in an error.
    #[serde(default)]
    pub expect_error: Option<String>,
    #[serde(default)]
    pub steps: Vec<Step>,
}

#[derive(Clone, Debug, Deserialize)]
pub enum Step {
    Step(u64),
    Until(String, u64),
    Expect(String),
    Act(ActSpec),
    Refused(ActSpec, String),
    Hash(String),
    Snapshot(String),
    SaveLoad(u64),
    Probe(String),
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ActSpec {
    /// Entity id; 0 = the first entity of `kind`.
    pub entity: u64,
    pub kind: String,
    #[serde(rename = "as")]
    pub seat: String,
    #[serde(rename = "do")]
    pub action: String,
    pub args: BTreeMap<String, i64>,
}

/// One scenario's result.
#[derive(Debug, Default)]
pub struct Report {
    pub name: String,
    pub failures: Vec<String>,
    /// (snapshot name, text): the caller decides how to compare (insta in `cargo test`).
    pub snapshots: Vec<(String, String)>,
    /// Probe output.
    pub notes: Vec<String>,
    pub ticks: u64,
}

impl Report {
    pub fn passed(&self) -> bool {
        self.failures.is_empty()
    }
}

/// The repository root (this crate lives in `test/`).
pub fn repo_root() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    root.canonicalize().unwrap_or(root)
}

/// Every scenario file under `test/scenarios`, sorted.
pub fn scenario_files() -> Vec<PathBuf> {
    let dir = repo_root().join("test/scenarios");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "ron")).collect())
        .unwrap_or_default();
    files.sort();
    files
}

pub fn load_scenario(path: &Path) -> Result<Scenario, String> {
    let src = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse_scenario(&src).map_err(|e| format!("{}: {e}", path.display()))
}

/// A scenario from text (optional fields need no `Some(...)`).
pub fn parse_scenario(src: &str) -> Result<Scenario, String> {
    ron::Options::default().with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME).from_str(src).map_err(|e| e.to_string())
}

/// The game's panel with the scenario's overrides applied.
fn panel(dir: &Path, s: &Scenario) -> Result<String, String> {
    let src = std::fs::read_to_string(dir.join("engine.toml")).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut t: toml::Table = toml::from_str(&src).map_err(|e| format!("engine.toml: {e}"))?;
    let section = |t: &mut toml::Table, name: &str| -> toml::Table { t.get(name).and_then(|v| v.as_table()).cloned().unwrap_or_default() };
    if let Some(seed) = s.seed {
        let mut run = section(&mut t, "run");
        run.insert("seed".into(), toml::Value::Integer(seed as i64));
        t.insert("run".into(), toml::Value::Table(run));
    }
    if !s.params.is_empty() {
        let mut params = section(&mut t, "params");
        params.extend(s.params.iter().map(|(k, v)| (k.clone(), toml::Value::Integer(*v))));
        t.insert("params".into(), toml::Value::Table(params));
    }
    if !s.switches.is_empty() {
        let mut sw = section(&mut t, "switches");
        sw.extend(s.switches.iter().map(|(k, v)| (k.clone(), toml::Value::Boolean(*v))));
        t.insert("switches".into(), toml::Value::Table(sw));
    }
    // Tests never write logs or open sockets.
    t.remove("bus");
    toml::to_string(&t).map_err(|e| e.to_string())
}

/// Loads a game for a test (panel overrides applied). Errors are the engine's own list.
pub fn boot(s: &Scenario) -> Result<Engine<Running, Game>, Vec<String>> {
    let dir = repo_root().join(&s.game);
    let panel = panel(&dir, s).map_err(|e| vec![e])?;
    let (world, game) = Game::load_panel(&dir, &panel).map_err(|e| vec![e])?;
    Ok(Engine::<Loaded, _>::new(world, game).validate()?.start())
}

/// Keeps what expressions can see beyond the world itself.
pub struct Run {
    pub engine: Engine<Running, Game>,
    pub events: BTreeMap<String, i64>,
}

impl Run {
    pub fn new(engine: Engine<Running, Game>) -> Run {
        Run { engine, events: BTreeMap::new() }
    }

    pub fn tick(&mut self) -> bool {
        if self.done() {
            return false;
        }
        for e in self.engine.tick().events {
            *self.events.entry(e.name).or_default() += 1;
        }
        true
    }

    pub fn done(&self) -> bool {
        let max = self.engine.rules().cfg.run.max_ticks;
        self.engine.outcome().is_some() || self.engine.world().tick >= max
    }

    /// `states.<kind>.<leaf>`, `sum.<kind>.<prop>`, `done`, `result` (`events.<name>` is substituted before).
    fn extra(&self) -> Map {
        let (w, g) = (self.engine.world(), self.engine.rules());
        let mut states: BTreeMap<String, BTreeMap<String, i64>> = BTreeMap::new();
        let mut sums: BTreeMap<String, BTreeMap<String, i64>> = BTreeMap::new();
        for e in w.entities().values() {
            for part in g.state_label(e).split('|') {
                let leaf = part.rsplit('.').next().unwrap_or("-").to_string();
                *states.entry(e.kind.clone()).or_default().entry(leaf).or_default() += 1;
            }
            for (p, v) in &e.props {
                *sums.entry(e.kind.clone()).or_default().entry(p.clone()).or_default() += v;
            }
        }
        // Every declared kind is present (0 when none), so `states.ant.Carry` is 0 rather than an error.
        for kind in g.def.kinds.keys() {
            let st = states.entry(kind.clone()).or_default();
            for path in g.states_of(kind) {
                st.entry(path.rsplit('.').next().unwrap_or(&path).to_string()).or_default();
            }
            let sm = sums.entry(kind.clone()).or_default();
            for p in g.def.kinds[kind].props.keys() {
                sm.entry(p.clone()).or_default();
            }
        }
        let nest = |m: BTreeMap<String, BTreeMap<String, i64>>| -> Dynamic {
            Dynamic::from(m.into_iter().map(|(k, v)| (k.into(), Dynamic::from(to_map(v)))).collect::<Map>())
        };
        let mut x = Map::new();
        x.insert("states".into(), nest(states));
        x.insert("sum".into(), nest(sums));
        x.insert("done".into(), Dynamic::from(self.done()));
        x.insert("result".into(), Dynamic::from(self.engine.outcome().unwrap_or("").to_string()));
        x
    }

    pub fn eval(&self, expr: &str) -> Result<Dynamic, String> {
        let src = rewrite_events(expr, &self.events);
        self.engine.rules().eval_world(self.engine.world(), &src, self.extra())
    }

    pub fn check(&self, expr: &str) -> Result<bool, String> {
        self.eval(expr)?.as_bool().map_err(|t| format!("`{expr}` is a {t}, not true/false"))
    }

    /// A readable summary: counts, states, events, props of kinds that exist once.
    pub fn summary(&self) -> String {
        let (w, g) = (self.engine.world(), self.engine.rules());
        let mut out = format!("tick {}{}\n", w.tick, self.engine.outcome().map(|r| format!(" (ended: {r})")).unwrap_or_default());
        let mut by: BTreeMap<&str, BTreeMap<String, usize>> = BTreeMap::new();
        for e in w.entities().values() {
            *by.entry(&e.kind).or_default().entry(g.state_label(e).to_string()).or_default() += 1;
        }
        for (kind, states) in &by {
            let n: usize = states.values().sum();
            let detail: Vec<String> = states.iter().filter(|(s, _)| *s != "-").map(|(s, c)| format!("{s} {c}")).collect();
            out += &format!("{kind}: {n}{}\n", if detail.is_empty() { String::new() } else { format!(" ({})", detail.join(", ")) });
        }
        for (kind, states) in &by {
            if states.values().sum::<usize>() == 1
                && let Some(e) = w.of_kind(kind).next()
                && !e.props.is_empty()
            {
                let props: Vec<String> = e.props.iter().map(|(k, v)| format!("{k}={v}")).collect();
                out += &format!("{kind} props: {}\n", props.join(" "));
            }
        }
        if !self.events.is_empty() {
            let ev: Vec<String> = self.events.iter().map(|(k, v)| format!("{k} {v}")).collect();
            out += &format!("events: {}\n", ev.join(", "));
        }
        out
    }

    fn find(&self, a: &ActSpec) -> Result<u64, String> {
        let w = self.engine.world();
        if a.entity != 0 {
            return Ok(a.entity);
        }
        w.of_kind(&a.kind).next().map(|e| e.id).ok_or_else(|| format!("no entity of kind '{}'", a.kind))
    }

    pub fn act(&mut self, a: &ActSpec) -> Result<(), String> {
        let id = self.find(a)?;
        let seat = (!a.seat.is_empty()).then_some(a.seat.as_str());
        let group = self.engine.rules().act(self.engine.world(), seat, id, &a.action, &a.args)?;
        self.engine.queue(group);
        Ok(())
    }
}

fn to_map(m: BTreeMap<String, i64>) -> Map {
    m.into_iter().map(|(k, v)| (k.into(), Dynamic::from(v))).collect()
}

/// `events.<name>` becomes the number of times that event fired (0 if never): Rhai maps error on unknown keys.
fn rewrite_events(expr: &str, events: &BTreeMap<String, i64>) -> String {
    let mut out = String::new();
    let mut rest = expr;
    while let Some(i) = rest.find("events.") {
        let before = rest[..i].chars().next_back();
        if before.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.') {
            out += &rest[..i + 7];
            rest = &rest[i + 7..];
            continue;
        }
        out += &rest[..i];
        let tail = &rest[i + 7..];
        let name: String = tail.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
        out += &events.get(&name).copied().unwrap_or(0).to_string();
        rest = &tail[name.len()..];
    }
    out + rest
}

/// Runs a scenario. Never panics: every problem is a failure in the report.
pub fn run(s: &Scenario) -> Report {
    let mut r = Report { name: s.name.clone(), ..Report::default() };
    let engine = match (boot(s), &s.expect_error) {
        (Err(errs), Some(want)) => {
            if !errs.iter().any(|e| e.contains(want.as_str())) {
                r.failures.push(format!("expected a load error containing '{want}', got: {errs:?}"));
            }
            return r;
        }
        (Ok(_), Some(want)) => {
            r.failures.push(format!("expected a load error containing '{want}', but the game loaded"));
            return r;
        }
        (Err(errs), None) => {
            r.failures.push(format!("the game does not load: {}", errs.join("; ")));
            return r;
        }
        (Ok(e), None) => e,
    };
    let mut run = Run::new(engine);
    for (i, step) in s.steps.iter().enumerate() {
        let at = |m: String| format!("step {} {step:?}: {m}", i + 1);
        let tick = run.engine.world().tick;
        match step {
            Step::Step(n) => {
                for _ in 0..*n {
                    if !run.tick() {
                        break;
                    }
                }
            }
            Step::Until(expr, max) => {
                let mut ok = false;
                for _ in 0..=*max {
                    match run.check(expr) {
                        Ok(true) => {
                            ok = true;
                            break;
                        }
                        Ok(false) => {}
                        Err(e) => {
                            r.failures.push(at(e));
                            break;
                        }
                    }
                    if !run.tick() {
                        break;
                    }
                }
                if !ok && r.failures.is_empty() {
                    r.failures.push(at(format!("not true within {max} ticks (from tick {tick})")));
                }
            }
            Step::Expect(expr) => match run.check(expr) {
                Ok(true) => {}
                Ok(false) => r.failures.push(at(format!("false at tick {tick}\n{}", run.summary()))),
                Err(e) => r.failures.push(at(e)),
            },
            Step::Act(a) => {
                if let Err(e) = run.act(a) {
                    r.failures.push(at(format!("refused: {e}")));
                }
            }
            Step::Refused(a, want) => match run.act(a) {
                Ok(()) => r.failures.push(at("was accepted".into())),
                Err(e) if !e.contains(want.as_str()) => r.failures.push(at(format!("refused with '{e}'"))),
                Err(_) => {}
            },
            Step::Hash(want) => {
                let got = format!("{:016x}", run.engine.world().hash());
                if &got != want {
                    r.failures.push(at(format!("hash {got} at tick {tick}")));
                }
            }
            Step::Snapshot(name) => r.snapshots.push((name.clone(), run.summary())),
            Step::SaveLoad(n) => {
                let snap = run.engine.snapshot();
                let events = run.events.clone();
                let a: Vec<u64> = (0..*n).map(|_| run.engine.tick().hash).collect();
                if let Err(e) = run.engine.restore(snap) {
                    r.failures.push(at(format!("restore failed: {}", e.join("; "))));
                    continue;
                }
                let b: Vec<u64> = (0..*n).map(|_| run.engine.tick().hash).collect();
                if a != b {
                    r.failures.push(at("the future after a restore differs".into()));
                }
                run.events = events;
            }
            Step::Probe(expr) => match run.eval(expr) {
                Ok(v) => r.notes.push(format!("tick {tick}: {expr} = {v}")),
                Err(e) => r.failures.push(at(e)),
            },
        }
        if !r.failures.is_empty() {
            break; // later steps would only repeat the first failure
        }
    }
    r.ticks = run.engine.world().tick;
    r
}
