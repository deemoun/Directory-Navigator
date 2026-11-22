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
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                    loop {
                        a.poll();
                        if support::text(&mut a, 80, 25)?.contains("tiny.txt (") {
                            break;
                        }
                        assert!(
                            std::time::Instant::now() < deadline,
                            "acoustic capture did not finish"
                        );
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
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

#[test]
fn dos_profile_roundtrip_defaults_and_nested_drafts_are_independent() -> io::Result<()> {
    let t = Fixture::new("dos-shared-settings")?;
    let old = Config::parse(b"config_version=2\n[plugins.dos_launcher]\nopen_on_enter=true\n")?;
    assert_eq!(old.dos.profile, Config::default().dos.profile);
    let mut config = old;
    config.dos.profile = Config::parse(b"config_version=2\n[plugins.dos_launcher]\ncpu_speed='fixed'\ncpu_cycles=25000\nmemory_mb=32\ndisplay='windowed'\nwindow_resolution='1024x768'\n")?.dos.profile;
    config.save(&t.0.join("profile.conf"))?;
    assert_eq!(Config::read(&t.0.join("profile.conf"))?.dos, config.dos);
    for bad in [
        "cpu_speed='guess'",
        "cpu_cycles=49",
        "cpu_cycles=2000001",
        "memory_mb=64",
        "display='guess'",
        "window_resolution='123x456'",
        "window_resolution=800",
    ] {
        assert!(Config::parse(
            format!("config_version=2\n[plugins.dos_launcher]\n{bad}\n").as_bytes()
        )
        .is_err());
    }
    for language in [Language::English, Language::Russian] {
        let mut a = t.app()?;
        a.config.language = language;
        a.config.set_plugin_enabled(DOS_LAUNCHER, true);
        menu::execute(&mut a, Action::Plugin(DOS_LAUNCHER));
        let committed = a.config.dos.clone();
        for (w, h) in [(80, 25), (132, 40), (80, 25)] {
            draw(&mut a, w, h)?;
            click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(8)));
            draw(&mut a, w, h)?;
            click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(104)));
            let text = support::text(&mut a, w, h)?;
            assert!(text.contains(language.text("CPU speed")));
            assert!(text.contains(language.text("Window resolution")));
            assert_eq!(text, support::text(&mut a, w, h)?);
            click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(100))); // Fixed
            draw(&mut a, w, h)?;
            click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(102))); // 1 MiB
            draw(&mut a, w, h)?;
            click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(110)));
            assert!(support::text(&mut a, w, h)?.contains("640x480"));
            assert_eq!(a.config.dos, committed);
            key(&mut a, K::Esc);
            assert_eq!(a.config.dos, committed);
            draw(&mut a, w, h)?;
            click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(8)));
            draw(&mut a, w, h)?;
            click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(104)));
            draw(&mut a, w, h)?;
            click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(100)));
            draw(&mut a, w, h)?;
            click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(110)));
            draw(&mut a, w, h)?;
            click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(105)));
            assert_eq!(a.config.dos, committed);
            draw(&mut a, w, h)?;
            click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(8)));
            draw(&mut a, w, h)?;
            click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(104))); // Reset only draft
            key(&mut a, K::Esc);
        }
        draw(&mut a, 80, 25)?;
        click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(2)));
        assert_eq!(a.config.dos.profile.cpu.key(), "fixed");
        assert_eq!(a.config.dos.profile.resolution.key(), "640x480");
        let applied = a.config.dos.clone();
        menu::execute(&mut a, Action::Plugin(DOS_LAUNCHER));
        draw(&mut a, 80, 25)?;
        click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(8)));
        draw(&mut a, 80, 25)?;
        click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(104)));
        draw(&mut a, 80, 25)?;
        click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(105)));
        draw(&mut a, 80, 25)?;
        key(&mut a, K::Esc);
        assert_eq!(a.config.dos, applied);
    }
    Ok(())
}

