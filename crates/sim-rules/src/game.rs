//! game.ron — the game designer's world: kinds, FSMs, rules.

use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename = "Game", deny_unknown_fields)]
pub struct GameDef {
    pub name: String,
    pub kinds: BTreeMap<String, KindDef>,
    #[serde(default)]
    pub fsms: BTreeMap<String, FsmDef>,
    /// Knobs the designer exposes, with defaults. The operator overrides them from engine.toml.
    #[serde(default)]
    pub params: BTreeMap<String, i64>,
    pub rules: Vec<RuleDef>,
    /// Hand-drawn map. If present, the world size comes from it.
    #[serde(default)]
    pub layout: Option<Layout>,
    /// Actions agents may request. Same shape as a rule; runs only when requested.
    #[serde(default)]
    pub actions: Vec<RuleDef>,
    /// End-of-game conditions; the first true one decides the outcome.
    #[serde(default)]
    pub end: Vec<EndDef>,
    /// Score expression for each controllable entity; summed per seat.
    #[serde(default)]
    pub score: Option<String>,
    /// Environments this game uses (`envs/<name>.ron`); merged in at load.
    #[serde(default)]
    pub environments: Vec<String>,
    /// Filled by the merge: the environments' own rules (salted after every other rule).
    #[serde(skip)]
    pub env_rules: Vec<RuleDef>,
    /// Filled by the merge: the environment kinds, in `environments` order.
    #[serde(skip)]
    pub env_kinds: Vec<String>,
}

/// An environment: the world's own state machine, written like a game and shared between games.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename = "Environment", deny_unknown_fields)]
pub struct EnvDef {
    pub name: String,
    #[serde(default)]
    pub params: BTreeMap<String, i64>,
    #[serde(default)]
    pub props: BTreeMap<String, i64>,
    #[serde(default)]
    pub fsm: Option<String>,
    #[serde(default)]
    pub fsms: BTreeMap<String, FsmDef>,
    /// Rules of the environment itself (no `for`).
    #[serde(default)]
    pub rules: Vec<RuleDef>,
}

