//! simcraft C API. Header: `include/simcraft.h`. The host (Unity, Unreal, our own engine)
//! loads a game as text, talks via JSON requests, and on the hot path (every frame)
//! copies entities into a JSON-free array.
//!
//! Rules: every function is NULL-safe and panic-safe (no panic crosses the boundary).
//! Every `char*` returned by this library is freed with `simcraft_string_free`.
//! If the ABI changes, `SIMCRAFT_ABI_VERSION` is bumped.

use std::ffi::{CStr, CString, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use sim_agent::Session;
use sim_core::{Filter, Msg};

pub const SIMCRAFT_ABI_VERSION: u32 = 1;

/// Opaque handle.
pub struct SimcraftSim {
    session: Session,
    /// Index of the `kind` field → name (alphabetical, same order as `info.kinds`).
    kinds: Vec<CString>,
    /// Messages from the bus not yet drained.
    inbox: Arc<Mutex<Vec<Msg>>>,
}

/// One entity for one frame: position, kind, state glyph (the designer's state → look map).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SimcraftEntity {
    pub id: u64,
    pub x: i64,
    pub y: i64,
    /// Converted to a name with `simcraft_kind_name`.
    pub kind: u32,
    /// Unicode code point (e.g. 'W', '*').
    pub glyph: u32,
}

fn to_c(v: &Value) -> *mut c_char {
    // JSON text cannot contain NUL (serde_json escapes it); stay on the safe side anyway.
    CString::new(v.to_string()).map_or(ptr::null_mut(), CString::into_raw)
}

unsafe fn from_c<'a>(s: *const c_char) -> Option<&'a str> {
    if s.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(s) }.to_str().ok()
}

fn guard<T>(fallback: T, f: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(fallback)
}

#[unsafe(no_mangle)]
pub extern "C" fn simcraft_abi_version() -> u32 {
    SIMCRAFT_ABI_VERSION
}

/// A simulation from `game.ron` and `engine.toml` texts. Returns NULL on failure and,
/// if `out_error` is not NULL, writes a JSON error there (`{"ok":false,"stage":...,"errors":[...]}`).
///
/// # Safety
/// `game_ron`, `engine_toml` must be NUL-terminated UTF-8 strings; `out_error` NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn simcraft_new(
    game_ron: *const c_char,
    engine_toml: *const c_char,
    out_error: *mut *mut c_char,
) -> *mut SimcraftSim {
    let fail = |v: Value| {
        if !out_error.is_null() {
            unsafe { *out_error = to_c(&v) };
        }
        ptr::null_mut()
    };
    let (Some(game), Some(panel)) = (unsafe { from_c(game_ron) }, unsafe { from_c(engine_toml) }) else {
        return fail(json!({ "ok": false, "stage": "load", "errors": ["game_ron and engine_toml must be UTF-8 text"] }));
    };
    let made = guard(Err(json!({ "ok": false, "stage": "load", "errors": ["panic while loading"] })), || Session::from_strs(game, panel));
    match made {
        Ok(mut session) => {
            let kinds = session.game().def.kinds.keys().filter_map(|k| CString::new(k.as_str()).ok()).collect();
            let inbox: Arc<Mutex<Vec<Msg>>> = Arc::default();
            session.engine.bus().subscribe(Filter::All, Box::new(inbox.clone()));
            Box::into_raw(Box::new(SimcraftSim { session, kinds, inbox }))
        }
        Err(v) => fail(v),
    }
}

/// # Safety
/// `sim` must be a handle returned by `simcraft_new` and not yet freed, or NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn simcraft_free(sim: *mut SimcraftSim) {
    if !sim.is_null() {
        drop(unsafe { Box::from_raw(sim) });
    }
}

/// Frees a string returned by this library.
///
/// # Safety
/// `s` must come from this library and not yet be freed, or be NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn simcraft_string_free(s: *mut c_char) {
    if !s.is_null() {
        drop(unsafe { CString::from_raw(s) });
    }
}

