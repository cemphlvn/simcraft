//! The drive view's sound, as data and decisions (the host plays it; the simulation never hears it): what
//! `sound:` in drive.ron says, where recordings are found, and when the spotter speaks. Pure, so it is tested
//! without a sound device.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Recordings are looked for with these extensions, in this order.
pub const EXTENSIONS: [&str; 4] = ["wav", "flac", "ogg", "mp3"];

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Sound {
    pub engine: bool,
    pub volume: f32,
    /// The firing frequency is rpm / 60 × cylinders / 2.
    pub cylinders: u32,
    /// Wind noise at 80 m/s.
    pub wind: f32,
    /// The spotter on the radio.
    pub voice: Voice,
}

impl Default for Sound {
    fn default() -> Self {
        Sound { engine: true, volume: 0.5, cylinders: 8, wind: 0.25, voice: Voice::default() }
    }
}

/// The spotter's voice: a folder of lines (under the game or an `assets/` folder above it), which line says each
/// call (the first file found of each list; a call without a list says the file of its own name), and how often.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Voice {
    pub dir: String,
    pub lines: BTreeMap<String, Vec<String>>,
    /// Seconds of quiet after a line before the next, and before the same line again.
    pub gap: f32,
    pub repeat: f32,
    /// A call older than this (s) when the radio is free again is not said any more.
    pub stale: f32,
    pub volume: f32,
}

impl Default for Voice {
    fn default() -> Self {
        Voice { dir: String::new(), lines: BTreeMap::new(), gap: 0.6, repeat: 4.0, stale: 1.2, volume: 1.0 }
    }
}

/// The recording `name` in `dir` (any of `EXTENSIONS`), next to the game or in an `assets/` folder above it.
pub fn find_clip(game: &Path, dir: &str, name: &str) -> Option<PathBuf> {
    EXTENSIONS.iter().find_map(|ext| super::photos::find(game, &format!("{dir}/{name}.{ext}")))
}

impl Voice {
    /// The file that says `call`: the first of its lines that exists.
    pub fn line(&self, game: &Path, call: &str) -> Option<PathBuf> {
        let own = [call.to_string()];
        let names = self.lines.get(call).map_or(&own[..], |v| &v[..]);
        names.iter().find_map(|n| find_clip(game, &self.dir, n))
    }
}

/// When the spotter speaks: a new call is said when the radio is free (a gap after the last line), unless the same
/// line was said moments ago, and it is dropped if it went stale waiting. One call waits at a time: the newest.
#[derive(Clone, Debug, Default)]
pub struct Radio {
    busy_until: f32,
    said: BTreeMap<String, f32>,
    last_call: Option<String>,
    waiting: Option<(String, f32)>,
}

impl Radio {
    /// This frame's call (`None`: nothing to say), the time, and how long each line lasts (s; None: no such line).
    /// Returns the line to start now.
    pub fn hear(&mut self, call: Option<&str>, now: f32, v: &Voice, length: impl Fn(&str) -> Option<f32>) -> Option<String> {
        if call != self.last_call.as_deref() {
            self.last_call = call.map(str::to_string);
            if let Some(c) = call {
                self.waiting = Some((c.to_string(), now));
            }
        }
        self.say_waiting(now, v, length)
    }

    /// Something the race says (the green flag, the white flag, the checkered): waits for the radio like a call.
    pub fn announce(&mut self, line: &str, now: f32) {
        self.waiting = Some((line.to_string(), now));
    }

    fn say_waiting(&mut self, now: f32, v: &Voice, length: impl Fn(&str) -> Option<f32>) -> Option<String> {
        let (call, at) = self.waiting.clone()?;
        if now - at > v.stale.max(0.0) && !matches!(call.as_str(), "green" | "white_flag" | "checkered") {
            self.waiting = None;
            return None;
        }
        if now < self.busy_until {
            return None;
        }
        if self.said.get(&call).is_some_and(|t| now - t < v.repeat) {
            self.waiting = None;
            return None;
        }
        self.waiting = None;
        let len = length(&call)?;
        self.busy_until = now + len + v.gap;
        self.said.insert(call.clone(), now);
        Some(call)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn say(r: &mut Radio, call: Option<&str>, now: f32) -> Option<String> {
        r.hear(call, now, &Voice::default(), |_| Some(1.0))
    }

    #[test]
    fn the_spotter_waits_for_the_radio_and_does_not_repeat_itself() {
        let mut r = Radio::default();
        assert_eq!(say(&mut r, Some("car_low"), 0.0).as_deref(), Some("car_low"));
        // Still alongside: the same call is not said again every frame.
        assert_eq!(say(&mut r, Some("car_low"), 0.5), None);
        // A new call while the line plays waits for it, then the gap.
        assert_eq!(say(&mut r, Some("clear_low"), 1.2), None);
        assert_eq!(say(&mut r, Some("clear_low"), 1.7).as_deref(), Some("clear_low"));
        // Low again at once: said moments ago, so not again.
        assert_eq!(say(&mut r, Some("car_low"), 3.4), None);
        // A call that went stale waiting is dropped.
        let mut r = Radio::default();
        say(&mut r, Some("three_wide"), 0.0);
        say(&mut r, Some("still_there"), 0.2);
        assert_eq!(say(&mut r, Some("still_there"), 3.0), None, "too late to be useful");
    }

    #[test]
    fn a_call_says_the_first_line_that_exists() {
        let game = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../games/race");
        let v = Voice {
            dir: "race/voice".into(),
            lines: BTreeMap::from([("car_low".to_string(), vec!["no_such_line".to_string(), "inside".to_string()])]),
            ..Voice::default()
        };
        assert!(v.line(&game, "car_low").is_some_and(|p| p.ends_with("inside.mp3")));
        assert!(v.line(&game, "clear").is_some_and(|p| p.ends_with("clear.mp3")), "its own name");
        assert!(v.line(&game, "nothing").is_none());
    }
}
