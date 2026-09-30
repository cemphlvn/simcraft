//! The drive view's sound, as data and decisions (the host plays it; the simulation never hears it): what
//! `sound:` in drive.ron says, where recordings are found, and when the spotter speaks. Pure, so it is tested
//! without a sound device.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Recordings are looked for with these extensions, in this order.
pub const EXTENSIONS: [&str; 4] = ["wav", "flac", "ogg", "mp3"];

/// The drive view's sound (`sound:` in drive.ron). Everything is optional: a recording that is not there falls back
/// to synthesis (the engine, wind, tyres, scrapes, knocks, the crowd) or to silence (a pass-by, music, a voice line);
/// `simcraft-check` lists which is which.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Sound {
    pub engine: bool,
    /// The master level.
    pub volume: f32,
    /// The firing frequency is rpm / 60 × cylinders / 2 (the synthesised engine).
    pub cylinders: u32,
    /// Wind noise at 80 m/s.
    pub wind: f32,
    /// Where recordings are looked for (folders under the game or an `assets/` folder above it), in order.
    pub dirs: Vec<String>,
    /// The mixer's buses and their levels: engine (yours), others (the field), effects, voice, music.
    pub buses: Buses,
    /// Your engine from recordings: loops, each tagged with the rpm it was recorded at and its load (on throttle,
    /// off, or both), crossfaded by rpm and throttle. A loop whose file is missing is played by the synthesiser.
    pub loops: Vec<EngineLoop>,
    /// One recording of a steady sweep from low to high rpm, sliced into loops (see `slice_sweep`).
    pub sweep: Option<Sweep>,
    /// Files for the effects and the music, by role (passby, squeal, scrape, impact, shift, crowd, wind,
    /// music_menu, music_results); a role not named here looks for a file of its own name.
    pub files: BTreeMap<String, String>,
    /// The other cars: full level within `reference` m, falling off by `rolloff`, silent past `max`; Doppler × `doppler`.
    pub others: Others,
    /// How far the music ducks under your engine (0..1).
    pub duck: f32,
    /// The spotter on the radio.
    pub voice: Voice,
}

