mod support;
use dn::{
    app::App,
    editor::{
        self,
        recovery::{self, store, Action as R, Record},
        EditorAction as E,
    },
    menu,
};
use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
fn wait(a: &mut App, ready: impl Fn(&App) -> bool) {
    let until = Instant::now() + Duration::from_secs(8);
    loop {
        a.poll();
        if ready(a) {
            return;
        }
        assert!(Instant::now() < until, "recovery timeout: {}", a.notice);
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn settled(a: &mut App) {
    wait(a, |a| a.editor_recovery.as_ref().is_some_and(|m| !m.busy()));
}
fn tick(a: &mut App) {
    static CLOCK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(10);
    let seconds = CLOCK.fetch_add(10, std::sync::atomic::Ordering::Relaxed);
    recovery::poll(a, Instant::now() + Duration::from_secs(seconds));
    settled(a);
}
fn files(root: &Path) -> Vec<PathBuf> {
    fs::read_dir(root)
        .unwrap()
        .flat_map(|s| fs::read_dir(s.unwrap().path()).unwrap())
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|s| s == "rec"))
        .collect()
}
fn record(root: &Path) -> Record {
    Record {
        text: "unsaved 界 e\u{301}\r\ntext".into(),
        base: root.to_owned(),
        path: Some(root.join("source.txt")),
        cursor: 0,
        anchor: None,
        bom: true,
        time: 1700000000,
    }
}
fn abandoned(root: &Path, base: &Path) -> io::Result<PathBuf> {
    let s = store::Store::open(root)?;
    let p = s.current.dir.join("1.rec");
    store::write(&p, &record(base))?;
    drop(s);
    Ok(p)
}
#[test]
fn checkpoints_roundtrip_unicode_bom_native_paths_and_detect_corruption() -> io::Result<()> {
    let f = support::Fixture::new("checkpoint-format")?.canonicalized()?;
    let root = f.0.join("recovery");
    let s = store::Store::open(&root)?;
    let p = s.current.dir.join("1.rec");
    let r = record(&f.0);
    store::write(&p, &r)?;
    let got = store::read(&p)?;
    assert_eq!(got.text, r.text);
    assert_eq!(got.path, r.path);
    assert!(got.bom);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(&p)?.permissions().mode() & 0o777, 0o600);
        assert_eq!(
            fs::metadata(&s.current.dir)?.permissions().mode() & 0o777,
            0o700
        );
    }
    let mut bytes = fs::read(&p)?;
    *bytes.last_mut().unwrap() ^= 1;
    fs::write(&p, bytes)?;
    assert!(store::read(&p).is_err());
    Ok(())
}
#[test]
fn live_sessions_are_excluded_and_claims_prevent_duplicate_recovery() -> io::Result<()> {
    let f = support::Fixture::new("checkpoint-leases")?.canonicalized()?;
    let root = f.0.join("recovery");
    let first = store::Store::open(&root)?;
    store::write(&first.current.dir.join("1.rec"), &record(&f.0))?;
    let second = store::Store::open(&root)?;
    assert!(second.pending.is_empty());
    drop(first);
    let third = store::Store::open(&root)?;
    assert_eq!(third.pending.len(), 1);
    let fourth = store::Store::open(&root)?;
    assert!(fourth.pending.is_empty());
    drop(third);
    drop(second);
    drop(fourth);
    assert_eq!(store::Store::open(&root)?.pending.len(), 1);
    Ok(())
}
#[test]
fn restart_recovers_an_unsaved_copy_without_touching_source() -> io::Result<()> {
    let f = support::Fixture::new("checkpoint-restart")?.canonicalized()?;
    let root = f.0.join("recovery");
    fs::write(f.0.join("source.txt"), "original")?;
    let mut a = f.app()?;
    recovery::start(&mut a, root.clone());
    settled(&mut a);
    editor::open_path(&mut a, f.0.join("source.txt"));
    support::wait(&mut a);
    editor::paste(&mut a, "unsaved 界\n");
    tick(&mut a);
    assert_eq!(files(&root).len(), 1);
    let bytes = a
        .desktop
        .active_editor()
        .unwrap()
        .buffer
        .as_ref()
        .unwrap()
        .text
        .clone();
    drop(a);
    let mut a = f.app()?;
    recovery::start(&mut a, root.clone());
    wait(&mut a, |a| a.modals.recovery().is_some());
    assert_eq!(a.modals.recovery().unwrap().record.text, bytes);
    recovery::action(&mut a, R::Recover);
    let e = a.desktop.active_editor().unwrap();
    assert!(e.path.is_none());
    assert!(e.dirty());
    assert_eq!(e.buffer.as_ref().unwrap().text, bytes);
    assert_eq!(fs::read(f.0.join("source.txt"))?, b"original");
    tick(&mut a);
    assert_eq!(files(&root).len(), 1);
    drop(a);
    let mut a = f.app()?;
    recovery::start(&mut a, root);
    wait(&mut a, |a| a.modals.recovery().is_some());
    assert_eq!(a.modals.recovery().unwrap().record.text, bytes);
    assert_eq!(
        a.modals.recovery().unwrap().record.path,
        Some(f.0.join("source.txt"))
    );
    Ok(())
}
#[test]
fn later_inspection_nested_focus_discard_and_corrupt_records_are_safe() -> io::Result<()> {
    use crossterm::event::{KeyCode as K, KeyModifiers as M};
    let f = support::Fixture::new("checkpoint-actions")?.canonicalized()?;
    let root = f.0.join("recovery");
    let p = abandoned(&root, &f.0)?;
    let mut a = f.app()?;
    recovery::start(&mut a, root.clone());
    wait(&mut a, |a| a.modals.recovery().is_some());
    support::draw(&mut a, 80, 25)?;
    support::key(&mut a, K::Tab);
    support::key(&mut a, K::Enter);
    assert!(a.native_help().is_some());
    support::key(&mut a, K::Esc);
    assert_eq!(a.modals.recovery().unwrap().focus, 1);
    support::modified(&mut a, K::Tab, M::SHIFT);
    assert_eq!(a.modals.recovery().unwrap().focus, 0);
    support::key(&mut a, K::Esc);
    assert!(a.modals.recovery().is_none());
    assert!(p.exists());
    menu::execute(&mut a, menu::Action::RecoverEditors);
    wait(&mut a, |a| a.modals.recovery().is_some());
    recovery::action(&mut a, R::Discard);
    settled(&mut a);
    assert!(!p.exists());
    drop(a);
    let p = abandoned(&root, &f.0)?;
    fs::write(&p, b"corrupt")?;
    let mut a = f.app()?;
    recovery::start(&mut a, root);
    wait(&mut a, |a| a.modals.recovery().is_some());
    assert!(a.modals.recovery().unwrap().error.is_some());
    recovery::action(&mut a, R::Recover);
    assert!(a.desktop.active_editor().is_none());
    recovery::action(&mut a, R::Discard);
    settled(&mut a);
    assert!(!p.exists());
    Ok(())
}
#[test]
fn save_close_and_cancelled_quit_handle_checkpoint_lifecycle() -> io::Result<()> {
    let f = support::Fixture::new("checkpoint-lifecycle")?.canonicalized()?;
    let root = f.0.join("recovery");
    let source = f.0.join("file");
    fs::write(&source, "original")?;
    let mut a = f.app()?;
    recovery::start(&mut a, root.clone());
    settled(&mut a);
    editor::open_path(&mut a, source.clone());
    support::wait(&mut a);
    editor::paste(&mut a, "changed");
    tick(&mut a);
    assert_eq!(files(&root).len(), 1);
    editor::action(&mut a, E::Save);
    support::wait(&mut a);
    tick(&mut a);
    assert!(files(&root).is_empty());
    editor::paste(&mut a, "again");
    tick(&mut a);
    assert_eq!(files(&root).len(), 1);
    a.request_exit();
    editor::action(&mut a, E::Cancel);
    assert!(!a.quit);
    tick(&mut a);
    assert_eq!(files(&root).len(), 1);
    editor::action(&mut a, E::Close);
    editor::action(&mut a, E::Discard);
    tick(&mut a);
    assert!(files(&root).is_empty());
    Ok(())
}
#[test]
fn quit_discard_retains_checkpoints_until_settings_checks_succeed() -> io::Result<()> {
    let f = support::Fixture::new("checkpoint-quit")?.canonicalized()?;
    let root = f.0.join("recovery");
    let mut a = f.app()?;
    recovery::start(&mut a, root.clone());
    settled(&mut a);
    editor::open(&mut a, true);
    editor::paste(&mut a, "retain me");
    tick(&mut a);
    a.config.save_on_exit = true;
    a.config_store = dn::config::Store::new(Some(f.0.join("missing/settings")));
    // Establish a valid baseline, then create an external change so exit saving fails.
    a.config_store.load()?;
    fs::create_dir_all(f.0.join("missing"))?;
    fs::write(f.0.join("missing/settings"), "external")?;
    a.request_exit();
    editor::action(&mut a, E::Discard);
    assert!(a.quit);
    assert!(!a.save_settings_on_exit());
    assert!(!a.quit);
    assert_eq!(files(&root).len(), 1);
    assert!(!recovery::finish(&mut a));
    a.config.save_on_exit = false;
    a.clear_dialogs();
    a.request_exit();
    editor::action(&mut a, E::Discard);
    assert!(a.quit);
    assert!(a.save_settings_on_exit());
    while !recovery::finish(&mut a) {
        a.poll();
    }
    assert!(files(&root).is_empty());
    Ok(())
}
#[cfg(unix)]
#[test]
fn recovery_refuses_links_and_public_directories_without_touching_targets() -> io::Result<()> {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let f = support::Fixture::new("checkpoint-safety")?.canonicalized()?;
    let root = f.0.join("root");
    fs::create_dir(&root)?;
    fs::set_permissions(&root, fs::Permissions::from_mode(0o755))?;
    assert!(store::Store::open(&root).is_err());
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
    let s = store::Store::open(&root)?;
    let target = f.0.join("target");
    fs::write(&target, "keep")?;
    let p = s.current.dir.join("1.rec");
    symlink(&target, &p)?;
    assert!(store::read(&p).is_err());
    assert!(store::write(&p, &record(&f.0)).is_err());
    assert!(store::remove(&p).is_err());
    assert_eq!(fs::read(&target)?, b"keep");
    Ok(())
}

