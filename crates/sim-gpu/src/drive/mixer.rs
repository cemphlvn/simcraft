//! The drive view's mixer, where the samples are made (the host's audio thread; `audio` decides what plays).
//!
//! - **Engines** (yours and the field's): RPM-crossfaded loops, the granular engine audio racing games use: each
//!   loop is tagged with the rpm it was recorded at and played pitched by rpm / recorded rpm, the two loops
//!   nearest the rpm crossfade at equal power, and the throttle blends the on-load loops into the off-load ones.
//!   A loop whose recording is missing plays the synthesiser (harmonics of the four-stroke cycle and a rasp at the
//!   firing rate) with its share, so any subset of recordings works.
//! - **Effects**: tyres, scraping, wind and the crowd as loops (or synthesised noise), knocks, shifts and pass-bys as
//!   one-shots.
//! - **Buses**: every level comes in already scaled by its bus (`audio::Buses`) and the master; the mix is stereo
//!   (equal-power balance), soft-clipped.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use super::audio::{EngineVoice, Load, Mix, RATE, Sound, band_weights, level, load_weights, make_loop, resample, slice_sweep};

/// A recording, mono at the mixer's rate.
pub type Clip = Arc<[f32]>;

/// A recording decoded to mono at the mixer's rate.
pub fn decode(path: &Path) -> Result<Vec<f32>, String> {
    use rodio::Source;
    let f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let dec = rodio::Decoder::new(std::io::BufReader::new(f)).map_err(|e| format!("{}: {e}", path.display()))?;
    let (ch, rate) = (dec.channels().get() as usize, dec.sample_rate().get());
    let all: Vec<f32> = dec.collect();
    let mono: Vec<f32> = all.chunks(ch.max(1)).map(|c| c.iter().sum::<f32>() / c.len() as f32).collect();
    if mono.is_empty() {
        return Err(format!("{}: no samples", path.display()));
    }
    Ok(resample(&mono, rate))
}

/// One engine loop: the rpm it was recorded at, its load, its samples (none: the synthesiser plays its share).
#[derive(Clone)]
pub struct Band {
    pub rpm: f32,
    pub load: Load,
    pub clip: Option<Clip>,
}

/// Everything the mixer plays from, decoded once.
#[derive(Clone, Default)]
pub struct Bank {
    pub bands: Vec<Band>,
    /// Effects by role (mono; the loops made seamless).
    pub roles: BTreeMap<String, Clip>,
    /// When the pass-by recording is loudest (s from its start).
    pub passby_peak: Option<f32>,
    pub cylinders: f32,
}

/// Roles that loop (the rest play once).
const LOOPS: [&str; 4] = ["squeal", "scrape", "wind", "crowd"];

impl Bank {
    /// Decodes what `sound` names and finds; reports what failed to decode (a missing file is not a failure).
    pub fn load(game: &Path, s: &Sound) -> (Bank, Vec<String>) {
        let mut problems = Vec::new();
        let mut get = |p: Option<std::path::PathBuf>| {
            p.and_then(|p| match decode(&p) {
                Ok(d) => Some(d),
                Err(e) => {
                    problems.push(e);
                    None
                }
            })
        };
        let mut bands: Vec<Band> = Vec::new();
        for l in &s.loops {
            let clip = get(s.file(game, &l.file)).map(|d| Clip::from(level(make_loop(d, RATE as usize / 10), 0.2)));
            bands.push(Band { rpm: l.rpm.max(100.0), load: l.load, clip });
        }
        if let Some(sw) = &s.sweep
            && let Some(d) = get(s.file(game, &sw.file))
        {
            // The sweep's slices fill the on-load engine; loops of their own that were found stay (idle, off-load).
            bands.retain(|b| b.clip.is_some() && b.load != Load::On);
            for (rpm, d) in slice_sweep(&d, sw) {
                bands.push(Band { rpm, load: Load::On, clip: Some(Clip::from(level(d, 0.2))) });
            }
        }
        let mut roles = BTreeMap::new();
        let mut passby_peak = None;
        for (role, _) in super::audio::ROLES {
            if role.starts_with("music") {
                continue;
            }
            let Some(d) = get(s.role(game, role)) else { continue };
            let d = if LOOPS.contains(&role) { level(make_loop(d, RATE as usize / 5), 0.2) } else { d };
            if role == "passby" {
                // The loudest 50 ms of the recording.
                let win = RATE as usize / 20;
                let (best, _) = d
                    .chunks(win)
                    .enumerate()
                    .map(|(i, c)| (i, c.iter().map(|v| v * v).sum::<f32>()))
                    .fold((0, 0.0), |a, b| if b.1 > a.1 { b } else { a });
                passby_peak = Some(best as f32 * win as f32 / RATE as f32);
            }
            roles.insert(role.to_string(), Clip::from(d));
        }
        (Bank { bands, roles, passby_peak, cylinders: s.cylinders.max(1) as f32 }, problems)
    }
}

