//! simtest: run scenario files and print a report.
//!
//!   cargo run -p simtest                          # every file in test/scenarios
//!   cargo run -p simtest -- path/to/scenario.ron  # one file
//!
//! Snapshots are printed here; `cargo test -p simtest` compares them with the accepted ones (insta).

use std::path::PathBuf;

fn main() {
    let args: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    let files = if args.is_empty() { simtest::scenario_files() } else { args };
    let (mut passed, mut failed) = (0, 0);
    for f in &files {
        let report = match simtest::load_scenario(f) {
            Ok(s) => simtest::run(&s),
            Err(e) => simtest::Report { name: f.display().to_string(), failures: vec![e], ..Default::default() },
        };
        let mark = if report.passed() { "ok  " } else { "FAIL" };
        println!("{mark} {:<48} {} ticks   {}", report.name, report.ticks, f.display());
        for n in &report.notes {
            println!("       probe  {n}");
        }
        for (name, text) in &report.snapshots {
            println!("       snapshot \"{name}\":");
            text.lines().for_each(|l| println!("         {l}"));
        }
        for fail in &report.failures {
            fail.lines().for_each(|l| println!("       {l}"));
        }
        if report.passed() { passed += 1 } else { failed += 1 }
    }
    println!("\n{passed} passed, {failed} failed");
    std::process::exit(i32::from(failed > 0));
}
