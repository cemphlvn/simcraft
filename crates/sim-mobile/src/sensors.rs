//! Sensors: what the phone feels, read once a frame and handed to the simulation as whole numbers, so a replay
//! can carry them. Motion is Core Motion's device motion on iOS (gravity with the hand's shake filtered out, no
//! permission prompt); in a browser it is the page's device orientation, handed in by `web::set_tilt` once the
//! player allowed it; elsewhere there is none and a card says so.

/// What the sensors read this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sense {
    /// Gravity in the phone's frame, thousandths of g: x to the right of the screen, y to its top, z out of it
    /// (a phone held upright reads about `[0, -1000, 0]`, flat on a table `[0, 0, -1000]`).
    pub gravity: Option<[i32; 3]>,
}

/// The phone's motion sensor.
pub struct Motion {
    #[cfg(target_os = "ios")]
    manager: Option<objc2::rc::Retained<objc2_core_motion::CMMotionManager>>,
}

impl Default for Motion {
    fn default() -> Motion {
        Motion::new()
    }
}

impl Motion {
    #[cfg(target_os = "ios")]
    pub fn new() -> Motion {
        use objc2_core_motion::CMMotionManager;
        // SAFETY: Core Motion's documented pull model, on the main thread: start updates, read the latest sample.
        let manager = unsafe {
            let m = CMMotionManager::new();
            if m.isDeviceMotionAvailable() {
                m.setDeviceMotionUpdateInterval(1.0 / 120.0);
                m.startDeviceMotionUpdates();
                Some(m)
            } else {
                None
            }
        };
        Motion { manager }
    }

    #[cfg(not(target_os = "ios"))]
    pub fn new() -> Motion {
        Motion {}
    }

    /// Gravity now (thousandths of g), or none without a motion sensor (or before its first sample).
    #[cfg(target_os = "ios")]
    pub fn gravity(&self) -> Option<[i32; 3]> {
        let m = self.manager.as_ref()?;
        // SAFETY: reading the latest device motion sample Core Motion keeps.
        let g = unsafe { m.deviceMotion()?.gravity() };
        let milli = |v: f64| (v * 1000.0).round() as i32;
        Some([milli(g.x), milli(g.y), milli(g.z)])
    }

    #[cfg(target_arch = "wasm32")]
    pub fn gravity(&self) -> Option<[i32; 3]> {
        WEB_GRAVITY.get()
    }

    #[cfg(not(any(target_os = "ios", target_arch = "wasm32")))]
    pub fn gravity(&self) -> Option<[i32; 3]> {
        None
    }

    pub fn sense(&self) -> Sense {
        Sense { gravity: self.gravity() }
    }
}

#[cfg(target_arch = "wasm32")]
thread_local! {
    /// The browser's latest gravity reading (the page's one thread).
    static WEB_GRAVITY: std::cell::Cell<Option<[i32; 3]>> = const { std::cell::Cell::new(None) };
}

/// The browser: gravity from the page's device orientation (degrees: `beta` front to back, `gamma` left to right,
/// a portrait phone), in the same frame and units as Core Motion's.
#[cfg(target_arch = "wasm32")]
pub fn set_web_gravity(beta: f64, gamma: f64) {
    let (b, g) = (beta.to_radians(), gamma.to_radians());
    // Earth's down, (0, 0, −1), turned into the phone's frame (W3C: R = Rz(α)·Rx(β)·Ry(γ)).
    let milli = |v: f64| (v * 1000.0).round() as i32;
    WEB_GRAVITY.set(Some([milli(b.cos() * g.sin()), milli(-b.sin()), milli(-b.cos() * g.cos())]));
}
