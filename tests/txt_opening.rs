//! TXT opening preferences use private fixtures and never launch real applications.
mod support;
use dn::{associations, config::Config};
use std::{fs, io, time::Duration};
fn select(app: &mut dn::app::App, name: &str) {
    let p = &mut app.panels[app.active];
    p.cursor = p.entries.iter().position(|e| e.name == name).unwrap();
}
#[test]
fn enabled_txt_preference_opens_viewer_before_a_custom_editor_association() -> io::Result<()> {
    let f = support::Fixture::new("txt-preference-routing")?.canonicalized()?;
    fs::write(f.0.join("readme.TXT"), "TXT viewer fixture")?;
    let mut app = f.app()?;
    app.apply_config(Config::parse(
        b"config_version=2\n[file_manager]\nopen_txt_in_viewer=true\n",
    )?);
    support::wait(&mut app);
    app.associations.rules = associations::format::parse(".txt=@editor")?;
    select(&mut app, "readme.TXT");
    app.open_current();
    support::wait_until(&mut app, Duration::from_secs(8), |a| {
        a.desktop
            .active_viewer()
            .is_some_and(|v| v.contents.is_some())
            || a.desktop.active_editor().is_some_and(|e| !e.busy())
    });
    assert!(
        app.desktop.active_viewer().is_some(),
        "the TXT preference must take precedence over the editor association"
    );
    assert!(!app.launcher.loading());
    Ok(())
}

#[test]
fn config_defaults_roundtrip_validation_and_unknown_values_are_preserved() -> io::Result<()> {
    let f = support::Fixture::new("txt-preference-config")?.canonicalized()?;
    assert!(!Config::default().open_txt_in_viewer);
    assert!(
        !Config::parse(b"config_version=2\n[file_manager]\nuse_trash=false\n")?.open_txt_in_viewer
    );
    assert!(
        Config::parse(b"config_version=2\n[file_manager]\nopen_txt_in_viewer=\"yes\"\n").is_err()
    );
    let path = f.0.join("config.conf");
    fs::write(
        &path,
        "config_version=2\n[file_manager]\nfuture_setting=\"keep\"\n[other]\nvalue=17\n",
    )?;
    let config = Config {
        open_txt_in_viewer: true,
        save_on_exit: false,
        ..Default::default()
    };
    config.save(&path)?;
    let saved = Config::read(&path)?;
    assert!(saved.open_txt_in_viewer);
    assert!(!saved.save_on_exit);
    let document: toml::Table = fs::read_to_string(&path)?.parse().unwrap();
    assert_eq!(document["config_version"].as_integer(), Some(2));
    assert_eq!(
        document["file_manager"]["future_setting"].as_str(),
        Some("keep")
    );
    assert_eq!(document["other"]["value"].as_integer(), Some(17));
    let mut app = f.app()?;
    app.config_store = dn::config::Store::new(Some(path.clone()));
    let before = fs::read(&path)?;
    app.config.open_txt_in_viewer = false;
    app.config.save_on_exit = false;
    assert!(app.save_settings_on_exit());
    assert_eq!(fs::read(&path)?, before);
    app.config.save_on_exit = true;
    assert!(app.save_settings_on_exit());
    assert!(!Config::read(&path)?.open_txt_in_viewer);
    Ok(())
}

#[test]
fn settings_draft_mouse_keyboard_localization_resize_and_focus_are_consistent() -> io::Result<()> {
    use crossterm::event::KeyCode as K;
    use dn::{
        localization::Language,
        mouse::Target,
        settings::{Setting, SettingsKind},
    };
    let f = support::Fixture::new("txt-preference-settings")?.canonicalized()?;
    for language in [Language::English, Language::Russian] {
        let mut app = f.app()?;
        app.config.language = language;
        app.config.show_clock = false;
        if language == Language::Russian {
            app.config.palette = dn::palette::PaletteProfile::ClassicDn
                .with_slot(dn::palette::theme::Slot::DialogText, 0x1e);
        }
        app.open_settings(SettingsKind::FileManager);
        let row = app
            .modals
            .settings()
            .unwrap()
            .rows()
            .iter()
            .position(|s| *s == Setting::TxtViewer)
            .unwrap();
        for (w, h) in [(80, 25), (132, 40), (50, 12), (80, 25)] {
            let first = support::draw(&mut app, w, h)?;
            assert_eq!(first, support::draw(&mut app, w, h)?);
        }
        let text = support::text(&mut app, 80, 25)?;
        assert!(text.contains(&dn::menu::plain(language.text(Setting::TxtViewer.label()))));
        let bounds = app
            .mouse
            .regions
            .iter()
            .find(|(_, t)| *t == Target::Setting(row))
            .unwrap()
            .0;
        assert!(
            bounds.width >= 60,
            "long translated checkbox must use its own full-width row"
        );
        let mut mnemonics = std::collections::BTreeSet::new();
        for field in app.modals.settings().unwrap().rows() {
            assert!(mnemonics.insert(dn::menu::mnemonic(language.text(field.label())).unwrap()));
        }
        assert!(language.matches(Setting::TxtViewer.label(), 'x'));
        support::key(
            &mut app,
            K::Char(if language == Language::Russian {
                'о'
            } else {
                'x'
            }),
        );
        assert!(app.modals.settings().unwrap().draft.open_txt_in_viewer);
        assert!(!app.config.open_txt_in_viewer);
        support::key(&mut app, K::Esc);
        assert!(!app.config.open_txt_in_viewer);
        app.open_settings(SettingsKind::FileManager);
        support::draw(&mut app, 80, 25)?;
        support::click(&mut app, Target::Setting(row));
        assert!(!app.config.open_txt_in_viewer);
        support::key(&mut app, K::Enter);
        assert!(app.config.open_txt_in_viewer);
        assert!(app.modals.settings().is_none());
        app.open_settings(SettingsKind::FileManager);
        app.modals.settings_mut().unwrap().cursor = row;
        support::key(&mut app, K::Char(' '));
        support::draw(&mut app, 80, 25)?;
        support::click(&mut app, Target::CancelSettings);
        assert!(app.config.open_txt_in_viewer);
    }
    Ok(())
}