// ---------------------------------------------------------------- synthesis

/// A little xorshift noise (per voice, so voices do not correlate).
#[derive(Clone)]
struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0 as f32 / u32::MAX as f32 * 2.0 - 1.0
    }
}

/// The engine note without recordings: harmonics of the four-stroke cycle (the firing order strongest, a V8's half
/// order, its doubles) and a rasp of noise pulsed at the firing rate that grows with load.
#[derive(Clone)]
struct Synth {
    phase: f64,
    rasp: f32,
    noise: Noise,
    harmonics: u32,
}

impl Synth {
    fn new(seed: u32, harmonics: u32) -> Synth {
        Synth { phase: 0.0, rasp: 0.0, noise: Noise(seed | 1), harmonics }
    }

    fn sample(&mut self, rpm: f32, load: f32, cylinders: f32) -> f32 {
        let cycle = (rpm / 120.0).max(1.0) as f64;
        self.phase = (self.phase + cycle / RATE as f64).fract();
        let tau = std::f64::consts::TAU;
        let fire = cylinders;
        let mut harm = 0.0f32;
        for n in 1..=self.harmonics {
            if cycle as f32 * n as f32 > 7000.0 {
                break;
            }
            let order = n as f32;
            let a = if order == fire {
                1.0
            } else if order == fire / 2.0 {
                0.55
            } else if order == fire * 2.0 {
                0.4
            } else if order == fire * 1.5 {
                0.28
            } else if order == fire * 3.0 {
                0.18
            } else {
                0.05 * (0.4 + load) / order.sqrt()
            };
            harm += a * ((tau * n as f64 * self.phase).sin() as f32);
        }
        let pulse = 0.5 + 0.5 * ((tau * fire as f64 * self.phase).sin() as f32);
        let w = self.noise.next();
        self.rasp += (w - self.rasp) * 0.25;
        let rasp = self.rasp * pulse * pulse * load * (0.25 + rpm / 12_000.0) * 0.6;
        (harm * 0.18 * (0.35 + 0.65 * load) + rasp) * 1.2
    }
}

/// A two-pole resonant band-pass (state-variable filter) for synthesised effects.
#[derive(Clone, Default)]
struct Svf {
    low: f32,
    band: f32,
}

impl Svf {
    fn band(&mut self, x: f32, hz: f32, q: f32) -> f32 {
        let f = 2.0 * (std::f32::consts::PI * hz / RATE as f32).sin();
        let high = x - self.low - self.band / q;
        self.band += f * high;
        self.low += f * self.band;
        self.band
    }
}

// ---------------------------------------------------------------- voices

/// An engine playing: a read position per loop, the synthesiser, the weights on a glide.
#[derive(Clone)]
struct Engine {
    pos: Vec<f64>,
    weights: Vec<f32>,
    synth_w: f32,
    synth: Synth,
    now: EngineVoice,
}

impl Engine {
    fn new(bands: usize, seed: u32, harmonics: u32) -> Engine {
        Engine {
            pos: vec![0.0; bands],
            weights: vec![0.0; bands],
            synth_w: 1.0,
            synth: Synth::new(seed, harmonics),
            now: EngineVoice::default(),
        }
    }

