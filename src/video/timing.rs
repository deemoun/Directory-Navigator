//! Monotonic job timing and bounded FFmpeg progress records, independent of paint.
use super::Progress;
use std::time::Duration;
#[derive(Default)]
pub(super) struct Records {
    pending: Vec<u8>,
    seconds: Option<f64>,
}
impl Records {
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<f64> {
        let mut result = vec![];
        for &byte in bytes {
            if byte == b'\n' {
                let line = String::from_utf8_lossy(&self.pending);
                let line = line.trim_end_matches('\r');
                if let Some(value) = line.strip_prefix("out_time_us=") {
                    self.seconds = value
                        .parse::<f64>()
                        .ok()
                        .filter(|s| s.is_finite() && *s >= 0.0)
                        .map(|s| s / 1_000_000.0);
                } else if line.starts_with("progress=") {
                    if let Some(s) = self.seconds.take() {
                        result.push(s);
                    }
                }
                self.pending.clear();
            } else if self.pending.len() < 1024 {
                self.pending.push(byte);
            } else {
                // Overlong lines cannot become a valid record suffix.
                self.pending[0] = b'!';
            }
        }
        result
    }
}
#[derive(Clone, Default, Debug, PartialEq)]
pub(crate) struct Timing {
    pub elapsed: Duration,
    pub remaining: Option<Duration>,
    pub processed: f64,
    pub total: f64,
    pub stage: &'static str,
    pub clip: usize,
    pub clips: usize,
    pub waiting: bool,
    pub outcome: &'static str,
}
pub(super) struct Clock {
    pub view: Timing,
    last: Duration,
    sampled: Duration,
    media: f64,
    active: Duration,
    rate: Option<f64>,
}
impl Clock {
    pub fn new(total: f64) -> Self {
        Self {
            view: Timing {
                total,
                ..Default::default()
            },
            last: Duration::ZERO,
            sampled: Duration::ZERO,
            media: 0.0,
            active: Duration::ZERO,
            rate: None,
        }
    }
    pub fn update(&mut self, progress: &Progress, elapsed: Duration, waiting: bool) {
        let dt = elapsed.saturating_sub(self.last);
        if self.view.stage == "Converting" && !self.view.waiting && !waiting {
            self.active += dt;
        }
        self.last = elapsed;
        self.view.elapsed = elapsed;
        self.view.waiting = waiting;
        if !progress.stage.is_empty() {
            self.view.stage = progress.stage;
        }
        self.view.clip = progress.clip;
        self.view.clips = progress.clips;
        if let Some(s) = progress.processed.filter(|v| v.is_finite()) {
            self.view.processed = self.view.processed.max(s.clamp(0.0, self.view.total));
        }
        let sample = self.active.saturating_sub(self.sampled).as_secs_f64();
        if sample >= 2.0 && self.view.processed > self.media {
            let speed = (self.view.processed - self.media) / sample;
            self.rate = Some(self.rate.map_or(speed, |r| r * 0.75 + speed * 0.25));
            self.sampled = self.active;
            self.media = self.view.processed;
        }
        self.view.remaining =
            if !waiting && self.view.stage == "Converting" && self.active.as_secs() >= 3 {
                self.rate.and_then(|r| {
                    let seconds = (self.view.total - self.view.processed) / r;
                    (seconds.is_finite() && (0.0..=365.0 * 86400.0).contains(&seconds))
                        .then(|| Duration::from_secs_f64(seconds))
                })
            } else {
                None
            };
    }
    pub fn finish(&mut self, elapsed: Duration, outcome: &'static str) {
        self.view.elapsed = elapsed;
        self.view.remaining = None;
        self.view.waiting = false;
        self.view.outcome = outcome;
    }
}
pub(crate) fn time(duration: Duration) -> String {
    let s = duration.as_secs();
    format!("{:02}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
}
pub(crate) fn media_time(seconds: f64) -> String {
    time(Duration::try_from_secs_f64(seconds.max(0.0)).unwrap_or(Duration::MAX))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn records_are_fragmented_bounded_and_only_publish_complete_valid_groups() {
        let mut r = Records::default();
        assert!(r.feed(b"out_time_us=120").is_empty());
        assert!(r.feed(b"00000\r\nspeed=0.4x\nprogress=cont").is_empty());
        assert_eq!(r.feed(b"inue\n"), vec![12.0]);
        assert!(r.feed(b"out_time_us=NaN\nprogress=end\n").is_empty());
        assert!(r.feed(&vec![b'x'; 10000]).is_empty());
        assert!(r.pending.len() <= 1024);
        assert!(r.feed(b"\nout_time_us=-1\nprogress=end\n").is_empty());
    }
    #[test]
    fn clock_tracks_conversion_waits_phases_clamps_and_retains_final_time() {
        let mut c = Clock::new(100.0);
        let mut p = Progress::new("Converting", String::new());
        p.processed = Some(0.0);
        c.update(&p, Duration::ZERO, false);
        p.processed = Some(10.0);
        c.update(&p, Duration::from_secs(5), false);
        assert_eq!(c.view.remaining, Some(Duration::from_secs(45)));
        c.update(&p, Duration::from_secs(25), true);
        assert!(c.view.remaining.is_none());
        c.update(&p, Duration::from_secs(35), false);
        p.processed = Some(20.0);
        c.update(&p, Duration::from_secs(40), false);
        assert_eq!(c.view.remaining, Some(Duration::from_secs(40)));
        p.processed = Some(-1.0);
        c.update(&p, Duration::from_secs(40), false);
        assert_eq!(c.view.processed, 20.0);
        c.update(&Progress::from("Joining"), Duration::from_secs(45), false);
        assert!(c.view.remaining.is_none());
        c.finish(Duration::from_secs(50), "Completed in");
        assert_eq!(time(c.view.elapsed), "00:00:50");
        assert_eq!(c.view.outcome, "Completed in");
    }
}
