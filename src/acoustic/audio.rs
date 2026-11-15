//! Streams live exclusively on the worker. Callbacks never block or access DN state.
use super::protocol::Radio;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::{
    collections::VecDeque,
    io,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};
pub struct Meters {
    pub sent: AtomicU32,
    pub received: AtomicU32,
    pub input: AtomicU32,
    pub output: AtomicU32,
    pub failed: AtomicBool,
    pub overflow: AtomicBool,
}
impl Default for Meters {
    fn default() -> Self {
        Self {
            sent: AtomicU32::new(0),
            received: AtomicU32::new(0),
            input: AtomicU32::new(0),
            output: AtomicU32::new(0),
            failed: AtomicBool::new(false),
            overflow: AtomicBool::new(false),
        }
    }
}
fn error(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}
pub struct Audio {
    pub devices: String,
    _input: cpal::Stream,
    _output: cpal::Stream,
    samples: mpsc::Receiver<[f32; 1024]>,
    waves: mpsc::SyncSender<(Vec<f32>, Arc<AtomicBool>)>,
    meters: Arc<Meters>,
    cancel: Arc<AtomicBool>,
    encoder: ggwave::Encoder,
    decoder: ggwave::Decoder,
    ready: VecDeque<Vec<u8>>,
    role: bool,
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
impl Audio {
    pub fn open(cancel: Arc<AtomicBool>, meters: Arc<Meters>, role: bool) -> io::Result<Self> {
        let host = cpal::default_host();
        let input_device = host.default_input_device().ok_or_else(|| {
            error("No default microphone; both sides need audio input for acknowledgements")
        })?;
        let output_device = host
            .default_output_device()
            .ok_or_else(|| error("No default audio output"))?;
        let devices = format!(
            "{} → {}",
            input_device.description().map_err(error)?.name(),
            output_device.description().map_err(error)?.name()
        );
        let ic = input_device.default_input_config().map_err(error)?;
        let oc = output_device.default_output_config().map_err(error)?;
        if ic.channels() == 0 || oc.channels() == 0 {
            return Err(error("Default audio device has no channels"));
        }
        let (tx, samples) = mpsc::sync_channel(32);
        let (waves, rx) = mpsc::sync_channel(1);
        let input = match ic.sample_format() {
            cpal::SampleFormat::F32 => {
                input::<f32>(&input_device, &ic.config(), tx, meters.clone())
            }
            cpal::SampleFormat::I16 => {
                input::<i16>(&input_device, &ic.config(), tx, meters.clone())
            }
            cpal::SampleFormat::U16 => {
                input::<u16>(&input_device, &ic.config(), tx, meters.clone())
            }
            _ => Err(error("Unsupported default microphone sample format")),
        }?;
        let output = match oc.sample_format() {
            cpal::SampleFormat::F32 => {
                output::<f32>(&output_device, &oc.config(), rx, meters.clone())
            }
            cpal::SampleFormat::I16 => {
                output::<i16>(&output_device, &oc.config(), rx, meters.clone())
            }
            cpal::SampleFormat::U16 => {
                output::<u16>(&output_device, &oc.config(), rx, meters.clone())
            }
            _ => Err(error("Unsupported default output sample format")),
        }?;
        let encoder = ggwave::Encoder::new(ggwave::Parameters {
            sample_rate_out: oc.sample_rate() as f32,
            ..Default::default()
        })
        .map_err(error)?;
        let decoder = ggwave::Decoder::with_protocols(
            ggwave::Parameters {
                sample_rate_inp: ic.sample_rate() as f32,
                ..Default::default()
            },
            &[ggwave::Protocol::AudibleNormal],
        )
        .map_err(error)?;
        input.play().map_err(error)?;
        output.play().map_err(error)?;
        Ok(Self {
            devices,
            _input: input,
            _output: output,
            samples,
            waves,
            meters,
            cancel,
            encoder,
            decoder,
            ready: VecDeque::new(),
            role,
        })
    }
    fn check(&self) -> io::Result<()> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Acoustic session stopped",
            ));
        }
        if self.meters.failed.load(Ordering::Relaxed) {
            return Err(error(
                "Audio device was lost or stopped; retry Send/Receive",
            ));
        }
        if self.meters.overflow.swap(false, Ordering::Relaxed) {
            return Err(error(
                "Audio input overrun; stop other audio processing and retry",
            ));
        }
        Ok(())
    }
    fn decode(&mut self, samples: &[f32]) -> io::Result<()> {
        self.check()?;
        if let Some(packet) = self.decoder.push(samples) {
            if packet.len() >= 6 && &packet[..4] == b"DNAC" && (packet[5] & 1 != 0) != self.role {
                if self.ready.len() >= 8 {
                    return Err(error("Acoustic packet queue limit"));
                }
                self.meters.received.fetch_add(1, Ordering::Relaxed);
                self.ready.push_back(packet);
            }
        }
        Ok(())
    }
    fn pump(&mut self, wait: Duration) -> io::Result<()> {
        self.check()?;
        match self.samples.recv_timeout(wait) {
            Ok(samples) => self.decode(&samples)?,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(e) => return Err(error(e)),
        }
        // A conflict prompt polls at 20 ms; 96 kHz input produces buffers faster.
        // Drain its bounded backlog rather than falling behind while awaiting a user.
        for _ in 0..31 {
            match self.samples.try_recv() {
                Ok(samples) => self.decode(&samples)?,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(e) => return Err(error(e)),
            }
        }
        Ok(())
    }
}
impl Radio for Audio {
    fn send(&mut self, packet: &[u8]) -> io::Result<()> {
        self.check()?;
        let mut wave = self
            .encoder
            .encode(packet, ggwave::Protocol::AudibleNormal, 10)
            .map_err(error)?;
        wave.resize(wave.len() + 4800, 0.);
        self.meters.sent.fetch_add(1, Ordering::Relaxed);
        let completion = Arc::new(AtomicBool::new(false));
        self.waves
            .try_send((wave, completion.clone()))
            .map_err(error)?;
        let until = Instant::now() + Duration::from_secs(25);
        while !completion.load(Ordering::Acquire) {
            self.pump(Duration::from_millis(20))?;
            if Instant::now() >= until {
                return Err(error("Audio output stalled"));
            }
        }
        Ok(())
    }
    fn receive(&mut self, wait: Duration) -> io::Result<Option<Vec<u8>>> {
        self.check()?;
        if let Some(p) = self.ready.pop_front() {
            return Ok(Some(p));
        }
        self.pump(wait)?;
        Ok(self.ready.pop_front())
    }
}