    /// Glides toward `to` (a few ms: a frame's new values never click), sets the loops' weights for it.
    fn target(&mut self, to: &EngineVoice, bank: &Bank, k: f32) {
        let n = &mut self.now;
        n.rpm += (to.rpm - n.rpm) * k;
        n.throttle += (to.throttle - n.throttle) * k;
        n.gain += (to.gain - n.gain) * k;
        n.pan += (to.pan - n.pan) * k;
        n.pitch += (to.pitch.max(0.1) - n.pitch) * k;
        let pick = |on: bool| -> Vec<usize> {
            (0..bank.bands.len()).filter(|i| bank.bands[*i].load == Load::Both || (bank.bands[*i].load == Load::On) == on).collect()
        };
        let (on_set, off_set) = (pick(true), pick(false));
        let (mut lon, mut loff) = load_weights(n.throttle);
        if off_set.is_empty() {
            (lon, loff) = (1.0, 0.0);
        }
        let mut w = vec![0.0f32; bank.bands.len()];
        for (set, share) in [(&on_set, lon), (&off_set, loff)] {
            let rpms: Vec<f32> = set.iter().map(|i| bank.bands[*i].rpm).collect();
            for (j, bw) in band_weights(&rpms, n.rpm).into_iter().enumerate() {
                w[set[j]] += bw * share;
            }
        }
        // Without off-load recordings, lifting off is quieter.
        let lift = if off_set.is_empty() { 0.55 + 0.45 * n.throttle } else { 1.0 };
        let synth: f32 =
            if bank.bands.is_empty() { 1.0 } else { (0..w.len()).filter(|i| bank.bands[*i].clip.is_none()).map(|i| w[i]).sum() };
        for (cur, want) in self.weights.iter_mut().zip(w) {
            *cur += (want * lift - *cur) * 0.2;
        }
        self.synth_w += (synth - self.synth_w) * 0.2;
    }

    fn sample(&mut self, bank: &Bank) -> f32 {
        let n = self.now;
        if n.gain < 1e-4 {
            return 0.0;
        }
        let mut x = 0.0;
        for (i, b) in bank.bands.iter().enumerate() {
            let Some(clip) = &b.clip else { continue };
            let w = self.weights[i];
            let len = clip.len() as f64;
            let p = &mut self.pos[i];
            if w > 1e-4 {
                let j = *p as usize;
                let f = (*p - j as f64) as f32;
                let (a, c) = (clip[j % clip.len()], clip[(j + 1) % clip.len()]);
                x += w * (a + (c - a) * f);
            }
            *p = (*p + (n.rpm / b.rpm * n.pitch).clamp(0.25, 4.0) as f64) % len;
        }
        if self.synth_w > 1e-3 {
            x += self.synth_w * self.synth.sample(n.rpm * n.pitch, n.throttle, bank.cylinders);
        }
        x * n.gain
    }
}

/// A looping effect: a recording read at a pitch, or its synthesis.
#[derive(Clone, Default)]
struct Loop {
    pos: f64,
    gain: f32,
    pitch: f32,
    svf: Svf,
    slow: f32,
}

impl Loop {
    fn sample(&mut self, clip: Option<&Clip>, to: (f32, f32), noise: &mut Noise, synth: fn(&mut Loop, f32) -> f32) -> f32 {
        self.gain += (to.0 - self.gain) * 0.002;
        self.pitch += (to.1.max(0.1) - self.pitch) * 0.002;
        if self.gain < 1e-4 {
            return 0.0;
        }
        let x = match clip {
            Some(c) => {
                let j = self.pos as usize % c.len();
                self.pos = (self.pos + self.pitch as f64) % c.len() as f64;
                c[j]
            }
            None => synth(self, noise.next()),
        };
        x * self.gain
    }
}

fn squeal_synth(l: &mut Loop, w: f32) -> f32 {
    let hz = 1300.0 * l.pitch;
    l.svf.band(w, hz, 12.0) * 0.5
}

fn scrape_synth(l: &mut Loop, w: f32) -> f32 {
    // Grinding: band noise around 500 Hz, chopped by a rough grain.
    l.slow += (w.abs() - l.slow) * 0.01;
    l.svf.band(w, 520.0, 2.0) * (0.4 + l.slow * 1.4)
}

