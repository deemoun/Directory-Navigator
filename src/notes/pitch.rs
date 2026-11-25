//! Monophonic YIN difference/CMND estimator. No audio I/O or allocations in callbacks.
//! The worker supplies a bounded frame; see de Cheveigné & Kawahara (2002), YIN.
use super::Reading;
pub const FRAME: usize = 4096;
pub struct Detector {
    normalized: Vec<f64>,
    interpolated: Vec<f64>,
    half_weights: [f64; 32],
}
impl Default for Detector {
    fn default() -> Self {
        let mut half_weights = [0.; 32];
        for (i, weight) in half_weights.iter_mut().enumerate() {
            let x = 15.5 - i as f64;
            *weight = (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x)
                * (0.54 - 0.46 * (std::f64::consts::TAU * i as f64 / 31.).cos());
        }
        let total = half_weights.iter().sum::<f64>();
        for weight in &mut half_weights {
            *weight /= total;
        }
        Self {
            normalized: vec![0.; FRAME],
            interpolated: vec![0.; FRAME * 2],
            half_weights,
        }
    }
}
impl Detector {
    pub fn detect(&mut self, samples: &[f32], rate: f64) -> Option<Reading> {
        if samples.len() != FRAME
            || !(8_000. ..=48_000.).contains(&rate)
            || samples.iter().any(|v| !v.is_finite())
        {
            return None;
        }
        let mean = samples.iter().map(|v| f64::from(*v)).sum::<f64>() / FRAME as f64;
        let power = samples
            .iter()
            .map(|v| (f64::from(*v) - mean).powi(2))
            .sum::<f64>()
            / FRAME as f64;
        if power < 0.000004 {
            return None;
        }
        // Reconstruct half-sample positions with a fixed windowed-sinc kernel.
        // High piano notes otherwise have too few integer lags and can be
        // mistaken for the octave below, especially with strong harmonics.
        for i in 0..FRAME {
            self.interpolated[2 * i] = f64::from(samples[i]);
            self.interpolated[2 * i + 1] = self
                .half_weights
                .iter()
                .enumerate()
                .map(|(j, w)| {
                    let index =
                        (i as isize + j as isize - 15).clamp(0, FRAME as isize - 1) as usize;
                    w * f64::from(samples[index])
                })
                .sum();
        }
        let rate = rate * 2.;
        let max = ((rate / 25.).ceil() as usize).min(FRAME - 1);
        let min = (rate / 4500.).floor().max(2.) as usize;
        let window = 1024;
        let start = FRAME * 2 - max - window - 64;
        let mut cumulative = 0.;
        self.normalized[0] = 1.;
        for lag in 1..=max {
            let mut sum = 0.;
            for i in start..start + window {
                let d = self.interpolated[i] - self.interpolated[i + lag];
                sum += d * d;
            }
            cumulative += sum;
            self.normalized[lag] = if cumulative > 0. {
                sum * lag as f64 / cumulative
            } else {
                1.
            };
        }
        // Short periods can fall between samples. Refine local minima over
        // several periods rather than reporting a subharmonic just because an
        // integer-lag minimum misses the strict threshold.
        let vertex = |lag: usize| {
            let y0 = self.normalized[lag - 1];
            let y1 = self.normalized[lag];
            let y2 = self.normalized[lag + 1];
            let denominator = y0 - 2. * y1 + y2;
            let offset = if denominator > 1e-12 {
                (0.5 * (y0 - y2) / denominator).clamp(-0.5, 0.5)
            } else {
                0.
            };
            (
                lag as f64 + offset,
                (y1 - 0.25 * (y0 - y2) * offset).max(0.),
            )
        };
        for lag in min..max {
            if self.normalized[lag] > self.normalized[lag - 1]
                || self.normalized[lag] > self.normalized[lag + 1]
                || self.normalized[lag] > 0.45
            {
                continue;
            }
            let (period, depth) = vertex(lag);
            if depth > 0.35 {
                continue;
            }
            let mut refined = period;
            let mut confidence = 1. - depth;
            for multiple in 2_usize..=8 {
                let expected = refined * multiple as f64;
                let radius = ((period * 0.4).floor() as usize).clamp(1, 4);
                let center = expected.round() as usize;
                if center + radius + 1 >= max {
                    break;
                }
                let start = center.saturating_sub(radius).max(2);
                if let Some(best) = (start..=center + radius)
                    .filter(|&i| {
                        self.normalized[i] <= self.normalized[i - 1]
                            && self.normalized[i] <= self.normalized[i + 1]
                    })
                    .min_by(|&a, &b| {
                        (a as f64 - expected)
                            .abs()
                            .total_cmp(&(b as f64 - expected).abs())
                    })
                {
                    let (long_period, long_depth) = vertex(best);
                    if long_depth < 0.12 {
                        refined = long_period / multiple as f64;
                        // Even multiples alone can corroborate a false octave
                        // when the second harmonic dominates the fundamental.
                        if multiple % 2 == 1 {
                            confidence = 1. - long_depth;
                        }
                    }
                }
            }
            if let Some(reading) = Reading::from_frequency(rate / refined, confidence) {
                return Some(reading);
            }
        }
        None
    }
}
/// Three consistent analysis frames suppress attack noise and octave flicker.
#[derive(Default)]
pub struct Stabilizer {
    candidate: Option<Reading>,
    frames: u8,
    misses: u8,
}
impl Stabilizer {
    pub fn update(&mut self, value: Option<Reading>) -> Option<Reading> {
        let Some(value) = value else {
            self.misses = self.misses.saturating_add(1);
            if self.misses >= 3 {
                self.candidate = None;
                self.frames = 0;
            }
            return if self.frames >= 3 {
                self.candidate
            } else {
                None
            };
        };
        self.misses = 0;
        if self.candidate.is_some_and(|previous| {
            previous.midi == value.midi && (previous.cents - value.cents).abs() < 35.
        }) {
            self.frames = self.frames.saturating_add(1);
        } else {
            self.frames = 1;
        }
        self.candidate = Some(value);
        (self.frames >= 3).then_some(value)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn wave(midi: u8, cents: f64, rate: f64, harmonics: bool) -> Vec<f32> {
        let frequency = 440. * 2_f64.powf((f64::from(midi) - 69.) / 12. + cents / 1200.);
        (0..FRAME)
            .map(|i| {
                let phase = std::f64::consts::TAU * frequency * i as f64 / rate;
                (0.2 * phase.sin()
                    + if harmonics {
                        (if frequency * 2. < rate * 0.5 {
                            0.3 * (phase * 2.).sin()
                        } else {
                            0.
                        }) + (if frequency * 3. < rate * 0.5 {
                            0.1 * (phase * 3.).sin()
                        } else {
                            0.
                        })
                    } else {
                        0.
                    }) as f32
            })
            .collect()
    }
    #[test]
    fn pure_and_piano_like_tones_keep_fundamentals_octaves_and_tuning() {
        let mut detector = Detector::default();
        for rate in [16000., 22050., 24000.] {
            for midi in 21..=108 {
                for harmonics in [false, true] {
                    let result = detector
                        .detect(&wave(midi, 0., rate, harmonics), rate)
                        .expect("tonal sample");
                    assert_eq!(result.midi, midi, "rate={rate}, harmonics={harmonics}");
                    assert!(result.cents.abs() < 18., "{midi}: {}", result.cents);
                }
            }
        }
        let result = detector
            .detect(&wave(69, -20., 24000., true), 24000.)
            .unwrap();
        assert_eq!(result.midi, 69);
        assert!((result.cents + 20.).abs() < 2.);
    }
    #[test]
    fn silence_dc_invalid_buffers_and_noise_do_not_become_notes() {
        let mut detector = Detector::default();
        assert!(detector.detect(&[0.; FRAME], 24000.).is_none());
        assert!(detector.detect(&[0.4; FRAME], 24000.).is_none());
        assert!(detector.detect(&[f32::NAN; FRAME], 24000.).is_none());
        assert!(detector.detect(&[0.; 4], 24000.).is_none());
        let mut state = 7_u32;
        let noise: Vec<_> = (0..FRAME)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state as f32 / u32::MAX as f32 - 0.5
            })
            .collect();
        assert!(detector.detect(&noise, 24000.).is_none());
    }
    #[test]
    fn stable_notes_require_confirmation_and_release_clears_a_held_note() {
        let mut stable = Stabilizer::default();
        let note = Reading::from_frequency(440., 1.);
        assert!(stable.update(note).is_none());
        assert!(stable.update(note).is_none());
        assert_eq!(stable.update(note).unwrap().midi, 69);
        for _ in 0..2 {
            assert_eq!(stable.update(None).unwrap().midi, 69);
        }
        assert!(stable.update(None).is_none());
        assert!(stable.update(note).is_none());
    }
}

