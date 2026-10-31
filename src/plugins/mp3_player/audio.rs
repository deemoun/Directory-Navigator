//! A single, lazily started worker owns the device and decoder. The mailbox
//! coalesces commands; neither a slow file nor seeking can block terminal input.
use super::*;

#[derive(Clone, Default)]
pub(super) struct Request {
    pub generation: u64,
    pub path: Option<PathBuf>,
    pub paused: bool,
    pub volume: u8,
    pub seek: Option<(u64, Duration)>,
}
#[derive(Clone, Default)]
pub(super) struct Snapshot {
    pub generation: u64,
    pub position: Duration,
    pub duration: Option<Duration>,
    pub ready: bool,
    pub seek_serial: Option<u64>,
    pub finished: bool,
    pub error: Option<String>,
}
pub(super) trait Engine {
    fn submit(&mut self, request: Request);
    fn snapshot(&self) -> Snapshot;
}
pub(super) fn engine() -> Box<dyn Engine> {
    Box::<System>::default()
}

#[cfg(not(target_os = "macos"))]
#[derive(Default)]
struct System;
#[cfg(not(target_os = "macos"))]
impl Engine for System {
    fn submit(&mut self, request: Request) {
        let _ = (request.path, request.paused);
    }
    fn snapshot(&self) -> Snapshot {
        Snapshot::default()
    }
}