#[test]
fn windows9x_profiles_save_per_image_without_changing_dos_preferences() -> io::Result<()> {
    let t = Fixture::new("windows9x-draft")?;
    let old = Config::parse(
        b"config_version=2\n[plugins.dos_launcher]\ncpu_speed='fixed'\ncpu_cycles=25000\n",
    )?;
    assert_eq!(old.dos.windows.edition.key(), "98");
    assert_eq!(old.dos.windows.memory_mb, 128);
    assert!(old.dos.windows.image.is_empty());
    assert!(!old.plugin_enabled(DOS_LAUNCHER));
    let bytes=b"config_version=2\n[plugins.dos_launcher.windows9x]\nedition='95_osr2'\nimage='guest disk.img'\nmemory_mb=64\nmouse=false\nsound=false\nfullscreen=true\nvoodoo=true\nunknown_future='retained'\n";
    let path = t.0.join("windows.conf");
    fs::write(&path, bytes)?;
    let c = Config::read(&path)?;
    c.save(&path)?;
    assert_eq!(Config::read(&path)?.dos.windows, c.dos.windows);
    assert!(fs::read_to_string(path)?.contains("unknown_future"));
    for bad in [
        "edition='me'",
        "memory_mb=32",
        "memory_mb=1024",
        "edition='95'\nmemory_mb=512",
        "image='bad%image.img'",
        "executable='bad\ttool'",
    ] {
        assert!(
            Config::parse(
                format!("config_version=2\n[plugins.dos_launcher.windows9x]\n{bad}\n").as_bytes()
            )
            .is_err(),
            "{bad}"
        );
    }
    for (language, palette) in [
        (Language::English, PaletteProfile::ClassicDn),
        (Language::Russian, PaletteProfile::Terminal),
    ] {
        let t = Fixture::new("windows-saved-images")?;
        let disk = t.0.join("config.conf");
        Config::default().save(&disk)?;
        let mut a = t.app()?;
        a.config_store.load()?;
        a.config.language = language;
        a.config.palette = palette;
        a.config.show_clock = false;
        a.config.set_plugin_enabled(DOS_LAUNCHER, true);
        let ordinary = a.config.dos.profile.clone();
        let image1 = t.0.join("Windows Ёжик.img");
        fs::write(&image1, b"image is untouched")?;
        menu::execute(&mut a, Action::Plugin(DOS_LAUNCHER));
        for (w, h) in [(80, 25), (120, 36), (80, 25)] {
            draw(&mut a, w, h)?;
            assert!(!a
                .mouse
                .regions
                .iter()
                .any(|(_, t)| *t == Target::Plugin(DOS_LAUNCHER, ActionId(10))));
            click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(9)));
            let text = support::text(&mut a, w, h)?;
            assert!(text.contains(language.text("Windows 9x — DOSBox-X")));
            assert_eq!(text, support::text(&mut a, w, h)?);
            assert!(!a
                .mouse
                .regions
                .iter()
                .any(|(_, t)| *t == Target::Plugin(DOS_LAUNCHER, ActionId(210))));
            key(&mut a, K::Esc);
        }
        draw(&mut a, 80, 25)?;
        click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(9)));
        draw(&mut a, 80, 25)?;
        dn::input::paste(&mut a, image1.to_str().unwrap());
        click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(204)));
        draw(&mut a, 80, 25)?;
        click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(207)));
        draw(&mut a, 80, 25)?;
        click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(211)));
        let saved = Config::read(&disk)?;
        assert_eq!(saved.dos.windows.memory_mb, 256);
        assert!(saved.dos.windows.fullscreen);
        assert_eq!(saved.dos.windows_images.len(), 1);
        assert!(saved.show_clock);
        assert!(!a.config.show_clock);
        assert!(!a.config.save_on_exit);
        assert_eq!(a.config.dos.profile, ordinary);
        draw(&mut a, 80, 25)?;
        key(&mut a, K::Esc); // Explicit profile save survives launcher Cancel.
        menu::execute(&mut a, Action::Plugin(DOS_LAUNCHER));
        draw(&mut a, 80, 25)?;
        assert!(a
            .mouse
            .regions
            .iter()
            .any(|(_, t)| *t == Target::Plugin(DOS_LAUNCHER, ActionId(10))));
        click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(9)));
        draw(&mut a, 80, 25)?;
        click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(214)));
        let text = support::text(&mut a, 80, 25)?;
        assert!(text.contains("Windows Ёжик.img"));
        assert_eq!(text, support::text(&mut a, 80, 25)?);
        key(&mut a, K::End);
        key(&mut a, K::Enter);
        draw(&mut a, 80, 25)?;
        click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(200)));
        let image2 = t.0.join("Other Windows.img");
        dn::input::paste(&mut a, image2.to_str().unwrap());
        draw(&mut a, 80, 25)?;
        click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(206)));
        draw(&mut a, 80, 25)?;
        click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(211)));
        let saved = Config::read(&disk)?;
        assert_eq!(saved.dos.windows_images.len(), 2);
        assert_eq!(saved.dos.windows_images[0].memory_mb, 256);
        assert_eq!(saved.dos.windows_images[1].memory_mb, 128);
        assert!(!saved.dos.windows.sound);
        draw(&mut a, 80, 25)?;
        click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(9)));
        draw(&mut a, 80, 25)?;
        click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(214)));
        key(&mut a, K::Home);
        key(&mut a, K::Enter);
        draw(&mut a, 80, 25)?;
        assert!(support::text(&mut a, 80, 25)?.contains("256 MiB"));
        click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(211)));
        assert_eq!(
            Config::read(&disk)?.dos.windows.image,
            image1.to_str().unwrap()
        );
        draw(&mut a, 80, 25)?;
        click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(9)));
        draw(&mut a, 80, 25)?;
        click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(215)));
        assert_eq!(Config::read(&disk)?.dos.windows_images.len(), 1);
        assert_eq!(fs::read(&image1)?, b"image is untouched");
    }
    Ok(())
}

