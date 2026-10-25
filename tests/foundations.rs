use dn::{
    app::App,
    desktop::{WindowId, WindowKind},
    editor::{self, EditorAction as A},
    focus::{self, Focus},
    presentation::layout::Rect,
    requests::{Owner, RequestScope},
    ui,
};
use std::{
    fs, io,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};
struct Temp {
    path: PathBuf,
    _guard: std::sync::MutexGuard<'static, ()>,
}
impl Temp {
    fn app() -> io::Result<(Self, App)> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        // Each scenario deliberately fills the bounded I/O pool; avoid making
        // unrelated fixtures compete for that capacity.
        static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let guard = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
        let root = Self {
            path: std::env::temp_dir().join(format!(
                "dn-foundations-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            )),
            _guard: guard,
        };
        fs::create_dir(&root.path)?;
        fs::write(root.path.join("a.txt"), "alpha\n")?;
        fs::write(root.path.join("b.txt"), "beta\n")?;
        let mut app = App::new(root.path.clone())?;
        app.config.save_on_exit = false;
        wait(&mut app);
        Ok((root, app))
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
fn wait(app: &mut App) {
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        app.poll();
        let busy = app.panels.iter().any(|p| p.loading)
            || app.desktop.windows().any(|w| {
                app.desktop.identity_pending(w.id)
                    || app.desktop.editor(w.id).is_some_and(|e| e.busy())
                    || app
                        .desktop
                        .viewer(w.id)
                        .is_some_and(|v| v.contents.is_none())
            });
        if !busy {
            break;
        }
        assert!(Instant::now() < end, "background timeout");
        thread::sleep(Duration::from_millis(1));
    }
}
fn text(app: &App, id: WindowId) -> &str {
    &app.desktop
        .editor(id)
        .unwrap()
        .buffer
        .as_ref()
        .unwrap()
        .text
}
#[test]
fn documents_keep_independent_buffers_and_reuse_resolved_paths() -> io::Result<()> {
    let (root, mut app) = Temp::app()?;
    editor::open_path(&mut app, root.path.join("a.txt"));
    let a = app.desktop.active().id;
    editor::open_path(&mut app, root.path.join("b.txt"));
    let b = app.desktop.active().id;
    wait(&mut app);
    assert_eq!(text(&app, a), "alpha\n");
    assert_eq!(text(&app, b), "beta\n");
    assert_eq!(app.desktop.active().id, b);
    app.desktop.activate(a);
    editor::paste(&mut app, "A");
    app.desktop.editor_mut(a).unwrap().search = "alpha".into();
    app.desktop.editor_mut(a).unwrap().scroll_column = 4;
    app.desktop.activate(b);
    editor::paste(&mut app, "B");
    app.desktop.editor_mut(b).unwrap().search = "beta".into();
    assert_eq!(app.desktop.editor(a).unwrap().search, "alpha");
    assert_eq!(app.desktop.editor(a).unwrap().scroll_column, 4);
    app.desktop.editor_mut(b).unwrap().scroll_column = 6;
    assert_eq!(app.desktop.editor(a).unwrap().scroll_column, 4);
    assert_eq!(app.desktop.editor(b).unwrap().scroll_column, 6);
    editor::action(&mut app, A::ExpandTabs);
    assert_eq!(
        app.desktop.editor(a).unwrap().preferences.expand_tabs,
        app.config.editor.expand_tabs
    );
    assert_eq!(
        app.desktop.editor(b).unwrap().preferences.expand_tabs,
        app.config.editor.expand_tabs
    );
    assert_ne!(text(&app, a), text(&app, b));
    editor::open_path(&mut app, root.path.join("a.txt"));
    assert_eq!(app.desktop.active().id, a);
    assert_eq!(app.desktop.document_ids(WindowKind::Editor).len(), 2);
    editor::action(&mut app, A::Undo);
    assert_eq!(text(&app, a), "alpha\n");
    assert!(text(&app, b).contains('B'));
    dn::windowing::open(&mut app);
    assert_eq!(app.modals.window_list().unwrap().ids.len(), 3);
    Ok(())
}
#[test]
fn cancelling_a_later_quit_review_keeps_discard_approved_buffers() -> io::Result<()> {
    let (_, mut app) = Temp::app()?;
    editor::open(&mut app, true);
    let a = app.desktop.active().id;
    editor::paste(&mut app, "unsaved a");
    editor::open(&mut app, true);
    let b = app.desktop.active().id;
    editor::paste(&mut app, "unsaved b");
    app.request_exit();
    assert_eq!(app.desktop.active().id, b);
    assert_eq!(focus::current(&app), Focus::EditorModal);
    editor::action(&mut app, A::Discard);
    assert_eq!(app.desktop.active().id, a);
    assert!(app.desktop.contains(b));
    editor::action(&mut app, A::Cancel);
    assert!(!app.quit);
    assert!(app.quit_review.is_none());
    assert_eq!(text(&app, a), "unsaved a");
    assert_eq!(text(&app, b), "unsaved b");
    app.request_exit();
    editor::action(&mut app, A::Discard);
    editor::action(&mut app, A::Discard);
    assert!(app.quit);
    assert!(app.desktop.contains(a) && app.desktop.contains(b));
    Ok(())
}
#[test]
fn save_as_cannot_overwrite_another_open_editor() -> io::Result<()> {
    let (root, mut app) = Temp::app()?;
    editor::open_path(&mut app, root.path.join("a.txt"));
    let a = app.desktop.active().id;
    editor::open_path(&mut app, root.path.join("b.txt"));
    let b = app.desktop.active().id;
    wait(&mut app);
    editor::paste(&mut app, "changed");
    editor::action(&mut app, A::SaveAs);
    editor::paste(&mut app, root.path.join("a.txt").to_str().unwrap());
    editor::action(&mut app, A::Submit);
    wait(&mut app);
    assert_eq!(fs::read_to_string(root.path.join("a.txt"))?, "alpha\n");
    assert_eq!(text(&app, a), "alpha\n");
    assert!(text(&app, b).contains("changed"));
    assert!(app
        .last_dialog()
        .unwrap()
        .message
        .contains("another editor"));
    Ok(())
}
#[test]
fn nested_help_restores_its_owner_confirmation_and_stale_hits_are_rejected() -> io::Result<()> {
    let (_, mut app) = Temp::app()?;
    editor::open(&mut app, true);
    editor::paste(&mut app, "dirty");
    app.request_exit();
    let parent = app.modal_top().unwrap();
    editor::action(&mut app, A::Help);
    let child = app.modal_top().unwrap();
    assert_ne!(parent.id, child.id);
    app.sync_modals();
    assert_eq!(app.modal_top().unwrap().parent, Some(parent.id));
    editor::action(&mut app, A::Cancel);
    assert_eq!(app.modal_top().unwrap().id, parent.id);
    assert!(app.quit_review.is_some());
    editor::action(&mut app, A::Cancel);
    assert!(app.quit_review.is_none());
    app.error("parent message\n".repeat(30));
    let parent = app.modal_top().unwrap().id;
    app.set_message_offset(3);
    app.error("child message");
    assert_eq!(app.message_offset(), 0);
    app.pop_dialog();
    app.sync_modals();
    assert_eq!(app.modal_top().unwrap().id, parent);
    assert_eq!(
        app.message_offset(),
        3,
        "nested messages retain their parent's scroll"
    );
    Ok(())
}
#[test]
fn rendering_is_read_only_and_local_coordinates_transform_together() -> io::Result<()> {
    let (_, mut app) = Temp::app()?;
    let snapshot = dn::layout::prepare_window(
        &mut app,
        Rect::new(7, 3, 80, 25),
        Rect::new(0, 0, 100, 35),
        Instant::now(),
    );
    let before = (
        app.panels[0].offset,
        app.panels[0].revision,
        app.command_line.text.clone(),
        app.layout_revision,
    );
    let first = ui::render(&app, &snapshot);
    let hits = first.hit_regions().to_vec();
    let cursor = first.cursor();
    let second = ui::render(&app, &snapshot);
    assert_eq!(
        dn::testing::Buffer::rendered(first),
        dn::testing::Buffer::rendered(second)
    );
    assert_eq!(
        before,
        (
            app.panels[0].offset,
            app.panels[0].revision,
            app.command_line.text.clone(),
            app.layout_revision
        )
    );
    assert!(hits
        .iter()
        .all(|(r, _)| r.x >= 7 && r.y >= 3 && r.right() <= 87 && r.bottom() <= 28));
    assert!(cursor.is_some_and(|(x, y)| x >= 7 && y == 26));
    Ok(())
}
#[test]
fn request_generations_and_retired_scopes_reject_old_replies() -> io::Result<()> {
    let scope = RequestScope::new(Owner::Panel(0));
    let old = scope.start("load");
    let new = scope.start("load");
    assert!(!scope.accepts(old));
    assert!(scope.accepts(new));
    let (send, recv) = std::sync::mpsc::channel();
    let (started, ready) = std::sync::mpsc::channel();
    let task = scope.spawn("delayed", move |_| {
        started.send(()).unwrap();
        recv.recv().unwrap();
        42
    })?;
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    drop(scope);
    send.send(()).ok();
    assert!(task.token().is_none());
    assert!(task.try_recv().is_err());
    Ok(())
}
#[cfg(unix)]
#[test]
fn canonical_aliases_merge_clean_placeholders_without_losing_edits() -> io::Result<()> {
    let (root, mut app) = Temp::app()?;
    editor::open_path(&mut app, root.path.join("a.txt"));
    let first = app.desktop.active().id;
    wait(&mut app);
    editor::paste(&mut app, "dirty");
    std::os::unix::fs::symlink(&root.path, root.path.join("alias"))?;
    editor::open_path(&mut app, root.path.join("alias/a.txt"));
    wait(&mut app);
    assert_eq!(app.desktop.document_ids(WindowKind::Editor), [first]);
    assert!(text(&app, first).contains("dirty"));
    assert_eq!(app.desktop.active().id, first);
    Ok(())
}

fn save_as(app: &mut App, path: &std::path::Path) {
    editor::action(app, A::SaveAs);
    if let Some(editor::Modal::File(picker)) = app
        .desktop
        .active_editor_mut()
        .unwrap()
        .modals
        .editor_modal_mut()
    {
        picker.set_filename(path.to_string_lossy().into_owned());
    } else {
        panic!("Save As prompt");
    }
    editor::action(app, A::Submit);
}
#[test]
fn save_reservations_coalesce_opens_reject_competitors_and_release_after_failure() -> io::Result<()>
{
    let (root, mut app) = Temp::app()?;
    editor::open(&mut app, true);
    let first = app.desktop.active().id;
    editor::paste(&mut app, "first");
    let destination = root.path.join("new.txt");
    save_as(&mut app, &destination);
    editor::open_path(&mut app, destination.clone());
    assert_eq!(
        app.desktop.active().id,
        first,
        "reserved destination belongs to saving editor"
    );
    editor::open(&mut app, true);
    let second = app.desktop.active().id;
    editor::paste(&mut app, "second");
    save_as(&mut app, &destination);
    assert!(app
        .last_dialog()
        .unwrap()
        .message
        .contains("another editor"));
    app.clear_dialogs();
    wait(&mut app);
    assert_eq!(
        app.desktop.active().id,
        second,
        "inactive save cannot activate its owner"
    );
    assert_eq!(fs::read_to_string(&destination)?, "first");
    assert!(!app.desktop.editor(first).unwrap().dirty());
    let failed = root.path.join("absent/out.txt");
    save_as(&mut app, &failed);
    wait(&mut app);
    assert!(app.desktop.editor(second).unwrap().dirty());
    app.clear_dialogs();
    fs::create_dir(root.path.join("absent"))?;
    editor::open(&mut app, true);
    editor::paste(&mut app, "new owner after failure");
    save_as(&mut app, &failed);
    wait(&mut app);
    assert!(app.dialogs().is_empty());
    assert_eq!(fs::read_to_string(failed)?, "new owner after failure");
    assert_eq!(text(&app, second), "second");
    Ok(())
}
#[test]
fn viewers_load_independently_reuse_paths_reload_explicit_modes_and_close_by_id() -> io::Result<()>
{
    use dn::viewer::{ViewMode, Viewer};
    let (root, mut app) = Temp::app()?;
    app.show_viewer(Viewer::open(root.path.join("a.txt"))?);
    let a = app.desktop.active().id;
    app.show_viewer(Viewer::open(root.path.join("b.txt"))?);
    let b = app.desktop.active().id;
    app.desktop.activate(WindowId::MANAGER);
    wait(&mut app);
    assert_eq!(app.desktop.active().id, WindowId::MANAGER);
    assert_eq!(
        app.desktop
            .viewer(a)
            .unwrap()
            .contents
            .as_ref()
            .unwrap()
            .lines[0],
        "alpha"
    );
    assert_eq!(
        app.desktop
            .viewer(b)
            .unwrap()
            .contents
            .as_ref()
            .unwrap()
            .lines[0],
        "beta"
    );
    app.desktop.viewer_mut(a).unwrap().offset = 3;
    app.desktop.viewer_mut(a).unwrap().horizontal = 7;
    app.show_viewer(Viewer::open(root.path.join("a.txt"))?);
    assert_eq!(app.desktop.active().id, a);
    assert_eq!(app.desktop.viewer(a).unwrap().horizontal, 7);
    app.show_viewer(Viewer::open_mode(root.path.join("a.txt"), ViewMode::Hex)?);
    wait(&mut app);
    assert_eq!(app.desktop.active().id, a);
    assert!(
        app.desktop
            .viewer(a)
            .unwrap()
            .contents
            .as_ref()
            .unwrap()
            .binary
    );
    fs::write(root.path.join("a.txt"), "changed")?;
    app.show_viewer(Viewer::open_mode(root.path.join("a.txt"), ViewMode::Hex)?);
    wait(&mut app);
    assert_eq!(
        app.desktop
            .viewer(a)
            .unwrap()
            .contents
            .as_ref()
            .unwrap()
            .lines,
        dn::viewer::from_bytes(b"changed".to_vec(), ViewMode::Hex).lines
    );
    assert_eq!(app.desktop.document_ids(WindowKind::Viewer).len(), 2);
    app.desktop.close(b);
    assert_eq!(app.desktop.active().id, a);
    assert!(!app.desktop.activate(b));
    Ok(())
}
#[test]
fn inactive_load_errors_stay_with_their_document_and_owner_close_retires_children() -> io::Result<()>
{
    let (root, mut app) = Temp::app()?;
    editor::open_path(&mut app, root.path.join("missing"));
    let failed = app.desktop.active().id;
    app.desktop.activate(WindowId::MANAGER);
    wait(&mut app);
    assert_eq!(app.desktop.active().id, WindowId::MANAGER);
    assert!(app.dialogs().is_empty());
    app.desktop.activate(failed);
    assert!(!app.last_dialog().unwrap().message.is_empty());
    let modal = app.modal_top().unwrap().id;
    app.desktop.close(failed);
    app.sync_modals();
    assert!(app.modal_infos().iter().all(|m| m.id != modal));
    assert_eq!(focus::current(&app), Focus::Manager);
    Ok(())
}
#[test]
fn quit_settings_failure_keeps_every_discarded_buffer_and_completed_save() -> io::Result<()> {
    let (root, mut app) = Temp::app()?;
    editor::open_path(&mut app, root.path.join("a.txt"));
    let saved = app.desktop.active().id;
    wait(&mut app);
    editor::paste(&mut app, "saved ");
    editor::open(&mut app, true);
    let discarded = app.desktop.active().id;
    editor::paste(&mut app, "retain discarded");
    app.config.save_on_exit = true;
    let settings = root.path.join("settings.toml");
    app.config_store = dn::config::Store::new(Some(settings.clone()));
    app.save_config();
    fs::write(
        &settings,
        "config_version=2\n[file_manager]\nhidden=false\n",
    )?;
    app.config.show_clock = !app.config.show_clock;
    app.request_exit();
    editor::action(&mut app, A::Discard);
    assert_eq!(app.desktop.active().id, saved);
    editor::action(&mut app, A::Save);
    wait(&mut app);
    assert!(app.quit);
    assert!(!app.save_settings_on_exit());
    assert!(!app.quit && app.quit_review.is_none());
    assert_eq!(text(&app, discarded), "retain discarded");
    assert!(app.desktop.editor(discarded).unwrap().dirty());
    assert_eq!(text(&app, saved), "saved alpha\n");
    assert!(!app.desktop.editor(saved).unwrap().dirty());
    assert_eq!(
        fs::read_to_string(root.path.join("a.txt"))?,
        "saved alpha\n"
    );
    Ok(())
}
#[test]
fn window_list_scrolls_in_creation_order_with_stable_untitled_labels() -> io::Result<()> {
    use dn::testing::{backend::TestBackend, Terminal};
    let (_, mut app) = Temp::app()?;
    let mut ids = vec![WindowId::MANAGER];
    for _ in 0..20 {
        editor::open(&mut app, true);
        ids.push(app.desktop.active().id);
    }
    app.desktop.activate(ids[2]);
    dn::windowing::open(&mut app);
    let mut terminal = Terminal::new(TestBackend::new(80, 25))?;
    terminal.draw(|f| ui::draw(f, &mut app))?;
    assert_eq!(app.modals.window_list().unwrap().ids, ids);
    dn::windowing::action(&mut app, dn::windowing::Action::Row(*ids.last().unwrap()));
    assert!(app.modals.window_list().unwrap().offset > 0);
    terminal.draw(|f| ui::draw(f, &mut app))?;
    let output = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(output.contains(&format!("New file {}", ids.last().unwrap())));
    app.desktop.close(ids[3]);
    dn::windowing::poll(&mut app);
    assert_eq!(app.modals.window_list().unwrap().ids.last(), ids.last());
    dn::windowing::action(&mut app, dn::windowing::Action::Switch);
    assert_eq!(app.desktop.active().id, *ids.last().unwrap());
    Ok(())
}
#[cfg(unix)]
#[test]
fn pending_alias_opens_coalesce_without_stealing_focus_and_honor_viewer_mode() -> io::Result<()> {
    use dn::viewer::{ViewMode, Viewer};
    let (root, mut app) = Temp::app()?;
    std::os::unix::fs::symlink(&root.path, root.path.join("alias"))?;
    editor::open_path(&mut app, root.path.join("alias/a.txt"));
    let first = app.desktop.active().id;
    editor::open_path(&mut app, root.path.join("a.txt"));
    app.desktop.activate(WindowId::MANAGER);
    wait(&mut app);
    assert_eq!(app.desktop.document_ids(WindowKind::Editor), [first]);
    assert_eq!(app.desktop.active().id, WindowId::MANAGER);
    app.show_viewer(Viewer::open_mode(root.path.join("a.txt"), ViewMode::Text)?);
    let viewer = app.desktop.active().id;
    app.show_viewer(Viewer::open_mode(
        root.path.join("alias/a.txt"),
        ViewMode::Hex,
    )?);
    app.desktop.activate(WindowId::MANAGER);
    wait(&mut app);
    assert_eq!(app.desktop.document_ids(WindowKind::Viewer), [viewer]);
    assert!(
        app.desktop
            .viewer(viewer)
            .unwrap()
            .contents
            .as_ref()
            .unwrap()
            .binary
    );
    assert_eq!(app.desktop.active().id, WindowId::MANAGER);
    Ok(())
}
#[test]
fn stale_published_mouse_frame_cannot_target_a_new_modal_or_document() -> io::Result<()> {
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    use dn::testing::{backend::TestBackend, Terminal};
    let (_, mut app) = Temp::app()?;
    editor::open(&mut app, true);
    let first = app.desktop.active().id;
    let mut terminal = Terminal::new(TestBackend::new(80, 25))?;
    terminal.draw(|f| ui::draw(f, &mut app))?;
    let old_frame = app.mouse.frame;
    editor::action(&mut app, A::Help);
    let help = app.modal_top().unwrap().id;
    dn::mouse::handle(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        },
    );
    assert_eq!(app.modal_top().unwrap().id, help);
    editor::action(&mut app, A::Cancel);
    editor::open(&mut app, true);
    let second = app.desktop.active().id;
    app.mouse.frame = old_frame;
    dn::mouse::handle(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        },
    );
    assert_ne!(first, second);
    assert_eq!(app.desktop.active().id, second);
    assert!(app.modal_top().is_none());
    Ok(())
}

