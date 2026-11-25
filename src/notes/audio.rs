//! CPAL is confined to the optional worker. PCM callbacks only enqueue fixed blocks.
use super::{
    pitch::{Detector, Downsample, Stabilizer, FRAME},
    Update,
};
use crate::task::Cancellation;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::{
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};
fn error(value: impl std::fmt::Display) -> io::Error {
    io::Error::other(value.to_string())
}
fn input<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    tx: mpsc::SyncSender<[f32; 512]>,
    lost: mpsc::SyncSender<String>,
    overflow: Arc<AtomicBool>,
) -> io::Result<cpal::Stream>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    let channels = usize::from(config.channels);
    let mut block = [0.; 512];
    let mut used = 0;
    device
        .build_input_stream(
            config,
            move |samples: &[T], _| {
                // A hostile/broken backend cannot make a callback process unbounded data.
                if samples.len() / channels > 16_384 {
                    overflow.store(true, Ordering::Relaxed);
                }
                for frame in samples.chunks_exact(channels).take(16_384) {
                    let sample = frame
                        .iter()
                        .map(|value| <f32 as cpal::Sample>::from_sample(*value))
                        .sum::<f32>()
                        / channels as f32;
                    block[used] = if sample.is_finite() {
                        sample.clamp(-1., 1.)
                    } else {
                        0.
                    };
                    used += 1;
                    if used == block.len() {
                        if tx.try_send(block).is_err() {
                            overflow.store(true, Ordering::Relaxed);
                        }
                        used = 0;
                    }
                }
            },
            move |e| {
                let _ = lost.try_send(super::bounded(&e.to_string()));
            },
            None,
        )
        .map_err(error)
}
pub(super) fn run(cancel: Cancellation, latest: Arc<Mutex<Option<Update>>>) -> io::Result<()> {
    cancel.check()?;
    let device = cpal::default_host()
        .default_input_device()
        .ok_or_else(|| error("No default microphone; choose a system input device"))?;
    let label = device
        .description()
        .map(|d| d.name().chars().take(160).collect::<String>())
        .unwrap_or_else(|_| "Default microphone".into());
    let config = device.default_input_config().map_err(error)?;
    let rate = config.sample_rate();
    if config.channels() == 0 || config.channels() > 32 || !(16000..=192000).contains(&rate) {
        return Err(error("Unsupported default microphone configuration; select 16–192 kHz input with 1–32 channels"));
    }
    let (tx, receive) = mpsc::sync_channel(8);
    let (lost_tx, lost_rx) = mpsc::sync_channel(1);
    let overflow = Arc::new(AtomicBool::new(false));
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => {
            input::<f32>(&device, &config.config(), tx, lost_tx, overflow.clone())
        }
        cpal::SampleFormat::I16 => {
            input::<i16>(&device, &config.config(), tx, lost_tx, overflow.clone())
        }
        cpal::SampleFormat::U16 => {
            input::<u16>(&device, &config.config(), tx, lost_tx, overflow.clone())
        }
        cpal::SampleFormat::I32 => {
            input::<i32>(&device, &config.config(), tx, lost_tx, overflow.clone())
        }
        _ => Err(error(
            "Unsupported microphone sample format; select a standard PCM input",
        )),
    }?;
    cancel.check()?;
    stream.play().map_err(error)?;
    let factor = rate.div_ceil(24000) as usize;
    let analysis_rate = f64::from(rate) / factor as f64;
    let mut samples = std::collections::VecDeque::with_capacity(FRAME);
    let mut detector = Detector::default();
    let mut stable = Stabilizer::default();
    let mut downsample = Downsample::new(factor);
    let mut peak = 0_f32;
    let mut report = Instant::now();
    let mut last_input = Instant::now();
    loop {
        cancel.check()?;
        if let Ok(cause) = lost_rx.try_recv() {
            return Err(error(cause));
        }
        if overflow.swap(false, Ordering::Relaxed) {
            for _ in 0..8 {
                let _ = receive.try_recv();
            }
            samples.clear();
            stable = Stabilizer::default();
            downsample = Downsample::new(factor);
        }
        match receive.recv_timeout(Duration::from_millis(20)) {
            Ok(block) => {
                last_input = Instant::now();
                for sample in block {
                    peak = peak.max(sample.abs());
                    if let Some(sample) = downsample.push(sample) {
                        if samples.len() == FRAME {
                            samples.pop_front();
                        }
                        samples.push_back(sample);
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout)
                if last_input.elapsed() < Duration::from_secs(3) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {
                return Err(error(
                    "Microphone provided no samples for three seconds; check the input device",
                ))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(error("Microphone input disconnected"))
            }
        }
        if report.elapsed() >= Duration::from_millis(50) {
            cancel.check()?;
            let reading =
                if samples.len() == FRAME && last_input.elapsed() < Duration::from_millis(100) {
                    detector.detect(samples.make_contiguous(), analysis_rate)
                } else {
                    None
                };
            let update = Update {
                reading: stable.update(reading),
                level: (peak * 100.).clamp(0., 100.) as u8,
                clipped: peak >= 0.98,
                device: label.clone(),
            };
            if let Ok(mut slot) = latest.try_lock() {
                *slot = Some(update);
            }
            peak = 0.;
            report = Instant::now();
        }
    }
}