#[test]
fn default_off_uses_associations_and_only_txt_is_intercepted_when_enabled() -> io::Result<()> {
    use crossterm::event::KeyCode as K;
    let f = support::Fixture::new("txt-preference-extensions")?.canonicalized()?;
    for name in [
        "file.txt",
        "notes.log",
        "document.md",
        "readme",
        "file.txt.bak",
        "notxt",
    ] {
        fs::write(f.0.join(name), "ordinary source")?;
    }
    let mut app = f.app()?;
    app.associations.rules = associations::format::parse(
        ".txt=@editor\n.log=@editor\n.md=@editor\n<none>=@editor\n.bak=@editor",
    )?;
    select(&mut app, "file.txt");
    app.open_current();
    support::wait(&mut app);
    support::wait_until(&mut app, Duration::from_secs(8), |a| {
        a.desktop.active_editor().is_some_and(|e| !e.busy())
    });
    assert!(app.desktop.active_viewer().is_none());
    support::key(&mut app, K::Esc);
    app.config.open_txt_in_viewer = true;
    for name in [
        "notes.log",
        "document.md",
        "readme",
        "file.txt.bak",
        "notxt",
    ] {
        select(&mut app, name);
        app.open_current();
        support::wait_until(&mut app, Duration::from_secs(8), |a| {
            a.desktop.active_editor().is_some_and(|e| !e.busy())
        });
        assert!(app.desktop.active_viewer().is_none());
        support::key(&mut app, K::Esc);
    }
    Ok(())
}

#[test]
fn txt_double_click_reuses_viewer_preserves_marks_and_f3_f4_remain_explicit() -> io::Result<()> {
    use crossterm::event::KeyCode as K;
    let f = support::Fixture::new("txt-preference-input")?.canonicalized()?;
    fs::write(f.0.join("one.TXT"), "current file")?;
    fs::write(f.0.join("two.txt"), "marked file")?;
    let mut app = f.app()?;
    app.config.open_txt_in_viewer = true;
    app.associations.rules = associations::format::parse(".txt=@editor")?;
    app.panels[0].selected.insert("two.txt".into());
    select(&mut app, "one.TXT");
    let marks = app.panels[0].selected.clone();
    support::draw(&mut app, 80, 25)?;
    let entry = dn::mouse::Target::Entry(0, app.panels[0].cursor);
    support::click(&mut app, entry);
    support::click(&mut app, entry);
    support::wait_until(&mut app, Duration::from_secs(8), |a| {
        a.desktop
            .active_viewer()
            .is_some_and(|v| v.contents.is_some())
    });
    let viewer = app.desktop.active().id;
    assert_eq!(
        app.desktop.active_viewer().unwrap().path,
        f.0.join("one.TXT")
    );
    assert_eq!(app.panels[0].selected, marks);
    support::modified(&mut app, K::F(6), crossterm::event::KeyModifiers::ALT);
    assert_eq!(app.desktop.active().id, dn::desktop::WindowId::MANAGER);
    app.open_current();
    assert_eq!(app.desktop.active().id, viewer);
    support::key(&mut app, K::Esc);
    support::key(&mut app, K::F(4));
    support::wait_until(&mut app, Duration::from_secs(8), |a| {
        a.desktop.active_editor().is_some_and(|e| !e.busy())
    });
    assert_eq!(
        app.desktop
            .active_editor()
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .text,
        "current file"
    );
    support::key(&mut app, K::Esc);
    app.config.open_txt_in_viewer = false;
    support::key(&mut app, K::F(3));
    support::wait_until(&mut app, Duration::from_secs(8), |a| {
        a.desktop
            .active_viewer()
            .is_some_and(|v| v.contents.is_some())
    });
    assert_eq!(app.panels[0].selected, marks);
    assert!(!app.launcher.loading());
    Ok(())
}

