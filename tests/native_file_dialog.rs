use crossterm::event::{
    KeyCode as K, KeyEvent, KeyModifiers as M, MouseButton, MouseEvent, MouseEventKind,
};
use dn::{
    app::App,
    desktop::{WindowId, WindowKind},
    editor::{self, EditorAction as A, Modal},
    input, layout,
    presentation::layout::Rect,
    testing::Buffer,
    turbo::file_dialog::{Mode, Picker},
    ui,
};
use std::{
    fs, io,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, MutexGuard,
    },
    thread,
    time::{Duration, Instant},
};
struct Temp {
    root: PathBuf,
    _guard: MutexGuard<'static, ()>,
}
impl Temp {
    fn app() -> io::Result<(Self, App)> {
        static SERIAL: Mutex<()> = Mutex::new(());
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let root = std::env::temp_dir().join(format!(
            "dn-native-picker-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("sub"))?;
        fs::write(root.join("a.txt"), "original\n")?;
        fs::write(root.join("sub/inside.txt"), "inside\n")?;
        fs::write(root.join("sub/skip.bin"), "skip")?;
        let temp = Self {
            root,
            _guard: guard,
        };
        let mut app = App::new(temp.root.clone())?;
        app.config.save_on_exit = false;
        wait(&mut app);
        Ok((temp, app))
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn wait(app: &mut App) {
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        app.poll();
        if !app.panels.iter().any(|p| p.loading)
            && !app.desktop.windows().any(|w| {
                app.desktop.identity_pending(w.id)
                    || app.desktop.editor(w.id).is_some_and(|e| e.busy())
            })
        {
            break;
        }
        assert!(Instant::now() < end);
        thread::sleep(Duration::from_millis(1));
    }
}
fn picker(app: &mut App) -> &mut Picker {
    match app
        .desktop
        .active_editor_mut()
        .unwrap()
        .modals
        .editor_modal_mut()
    {
        Some(Modal::File(picker)) => picker,
        _ => panic!("native file dialog"),
    }
}
fn key(app: &mut App, code: K) {
    input::handle(app, KeyEvent::new(code, M::NONE));
}
fn render(app: &mut App, bounds: Rect) -> Buffer {
    let snapshot = layout::prepare_window(app, bounds, Rect::new(0, 0, 100, 35), Instant::now());
    let frame = ui::render(app, &snapshot);
    app.mouse.regions = frame.hit_regions().to_vec();
    app.mouse.frame = Some(frame.context);
    Buffer::rendered(frame)
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
fn buffer_text(buffer: &Buffer) -> String {
    buffer.content.iter().map(|c| c.symbol()).collect()
}
#[test]
fn native_open_browses_filters_and_opens_without_replacing_the_owner() -> io::Result<()> {
    let (root, mut app) = Temp::app()?;
    editor::open(&mut app, true);
    editor::paste(&mut app, "keep draft");
    let original = app.desktop.active().id;
    editor::action(&mut app, A::Open);
    assert_eq!(picker(&mut app).mode, Mode::Open);
    picker(&mut app).set_filename("sub".into());
    editor::action(&mut app, A::Submit);
    assert_eq!(picker(&mut app).directory(), root.root.join("sub"));
    picker(&mut app).set_filename("*.txt".into());
    editor::action(&mut app, A::Submit);
    let screen = buffer_text(&render(&mut app, Rect::new(0, 0, 80, 25)));
    assert!(screen.contains("inside.txt"));
    assert!(!screen.contains("skip.bin"));
    key(&mut app, K::Tab);
    key(&mut app, K::Down);
    key(&mut app, K::Enter);
    wait(&mut app);
    assert_ne!(app.desktop.active().id, original);
    assert_eq!(
        app.desktop.active_editor().unwrap().path.as_ref(),
        Some(&root.root.join("sub/inside.txt"))
    );
    assert_eq!(text(&app, original), "keep draft");
    assert_eq!(app.desktop.document_ids(WindowKind::Editor).len(), 2);
    Ok(())
}
#[test]
fn native_save_as_rejects_owned_paths_and_recovers_with_a_unicode_destination() -> io::Result<()> {
    let (root, mut app) = Temp::app()?;
    editor::open_path(&mut app, root.root.join("a.txt"));
    wait(&mut app);
    let original = app.desktop.active().id;
    editor::open(&mut app, true);
    editor::paste(&mut app, "new text\n");
    let new = app.desktop.active().id;
    editor::action(&mut app, A::SaveAs);
    picker(&mut app).set_filename(root.root.join("a.txt").to_string_lossy().into_owned());
    editor::action(&mut app, A::Submit);
    assert!(app
        .last_dialog()
        .unwrap()
        .message
        .contains("another editor"));
    assert_eq!(picker(&mut app).mode, Mode::SaveAs);
    key(&mut app, K::Esc);
    let path = root.root.join("новый 界.txt");
    picker(&mut app).set_filename(String::new());
    let filename = path.to_string_lossy().into_owned();
    for c in filename.chars() {
        key(&mut app, K::Char(c));
    }
    assert_eq!(picker(&mut app).filename(), filename);
    picker(&mut app).set_filename(String::new());
    editor::paste(&mut app, &filename);
    assert_eq!(picker(&mut app).filename(), filename);
    key(&mut app, K::Enter);
    wait(&mut app);
    assert_eq!(fs::read_to_string(path)?, "new text\n");
    assert_eq!(text(&app, original), "original\n");
    assert_eq!(app.desktop.active().id, new);
    assert!(!app.desktop.editor(new).unwrap().dirty());
    Ok(())
}
#[test]
fn cancelling_native_save_as_cancels_quit_and_retains_the_dirty_document() -> io::Result<()> {
    let (_, mut app) = Temp::app()?;
    editor::open(&mut app, true);
    editor::paste(&mut app, "keep");
    let id = app.desktop.active().id;
    app.request_exit();
    editor::action(&mut app, A::Save);
    assert_eq!(picker(&mut app).mode, Mode::SaveAs);
    key(&mut app, K::Esc);
    assert!(app.quit_review.is_none());
    assert!(!app.quit);
    assert_eq!(text(&app, id), "keep");
    assert!(app.desktop.editor(id).unwrap().dirty());
    Ok(())
}
#[test]
fn native_dialog_state_survives_help_and_rendering_is_read_only() -> io::Result<()> {
    let (root, mut app) = Temp::app()?;
    editor::open(&mut app, true);
    editor::action(&mut app, A::Open);
    picker(&mut app).set_filename("sub".into());
    editor::action(&mut app, A::Submit);
    picker(&mut app).set_filename("inside.txt".into());
    app.sync_modals();
    let parent = app.modal_top().unwrap().id;
    key(&mut app, K::F(1));
    app.sync_modals();
    assert_eq!(app.modal_top().unwrap().parent, Some(parent));
    key(&mut app, K::Esc);
    assert_eq!(app.modal_top().unwrap().id, parent);
    assert_eq!(picker(&mut app).directory(), root.root.join("sub"));
    assert_eq!(picker(&mut app).filename(), "inside.txt");
    let snapshot = layout::prepare_window(
        &mut app,
        Rect::new(7, 3, 80, 25),
        Rect::new(0, 0, 83, 27),
        Instant::now(),
    );
    let before = (picker(&mut app).filename(), picker(&mut app).directory());
    let first = ui::render(&app, &snapshot);
    let second = ui::render(&app, &snapshot);
    assert!(first
        .hit_regions()
        .iter()
        .all(|(r, _)| r.x >= 7 && r.y >= 3 && r.right() <= 83 && r.bottom() <= 27));
    assert_eq!(Buffer::rendered(first), Buffer::rendered(second));
    assert_eq!(
        before,
        (picker(&mut app).filename(), picker(&mut app).directory())
    );
    Ok(())
}
#[test]
fn native_cancel_button_routes_mouse_at_translated_window_coordinates() -> io::Result<()> {
    let (_, mut app) = Temp::app()?;
    editor::open(&mut app, true);
    editor::paste(&mut app, "draft");
    let id = app.desktop.active().id;
    editor::action(&mut app, A::SaveAs);
    let bounds = Rect::new(7, 3, 80, 25);
    let buffer = render(&mut app, bounds);
    let needle = ["C", "a", "n", "c", "e", "l"];
    let (x, y) = buffer
        .content
        .chunks(usize::from(buffer.area.width))
        .enumerate()
        .find_map(|(y, row)| {
            row.windows(needle.len())
                .position(|cells| {
                    cells
                        .iter()
                        .zip(needle)
                        .all(|(cell, ch)| cell.symbol() == ch)
                })
                .map(|x| (x + 2, y))
        })
        .expect("native Cancel button");
    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
    ] {
        dn::mouse::handle(
            &mut app,
            MouseEvent {
                kind,
                column: x as u16,
                row: y as u16,
                modifiers: M::NONE,
            },
        );
        render(&mut app, bounds);
    }
    assert!(app
        .desktop
        .editor(id)
        .unwrap()
        .modals
        .editor_modal()
        .is_none());
    assert_eq!(text(&app, id), "draft");
    assert!(app.desktop.editor(id).unwrap().dirty());
    Ok(())
}
#[test]
fn native_open_reuses_documents_and_failed_save_never_overwrites() -> io::Result<()> {
    let (root, mut app) = Temp::app()?;
    editor::open_path(&mut app, root.root.join("a.txt"));
    wait(&mut app);
    let existing = app.desktop.active().id;
    editor::open(&mut app, true);
    editor::paste(&mut app, "draft");
    let new = app.desktop.active().id;
    editor::action(&mut app, A::Open);
    picker(&mut app).set_filename(root.root.join("a.txt").to_string_lossy().into_owned());
    key(&mut app, K::Enter);
    assert_eq!(app.desktop.active().id, existing);
    assert_eq!(text(&app, new), "draft");
    app.desktop.activate(new);
    fs::write(root.root.join("occupied.txt"), "outside")?;
    editor::action(&mut app, A::SaveAs);
    picker(&mut app).set_filename("occupied.txt".into());
    key(&mut app, K::Enter);
    wait(&mut app);
    assert_eq!(
        fs::read_to_string(root.root.join("occupied.txt"))?,
        "outside"
    );
    assert_eq!(text(&app, new), "draft");
    assert!(app.desktop.editor(new).unwrap().dirty());
    assert!(!app.dialogs().is_empty());
    Ok(())
}

#[test]
fn native_hidden_toggle_and_translated_hotkeys_keep_their_native_controls() -> io::Result<()> {
    use dn::localization::Language;
    let (root, mut app) = Temp::app()?;
    fs::write(root.root.join(".hidden.txt"), "hidden")?;
    app.config.show_hidden = false;
    editor::open(&mut app, true);
    for language in [Language::English, Language::Russian] {
        app.config.language = language;
        editor::action(&mut app, A::Open);
        let bounds = Rect::new(0, 0, 80, 25);
        assert!(!buffer_text(&render(&mut app, bounds)).contains(".hidden.txt"));
        input::handle(
            &mut app,
            KeyEvent::new(
                K::Char(if language == Language::English {
                    'h'
                } else {
                    'к'
                }),
                M::ALT,
            ),
        );
        assert!(buffer_text(&render(&mut app, bounds)).contains(".hidden.txt"));
        input::handle(&mut app, KeyEvent::new(K::Char('h'), M::ALT));
        assert!(!buffer_text(&render(&mut app, bounds)).contains(".hidden.txt"));
        input::handle(
            &mut app,
            KeyEvent::new(
                K::Char(if language == Language::English {
                    'c'
                } else {
                    'м'
                }),
                M::ALT,
            ),
        );
        assert!(app
            .desktop
            .active_editor()
            .unwrap()
            .modals
            .editor_modal()
            .is_none());
    }
    Ok(())
}
#[cfg(unix)]
#[test]
fn native_listing_never_emits_control_characters_from_filenames() -> io::Result<()> {
    let (root, mut app) = Temp::app()?;
    fs::write(root.root.join("bad\u{1b}[31m.txt"), "content")?;
    editor::open(&mut app, true);
    editor::action(&mut app, A::Open);
    let buffer = render(&mut app, Rect::new(0, 0, 80, 25));
    assert!(buffer_text(&buffer).contains("[31m.txt"));
    assert!(buffer
        .content
        .iter()
        .all(|c| !c.symbol().chars().any(char::is_control)));
    Ok(())
}
