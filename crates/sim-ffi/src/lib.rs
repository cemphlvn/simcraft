//! simcraft C API. Başlık: `include/simcraft.h`. Host (Unity, Unreal, kendi motorumuz)
//! bir oyunu metin olarak yükler, JSON istekleriyle konuşur, sıcak yolda (her kare)
//! entity'leri JSON'suz bir diziye kopyalar.
//!
//! Kurallar: her fonksiyon NULL'a ve panic'e karşı güvenlidir (panic sınırı geçmez).
//! Bu kütüphanenin döndürdüğü her `char*` `simcraft_string_free` ile bırakılır.
//! ABI değişirse `SIMCRAFT_ABI_VERSION` artar.

use std::ffi::{CStr, CString, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use sim_agent::Session;
use sim_core::{Filter, Msg};

pub const SIMCRAFT_ABI_VERSION: u32 = 1;

/// Opak tutamaç.
pub struct SimcraftSim {
    session: Session,
    /// `kind` alanının indeksi → ad (alfabetik, `info.kinds` ile aynı sıra).
    kinds: Vec<CString>,
    /// Veriyolundan gelen, henüz boşaltılmamış mesajlar.
    inbox: Arc<Mutex<Vec<Msg>>>,
}

/// Bir kare için entity: konum, tür, durumun glyph'i (tasarımcının durum → görünüm eşlemesi).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SimcraftEntity {
    pub id: u64,
    pub x: i64,
    pub y: i64,
    /// `simcraft_kind_name` ile ada çevrilir.
    pub kind: u32,
    /// Unicode kod noktası (ör. 'W', '*').
    pub glyph: u32,
}

fn to_c(v: &Value) -> *mut c_char {
    // JSON metninde NUL olamaz (serde_json kaçışlar); yine de güvenli taraf.
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

/// `game.ron` ve `engine.toml` metinlerinden bir simülasyon. Başarısızsa NULL döner ve
/// `out_error` NULL değilse oraya JSON hata yazılır (`{"ok":false,"stage":...,"errors":[...]}`).
///
/// # Safety
/// `game_ron`, `engine_toml` NUL ile biten UTF-8 metinler olmalı; `out_error` NULL ya da yazılabilir.
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
    let made = guard(Err(json!({ "ok": false, "stage": "load", "errors": ["panic while loading"] })), || {
        Session::from_strs(game, panel)
    });
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
/// `sim`, `simcraft_new`'in döndürdüğü ve henüz bırakılmamış tutamaç ya da NULL olmalı.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn simcraft_free(sim: *mut SimcraftSim) {
    if !sim.is_null() {
        drop(unsafe { Box::from_raw(sim) });
    }
}

/// Bu kütüphanenin döndürdüğü metni bırakır.
///
/// # Safety
/// `s` bu kütüphaneden gelmiş ve bırakılmamış olmalı ya da NULL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn simcraft_string_free(s: *mut c_char) {
    if !s.is_null() {
        drop(unsafe { CString::from_raw(s) });
    }
}

/// Bir JSON istek → bir JSON cevap; `simcraft-agent` ile aynı protokol
/// (info, observe, act, step, hash, snapshot, restore). Hata da JSON'dur (`"ok": false`).
///
/// # Safety
/// `sim` geçerli bir tutamaç, `request` NUL ile biten UTF-8 metin olmalı.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn simcraft_request(sim: *mut SimcraftSim, request: *const c_char) -> *mut c_char {
    let Some(sim) = (unsafe { sim.as_mut() }) else {
        return to_c(&json!({ "ok": false, "error": "sim is NULL" }));
    };
    let Some(req) = (unsafe { from_c(request) }) else {
        return to_c(&json!({ "ok": false, "error": "request must be UTF-8 text" }));
    };
    let resp = guard(Some(json!({ "ok": false, "error": "panic while handling the request" })), || {
        sim.session.handle_line(req)
    });
    to_c(&resp.unwrap_or_else(|| json!({ "ok": true, "quit": "close the handle with simcraft_free" })))
}

/// `n` tick ilerler (oyun bittiyse ya da `max_ticks`'e varıldıysa durur). Yeni tick'i döner;
/// `sim` NULL ise -1. Olaylar veriyolundan `simcraft_drain` ile alınır.
///
/// # Safety
/// `sim` geçerli bir tutamaç olmalı.
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

/// Sıcak yol: en fazla `cap` entity'yi (id sırasıyla) `out`'a kopyalar; toplam sayıyı döner.
/// Toplam `cap`'ten büyükse daha büyük bir diziyle yeniden çağırın. `out` NULL olabilir (yalnızca sayı).
///
/// # Safety
/// `sim` geçerli bir tutamaç; `out` NULL ya da en az `cap` elemanlık yazılabilir dizi olmalı.
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

/// `SimcraftEntity.kind` → kind adı. Aralık dışıysa NULL. Tutamaç yaşadıkça geçerlidir (bırakmayın).
///
/// # Safety
/// `sim` geçerli bir tutamaç olmalı.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn simcraft_kind_name(sim: *const SimcraftSim, kind: u32) -> *const c_char {
    let Some(sim) = (unsafe { sim.as_ref() }) else { return ptr::null() };
    sim.kinds.get(kind as usize).map_or(ptr::null(), |c| c.as_ptr())
}

/// Son boşaltmadan beri veriyolundaki her mesaj, JSON dizi olarak
/// (`start`, `act`, `event`, `tick`, `end`, `restore`; biçim: architecture.md, Event bus).
///
/// # Safety
/// `sim` geçerli bir tutamaç olmalı.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn simcraft_drain(sim: *mut SimcraftSim) -> *mut c_char {
    let Some(sim) = (unsafe { sim.as_mut() }) else { return to_c(&json!([])) };
    let msgs = sim.inbox.lock().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default();
    to_c(&serde_json::to_value(msgs).unwrap_or_else(|_| json!([])))
}

#[cfg(test)]
mod tests;
