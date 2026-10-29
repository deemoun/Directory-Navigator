use dn::{
    app::App,
    desktop::{WindowId, WindowKind},
    editor::{self, CloseIntent, EditorAction, Modal},
    focus::{self, Focus},
    input,
    viewer::{self, Viewer},
};
use std::{
    fs, io,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

struct Temp(PathBuf);
impl Temp {
    fn new() -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "dn-desktop-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
    fn app(&self) -> io::Result<App> {
        let mut app = App::new(self.0.clone())?;
        ready(&mut app);
        Ok(app)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn ready(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.panels.iter().any(|p| p.loading)
        || app.desktop.active_editor().is_some_and(|e| e.busy())
        || app
            .desktop
            .active_viewer()
            .is_some_and(|v| v.contents.is_none())
    {
        app.poll();
        assert!(Instant::now() < deadline, "background load did not finish");
        thread::sleep(Duration::from_millis(1));
    }
}
fn preview(path: PathBuf) -> Viewer {
    Viewer::from_contents(
        path,
        viewer::from_bytes(b"first\nsecond\n".to_vec(), viewer::ViewMode::Text),
    )
}

#[test]
fn stable_identities_activate_new_documents_and_keep_multiple_instances() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    assert_eq!(app.desktop.active().id, WindowId::MANAGER);
    assert_eq!(app.desktop.windows().count(), 1);
    editor::open(&mut app, true);
    let editor = app.desktop.active();
    app.show_game(dn::tetris::Game::default());
    let game = app
        .desktop
        .id(WindowKind::Plugin(dn::plugins::TETRIS))
        .unwrap();
    app.show_viewer(preview(temp.0.join("preview")));
    let viewer = app.desktop.id(WindowKind::Viewer).unwrap();
    assert_eq!(app.desktop.windows().count(), 4);
    assert_eq!(app.desktop.active().id, viewer);
    assert_eq!(focus::current(&app), Focus::Viewer);
    app.desktop.close(editor.id);
    assert!(!app.desktop.contains(editor.id));
    assert_eq!(app.desktop.active().id, viewer);
    app.desktop.close_viewer();
    assert_eq!(app.desktop.active().id, game);
    app.desktop.close_game();
    assert_eq!(app.desktop.active().id, WindowId::MANAGER);
    app.show_viewer(preview(temp.0.join("new")));
    let new = app.desktop.active().id;
    assert_ne!(new, viewer);
    app.show_viewer(preview(temp.0.join("replacement")));
    assert_ne!(app.desktop.active().id, new);
    assert!(app.desktop.contains(new));
    assert_eq!(app.desktop.windows().count(), 3);
    Ok(())
}

#[test]
fn modal_focus_close_cancel_and_quit_guards_retain_window_identity() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    editor::open(&mut app, true);
    let owner = app.desktop.active();
    editor::paste(&mut app, "unsaved 界é");
    let mut viewer = preview(temp.0.join("preview"));
    viewer.offset = 1;
    app.show_viewer(viewer);
    app.desktop.activate(owner.id);
    editor::request_close(&mut app, CloseIntent::Panel);
    assert_eq!(focus::current(&app), Focus::EditorModal);
    assert_eq!(app.desktop.active(), owner);
    app.error("nested background error");
    input::paste(&mut app, "must not enter the document");
    assert_eq!(focus::current(&app), Focus::Message);
    app.clear_dialogs();
    editor::action(&mut app, EditorAction::Cancel);
    assert_eq!(app.desktop.active(), owner);
    assert_eq!(
        app.desktop
            .active_editor()
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .text,
        "unsaved 界é"
    );
    app.request_exit();
    assert!(!app.quit);
    assert!(matches!(
        app.desktop.active_editor().unwrap().modals.editor_modal(),
        Some(Modal::ConfirmClose { .. })
    ));
    editor::action(&mut app, EditorAction::Cancel);
    assert!(!app.quit);
    assert_eq!(app.desktop.active(), owner);
    editor::request_close(&mut app, CloseIntent::Panel);
    editor::action(&mut app, EditorAction::Discard);
    assert!(!app.desktop.contains(owner.id));
    assert_eq!(app.desktop.active().kind, WindowKind::Viewer);
    assert_eq!(app.desktop.active_viewer().unwrap().offset, 1);
    Ok(())
}

#[test]
fn opening_files_creates_documents_and_failed_load_preserves_the_previous_buffer() -> io::Result<()>
{
    let temp = Temp::new()?;
    let path = temp.0.join("document.txt");
    fs::write(&path, "loaded document")?;
    let mut app = temp.app()?;
    editor::open(&mut app, true);
    let owner = app.desktop.active();
    editor::open_path(&mut app, path.clone());
    ready(&mut app);
    assert_ne!(app.desktop.active(), owner);
    assert!(app.desktop.contains(owner.id));
    let loaded = app.desktop.active();
    assert_eq!(
        app.desktop.active_editor().unwrap().path.as_ref(),
        Some(&path)
    );
    editor::open_path(&mut app, temp.0.join("missing"));
    ready(&mut app);
    assert!(!app.dialogs().is_empty());
    assert_eq!(app.desktop.active(), loaded);
    assert_eq!(
        app.desktop
            .active_editor()
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .text,
        "loaded document"
    );
    Ok(())
}

#[test]
fn closed_or_replaced_content_cannot_receive_old_background_results() -> io::Result<()> {
    let temp = Temp::new()?;
    let path = temp.0.join("current.txt");
    fs::write(&path, "current preview")?;
    let mut app = temp.app()?;
    app.show_viewer(Viewer::open(temp.0.join("missing-viewer"))?);
    let previous = app.desktop.active().id;
    app.desktop.close(previous);
    app.show_viewer(Viewer::open(path.clone())?);
    assert!(!app.desktop.contains(previous));
    ready(&mut app);
    assert!(app.dialogs().is_empty());
    assert_eq!(app.desktop.active_viewer().unwrap().path, path);
    assert_eq!(
        app.desktop
            .active_viewer()
            .unwrap()
            .contents
            .as_ref()
            .unwrap()
            .lines[0],
        "current preview"
    );
    app.desktop.close_viewer();
    editor::open_path(&mut app, temp.0.join("missing-editor"));
    let previous = app.desktop.active().id;
    app.desktop.close_editor();
    editor::open(&mut app, true);
    let owner = app.desktop.active();
    assert!(!app.desktop.contains(previous));
    editor::paste(&mut app, "fresh unsaved document");
    for _ in 0..100 {
        app.poll();
        thread::sleep(Duration::from_millis(1));
    }
    assert!(app.dialogs().is_empty());
    assert_eq!(app.desktop.active(), owner);
    assert_eq!(
        app.desktop
            .active_editor()
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .text,
        "fresh unsaved document"
    );
    Ok(())
}

fn key(app: &mut App, code: crossterm::event::KeyCode) {
    input::handle(
        app,
        crossterm::event::KeyEvent::new(code, crossterm::event::KeyModifiers::NONE),
    );
}
fn windows(app: &mut App) {
    input::handle(
        app,
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('0'),
            crossterm::event::KeyModifiers::ALT,
        ),
    );
}
fn switch(app: &mut App, kind: WindowKind) {
    windows(app);
    let id = app.desktop.id(kind).unwrap();
    dn::windowing::action(app, dn::windowing::Action::Row(id));
    key(app, crossterm::event::KeyCode::Enter);
    assert_eq!(app.desktop.active().id, id);
    assert!(app.modals.window_list().is_none());
}
#[test]
fn switching_retains_documents_prompt_panels_and_independent_undo() -> io::Result<()> {
    use crossterm::event::KeyCode as K;
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    app.command_line.text = "draft command".into();
    let panel = (
        app.active,
        app.panels[0].path.clone(),
        app.panels[0].cursor,
        app.panels[0].revision,
    );
    editor::open(&mut app, true);
    editor::paste(&mut app, "unsaved 界é");
    let id = app.desktop.active().id;
    switch(&mut app, WindowKind::Manager);
    assert_eq!(app.command_line.text, "draft command");
    let mut viewer = preview(temp.0.join("preview"));
    viewer.offset = 1;
    app.show_viewer(viewer);
    assert_eq!(focus::screen(&app), WindowKind::Viewer);
    switch(&mut app, WindowKind::Manager);
    windows(&mut app);
    dn::windowing::action(&mut app, dn::windowing::Action::Row(id));
    key(&mut app, K::Enter);
    assert_eq!(app.desktop.active().id, id);
    assert_eq!(
        app.desktop
            .active_editor()
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .text,
        "unsaved 界é"
    );
    editor::action(&mut app, EditorAction::Undo);
    assert_eq!(
        app.desktop
            .active_editor()
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .text,
        ""
    );
    switch(&mut app, WindowKind::Viewer);
    assert_eq!(app.desktop.active_viewer().unwrap().offset, 1);
    app.desktop.close_viewer();
    assert_eq!(app.desktop.active().id, id);
    assert_eq!(
        (
            app.active,
            app.panels[0].path.clone(),
            app.panels[0].cursor,
            app.panels[0].revision
        ),
        panel
    );
    Ok(())
}
#[test]
fn hidden_dirty_editor_quit_and_open_path_keep_unsaved_guards() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    editor::open(&mut app, true);
    editor::paste(&mut app, "keep me");
    let id = app.desktop.active().id;
    switch(&mut app, WindowKind::Manager);
    let next = temp.0.join("next");
    fs::write(&next, "next")?;
    editor::open_path(&mut app, next);
    let next_id = app.desktop.active().id;
    assert_ne!(next_id, id);
    ready(&mut app);
    assert!(app.desktop.editor(id).unwrap().dirty());
    assert_eq!(focus::current(&app), Focus::Editor);
    switch(&mut app, WindowKind::Manager);
    app.request_exit();
    assert_eq!(app.desktop.active().id, id);
    assert_eq!(focus::current(&app), Focus::EditorModal);
    assert!(!app.quit);
    editor::action(&mut app, EditorAction::Cancel);
    assert!(!app.quit);
    assert_eq!(
        app.desktop
            .active_editor()
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .text,
        "keep me"
    );
    switch(&mut app, WindowKind::Manager);
    app.request_exit();
    editor::action(&mut app, EditorAction::Discard);
    assert!(app.quit);
    Ok(())
}
#[test]
fn list_consumes_input_and_cancels_without_changing_focus_and_modals_take_precedence(
) -> io::Result<()> {
    use crossterm::event::KeyCode as K;
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    editor::open(&mut app, true);
    editor::paste(&mut app, "original");
    let id = app.desktop.active().id;
    windows(&mut app);
    assert_eq!(focus::current(&app), Focus::WindowList);
    input::paste(&mut app, "invisible paste");
    key(&mut app, K::Char('x'));
    key(&mut app, K::F(10));
    assert!(!app.quit);
    assert!(app
        .desktop
        .active_editor()
        .unwrap()
        .modals
        .editor_modal()
        .is_none());
    assert_eq!(
        app.desktop
            .active_editor()
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .text,
        "original"
    );
    key(&mut app, K::Home);
    key(&mut app, K::Tab);
    key(&mut app, K::Tab);
    key(&mut app, K::Enter);
    assert!(app.modals.window_list().is_none());
    assert_eq!(app.desktop.active().id, id);
    editor::request_close(&mut app, CloseIntent::Panel);
    windows(&mut app);
    assert!(app.modals.window_list().is_none());
    editor::action(&mut app, EditorAction::Cancel);
    windows(&mut app);
    app.error("higher priority");
    key(&mut app, K::Esc);
    assert!(app.dialogs().is_empty());
    assert!(app.modals.window_list().is_some());
    key(&mut app, K::Esc);
    assert!(app.modals.window_list().is_none());
    Ok(())
}
#[test]
fn stale_rows_cannot_activate_replacements_and_inactive_close_preserves_focus() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    app.show_viewer(preview(temp.0.join("old")));
    let old = app.desktop.active().id;
    switch(&mut app, WindowKind::Manager);
    windows(&mut app);
    dn::windowing::action(&mut app, dn::windowing::Action::Row(old));
    app.desktop.close(old);
    assert_eq!(app.desktop.active().id, WindowId::MANAGER);
    app.show_viewer(preview(temp.0.join("replacement")));
    let replacement = app.desktop.active().id;
    app.desktop.activate(WindowId::MANAGER);
    dn::windowing::action(&mut app, dn::windowing::Action::Switch);
    assert!(app.modals.window_list().is_some());
    assert_eq!(app.desktop.active().id, WindowId::MANAGER);
    assert!(!app.desktop.activate(old));
    dn::windowing::action(&mut app, dn::windowing::Action::Row(old));
    assert_eq!(app.modals.window_list().unwrap().cursor, 0);
    app.desktop.close(replacement);
    assert_eq!(app.desktop.active().id, WindowId::MANAGER);
    assert!(dn::windowing::poll(&mut app));
    assert_eq!(app.modals.window_list().unwrap().ids, [WindowId::MANAGER]);
    Ok(())
}
#[test]
fn window_list_profiles_languages_resize_mouse_and_background_tools() -> io::Result<()> {
    use crossterm::event::{
        KeyCode as K, KeyEvent, KeyModifiers as M, MouseButton, MouseEvent, MouseEventKind,
    };
    use dn::testing::{backend::TestBackend, Terminal};
    use dn::{
        localization::Language,
        mouse::{self, Target},
        palette::PaletteProfile,
        shortcuts::Profile,
        windowing::Action,
    };
    let temp = Temp::new()?;
    for language in [Language::English, Language::Russian] {
        for profile in [Profile::ClassicDn, Profile::Mac] {
            for palette in [PaletteProfile::ClassicDn, PaletteProfile::Terminal] {
                let mut app = temp.app()?;
                app.config.language = language;
                app.config.shortcuts = profile;
                app.config.palette = palette;
                editor::open(&mut app, true);
                editor::paste(&mut app, "retain");
                let editor = app.desktop.active().id;
                if profile == Profile::Mac {
                    input::handle(&mut app, KeyEvent::new(K::Char('o'), M::CONTROL));
                    key(&mut app, K::Char('u'));
                } else {
                    windows(&mut app);
                }
                assert_eq!(focus::current(&app), Focus::WindowList);
                for (w, h) in [(80, 25), (132, 40), (50, 15), (80, 25)] {
                    let mut terminal = Terminal::new(TestBackend::new(w, h))?;
                    terminal.draw(|f| dn::ui::draw(f, &mut app))?;
                    assert!(app.modals.window_list().is_some());
                    assert!(app
                        .mouse
                        .regions
                        .iter()
                        .all(|(r, _)| r.right() <= w && r.bottom() <= h));
                    if w >= 80 {
                        let text: String = terminal
                            .backend()
                            .buffer()
                            .content
                            .iter()
                            .map(|c| c.symbol())
                            .collect();
                        assert!(text.contains(language.text("Open windows")));
                        assert!(text.contains(language.text("Switch")));
                    } else {
                        assert!(app.mouse.regions.is_empty());
                    }
                }
                dn::screensaver::poll(&mut app, Instant::now() + Duration::from_secs(3600));
                assert!(!app.screensaver.active);
                let row = app
                    .mouse
                    .regions
                    .iter()
                    .find(|(_, t)| *t == Target::Window(Action::Row(WindowId::MANAGER)))
                    .unwrap()
                    .0;
                let click = MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column: row.x,
                    row: row.y,
                    modifiers: M::NONE,
                };
                mouse::handle(&mut app, click);
                assert!(app.modals.window_list().is_some());
                // Row selection changes Size/Move/Zoom availability; publish its new revision.
                let mut terminal = Terminal::new(TestBackend::new(80, 25))?;
                terminal.draw(|f| dn::ui::draw(f, &mut app))?;
                mouse::handle(&mut app, click);
                assert!(app.modals.window_list().is_none());
                assert_eq!(app.desktop.active().id, WindowId::MANAGER);
                dn::quick_search::open(&mut app);
                assert!(app.modals.quick_search().is_some());
                app.modals.set_quick_search(None);
                dn::quick_view::toggle(&mut app);
                dn::quick_view::poll(&mut app);
                assert!(app.quick_view.is_some());
                windows(&mut app);
                dn::windowing::action(&mut app, Action::Row(editor));
                let mut terminal = Terminal::new(TestBackend::new(80, 25))?;
                terminal.draw(|f| dn::ui::draw(f, &mut app))?;
                let button = app
                    .mouse
                    .regions
                    .iter()
                    .find(|(_, t)| *t == Target::Window(Action::Switch))
                    .unwrap()
                    .0;
                mouse::handle(
                    &mut app,
                    MouseEvent {
                        column: button.x,
                        row: button.y,
                        ..click
                    },
                );
                assert_eq!(app.desktop.active().id, editor);
            }
        }
    }
    Ok(())
}

