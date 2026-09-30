//! The physics layer against real data: tracks and cars that games ship.

use sim_physics::{Fx, Track, TrackDef};

fn track(path: &str) -> Track {
    let text = std::fs::read_to_string(format!("{}/../{path}", env!("CARGO_MANIFEST_DIR"))).expect(path);
    let def: TrackDef = ron::from_str(&text).unwrap_or_else(|e| panic!("{path}: {e}"));
    Track::new(&def).unwrap_or_else(|e| panic!("{path}: {e:#?}"))
}

#[test]
fn the_charlotte_oval_closes_at_its_published_length() {
    let t = track("games/race/tracks/charlotte.ron");
    let miles = t.length.0 as f64 / 65536.0 / 1609.344;
    assert!((miles - 1.5).abs() < 0.0005, "{miles} mi");
    // Round the lap on the racing line's two edges and the centre: every point comes back to where it was.
    for i in 0..500 {
        let s = Fx(t.length.0 * i / 500);
        for off in [-8, 0, 8] {
            let p = t.pose(s, Fx::int(off));
            let at = t.locate(p.x, p.y, None);
            let ds = ((at.s - s).0.abs()).min(t.length.0 - (at.s - s).0.abs());
            assert!(ds < Fx::ratio(1, 100).0 && (at.offset - Fx::int(off)).abs() < Fx::ratio(1, 100), "{s:?} {off}: {at:?}");
        }
    }
}
