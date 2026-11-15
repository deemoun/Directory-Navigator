//! Experimental defaults and shared UI are portable and never open real audio hardware.
use crossterm::event::KeyCode as K;
use dn::{
    config::Config,
    localization::Language,
    menu::{self, Action},
    mouse::Target,
    palette::PaletteProfile,
    plugins::{ActionId, ACOUSTIC_TRANSFERS, DOS_LAUNCHER},
    settings::SettingsKind,
};
use std::{fs, io};
mod support;
use support::{click, draw, key, Fixture};
#[test]
fn upgrade_defaults_and_saved_choices_remain_explicit() -> io::Result<()> {
    let t = Fixture::new("experimental-defaults")?;
    for bytes in [
        b"config_version=2\n".as_slice(),
        b"config_version=2\n[plugins]\ndisabled=['future_tool']\n",
    ] {
        let mut c = Config::parse(bytes)?;
        for id in [DOS_LAUNCHER, ACOUSTIC_TRANSFERS] {
            assert!(!c.plugin_enabled(id));
            c.set_plugin_enabled(id, true);
        }
        assert!(!c.dos.open_on_enter);
        c.dos.open_on_enter = true;
        c.dos.executable = "/apps/Ёжик DOSBox Staging".into();
        let path = t.0.join("config.conf");
        c.save(&path)?;
        let c = Config::read(&path)?;
        assert!(c.dos.open_on_enter);
        assert_eq!(c.dos.executable, "/apps/Ёжик DOSBox Staging");
        for id in [DOS_LAUNCHER, ACOUSTIC_TRANSFERS] {
            assert!(c.plugin_enabled(id));
        }
    }
    for value in [
        "open_on_enter=1",
        "executable=true",
        "executable='bad\tpath'",
    ] {
        assert!(Config::parse(format!("[plugins.dos_launcher]\n{value}").as_bytes()).is_err());
    }
    Ok(())
}
#[test]
fn dos_option_uses_plugin_draft_and_retains_value_when_disabled() -> io::Result<()> {
    let t = Fixture::new("dos-draft")?;
    let mut a = t.app()?;
    a.open_settings(SettingsKind::Plugins);
    let count = a.modals.settings().unwrap().control_count();
    let dos_option = count - 1;
    draw(&mut a, 80, 25)?;
    assert!(!a
        .mouse
        .regions
        .iter()
        .any(|(_, t)| *t == Target::Setting(dos_option)));
    click(&mut a, Target::Setting(6));
    draw(&mut a, 80, 25)?;
    click(&mut a, Target::Setting(dos_option));
    assert!(a.modals.settings().unwrap().draft.dos.open_on_enter);
    key(&mut a, K::Esc);
    assert!(!a.config.dos.open_on_enter);
    assert!(!a.config.plugin_enabled(DOS_LAUNCHER));
    a.open_settings(SettingsKind::Plugins);
    draw(&mut a, 80, 25)?;
    click(&mut a, Target::Setting(6));
    draw(&mut a, 80, 25)?;
    click(&mut a, Target::Setting(dos_option));
    key(&mut a, K::Enter);
    assert!(a.config.dos.open_on_enter && a.config.plugin_enabled(DOS_LAUNCHER));
    a.open_settings(SettingsKind::Plugins);
    draw(&mut a, 80, 25)?;
    click(&mut a, Target::Setting(6));
    key(&mut a, K::Enter);
    assert!(!a.config.plugin_enabled(DOS_LAUNCHER));
    assert!(a.config.dos.open_on_enter);
    Ok(())
}
#[test]
fn owned_plugin_forms_fit_and_redraw_immutably_without_devices() -> io::Result<()> {
    let t = Fixture::new("experimental-ui")?;
    fs::write(t.0.join("tiny.txt"), b"tiny")?;
    for language in [Language::English, Language::Russian] {
        for palette in [
            PaletteProfile::ClassicDn,
            PaletteProfile::Terminal,
            PaletteProfile::ClassicDn.with_slot(dn::palette::theme::Slot::DialogText, 0x1e),
        ] {
            for id in [DOS_LAUNCHER, ACOUSTIC_TRANSFERS] {
                let mut a = t.app()?;
                a.config.language = language;
                a.config.palette = palette;
                a.config.set_plugin_enabled(id, true);
                menu::execute(&mut a, Action::Plugin(id));
                let owner = a.desktop.active().id;
                for (w, h) in [(80, 25), (132, 40), (60, 18), (80, 25)] {
                    let first = draw(&mut a, w, h)?;
                    for _ in 0..3 {
                        assert_eq!(first, draw(&mut a, w, h)?);
                    }
                    assert_eq!(a.desktop.active().id, owner);
                    assert!(a.last_dialog().is_none());
                }
                if id == ACOUSTIC_TRANSFERS && !cfg!(feature = "acoustic") {
                    let text = support::text(&mut a, 80, 25)?;
                    assert!(text.contains(language.text("Acoustic support is unavailable in this build; rebuild with --features acoustic.").split(';').next().unwrap()));
                    assert!(!a
                        .mouse
                        .regions
                        .iter()
                        .any(|(_, t)| *t == Target::Plugin(id, ActionId(4))));
                    key(&mut a, K::Esc);
                    assert!(!a.desktop.contains(owner));
                    continue;
                }
                if id == DOS_LAUNCHER {
                    click(&mut a, Target::Plugin(id, ActionId(0)));
                    dn::input::paste(&mut a, "/apps/test DOSBox");
                    draw(&mut a, 80, 25)?;
                    click(&mut a, Target::Plugin(id, ActionId(3)));
                    assert!(a.config.dos.executable.is_empty());
                } else {
                    let panel = &mut a.panels[a.active];
                    panel.cursor = panel
                        .entries
                        .iter()
                        .position(|e| e.name == "tiny.txt")
                        .unwrap();
                    draw(&mut a, 80, 25)?;
                    click(&mut a, Target::Plugin(id, ActionId(3)));
                    assert!(support::text(&mut a, 80, 25)?.contains("tiny.txt"));
                    click(&mut a, Target::Plugin(id, ActionId(2)));
                    draw(&mut a, 80, 25)?;
                    click(&mut a, Target::Plugin(id, ActionId(0)));
                    dn::input::paste(&mut a, "https://example.org/Ёжик");
                    let _ = draw(&mut a, 80, 25)?;
                    key(&mut a, K::Esc);
                }
                assert!(!a.desktop.contains(owner));
            }
        }
    }
    Ok(())
}
#[test]
fn invalid_saved_launcher_path_never_replaces_configuration() -> io::Result<()> {
    let t = Fixture::new("dos-config-safe")?;
    let path = t.0.join("config.conf");
    let mut c = Config::default();
    c.save(&path)?;
    let before = fs::read(&path)?;
    c.dos.executable = "x".repeat(4097);
    assert!(c.save(&path).is_err());
    assert_eq!(before, fs::read(&path)?);
    Ok(())
}