#[test]
fn delayed_definition_editor_open_observes_newer_focus_intent() -> io::Result<()> {
    let (root, mut app) = Temp::app()?;
    app.config_store = dn::config::Store::new(Some(root.path.join("config.conf")));
    dn::associations::edit(&mut app);
    editor::open(&mut app, true);
    let foreground = app.desktop.active().id;
    editor::paste(&mut app, "foreground");
    let end = Instant::now() + Duration::from_secs(5);
    while app.associations.busy() {
        app.poll();
        assert!(Instant::now() < end);
        thread::sleep(Duration::from_millis(1));
    }
    wait(&mut app);
    assert_eq!(app.desktop.active().id, foreground);
    assert_eq!(text(&app, foreground), "foreground");
    let definition = app
        .desktop
        .document_ids(WindowKind::Editor)
        .into_iter()
        .find(|id| *id != foreground)
        .unwrap();
    assert!(app
        .desktop
        .editor(definition)
        .unwrap()
        .path
        .as_ref()
        .unwrap()
        .ends_with("associations.conf"));
    app.desktop.activate(WindowId::MANAGER);
    app.poll();
    assert!(
        !app.associations.busy(),
        "switching away does not mean the definition editor closed"
    );
    Ok(())
}
#[test]
fn running_replaced_requests_cannot_apply_their_late_completion() -> io::Result<()> {
    let scope = RequestScope::new(Owner::Workspace);
    let (started, ready) = std::sync::mpsc::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let old = scope.spawn("request", move |_| {
        started.send(()).unwrap();
        wait.recv().unwrap();
        "old"
    })?;
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    let old_token = old.token().unwrap();
    let current = scope.spawn("request", move |_| "new")?;
    let current_token = current.token().unwrap();
    assert!(!scope.accepts(old_token));
    assert!(scope.accepts(current_token));
    release.send(()).unwrap();
    assert_eq!(
        old.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Disconnected)
    );
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        match current.try_recv() {
            Ok(value) => {
                assert_eq!(value, "new");
                break;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            other => panic!("unexpected reply {other:?}"),
        };
        assert!(Instant::now() < end);
        thread::sleep(Duration::from_millis(1));
    }
    assert!(!scope.accepts(current_token), "one-shot result is consumed");
    Ok(())
}

