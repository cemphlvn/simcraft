use super::*;

type S = Spec<&'static str, &'static str>;

/// Koşul adı → değer. Listede olmayan koşul yanlış, puanı 0.
#[derive(Default)]
struct Facts(BTreeMap<&'static str, i64>);

impl Oracle<&'static str> for Facts {
    fn test(&mut self, g: &&'static str, _: u64) -> Result<bool, String> {
        Ok(*g == "true" || self.0.get(g).is_some_and(|v| *v != 0))
    }
    fn score(&mut self, g: &&'static str, _: u64) -> Result<i64, String> {
        Ok(self.0.get(g).copied().unwrap_or(0))
    }
}

fn facts(kv: &[(&'static str, i64)]) -> Facts {
    Facts(kv.iter().copied().collect())
}

fn leaf() -> S {
    S::default()
}

fn named(enter: &'static str, exit: &'static str) -> S {
    S { enter: vec![enter], exit: vec![exit], ..S::default() }
}

fn t(from: &str, to: &str, when: &'static str) -> TransitionSpec<&'static str, &'static str> {
    TransitionSpec { from: from.into(), to: Some(to.into()), back: false, interrupt: false, when, then: vec![] }
}

fn states(v: Vec<(&str, S)>) -> Vec<(String, S)> {
    v.into_iter().map(|(n, s)| (n.to_string(), s)).collect()
}

fn chart(root: S, extra: Vec<(&str, S)>) -> Chart<&'static str, &'static str> {
    let mut ms: BTreeMap<String, S> = extra.into_iter().map(|(n, s)| (n.to_string(), s)).collect();
    ms.insert("m".into(), root);
    Chart::build("m", &ms).unwrap_or_else(|e| panic!("{e:?}"))
}

fn step(c: &Chart<&'static str, &'static str>, state: &str, f: &mut Facts) -> (String, Vec<&'static str>) {
    let out = c.step(&c.decode(state).expect("decodes"), f).expect("steps");
    (c.encode(&out.mem), out.actions)
}

#[test]
fn flat_machine_state_is_the_leaf_name() {
    let c = chart(
        S {
            initial: Some("Roam".into()),
            states: states(vec![("Hunt", leaf()), ("Roam", leaf())]),
            transitions: vec![t("Roam", "Hunt", "hungry")],
            ..S::default()
        },
        vec![],
    );
    assert_eq!(c.encode(&c.initial()), "Roam");
    assert_eq!(step(&c, "Roam", &mut facts(&[])).0, "Roam");
    assert_eq!(step(&c, "Roam", &mut facts(&[("hungry", 1)])).0, "Hunt");
}

fn life() -> S {
    S {
        initial: Some("Awake".into()),
        states: states(vec![
            (
                "Awake",
                S {
                    initial: Some("Work".into()),
                    enter: vec!["enter Awake"],
                    exit: vec!["exit Awake"],
                    states: states(vec![
                        (
                            "Work",
                            S {
                                initial: Some("Design".into()),
                                remember: true,
                                states: states(vec![("Build", named("enter Build", "exit Build")), ("Design", leaf())]),
                                transitions: vec![t("Design", "Build", "designed")],
                                ..S::default()
                            },
                        ),
                        ("Break", named("enter Break", "exit Break")),
                        ("Stuck", leaf()),
                    ]),
                    transitions: vec![
                        t("Work", "Break", "tired"),
                        t("Break", "Work", "rested"),
                        TransitionSpec { interrupt: true, ..t("Work", "Stuck", "wall") },
                        TransitionSpec { to: None, back: true, ..t("Stuck", "", "unblocked") },
                    ],
                    ..S::default()
                },
            ),
            ("Asleep", leaf()),
        ]),
        transitions: vec![t("Awake", "Asleep", "night"), t("Asleep", "Awake", "morning")],
        ..S::default()
    }
}

#[test]
fn outer_transitions_win_and_inherit_to_children() {
    let c = chart(life(), vec![]);
    assert_eq!(c.encode(&c.initial()), "Awake.Work.Design");
    // `night` is declared on the root, from Awake: it applies deep inside Awake too, and beats `designed`.
    let (s, acts) = step(&c, "Awake.Work.Design", &mut facts(&[("night", 1), ("designed", 1)]));
    assert_eq!(s, "Asleep#Awake.Work=Design");
    assert_eq!(acts, vec!["exit Awake"]);
}

#[test]
fn enter_and_exit_run_in_order_and_unchanged_parents_stay() {
    let c = chart(life(), vec![]);
    let (s, acts) = step(&c, "Awake.Work.Build", &mut facts(&[("tired", 1)]));
    assert_eq!(s, "Awake.Break#Awake.Work=Build");
    assert_eq!(acts, vec!["exit Build", "enter Break"], "Awake itself is neither left nor entered");
}

#[test]
fn remember_resumes_the_child_you_left() {
    let c = chart(life(), vec![]);
    let (s, _) = step(&c, "Awake.Break#Awake.Work=Build", &mut facts(&[("rested", 1)]));
    assert_eq!(s, "Awake.Work.Build#Awake.Work=Build");
}

#[test]
fn interrupt_and_back_return_to_the_exact_place() {
    let c = chart(life(), vec![]);
    let (s, _) = step(&c, "Awake.Work.Build", &mut facts(&[("wall", 1)]));
    assert_eq!(s, "Awake.Stuck#Awake.Work=Build^Awake=Awake.Work.Build");
    let (s, acts) = step(&c, &s, &mut facts(&[("unblocked", 1)]));
    assert_eq!(s, "Awake.Work.Build#Awake.Work=Build");
    assert_eq!(acts, vec!["enter Build"]);
}

#[test]
fn back_with_nothing_saved_takes_the_default_entry() {
    let c = chart(life(), vec![]);
    let (s, _) = step(&c, "Awake.Stuck", &mut facts(&[("unblocked", 1)]));
    assert_eq!(s, "Awake.Work.Design");
}

#[test]
fn leaving_a_level_drops_interrupts_saved_inside_it() {
    let c = chart(life(), vec![]);
    let (s, _) = step(&c, "Awake.Stuck^Awake=Awake.Work.Build", &mut facts(&[("night", 1)]));
    assert_eq!(s, "Asleep");
}

#[test]
fn nested_interrupts_stack_like_recursion() {
    let c = chart(life(), vec![]);
    let ids = c.resolve("Stuck");
    let m = c.decode("Awake.Work.Build").expect("decodes");
    let once = c.interrupt(&m, ids[0], &mut NoOracle).expect("ok").mem;
    let twice = c.interrupt(&once, ids[0], &mut NoOracle).expect("ok").mem;
    assert_eq!(twice.stack.len(), 2, "interrupting into Stuck from Stuck saves twice");
    let back = c.back(&twice, &mut NoOracle).expect("ok").mem;
    assert_eq!(c.encode(&back), c.encode(&once));
    let home = c.back(&back, &mut NoOracle).expect("ok").mem;
    assert_eq!(c.encode(&home), "Awake.Work.Build#Awake.Work=Build");
}

#[test]
fn layers_run_side_by_side_one_transition_each() {
    let mood = S {
        initial: Some("Calm".into()),
        states: states(vec![("Calm", leaf()), ("Angry", leaf())]),
        transitions: vec![t("Calm", "Angry", "hurt")],
        ..S::default()
    };
    let body = S {
        initial: Some("Idle".into()),
        states: states(vec![("Idle", leaf()), ("Walk", leaf())]),
        transitions: vec![t("Idle", "Walk", "go")],
        ..S::default()
    };
    let c = chart(S { layers: states(vec![("Body", body), ("Mood", mood)]), ..S::default() }, vec![]);
    assert_eq!(c.encode(&c.initial()), "Body.Idle|Mood.Calm");
    let (s, _) = step(&c, "Body.Idle|Mood.Calm", &mut facts(&[("hurt", 1), ("go", 1)]));
    assert_eq!(s, "Body.Walk|Mood.Angry");
    assert!(in_label(&s, "Angry") && in_label(&s, "Body") && !in_label(&s, "Calm"));
}

#[test]
fn use_mounts_a_machine_everywhere_and_tracks_origins() {
    let focus = S {
        initial: Some("Warmup".into()),
        enter: vec!["reset focus"],
        states: states(vec![("Warmup", leaf()), ("Flow", leaf())]),
        transitions: vec![t("Warmup", "Flow", "focused")],
        ..S::default()
    };
    let root = S {
        initial: Some("Design".into()),
        states: states(vec![
            ("Build", S { uses: Some("focus".into()), ..S::default() }),
            ("Design", S { uses: Some("focus".into()), ..S::default() }),
        ]),
        transitions: vec![t("Design", "Build", "designed")],
        ..S::default()
    };
    let c = chart(root, vec![("focus", focus)]);
    assert_eq!(c.encode(&c.initial()), "Design.Warmup");
    assert_eq!(c.with_origin("focus", "").len(), 2, "one per mount");
    assert_eq!(c.with_origin("focus", "Flow").len(), 2);
    let (s, acts) = step(&c, "Design.Flow", &mut facts(&[("designed", 1)]));
    assert_eq!(s, "Build.Warmup");
    assert_eq!(acts, vec!["reset focus"]);
    assert_eq!(c.resolve("Flow").len(), 2, "a bare name matches both mounts");
    assert_eq!(c.resolve("Build.Flow").len(), 1, "a path picks one");
}

#[test]
fn machine_loops_are_rejected() {
    let a = S { initial: Some("X".into()), states: states(vec![("X", S { uses: Some("b".into()), ..S::default() })]), ..S::default() };
    let b = S { initial: Some("Y".into()), states: states(vec![("Y", S { uses: Some("m".into()), ..S::default() })]), ..S::default() };
    let mut ms = BTreeMap::new();
    ms.insert("m".to_string(), a);
    ms.insert("b".to_string(), b);
    let errs = Chart::build("m", &ms).expect_err("loop");
    assert!(errs.iter().any(|e| e.contains("machine loop m -> b -> m")), "{errs:?}");
}

#[test]
fn pick_first_best_and_recheck() {
    let root = S {
        pick: Some(PickSpec { kind: PickKind::Best, options: vec![("Eat".into(), "hunger"), ("Sleep".into(), "fatigue")] }),
        recheck: true,
        states: states(vec![("Eat", leaf()), ("Sleep", leaf())]),
        ..S::default()
    };
    let c = chart(root, vec![]);
    assert_eq!(c.encode(&c.initial()), "Eat", "no scores at birth: the first option");
    assert_eq!(step(&c, "Eat", &mut facts(&[("fatigue", 5)])).0, "Sleep");
    assert_eq!(step(&c, "Sleep", &mut facts(&[("hunger", 5), ("fatigue", 5)])).0, "Sleep", "a tie keeps the current");
    assert_eq!(step(&c, "Sleep", &mut facts(&[("hunger", 6), ("fatigue", 5)])).0, "Eat");

    let first = S {
        pick: Some(PickSpec { kind: PickKind::First, options: vec![("Flee".into(), "danger"), ("Graze".into(), "true")] }),
        recheck: true,
        states: states(vec![("Flee", leaf()), ("Graze", leaf())]),
        ..S::default()
    };
    let c = chart(first, vec![]);
    assert_eq!(step(&c, "Graze", &mut facts(&[("danger", 1)])).0, "Flee");
    assert_eq!(step(&c, "Flee", &mut facts(&[])).0, "Graze");
}

#[test]
fn steps_count_transitions_to_a_state() {
    let c = chart(life(), vec![]);
    let m = c.decode("Awake.Work.Design").expect("decodes");
    assert_eq!(c.steps_to(&m, &c.resolve("Design")), 0);
    assert_eq!(c.steps_to(&m, &c.resolve("Build")), 1);
    assert_eq!(c.steps_to(&m, &c.resolve("Asleep")), 1, "inherited from Awake");
    assert_eq!(c.steps_to(&m, &c.resolve("Break")), 1);
    let asleep = c.decode("Asleep").expect("decodes");
    assert_eq!(c.steps_to(&asleep, &c.resolve("Build")), 1, "Work remembers: waking can resume straight into Build");
    assert_eq!(c.steps_to(&asleep, &c.resolve("Stuck")), 2, "wake up, then hit the wall");
}

#[test]
fn depth_and_selectors() {
    let c = chart(life(), vec![]);
    let m = c.decode("Awake.Work.Build").expect("decodes");
    let work = c.resolve("Work");
    assert!(c.in_any(&m, &work, None));
    assert!(c.in_any(&m, &work, Some(1)));
    assert!(!c.in_any(&m, &work, Some(0)), "Build is one level below Work");
    assert_eq!(depth_in_label("Awake.Work.Build", "Work"), 1);
    assert_eq!(depth_in_label("Awake.Work.Build", "Awake"), 2);
    assert_eq!(depth_in_label("Awake.Work.Build", "Asleep"), -1);
    assert!(in_label("Awake.Work.Build#Awake.Work=Design", "Awake.Work"));
    assert!(!in_label("Awake.Work.Build#Awake.Work=Design", "Design"), "remembered is not active");
}

#[test]
fn memory_round_trips() {
    let c = chart(life(), vec![]);
    for s in ["Asleep", "Awake.Stuck#Awake.Work=Build^Awake=Awake.Work.Build", "Awake.Work.Design"] {
        assert_eq!(c.encode(&c.decode(s).expect("decodes")), s);
    }
    assert!(c.decode("Awake.Nope").is_err());
}

#[test]
fn bad_definitions_say_what_is_wrong() {
    let root = S {
        initial: Some("Nope".into()),
        states: states(vec![("A.B", leaf())]),
        transitions: vec![t("A.B", "Missing", "x")],
        ..S::default()
    };
    let mut ms = BTreeMap::new();
    ms.insert("m".to_string(), root);
    let errs = Chart::build("m", &ms).expect_err("invalid");
    let all = errs.join("\n");
    assert!(all.contains("names may not"), "{all}");
    assert!(all.contains("initial 'Nope'"), "{all}");
    assert!(all.contains("no state 'Missing'"), "{all}");
}
