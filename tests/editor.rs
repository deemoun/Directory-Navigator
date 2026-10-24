use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use dn::{
    app::App,
    editor::{
        self,
        buffer::{Buffer, MAX_BYTES, MAX_LINES},
        highlight::{Cache, Language},
        storage, CloseIntent, EditorAction as A, Modal,
    },
    input, menu,
    mouse::{self, Target},
    ui,
};
use ratatui::{backend::TestBackend, Terminal};
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
            "dn-editor-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
    fn app(&self) -> io::Result<App> {
        let mut app = App::new(self.0.clone())?;
        wait(&mut app);
        Ok(app)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn wait(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.panels.iter().any(|p| p.loading) || app.desktop.editor().is_some_and(|e| e.busy()) {
        app.poll();
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
}
fn key(app: &mut App, code: KeyCode) {
    input::handle(app, KeyEvent::new(code, KeyModifiers::NONE));
}
fn modified(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    input::handle(app, KeyEvent::new(code, modifiers));
}
fn render(app: &mut App, width: u16, height: u16) -> io::Result<Terminal<TestBackend>> {
    let mut terminal = Terminal::new(TestBackend::new(width, height))?;
    terminal.draw(|frame| ui::draw(frame, app))?;
    assert!(app
        .mouse
        .regions
        .iter()
        .all(|(rect, _)| rect.right() <= width && rect.bottom() <= height));
    Ok(terminal)
}
fn click(app: &mut App, command: A) {
    let rect = app
        .mouse
        .regions
        .iter()
        .find(|(_, target)| *target == Target::Editor(command))
        .unwrap()
        .0;
    mouse::handle(
        app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: rect.x,
            row: rect.y,
            modifiers: KeyModifiers::NONE,
        },
    );
}
fn focus(app: &mut App, name: &str) {
    app.panels[app.active].cursor = app.panels[app.active]
        .entries
        .iter()
        .position(|e| e.name == name)
        .unwrap();
}
fn buffer(app: &App) -> &Buffer {
    app.desktop.editor().unwrap().buffer.as_ref().unwrap()
}

#[test]
fn unicode_graphemes_tabs_navigation_delete_selection_undo_and_redo() {
    let mut b = Buffer::new("aé終👩‍💻e\u{301}\r\n\tlast\r\n".into()).unwrap();
    assert_eq!(b.lines.len(), 3);
    assert_eq!(b.newline, "\r\n");
    b.move_to(b.lines[0].end, false);
    b.delete(true).unwrap();
    assert!(!b.text.contains("e\u{301}"));
    b.delete(true).unwrap();
    assert!(!b.text.contains("👩‍💻"));
    b.undo(false);
    b.undo(false);
    assert_eq!(b.line_text(0), "aé終👩‍💻e\u{301}");
    b.move_to(0, false);
    b.horizontal(true, true);
    b.horizontal(true, true);
    assert_eq!(&b.text[b.selected()], "aé");
    b.insert("λ").unwrap();
    assert!(b.text.starts_with("λ終"));
    b.undo(false);
    b.undo(true);
    assert!(b.text.starts_with("λ終"));
    b.move_to(b.lines[1].start, false);
    b.horizontal(true, false);
    assert_eq!(b.column(), 8);
    b.move_to(b.lines[1].start, false);
    b.delete(true).unwrap();
    assert_eq!(b.lines.len(), 2);
    b.undo(false);
    assert_eq!(b.lines.len(), 3);
    b.move_to(b.lines[0].end, false);
    b.insert("\n新").unwrap();
    assert!(b.text.contains("\r\n新\r\n"));
    let mut joining = Buffer::new("👩💻".into()).unwrap();
    joining.horizontal(true, false);
    joining.insert("\u{200d}").unwrap();
    assert_eq!(joining.cursor, joining.text.len());
    joining.delete(true).unwrap();
    assert!(joining.text.is_empty());
    joining.undo(false);
    assert_eq!(joining.text, "👩‍💻");
}

#[test]
fn dirty_revision_search_wrap_and_limits() {
    let mut b = Buffer::new("one\ntwo one\n".into()).unwrap();
    b.insert("X").unwrap();
    assert!(b.dirty());
    b.mark_saved();
    assert!(!b.dirty());
    b.insert("Y").unwrap();
    assert!(b.dirty());
    b.undo(false);
    assert!(!b.dirty());
    b.undo(true);
    assert!(b.dirty());
    assert!(b.find("one"));
    assert_eq!(&b.text[b.selected()], "one");
    let first = b.selected();
    assert!(b.find("one"));
    assert_ne!(first, b.selected());
    assert!(b.find("one"));
    assert_eq!(first, b.selected());
    assert!(!b.find("missing"));
    assert!(Buffer::new("x".repeat(MAX_BYTES + 1)).is_err());
    assert!(Buffer::new("\n".repeat(MAX_LINES)).is_err());
    assert!(Buffer::new("binary\0text".into()).is_err());
    let original = b.text.clone();
    assert!(b.insert(&"x".repeat(MAX_BYTES)).is_err());
    assert_eq!(b.text, original);
    let position = b.cursor;
    assert!(b.insert(&"\n".repeat(MAX_LINES)).is_err());
    assert_eq!(b.cursor, position);
}

#[test]
fn lexical_colors_multiline_state_and_cache_invalidation() {
    let mut b = Buffer::new(
        "fn main() {\n/* outer\nnested */ let x = r#\"text\"#; // comment\nlet y = 42;\n}\n".into(),
    )
    .unwrap();
    let mut cache = Cache::new(Language::Rust);
    cache.refresh(&b);
    assert_eq!(cache.color(0, 0), 0x1b);
    assert_eq!(cache.color(1, 0), 0x1a);
    assert_eq!(cache.color(2, 0), 0x1a);
    assert_eq!(cache.color(2, 10), 0x1b);
    assert_eq!(cache.color(3, 8), 0x1d);
    let close = b.text.find("*/").unwrap();
    b.replace(close..close + 2, "  ").unwrap();
    cache.refresh(&b);
    assert_eq!(cache.color(3, 0), 0x1a);
    b.undo(false);
    cache.refresh(&b);
    assert_eq!(cache.color(3, 0), 0x1b);
    b.replace(0..0, "// intro\n").unwrap();
    cache.refresh(&b);
    assert_eq!(cache.color(1, 0), 0x1b);
    for (language, text, expected) in [
        (Language::Pascal, "BEGIN {comment}", 0x1b),
        (Language::Python, "# comment", 0x1a),
        (Language::Json, "\"key\": 42", 0x1e),
        (Language::Toml, "true", 0x1b),
        (Language::Plain, "fn", 0x1f),
    ] {
        let b = Buffer::new(text.into()).unwrap();
        let mut c = Cache::new(language);
        c.refresh(&b);
        assert_eq!(c.color(0, 0), expected);
    }
}

#[test]
fn safe_save_preserves_bytes_bom_endings_final_newline_and_permissions() -> io::Result<()> {
    let temp = Temp::new()?;
    for (index, text) in ["a\r\nb\r\n", "a\nb", "a\rb\r", "a\r\nb\nc\r", "", "é終\n"]
        .iter()
        .enumerate()
    {
        let path = temp.0.join(format!("file-{index}"));
        let mut bytes = vec![0xef, 0xbb, 0xbf];
        bytes.extend_from_slice(text.as_bytes());
        fs::write(&path, &bytes)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o640))?;
        }
        let (b, snapshot, bom) = storage::load(&path).unwrap();
        assert!(bom);
        assert_eq!(&b.text, text);
        let snapshot = storage::save(&path, &b.text, bom, Some(&snapshot)).unwrap();
        assert_eq!(fs::read(&path)?, bytes);
        storage::save(&path, &b.text, bom, Some(&snapshot)).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o640);
        }
    }
    assert!(
        !fs::read_dir(&temp.0)?
            .any(|entry| entry
                .is_ok_and(|e| e.file_name().to_string_lossy().starts_with(".dn-edit-")))
    );
    Ok(())
}