impl GameDef {
    /// Merges environments in as ordinary parts: a hidden kind, its machines, params and rules.
    /// A name clash with the game is an error.
    pub fn merge_envs(&mut self, envs: &[EnvDef]) -> Result<(), Vec<String>> {
        let mut errs = Vec::new();
        for name in self.environments.clone() {
            let Some(env) = envs.iter().find(|e| e.name == name) else {
                errs.push(format!("environment '{name}' not found (envs/{name}.ron)"));
                continue;
            };
            if self.kinds.contains_key(&name) {
                errs.push(format!("environment '{name}': the game already has a kind named '{name}'"));
                continue;
            }
            self.kinds.insert(
                name.clone(),
                KindDef {
                    glyph: ' ',
                    props: env.props.clone(),
                    fsm: env.fsm.clone(),
                    solid: false,
                    hidden: true,
                    glyphs: BTreeMap::new(),
                },
            );
            for (k, v) in &env.params {
                if self.params.insert(k.clone(), *v).is_some() {
                    errs.push(format!("environment '{name}': param '{k}' is also a game param"));
                }
            }
            for (k, f) in &env.fsms {
                if self.fsms.insert(k.clone(), f.clone()).is_some() {
                    errs.push(format!("environment '{name}': machine '{k}' is also a game machine"));
                }
            }
            for r in &env.rules {
                if r.for_kind.is_some() {
                    errs.push(format!("environment '{name}': rule '{}' belongs to the environment; drop `for`", r.name));
                }
                self.env_rules.push(RuleDef { for_kind: Some(name.clone()), ..r.clone() });
            }
            self.env_kinds.push(name);
        }
        if errs.is_empty() { Ok(()) } else { Err(errs) }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    /// glyph → kind or (kind, {prop: value}). '.' and ' ' are empty cells.
    pub legend: BTreeMap<char, Legend>,
    pub rows: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Legend {
    Kind(String),
    /// Props specific to entities at this glyph (e.g. `("village", {"owner": 1})`).
    With(String, BTreeMap<String, i64>),
}

impl Legend {
    pub fn kind(&self) -> &str {
        match self {
            Legend::Kind(k) | Legend::With(k, _) => k,
        }
    }
    pub fn props(&self) -> Option<&BTreeMap<String, i64>> {
        match self {
            Legend::Kind(_) => None,
            Legend::With(_, p) => Some(p),
        }
    }
}

impl Layout {
    pub fn size(&self) -> (i64, i64) {
        let w = self.rows.iter().map(|r| r.chars().count()).max().unwrap_or(0);
        (w as i64, self.rows.len() as i64)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndDef {
    /// World-level Rhai expression (`count`, `p`, `tick`) → bool.
    pub when: String,
    pub result: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KindDef {
    pub glyph: char,
    #[serde(default)]
    pub props: BTreeMap<String, i64>,
    #[serde(default)]
    pub fsm: Option<String>,
    /// At most one solid per cell; solids cannot pass through each other.
    #[serde(default)]
    pub solid: bool,
    /// Not drawn (environments).
    #[serde(default)]
    pub hidden: bool,
    /// Glyph per state (else `glyph`).
    #[serde(default)]
    pub glyphs: BTreeMap<String, char>,
}

/// A machine or a state inside one. A flat machine is just `(initial, transitions)`;
/// everything else is optional (see architecture.md, State machines).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateDef {
    #[serde(default)]
    pub initial: Option<String>,
    /// Child states (submachine).
    #[serde(default)]
    pub states: BTreeMap<String, StateDef>,
    /// Machines running side by side.
    #[serde(default)]
    pub layers: BTreeMap<String, StateDef>,
    /// Mount another machine here.
    #[serde(default, rename = "use")]
    pub uses: Option<String>,
    /// On return, resume from the child you left.
    #[serde(default)]
    pub remember: bool,
    #[serde(default)]
    pub pick: Option<PickDef>,
    /// `pick` is re-evaluated every tick.
    #[serde(default)]
    pub recheck: bool,
    #[serde(default)]
    pub enter: Vec<Do>,
    #[serde(default)]
    pub exit: Vec<Do>,
    #[serde(default)]
    pub transitions: Vec<TransitionDef>,
    /// Rules living in this state (no `for`).
    #[serde(default)]
    pub rules: Vec<RuleDef>,
}

pub type FsmDef = StateDef;

#[derive(Debug, Clone, Deserialize)]
pub enum PickDef {
    /// (state, condition): first that holds.
    First(Vec<(String, String)>),
    /// (state, score): highest.
    Best(Vec<(String, String)>),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransitionDef {
    /// Child or path (`"Dev.Polish"`), `"*"` = any.
    pub from: String,
    #[serde(default)]
    pub to: Option<String>,
    /// Rhai expression → bool. The first matching transition at a level wins.
    pub when: String,
    #[serde(default)]
    pub then: Vec<Do>,
    /// Remember the place at this level before leaving.
    #[serde(default)]
    pub interrupt: bool,
    /// Instead of `to`: return to the remembered place.
    #[serde(default)]
    pub back: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleDef {
    pub name: String,
    /// Which kind it applies to; "*" for all. Absent in rules written inside a state.
    #[serde(rename = "for", default)]
    pub for_kind: Option<String>,
    /// State selector: `"Work"` covers Work and everything inside it; `"Awake.Work"` is a path.
    #[serde(default)]
    pub state: Option<String>,
    /// The active state may be at most this deep below the bound state.
    #[serde(default)]
    pub depth: Option<usize>,
    /// Target: `Nearest(kind)`. If none, the rule does not fire. `it` in expressions (incl. `it.dist`).
    #[serde(default)]
    pub target: Option<Target>,
    /// Actions only: arguments the agent must give. `arg.<name>` in expressions.
    #[serde(default)]
    pub args: Vec<String>,
    /// Rhai expression → bool. If absent, fires every tick.
    #[serde(default)]
    pub when: Option<String>,
    /// Layer A: declarative actions.
    #[serde(default)]
    pub then: Vec<Do>,
    /// Layer B (escape hatch): a Rhai script returning an array of effect maps.
    #[serde(default)]
    pub script: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub enum Do {
    /// prop = expr
    Set(String, String),
    /// prop += expr
    Add(String, String),
    Emit(String),
    Despawn(Target),
    /// Spawns a new kind at its own position.
    Spawn(String),
    MoveToward(String),
    MoveAway(String),
    /// One step up a gradient: to the neighbouring cell whose `kind` has the highest `prop`.
    Climb(String, String),
    Wander,
    /// Changes the FSM state (together with its effect: `[Goto("Fire"), Emit("lightning")]`).
    Goto(String),
    /// One step; dx, dy are expressions (e.g. `Move("arg.dx", "arg.dy")`).
    Move(String, String),
    /// Applies the inner actions to another entity: `On(It, [Add("hp", "-3")])`.
    On(Target, Vec<Do>),
    /// prop must be >= expr. Checked at request time and at apply time (on live state);
    /// if it fails the whole group is dropped (two buyers, one stock, same tick).
    Need(String, String),
    /// Remember the place at this level and go to the state; `Back` returns.
    Interrupt(String),
    /// Return to the most recently remembered interrupt.
    Back,
}

#[derive(Debug, Clone, Deserialize)]
pub enum Target {
    /// The rule's owner.
    Me,
    /// The rule's `target`.
    It,
    Nearest(String),
    /// Nearest kind in that state: `NearestIn("project", "Blocked")`.
    NearestIn(String, String),
}
