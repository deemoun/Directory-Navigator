//! Portable plugin controls; never accesses audio hardware.
use dn::{
    config::Config,
    menu::{self, Action},
    mouse::Target,
    plugins::{ActionId, NOTE_TRAINER},
    settings::SettingsKind,
};
use std::{fs, io};
mod support;
use crossterm::event::KeyCode;
use support::{click, draw, key, Fixture};
#[test]
fn note_plugin_defaults_off_saved_choices_and_settings_cancel_are_preserved() -> io::Result<()> {
    let f = Fixture::new("notes-preferences")?;
    let mut app = f.app()?;
    assert!(!app.config.plugin_enabled(NOTE_TRAINER));
    let mut config = Config::parse(b"config_version=2\n[plugins]\ndisabled=['future-plugin']\n")?;
    config.set_plugin_enabled(NOTE_TRAINER, true);
    let path = f.0.join("notes.conf");
    config.save(&path)?;
    assert!(Config::read(&path)?.plugin_enabled(NOTE_TRAINER));
    assert!(fs::read_to_string(&path)?.contains("future-plugin"));
    app.open_settings(SettingsKind::Plugins);
    // Before Cache directories and the two opening preferences.
    let index = app.modals.settings().unwrap().control_count() - 4;
    app.modals.settings_mut().unwrap().toggle(index);
    key(&mut app, KeyCode::Esc);
    assert!(!app.config.plugin_enabled(NOTE_TRAINER));
    Ok(())
}
#[test]
fn board_controls_never_open_microphone_and_close_restores_owner() -> io::Result<()> {
    let f = Fixture::new("notes-board")?;
    let mut app = f.app()?;
    app.config.set_plugin_enabled(NOTE_TRAINER, true);
    menu::execute(&mut app, Action::Plugin(NOTE_TRAINER));
    let owner = app.desktop.active().id;
    draw(&mut app, 80, 25)?;
    let listen = Target::Plugin(NOTE_TRAINER, ActionId(0));
    let compiled = cfg!(all(
        feature = "note-trainer",
        any(
            target_os = "macos",
            target_os = "linux",
            target_os = "windows"
        )
    ));
    assert_eq!(
        app.mouse.regions.iter().any(|(_, t)| *t == listen),
        compiled
    );
    click(&mut app, Target::Plugin(NOTE_TRAINER, ActionId(2)));
    draw(&mut app, 80, 25)?;
    click(&mut app, Target::Plugin(NOTE_TRAINER, ActionId(6)));
    draw(&mut app, 80, 25)?;
    click(&mut app, Target::Plugin(NOTE_TRAINER, ActionId(160)));
    draw(&mut app, 80, 25)?;
    key(&mut app, KeyCode::F(1));
    assert!(!app.dialogs().is_empty());
    key(&mut app, KeyCode::Esc);
    draw(&mut app, 80, 25)?;
    click(&mut app, Target::Plugin(NOTE_TRAINER, ActionId(8)));
    assert!(!app.desktop.contains(owner));
    assert_eq!(app.desktop.active().id, dn::desktop::WindowId::MANAGER);
    Ok(())
}
