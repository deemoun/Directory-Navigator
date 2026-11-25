//! Optional, session-only note learning. Host owns microphone and worker lifetime.
use crate::{
    app::App,
    plugins::{Binding, Owner, NOTE_TRAINER},
};
use std::{
    io,
    sync::{Arc, Mutex},
};
#[cfg(all(
    feature = "note-trainer",
    any(target_os = "macos", target_os = "linux", target_os = "windows")
))]
mod audio;
#[cfg(feature = "note-trainer")]
mod pitch;
pub(crate) const fn available() -> bool {
    cfg!(all(
        feature = "note-trainer",
        any(
            target_os = "macos",
            target_os = "linux",
            target_os = "windows"
        )
    ))
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Reading {
    pub midi: u8,
    pub frequency: f64,
    pub cents: f64,
    pub confidence: f64,
}
impl Reading {
    #[cfg(any(test, feature = "note-trainer"))]
    fn from_frequency(frequency: f64, confidence: f64) -> Option<Self> {
        if !frequency.is_finite() || frequency <= 0. || !confidence.is_finite() || confidence < 0.88
        {
            return None;
        }
        let exact = 69. + 12. * (frequency / 440.).log2();
        let midi = exact.round();
        if !(21. ..=108.).contains(&midi) {
            return None;
        }
        Some(Self {
            midi: midi as u8,
            frequency,
            cents: (exact - midi) * 100.,
            confidence: confidence.clamp(0., 1.),
        })
    }
}
pub(crate) fn name(midi: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    format!(
        "{}{}",
        NAMES[usize::from(midi % 12)],
        i16::from(midi) / 12 - 1
    )
}
#[derive(Clone, Default, PartialEq)]
pub(crate) struct Snapshot {
    pub generation: u64,
    pub serial: u64,
    pub listening: bool,
    pub ready: bool,
    pub reading: Option<Reading>,
    pub level: u8,
    pub clipped: bool,
    pub device: String,
    pub notice: String,
    pub cause: String,
}
#[derive(Clone, Default)]
struct Update {
    reading: Option<Reading>,
    level: u8,
    clipped: bool,
    device: String,
}
#[derive(Clone, Copy)]
pub(crate) enum Action {
    Listen,
    Stop,
}
struct Service {
    binding: Binding,
    worker: crate::workers::Worker<()>,
    latest: Arc<Mutex<Option<Update>>>,
}
#[derive(Default)]
pub(crate) struct State {
    pub view: Snapshot,
    service: Option<Service>,
    #[cfg(any(test, feature = "note-trainer"))]
    requests: crate::requests::RequestScope,
}
fn alive(app: &App, binding: Binding) -> bool {
    binding.plugin == NOTE_TRAINER
        && app.config.plugin_enabled(NOTE_TRAINER)
        && app.desktop.plugins.binding(NOTE_TRAINER) == Some(binding)
        && matches!(binding.owner, Owner::Document(id) if app.desktop.contains(id))
}
pub(crate) fn stop(app: &mut App) {
    app.notes.service = None; // Worker drop cancels; the supervisor retains cleanup.
    app.notes.view.generation = app.notes.view.generation.wrapping_add(1);
    app.notes.view.listening = false;
    app.notes.view.ready = false;
    app.notes.view.reading = None;
    app.notes.view.level = 0;
    app.notes.view.clipped = false;
    app.notes.view.device.clear();
    app.notes.view.notice = "Microphone stopped".into();
}
pub(crate) fn opened(app: &mut App) {
    stop(app);
    #[cfg(any(test, feature = "note-trainer"))]
    {
        app.notes.requests = Default::default();
    }
    app.notes.view.notice.clear();
    app.notes.view.cause.clear();
}
pub(crate) fn action(app: &mut App, binding: Binding, generation: u64, action: Action) {
    if !alive(app, binding) || generation != app.notes.view.generation {
        return;
    }
    if matches!(action, Action::Stop) {
        stop(app);
        return;
    }
    if !available() {
        app.notes.view.notice = UNAVAILABLE.into();
        return;
    }
    if app.notes.view.listening {
        return;
    }
    if crate::acoustic::active(app) {
        app.notes.view.notice = "Stop Acoustic transfers before using the microphone.".into();
    } else {
        begin_listening(app, binding);
    }
}
fn begin_listening(app: &mut App, binding: Binding) {
    #[cfg(all(
        feature = "note-trainer",
        any(target_os = "macos", target_os = "linux", target_os = "windows")
    ))]
    if let Err(error) = start(app, binding, audio::run) {
        app.notes.view.notice =
            "Microphone could not start; check system input and privacy permissions.".into();
        app.notes.view.cause = bounded(&error.to_string());
    }
    #[cfg(not(all(
        feature = "note-trainer",
        any(target_os = "macos", target_os = "linux", target_os = "windows")
    )))]
    let _ = (app, binding);
}
#[cfg(any(
    test,
    all(
        feature = "note-trainer",
        any(target_os = "macos", target_os = "linux", target_os = "windows")
    )
))]
fn start(
    app: &mut App,
    binding: Binding,
    run: impl FnOnce(crate::task::Cancellation, Arc<Mutex<Option<Update>>>) -> io::Result<()>
        + Send
        + 'static,
) -> io::Result<()> {
    let Owner::Document(owner) = binding.owner else {
        return Err(io::Error::other("Note Trainer requires a document owner"));
    };
    let latest = Arc::new(Mutex::new(None));
    let output = latest.clone();
    let worker = app.workers.spawn(
        "dn-note-trainer",
        &app.notes.requests,
        crate::requests::Owner::Document(owner),
        "notes",
        move |cancel| run(cancel, output),
    )?;
    app.notes.view = Snapshot {
        generation: app.notes.view.generation.wrapping_add(1),
        listening: true,
        notice: "Opening microphone…".into(),
        ..Default::default()
    };
    app.notes.service = Some(Service {
        binding,
        worker,
        latest,
    });
    Ok(())
}
fn bounded(text: &str) -> String {
    text.chars().take(2048).collect()
}
pub(crate) fn poll(app: &mut App) -> bool {
    let Some(service) = &app.notes.service else {
        return false;
    };
    if !alive(app, service.binding) || app.quit {
        stop(app);
        return true;
    }
    let (width, height) = app.utilities.size;
    if width != 0 && height != 0 && (width < 80 || height < 25) {
        stop(app);
        app.notes.view.notice =
            "Microphone stopped for a small terminal; resize and press Listen.".into();
        return true;
    }
    let mut changed = false;
    if let Some(update) = service
        .latest
        .try_lock()
        .ok()
        .and_then(|mut slot| slot.take())
    {
        app.notes.view.serial = app.notes.view.serial.wrapping_add(1);
        app.notes.view.reading = update.reading;
        app.notes.view.level = update.level;
        app.notes.view.clipped = update.clipped;
        app.notes.view.device = update.device;
        app.notes.view.ready = true;
        app.notes.view.notice = "Listening — play one note at a time".into();
        changed = true;
    }
    match service.worker.try_recv() {
        Err(std::sync::mpsc::TryRecvError::Empty) => changed,
        result => {
            let cause = match result {
                Ok(Err(error)) if error.kind() != io::ErrorKind::Interrupted => {
                    bounded(&error.to_string())
                }
                Err(_) => "Microphone worker ended unexpectedly".into(),
                _ => String::new(),
            };
            stop(app);
            if !cause.is_empty() {
                app.notes.view.notice =
                    "Microphone stopped; check input/privacy settings, then Listen to retry."
                        .into();
                app.notes.view.cause = cause;
            }
            true
        }
    }
}
pub(crate) const UNAVAILABLE: &str =
    "Note Trainer is not compiled; rebuild with --features note-trainer.";

#[cfg(test)]
mod tests;
