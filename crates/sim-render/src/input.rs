//! Input: devices produce actions; contexts send actions to the view or to the game. As data (`input.ron`).
//! Modelled on Unreal's Enhanced Input and Unity's Input System; see docs/architecture.md, "Input".

use std::collections::BTreeMap;

use serde::Deserialize;

/// A physical control, named the same on every backend.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub enum Binding {
    /// One key: "a".."z", "0".."9", "space", "tab", "enter", "esc", "up", "down", "left", "right", "plus",
    /// "minus", "[", "]".
    Key(String),
    /// Four keys as one 2D direction.
    Dpad { up: String, down: String, left: String, right: String },
    Mouse(MouseButton),
    /// Any of several bindings (e.g. `Any([Key("q"), Key("esc")])`).
    Any(Vec<Binding>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// What an action does.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub enum Target {
    View(ViewAction),
    /// A declared game action of the selected entity. `args`: argument → "x" / "y" (the 2D input) or a number.
    Game {
        #[serde(rename = "do")]
        action: String,
        #[serde(default)]
        args: BTreeMap<String, String>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum ViewAction {
    Pause,
    Faster,
    Slower,
    Step,
    SelectNext,
    Pan,
    Orbit,
    Perspective,
    CutIn,
    CutOut,
    Quit,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    pub name: String,
    /// Active when this holds (view: `paused`, `selected.kind`, `selected.controllable`, `selected.state`;
    /// world: `count`, `env`, `tick`...). No condition: always active.
    #[serde(default)]
    pub when: Option<String>,
    pub actions: BTreeMap<String, Target>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename = "Input", deny_unknown_fields)]
pub struct InputMap {
    pub scheme: String,
    pub schemes: BTreeMap<String, BTreeMap<String, Binding>>,
    pub contexts: Vec<Context>,
}

/// A device event, backend-neutral.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Key(String),
    Mouse(MouseButton, u16, u16),
}

/// A resolved action: which one, where it goes, its 2D value (for a D-pad), and where a mouse event was.
#[derive(Clone, Debug, PartialEq)]
pub struct Fired {
    pub action: String,
    pub context: String,
    pub target: Target,
    pub value: (i64, i64),
    pub at: Option<(u16, u16)>,
}

impl Binding {
    /// Does this event trigger the binding? Returns the 2D value (0, 0 for buttons).
    fn matches(&self, e: &Event) -> Option<(i64, i64)> {
        match (self, e) {
            (Binding::Key(k), Event::Key(p)) if k == p => Some((0, 0)),
            (Binding::Dpad { up, down, left, right }, Event::Key(p)) => {
                if p == up {
                    Some((0, -1))
                } else if p == down {
                    Some((0, 1))
                } else if p == left {
                    Some((-1, 0))
                } else if p == right {
                    Some((1, 0))
                } else {
                    None
                }
            }
            (Binding::Mouse(b), Event::Mouse(m, ..)) if b == m => Some((0, 0)),
            (Binding::Any(v), e) => v.iter().find_map(|b| b.matches(e)),
            _ => None,
        }
    }
}

impl InputMap {
    /// Reads an input.ron (optional fields need no `Some(...)`).
    pub fn parse(src: &str) -> Result<InputMap, String> {
        ron::Options::default()
            .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
            .from_str(src)
            .map_err(|e| e.to_string())
    }

    /// The viewer's built-in controls, for games without an input.ron.
    pub fn builtin() -> InputMap {
        InputMap::parse(include_str!("../input.ron")).expect("the built-in input.ron parses")
    }

    /// Checks names: the scheme exists, every context action is bound in every scheme... at least in one.
    pub fn check(&self) -> Result<(), Vec<String>> {
        let mut errs = Vec::new();
        if !self.schemes.contains_key(&self.scheme) {
            errs.push(format!("input: scheme '{}' is not defined (schemes: {:?})", self.scheme, self.schemes.keys().collect::<Vec<_>>()));
        }
        for c in &self.contexts {
            for a in c.actions.keys() {
                if !self.schemes.values().any(|s| s.contains_key(a)) {
                    errs.push(format!("input: context '{}' uses action '{a}', which no scheme binds", c.name));
                }
            }
        }
        if errs.is_empty() { Ok(()) } else { Err(errs) }
    }

