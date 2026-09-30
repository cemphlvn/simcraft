//! Haptics: a small vocabulary both platforms share (`docs/research/mobile-template.md` §3). A pulse is a kind
//! with an intensity and a sharpness (0 to 1); iOS plays it through Core Haptics (a transient or continuous event),
//! Android through the nearest `VibrationEffect` composition primitive. Games fire pulses from what happened (bus
//! events, a rope going taut), never from inside the simulation.

/// The kinds of pulse.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// A crisp click: a button, a catch.
    Tap,
    /// A heavy knock: a landing, a hit, a let-go.
    Thud,
    /// A tiny tick: a notch passed, tension rising a step.
    Tick,
    /// Swelling: charging up.
    Rise,
    /// Fading: releasing.
    Fall,
    /// A sustained buzz of `ms`.
    Buzz { ms: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pulse {
    pub kind: Kind,
    pub intensity: f32,
    pub sharpness: f32,
}

impl Pulse {
    pub fn new(kind: Kind, intensity: f32, sharpness: f32) -> Pulse {
        Pulse { kind, intensity: intensity.clamp(0.0, 1.0), sharpness: sharpness.clamp(0.0, 1.0) }
    }
}

/// Plays pulses on a device.
pub trait Haptics {
    fn play(&mut self, p: Pulse);
    /// Which backend this is (in the stats line).
    fn name(&self) -> &'static str;
    /// Pulses the device refused or could not play (0 when every pulse was felt).
    fn failed(&self) -> u32 {
        0
    }
}

/// The desktop's haptics: none. Keeps the pulses, so the preview can show them and tests can count them.
#[derive(Clone, Debug, Default)]
pub struct Record {
    pub played: Vec<Pulse>,
}

impl Haptics for Record {
    fn play(&mut self, p: Pulse) {
        self.played.push(p);
    }

    fn name(&self) -> &'static str {
        "record"
    }
}

/// The platform's haptics: Core Haptics on iOS; elsewhere (Android's `VibrationEffect` comes next, and the desktop)
/// the pulses are recorded, so nothing is silently lost.
pub fn platform() -> Box<dyn Haptics> {
    #[cfg(target_os = "ios")]
    match ios::CoreHaptics::new() {
        Ok(h) => return Box::new(h),
        Err(e) => eprintln!("sim-mobile: no Core Haptics ({e}); pulses are recorded instead"),
    }
    Box::new(Record::default())
}

#[cfg(target_os = "ios")]
mod ios {
    use objc2::AnyThread;
    use objc2::rc::Retained;
    use objc2_core_haptics::{
        CHHapticDynamicParameter, CHHapticEngine, CHHapticEvent, CHHapticEventParameter, CHHapticEventParameterIDAttackTime,
        CHHapticEventParameterIDDecayTime, CHHapticEventParameterIDHapticIntensity, CHHapticEventParameterIDHapticSharpness,
        CHHapticEventTypeHapticContinuous, CHHapticEventTypeHapticTransient, CHHapticPattern, CHHapticPatternPlayer,
        CHHapticTimeImmediate,
    };
    use objc2_foundation::NSArray;

    use super::{Haptics, Kind, Pulse};

    /// Core Haptics: a pulse becomes one event (transient for a click, continuous with an envelope for the rest),
    /// played by a fresh pattern player. An engine iOS stopped (the app went to the background) is started again.
    pub struct CoreHaptics {
        engine: Retained<CHHapticEngine>,
        failed: u32,
    }

    impl CoreHaptics {
        pub fn new() -> Result<CoreHaptics, String> {
            // SAFETY: plain Core Haptics calls on the main thread, as Apple documents them.
            unsafe {
                let engine = CHHapticEngine::initAndReturnError(CHHapticEngine::alloc()).map_err(|e| e.to_string())?;
                engine.setPlaysHapticsOnly(true);
                engine.startAndReturnError().map_err(|e| e.to_string())?;
                Ok(CoreHaptics { engine, failed: 0 })
            }
        }

        fn event(p: Pulse) -> Retained<CHHapticEvent> {
            // SAFETY: the constants are Core Haptics' own; the arrays hold the parameter objects made here.
            unsafe {
                let param = |id, v: f32| CHHapticEventParameter::initWithParameterID_value(CHHapticEventParameter::alloc(), id, v);
                let mut params = vec![param(CHHapticEventParameterIDHapticIntensity, p.intensity), param(CHHapticEventParameterIDHapticSharpness, p.sharpness)];
                let (transient, secs) = match p.kind {
                    Kind::Tap | Kind::Thud | Kind::Tick => (true, 0.0),
                    Kind::Rise => {
                        params.push(param(CHHapticEventParameterIDAttackTime, 0.15));
                        (false, 0.15)
                    }
                    Kind::Fall => {
                        params.push(param(CHHapticEventParameterIDDecayTime, 0.2));
                        (false, 0.2)
                    }
                    Kind::Buzz { ms } => (false, f64::from(ms) / 1000.0),
                };
                let params = NSArray::from_retained_slice(&params);
                if transient {
                    CHHapticEvent::initWithEventType_parameters_relativeTime(CHHapticEvent::alloc(), CHHapticEventTypeHapticTransient, &params, 0.0)
                } else {
                    CHHapticEvent::initWithEventType_parameters_relativeTime_duration(
                        CHHapticEvent::alloc(),
                        CHHapticEventTypeHapticContinuous,
                        &params,
                        0.0,
                        secs,
                    )
                }
            }
        }

        fn start(&self, p: Pulse) -> Result<(), String> {
            // SAFETY: a one-event pattern and its player, started immediately.
            unsafe {
                let events = NSArray::from_retained_slice(&[CoreHaptics::event(p)]);
                let none: Retained<NSArray<CHHapticDynamicParameter>> = NSArray::new();
                let pattern = CHHapticPattern::initWithEvents_parameters_error(CHHapticPattern::alloc(), &events, &none).map_err(|e| e.to_string())?;
                let player = self.engine.createPlayerWithPattern_error(&pattern).map_err(|e| e.to_string())?;
                player.startAtTime_error(CHHapticTimeImmediate).map_err(|e| e.to_string())
            }
        }
    }

    impl Haptics for CoreHaptics {
        fn play(&mut self, p: Pulse) {
            if self.start(p).is_err() {
                // SAFETY: restarting a stopped engine is how Apple recovers from interruptions.
                let restarted = unsafe { self.engine.startAndReturnError() }.is_ok();
                if !restarted || self.start(p).is_err() {
                    self.failed += 1;
                }
            }
        }

        fn name(&self) -> &'static str {
            "core-haptics"
        }

        fn failed(&self) -> u32 {
            self.failed
        }
    }
}