#[test]
fn clipping_and_viewport_revisions_keep_cells_cursor_and_mouse_in_agreement() -> io::Result<()> {
    use dn::testing::{backend::TestBackend, Terminal};
    use unicode_width::UnicodeWidthStr;
    let (_, mut app) = Temp::app()?;
    editor::open(&mut app, true);
    editor::paste(&mut app, &"界".repeat(50));
    let snapshot = dn::layout::prepare_window(
        &mut app,
        Rect::new(7, 3, 80, 25),
        Rect::new(0, 0, 84, 27),
        Instant::now(),
    );
    let rendered = ui::render(&app, &snapshot);
    assert!(rendered
        .hit_regions()
        .iter()
        .all(|(r, _)| r.x >= 7 && r.y >= 3 && r.right() <= 84 && r.bottom() <= 27));
    assert!(rendered.cursor().is_none_or(|(x, y)| x < 84 && y < 27));
    let buffer = dn::testing::Buffer::rendered(rendered);
    for y in 0..27 {
        assert!(
            buffer[(83, y)].symbol().width() <= 1,
            "wide glyph crosses right clip"
        );
    }
    let mut terminal = Terminal::new(TestBackend::new(80, 25))?;
    terminal.draw(|f| ui::draw(f, &mut app))?;
    let published = app.mouse.frame.unwrap();
    editor::paste(&mut app, "changed");
    dn::layout::update(&mut app);
    assert_ne!(
        published.revision, app.layout_revision,
        "edited hit positions invalidate the old frame"
    );
    Ok(())
}
