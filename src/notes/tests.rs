use super::*;
use crate::{
    plugins::{self, Event},
    testing::{backend::TestBackend, Terminal},
    ui,
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
struct Temp(PathBuf);
impl Temp {
    fn new() -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "dn-notes-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p)?;
        Ok(Self(p))
    }
    fn app(&self) -> io::Result<App> {
        let mut app = App::new(self.0.clone())?;
        app.config.show_clock = false;
        app.config.set_plugin_enabled(NOTE_TRAINER, true);
        let end = Instant::now() + Duration::from_secs(5);
        while app.panels.iter().any(|p| p.loading) {
            app.poll();
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(1));
        }
        plugins::host::open(&mut app, NOTE_TRAINER);
        Ok(app)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn defaults_capability_settings_and_no_device_open_on_activation() -> io::Result<()> {
    let t = Temp::new()?;
    assert!(!crate::config::Config::default().plugin_enabled(NOTE_TRAINER));
    let mut app = t.app()?;
    assert!(!app.notes.view.listening);
    assert!(app.notes.service.is_none());
    let b = app.desktop.plugins.binding(NOTE_TRAINER).unwrap();
    if !available() {
        let generation = app.notes.view.generation;
        action(&mut app, b, generation, Action::Listen);
        assert!(!app.notes.view.listening);
        assert_eq!(app.notes.view.notice, UNAVAILABLE);
    }
    app.open_settings(crate::settings::SettingsKind::Plugins);
    let settings = app.modals.settings_mut().unwrap();
    let index = settings
        .plugins
        .iter()
        .position(|(id, _)| *id == NOTE_TRAINER)
        .unwrap();
    if !available() {
        settings.draft.set_plugin_enabled(NOTE_TRAINER, false);
        settings.toggle(index);
        assert!(!settings.draft.plugin_enabled(NOTE_TRAINER));
    }
    Ok(())
}
fn pump(app: &mut App, done: impl Fn(&App) -> bool) {
    let end = Instant::now() + Duration::from_secs(5);
    while !done(app) {
        app.poll();
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(1));
    }
}
#[test]
fn fake_capture_errors_retry_retirement_and_stale_results_never_steal_focus() -> io::Result<()> {
    let t = Temp::new()?;
    let mut app = t.app()?;
    let b = app.desktop.plugins.binding(NOTE_TRAINER).unwrap();
    start(&mut app, b, |_, _| {
        Err(io::Error::other("private mock: input denied"))
    })?;
    pump(&mut app, |a| !a.notes.view.listening);
    assert!(app.notes.view.cause.contains("input denied"));
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    start(&mut app, b, move |cancel, latest| {
        *latest.lock().unwrap() = Some(Update {
            device: "mock microphone".into(),
            reading: Reading::from_frequency(440., 1.),
            ..Default::default()
        });
        let _ = tx.try_send(());
        while !cancel.cancelled() {
            std::thread::sleep(Duration::from_millis(1));
        }
        Ok(())
    })?;
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    app.desktop.activate(crate::desktop::WindowId::MANAGER);
    pump(&mut app, |a| a.notes.view.ready);
    assert_eq!(app.desktop.active().id, crate::desktop::WindowId::MANAGER);
    assert_eq!(app.notes.view.reading.unwrap().midi, 69);
    let stale = app.notes.view.generation;
    stop(&mut app);
    action(&mut app, b, stale, Action::Listen);
    assert!(!app.notes.view.listening);
    start(&mut app, b, |cancel, _| {
        while !cancel.cancelled() {
            std::thread::sleep(Duration::from_millis(1));
        }
        Ok(())
    })?;
    app.config.set_plugin_enabled(NOTE_TRAINER, false);
    app.poll();
    assert!(app.notes.service.is_none());
    assert!(!app.notes.view.listening);
    // A fresh window must get a fresh request scope; retired replies cannot
    // overwrite it, even when the old worker finishes after reopening.
    app.config.set_plugin_enabled(NOTE_TRAINER, true);
    plugins::host::open(&mut app, NOTE_TRAINER);
    let fresh = app.desktop.plugins.binding(NOTE_TRAINER).unwrap();
    assert_ne!(fresh, b);
    start(&mut app, fresh, |_, latest| {
        *latest.lock().unwrap() = Some(Update {
            device: "new mock".into(),
            ..Default::default()
        });
        Ok(())
    })?;
    pump(&mut app, |a| !a.notes.view.listening);
    assert!(app.notes.view.cause.is_empty());
    Ok(())
}
#[test]
fn localized_board_input_help_resize_palette_and_frames_are_immutable() -> io::Result<()> {
    let t = Temp::new()?;
    let mut app = t.app()?;
    let b = app.desktop.plugins.binding(NOTE_TRAINER).unwrap();
    for language in [
        crate::localization::Language::English,
        crate::localization::Language::Russian,
    ] {
        app.config.language = language;
        app.config.palette = crate::palette::PaletteProfile::ClassicDn
            .with_slot(crate::palette::theme::Slot::DialogText, 0x2f);
        for (w, h) in [(80, 25), (110, 35), (45, 16), (80, 25)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h))?;
            terminal.draw(|f| ui::draw(f, &mut app))?;
            let previous = terminal.backend().buffer().clone();
            let hits = app.mouse.regions.clone();
            for _ in 0..3 {
                terminal.draw(|f| ui::draw(f, &mut app))?;
                assert_eq!(terminal.backend().buffer(), &previous);
                assert_eq!(app.mouse.regions, hits);
            }
            assert!(app
                .mouse
                .regions
                .iter()
                .all(|(r, _)| r.right() <= w && r.bottom() <= h));
        }
    }
    plugins::host::dispatch(&mut app, b, Event::Action(plugins::ActionId(2)));
    plugins::host::dispatch(&mut app, b, Event::Action(plugins::ActionId(160)));
    plugins::host::dispatch(&mut app, b, Event::Action(plugins::ActionId(7)));
    assert!(!app.dialogs().is_empty());
    Ok(())
}

#[test]
fn small_terminal_and_quit_cancel_capture_without_automatic_restart() -> io::Result<()> {
    let t = Temp::new()?;
    let mut app = t.app()?;
    let b = app.desktop.plugins.binding(NOTE_TRAINER).unwrap();
    start(&mut app, b, |cancel, _| {
        while !cancel.cancelled() {
            std::thread::sleep(Duration::from_millis(1));
        }
        Ok(())
    })?;
    let cancel = app
        .notes
        .service
        .as_ref()
        .unwrap()
        .worker
        .cancellation_flag();
    app.utilities.size = (45, 16);
    poll(&mut app);
    assert!(cancel.load(Ordering::Relaxed));
    assert!(!app.notes.view.listening);
    app.utilities.size = (80, 25);
    poll(&mut app);
    assert!(app.notes.service.is_none());
    start(&mut app, b, |cancel, _| {
        while !cancel.cancelled() {
            std::thread::sleep(Duration::from_millis(1));
        }
        Ok(())
    })?;
    app.quit = true;
    poll(&mut app);
    assert!(app.notes.service.is_none());
    Ok(())
}
