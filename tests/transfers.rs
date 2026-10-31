//! Transfer service and native view tests use private filesystem fixtures only.
mod support;
use crossterm::event::{KeyCode as K, KeyModifiers as M};
use dn::{
    app::App,
    desktop::{WindowId, WindowKind},
    operations::{CopyOptions, ExistingDestination, Kind, Plan},
    transfers::{self, Action, Status},
};
use std::{fs, io, path::Path, time::Duration};
fn plan(root: &Path, name: &str, dest: &str) -> Plan {
    Plan {
        kind: Kind::Copy,
        sources: vec![root.join(name)],
        destination: root.join(dest),
    }
}
fn wait(app: &mut App, ready: impl Fn(&App) -> bool) {
    support::wait_until(app, Duration::from_secs(8), ready);
}

#[test]
fn queue_runs_serially_without_focus_or_unrelated_selection_changes_and_view_can_close(
) -> io::Result<()> {
    let f = support::Fixture::new("transfers-background")?.canonicalized()?;
    fs::write(f.0.join("one"), "one 界")?;
    fs::write(f.0.join("two"), "two")?;
    fs::create_dir(f.0.join("out"))?;
    let mut a = f.app()?;
    a.transfers.paused = true;
    let one = transfers::submit(&mut a, plan(&f.0, "one", "out"), CopyOptions::default()).unwrap();
    let window = a.desktop.active().id;
    assert_eq!(
        a.desktop.active().kind,
        WindowKind::Tool(dn::tool_windows::Kind::Transfers)
    );
    let two = transfers::submit(&mut a, plan(&f.0, "two", "out"), CopyOptions::default()).unwrap();
    assert_eq!(a.desktop.active().id, window);
    assert_ne!(one, two);
    wait(&mut a, |a| {
        a.transfers
            .entries
            .iter()
            .all(|e| e.status == Status::Queued)
    });
    assert!(!f.0.join("out/one").exists());
    support::draw(&mut a, 100, 32)?;
    support::key(&mut a, K::Esc);
    assert!(!a.desktop.contains(window));
    dn::editor::open(&mut a, true);
    dn::editor::paste(&mut a, "foreground draft");
    let foreground = a.desktop.active().id;
    a.panels[0].selected.insert("two".into());
    transfers::action(&mut a, Action::Resume);
    wait(&mut a, |a| !a.transfers.has_work());
    assert_eq!(a.desktop.active().id, foreground);
    assert!(a.panels[0].selected.contains(std::ffi::OsStr::new("two")));
    assert_eq!(fs::read(f.0.join("out/one"))?, "one 界".as_bytes());
    assert_eq!(fs::read(f.0.join("out/two"))?, b"two");
    assert!(a
        .transfers
        .entries
        .iter()
        .all(|e| e.status == Status::Completed));
    assert!(a.dialogs().is_empty());
    assert!(a.modal_infos().is_empty());
    transfers::open(&mut a);
    assert_ne!(a.desktop.active().id, window);
    assert_eq!(a.transfers.entries.len(), 2);
    Ok(())
}
#[test]
fn conflicts_pause_remaining_queue_and_resume_never_retries_failed_entries() -> io::Result<()> {
    let f = support::Fixture::new("transfers-conflict")?.canonicalized()?;
    fs::write(f.0.join("one"), "new")?;
    fs::write(f.0.join("two"), "second")?;
    fs::create_dir(f.0.join("out"))?;
    fs::write(f.0.join("out/one"), "keep")?;
    let mut a = f.app()?;
    a.transfers.paused = true;
    let first =
        transfers::submit(&mut a, plan(&f.0, "one", "out"), CopyOptions::default()).unwrap();
    transfers::submit(&mut a, plan(&f.0, "two", "out"), CopyOptions::default()).unwrap();
    wait(&mut a, |a| {
        a.transfers
            .entries
            .iter()
            .all(|e| e.status == Status::Queued)
    });
    transfers::action(&mut a, Action::Resume);
    wait(&mut a, |a| {
        a.transfers
            .entries
            .iter()
            .any(|e| e.id == first && e.status == Status::Failed)
    });
    assert!(a.transfers.paused);
    assert!(!f.0.join("out/two").exists());
    assert_eq!(fs::read(f.0.join("out/one"))?, b"keep");
    assert!(a.modal_infos().is_empty());
    transfers::action(&mut a, Action::Resume);
    wait(&mut a, |a| !a.transfers.has_work());
    assert_eq!(fs::read(f.0.join("out/two"))?, b"second");
    assert_eq!(a.transfers.entries[0].status, Status::Failed);
    assert_eq!(a.transfers.entries[1].status, Status::Completed);
    Ok(())
}
#[test]
fn queued_source_and_destination_replacement_fail_before_mutation() -> io::Result<()> {
    for replace_source in [false, true] {
        let f = support::Fixture::new("transfers-identity")?.canonicalized()?;
        fs::write(f.0.join("source"), "original")?;
        fs::create_dir(f.0.join("out"))?;
        let mut a = f.app()?;
        a.transfers.paused = true;
        transfers::submit(&mut a, plan(&f.0, "source", "out"), CopyOptions::default()).unwrap();
        wait(&mut a, |a| a.transfers.entries[0].status == Status::Queued);
        if replace_source {
            let modified = fs::metadata(f.0.join("source"))?.modified()?;
            fs::rename(f.0.join("source"), f.0.join("original"))?;
            fs::write(f.0.join("source"), "original")?;
            fs::OpenOptions::new()
                .write(true)
                .open(f.0.join("source"))?
                .set_times(fs::FileTimes::new().set_modified(modified))?;
        } else {
            fs::rename(f.0.join("out"), f.0.join("old-out"))?;
            fs::create_dir(f.0.join("out"))?;
        }
        transfers::action(&mut a, Action::Resume);
        wait(&mut a, |a| a.transfers.entries[0].status.terminal());
        assert_eq!(a.transfers.entries[0].status, Status::Failed);
        assert!(!f.0.join("out/source").exists());
        assert!(a.transfers.paused);
    }
    Ok(())
}
#[test]
fn skip_and_queued_cancel_keep_files_and_explain_results() -> io::Result<()> {
    let f = support::Fixture::new("transfers-cancel")?.canonicalized()?;
    fs::write(f.0.join("one"), "source")?;
    fs::create_dir(f.0.join("out"))?;
    fs::write(f.0.join("out/one"), "keep")?;
    let mut a = f.app()?;
    a.transfers.paused = true;
    let id = transfers::submit(
        &mut a,
        plan(&f.0, "one", "out"),
        CopyOptions {
            existing: ExistingDestination::Skip,
            ..Default::default()
        },
    )
    .unwrap();
    let cancel =
        transfers::submit(&mut a, plan(&f.0, "one", "never"), CopyOptions::default()).unwrap();
    transfers::action(&mut a, Action::Select(cancel));
    transfers::action(&mut a, Action::Cancel);
    assert_eq!(a.transfers.selected().unwrap().status, Status::Cancelled);
    assert!(a.transfers.paused);
    wait(&mut a, |a| {
        a.transfers
            .entries
            .iter()
            .find(|e| e.id == id)
            .unwrap()
            .status
            == Status::Queued
    });
    transfers::action(&mut a, Action::Resume);
    wait(&mut a, |a| !a.transfers.has_work());
    let result = a.transfers.entries.iter().find(|e| e.id == id).unwrap();
    assert_eq!(result.status, Status::Completed);
    assert_eq!(result.progress.skipped, 1);
    assert_eq!(result.progress.details.len(), 1);
    assert_eq!(fs::read(f.0.join("one"))?, b"source");
    assert_eq!(fs::read(f.0.join("out/one"))?, b"keep");
    assert!(!f.0.join("never").exists());
    Ok(())
}
#[test]
fn transfer_view_is_native_immutable_localized_and_switchable_at_both_sizes() -> io::Result<()> {
    for (w, h, language) in [
        (80, 25, dn::localization::Language::English),
        (120, 40, dn::localization::Language::Russian),
    ] {
        let f = support::Fixture::new("transfers-native")?.canonicalized()?;
        fs::write(f.0.join("source 界"), "data")?;
        let mut a = f.app()?;
        a.config.language = language;
        if language == dn::localization::Language::Russian {
            a.config.palette = a
                .config
                .palette
                .with_slot(dn::palette::theme::Slot::DialogText, 0x1e)
                .with_slot(dn::palette::theme::Slot::DialogButton, 0x2f);
        }
        a.transfers.paused = true;
        transfers::submit(
            &mut a,
            plan(&f.0, "source 界", "out"),
            CopyOptions::default(),
        )
        .unwrap();
        wait(&mut a, |a| a.transfers.entries[0].status == Status::Queued);
        let id = a.desktop.active().id;
        let snapshot = dn::layout::prepare(&mut a, dn::presentation::layout::Rect::new(0, 0, w, h));
        let first = dn::ui::render(&a, &snapshot);
        let second = dn::ui::render(&a, &snapshot);
        assert_eq!(first.hit_regions(), second.hit_regions());
        assert_eq!(
            dn::testing::Buffer::rendered(first),
            dn::testing::Buffer::rendered(second)
        );
        support::draw(&mut a, w, h)?;
        support::modified(&mut a, K::F(6), M::ALT);
        assert_eq!(a.desktop.active().id, WindowId::MANAGER);
        support::draw(&mut a, w, h)?;
        let indicator = a
            .mouse
            .regions
            .iter()
            .find(|(_, t)| *t == dn::mouse::Target::Transfer(Action::Open))
            .unwrap()
            .0;
        assert!(indicator.width > 0);
        support::click(&mut a, dn::mouse::Target::Transfer(Action::Open));
        assert_eq!(a.desktop.active().id, id);
        let s = dn::layout::prepare(&mut a, dn::presentation::layout::Rect::new(0, 0, w, h))
            .surfaces
            .into_iter()
            .find(|s| s.id.key == dn::windowing::geometry::SurfaceKey::Document(id))
            .unwrap();
        assert!(s.caps.resize && s.caps.tile && s.caps.cascade);
        transfers::action(&mut a, Action::Cancel);
    }
    Ok(())
}
#[test]
fn transfer_quit_default_returns_and_cancel_wait_for_acknowledgement() -> io::Result<()> {
    for cancel in [false, true] {
        let f = support::Fixture::new("transfers-quit")?.canonicalized()?;
        fs::write(f.0.join("one"), "data")?;
        let mut a = f.app()?;
        a.transfers.paused = true;
        transfers::submit(&mut a, plan(&f.0, "one", "out"), CopyOptions::default()).unwrap();
        wait(&mut a, |a| a.transfers.entries[0].status == Status::Queued);
        a.request_exit();
        support::draw(&mut a, 100, 32)?;
        assert!(!a.quit);
        assert!(a.modal_top().is_some());
        support::key(&mut a, K::Enter);
        assert!(!a.quit);
        assert!(a.quit_review.is_none());
        assert!(a.transfers.has_work());
        a.request_exit();
        support::draw(&mut a, 100, 32)?;
        transfers::action(
            &mut a,
            if cancel {
                Action::QuitCancel
            } else {
                Action::QuitWait
            },
        );
        wait(&mut a, |a| a.quit);
        assert_eq!(f.0.join("out").exists(), !cancel);
        assert!(!a.transfers.has_work());
    }
    Ok(())
}