#[test]
fn windows_image_profiles_bounds_unknown_fields_and_save_failure() -> io::Result<()> {
    let t = Fixture::new("windows-profile-storage")?;
    let path = t.0.join("config.conf");
    let text="config_version=2\n[plugins.dos_launcher.windows9x]\nimages=[{image='A.img',memory_mb=256,future='keep'}]\n";
    fs::write(&path, text)?;
    let mut c = Config::read(&path)?;
    c.dos.windows_images[0].fullscreen = true;
    c.save(&path)?;
    assert!(fs::read_to_string(&path)?.contains("future"));
    for images in [
        "[{image='A.img'},{image='A.img'}]".to_owned(),
        "[{image=''}]".into(),
        "[{image='A.img',memory_mb=32}]".into(),
        format!(
            "[{}]",
            (0..33)
                .map(|i| format!("{{image='x{i}.img'}}"))
                .collect::<Vec<_>>()
                .join(",")
        ),
    ] {
        assert!(Config::parse(
            format!("config_version=2\n[plugins.dos_launcher.windows9x]\nimages={images}\n")
                .as_bytes()
        )
        .is_err());
    }
    let mut a = t.app()?;
    a.config = a.config_store.load()?;
    a.config.set_plugin_enabled(DOS_LAUNCHER, true);
    let before = a.config.dos.clone();
    menu::execute(&mut a, Action::Plugin(DOS_LAUNCHER));
    draw(&mut a, 80, 25)?;
    click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(9)));
    draw(&mut a, 80, 25)?;
    dn::input::paste(&mut a, t.0.join("B.img").to_str().unwrap());
    let changed = format!(
        "{}\n# another instance changed this file\n",
        fs::read_to_string(&path)?
    );
    fs::write(&path, &changed)?;
    click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(211)));
    assert!(a
        .last_dialog()
        .unwrap()
        .message
        .contains("Settings changed outside"));
    assert_eq!(a.config.dos, before);
    assert_eq!(fs::read_to_string(&path)?, changed);
    a.pop_dialog();
    draw(&mut a, 80, 25)?;
    click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(200)));
    assert!(support::text(&mut a, 80, 25)?.contains("B.img"));
    Ok(())
}

