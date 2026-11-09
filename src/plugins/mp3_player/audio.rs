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

#[cfg(not(all(
    feature = "audio",
    any(target_os = "macos", target_os = "linux", target_os = "windows")
)))]
#[derive(Default)]
struct System;
#[cfg(not(all(
    feature = "audio",
    any(target_os = "macos", target_os = "linux", target_os = "windows")
)))]
impl Engine for System {
    fn submit(&mut self, request: Request) {
        let _ = (request.path, request.paused);
    }
    fn snapshot(&self) -> Snapshot {
        Snapshot::default()
    }
}

#[cfg(all(
    feature = "audio",
    any(target_os = "macos", target_os = "linux", target_os = "windows")
))]
use std::sync::{Arc, Condvar, Mutex};
#[cfg(all(
    feature = "audio",
    any(target_os = "macos", target_os = "linux", target_os = "windows")
))]
#[derive(Default)]
struct Mailbox {
    request: Request,
    snapshot: Snapshot,
    closed: bool,
    device_error: Option<(u64, String)>,
    // Also lets Stop/close silence output immediately while the worker is busy.
    player: Option<Arc<rodio::Player>>,
}
#[cfg(all(
    feature = "audio",
    any(target_os = "macos", target_os = "linux", target_os = "windows")
))]
impl Mailbox {
    fn fail_device(&mut self, generation: u64, message: String) -> bool {
        if self.closed || self.request.generation != generation {
            return false;
        }
        self.device_error = Some((generation, message));
        true
    }
}
#[cfg(all(
    feature = "audio",
    any(target_os = "macos", target_os = "linux", target_os = "windows")
))]
fn describe_error(error: &dyn std::error::Error) -> String {
    let mut message = error.to_string();
    let mut cause = error.source();
    for _ in 0..8 {
        let Some(error) = cause else {
            break;
        };
        message.push_str(": ");
        message.push_str(&error.to_string());
        cause = error.source();
    }
    message
}
#[cfg(all(
    feature = "audio",
    any(target_os = "macos", target_os = "linux", target_os = "windows")
))]
#[derive(Default)]
struct System {
    shared: Option<Arc<(Mutex<Mailbox>, Condvar)>>,
}
#[cfg(all(
    feature = "audio",
    any(target_os = "macos", target_os = "linux", target_os = "windows")
))]
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
#[cfg(all(
    feature = "audio",
    any(target_os = "macos", target_os = "linux", target_os = "windows")
))]
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
#[cfg(all(
    feature = "audio",
    any(target_os = "macos", target_os = "linux", target_os = "windows")
))]
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
                        .map_err(|e| describe_error(&e))?
                        .with_error_callback(move |error| {
                            if let Some(shared) = weak.upgrade() {
                                if shared
                                    .0
                                    .lock()
                                    .unwrap()
                                    .fail_device(generation, describe_error(&error))
                                {
                                    shared.1.notify_one();
                                }
                            }
                        })
                        .open_sink_or_fallback()
                        .map_err(|e| describe_error(&e))?;
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

#[cfg(all(
    test,
    all(
        feature = "audio",
        any(target_os = "macos", target_os = "linux", target_os = "windows")
    )
))]
mod tests {
    use super::*;
    use rodio::Source;
    #[test]
    fn retired_device_callbacks_cannot_replace_a_live_error_or_touch_a_closed_engine() {
        let mut mailbox = Mailbox::default();
        mailbox.request.generation = 2;
        assert!(mailbox.fail_device(2, "live device disconnected".into()));
        assert!(!mailbox.fail_device(1, "retired stream error".into()));
        assert_eq!(
            mailbox.device_error.as_ref().unwrap().1,
            "live device disconnected"
        );
        mailbox.closed = true;
        assert!(!mailbox.fail_device(2, "late callback".into()));
        assert_eq!(
            mailbox.device_error.as_ref().unwrap().1,
            "live device disconnected"
        );
    }
    #[test]
    fn device_diagnostics_include_the_underlying_access_failure() {
        #[derive(Debug)]
        struct DeviceFailure(std::io::Error);
        impl std::fmt::Display for DeviceFailure {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("Cannot open default output")
            }
        }
        impl std::error::Error for DeviceFailure {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                Some(&self.0)
            }
        }
        let failure = DeviceFailure(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "fixture denied",
        ));
        assert_eq!(
            describe_error(&failure),
            "Cannot open default output: fixture denied"
        );
    }
    #[test]
    fn opening_and_stopping_an_empty_engine_never_opens_an_audio_device() {
        let mut engine = System::default();
        for generation in 0..3 {
            engine.submit(Request {
                generation,
                ..Default::default()
            });
            assert!(engine.shared.is_none());
            assert!(!engine.snapshot().ready);
        }
    }
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