    /// Resolves an event: the active scheme's actions it triggers, then the first active context that binds one.
    /// `active` answers whether a context's `when` holds right now.
    pub fn resolve(&self, e: &Event, mut active: impl FnMut(&Context) -> bool) -> Option<Fired> {
        let scheme = self.schemes.get(&self.scheme)?;
        let triggered: Vec<(&String, (i64, i64))> = scheme.iter().filter_map(|(a, b)| b.matches(e).map(|v| (a, v))).collect();
        if triggered.is_empty() {
            return None;
        }
        for c in &self.contexts {
            if !active(c) {
                continue;
            }
            if let Some((a, v)) = triggered.iter().find(|(a, _)| c.actions.contains_key(*a)) {
                let at = match e {
                    Event::Mouse(_, x, y) => Some((*x, *y)),
                    Event::Key(_) => None,
                };
                return Some(Fired { action: (*a).clone(), context: c.name.clone(), target: c.actions[*a].clone(), value: *v, at });
            }
        }
        None
    }
}

impl InputMap {
    /// One line of help from the active scheme: "wasd pan · space pause · ...".
    pub fn help(&self) -> String {
        let Some(scheme) = self.schemes.get(&self.scheme) else { return String::new() };
        let name = |b: &Binding| -> String {
            match b {
                Binding::Key(k) => k.clone(),
                Binding::Dpad { up, down, left, right } if [up, left, down, right].iter().all(|k| k.len() == 1) => {
                    format!("{up}{left}{down}{right}")
                }
                Binding::Dpad { .. } => "arrows".into(),
                Binding::Mouse(m) => format!("{m:?} click").to_lowercase(),
                Binding::Any(v) => v.iter().map(|b| match b {
                    Binding::Key(k) => k.clone(),
                    Binding::Mouse(m) => format!("{m:?} click").to_lowercase(),
                    _ => "…".into(),
                }).collect::<Vec<_>>().join("/"),
            }
        };
        let mut parts: Vec<String> = scheme.iter().map(|(action, b)| format!("{} {}", name(b), action.replace('_', " "))).collect();
        parts.sort();
        format!("{} · scheme {}", parts.join(" · "), self.scheme)
    }
}

/// The value of a game action's argument from the input ("x", "y", or a number).
pub fn arg_value(spec: &str, value: (i64, i64)) -> Option<i64> {
    match spec {
        "x" => Some(value.0),
        "y" => Some(value.1),
        n => n.parse().ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map() -> InputMap {
        InputMap::parse(
            r#"Input(
                scheme: "right_hand",
                schemes: {
                    "right_hand": { "pan": Dpad(up: "w", down: "s", left: "a", right: "d"), "pause": Key("space") },
                    "left_hand":  { "pan": Dpad(up: "up", down: "down", left: "left", right: "right"), "pause": Key("enter") },
                },
                contexts: [
                    (name: "drive", when: "selected.controllable", actions: {
                        "pan": Game(do: "move", args: { "dx": "x", "dy": "y" }),
                    }),
                    (name: "watch", actions: { "pan": View(Pan), "pause": View(Pause) }),
                ],
            )"#,
        )
        .expect("parses")
    }

    #[test]
    fn a_dpad_is_one_2d_action_and_contexts_decide_where_it_goes() {
        let m = map();
        let drive = m.resolve(&Event::Key("a".into()), |_| true).unwrap();
        assert_eq!((drive.context.as_str(), drive.value), ("drive", (-1, 0)));
        assert!(matches!(drive.target, Target::Game { ref action, .. } if action == "move"));
        let watch = m.resolve(&Event::Key("a".into()), |c| c.when.is_none()).unwrap();
        assert_eq!((watch.context.as_str(), watch.target.clone()), ("watch", Target::View(ViewAction::Pan)));
        // An action only the lower context binds falls through to it.
        let pause = m.resolve(&Event::Key("space".into()), |_| true).unwrap();
        assert_eq!(pause.target, Target::View(ViewAction::Pause));
    }

    #[test]
    fn schemes_rebind_the_same_actions() {
        let mut m = map();
        m.scheme = "left_hand".into();
        assert!(m.resolve(&Event::Key("a".into()), |_| true).is_none(), "wasd is not bound for the left hand");
        assert_eq!(m.resolve(&Event::Key("up".into()), |_| true).unwrap().value, (0, -1));
        assert_eq!(arg_value("y", (0, -1)), Some(-1));
        assert_eq!(arg_value("3", (0, 0)), Some(3));
    }

    #[test]
    fn the_builtin_controls_load_and_check() {
        let m = InputMap::builtin();
        m.check().expect("consistent");
        assert!(m.schemes.len() >= 2, "right and left hand");
    }
}