fn wind_synth(l: &mut Loop, w: f32) -> f32 {
    let hz = 380.0 * l.pitch;
    l.svf.band(w, hz, 0.7) * 0.9
}

fn crowd_synth(l: &mut Loop, w: f32) -> f32 {
    // Many voices far off: low band noise, swelling slowly.
    l.slow += (w - l.slow) * 0.00005;
    l.svf.band(w, 700.0, 0.8) * (0.5 + l.slow.abs() * 20.0).min(1.2) * 0.5
}

/// A one-shot playing: its recording or a synthesised knock or clunk.
struct Playing {
    clip: Option<Clip>,
    role: String,
    pos: usize,
    gain: f32,
    pan: f32,
}

impl Playing {
    /// The next sample, or None when it is over.
    fn sample(&mut self, noise: &mut Noise) -> Option<f32> {
        let i = self.pos;
        self.pos += 1;
        match &self.clip {
            Some(c) => c.get(i).map(|v| v * self.gain),
            None => {
                let t = i as f32 / RATE as f32;
                let tau = std::f32::consts::TAU;
                let x = match self.role.as_str() {
                    // A thump (55 Hz, falling) under a crunch of noise.
                    "impact" if t < 0.6 => {
                        (tau * 55.0 * t * (1.0 - t * 0.5)).sin() * (-t * 9.0).exp() + noise.next() * (-t * 14.0).exp() * 0.6
                    }
                    // A short click and a low clunk.
                    "shift" if t < 0.15 => noise.next() * (-t * 300.0).exp() * 0.5 + (tau * 90.0 * t).sin() * (-t * 40.0).exp() * 0.5,
                    _ => return None,
                };
                Some(x * self.gain)
            }
        }
    }
}

// ---------------------------------------------------------------- the desk (a rodio source)

/// Where the host puts this frame's mix; the audio thread reads it every few milliseconds.
pub type Shared = Arc<Mutex<Mix>>;

/// The mixer as a stereo source at 44.1 kHz.
pub struct Desk {
    bank: Arc<Bank>,
    shared: Shared,
    target: Mix,
    you: Engine,
    others: Vec<Engine>,
    squeal: Loop,
    scrape: Loop,
    wind: Loop,
    crowd: Loop,
    shots: Vec<Playing>,
    noise: Noise,
    frame: usize,
    right: Option<f32>,
}

impl Desk {
    pub fn new(bank: Arc<Bank>, shared: Shared) -> Desk {
        let n = bank.bands.len();
        Desk {
            you: Engine::new(n, 0x1234_5678, 32),
            bank,
            shared,
            target: Mix::default(),
            others: Vec::new(),
            squeal: Loop::default(),
            scrape: Loop::default(),
            wind: Loop::default(),
            crowd: Loop::default(),
            shots: Vec::new(),
            noise: Noise(0x9E37_79B9),
            frame: 0,
            right: None,
        }
    }

    /// Every 128 frames: the host's latest mix (the one-shots taken, the rest copied).
    fn refresh(&mut self) {
        if let Ok(mut m) = self.shared.try_lock() {
            let shots = std::mem::take(&mut m.shots);
            self.target = Mix { shots: Vec::new(), ..m.clone() };
            for s in shots {
                let clip = self.bank.roles.get(&s.role).cloned();
                if clip.is_none() && s.role == "passby" {
                    continue;
                }
                self.shots.push(Playing { clip, role: s.role, pos: 0, gain: s.gain, pan: s.pan });
            }
        }
        let n = self.bank.bands.len();
        while self.others.len() < self.target.others.len() {
            let seed = 0x5EED_0000 + self.others.len() as u32 * 7919;
            self.others.push(Engine::new(n, seed, 12));
        }
        let k = 0.25;
        self.you.target(&self.target.engine, &self.bank, k);
        let silent = EngineVoice::default();
        for (i, e) in self.others.iter_mut().enumerate() {
            e.target(self.target.others.get(i).unwrap_or(&silent), &self.bank, k);
        }
    }

