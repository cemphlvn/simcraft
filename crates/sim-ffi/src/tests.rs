use std::ffi::CString;

use super::*;

const GAME: &str = include_str!("../../../games/wolf_sheep/game.ron");
const PANEL: &str = include_str!("../../../games/wolf_sheep/engine.toml");

fn text(s: *mut c_char) -> String {
    let out = unsafe { CStr::from_ptr(s) }.to_str().expect("utf-8").to_string();
    unsafe { simcraft_string_free(s) };
    out
}

fn new(game: &str, panel: &str) -> Result<*mut SimcraftSim, String> {
    let (g, p) = (CString::new(game).unwrap(), CString::new(panel).unwrap());
    let mut err = ptr::null_mut();
    let sim = unsafe { simcraft_new(g.as_ptr(), p.as_ptr(), &mut err) };
    if sim.is_null() { Err(text(err)) } else { Ok(sim) }
}

fn request(sim: *mut SimcraftSim, req: &str) -> Value {
    let r = CString::new(req).unwrap();
    serde_json::from_str(&text(unsafe { simcraft_request(sim, r.as_ptr()) })).expect("json")
}

#[test]
fn the_c_api_plays_the_same_game_as_the_agent() {
    let sim = new(GAME, PANEL).expect("loads");
    assert_eq!(simcraft_abi_version(), 1);
    assert_eq!(unsafe { simcraft_step(sim, 300) }, 300);
    // Same golden hash as the stdio agent and the engine tests.
    assert_eq!(request(sim, r#"{"cmd":"hash"}"#)["hash"], "ee9a66d10246d6f9");

    let n = unsafe { simcraft_entities(sim, ptr::null_mut(), 0) };
    let mut buf = vec![SimcraftEntity::default(); n];
    assert_eq!(unsafe { simcraft_entities(sim, buf.as_mut_ptr(), buf.len()) }, n);
    let name = |k| unsafe { CStr::from_ptr(simcraft_kind_name(sim, k)) }.to_str().unwrap().to_string();
    assert!(buf.iter().any(|e| name(e.kind) == "wolf" && e.glyph == 'W' as u32));
    assert!(unsafe { simcraft_kind_name(sim, 99) }.is_null());
    unsafe { simcraft_free(sim) };
}

#[test]
fn bus_messages_drain_once() {
    let sim = new(GAME, PANEL).expect("loads");
    unsafe { simcraft_step(sim, 5) };
    let msgs: Value = serde_json::from_str(&text(unsafe { simcraft_drain(sim) })).expect("json");
    let ticks = msgs.as_array().unwrap().iter().filter(|m| m["t"] == "tick").count();
    assert_eq!(ticks, 5);
    assert_eq!(text(unsafe { simcraft_drain(sim) }), "[]");
    unsafe { simcraft_free(sim) };
}

#[test]
fn errors_come_back_as_json_not_crashes() {
    let err = new(&GAME.replace("me.hunger >= p.starve_at", "me.hungr >= p.starve_at"), PANEL).expect_err("typo");
    let v: Value = serde_json::from_str(&err).expect("json");
    assert_eq!(v["stage"], "validate");

    assert_eq!(unsafe { simcraft_step(ptr::null_mut(), 1) }, -1);
    assert_eq!(unsafe { simcraft_entities(ptr::null(), ptr::null_mut(), 0) }, 0);
    let bad = CString::new("{").unwrap();
    let v: Value = serde_json::from_str(&text(unsafe { simcraft_request(ptr::null_mut(), bad.as_ptr()) })).unwrap();
    assert_eq!(v["ok"], false);
    unsafe { simcraft_free(ptr::null_mut()) };
}

#[test]
fn save_and_load_through_the_c_api() {
    let sim = new(GAME, PANEL).expect("loads");
    unsafe { simcraft_step(sim, 20) };
    let snap = request(sim, r#"{"cmd":"snapshot"}"#);
    unsafe { simcraft_step(sim, 30) };
    let later = request(sim, r#"{"cmd":"hash"}"#)["hash"].clone();
    let restore = json!({ "cmd": "restore", "snapshot": snap["snapshot"], "source": snap["source"] }).to_string();
    assert_eq!(request(sim, &restore)["tick"], 20);
    unsafe { simcraft_step(sim, 30) };
    assert_eq!(request(sim, r#"{"cmd":"hash"}"#)["hash"], later);
    unsafe { simcraft_free(sim) };
}

#[test]
fn a_save_file_is_the_snapshot_reply_as_is() {
    // What the adapters do without a JSON library: `{"cmd":"restore",` + reply minus its `{`.
    let sim = new(GAME, PANEL).expect("loads");
    unsafe { simcraft_step(sim, 10) };
    let save = text(unsafe { simcraft_request(sim, CString::new(r#"{"cmd":"snapshot"}"#).unwrap().as_ptr()) });
    let at10 = request(sim, r#"{"cmd":"hash"}"#)["hash"].clone();
    unsafe { simcraft_step(sim, 10) };
    let restore = format!(r#"{{"cmd":"restore",{}"#, &save[1..]);
    let r = request(sim, &restore);
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["hash"], at10);
    unsafe { simcraft_free(sim) };
}