#[test]
fn checkpoint_child_exits_without_destructors() -> io::Result<()> {
    let Some(base) = std::env::var_os("DN_RECOVERY_TEST_ROOT").map(PathBuf::from) else {
        return Ok(());
    };
    let root = base.join("recovery");
    let mut a = App::new(base.clone())?;
    a.config.save_on_exit = false;
    recovery::start(&mut a, root.clone());
    settled(&mut a);
    editor::open(&mut a, true);
    editor::paste(&mut a, "survive forced exit 界\n");
    tick(&mut a);
    // Leave an interrupted sibling write beside the completed checkpoint.
    use std::io::Write;
    let target = files(&root).pop().unwrap();
    let mut partial = dn::publication::StagedFile::create(&target)?;
    partial.file.write_all(b"partial new checkpoint")?;
    partial.file.sync_all()?;
    std::process::exit(86);
}
#[test]
fn process_exit_and_interrupted_checkpoint_preserve_last_complete_copy() -> io::Result<()> {
    let f = support::Fixture::new("checkpoint-process")?.canonicalized()?;
    let output = std::process::Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "checkpoint_child_exits_without_destructors",
            "--nocapture",
        ])
        .env("DN_RECOVERY_TEST_ROOT", &f.0)
        .output()?;
    assert_eq!(
        output.status.code(),
        Some(86),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let root = f.0.join("recovery");
    let p = files(&root).pop().unwrap();
    assert_eq!(store::read(&p)?.text, "survive forced exit 界\n");
    let mut a = f.app()?;
    recovery::start(&mut a, root);
    wait(&mut a, |a| a.modals.recovery().is_some());
    recovery::action(&mut a, R::Recover);
    assert_eq!(
        a.desktop
            .active_editor()
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .text,
        "survive forced exit 界\n"
    );
    Ok(())
}

