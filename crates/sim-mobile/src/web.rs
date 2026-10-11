//! The browser: the same shell on a canvas the page provides room for (`docs/architecture.md`, Distribution:
//! `web`). The page reads what happened from `<html data-simcraft="…">`: `ready:<backend>` or `error:<why>`.

use wasm_bindgen::prelude::wasm_bindgen;
use winit::event_loop::EventLoop;
use winit::platform::web::WindowExtWebSys;
use winit::window::Window;

/// Runs when the module loads: panics go to the console and the page, then the app starts.
#[wasm_bindgen(start)]
pub fn start() {
    std::panic::set_hook(Box::new(|info| {
        let msg = info.to_string();
        web_sys::console::error_1(&msg.as_str().into());
        status(&format!("error:{msg}"));
    }));
    match EventLoop::new() {
        Ok(el) => crate::run(el),
        Err(e) => status(&format!("error:no event loop: {e}")),
    }
}

/// Tells the page what happened (and the console).
pub fn status(s: &str) {
    web_sys::console::log_1(&s.into());
    if let Some(root) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.document_element()) {
        let _ = root.set_attribute("data-simcraft", s);
    }
}

/// Keeps the canvas's pixels at its size on the page times the device's pixel ratio (the page sizes it with CSS):
/// the new size when it changed. Checked every frame, so rotating the phone or resizing the page follows at once.
pub fn fit_canvas(window: &Window) -> Option<(u32, u32)> {
    let canvas = window.canvas()?;
    let ratio = web_sys::window()?.device_pixel_ratio();
    let w = ((canvas.client_width() as f64 * ratio).round() as u32).max(1);
    let h = ((canvas.client_height() as f64 * ratio).round() as u32).max(1);
    if (canvas.width(), canvas.height()) == (w, h) {
        return None;
    }
    canvas.set_width(w);
    canvas.set_height(h);
    Some((w, h))
}

/// The page's device orientation (degrees), on every `deviceorientation` event once the player allowed motion.
#[wasm_bindgen]
pub fn set_tilt(beta: f64, gamma: f64) {
    crate::sensors::set_web_gravity(beta, gamma);
}
