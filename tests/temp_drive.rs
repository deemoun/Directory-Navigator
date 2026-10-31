use crossterm::event::KeyCode as K;
use dn::{
    app::App,
    dialog::PromptKind,
    menu::{self, Action},
    operations::Kind,
};
use std::{
    fs, io,
    path::Path,
    time::{Duration, Instant},
};
mod support;
use support::{key, wait, Fixture};

fn done(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.job.is_some() || app.transfers.has_work() || app.panels.iter().any(|p| p.loading) {
        app.poll();
        assert!(Instant::now() < deadline, "Temp Drive operation stalled");
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn select(app: &mut App, name: &str) {
    wait(app);
    app.panels[app.active].cursor = app.panels[app.active]
        .entries
        .iter()
        .position(|e| e.name == name)
        .unwrap();
}
fn dismiss(app: &mut App) {
    while !app.dialogs().is_empty() {
        app.pop_dialog();
    }
}
fn nonempty(path: &Path) -> io::Result<bool> {
    Ok(fs::read_dir(path)?.next().is_some())
}

#[test]
fn real_directory_opens_either_panel_and_survives_restart_without_saving_settings() -> io::Result<()>
{
    let f = Fixture::new("temp-drive-persist")?.canonicalized()?;
    let mut app = f.app()?;
    let root = f.0.join("temp-drive");
    assert!(!root.exists());
    menu::execute(&mut app, Action::ClearTempDrive);
    assert!(!root.exists());
    assert_eq!(app.notice, "Temp Drive is empty.");
    dismiss(&mut app);
    for active in [1, 0] {
        app.active = active;
        let other = app.panels[active ^ 1].path.clone();
        dn::editor::open_smartpad(&mut app);
        menu::execute(&mut app, Action::TempDrive);
        wait(&mut app);
        assert_eq!(app.desktop.active().id, dn::desktop::WindowId::MANAGER);
        assert_eq!(app.panels[active].path, root);
        assert_eq!(app.panels[active ^ 1].path, other);
    }
    fs::write(root.join("kept.txt"), b"persistent")?;
    drop(app);
    let mut app = f.app()?;
    assert!(!app.config.save_on_exit);
    menu::execute(&mut app, Action::TempDrive);
    wait(&mut app);
    assert!(app.panels[0].entries.iter().any(|e| e.name == "kept.txt"));
    assert_eq!(fs::read(root.join("kept.txt"))?, b"persistent");
    assert!(!f.0.join("config.conf").exists());
    Ok(())
}

#[test]
fn quick_copy_pins_sources_and_native_destination_with_cancel_collisions_and_folders(
) -> io::Result<()> {
    let f = Fixture::new("temp-drive-copy")?.canonicalized()?;
    fs::write(f.0.join("文.txt"), b"original")?;
    fs::create_dir(f.0.join("folder"))?;
    fs::write(f.0.join("folder/nested"), b"nested")?;
    let mut app = f.app()?;
    select(&mut app, "文.txt");
    menu::execute(&mut app, Action::CopyToTempDrive);
    let root = f.0.join("temp-drive");
    let prompt = app.modals.prompt().unwrap();
    assert_eq!(prompt.base, root);
    assert_eq!(prompt.copy.as_ref().unwrap().picked.as_ref(), Some(&root));
    assert_eq!(prompt.sources, vec![f.0.join("文.txt")]);
    key(&mut app, K::Esc);
    assert!(!nonempty(&root)?);
    app.panels[0]
        .selected
        .extend(["文.txt".into(), "folder".into()]);
    menu::execute(&mut app, Action::CopyToTempDrive);
    // Later selection changes must not retarget the captured request.
    app.panels[0].selected.clear();
    app.panels[0].cursor = 0;
    app.submit_prompt();
    done(&mut app);
    assert_eq!(fs::read(root.join("文.txt"))?, b"original");
    assert_eq!(fs::read(root.join("folder/nested"))?, b"nested");
    assert_eq!(fs::read(f.0.join("文.txt"))?, b"original");
    assert!(f.0.join("folder/nested").exists());
    key(&mut app, K::Esc); // hide Transfer Center
    select(&mut app, "文.txt");
    menu::execute(&mut app, Action::CopyToTempDrive);
    app.submit_prompt();
    done(&mut app);
    assert_eq!(
        app.transfers.entries.back().unwrap().status,
        dn::transfers::Status::Failed
    );
    assert_eq!(fs::read(root.join("文.txt"))?, b"original");
    Ok(())
}

#[test]
fn ordinary_f5_uses_temp_drive_as_destination_and_keeps_originals() -> io::Result<()> {
    let f = Fixture::new("temp-drive-f5")?.canonicalized()?;
    fs::write(f.0.join("source"), b"bytes")?;
    let mut app = f.app()?;
    app.active = 1;
    menu::execute(&mut app, Action::TempDrive);
    wait(&mut app);
    app.active = 0;
    select(&mut app, "source");
    key(&mut app, K::F(5));
    assert_eq!(app.modals.prompt().unwrap().base, f.0.join("temp-drive"));
    app.submit_prompt();
    done(&mut app);
    assert_eq!(fs::read(f.0.join("temp-drive/source"))?, b"bytes");
    assert_eq!(fs::read(f.0.join("source"))?, b"bytes");
    Ok(())
}

#[test]
fn clear_defaults_to_cancel_includes_hidden_filtered_items_and_keeps_root_and_outside_data(
) -> io::Result<()> {
    let f = Fixture::new("temp-drive-clear")?.canonicalized()?;
    let mut app = f.app()?;
    app.config.use_trash = false;
    menu::execute(&mut app, Action::TempDrive);
    wait(&mut app);
    let root = f.0.join("temp-drive");
    for name in [".hidden", "visible", "文.txt"] {
        fs::write(root.join(name), b"data")?;
    }
    fs::create_dir(root.join("folder"))?;
    fs::write(root.join("folder/child"), b"child")?;
    fs::write(f.0.join("outside"), b"keep")?;
    app.panels[0].filter = "nothing-matches".into();
    menu::execute(&mut app, Action::ClearTempDrive);
    let prompt = app.modals.prompt().unwrap();
    assert_eq!(prompt.sources.len(), 4);
    assert_eq!(prompt.focus, 2);
    assert!(matches!(prompt.kind, PromptKind::ClearTempDrive(_)));
    key(&mut app, K::Enter);
    assert!(app.modals.prompt().is_none());
    assert!(nonempty(&root)?);
    menu::execute(&mut app, Action::ClearTempDrive);
    app.submit_prompt();
    done(&mut app);
    assert!(root.is_dir());
    assert!(!nonempty(&root)?);
    assert_eq!(fs::read(f.0.join("outside"))?, b"keep");
    menu::execute(&mut app, Action::ClearTempDrive);
    assert!(app.modals.prompt().is_none());
    assert_eq!(app.notice, "Temp Drive is empty.");
    Ok(())
}

#[test]
fn clear_confirmation_honours_trash_preference_and_ui_is_stable_in_both_languages() -> io::Result<()>
{
    let f = Fixture::new("temp-drive-ui")?.canonicalized()?;
    let mut app = f.app()?;
    menu::execute(&mut app, Action::TempDrive);
    wait(&mut app);
    fs::write(f.0.join("temp-drive/item"), b"data")?;
    for lang in [
        dn::localization::Language::English,
        dn::localization::Language::Russian,
    ] {
        app.config.language = lang;
        for trash in [true, false] {
            app.config.use_trash = trash;
            for (w, h) in [(80, 25), (132, 40), (80, 25)] {
                app.config.palette = dn::palette::PaletteProfile::ClassicDn
                    .with_slot(dn::palette::theme::Slot::DialogText, 0x1e);
                menu::execute(&mut app, Action::ClearTempDrive);
                let before = support::draw(&mut app, w, h)?;
                assert_eq!(before, support::draw(&mut app, w, h)?);
                assert_eq!(before, support::draw(&mut app, w, h)?);
                let text: String = before.content.iter().map(|cell| cell.symbol()).collect();
                assert!(text.contains(lang.text("All contents, including hidden items.")));
                assert!(text.contains(lang.text(if trash {
                    "Restore using your desktop file manager."
                } else {
                    "This cannot be undone."
                })));
                support::click(&mut app, dn::mouse::Target::CancelPrompt);
                assert!(app.modals.prompt().is_none());
            }
        }
    }
    assert!(f.0.join("temp-drive/item").exists());
    Ok(())
}

#[test]
fn unsafe_or_replaced_roots_and_sources_are_rejected_without_touching_other_files() -> io::Result<()>
{
    let f = Fixture::new("temp-drive-replaced")?.canonicalized()?;
    let mut app = f.app()?;
    let root = f.0.join("temp-drive");
    fs::write(&root, b"not a directory")?;
    let original = app.panels[0].path.clone();
    menu::execute(&mut app, Action::TempDrive);
    assert_eq!(app.panels[0].path, original);
    assert!(!app.dialogs().is_empty());
    dismiss(&mut app);
    fs::remove_file(&root)?;
    menu::execute(&mut app, Action::TempDrive);
    wait(&mut app);
    app.config.use_trash = false;
    fs::write(root.join("item"), b"old")?;
    menu::execute(&mut app, Action::ClearTempDrive);
    fs::rename(root.join("item"), root.join("original"))?;
    fs::write(root.join("item"), b"replacement")?;
    app.submit_prompt();
    assert!(app.job.is_none());
    assert_eq!(fs::read(root.join("item"))?, b"replacement");
    dismiss(&mut app);
    menu::execute(&mut app, Action::ClearTempDrive);
    fs::rename(&root, f.0.join("old-root"))?;
    fs::create_dir(&root)?;
    fs::write(root.join("keep"), b"new root")?;
    app.submit_prompt();
    assert!(app.job.is_none());
    assert_eq!(fs::read(root.join("keep"))?, b"new root");
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlink_root_is_rejected_and_links_inside_clear_do_not_follow_targets() -> io::Result<()> {
    use std::os::unix::fs::symlink;
    let f = Fixture::new("temp-drive-links")?.canonicalized()?;
    let root = f.0.join("temp-drive");
    fs::create_dir(f.0.join("outside"))?;
    fs::write(f.0.join("outside/keep"), b"keep")?;
    symlink(f.0.join("outside"), &root)?;
    let mut app = f.app()?;
    menu::execute(&mut app, Action::ClearTempDrive);
    assert!(app.modals.prompt().is_none());
    assert!(!app.dialogs().is_empty());
    dismiss(&mut app);
    fs::remove_file(&root)?;
    fs::create_dir(&root)?;
    symlink(f.0.join("outside"), root.join("link"))?;
    app.config.use_trash = false;
    menu::execute(&mut app, Action::ClearTempDrive);
    app.submit_prompt();
    done(&mut app);
    assert!(!nonempty(&root)?);
    assert_eq!(fs::read(f.0.join("outside/keep"))?, b"keep");
    Ok(())
}

#[test]
fn copy_guard_rejects_root_replacement_and_clear_does_not_run_during_transfers() -> io::Result<()> {
    let f = Fixture::new("temp-drive-copy-guard")?.canonicalized()?;
    fs::write(f.0.join("source"), b"keep")?;
    let mut app = f.app()?;
    select(&mut app, "source");
    menu::execute(&mut app, Action::CopyToTempDrive);
    let root = f.0.join("temp-drive");
    fs::rename(&root, f.0.join("old-root"))?;
    fs::create_dir(&root)?;
    app.submit_prompt();
    assert!(!app.transfers.has_work());
    assert!(!nonempty(&root)?);
    dismiss(&mut app);
    app.transfers.paused = true;
    dn::transfers::submit(
        &mut app,
        dn::operations::Plan {
            kind: Kind::Copy,
            sources: vec![f.0.join("source")],
            destination: root.clone(),
        },
        Default::default(),
    )
    .unwrap();
    menu::execute(&mut app, Action::ClearTempDrive);
    assert!(app.modals.prompt().is_none());
    assert_eq!(fs::read(f.0.join("source"))?, b"keep");
    Ok(())
}

#[test]
fn queued_quick_copy_rejects_root_replacement_before_preparation() -> io::Result<()> {
    let f = Fixture::new("temp-drive-queued")?.canonicalized()?;
    fs::write(f.0.join("source"), b"keep")?;
    let mut app = f.app()?;
    select(&mut app, "source");
    app.transfers.paused = true;
    menu::execute(&mut app, Action::CopyToTempDrive);
    app.submit_prompt();
    assert!(app.transfers.has_work());
    let root = f.0.join("temp-drive");
    fs::rename(&root, f.0.join("old-root"))?;
    fs::create_dir(&root)?;
    app.transfers.paused = false;
    done(&mut app);
    assert_eq!(
        app.transfers.entries.back().unwrap().status,
        dn::transfers::Status::Failed
    );
    assert!(!nonempty(&root)?);
    assert!(!nonempty(&f.0.join("old-root"))?);
    assert_eq!(fs::read(f.0.join("source"))?, b"keep");
    Ok(())
}

// Linux permits native non-UTF-8 names; macOS APFS requires valid UTF-8.
#[cfg(target_os = "linux")]
#[test]
fn native_non_utf8_storage_path_and_unrelated_settings_are_preserved() -> io::Result<()> {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let f = Fixture::new("temp-drive-native")?.canonicalized()?;
    fs::write(f.0.join("source"), b"native")?;
    let parent = f.0.join(OsString::from_vec(b"settings-\xff".to_vec()));
    fs::create_dir(&parent)?;
    let config = parent.join("config.conf");
    let bytes = b"config_version=2\n[unknown]\nkeep=\"untouched\"\n";
    fs::write(&config, bytes)?;
    let mut app = f.app()?;
    app.config_store = dn::config::Store::new(Some(config.clone()));
    select(&mut app, "source");
    menu::execute(&mut app, Action::CopyToTempDrive);
    app.submit_prompt();
    done(&mut app);
    assert_eq!(fs::read(parent.join("temp-drive/source"))?, b"native");
    assert_eq!(fs::read(config)?, bytes);
    Ok(())
}

#[test]
fn actual_config_location_is_used_without_changing_unrelated_settings() -> io::Result<()> {
    let f = Fixture::new("temp-drive-config-location")?.canonicalized()?;
    let parent = f.0.join("настройки");
    fs::create_dir(&parent)?;
    let config = parent.join("custom.conf");
    let bytes = b"config_version=2\n[unknown]\nkeep=\"untouched\"\n";
    fs::write(&config, bytes)?;
    let mut app = f.app()?;
    app.config_store = dn::config::Store::new(Some(config.clone()));
    menu::execute(&mut app, Action::TempDrive);
    wait(&mut app);
    assert_eq!(app.panels[0].path, parent.join("temp-drive"));
    fs::write(parent.join("temp-drive/kept"), b"persistent")?;
    assert_eq!(fs::read(config)?, bytes);
    assert!(!f.0.join("temp-drive").exists());
    Ok(())
}