impl Default for Sound {
    fn default() -> Self {
        Sound {
            engine: true,
            volume: 0.5,
            cylinders: 8,
            wind: 0.25,
            dirs: Vec::new(),
            buses: Buses::default(),
            loops: Vec::new(),
            sweep: None,
            files: BTreeMap::new(),
            others: Others::default(),
            duck: 0.6,
            voice: Voice::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Buses {
    pub engine: f32,
    pub others: f32,
    pub effects: f32,
    pub voice: f32,
    pub music: f32,
}

impl Default for Buses {
    fn default() -> Self {
        Buses { engine: 1.0, others: 0.7, effects: 0.8, voice: 1.0, music: 0.5 }
    }
}

/// On-load (throttle), off-load (coasting, engine braking), or both (idle).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum Load {
    On,
    Off,
    Both,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineLoop {
    pub file: String,
    pub rpm: f32,
    #[serde(default = "on")]
    pub load: Load,
}

fn on() -> Load {
    Load::On
}

/// A sweep: its file, the rpm at its start and at its end (it is taken as linear in time between), how many loops
/// to cut from it, and each loop's length (s).
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sweep {
    pub file: String,
    pub from: f32,
    pub to: f32,
    #[serde(default = "bands")]
    pub bands: usize,
    #[serde(default = "slice_len")]
    pub length: f32,
}

fn bands() -> usize {
    6
}
fn slice_len() -> f32 {
    0.6
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Others {
    pub reference: f32,
    pub rolloff: f32,
    pub max: f32,
    pub doppler: f32,
}

impl Default for Others {
    fn default() -> Self {
        Others { reference: 8.0, rolloff: 1.0, max: 450.0, doppler: 1.0 }
    }
}

/// The effect and music roles, and what plays when a role has no recording.
pub const ROLES: [(&str, &str); 9] = [
    ("passby", "nothing: the other cars' engines pass with Doppler"),
    ("squeal", "synthesised (resonant noise)"),
    ("scrape", "synthesised (grinding noise)"),
    ("impact", "synthesised (a thump and a crunch)"),
    ("shift", "synthesised (a clunk)"),
    ("crowd", "synthesised (a murmur)"),
    ("wind", "synthesised (filtered noise)"),
    ("music_menu", "silence"),
    ("music_results", "silence"),
];

impl Sound {
    /// The recording for a role (`files`, else the role's own name), in the first of `dirs` that has it.
    pub fn role(&self, game: &Path, role: &str) -> Option<PathBuf> {
        let name = self.files.get(role).map_or(role, String::as_str);
        self.dirs.iter().find_map(|d| find_clip(game, d, name))
    }

    /// A named recording (a loop, the sweep) in the first of `dirs` that has it.
    pub fn file(&self, game: &Path, name: &str) -> Option<PathBuf> {
        self.dirs.iter().find_map(|d| find_clip(game, d, name))
    }

    /// Every recording the view can use: (what, the file found, what plays without it).
    pub fn inventory(&self, game: &Path) -> Vec<(String, Option<PathBuf>, String)> {
        let mut out = Vec::new();
        let sweep = self.sweep.as_ref().and_then(|s| self.file(game, &s.file));
        for l in &self.loops {
            let fallback = if sweep.is_some() { "the sweep's slices" } else { "the synthesised engine" };
            out.push((format!("engine {} ({:.0} rpm, {:?})", l.file, l.rpm, l.load), self.file(game, &l.file), fallback.to_string()));
        }
        if let Some(s) = &self.sweep {
            out.push((format!("sweep {} ({:.0}-{:.0} rpm, {} loops)", s.file, s.from, s.to, s.bands), sweep, "the loops above".into()));
        }
        for (role, fallback) in ROLES {
            out.push((role.to_string(), self.role(game, role), fallback.to_string()));
        }
        out
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

// ---------------------------------------------------------------- signal helpers (pure)

/// The mixer's rate.
pub const RATE: u32 = 44_100;

/// Equal-power crossfade across loops recorded at `rpms` (any order): the two nearest `rpm` share it
/// (cos/sin of how far between them it is), the rest are silent; below the lowest or above the highest, that one
/// plays alone. Weights in the order given; their squares sum to 1.
pub fn band_weights(rpms: &[f32], rpm: f32) -> Vec<f32> {
    let mut w = vec![0.0; rpms.len()];
    if rpms.is_empty() {
        return w;
    }
    let mut order: Vec<usize> = (0..rpms.len()).collect();
    order.sort_by(|a, b| rpms[*a].total_cmp(&rpms[*b]));
    let (lo, hi) = (order[0], order[order.len() - 1]);
    if rpm <= rpms[lo] {
        w[lo] = 1.0;
        return w;
    }
    if rpm >= rpms[hi] {
        w[hi] = 1.0;
        return w;
    }
    let k = order.windows(2).position(|p| rpm >= rpms[p[0]] && rpm <= rpms[p[1]]).unwrap_or(0);
    let (a, b) = (order[k], order[k + 1]);
    let t = ((rpm - rpms[a]) / (rpms[b] - rpms[a]).max(1e-3)).clamp(0.0, 1.0);
    let (s, c) = (t * std::f32::consts::FRAC_PI_2).sin_cos();
    w[a] = c;
    w[b] = s;
    w
}

/// On-load and off-load shares for a throttle (0..1), equal power.
pub fn load_weights(throttle: f32) -> (f32, f32) {
    let (s, c) = (throttle.clamp(0.0, 1.0) * std::f32::consts::FRAC_PI_2).sin_cos();
    (s, c)
}

/// A recording made seamless: its last `xf` samples crossfaded (equal power) into its first, and cut there.
pub fn make_loop(mut data: Vec<f32>, xf: usize) -> Vec<f32> {
    let xf = xf.min(data.len() / 3);
    if xf == 0 {
        return data;
    }
    let n = data.len() - xf;
    for i in 0..xf {
        let t = (i as f32 + 0.5) / xf as f32 * std::f32::consts::FRAC_PI_2;
        data[i] = data[i] * t.sin() + data[n + i] * t.cos();
    }
    data.truncate(n);
    data
}

/// Mono samples at `from` Hz to the mixer's rate (linear interpolation).
pub fn resample(data: &[f32], from: u32) -> Vec<f32> {
    if from == RATE || data.is_empty() {
        return data.to_vec();
    }
    let k = from as f64 / RATE as f64;
    let n = (data.len() as f64 / k) as usize;
    (0..n)
        .map(|i| {
            let x = i as f64 * k;
            let j = x as usize;
            let f = (x - j as f64) as f32;
            let a = data[j.min(data.len() - 1)];
            let b = data[(j + 1).min(data.len() - 1)];
            a + (b - a) * f
        })
        .collect()
}

/// Scales a recording to an RMS level (loops that crossfade must be equally loud).
pub fn level(mut data: Vec<f32>, rms: f32) -> Vec<f32> {
    let now = (data.iter().map(|v| v * v).sum::<f32>() / data.len().max(1) as f32).sqrt();
    if now > 1e-6 {
        let k = rms / now;
        data.iter_mut().for_each(|v| *v *= k);
    }
    data
}

/// Slices a steady sweep (mono, at the mixer's rate) into engine loops: the sweep is taken as linear in rpm over
/// its length, so the loop for band k is cut around the time its rpm (the middle of the band) was reached, `length`
/// seconds long, then made seamless. Returns (rpm, samples) per band. A slice of a rising sweep rises a little in
/// pitch across itself; short slices keep that under a few percent (0.6 s of a 10 s 2,000-9,500 rpm sweep is 450
/// rpm, ±2.5 %), which the crossfade to the next band hides.
pub fn slice_sweep(data: &[f32], s: &Sweep) -> Vec<(f32, Vec<f32>)> {
    let n = data.len();
    let len = ((s.length * RATE as f32) as usize).min(n / 2).max(1);
    (0..s.bands.max(1))
        .map(|k| {
            let t = (k as f32 + 0.5) / s.bands.max(1) as f32;
            let rpm = s.from + (s.to - s.from) * t;
            let mid = (t * n as f32) as usize;
            let a = mid.saturating_sub(len / 2).min(n - len);
            let slice = data[a..a + len].to_vec();
            (rpm, make_loop(slice, len / 6))
        })
        .collect()
}

// ---------------------------------------------------------------- the director: the race → what to play

/// A car as the ears hear it (track frame, metres, m/s).
#[derive(Clone, Copy, Debug, Default)]
pub struct Heard {
    pub id: u64,
    pub pos: (f32, f32),
    pub vel: (f32, f32),
    pub heading: f32,
    pub rpm: f32,
    pub throttle: f32,
    pub speed: f32,
    pub gear: i64,
    /// Contact this tick (N·s), the tyres' slip (radians), the sideways g felt (m/s²).
    pub impact: f32,
    pub slip: f32,
    pub g_lat: f32,
}

/// Where the race is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Phase {
    /// On the grid, before the green flag (or paused): the menu music.
    #[default]
    Grid,
    Racing,
    /// After the checkered flag: the results music.
    Finished,
}

/// A car's engine as the mixer plays it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EngineVoice {
    pub rpm: f32,
    pub throttle: f32,
    pub gain: f32,
    /// Stereo balance -1 (left) .. 1 (right), and the pitch the Doppler effect puts on it.
    pub pan: f32,
    pub pitch: f32,
}

/// A one-shot: a role's recording (or its synthesis) at a gain and a balance.
#[derive(Clone, Debug, PartialEq)]
pub struct Shot {
    pub role: String,
    pub gain: f32,
    pub pan: f32,
}

/// What the mixer plays now (levels include the buses and the master).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mix {
    pub engine: EngineVoice,
    pub others: Vec<EngineVoice>,
    /// Loops: tyres (gain, pitch), scraping, wind (gain, pitch), the crowd.
    pub squeal: (f32, f32),
    pub scrape: f32,
    pub wind: (f32, f32),
    pub crowd: f32,
    pub shots: Vec<Shot>,
    /// The music to loop (a role) and its level; the voice's level.
    pub music: Option<String>,
    pub music_gain: f32,
    pub voice_gain: f32,
}

/// Speed of sound (m/s).
const C: f32 = 343.0;

/// Per car, what the ears remember between frames.
#[derive(Clone, Debug, Default)]
struct Ear {
    passby_at: Option<f32>,
}

/// Turns the race into a mix, frame by frame: levels by distance, balance by bearing, Doppler by closing speed,
/// the effects from the car's state, one-shots on events (a shift, a hit, a car going by), the music by phase.
#[derive(Clone, Debug, Default)]
pub struct Director {
    ears: BTreeMap<u64, Ear>,
    gear: Option<i64>,
    tick: u64,
    contact: Vec<f32>,
    last_hit: Option<f32>,
    wreck_at: Option<f32>,
    duck: f32,
}

/// The inputs of one frame.
pub struct Scene<'a> {
    pub you: Option<Heard>,
    pub others: &'a [Heard],
    /// Points along the grandstands (track frame, m).
    pub stands: &'a [(f32, f32)],
    pub time: f32,
    pub tick: u64,
    pub phase: Phase,
    pub paused: bool,
    /// How long the loudest part of the pass-by recording comes after its start (s); none: no recording.
    pub passby_peak: Option<f32>,
}

impl Director {
    /// This frame's mix, and a spotter line to announce (a wreck ahead).
    pub fn listen(&mut self, s: &Sound, sc: &Scene) -> (Mix, Option<&'static str>) {
        let b = s.buses;
        let master = s.volume;
        let mut mix = Mix { voice_gain: master * b.voice * s.voice.volume, ..Mix::default() };
        let mut announce = None;
        let new_tick = sc.tick != self.tick;
        self.tick = sc.tick;
        if let Some(y) = sc.you {
            // Your engine: louder on throttle (the exhaust), always there.
            let eng = if s.engine && !sc.paused { master * b.engine } else { 0.0 };
            mix.engine = EngineVoice { rpm: y.rpm, throttle: y.throttle, gain: eng, pan: 0.0, pitch: 1.0 };
            let fx = if sc.paused { 0.0 } else { master * b.effects };
            // Tyres: squeal from slip (past 40 mrad, full at 110), sliding sideways at speed.
            let sq = ((y.slip - 0.04) / 0.07).clamp(0.0, 1.0) * (y.speed / 10.0).clamp(0.0, 1.0);
            mix.squeal = (fx * sq * 0.8, 0.9 + 0.3 * sq);
            // Wind with the square of speed.
            let v = (y.speed / 80.0).max(0.0);
            mix.wind = (fx * s.wind * 2.0 * v * v, 0.8 + 0.4 * v.min(1.5));
            // Contact: a new hit is a knock; contact held over ticks is a scrape along the wall.
            if new_tick {
                self.contact.push(y.impact);
                if self.contact.len() > 6 {
                    self.contact.remove(0);
                }
                let fresh = y.impact > 400.0 && self.last_hit.is_none_or(|t| sc.time - t > 0.35);
                if fresh || y.impact > 4000.0 && self.last_hit.is_none_or(|t| sc.time - t > 0.15) {
                    self.last_hit = Some(sc.time);
                    let g = ((y.impact / 300.0).log10() / 1.5).clamp(0.2, 1.0);
                    mix.shots.push(Shot { role: "impact".into(), gain: fx * g, pan: 0.0 });
                }
            }
            let touching = self.contact.iter().filter(|i| **i > 0.0).count();
            let mean = self.contact.iter().sum::<f32>() / self.contact.len().max(1) as f32;
            mix.scrape = if touching >= 2 && y.speed > 5.0 { fx * (mean / 300.0).clamp(0.25, 1.0) } else { 0.0 };
            // A gear change: the clunk.
            if self.gear.is_some_and(|g| g != y.gear) && y.gear != 0 {
                mix.shots.push(Shot { role: "shift".into(), gain: fx * 0.7, pan: 0.0 });
            }
            self.gear = Some(y.gear);
            // The crowd: a murmur everywhere, a roar by the grandstands.
            let d = sc.stands.iter().map(|p| ((p.0 - y.pos.0).powi(2) + (p.1 - y.pos.1).powi(2)).sqrt()).fold(f32::MAX, f32::min);
            mix.crowd = master * b.effects * (0.12 + 0.6 / (1.0 + (d / 60.0).powi(2)));
            // The field: each car's engine by distance, bearing and closing speed; a pass-by as one goes by.
            let right = (y.heading.sin(), -y.heading.cos());
            let o = s.others;
            for c in sc.others {
                let (dx, dy) = (c.pos.0 - y.pos.0, c.pos.1 - y.pos.1);
                let d = (dx * dx + dy * dy).sqrt().max(0.1);
                let u = (dx / d, dy / d);
                let att = if d > o.max {
                    0.0
                } else if d <= o.reference {
                    1.0
                } else {
                    o.reference / (o.reference + o.rolloff * (d - o.reference))
                };
                let (toward_you, away) = (y.vel.0 * u.0 + y.vel.1 * u.1, c.vel.0 * u.0 + c.vel.1 * u.1);
                let pitch = ((C + toward_you * o.doppler) / (C + away * o.doppler)).clamp(0.5, 2.0);
                let pan = (u.0 * right.0 + u.1 * right.1).clamp(-1.0, 1.0);
                let gain = if sc.paused { 0.0 } else { master * b.others * att * (0.45 + 0.55 * c.throttle) };
                mix.others.push(EngineVoice { rpm: c.rpm, throttle: c.throttle, gain, pan, pitch });
                // Closest approach: when it comes, how close; the recording starts so its loudest part lands there.
                let rel = (c.vel.0 - y.vel.0, c.vel.1 - y.vel.1);
                let rv = rel.0 * rel.0 + rel.1 * rel.1;
                let ear = self.ears.entry(c.id).or_default();
                if let (Some(peak), true) = (sc.passby_peak, rv > 100.0) {
                    let t = -(dx * rel.0 + dy * rel.1) / rv;
                    let miss = ((dx + rel.0 * t).powi(2) + (dy + rel.1 * t).powi(2)).sqrt();
                    if t > 0.0 && t < peak && miss < 10.0 && ear.passby_at.is_none_or(|t| sc.time - t > 3.0) && !sc.paused {
                        ear.passby_at = Some(sc.time);
                        let k = (rv.sqrt() / 30.0).clamp(0.3, 1.0) * (4.0 / miss.max(4.0));
                        mix.shots.push(Shot { role: "passby".into(), gain: master * b.others * k, pan });
                    }
                }
                // A big hit ahead: the spotter calls the wreck.
                let ahead = u.0 * y.heading.cos() + u.1 * y.heading.sin() > 0.3;
                if c.impact > 3000.0 && ahead && d < 300.0 && self.wreck_at.is_none_or(|t| sc.time - t > 10.0) {
                    self.wreck_at = Some(sc.time);
                    announce = Some("wreck");
                }
            }
            // Music ducks under the engine (sidechain), louder on throttle.
            let level = eng * (0.35 + 0.65 * y.throttle);
            self.duck += (level - self.duck) * 0.1;
        }
        // Music: the menu before the green and while paused, the results after the flag, none while racing.
        mix.music = match (sc.phase, sc.paused) {
            (Phase::Finished, _) => Some("music_results".into()),
            (Phase::Grid, _) | (_, true) => Some("music_menu".into()),
            _ => None,
        };
        let duck = if sc.paused { 0.0 } else { (self.duck / (master * b.engine).max(1e-3)).clamp(0.0, 1.0) * s.duck };
        mix.music_gain = master * b.music * (1.0 - duck);
        (mix, announce)
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

    #[test]
    fn loops_crossfade_at_equal_power_between_the_two_nearest() {
        let rpms = [8000.0, 2000.0, 5000.0];
        assert_eq!(band_weights(&rpms, 1000.0), vec![0.0, 1.0, 0.0], "below the lowest: it alone");
        assert_eq!(band_weights(&rpms, 9000.0), vec![1.0, 0.0, 0.0]);
        let w = band_weights(&rpms, 6500.0);
        assert!((w[2] - w[0]).abs() < 1e-5 && w[1] == 0.0, "halfway: equal shares {w:?}");
        assert!((w.iter().map(|x| x * x).sum::<f32>() - 1.0).abs() < 1e-5, "constant power");
        let (on, off) = load_weights(1.0);
        assert!(on > 0.999 && off < 1e-3);
    }

    #[test]
    fn a_sweep_is_sliced_into_seamless_loops_at_the_rpm_of_their_middle() {
        // 10 s rising 2,000 → 9,500 rpm.
        let data: Vec<f32> = (0..RATE as usize * 10).map(|i| (i as f32 * 0.01).sin()).collect();
        let s = Sweep { file: String::new(), from: 2000.0, to: 9500.0, bands: 5, length: 0.6 };
        let bands = slice_sweep(&data, &s);
        assert_eq!(bands.len(), 5);
        assert!(
            (bands[0].0 - 2750.0).abs() < 1.0 && (bands[4].0 - 8750.0).abs() < 1.0,
            "{:?}",
            bands.iter().map(|b| b.0).collect::<Vec<_>>()
        );
        // A loop is its length less the crossfade, and its seam is continuous.
        let l = &bands[2].1;
        assert!(l.len() < (0.6 * RATE as f32) as usize && l.len() > (0.45 * RATE as f32) as usize);
        assert!((l[0] - l[l.len() - 1]).abs() < 0.05, "no click at the seam");
    }

    fn car(id: u64, pos: (f32, f32), vel: (f32, f32)) -> Heard {
        Heard {
            id,
            pos,
            vel,
            heading: 0.0,
            rpm: 8000.0,
            throttle: 1.0,
            speed: (vel.0 * vel.0 + vel.1 * vel.1).sqrt(),
            gear: 4,
            ..Heard::default()
        }
    }

    fn scene<'a>(you: Heard, others: &'a [Heard], time: f32, tick: u64, phase: Phase) -> Scene<'a> {
        Scene { you: Some(you), others, stands: &[], time, tick, phase, paused: false, passby_peak: Some(1.0) }
    }

    #[test]
    fn another_car_is_quieter_far_off_higher_coming_and_lower_going_and_on_its_side() {
        let s = Sound::default();
        let mut d = Director::default();
        let you = car(1, (0.0, 0.0), (60.0, 0.0));
        // Ahead and closing (it is slower), to your left; behind and dropping away, far.
        let others = [car(2, (40.0, 5.0), (40.0, 0.0)), car(3, (-300.0, 0.0), (40.0, 0.0))];
        let (m, _) = d.listen(&s, &scene(you, &others, 0.0, 1, Phase::Racing));
        let (near, far) = (m.others[0], m.others[1]);
        assert!(near.gain > far.gain * 5.0, "{} vs {}", near.gain, far.gain);
        assert!(near.pitch > 1.0 && far.pitch < 1.0, "Doppler: closing {} opening {}", near.pitch, far.pitch);
        assert!(near.pan < -0.05, "left of you (heading +x, left is +y): {}", near.pan);
        assert!(m.music.is_none(), "no music while racing");
    }

    #[test]
    fn sliding_squeals_a_gear_change_clunks_a_hit_knocks_and_the_grid_has_its_music() {
        let s = Sound::default();
        let mut d = Director::default();
        let mut you = car(1, (0.0, 0.0), (50.0, 0.0));
        let (m, _) = d.listen(&s, &scene(you, &[], 0.0, 1, Phase::Grid));
        assert_eq!(m.music.as_deref(), Some("music_menu"));
        assert!(m.squeal.0 == 0.0 && m.shots.is_empty());
        you.slip = 0.12;
        you.gear = 5;
        you.impact = 2000.0;
        let (m, _) = d.listen(&s, &scene(you, &[], 1.0, 2, Phase::Racing));
        assert!(m.squeal.0 > 0.2, "{:?}", m.squeal);
        let roles: Vec<&str> = m.shots.iter().map(|x| x.role.as_str()).collect();
        assert!(roles.contains(&"shift") && roles.contains(&"impact"), "{roles:?}");
        // Held against the wall: a scrape, not a knock every tick.
        you.impact = 150.0;
        let mut knocks = 0;
        let mut scrape = 0.0;
        for t in 3..12 {
            let (m, _) = d.listen(&s, &scene(you, &[], 1.0 + t as f32 / 60.0, t, Phase::Racing));
            knocks += m.shots.iter().filter(|x| x.role == "impact").count();
            scrape = m.scrape;
        }
        assert_eq!(knocks, 0);
        assert!(scrape > 0.0);
        let (m, _) = d.listen(&s, &scene(you, &[], 9.0, 99, Phase::Finished));
        assert_eq!(m.music.as_deref(), Some("music_results"));
    }

    #[test]
    fn a_car_going_by_starts_the_passby_so_its_loudest_part_lands_as_it_passes() {
        let s = Sound::default();
        let mut d = Director::default();
        let you = car(1, (0.0, 0.0), (50.0, 0.0));
        // 30 m behind, 20 m/s faster, one lane over: alongside in 1.5 s; the recording peaks 1 s in.
        let mut shots = Vec::new();
        for f in 0..90 {
            let t = f as f32 / 60.0;
            let other = [car(2, (-30.0 + 20.0 * t, 3.0), (70.0, 0.0))];
            let (m, _) = d.listen(&s, &scene(car(1, (0.0, 0.0), you.vel), &other, t, f, Phase::Racing));
            shots.extend(m.shots.into_iter().filter(|x| x.role == "passby").map(|_| t));
        }
        assert_eq!(shots.len(), 1, "once: {shots:?}");
        assert!((shots[0] - 0.5).abs() < 0.05, "1 s before it is alongside: {}", shots[0]);
    }
}
