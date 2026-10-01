//! Games as data: the player reads simcraft games (`game.ron` + `engine.toml` per level) from a folder that
//! travels with the app, so a game never adds native code (`docs/architecture.md`, Mobile core). On iOS the folder
//! is `Games/` inside the app bundle (copied there by `tools/mobile/build.sh` from `SIMCRAFT_GAMES`); on the
//! desktop it is `SIMCRAFT_GAMES` itself. A game folder holds one folder per level (any depth) and optionally
//! `order.txt`: the levels to play first, one path per line; the rest follow in name order.

use std::path::{Path, PathBuf};

/// One level: its name (its path in the game folder) and its two files.
#[derive(Clone, Debug)]
pub struct Level {
    pub name: String,
    pub game_ron: String,
    pub engine_toml: String,
}

/// Where the bundled games are, if anywhere.
pub fn root() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("SIMCRAFT_GAMES") {
        return Some(PathBuf::from(dir));
    }
    // iOS: the executable sits at the top of the app bundle, next to its resources.
    let exe = std::env::current_exe().ok()?;
    let games = exe.parent()?.join("Games");
    games.is_dir().then_some(games)
}

/// Every level under `dir`, in play order.
pub fn levels(dir: &Path) -> Vec<Level> {
    let mut found = Vec::new();
    collect(dir, dir, &mut found);
    found.sort_by(|a, b| a.name.cmp(&b.name));
    let order: Vec<String> = std::fs::read_to_string(dir.join("order.txt"))
        .map(|s| s.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect())
        .unwrap_or_default();
    let mut out: Vec<Level> = order.iter().filter_map(|n| found.iter().find(|l| &l.name == n).cloned()).collect();
    out.extend(found.into_iter().filter(|l| !order.contains(&l.name)));
    out
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<Level>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    for p in paths {
        if p.is_dir() {
            collect(root, &p, out);
        }
    }
    let (game, engine) = (dir.join("game.ron"), dir.join("engine.toml"));
    if let (Ok(game_ron), Ok(engine_toml)) = (std::fs::read_to_string(&game), std::fs::read_to_string(&engine)) {
        let name = dir.strip_prefix(root).unwrap_or(dir).to_string_lossy().replace('\\', "/");
        out.push(Level { name, game_ron, engine_toml });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_follow_order_txt_then_names() {
        let dir = std::env::temp_dir().join(format!("sim-mobile-games-{}", std::process::id()));
        for name in ["b/one", "a/two", "c/three"] {
            let d = dir.join(name);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("game.ron"), "g").unwrap();
            std::fs::write(d.join("engine.toml"), "e").unwrap();
        }
        std::fs::write(dir.join("order.txt"), "c/three\nmissing/level\n").unwrap();
        let names: Vec<String> = levels(&dir).into_iter().map(|l| l.name).collect();
        assert_eq!(names, vec!["c/three", "a/two", "b/one"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