/// Fixed FIR anti-aliasing before integer decimation. Constructed on the worker;
/// even high-rate inputs have bounded memory and no per-sample allocations.
pub(super) struct Downsample {
    factor: usize,
    used: usize,
    position: usize,
    history: [f32; 127],
    weights: [f32; 127],
}
impl Downsample {
    pub fn new(factor: usize) -> Self {
        let factor = factor.max(1);
        let mut weights = [0.; 127];
        let cutoff = 0.4 / factor as f64;
        for (i, weight) in weights.iter_mut().enumerate() {
            let x = i as f64 - 63.;
            let sinc = if x == 0. {
                2. * cutoff
            } else {
                (std::f64::consts::TAU * cutoff * x).sin() / (std::f64::consts::PI * x)
            };
            let window = 0.54 - 0.46 * (std::f64::consts::TAU * i as f64 / 126.).cos();
            *weight = (sinc * window) as f32;
        }
        let total = weights.iter().sum::<f32>();
        for weight in &mut weights {
            *weight /= total;
        }
        Self {
            factor,
            used: 0,
            position: 0,
            history: [0.; 127],
            weights,
        }
    }
    pub fn push(&mut self, sample: f32) -> Option<f32> {
        if self.factor == 1 {
            return Some(sample);
        }
        self.history[self.position] = sample;
        self.position = (self.position + 1) % 127;
        self.used += 1;
        if self.used < self.factor {
            return None;
        }
        self.used = 0;
        Some(
            self.weights
                .iter()
                .enumerate()
                .map(|(i, w)| w * self.history[(self.position + 126 - i) % 127])
                .sum(),
        )
    }
}
#[cfg(test)]
mod decimation_tests {
    use super::*;
    #[test]
    fn high_rate_keyboard_harmonics_are_filtered_before_decimation() {
        let mut detector = Detector::default();
        for rate in [44100_u32, 48000, 96000, 192000] {
            let factor = rate.div_ceil(24000) as usize;
            for midi in [36, 60, 69, 96, 102, 108] {
                let mut downsample = Downsample::new(factor);
                let mut samples = Vec::new();
                let frequency = 440. * 2_f64.powf((f64::from(midi) - 69.) / 12.);
                for i in 0..(FRAME + 256) * factor {
                    let phase = std::f64::consts::TAU * frequency * i as f64 / f64::from(rate);
                    let sample = (0.2 * phase.sin()
                        + 0.3 * (2. * phase).sin()
                        + 0.1 * (3. * phase).sin()) as f32;
                    if let Some(sample) = downsample.push(sample) {
                        samples.push(sample);
                    }
                }
                let reading = detector
                    .detect(&samples[256..], f64::from(rate) / factor as f64)
                    .expect("filtered tone");
                assert_eq!(reading.midi, midi, "rate {rate}");
                assert!(
                    reading.cents.abs() < 18.,
                    "rate {rate}, note {midi}, cents {}",
                    reading.cents
                );
            }
        }
    }
}