    fn stereo(&mut self) -> (f32, f32) {
        if self.frame.is_multiple_of(128) {
            self.refresh();
        }
        self.frame += 1;
        let bank = self.bank.clone();
        let mid = self.you.sample(&bank);
        let (mut l, mut r) = (mid, mid);
        let pan = |p: f32| {
            let a = (p.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
            (a.cos() * std::f32::consts::SQRT_2, a.sin() * std::f32::consts::SQRT_2)
        };
        for e in &mut self.others {
            let x = e.sample(&bank);
            let (pl, pr) = pan(e.now.pan);
            l += x * pl;
            r += x * pr;
        }
        let t = &self.target;
        let fx = self.squeal.sample(bank.roles.get("squeal"), t.squeal, &mut self.noise, squeal_synth)
            + self.scrape.sample(bank.roles.get("scrape"), (t.scrape, 1.0), &mut self.noise, scrape_synth)
            + self.crowd.sample(bank.roles.get("crowd"), (t.crowd, 1.0), &mut self.noise, crowd_synth);
        let wind = self.wind.sample(bank.roles.get("wind"), t.wind, &mut self.noise, wind_synth);
        l += fx + wind;
        r += fx + wind;
        let noise = &mut self.noise;
        self.shots.retain_mut(|s| match s.sample(noise) {
            Some(x) => {
                let (pl, pr) = pan(s.pan);
                l += x * pl;
                r += x * pr;
                true
            }
            None => false,
        });
        (l.tanh(), r.tanh())
    }
}

impl Iterator for Desk {
    type Item = rodio::Sample;

    fn next(&mut self) -> Option<rodio::Sample> {
        if let Some(r) = self.right.take() {
            return Some(r);
        }
        let (l, r) = self.stereo();
        self.right = Some(r);
        Some(l)
    }
}

impl rodio::Source for Desk {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> rodio::ChannelCount {
        std::num::NonZero::new(2).expect("two")
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        std::num::NonZero::new(RATE).expect("a rate")
    }
    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bank(bands: Vec<Band>) -> Arc<Bank> {
        Arc::new(Bank { bands, cylinders: 8.0, ..Bank::default() })
    }

    fn tone(hz: f32) -> Clip {
        Clip::from((0..RATE as usize).map(|i| (std::f32::consts::TAU * hz * i as f32 / RATE as f32).sin() * 0.3).collect::<Vec<_>>())
    }

    fn run(desk: &mut Desk, seconds: f32) -> Vec<(f32, f32)> {
        (0..(seconds * RATE as f32) as usize).map(|_| desk.stereo()).collect()
    }

    #[test]
    fn recordings_are_decoded_made_seamless_and_a_missing_one_is_left_to_the_synth() {
        // Any recording will do to exercise the path: the spotter's lines stand in for an engine loop and a knock.
        let game = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../games/race");
        let s = Sound {
            dirs: vec!["race/voice".into()],
            loops: vec![
                super::super::audio::EngineLoop { file: "green".into(), rpm: 5000.0, load: Load::On },
                super::super::audio::EngineLoop { file: "no_such_loop".into(), rpm: 8000.0, load: Load::On },
            ],
            files: BTreeMap::from([("impact".to_string(), "wreck".to_string())]),
            ..Sound::default()
        };
        let (b, problems) = Bank::load(&game, &s);
        assert!(problems.is_empty(), "{problems:?}");
        assert!(b.bands[0].clip.as_ref().is_some_and(|c| c.len() > RATE as usize / 2), "decoded at the mixer's rate");
        assert!(b.bands[1].clip.is_none(), "missing: the synthesiser's share");
        let rms = |c: &Clip| (c.iter().map(|v| v * v).sum::<f32>() / c.len() as f32).sqrt();
        assert!((rms(b.bands[0].clip.as_ref().unwrap()) - 0.2).abs() < 0.01, "levelled for the crossfade");
        assert!(b.roles.contains_key("impact") && !b.roles.contains_key("squeal"));
    }