#[test]
fn storage_rejects_conflicts_existing_destinations_missing_files_and_binary() -> io::Result<()> {
    let temp = Temp::new()?;
    let path = temp.0.join("source");
    fs::write(&path, b"original")?;
    let (_, original, bom) = storage::load(&path).unwrap();
    fs::write(&path, b"external")?;
    assert!(storage::save(&path, "edited", bom, Some(&original)).is_err());
    assert_eq!(fs::read(&path)?, b"external");
    assert!(storage::save(&path, "unrelated", false, None).is_err());
    assert_eq!(fs::read(&path)?, b"external");
    let destination = temp.0.join("new");
    let saved = storage::save(&destination, "new text", false, None).unwrap();
    storage::save(&destination, "next", false, Some(&saved)).unwrap();
    assert_eq!(fs::read(&destination)?, b"next");
    fs::remove_file(&path)?;
    assert!(storage::save(&path, "edited", false, Some(&original)).is_err());
    assert!(!path.exists());
    assert!(storage::load(&temp.0).is_err());
    fs::write(&path, [0xff, 0xfe])?;
    assert!(storage::load(&path).is_err());
    fs::write(&path, b"has\0nul")?;
    assert!(storage::load(&path).is_err());
    let big = temp.0.join("large");
    fs::write(&big, vec![b'x'; MAX_BYTES + 1])?;
    assert!(storage::load(&big).is_err());
    assert!(storage::save(&temp.0.join("missing-parent/new"), "text", false, None).is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn storage_refuses_symlinks_hardlinks_replaced_identity_and_readonly() -> io::Result<()> {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let temp = Temp::new()?;
    let path = temp.0.join("source");
    fs::write(&path, b"keep")?;
    let (_, original, _) = storage::load(&path).unwrap();
    let link = temp.0.join("link");
    symlink(&path, &link)?;
    assert!(storage::load(&link).is_err());
    assert!(storage::save(&link, "bad", false, None).is_err());
    let hard = temp.0.join("hard");
    fs::hard_link(&path, &hard)?;
    assert!(storage::save(&path, "bad", false, Some(&original)).is_err());
    fs::remove_file(&hard)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o444))?;
    assert!(storage::save(&path, "bad", false, Some(&original)).is_err());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644))?;
    let replacement = temp.0.join("replacement");
    fs::write(&replacement, b"keep")?;
    fs::rename(&replacement, &path)?;
    assert!(storage::save(&path, "bad", false, Some(&original)).is_err());
    assert_eq!(fs::read(&path)?, b"keep");
    fs::remove_file(&link)?;
    symlink(temp.0.join("absent"), &link)?;
    assert!(storage::load(&link).is_err());
    Ok(())
}

