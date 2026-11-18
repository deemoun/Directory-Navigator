//! One direction per stream: speakers never inspect input devices; microphones
//! never inspect output devices. Audio callbacks stay bounded and nonblocking.
use super::{model::Speed, protocol::Prepared};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::{
    collections::{BTreeMap, VecDeque},
    io,
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};
#[derive(Default)]
pub(super) struct Meters {
    pub sent: AtomicU32,
    pub received: AtomicU32,
    pub input: AtomicU32,
    pub output: AtomicU32,
    pub failed: AtomicBool,
    pub overflow: AtomicBool,
    pub dropped: AtomicU32,
    pub completed: AtomicU64,
    pub total: AtomicU64,
}
fn error(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}
pub(super) fn check(cancel: &AtomicBool, meters: &Meters) -> io::Result<()> {
    if cancel.load(Ordering::Relaxed) {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Acoustic operation stopped; no delivery confirmation is available",
        ));
    }
    if meters.failed.load(Ordering::Relaxed) {
        return Err(error(
            "Audio device was lost or stopped; retry Send/Receive",
        ));
    }
    Ok(())
}
pub(super) struct Codec {
    encoder: ggwave::Encoder,
    speed: Speed,
    rate: u32,
    lengths: BTreeMap<usize, usize>,
}
impl Codec {
    pub fn new(rate: u32, speed: Speed) -> io::Result<Self> {
        Ok(Self {
            encoder: ggwave::Encoder::new(ggwave::Parameters {
                sample_rate_out: rate as f32,
                ..Default::default()
            })
            .map_err(error)?,
            speed,
            rate,
            lengths: BTreeMap::new(),
        })
    }
    pub fn encode(&mut self, packet: &[u8]) -> io::Result<Vec<f32>> {
        let mut wave = self
            .encoder
            .encode(packet, self.speed.protocol(), 10)
            .map_err(error)?;
        wave.resize(wave.len() + self.rate as usize / 10, 0.);
        Ok(wave)
    }
    pub fn plan(
        &mut self,
        prepared: &Prepared,
        repeats: u8,
        cancel: &AtomicBool,
    ) -> io::Result<(u64, u64)> {
        let mut samples = 0_u64;
        let mut frames = 0;
        for frame in prepared.frames() {
            if cancel.load(Ordering::Relaxed) {
                return Err(io::ErrorKind::Interrupted.into());
            }
            let n = frame.len();
            let count = if let Some(v) = self.lengths.get(&n) {
                *v
            } else {
                let v = self.encode(&vec![0; n])?.len();
                self.lengths.insert(n, v);
                v
            };
            samples = samples
                .checked_add(count as u64)
                .ok_or_else(|| error("Acoustic duration overflow"))?;
            frames += 1;
        }
        Ok((
            samples * u64::from(repeats.max(1)),
            frames * u64::from(repeats),
        ))
    }
}
fn input<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    tx: mpsc::SyncSender<[f32; 1024]>,
    meters: Arc<Meters>,
) -> io::Result<cpal::Stream>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    let channels = usize::from(config.channels);
    let mut buffer = [0.; 1024];
    let mut used = 0;
    let failure = meters.clone();
    device
        .build_input_stream(
            config,
            move |samples: &[T], _| {
                let mut peak = 0f32;
                for frame in samples.chunks_exact(channels) {
                    let sample = frame
                        .iter()
                        .map(|s| <f32 as cpal::Sample>::from_sample(*s))
                        .sum::<f32>()
                        / channels as f32;
                    peak = peak.max(sample.abs());
                    buffer[used] = sample;
                    used += 1;
                    if used == 1024 {
                        if tx.try_send(buffer).is_err() {
                            meters.overflow.store(true, Ordering::Relaxed);
                        }
                        used = 0;
                    }
                }
                meters.input.store(peak.to_bits(), Ordering::Relaxed);
            },
            move |_| {
                failure.failed.store(true, Ordering::Relaxed);
            },
            None,
        )
        .map_err(error)
}
fn output<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    rx: mpsc::Receiver<(Vec<f32>, Arc<AtomicBool>)>,
    meters: Arc<Meters>,
) -> io::Result<cpal::Stream>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let channels = usize::from(config.channels);
    let mut wave = Vec::new();
    let mut index = 0;
    let mut completion: Option<Arc<AtomicBool>> = None;
    let failure = meters.clone();
    device
        .build_output_stream(
            config,
            move |samples: &mut [T], _| {
                let mut peak = 0f32;
                for frame in samples.chunks_mut(channels) {
                    if index >= wave.len() {
                        if let Some(done) = completion.take() {
                            done.store(true, Ordering::Release);
                        }
                        if let Ok((next, done)) = rx.try_recv() {
                            wave = next;
                            completion = Some(done);
                            index = 0;
                        }
                    }
                    let value = wave.get(index).copied().unwrap_or(0.);
                    if index < wave.len() {
                        index += 1;
                    }
                    peak = peak.max(value.abs());
                    for sample in frame {
                        *sample = T::from_sample(value);
                    }
                }
                meters.output.store(peak.to_bits(), Ordering::Relaxed);
            },
            move |_| {
                failure.failed.store(true, Ordering::Relaxed);
            },
            None,
        )
        .map_err(error)
}