#[test]
fn queued_limit_blocks_only_mutating_legacy_jobs_and_ids_are_not_reused() -> io::Result<()> {
    let f = support::Fixture::new("transfers-bounds")?.canonicalized()?;
    fs::write(f.0.join("source"), "data")?;
    let mut a = f.app()?;
    a.transfers.paused = true;
    let mut ids = Vec::new();
    for i in 0..64 {
        ids.push(
            transfers::submit(
                &mut a,
                plan(&f.0, "source", &format!("out-{i}")),
                CopyOptions::default(),
            )
            .unwrap(),
        );
    }
    assert!(transfers::submit(
        &mut a,
        plan(&f.0, "source", "overflow"),
        CopyOptions::default()
    )
    .is_err());
    a.prompt_operation(Kind::Mkdir, false);
    assert!(a.modals.prompt().is_none());
    for id in &ids {
        transfers::action(&mut a, Action::Select(*id));
        transfers::action(&mut a, Action::Cancel);
    }
    assert!(!a.transfers.has_work());
    transfers::action(&mut a, Action::Clear);
    assert!(a.transfers.entries.is_empty());
    let next =
        transfers::submit(&mut a, plan(&f.0, "source", "new"), CopyOptions::default()).unwrap();
    assert!(!ids.contains(&next));
    transfers::action(&mut a, Action::Select(ids[0]));
    assert_eq!(a.transfers.selected().unwrap().id, next);
    transfers::action(&mut a, Action::Cancel);
    assert!(!f.0.join("new").exists());
    Ok(())
}
#[test]
fn origin_tracks_panel_swaps_and_only_clears_the_submitted_selection() -> io::Result<()> {
    let f = support::Fixture::new("transfers-panel-context")?.canonicalized()?;
    fs::write(f.0.join("source"), "data")?;
    fs::write(f.0.join("unrelated"), "other")?;
    fs::create_dir(f.0.join("out"))?;
    let mut a = f.app()?;
    a.transfers.paused = true;
    a.panels[0].selected.insert("source".into());
    transfers::submit(&mut a, plan(&f.0, "source", "out"), CopyOptions::default()).unwrap();
    wait(&mut a, |a| a.transfers.entries[0].status == Status::Queued);
    dn::menu::execute(&mut a, dn::menu::Action::SwapPanels);
    a.active = 0;
    a.panels[0].selected.insert("unrelated".into());
    a.desktop.activate(WindowId::MANAGER);
    a.sync_modals();
    transfers::action(&mut a, Action::Resume);
    wait(&mut a, |a| {
        !a.transfers.has_work() && a.panels.iter().all(|p| !p.loading)
    });
    assert!(a.panels[0]
        .selected
        .contains(std::ffi::OsStr::new("unrelated")));
    assert!(!a.panels[1]
        .selected
        .contains(std::ffi::OsStr::new("source")));
    assert_eq!(a.desktop.active().id, WindowId::MANAGER);
    Ok(())
}
#[test]
fn native_and_verified_moves_keep_correct_io_counts_and_source_cleanup() -> io::Result<()> {
    for verified in [false, true] {
        let f = support::Fixture::new("transfers-move")?.canonicalized()?;
        fs::write(f.0.join("source"), "move bytes")?;
        let mut a = f.app()?;
        let mut p = plan(&f.0, "source", "moved");
        p.kind = Kind::Move;
        transfers::submit(
            &mut a,
            p,
            CopyOptions {
                verify_writes: verified,
                ..Default::default()
            },
        )
        .unwrap();
        wait(&mut a, |a| !a.transfers.has_work());
        let e = &a.transfers.entries[0];
        assert_eq!(e.status, Status::Completed);
        assert_eq!(e.progress.written, if verified { 10 } else { 0 });
        assert!(!f.0.join("source").exists());
        assert_eq!(fs::read(f.0.join("moved"))?, b"move bytes");
    }
    Ok(())
}
#[test]
fn dirty_quit_return_and_failed_settings_save_retain_buffers_while_transfers_finish(
) -> io::Result<()> {
    let f = support::Fixture::new("transfers-quit-dirty")?.canonicalized()?;
    fs::write(f.0.join("source"), "data")?;
    let mut a = f.app()?;
    a.transfers.paused = true;
    transfers::submit(&mut a, plan(&f.0, "source", "out"), CopyOptions::default()).unwrap();
    wait(&mut a, |a| a.transfers.entries[0].status == Status::Queued);
    dn::editor::open(&mut a, true);
    dn::editor::paste(&mut a, "retained dirty draft");
    let editor = a.desktop.active().id;
    a.request_exit();
    dn::editor::action(&mut a, dn::editor::EditorAction::Cancel);
    assert!(!a.quit);
    assert!(a.transfers.has_work());
    a.request_exit();
    dn::editor::action(&mut a, dn::editor::EditorAction::Discard);
    support::draw(&mut a, 100, 32)?;
    assert!(a.dialogs().iter().any(|d| d.title == " Transfers pending "));
    transfers::action(&mut a, Action::QuitWait);
    wait(&mut a, |a| a.quit);
    a.config.save_on_exit = true;
    a.config_store = dn::config::Store::new(Some(f.0.join("config.conf")));
    a.config_store.load()?;
    fs::write(f.0.join("config.conf"), "external change")?;
    assert!(!a.save_settings_on_exit());
    assert!(!a.quit);
    assert!(a.desktop.contains(editor));
    assert_eq!(
        a.desktop
            .editor(editor)
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .text,
        "retained dirty draft"
    );
    assert_eq!(fs::read(f.0.join("out"))?, b"data");
    Ok(())
}