#[test]
fn extra_dos_drive_defaults_persistence_owned_picker_and_cancel() -> io::Result<()> {
    let t = Fixture::new("dos-extra-drive")?;
    let old = Config::parse(b"config_version=2\n")?;
    assert_eq!(old.dos.mount.mode.key(), "off");
    assert_eq!(old.dos.mount.drive, b'D');
    let text=format!("config_version=2\n[plugins.dos_launcher]\nmount_mode='folder'\nmount_folder={}\nmount_drive='X'\n",toml::Value::String(t.0.to_str().unwrap().into()));
    let c = Config::parse(text.as_bytes())?;
    let path = t.0.join("mount-config.conf");
    c.save(&path)?;
    assert_eq!(Config::read(&path)?.dos.mount, c.dos.mount);
    for bad in [
        "mount_mode='other'",
        "mount_drive='C'",
        "mount_drive='Z'",
        "mount_drive='DD'",
        "mount_mode='folder'\nmount_folder='relative'",
        "mount_mode='folder'\nmount_folder='bad%path'",
    ] {
        assert!(
            Config::parse(format!("config_version=2\n[plugins.dos_launcher]\n{bad}\n").as_bytes())
                .is_err(),
            "{bad}"
        );
    }
    for language in [Language::English, Language::Russian] {
        let mut a = t.app()?;
        a.config.language = language;
        a.config.set_plugin_enabled(DOS_LAUNCHER, true);
        let committed = a.config.dos.clone();
        let opposite = a.panels[1].path.clone();
        menu::execute(&mut a, Action::Plugin(DOS_LAUNCHER));
        for (w, h) in [(80, 25), (120, 36), (80, 25)] {
            draw(&mut a, w, h)?;
            click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(8)));
            draw(&mut a, w, h)?;
            click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(109)));
            let text = support::text(&mut a, w, h)?;
            assert!(text.contains(language.text("Extra DOS drive")));
            assert_eq!(text, support::text(&mut a, w, h)?);
            assert!(!a
                .mouse
                .regions
                .iter()
                .any(|(_, t)| *t == Target::Plugin(DOS_LAUNCHER, ActionId(303))));
            key(&mut a, K::Esc);
            draw(&mut a, w, h)?;
            key(&mut a, K::Esc);
            assert_eq!(a.config.dos, committed);
        }
        draw(&mut a, 80, 25)?;
        click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(8)));
        draw(&mut a, 80, 25)?;
        click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(109)));
        draw(&mut a, 80, 25)?;
        click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(300)));
        draw(&mut a, 80, 25)?;
        click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(300))); // Folder
        draw(&mut a, 80, 25)?;
        click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(303)));
        assert!(a.directory_tree().is_some());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while a
            .directory_tree()
            .is_some_and(|tree| tree.selected().is_none())
        {
            a.poll();
            assert!(
                std::time::Instant::now() < deadline,
                "folder picker did not load its selected row"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let selected = a.directory_tree().unwrap().selected().unwrap().path.clone();
        dn::directory_tree::action(&mut a, dn::directory_tree::Action::Open);
        assert!(a.directory_tree().is_none());
        draw(&mut a, 80, 25)?;
        click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(301)));
        draw(&mut a, 80, 25)?;
        click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(304)));
        draw(&mut a, 80, 25)?;
        click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(105)));
        assert_eq!(a.config.dos, committed);
        draw(&mut a, 80, 25)?;
        click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(2)));
        assert_eq!(a.config.dos.mount.mode.key(), "folder");
        assert_eq!(a.config.dos.mount.folder, selected.to_str().unwrap());
        assert_eq!(a.config.dos.mount.drive, b'E');
        assert_eq!(a.panels[1].path, opposite);
        assert_eq!(a.config.dos.windows, committed.windows);
    }
    Ok(())
}

#[test]
fn long_windows_image_names_keep_ram_and_edition_visible() -> io::Result<()> {
    let t = Fixture::new("dos-image-summary")?;
    for language in [Language::English, Language::Russian] {
        let mut a = t.app()?;
        a.config.language = language;
        a.config.set_plugin_enabled(DOS_LAUNCHER, true);
        let image = t.0.join(format!(
            "{}-e\u{301}-界-final.img",
            "ОченьДлинноеИмя".repeat(12)
        ));
        a.config.dos.windows.image = image.to_str().unwrap().into();
        a.config.dos.windows.memory_mb = 128;
        a.config.dos.windows_images = vec![a.config.dos.windows.clone()];
        let original = a.config.dos.clone();
        menu::execute(&mut a, Action::Plugin(DOS_LAUNCHER));
        let text = support::text(&mut a, 80, 25)?;
        assert!(text.contains("128 MiB"), "image RAM clipped:\n{text}");
        assert!(
            text.contains("final.img"),
            "image filename suffix clipped:\n{text}"
        );
        click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(9)));
        draw(&mut a, 80, 25)?;
        click(&mut a, Target::Plugin(DOS_LAUNCHER, ActionId(214)));
        let text = support::text(&mut a, 80, 25)?;
        assert!(text.contains("128 MiB"), "saved image RAM clipped:\n{text}");
        assert_eq!(a.config.dos, original);
    }
    Ok(())
}