#[test]
fn f4_worker_edit_undo_save_and_unsaved_close_cancel_discard() -> io::Result<()> {
    let temp = Temp::new()?;
    let path = temp.0.join("code.rs");
    fs::write(&path, b"fn main() {}\r\n")?;
    let mut app = temp.app()?;
    focus(&mut app, "code.rs");
    app.panels[0].toggle_selection();
    key(&mut app, KeyCode::F(4));
    wait(&mut app);
    assert!(app.dialogs.is_empty());
    assert_eq!(
        app.desktop.editor().unwrap().highlight.language,
        Language::Rust
    );
    editor::paste(&mut app, "// é終\n");
    assert!(buffer(&app).dirty());
    modified(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert!(!buffer(&app).dirty());
    modified(&mut app, KeyCode::Char('y'), KeyModifiers::CONTROL);
    assert!(buffer(&app).dirty());
    key(&mut app, KeyCode::F(2));
    wait(&mut app);
    assert!(!buffer(&app).dirty());
    assert_eq!(fs::read(&path)?, "// é終\r\nfn main() {}\r\n".as_bytes());
    key(&mut app, KeyCode::Char('x'));
    key(&mut app, KeyCode::Esc);
    assert!(matches!(
        app.desktop.editor().unwrap().modal,
        Some(Modal::ConfirmClose)
    ));
    key(&mut app, KeyCode::Enter);
    assert!(app.desktop.editor().unwrap().modal.is_none());
    key(&mut app, KeyCode::Esc);
    key(&mut app, KeyCode::Char('d'));
    assert!(app.desktop.editor().is_none());
    assert!(!app.quit);
    assert_eq!(app.panels[0].selected.len(), 1);
    assert!(!fs::read(&path)?.starts_with(b"x"));
    Ok(())
}

#[test]
fn new_document_save_as_and_all_quit_paths_preserve_unsaved_text() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    modified(&mut app, KeyCode::F(4), KeyModifiers::SHIFT);
    editor::paste(&mut app, "new é\n");
    modified(&mut app, KeyCode::Char('x'), KeyModifiers::ALT);
    assert!(!app.quit);
    key(&mut app, KeyCode::Char('s'));
    editor::paste(&mut app, "new.txt");
    key(&mut app, KeyCode::Enter);
    wait(&mut app);
    assert!(app.quit && app.desktop.editor().is_none());
    assert_eq!(fs::read(temp.0.join("new.txt"))?, "new é\n".as_bytes());
    let mut app = temp.app()?;
    editor::open(&mut app, true);
    editor::paste(&mut app, "keep");
    app.request_exit();
    key(&mut app, KeyCode::Esc);
    assert!(!app.quit && app.desktop.editor().is_some());
    modified(&mut app, KeyCode::Char('x'), KeyModifiers::ALT);
    key(&mut app, KeyCode::Char('c'));
    assert!(!app.quit);
    modified(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert!(!app.quit);
    app.request_exit();
    key(&mut app, KeyCode::Char('d'));
    assert!(app.quit);
    Ok(())
}

#[test]
fn failed_save_keeps_text_open_and_cancels_pending_exit() -> io::Result<()> {
    let temp = Temp::new()?;
    let path = temp.0.join("file");
    fs::write(&path, b"initial")?;
    let mut app = temp.app()?;
    focus(&mut app, "file");
    editor::open(&mut app, false);
    wait(&mut app);
    editor::paste(&mut app, "edit");
    fs::write(&path, b"external")?;
    editor::request_close(&mut app, CloseIntent::Quit);
    key(&mut app, KeyCode::Char('s'));
    wait(&mut app);
    assert!(!app.quit && buffer(&app).dirty());
    assert!(!app.dialogs.is_empty());
    assert_eq!(fs::read(&path)?, b"external");
    key(&mut app, KeyCode::Esc);
    key(&mut app, KeyCode::Char('!'));
    assert!(buffer(&app).text.contains('!'));
    modified(&mut app, KeyCode::F(2), KeyModifiers::SHIFT);
    editor::paste(&mut app, "copy.txt");
    key(&mut app, KeyCode::Enter);
    wait(&mut app);
    assert!(!buffer(&app).dirty());
    assert_eq!(fs::read(&path)?, b"external");
    assert_eq!(
        fs::read_to_string(temp.0.join("copy.txt"))?,
        buffer(&app).text
    );
    Ok(())
}

#[test]
fn selection_clipboard_search_goto_and_mouse_colored_rendering() -> io::Result<()> {
    let temp = Temp::new()?;
    let path = temp.0.join("example.rs");
    fs::write(&path, "fn main() {\n\tlet 終 = 42; // hello\n}\n")?;
    let mut app = temp.app()?;
    focus(&mut app, "example.rs");
    menu::execute(&mut app, menu::Action::Key(4));
    wait(&mut app);
    for (width, height) in [(80, 25), (120, 40), (161, 30), (40, 10), (80, 25)] {
        render(&mut app, width, height)?;
    }
    let terminal = render(&mut app, 80, 25)?;
    assert_eq!(
        terminal.backend().buffer()[(3, 2)].fg,
        app.config.palette.color(11)
    ); // fn: DOS cyan
    key(&mut app, KeyCode::Right);
    modified(&mut app, KeyCode::Right, KeyModifiers::SHIFT);
    modified(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert!(!app.quit);
    key(&mut app, KeyCode::End);
    modified(&mut app, KeyCode::Char('v'), KeyModifiers::CONTROL);
    assert!(buffer(&app).dirty());
    modified(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
    key(&mut app, KeyCode::F(7));
    editor::paste(&mut app, "hello");
    key(&mut app, KeyCode::Enter);
    assert_eq!(&buffer(&app).text[buffer(&app).selected()], "hello");
    modified(&mut app, KeyCode::Char('g'), KeyModifiers::ALT);
    editor::paste(&mut app, "3");
    key(&mut app, KeyCode::Enter);
    assert_eq!(buffer(&app).row(), 2);
    render(&mut app, 80, 25)?;
    click(&mut app, A::Position(0, false));
    assert_eq!(buffer(&app).cursor, 0);
    render(&mut app, 80, 25)?;
    click(&mut app, A::MenuRoot(6));
    render(&mut app, 80, 25)?;
    click(&mut app, A::MenuRow(1));
    assert!(!app.desktop.editor().unwrap().colors);
    render(&mut app, 80, 25)?;
    click(&mut app, A::Help);
    render(&mut app, 80, 25)?;
    assert!(app
        .mouse
        .regions
        .iter()
        .all(|(_, target)| *target == Target::Editor(A::Cancel)));
    click(&mut app, A::Cancel);
    key(&mut app, KeyCode::Esc);
    assert!(app.desktop.editor().is_none());
    Ok(())
}