#[test]
fn editor_quit_waits_for_operation_and_cancelled_exit_never_rearms_itself() -> io::Result<()> {
    use dn::operations::{Job, Kind, Plan};
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    editor::open(&mut app, true);
    editor::paste(&mut app, "guard");
    switch(&mut app, WindowKind::Manager);
    let source = temp.0.join("source");
    fs::write(&source, "keep source")?;
    app.job = Some(Job::start(Plan {
        kind: Kind::Copy,
        sources: vec![source.clone()],
        destination: temp.0.join("destination"),
    })?);
    app.request_exit();
    assert_eq!(focus::current(&app), Focus::EditorModal);
    editor::action(&mut app, EditorAction::Cancel);
    while app.job.is_some() {
        app.poll();
        thread::sleep(Duration::from_millis(1));
    }
    assert!(!app.quit);
    assert!(!app.exit_pending);
    app.clear_dialogs();
    app.job = Some(Job::start(Plan {
        kind: Kind::Copy,
        sources: vec![source.clone()],
        destination: temp.0.join("another"),
    })?);
    app.request_exit();
    editor::action(&mut app, EditorAction::Discard);
    assert!(!app.quit, "worker must finish before exit");
    assert!(app.exit_pending);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !app.quit {
        app.poll();
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    assert!(app.job.is_none());
    assert_eq!(fs::read_to_string(source)?, "keep source");
    Ok(())
}

#[test]
fn window_menu_actions_and_modal_title_close_preserve_existing_shortcuts() -> io::Result<()> {
    use crossterm::event::{KeyCode as K, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    use dn::testing::{backend::TestBackend, Terminal};
    use dn::{
        menu,
        mouse::{self, Target},
        windowing::Action,
    };
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    assert!(menu::items(4)
        .iter()
        .any(|i| i.action == menu::Action::WindowList && i.shortcut == "Alt+0"));
    menu::open(&mut app, 4);
    menu::execute(&mut app, menu::Action::WindowList);
    assert_eq!(focus::current(&app), Focus::WindowList);
    let mut terminal = Terminal::new(TestBackend::new(80, 25))?;
    terminal.draw(|f| dn::ui::draw(f, &mut app))?;
    let close = app
        .mouse
        .regions
        .iter()
        .find(|(_, t)| *t == Target::Window(Action::Cancel))
        .unwrap()
        .0;
    mouse::handle(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: close.x,
            row: close.y,
            modifiers: KeyModifiers::NONE,
        },
    );
    assert_eq!(focus::current(&app), Focus::Manager);
    key(&mut app, K::F(9));
    assert!(app.modals.menu().is_some());
    windows(&mut app);
    assert_eq!(focus::current(&app), Focus::WindowList);
    assert!(app.modals.menu().is_none());
    key(&mut app, K::Esc);
    app.prompt_operation(dn::operations::Kind::Mkdir, false);
    windows(&mut app);
    assert!(app.modals.window_list().is_none());
    assert!(app.modals.prompt().is_some());
    app.modals.set_prompt(None);
    editor::open(&mut app, true);
    editor::menus::open(&mut app, 0);
    let row = editor::menus::items(0)
        .iter()
        .position(|i| i.action == Some(EditorAction::Windows))
        .unwrap();
    editor::menus::activate(&mut app, row);
    assert_eq!(focus::current(&app), Focus::WindowList);
    key(&mut app, K::Esc);
    assert!(app
        .desktop
        .active_editor()
        .unwrap()
        .modals
        .editor_menu()
        .is_none());
    editor::menus::open(&mut app, 0);
    windows(&mut app);
    assert_eq!(focus::current(&app), Focus::WindowList);
    key(&mut app, K::Esc);
    app.desktop.close_editor();
    app.show_game(dn::tetris::Game::default());
    let game = app.desktop.active().id;
    windows(&mut app);
    assert!(app.modals.window_list().unwrap().ids.contains(&game));
    assert_eq!(focus::current(&app), Focus::WindowList);
    key(&mut app, K::Esc);
    assert_eq!(focus::current(&app), Focus::Plugin);
    Ok(())
}