/// One JSON request → one JSON response; same protocol as `simcraft-agent`
/// (info, observe, act, step, hash, snapshot, restore). Errors are JSON too (`"ok": false`).
///
/// # Safety
/// `sim` must be a valid handle, `request` a NUL-terminated UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn simcraft_request(sim: *mut SimcraftSim, request: *const c_char) -> *mut c_char {
    let Some(sim) = (unsafe { sim.as_mut() }) else {
        return to_c(&json!({ "ok": false, "error": "sim is NULL" }));
    };
    let Some(req) = (unsafe { from_c(request) }) else {
        return to_c(&json!({ "ok": false, "error": "request must be UTF-8 text" }));
    };
    let resp = guard(Some(json!({ "ok": false, "error": "panic while handling the request" })), || sim.session.handle_line(req));
    to_c(&resp.unwrap_or_else(|| json!({ "ok": true, "quit": "close the handle with simcraft_free" })))
}

/// Advances `n` ticks (stops if the game ended or `max_ticks` was reached). Returns the new tick;
/// -1 if `sim` is NULL. Events are taken from the bus with `simcraft_drain`.
///
/// # Safety
/// `sim` must be a valid handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn simcraft_step(sim: *mut SimcraftSim, n: u32) -> i64 {
    let Some(sim) = (unsafe { sim.as_mut() }) else { return -1 };
    guard(-1, || {
        let max = sim.session.game().cfg.run.max_ticks;
        let engine = &mut sim.session.engine;
        for _ in 0..n {
            if engine.world().tick >= max || engine.outcome().is_some() {
                break;
            }
            engine.tick();
        }
        engine.world().tick as i64
    })
}

/// Hot path: copies up to `cap` entities (in id order) into `out`; returns the total count.
/// If the total exceeds `cap`, call again with a larger array. `out` may be NULL (count only).
///
/// # Safety
/// `sim` must be a valid handle; `out` NULL or a writable array of at least `cap` elements.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn simcraft_entities(sim: *const SimcraftSim, out: *mut SimcraftEntity, cap: usize) -> usize {
    let Some(sim) = (unsafe { sim.as_ref() }) else { return 0 };
    guard(0, || {
        let game = sim.session.game();
        let world = sim.session.engine.world();
        let kind_index = |k: &str| game.def.kinds.keys().position(|x| x == k).unwrap_or(u32::MAX as usize) as u32;
        let all = world.entities();
        if !out.is_null() {
            let slots = unsafe { std::slice::from_raw_parts_mut(out, cap) };
            for (slot, e) in slots.iter_mut().zip(all.values()) {
                *slot = SimcraftEntity { id: e.id, x: e.x, y: e.y, kind: kind_index(&e.kind), glyph: game.glyph_of(e) as u32 };
            }
        }
        all.len()
    })
}

/// `SimcraftEntity.kind` → kind name. NULL if out of range. Valid while the handle lives (do not free).
///
/// # Safety
/// `sim` must be a valid handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn simcraft_kind_name(sim: *const SimcraftSim, kind: u32) -> *const c_char {
    let Some(sim) = (unsafe { sim.as_ref() }) else { return ptr::null() };
    sim.kinds.get(kind as usize).map_or(ptr::null(), |c| c.as_ptr())
}

/// Every bus message since the last drain, as a JSON array
/// (`start`, `act`, `event`, `tick`, `end`, `restore`; format: architecture.md, Event bus).
///
/// # Safety
/// `sim` must be a valid handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn simcraft_drain(sim: *mut SimcraftSim) -> *mut c_char {
    let Some(sim) = (unsafe { sim.as_mut() }) else { return to_c(&json!([])) };
    let msgs = sim.inbox.lock().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default();
    to_c(&serde_json::to_value(msgs).unwrap_or_else(|_| json!([])))
}

#[cfg(test)]
mod tests;
