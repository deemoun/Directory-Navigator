use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use dn::testing::{backend::TestBackend, Terminal};
use dn::{
    app::App,
    config::Config,
    editor::{
        self,
        buffer::{Buffer, MAX_BYTES, MAX_LINES},
        menus,
        search::{self, ReplaceSession, SearchOptions},
        EditorAction as A, Modal,
    },
    input,
    mouse::{self, Target},
    ui,
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
            "dn-editor-tools-{}-{}",
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
    while app.panels.iter().any(|panel| panel.loading)
        || app.desktop.editor().is_some_and(|editor| editor.busy())
    {
        app.poll();
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
}
fn key(app: &mut App, code: KeyCode) {
    modified(app, code, KeyModifiers::NONE);
}
fn modified(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    input::handle(app, KeyEvent::new(code, modifiers));
}
fn buffer(app: &App) -> &Buffer {
    app.desktop.editor().unwrap().buffer.as_ref().unwrap()
}
fn begin(app: &mut App, text: &str) {
    editor::open(app, true);
    editor::paste(app, text);
}
fn render(app: &mut App, width: u16, height: u16) -> io::Result<Terminal<TestBackend>> {
    let mut terminal = Terminal::new(TestBackend::new(width, height))?;
    terminal.draw(|frame| ui::draw(frame, app))?;
    assert!(app
        .mouse
        .regions
        .iter()
        .all(|(r, _)| r.right() <= width && r.bottom() <= height));
    Ok(terminal)
}
fn click(app: &mut App, command: A) {
    let rect = app
        .mouse
        .regions
        .iter()
        .find(|(_, t)| *t == Target::Editor(command))
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
fn search_field(app: &mut App, query: &str, replacement: Option<&str>) {
    if let Some(Modal::Search(dialog)) = &mut app.desktop.editor_mut().unwrap().modal {
        dialog.query = query.into();
        dialog.cursors[0] = query.len();
        if let Some(replacement) = replacement {
            dialog.replacement = replacement.into();
            dialog.cursors[1] = replacement.len();
        }
    } else {
        panic!("search dialog missing");
    }
}
fn open_path(app: &mut App, path: &str) {
    key(app, KeyCode::F(3));
    editor::paste(app, path);
    key(app, KeyCode::Enter);
}

#[test]
fn word_navigation_matches_dn_line_stops_and_unicode_selection() {
    let mut b = Buffer::new("one, two_2\r\n\r\n  é終 👩‍💻 last\n".into()).unwrap();
    b.word_move(true, false);
    assert_eq!(b.cursor, 5);
    b.word_move(true, false);
    assert_eq!(b.cursor, b.lines[0].end);
    b.word_move(true, false);
    assert_eq!(b.cursor, b.lines[2].start + 2);
    b.word_move(true, true);
    assert_eq!(&b.text[b.selected()], "é終 👩‍💻 ");
    b.word_move(false, false);
    assert_eq!(b.cursor, b.lines[2].start + 2);
    b.word_move(false, false);
    assert_eq!(b.cursor, 5);
    b.word_move(false, false);
    assert_eq!(b.cursor, 0);
    b.word_move(false, false);
    assert_eq!(b.cursor, 0);
    b.move_to(b.text.len(), false);
    b.word_move(false, false);
    assert_eq!(&b.text[b.cursor..b.lines[2].end], "last");
}

#[test]
fn auto_indent_and_duplicate_preserve_whitespace_endings_cursor_and_history() {
    for ending in ["\n", "\r\n", "\r"] {
        let text = format!("\t  first  {ending}second");
        let mut b = Buffer::new(text.clone()).unwrap();
        b.move_to(b.lines[0].start + 3, false);
        b.enter(true).unwrap();
        assert!(b.text.starts_with(&format!("\t  {ending}\t  first  ")));
        b.undo(false);
        assert_eq!(b.text, text);
        b.undo(true);
        assert_eq!(b.cursor, 6 + ending.len());
        b.undo(false);
        b.move_to(2, false);
        b.duplicate_line().unwrap();
        assert_eq!(
            b.text,
            format!("\t  first  {ending}\t  first  {ending}second")
        );
        assert_eq!(b.cursor, 2);
        b.undo(false);
        assert_eq!(b.text, text);
        b.undo(true);
        assert_eq!(b.cursor, 2);
        b.move_to(b.text.len(), false);
        b.duplicate_line().unwrap();
        assert!(b.text.ends_with(&format!("second{ending}second")));
    }
    let mut b = Buffer::new("  abc  ".into()).unwrap();
    b.move_to(0, false);
    b.enter(true).unwrap();
    assert_eq!(b.text, "\n  abc  ");
    b.undo(false);
    b.move_to(3, false);
    b.enter(false).unwrap();
    assert_eq!(b.text, "  a\nbc  ");
}

#[test]
fn unicode_search_case_mapping_whole_words_scope_and_backward_overlap() {
    let text = "é É foo foo_bar food İ i\u{307} 👩‍💻";
    let options = SearchOptions {
        case_sensitive: false,
        whole_words: true,
        ..Default::default()
    };
    let ranges = search::matches(text, "é", 0..text.len(), options).unwrap();
    assert_eq!(ranges.len(), 2);
    assert_eq!(
        search::matches(text, "foo", 0..text.len(), options)
            .unwrap()
            .len(),
        1
    );
    let ranges = search::matches(text, "İ", 0..text.len(), options).unwrap();
    assert_eq!(ranges.len(), 2);
    assert_eq!(&text[ranges[0].clone()], "İ");
    assert_eq!(&text[ranges[1].clone()], "i\u{307}");
    assert!(search::matches("i\u{307}", "i", 0..3, options)
        .unwrap()
        .is_empty());
    assert!(
        search::matches("👩‍💻", "👩", 0.."👩‍💻".len(), SearchOptions::default())
            .unwrap()
            .is_empty()
    );
    assert!(search::matches(text, "", 0..text.len(), options).is_err());
    assert!(search::matches(text, "foo", 1..text.len(), options).is_err());
    let backward = SearchOptions {
        backward: true,
        ..Default::default()
    };
    assert_eq!(
        search::matches("aaa", "aa", 0..3, backward).unwrap(),
        vec![1..3]
    );
    assert_eq!(
        search::matches("aaa\u{301}", "aa", 0..5, backward).unwrap(),
        vec![0..2]
    );
    assert_eq!(
        search::ordered(vec![0..1, 3..4, 6..7], false, 4),
        vec![6..7, 0..1, 3..4]
    );
    assert_eq!(
        search::ordered(vec![0..1, 3..4, 6..7], true, 4),
        vec![3..4, 0..1, 6..7]
    );
    assert_eq!(
        search::matches("x foo y foo", "foo", 2..5, options).unwrap(),
        vec![2..5]
    );
}

#[test]
fn replacement_snapshot_wrap_skip_growth_empty_and_atomic_all_undo() {
    let mut b = Buffer::new("a a a".into()).unwrap();
    let ranges = search::ordered(
        search::matches(&b.text, "a", 0..b.text.len(), SearchOptions::default()).unwrap(),
        false,
        2,
    );
    let mut session = ReplaceSession::new(ranges, "aa".into());
    session.yes(&mut b).unwrap();
    assert_eq!(b.text, "a aa a");
    session.skip();
    session.all(&mut b).unwrap();
    assert_eq!(b.text, "aa aa a");
    assert_eq!(session.changed, 2);
    b.undo(false);
    assert_eq!(b.text, "a aa a");
    b.undo(false);
    assert_eq!(b.text, "a a a");
    let ranges = search::matches(&b.text, "a", 0..b.text.len(), SearchOptions::default()).unwrap();
    let mut session = ReplaceSession::new(ranges, String::new());
    session.all(&mut b).unwrap();
    assert_eq!(b.text, "  ");
    b.undo(false);
    assert_eq!(b.text, "a a a");
    let mut session = ReplaceSession::new(vec![0..1, 2..3], "x".repeat(MAX_BYTES));
    assert!(session.all(&mut b).is_err());
    assert_eq!(b.text, "a a a");
    let mut session = ReplaceSession::new(std::iter::once(0..1).collect(), "\n".repeat(MAX_LINES));
    assert!(session.all(&mut b).is_err());
    assert_eq!(b.text, "a a a");
}

#[test]
fn original_keys_menu_precedence_and_deferred_function_keys() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    begin(&mut app, "one two");
    key(&mut app, KeyCode::F(10));
    assert!(app.desktop.editor().unwrap().menu.is_some());
    assert!(!app.quit);
    key(&mut app, KeyCode::Esc);
    assert!(app.desktop.editor().unwrap().menu.is_none());
    assert!(app.desktop.editor().unwrap().modal.is_none());
    for code in [KeyCode::F(4), KeyCode::F(5), KeyCode::F(8), KeyCode::F(9)] {
        key(&mut app, code);
    }
    assert_eq!(buffer(&app).text, "one two");
    assert!(app.desktop.editor().unwrap().modal.is_none());
    key(&mut app, KeyCode::Home);
    modified(
        &mut app,
        KeyCode::Right,
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    modified(&mut app, KeyCode::Insert, KeyModifiers::CONTROL);
    assert_eq!(app.editor_clipboard, "one ");
    modified(&mut app, KeyCode::Delete, KeyModifiers::SHIFT);
    assert_eq!(buffer(&app).text, "two");
    modified(&mut app, KeyCode::Insert, KeyModifiers::SHIFT);
    assert_eq!(buffer(&app).text, "one two");
    key(&mut app, KeyCode::F(6));
    assert_eq!(buffer(&app).text, "one two\none two");
    modified(&mut app, KeyCode::Backspace, KeyModifiers::ALT);
    assert_eq!(buffer(&app).text, "one two");
    modified(&mut app, KeyCode::Char('g'), KeyModifiers::ALT);
    assert!(matches!(
        app.desktop.editor().unwrap().modal,
        Some(Modal::Prompt { .. })
    ));
    key(&mut app, KeyCode::F(10));
    assert!(app.desktop.editor().unwrap().menu.is_none());
    key(&mut app, KeyCode::Esc);
    modified(&mut app, KeyCode::F(4), KeyModifiers::CONTROL);
    assert!(matches!(
        app.desktop.editor().unwrap().modal,
        Some(Modal::ConfirmClose)
    ));
    key(&mut app, KeyCode::Char('c'));
    modified(&mut app, KeyCode::Char('x'), KeyModifiers::ALT);
    key(&mut app, KeyCode::Char('d'));
    assert!(app.quit);
    Ok(())
}

#[test]
fn open_guard_cancel_discard_failed_load_and_clipboard_across_files() -> io::Result<()> {
    let temp = Temp::new()?;
    fs::write(temp.0.join("second"), "second\r\n")?;
    let mut app = temp.app()?;
    begin(&mut app, "original");
    editor::action(&mut app, A::SelectAll);
    editor::action(&mut app, A::Copy);
    open_path(&mut app, "second");
    assert!(matches!(
        app.desktop.editor().unwrap().modal,
        Some(Modal::ConfirmClose)
    ));
    key(&mut app, KeyCode::Esc);
    assert_eq!(buffer(&app).text, "original");
    open_path(&mut app, "missing");
    key(&mut app, KeyCode::Char('d'));
    wait(&mut app);
    assert!(!app.dialogs.is_empty());
    assert_eq!(buffer(&app).text, "original");
    assert!(buffer(&app).dirty());
    key(&mut app, KeyCode::Esc);
    open_path(&mut app, "second");
    key(&mut app, KeyCode::Char('d'));
    wait(&mut app);
    assert_eq!(buffer(&app).text, "second\r\n");
    assert_eq!(app.editor_clipboard, "original");
    key(&mut app, KeyCode::Esc);
    assert!(app.desktop.editor().is_none());
    editor::open(&mut app, true);
    modified(&mut app, KeyCode::Insert, KeyModifiers::SHIFT);
    assert_eq!(buffer(&app).text, "original");
    Ok(())
}

#[test]
fn open_after_save_as_and_failed_save_does_not_switch() -> io::Result<()> {
    let temp = Temp::new()?;
    fs::write(temp.0.join("second"), "second")?;
    let mut app = temp.app()?;
    begin(&mut app, "first");
    open_path(&mut app, "second");
    key(&mut app, KeyCode::Char('s'));
    editor::paste(&mut app, "first.txt");
    key(&mut app, KeyCode::Enter);
    wait(&mut app);
    assert_eq!(fs::read_to_string(temp.0.join("first.txt"))?, "first");
    assert_eq!(buffer(&app).text, "second");
    editor::paste(&mut app, "edit");
    fs::write(temp.0.join("second"), "external")?;
    open_path(&mut app, "first.txt");
    key(&mut app, KeyCode::Char('s'));
    wait(&mut app);
    assert_eq!(buffer(&app).text, "editsecond");
    assert!(buffer(&app).dirty());
    assert!(!app.dialogs.is_empty());
    assert_eq!(
        app.desktop.editor().unwrap().path.as_ref(),
        Some(&temp.0.join("second"))
    );
    Ok(())
}

#[test]
fn find_scope_reverse_origin_and_prompted_replace_cancel() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    begin(&mut app, "foo x foo y foo");
    key(&mut app, KeyCode::F(7));
    search_field(&mut app, "foo", None);
    if let Some(Modal::Search(dialog)) = &mut app.desktop.editor_mut().unwrap().modal {
        dialog.options.entire_scope = true;
    }
    key(&mut app, KeyCode::Enter);
    assert_eq!(buffer(&app).selected(), 0..3);
    modified(&mut app, KeyCode::F(7), KeyModifiers::SHIFT);
    assert_eq!(buffer(&app).selected(), 6..9);
    modified(&mut app, KeyCode::F(7), KeyModifiers::ALT);
    assert_eq!(buffer(&app).selected(), 0..3);
    assert!(!app.desktop.editor().unwrap().search_options.backward);
    editor::action(&mut app, A::SelectAll);
    modified(&mut app, KeyCode::F(7), KeyModifiers::CONTROL);
    search_field(&mut app, "foo", Some("bar"));
    if let Some(Modal::Search(dialog)) = &mut app.desktop.editor_mut().unwrap().modal {
        dialog.options.selected_scope = true;
    }
    key(&mut app, KeyCode::Enter);
    assert!(matches!(
        app.desktop.editor().unwrap().modal,
        Some(Modal::Replace(_))
    ));
    key(&mut app, KeyCode::Char('y'));
    key(&mut app, KeyCode::Char('n'));
    key(&mut app, KeyCode::Esc);
    assert_eq!(buffer(&app).text, "bar x foo y foo");
    assert!(app.desktop.editor().unwrap().notice.contains("1"));
    editor::action(&mut app, A::Undo);
    assert_eq!(buffer(&app).text, "foo x foo y foo");
    Ok(())
}

#[test]
fn change_all_single_undo_and_search_options_disabled_without_selection() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    begin(&mut app, "a a a");
    modified(&mut app, KeyCode::F(7), KeyModifiers::CONTROL);
    search_field(&mut app, "a", Some("aa"));
    render(&mut app, 80, 25)?;
    assert!(!app
        .mouse
        .regions
        .iter()
        .any(|(_, t)| *t == Target::Editor(A::SearchChoice(6))));
    click(&mut app, A::SearchSubmit(true));
    assert_eq!(buffer(&app).text, "aa aa aa");
    assert!(app.desktop.editor().unwrap().notice.contains("3"));
    modified(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert_eq!(buffer(&app).text, "a a a");
    modified(&mut app, KeyCode::F(7), KeyModifiers::CONTROL);
    search_field(&mut app, "a", Some(""));
    if let Some(Modal::Search(dialog)) = &mut app.desktop.editor_mut().unwrap().modal {
        dialog.options.prompt = false;
    }
    key(&mut app, KeyCode::Enter);
    assert_eq!(buffer(&app).text, "  ");
    Ok(())
}

#[test]
fn editor_preferences_tabs_menu_mouse_and_configuration_roundtrip() -> io::Result<()> {
    let temp = Temp::new()?;
    let path = temp.0.join("config");
    fs::write(
        &path,
        "config_version=2\n[file_manager]\nhidden=false\n[editor]\ntab_size=0\n",
    )?;
    assert!(Config::read(&path).is_err());
    fs::write(&path, "config_version=2\n")?;
    let config = Config::read(&path)?;
    assert_eq!(config.editor.tab_size, 8);
    assert!(config.editor.auto_indent && config.editor.expand_tabs && config.editor.syntax);
    let mut app = temp.app()?;
    begin(&mut app, "");
    key(&mut app, KeyCode::Tab);
    assert_eq!(buffer(&app).text, "        ");
    for (width, height) in [(80, 25), (120, 40), (161, 30)] {
        render(&mut app, width, height)?;
        click(&mut app, A::MenuRoot(6));
        render(&mut app, width, height)?;
        assert!(app
            .mouse
            .regions
            .iter()
            .any(|(_, t)| *t == Target::Editor(A::MenuRow(0))));
        key(&mut app, KeyCode::Esc);
    }
    modified(&mut app, KeyCode::Char('o'), KeyModifiers::ALT);
    key(&mut app, KeyCode::Char('i'));
    assert!(!app.config.editor.auto_indent);
    editor::action(&mut app, A::TabWidth);
    modified(&mut app, KeyCode::Char('a'), KeyModifiers::CONTROL);
    editor::paste(&mut app, "4");
    key(&mut app, KeyCode::Enter);
    assert_eq!(buffer(&app).tab_size, 4);
    editor::action(&mut app, A::ExpandTabs);
    key(&mut app, KeyCode::Tab);
    assert!(buffer(&app).text.ends_with('\t'));
    app.config.save(&path)?;
    let config = Config::read(&path)?;
    assert_eq!(config.editor, app.config.editor);
    key(&mut app, KeyCode::Esc);
    key(&mut app, KeyCode::Char('d'));
    editor::open(&mut app, true);
    assert_eq!(app.desktop.editor().unwrap().preferences, config.editor);
    Ok(())
}

#[test]
fn all_editor_menu_roots_mnemonics_mouse_targets_and_clipboard_modern_aliases() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    begin(&mut app, "alpha beta");
    for root in [0, 1, 2, 4, 5, 6] {
        menus::open(&mut app, root);
        render(&mut app, 80, 25)?;
        assert!(app
            .mouse
            .regions
            .iter()
            .all(|(_, t)| matches!(t, Target::Editor(_))));
        key(&mut app, KeyCode::Esc);
    }
    menus::open(&mut app, 3);
    assert!(app.desktop.editor().unwrap().menu.is_none());
    modified(&mut app, KeyCode::Char('a'), KeyModifiers::CONTROL);
    modified(&mut app, KeyCode::Char('x'), KeyModifiers::CONTROL);
    assert!(buffer(&app).text.is_empty());
    modified(&mut app, KeyCode::Char('v'), KeyModifiers::CONTROL);
    assert_eq!(buffer(&app).text, "alpha beta");
    modified(&mut app, KeyCode::Char('s'), KeyModifiers::ALT);
    key(&mut app, KeyCode::Char('f'));
    assert!(matches!(
        app.desktop.editor().unwrap().modal,
        Some(Modal::Search(_))
    ));
    key(&mut app, KeyCode::Esc);
    assert!(!app.quit);
    render(&mut app, 40, 10)?;
    assert!(app.mouse.regions.is_empty());
    Ok(())
}