#[test]
fn ctrl_enter_and_default_app_bypass_txt_preference_without_launching_a_real_app() -> io::Result<()>
{
    use crossterm::event::{KeyCode as K, KeyModifiers as M};
    for ctrl_enter in [false, true] {
        let f = support::Fixture::new("txt-preference-bypass")?.canonicalized()?;
        let path = f.0.join("file.txt");
        fs::write(&path, "data")?;
        let mut app = f.app()?;
        app.config.open_txt_in_viewer = true;
        app.associations.rules = associations::format::parse(".txt=@editor")?;
        select(&mut app, "file.txt");
        // Preserve the panel's captured entry, but make the opener fail safely
        // before reaching the operating system application launcher.
        fs::remove_file(&path)?;
        if ctrl_enter {
            support::modified(&mut app, K::Enter, M::CONTROL);
        } else {
            app.open_external_current();
        }
        assert!(app.launcher.loading());
        assert!(app.desktop.active_viewer().is_none());
        assert!(!app.associations.busy());
        support::wait_until(&mut app, Duration::from_secs(8), |a| !a.launcher.loading());
        assert!(!app.dialogs().is_empty());
        assert!(app.desktop.active_editor().is_none());
    }
    Ok(())
}

#[test]
fn directory_suffixes_and_failed_internal_opens_keep_existing_behavior() -> io::Result<()> {
    let f = support::Fixture::new("txt-preference-failure")?.canonicalized()?;
    fs::create_dir(f.0.join("folder.txt"))?;
    fs::write(f.0.join("missing.txt"), "temporary")?;
    let mut app = f.app()?;
    app.config.open_txt_in_viewer = true;
    select(&mut app, "folder.txt");
    app.open_current();
    support::wait(&mut app);
    assert_eq!(app.panels[0].path, f.0.join("folder.txt"));
    app.panels[0].parent(true);
    support::wait(&mut app);
    select(&mut app, "missing.txt");
    fs::remove_file(f.0.join("missing.txt"))?;
    app.open_current();
    support::wait_until(&mut app, Duration::from_secs(8), |a| {
        !a.dialogs().is_empty()
    });
    assert!(!app.launcher.loading());
    assert!(!app.associations.busy());
    assert!(app.desktop.active_viewer().is_none());
    Ok(())
}

#[cfg(unix)]
#[test]
fn txt_symlinks_retain_external_opening() -> io::Result<()> {
    let f = support::Fixture::new("txt-preference-symlink")?.canonicalized()?;
    fs::write(f.0.join("target"), "data")?;
    std::os::unix::fs::symlink(f.0.join("target"), f.0.join("link.txt"))?;
    let mut app = f.app()?;
    app.config.open_txt_in_viewer = true;
    select(&mut app, "link.txt");
    fs::remove_file(f.0.join("target"))?;
    app.open_current();
    assert!(app.launcher.loading());
    assert!(app.desktop.active_viewer().is_none());
    support::wait_until(&mut app, Duration::from_secs(8), |a| !a.launcher.loading());
    Ok(())
}

#[test]
fn txt_preference_keeps_encoding_prompts_and_archive_member_opening() -> io::Result<()> {
    use crossterm::event::KeyCode as K;
    use std::io::Write;
    let f = support::Fixture::new("txt-preference-encoding-archive")?.canonicalized()?;
    fs::write(f.0.join("legacy.txt"), [0xcf, 0xf0, 0xe8, 0xe2, 0xe5, 0xf2])?;
    let archive = f.0.join("docs.zip");
    let mut zip = zip::ZipWriter::new(fs::File::create(&archive)?);
    zip.start_file("note.txt", zip::write::SimpleFileOptions::default())?;
    zip.write_all(b"archived note")?;
    zip.finish()?;
    let mut app = f.app()?;
    app.config.open_txt_in_viewer = true;
    select(&mut app, "legacy.txt");
    app.open_current();
    support::wait_until(&mut app, Duration::from_secs(8), |a| {
        a.desktop
            .active_viewer()
            .is_some_and(|v| v.modals.viewer_encoding().is_some())
    });
    assert!(!app.launcher.loading());
    assert_eq!(
        app.desktop
            .active_viewer()
            .unwrap()
            .contents
            .as_ref()
            .unwrap()
            .raw
            .as_deref(),
        Some([0xcf, 0xf0, 0xe8, 0xe2, 0xe5, 0xf2].as_slice())
    );
    support::key(&mut app, K::Esc);
    support::key(&mut app, K::Esc);
    dn::archive::open(&mut app, archive);
    support::wait(&mut app);
    for enabled in [false, true] {
        app.config.open_txt_in_viewer = enabled;
        select(&mut app, "note.txt");
        app.open_current();
        support::wait_until(&mut app, Duration::from_secs(8), |a| {
            a.desktop
                .active_viewer()
                .is_some_and(|v| v.contents.is_some())
        });
        assert!(app
            .desktop
            .active_viewer()
            .unwrap()
            .contents
            .as_ref()
            .unwrap()
            .source
            .as_deref()
            .unwrap()
            .contains("archived note"));
        support::key(&mut app, K::Esc);
        assert!(app.panels[0].is_archive());
    }
    Ok(())
}
