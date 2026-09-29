//! simcraft-model: what the engine sees in a model, and the contract to paste into a view.
//!
//!   simcraft-model assets/models/termite.glb [--game games/mound --kind termite]
//!
//! Prints the model (triangles, materials and their maps, sockets, clips and their lengths, size) and checks the
//! conventions (glTF: +Y up, +Z forward, feet at y = 0). With a game and a kind, it writes the `model: (...)` block
//! for `roam.ron`: the kind's states mapped to the file's clips by name (a state `Carrying` → clip `carry`), the
//! rest to `walk` or the first clip, `idle` for standing still if there is one. Edit it, then `simcraft-check`.

use std::path::{Path, PathBuf};

use sim_gpu::model::Model;
use sim_rules::Game;

fn main() {
    let mut args = std::env::args().skip(1);
    let (mut file, mut game, mut kind) = (None, None, None);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--game" => game = args.next().map(PathBuf::from),
            "--kind" => kind = args.next(),
            _ => file = Some(PathBuf::from(a)),
        }
    }
    let Some(file) = file else {
        eprintln!("usage: simcraft-model FILE.glb [--game games/NAME --kind KIND]");
        std::process::exit(2);
    };
    let m = match Model::load(&file) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let size: Vec<f32> = (0..3).map(|a| m.max[a] - m.min[a]).collect();
    println!("{}", file.display());
    println!("  {} triangles in {} parts, {} palette slots", m.triangles(), m.parts.len(), m.slots);
    println!("  size x {:.3} y {:.3} z {:.3} (min {:?})", size[0], size[1], size[2], m.min.map(|v| (v * 1000.0).round() / 1000.0));
    for mat in &m.materials {
        let tex = |t: Option<usize>| t.map(|i| format!("{}x{}", m.textures[i].w, m.textures[i].h)).unwrap_or_else(|| "-".into());
        println!(
            "  material '{}': base {} normal {} rough/metal {} occlusion {}",
            mat.name,
            tex(mat.base_tex),
            tex(mat.normal_tex),
            tex(mat.mr_tex),
            tex(mat.occlusion_tex)
        );
    }
    for (name, c) in &m.clips {
        println!("  clip '{name}': {:.2} s, {} frames", c.duration, c.frames.len());
    }
    let sockets: Vec<&String> = m.nodes.keys().filter(|n| !n.starts_with("leg_") && !n.starts_with("antenna_")).collect();
    println!("  nodes (sockets): {sockets:?}");
    // Conventions.
    if size[2] < size[0].max(size[1]) {
        println!(
            "  warn: longest along {} — glTF's front is +Z; turn the model to face +Z (Blender: -Y) before export",
            if size[0] > size[1] { "X" } else { "Y" }
        );
    }
    if m.min[1].abs() > size[1] * 0.05 {
        println!("  warn: the lowest point is at y = {:.3}; put the feet at y = 0 (the engine stands it on a surface there)", m.min[1]);
    }
    if size.iter().any(|s| *s > 50.0) {
        println!("  warn: {size:?} units — centimetres? `length` in the view scales it anyway");
    }
    let (Some(game), Some(kind)) = (game, kind) else { return };
    let (_, g) = match Game::load(&game, None) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("{}: {e}", game.display());
            std::process::exit(1);
        }
    };
    let states = g.states_of(&kind);
    let clips: Vec<&String> = m.clips.keys().filter(|c| c.as_str() != "rest").collect();
    let default =
        clips.iter().find(|c| c.as_str() == "walk").or_else(|| clips.first()).map_or_else(|| "rest".to_string(), |c| (*c).clone());
    let mut pairs: Vec<(String, String)> = Vec::new();
    for s in states.iter().filter(|s| s.as_str() != "-") {
        let leaf = s.rsplit('.').next().unwrap_or(s).to_lowercase();
        if let Some(c) = clips.iter().find(|c| leaf.starts_with(&c.to_lowercase()) || c.to_lowercase().starts_with(&leaf)) {
            pairs.push((s.clone(), (*c).clone()));
        }
    }
    pairs.push(("*".into(), default));
    let rel = relative(&file, &game);
    println!(
        "\n// For games/{}/roam.ron, kind \"{kind}\" (its states: {states:?}):",
        game.file_name().map_or(String::new(), |n| n.to_string_lossy().to_string())
    );
    println!("\"{kind}\": (model: (");
    println!("    file: \"{rel}\",");
    println!("    length: 0.8,");
    let list: Vec<String> = pairs.iter().map(|(s, c)| format!("(\"{s}\", \"{c}\")")).collect();
    println!("    clips: [{}],", list.join(", "));
    if m.clips.contains_key("idle") {
        println!("    still: \"idle\",");
    }
    if let Some(s) = ["carry", "socket_carry", "hand_r"].iter().find(|s| m.nodes.contains_key(**s)) {
        println!("    carry: \"{s}\",");
    }
    println!(")),");
}

/// The file as a view would name it: relative to the game, or to an `assets/` folder above it.
fn relative(file: &Path, game: &Path) -> String {
    let file = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let game = game.canonicalize().unwrap_or_else(|_| game.to_path_buf());
    if let Ok(r) = file.strip_prefix(&game) {
        return r.display().to_string();
    }
    for a in game.ancestors() {
        if let Ok(r) = file.strip_prefix(a.join("assets")) {
            return r.display().to_string();
        }
    }
    file.display().to_string()
}
