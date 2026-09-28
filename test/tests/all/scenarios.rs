//! Every scenario in test/scenarios, one test run; snapshots are compared with the accepted ones (insta).

#[test]
fn scenarios() {
    let files = simtest::scenario_files();
    assert!(!files.is_empty(), "no scenarios found");
    let mut failures = Vec::new();
    for f in &files {
        let stem = f.file_stem().and_then(|s| s.to_str()).unwrap_or("scenario").to_string();
        let report = match simtest::load_scenario(f) {
            Ok(s) => simtest::run(&s),
            Err(e) => {
                failures.push(e);
                continue;
            }
        };
        for fail in &report.failures {
            failures.push(format!("{} ({}): {fail}", report.name, f.display()));
        }
        let mut settings = insta::Settings::clone_current();
        settings.set_snapshot_path(simtest::repo_root().join("test/snapshots"));
        settings.set_prepend_module_to_snapshot(false);
        settings.set_description(report.name.clone());
        settings.bind(|| {
            for (name, text) in &report.snapshots {
                let slug: String = name.chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect();
                insta::assert_snapshot!(format!("{stem}__{slug}"), text);
            }
        });
    }
    assert!(failures.is_empty(), "{} scenario failures:\n{}", failures.len(), failures.join("\n\n"));
}

/// The runner must report failures precisely, not only pass things.
#[test]
fn the_runner_reports_what_went_wrong() {
    let s = simtest::parse_scenario(
        r#"Scenario(name: "x", game: "games/wolf_sheep", steps: [
            Step(10), Expect("count.wolf > 1000"), Expect("never reached"),
        ])"#,
    )
    .unwrap();
    let r = simtest::run(&s);
    assert_eq!(r.failures.len(), 1, "stops at the first failure: {:?}", r.failures);
    assert!(r.failures[0].contains("step 2") && r.failures[0].contains("false at tick 10"), "{}", r.failures[0]);

    let s = simtest::parse_scenario(r#"Scenario(name: "x", game: "games/wolf_sheep", steps: [ Until("count.wolf > 1000", 5) ])"#).unwrap();
    assert!(simtest::run(&s).failures[0].contains("not true within 5 ticks"));

    let s = simtest::parse_scenario(r#"Scenario(name: "x", game: "games/wolf_sheep", steps: [ Hash("0000000000000000") ])"#).unwrap();
    assert!(simtest::run(&s).failures[0].contains("hash"));

    let s = simtest::parse_scenario(r#"Scenario(name: "x", game: "games/wolf_sheep", steps: [ Expect("cuont.wolf > 0") ])"#).unwrap();
    assert!(simtest::run(&s).failures[0].contains("cuont"), "an expression typo names itself");

    let s = simtest::parse_scenario(r#"Scenario(name: "x", game: "games/wolf_sheep", expect_error: "nothing like this")"#).unwrap();
    assert!(simtest::run(&s).failures[0].contains("but the game loaded"));
}