    /// What a second of the full field costs to mix (`cargo test --release -p sim-gpu mixer_cost -- --ignored
    /// --nocapture`): your engine and seven others synthesised, every effect on.
    #[test]
    #[ignore]
    fn mixer_cost() {
        let shared: Shared = Arc::default();
        {
            let mut m = shared.lock().unwrap();
            let v = EngineVoice { rpm: 8000.0, throttle: 1.0, gain: 0.3, pan: 0.3, pitch: 1.0 };
            (m.engine, m.others) = (v, vec![v; 7]);
            (m.squeal, m.scrape, m.wind, m.crowd) = ((0.3, 1.0), 0.3, (0.3, 1.0), 0.3);
        }
        let mut d = Desk::new(bank(Vec::new()), shared);
        let t = std::time::Instant::now();
        run(&mut d, 1.0);
        println!("one second of the full field: {:.1} ms", t.elapsed().as_secs_f64() * 1000.0);
    }

    #[test]
    fn with_no_recordings_the_synthesiser_plays_the_engine() {
        let shared: Shared = Arc::default();
        shared.lock().unwrap().engine = EngineVoice { rpm: 6000.0, throttle: 1.0, gain: 0.5, pan: 0.0, pitch: 1.0 };
        let mut d = Desk::new(bank(Vec::new()), shared);
        let out = run(&mut d, 0.3);
        let rms = (out.iter().skip(4000).map(|s| s.0 * s.0).sum::<f32>() / (out.len() - 4000) as f32).sqrt();
        assert!(rms > 0.02, "an engine note: rms {rms}");
    }

    #[test]
    fn a_loop_is_pitched_by_rpm_over_its_recorded_rpm_and_a_missing_band_falls_to_the_synth() {
        // Two loops: 4,000 rpm (a 100 Hz tone) and 8,000 rpm (missing).
        let b = bank(vec![Band { rpm: 4000.0, load: Load::On, clip: Some(tone(100.0)) }, Band { rpm: 8000.0, load: Load::On, clip: None }]);
        let shared: Shared = Arc::default();
        shared.lock().unwrap().engine = EngineVoice { rpm: 4000.0, throttle: 1.0, gain: 1.0, pan: 0.0, pitch: 1.0 };
        let mut d = Desk::new(b, shared.clone());
        run(&mut d, 0.2);
        assert!(d.you.weights[0] > 0.95 && d.you.synth_w < 0.05, "at its own rpm the loop plays alone");
        // At 6,000 rpm: halfway, equal power between the loop and the synth; the loop reads 1.5 samples a sample.
        shared.lock().unwrap().engine.rpm = 6000.0;
        run(&mut d, 0.3);
        assert!((d.you.weights[0] - 0.707).abs() < 0.03 && (d.you.synth_w - 0.707).abs() < 0.03, "{:?} {}", d.you.weights, d.you.synth_w);
        let p0 = d.you.pos[0];
        d.stereo();
        let step = (d.you.pos[0] - p0 + RATE as f64) % RATE as f64;
        assert!((step - 1.5).abs() < 0.02, "pitch 6000 / 4000: {step}");
    }

    #[test]
    fn another_car_is_panned_to_its_side_and_a_knock_plays_once() {
        let shared: Shared = Arc::default();
        {
            let mut m = shared.lock().unwrap();
            m.others = vec![EngineVoice { rpm: 7000.0, throttle: 1.0, gain: 0.6, pan: 1.0, pitch: 1.0 }];
            m.shots.push(super::super::audio::Shot { role: "impact".into(), gain: 0.8, pan: 0.0 });
        }
        let mut d = Desk::new(bank(Vec::new()), shared.clone());
        run(&mut d, 0.01);
        assert!(shared.lock().unwrap().shots.is_empty() && d.shots.len() == 1, "the knock was taken, once");
        run(&mut d, 1.0);
        // The knock over, only the car on the right is left.
        let out = run(&mut d, 0.3);
        let (l, r) = out.iter().fold((0.0, 0.0), |a, s| (a.0 + s.0 * s.0, a.1 + s.1 * s.1));
        assert!(r > l * 20.0, "hard right: left {l}, right {r}");
        assert!(d.shots.is_empty(), "and it ended");
    }
}
