//! Observability: what the app did in the last second, as one JSON line. The shell prints it on standard output,
//! which `xcrun devicectl device process launch --console` streams from a phone to the developer's machine over
//! Wi-Fi. The first step of the live link's telemetry (`docs/architecture.md`, Mobile core); later the same line
//! goes over the network with the touches as a replay.

use std::fmt::Write;

/// Counts and times over a window (a second), then reports and starts again.
#[derive(Clone, Debug, Default)]
pub struct Stats {
    /// Frame intervals (ms): the time between frames as the player sees it.
    frames: Vec<f32>,
    /// CPU time spent inside each frame (ms): simulation ticks plus building and submitting the drawing.
    work: Vec<f32>,
    /// Simulation ticks run, and their total time (µs).
    ticks: u32,
    tick_us: f32,
    gestures: u32,
    pulses: u32,
    /// The window's length so far (ms).
    elapsed: f32,
}

/// One window's summary.
#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    pub fps: f32,
    /// Median and worst frame interval (ms).
    pub frame_ms: (f32, f32),
    /// Median and worst CPU work per frame (ms).
    pub work_ms: (f32, f32),
    pub ticks: u32,
    /// Mean time of one simulation tick (µs).
    pub tick_us: f32,
    pub gestures: u32,
    pub pulses: u32,
}

impl Stats {
    /// A frame `interval_ms` after the last one, whose own work took `work_ms`.
    pub fn frame(&mut self, interval_ms: f32, work_ms: f32) {
        self.frames.push(interval_ms);
        self.work.push(work_ms);
        self.elapsed += interval_ms;
    }

    pub fn tick(&mut self, us: f32) {
        self.ticks += 1;
        self.tick_us += us;
    }

    pub fn gesture(&mut self) {
        self.gestures += 1;
    }

    pub fn pulse(&mut self) {
        self.pulses += 1;
    }

    /// The report once a window of `window_ms` is full (and a fresh window), else nothing.
    pub fn take(&mut self, window_ms: f32) -> Option<Report> {
        if self.elapsed < window_ms || self.frames.is_empty() {
            return None;
        }
        let s = std::mem::take(self);
        let spread = |mut v: Vec<f32>| {
            v.sort_by(f32::total_cmp);
            (v[v.len() / 2], v[v.len() - 1])
        };
        Some(Report {
            fps: s.frames.len() as f32 * 1000.0 / s.elapsed,
            frame_ms: spread(s.frames),
            work_ms: spread(s.work),
            ticks: s.ticks,
            tick_us: if s.ticks > 0 { s.tick_us / s.ticks as f32 } else { 0.0 },
            gestures: s.gestures,
            pulses: s.pulses,
        })
    }
}

impl Report {
    /// One JSON line; `extra` holds what the scene adds (already JSON: `"tension":0.12`).
    pub fn json(&self, t_ms: u64, size: (u32, u32), extra: &str) -> String {
        let mut j = String::new();
        let _ = write!(
            j,
            "{{\"t\":{t_ms},\"fps\":{:.1},\"frame_ms\":[{:.2},{:.2}],\"work_ms\":[{:.2},{:.2}],\"ticks\":{},\"tick_us\":{:.1},\"gestures\":{},\"pulses\":{},\"size\":[{},{}]",
            self.fps,
            self.frame_ms.0,
            self.frame_ms.1,
            self.work_ms.0,
            self.work_ms.1,
            self.ticks,
            self.tick_us,
            self.gestures,
            self.pulses,
            size.0,
            size.1
        );
        if !extra.is_empty() {
            j.push(',');
            j.push_str(extra);
        }
        j.push('}');
        j
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_reports_once_full_and_starts_again() {
        let mut s = Stats::default();
        for _ in 0..59 {
            s.frame(1000.0 / 60.0, 2.0);
        }
        assert_eq!(s.take(1000.0), None, "59 frames at 60 fps are not yet a second");
        s.frame(1000.0 / 60.0, 9.0);
        s.tick(40.0);
        s.tick(60.0);
        let r = s.take(1000.0).expect("a full second");
        assert!((r.fps - 60.0).abs() < 0.01, "{r:?}");
        assert_eq!(r.work_ms, (2.0, 9.0));
        assert_eq!((r.ticks, r.tick_us), (2, 50.0));
        assert_eq!(s.take(1000.0), None, "the next window starts empty");
    }

    #[test]
    fn a_hitch_shows_as_the_worst_frame_not_the_median() {
        let mut s = Stats::default();
        for i in 0..60 {
            s.frame(if i == 30 { 100.0 } else { 16.0 }, 1.0);
        }
        let r = s.take(1000.0).unwrap();
        assert_eq!(r.frame_ms, (16.0, 100.0));
    }

    #[test]
    fn the_line_is_json() {
        let r = Report { fps: 59.94, frame_ms: (16.7, 33.4), work_ms: (1.2, 3.4), ticks: 60, tick_us: 38.25, gestures: 3, pulses: 1 };
        assert_eq!(
            r.json(5000, (1179, 2556), "\"tension\":0.10"),
            "{\"t\":5000,\"fps\":59.9,\"frame_ms\":[16.70,33.40],\"work_ms\":[1.20,3.40],\"ticks\":60,\"tick_us\":38.2,\"gestures\":3,\"pulses\":1,\"size\":[1179,2556],\"tension\":0.10}"
        );
    }
}
