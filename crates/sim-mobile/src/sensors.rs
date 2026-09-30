//! Sensors: what the phone feels, read once a frame and handed to the simulation as whole numbers, so a replay
//! can carry them. Motion is Core Motion's device motion on iOS (gravity with the hand's shake filtered out, no
//! permission prompt); elsewhere there is none yet and a card says so.

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

    #[cfg(not(target_os = "ios"))]
    pub fn gravity(&self) -> Option<[i32; 3]> {
        None
    }

    pub fn sense(&self) -> Sense {
        Sense { gravity: self.gravity() }
    }
}