#[test]
fn block_menu_reuses_selection_clipboard_actions_and_keeps_paragraph_dimmed() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    begin(&mut app, "alpha beta");
    assert_eq!(
        menus::LABELS.map(dn::menu::plain),
        [
            "File",
            "Edit",
            "Search",
            "Paragraph",
            "Block",
            "Misc",
            "Options"
        ]
    );
    for (w, h) in [(80, 25), (132, 40), (80, 25)] {
        let terminal = render(&mut app, w, h)?;
        assert!(app
            .mouse
            .regions
            .iter()
            .all(|(_, t)| *t != Target::Editor(A::MenuRoot(3))));
        assert!(app
            .mouse
            .regions
            .iter()
            .any(|(_, t)| *t == Target::Editor(A::MenuRoot(4))));
        let paragraph_x = menus::LABELS[..3]
            .iter()
            .map(|s| dn::menu::plain(s).len() as u16 + 3)
            .sum::<u16>()
            + 1;
        assert_eq!(
            terminal.backend().buffer()[(paragraph_x, 0)].fg,
            app.config.palette.dos(0x78).fg.unwrap()
        );
        assert!(app
            .mouse
            .regions
            .iter()
            .filter(|(_, t)| matches!(t, Target::Editor(A::MenuRoot(_))))
            .all(|(r, _)| r.right() <= w - 10));
        modified(&mut app, KeyCode::Char('b'), KeyModifiers::ALT);
        assert_eq!(app.desktop.editor().unwrap().menu, Some((4, 0)));
        render(&mut app, w, h)?;
        key(&mut app, KeyCode::Esc);
    }
    modified(&mut app, KeyCode::Char('b'), KeyModifiers::ALT);
    key(&mut app, KeyCode::Char('a'));
    assert_eq!(&buffer(&app).text[buffer(&app).selected()], "alpha beta");
    modified(&mut app, KeyCode::Char('b'), KeyModifiers::ALT);
    key(&mut app, KeyCode::Char('c'));
    assert_eq!(app.editor_clipboard, "alpha beta");
    modified(&mut app, KeyCode::Char('b'), KeyModifiers::ALT);
    key(&mut app, KeyCode::Char('t'));
    assert!(buffer(&app).text.is_empty());
    render(&mut app, 80, 25)?;
    click(&mut app, A::MenuRoot(4));
    render(&mut app, 80, 25)?;
    let paste = menus::items(4)
        .iter()
        .position(|i| i.action == Some(A::Paste))
        .unwrap();
    click(&mut app, A::MenuRow(paste));
    assert_eq!(buffer(&app).text, "alpha beta");
    modified(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
    assert!(buffer(&app).text.is_empty());
    modified(&mut app, KeyCode::Char('y'), KeyModifiers::CONTROL);
    assert_eq!(buffer(&app).text, "alpha beta");
    Ok(())
}
