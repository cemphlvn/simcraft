//! sim-mobile: the mobile core (`docs/architecture.md`, Mobile core). One shell for iOS, Android and a desktop
//! preview: gestures (`gesture`), screen layers (`layer`), haptics (`haptics`), sensors (`sensors`), shape drawing
//! (`draw`), text (`font`), the Mobile Capability Playground (`playground`) and the application loop (`app`). Designed from the types of casual mobile games (`docs/research/mobile-types.md`).
//!
//! Entry points: `simcraft_mobile_main` (iOS: the Xcode app's `main` calls it; it never returns), `android_main`
//! (Android: `NativeActivity` loads this library and calls it), [`preview`] (the desktop).

pub mod app;
pub mod draw;
pub mod font;
pub mod gesture;
pub mod haptics;
pub mod layer;
pub mod playground;
pub mod sensors;
pub mod stats;

use winit::event_loop::EventLoop;

/// Runs the app on `el` until it ends (on iOS: forever).
pub fn run(el: EventLoop<()>) {
    let mut app = app::App::new();
    if let Err(e) = el.run_app(&mut app) {
        eprintln!("sim-mobile: {e}");
    }
}

/// The desktop preview: a phone-sized window.
pub fn preview() -> Result<(), String> {
    let el = EventLoop::new().map_err(|e| e.to_string())?;
    run(el);
    Ok(())
}

/// iOS: called from the Xcode app's `main`. Never returns.
#[cfg(target_os = "ios")]
#[unsafe(no_mangle)]
pub extern "C" fn simcraft_mobile_main() {
    run(EventLoop::new().expect("an event loop"));
}

/// Android: called by `NativeActivity` on its own thread.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub fn android_main(app: winit::platform::android::activity::AndroidApp) {
    use winit::platform::android::EventLoopBuilderExtAndroid;
    let el = EventLoop::builder().with_android_app(app).build().expect("an event loop");
    run(el);
}