#[test]
fn inactive_documents_checkpoint_without_activation_and_rejected_updates_keep_previous_copy(
) -> io::Result<()> {
    let f = support::Fixture::new("checkpoint-inactive")?.canonicalized()?;
    let root = f.0.join("recovery");
    let mut a = f.app()?;
    recovery::start(&mut a, root.clone());
    settled(&mut a);
    editor::open(&mut a, true);
    editor::paste(&mut a, "first");
    editor::open(&mut a, true);
    editor::paste(&mut a, "second");
    a.show_viewer(dn::viewer::Viewer::from_contents(
        "preview".into(),
        dn::viewer::from_bytes(b"view".to_vec(), dn::viewer::ViewMode::Text),
    ));
    let active = a.desktop.active().id;
    tick(&mut a);
    assert_eq!(a.desktop.active().id, active);
    let paths = files(&root);
    assert_eq!(paths.len(), 2);
    let mut values = paths
        .iter()
        .map(|p| store::read(p).unwrap().text)
        .collect::<Vec<_>>();
    values.sort();
    assert_eq!(values, ["first", "second"]);
    let p = &paths[0];
    let previous = fs::read(p)?;
    let mut bad = record(&f.0);
    bad.text = "x".repeat(dn::editor::buffer::MAX_BYTES + 1);
    assert!(store::write(p, &bad).is_err());
    assert_eq!(fs::read(p)?, previous);
    Ok(())
}
#[test]
fn recovery_frames_share_controls_across_languages_palettes_and_resize() -> io::Result<()> {
    use dn::{
        layout,
        localization::Language,
        mouse::Target,
        palette::{theme::Slot, PaletteProfile},
        presentation::layout::Rect,
        ui,
    };
    let f = support::Fixture::new("checkpoint-frames")?.canonicalized()?;
    for (index, language) in [Language::English, Language::Russian]
        .into_iter()
        .enumerate()
    {
        let root = f.0.join(format!("recovery-{index}"));
        abandoned(&root, &f.0)?;
        let mut a = f.app()?;
        a.config.language = language;
        a.config.show_clock = false;
        recovery::start(&mut a, root);
        wait(&mut a, |a| a.modals.recovery().is_some());
        for palette in [
            PaletteProfile::ClassicDn,
            PaletteProfile::ClassicDn.with_slot(Slot::DialogText, 0x1e),
        ] {
            a.config.palette = palette;
            for (w, h) in [(80, 25), (132, 40)] {
                let snap = layout::prepare(&mut a, Rect::new(0, 0, w, h));
                let first = ui::render(&a, &snap);
                let second = ui::render(&a, &snap);
                assert_eq!(
                    format!("{:?}", dn::testing::Buffer::rendered(first)),
                    format!("{:?}", dn::testing::Buffer::rendered(second))
                );
                support::draw(&mut a, w, h)?;
                assert!(a
                    .mouse
                    .regions
                    .iter()
                    .any(|(_, t)| *t == Target::Recovery(R::Recover)));
            }
        }
        support::click(&mut a, Target::Recovery(R::Later));
        assert!(a.modals.recovery().is_none());
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn cleanup_failure_keeps_data_and_allows_acknowledged_exit() -> io::Result<()> {
    use std::os::unix::fs::symlink;
    let f = support::Fixture::new("checkpoint-cleanup")?.canonicalized()?;
    let root = f.0.join("recovery");
    let mut a = f.app()?;
    recovery::start(&mut a, root.clone());
    settled(&mut a);
    editor::open(&mut a, true);
    editor::paste(&mut a, "unsaved");
    tick(&mut a);
    let p = files(&root).pop().unwrap();
    let keep = f.0.join("retained");
    fs::rename(&p, &keep)?;
    symlink(&keep, &p)?;
    a.request_exit();
    editor::action(&mut a, E::Discard);
    assert!(a.quit);
    assert!(!recovery::finish(&mut a));
    assert!(a.last_dialog().unwrap().message.contains("cleanup failed"));
    assert!(keep.exists());
    a.pop_dialog();
    assert!(recovery::finish(&mut a));
    assert!(keep.exists());
    assert!(p.is_symlink());
    Ok(())
}