#[cfg(target_os = "macos")]
use std::sync::{Arc, Condvar, Mutex};
#[cfg(target_os = "macos")]
#[derive(Default)]
struct Mailbox {
    request: Request,
    snapshot: Snapshot,
    closed: bool,
    device_error: Option<(u64, String)>,
    // Also lets Stop/close silence output immediately while the worker is busy.
    player: Option<Arc<rodio::Player>>,
}
#[cfg(target_os = "macos")]
#[derive(Default)]
struct System {
    shared: Option<Arc<(Mutex<Mailbox>, Condvar)>>,
}
#[cfg(target_os = "macos")]
impl Engine for System {
    fn submit(&mut self, request: Request) {
        if self
            .shared
            .as_ref()
            .is_some_and(|s| s.0.lock().unwrap().closed)
        {
            self.shared = None;
        }
        if self.shared.is_none() && request.path.is_some() {
            let shared = Arc::new((Mutex::new(Mailbox::default()), Condvar::new()));
            let worker = shared.clone();
            match std::thread::Builder::new()
                .name("dn-mp3".into())
                .spawn(move || run(worker))
            {
                Ok(_) => self.shared = Some(shared),
                Err(e) => {
                    let mut state = shared.0.lock().unwrap();
                    state.closed = true;
                    state.snapshot = Snapshot {
                        generation: request.generation,
                        error: Some(e.to_string()),
                        ..Snapshot::default()
                    };
                    drop(state);
                    self.shared = Some(shared);
                }
            }
        }
        if let Some(shared) = &self.shared {
            let mut state = shared.0.lock().unwrap();
            if state.request.generation != request.generation {
                if let Some(player) = state.player.take() {
                    player.stop();
                }
            }
            state.request = request;
            shared.1.notify_one();
        }
    }
    fn snapshot(&self) -> Snapshot {
        self.shared
            .as_ref()
            .map(|s| s.0.lock().unwrap().snapshot.clone())
            .unwrap_or_default()
    }
}
#[cfg(target_os = "macos")]
impl Drop for System {
    fn drop(&mut self) {
        if let Some(shared) = &self.shared {
            let mut state = shared.0.lock().unwrap();
            state.closed = true;
            if let Some(player) = state.player.take() {
                player.stop();
            }
            shared.1.notify_one();
        }
    }
}
#[cfg(target_os = "macos")]
fn run(shared: Arc<(Mutex<Mailbox>, Condvar)>) {
    use rodio::Source;
    let mut generation = u64::MAX;
    let mut output = None;
    let mut player: Option<Arc<rodio::Player>> = None;
    let mut snapshot = Snapshot::default();
    let mut last_seek = None;
    loop {
        let request = {
            let state = shared.0.lock().unwrap();
            if state.closed {
                break;
            }
            state.request.clone()
        };
        if generation != request.generation {
            player = None;
            output = None;
            generation = request.generation;
            last_seek = None;
            snapshot = Snapshot {
                generation,
                ..Snapshot::default()
            };
            if let Some(path) = &request.path {
                let loaded = (|| -> Result<_, String> {
                    let file = crate::filesystem::open_regular(path).map_err(|e| e.to_string())?;
                    let source = rodio::Decoder::try_from(file).map_err(|e| e.to_string())?;
                    let duration = source.total_duration();
                    let weak = Arc::downgrade(&shared);
                    let mut device = rodio::DeviceSinkBuilder::from_default_device()
                        .map_err(|e| e.to_string())?
                        .with_error_callback(move |error| {
                            if let Some(shared) = weak.upgrade() {
                                shared.0.lock().unwrap().device_error =
                                    Some((generation, error.to_string()));
                                shared.1.notify_one();
                            }
                        })
                        .open_sink_or_fallback()
                        .map_err(|e| e.to_string())?;
                    device.log_on_drop(false);
                    let sink = Arc::new(rodio::Player::connect_new(device.mixer()));
                    sink.pause();
                    sink.set_volume(f32::from(request.volume) / 100.0);
                    // Commit under the mailbox lock: a retired generation must
                    // never start sound, even if loading finished after close.
                    let mut state = shared.0.lock().unwrap();
                    if state.closed || state.request.generation != generation {
                        return Ok(None);
                    }
                    sink.append(source);
                    state.player = Some(sink.clone());
                    Ok(Some((device, sink, duration)))
                })();
                match loaded {
                    Ok(Some((device, sink, duration))) => {
                        output = Some(device);
                        player = Some(sink);
                        snapshot.duration = duration;
                        snapshot.ready = true;
                    }
                    Ok(None) => continue,
                    Err(e) => snapshot.error = Some(e),
                }
            }
        }
        if let Some(sink) = &player {
            sink.set_volume(f32::from(request.volume) / 100.0);
            if request.paused {
                sink.pause();
            } else {
                sink.play();
            }
            if let Some((serial, position)) = request.seek {
                if last_seek != Some(serial) {
                    if let Err(e) = sink.try_seek(position) {
                        snapshot.error = Some(e.to_string());
                        sink.stop();
                    }
                    last_seek = Some(serial);
                    snapshot.seek_serial = Some(serial);
                }
            }
            snapshot.position = sink.get_pos();
            snapshot.finished = sink.empty();
        }
        let mut state = shared.0.lock().unwrap();
        if let Some((tag, error)) = state.device_error.take() {
            if tag == generation {
                snapshot.error = Some(error);
                if let Some(sink) = &player {
                    sink.stop();
                }
            }
        }
        if state.request.generation == generation {
            state.snapshot = snapshot.clone();
        }
        if state.closed {
            break;
        }
        // Do not miss a command arriving while the device/file was opening.
        if state.request.generation != generation {
            continue;
        }
        if request.path.is_none() {
            let _guard = shared.1.wait(state).unwrap();
        } else {
            let _guard = shared
                .1
                .wait_timeout(state, Duration::from_millis(50))
                .unwrap();
        }
    }
    drop(player);
    drop(output);
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use rodio::Source;
    #[test]
    fn synthetic_mp3_decodes_without_an_audio_device() {
        let mut decoder = rodio::Decoder::try_from(std::io::Cursor::new(
            include_bytes!("../../../tests/fixtures/mp3/tone.mp3").as_slice(),
        ))
        .unwrap();
        assert_eq!(decoder.channels().get(), 1);
        assert!(decoder.total_duration().is_some_and(|d| d.as_secs() >= 11));
        assert!(decoder.any(|sample| sample.abs() > 0.0001));
    }
}