pub(super) struct Speaker {
    pub devices: String,
    _stream: cpal::Stream,
    waves: mpsc::SyncSender<(Vec<f32>, Arc<AtomicBool>)>,
    meters: Arc<Meters>,
    cancel: Arc<AtomicBool>,
    codec: Codec,
}
impl Speaker {
    pub fn open(cancel: Arc<AtomicBool>, meters: Arc<Meters>, speed: Speed) -> io::Result<Self> {
        check(&cancel, &meters)?;
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| error("No default audio output"))?;
        let devices = device.description().map_err(error)?.name().to_owned();
        let config = device.default_output_config().map_err(error)?;
        if config.channels() == 0 {
            return Err(error("Default audio output has no channels"));
        }
        let (waves, rx) = mpsc::sync_channel(1);
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => output::<f32>(&device, &config.config(), rx, meters.clone()),
            cpal::SampleFormat::I16 => output::<i16>(&device, &config.config(), rx, meters.clone()),
            cpal::SampleFormat::U16 => output::<u16>(&device, &config.config(), rx, meters.clone()),
            _ => Err(error("Unsupported default output sample format")),
        }?;
        let codec = Codec::new(config.sample_rate(), speed)?;
        stream.play().map_err(error)?;
        Ok(Self {
            devices,
            _stream: stream,
            waves,
            meters,
            cancel,
            codec,
        })
    }
    pub fn send(&mut self, packet: &[u8]) -> io::Result<()> {
        check(&self.cancel, &self.meters)?;
        let wave = self.codec.encode(packet)?;
        let seconds = wave.len() as f64 / f64::from(self.codec.rate);
        let completion = Arc::new(AtomicBool::new(false));
        self.waves
            .try_send((wave, completion.clone()))
            .map_err(error)?;
        let until = Instant::now() + Duration::from_secs_f64(seconds + 10.);
        while !completion.load(Ordering::Acquire) {
            check(&self.cancel, &self.meters)?;
            if Instant::now() >= until {
                return Err(error("Audio output stalled"));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        self.meters.sent.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}
pub(super) struct Microphone {
    pub devices: String,
    _stream: cpal::Stream,
    samples: mpsc::Receiver<[f32; 1024]>,
    meters: Arc<Meters>,
    cancel: Arc<AtomicBool>,
    decoder: ggwave::Decoder,
    rate: u32,
    ready: VecDeque<Vec<u8>>,
}
fn decoder(rate: u32) -> io::Result<ggwave::Decoder> {
    ggwave::Decoder::with_protocols(
        ggwave::Parameters {
            sample_rate_inp: rate as f32,
            ..Default::default()
        },
        &[
            ggwave::Protocol::AudibleNormal,
            ggwave::Protocol::AudibleFast,
            ggwave::Protocol::AudibleFastest,
        ],
    )
    .map_err(error)
}
impl Microphone {
    pub fn open(cancel: Arc<AtomicBool>, meters: Arc<Meters>) -> io::Result<Self> {
        check(&cancel, &meters)?;
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| error("No default microphone"))?;
        let devices = device.description().map_err(error)?.name().to_owned();
        let config = device.default_input_config().map_err(error)?;
        if config.channels() == 0 {
            return Err(error("Default microphone has no channels"));
        }
        let (tx, samples) = mpsc::sync_channel(32);
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => input::<f32>(&device, &config.config(), tx, meters.clone()),
            cpal::SampleFormat::I16 => input::<i16>(&device, &config.config(), tx, meters.clone()),
            cpal::SampleFormat::U16 => input::<u16>(&device, &config.config(), tx, meters.clone()),
            _ => Err(error("Unsupported default microphone sample format")),
        }?;
        let rate = config.sample_rate();
        let decoder = decoder(rate)?;
        stream.play().map_err(error)?;
        Ok(Self {
            devices,
            _stream: stream,
            samples,
            meters,
            cancel,
            decoder,
            rate,
            ready: VecDeque::new(),
        })
    }
    pub fn receive(&mut self, wait: Duration) -> io::Result<Option<Vec<u8>>> {
        check(&self.cancel, &self.meters)?;
        if self.meters.overflow.swap(false, Ordering::Relaxed) {
            self.meters.dropped.fetch_add(1, Ordering::Relaxed);
            self.decoder = decoder(self.rate)?;
            for _ in 0..32 {
                if self.samples.try_recv().is_err() {
                    break;
                }
            }
        }
        if let Some(p) = self.ready.pop_front() {
            return Ok(Some(p));
        }
        match self.samples.recv_timeout(wait) {
            Ok(samples) => self.decode(&samples)?,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(e) => return Err(error(e)),
        }
        for _ in 0..31 {
            match self.samples.try_recv() {
                Ok(samples) => self.decode(&samples)?,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(e) => return Err(error(e)),
            }
        }
        Ok(self.ready.pop_front())
    }
    fn decode(&mut self, samples: &[f32]) -> io::Result<()> {
        if let Some(p) = self.decoder.push(samples) {
            if p.starts_with(b"DNA2") {
                if self.ready.len() == 8 {
                    return Err(error("Acoustic packet queue limit"));
                }
                self.meters.received.fetch_add(1, Ordering::Relaxed);
                self.ready.push_back(p);
            }
        }
        Ok(())
    }
}