#[cfg(unix)]
#[test]
fn queued_symlink_parent_retargeting_is_rejected_without_writing_either_target() -> io::Result<()> {
    use std::os::unix::fs::symlink;
    for source_link in [false, true] {
        let f = support::Fixture::new("transfers-retarget")?.canonicalized()?;
        for dir in ["a", "b", "out"] {
            fs::create_dir(f.0.join(dir))?;
        }
        fs::write(f.0.join("a/source"), "first")?;
        fs::write(f.0.join("b/source"), "second")?;
        symlink(f.0.join("a"), f.0.join("link"))?;
        let mut a = f.app()?;
        a.transfers.paused = true;
        let p = if source_link {
            plan(&f.0, "link/source", "out")
        } else {
            plan(&f.0, "a/source", "link")
        };
        transfers::submit(&mut a, p, CopyOptions::default()).unwrap();
        wait(&mut a, |a| a.transfers.entries[0].status == Status::Queued);
        fs::remove_file(f.0.join("link"))?;
        symlink(f.0.join("b"), f.0.join("link"))?;
        transfers::action(&mut a, Action::Resume);
        wait(&mut a, |a| a.transfers.entries[0].status.terminal());
        assert_eq!(a.transfers.entries[0].status, Status::Failed);
        assert_eq!(fs::read(f.0.join("a/source"))?, b"first");
        assert_eq!(fs::read(f.0.join("b/source"))?, b"second");
        assert!(!f.0.join("out/source").exists());
    }
    Ok(())
}
