use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use dn::{
    app::App,
    bookmarks,
    config::Config,
    input, menu,
    mouse::{self, Target},
    navigation_tools::{self, Action as Nav, Modal},
    panel::SortMode,
    pattern, ui,
    viewer::{self, Action as View, Contents, SearchMemory, Viewer},
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
            "dn-gaps-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
    fn app(&self) -> io::Result<App> {
        let mut app = App::new(self.0.clone())?;
        wait(&mut app, |a| a.panels.iter().all(|p| !p.loading));
        Ok(app)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn wait(app: &mut App, done: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done(app) {
        app.poll();
        assert!(Instant::now() < deadline, "background timeout");
        thread::sleep(Duration::from_millis(1));
    }
}
fn key(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    input::handle(app, KeyEvent::new(code, modifiers));
}
fn draw(app: &mut App, w: u16, h: u16) -> io::Result<String> {
    let mut terminal = Terminal::new(TestBackend::new(w, h))?;
    terminal.draw(|f| ui::draw(f, app))?;
    for (r, _) in &app.mouse.regions {
        assert!(r.right() <= w && r.bottom() <= h);
    }
    Ok(terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect())
}
fn click(app: &mut App, target: Target) {
    let r = app
        .mouse
        .regions
        .iter()
        .find(|(_, t)| *t == target)
        .expect("mouse target")
        .0;
    mouse::handle(
        app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: r.x,
            row: r.y,
            modifiers: KeyModifiers::NONE,
        },
    );
}
#[test]
fn bookmark_encoding_configuration_native_legacy_and_invalid() -> io::Result<()> {
    let t = Temp::new()?;
    let mut config = Config::default();
    let path = t.0.join("子 = name\nline");
    assert_eq!(
        bookmarks::decode(&bookmarks::encode(&path)),
        Some(path.clone())
    );
    config.bookmarks[3] = Some(path);
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        config.bookmarks[0] = Some(t.0.join(std::ffi::OsString::from_vec(vec![b'n', 255])));
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        config.bookmarks[0] = Some(t.0.join(std::ffi::OsString::from_wide(&[0xd800])));
    }
    let destination = t.0.join("config.conf");
    config.save(&destination)?;
    assert_eq!(Config::read(&destination)?.bookmarks, config.bookmarks);
    for value in [
        "",
        "unix:0",
        "windows:0",
        "foreign:ff",
        "unix:gg",
        "unix:00",
        "windows:0000",
    ] {
        assert!(bookmarks::decode(value).is_none(), "{value}");
    }
    fs::write(
        &destination,
        "config_version=2\n[file_manager]\nhidden=false\n[bookmarks]\n1=\"foreign:abcd\"\n9=\"bad\"\n500=\"unix:ff\"\n",
    )?;
    let config = Config::read(&destination)?;
    assert!(!config.show_hidden);
    assert!(config.bookmarks.iter().all(Option::is_none));
    fs::write(
        &destination,
        "config_version=2\n[file_manager]\nhidden=true\n",
    )?;
    assert!(Config::read(&destination)?
        .bookmarks
        .iter()
        .all(Option::is_none));
    Ok(())
}
#[test]
fn bookmark_shortcuts_dialog_mouse_and_missing_directory() -> io::Result<()> {
    let t = Temp::new()?;
    fs::create_dir(t.0.join("child"))?;
    let mut app = t.app()?;
    key(&mut app, KeyCode::Char('3'), KeyModifiers::CONTROL);
    assert_eq!(app.config.bookmarks[2], Some(t.0.clone()));
    app.panels[0].go_to(t.0.join("child"), false);
    wait(&mut app, |a| !a.panels[0].loading);
    key(&mut app, KeyCode::Char('3'), KeyModifiers::ALT);
    wait(&mut app, |a| !a.panels[0].loading);
    assert_eq!(app.panels[0].path, t.0);
    navigation_tools::open_bookmarks(&mut app);
    draw(&mut app, 80, 25)?;
    click(&mut app, Target::Navigation(Nav::Slot(8)));
    click(&mut app, Target::Navigation(Nav::Set));
    assert_eq!(app.config.bookmarks[8], Some(t.0.clone()));
    draw(&mut app, 120, 40)?;
    click(&mut app, Target::Navigation(Nav::Clear));
    assert!(app.config.bookmarks[8].is_none());
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    app.config.bookmarks[0] = Some(t.0.join("missing"));
    navigation_tools::go(&mut app, 0);
    wait(&mut app, |a| !a.panels[0].loading);
    assert_eq!(app.panels[0].path, t.0);
    assert!(!app.dialogs.is_empty());
    app.dialogs.clear();
    navigation_tools::open_bookmarks(&mut app);
    key(&mut app, KeyCode::End, KeyModifiers::NONE);
    key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
    assert!(matches!(
        app.navigation_dialog,
        Some(Modal::Bookmarks { slot: 8, .. })
    ));
    draw(&mut app, 80, 25)?;
    Ok(())
}
#[test]
fn filtering_moves_cached_paths_preserves_marks_and_visible_targets() -> io::Result<()> {
    let t = Temp::new()?;
    fs::write(t.0.join("alpha.rs"), "aaa")?;
    fs::write(t.0.join("beta.txt"), "bb")?;
    fs::create_dir(t.0.join("folder.rs"))?;
    let mut app = t.app()?;
    let panel = &mut app.panels[0];
    panel.cursor = panel
        .entries
        .iter()
        .position(|e| e.name == "alpha.rs")
        .unwrap();
    panel
        .selected
        .extend(["alpha.rs".into(), "beta.txt".into()]);
    let original = panel
        .entries
        .iter()
        .find(|e| e.name == "alpha.rs")
        .unwrap()
        .path
        .as_os_str()
        .as_encoded_bytes()
        .as_ptr();
    panel.set_filter("*.rs".into());
    assert!(!panel.loading);
    assert_eq!(panel.filtered_marks(), 1);
    assert_eq!(panel.targets(), [t.0.join("alpha.rs")]);
    assert_eq!(panel.total_files, 1);
    assert_eq!(panel.total_bytes, 3);
    assert!(panel.entries.iter().any(|e| e.parent));
    assert!(panel.entries.iter().any(|e| e.name == "folder.rs"));
    assert_eq!(panel.entries[panel.cursor].name, "alpha.rs");
    assert_eq!(
        panel
            .entries
            .iter()
            .find(|e| e.name == "alpha.rs")
            .unwrap()
            .path
            .as_os_str()
            .as_encoded_bytes()
            .as_ptr(),
        original
    );
    let screen = draw(&mut app, 80, 25)?;
    assert!(screen.contains("filtered marks"));
    assert!(screen.contains("Filter:"));
    pattern::apply(&mut app.panels[0], "*", false);
    assert_eq!(app.panels[0].filtered_marks(), 1);
    assert!(app.panels[0].only_filtered_marks());
    assert!(app.panels[0].targets().is_empty());
    app.prompt_operation(dn::operations::Kind::Delete, false);
    assert!(app.prompt.is_none());
    assert!(app.dialogs.last().unwrap().message.contains("filtered out"));
    app.dialogs.clear();
    dn::file_tools::open_export(&mut app);
    assert!(app.file_tools.dialog.is_none());
    assert!(app.dialogs.last().unwrap().message.contains("filtered out"));
    app.panels[0].set_filter(String::new());
    assert!(app.panels[0]
        .selected
        .contains(std::ffi::OsStr::new("beta.txt")));
    assert_eq!(app.panels[0].targets(), [t.0.join("beta.txt")]);
    Ok(())
}
#[test]
fn filter_cancel_sort_refresh_hidden_files_native_names_and_navigation() -> io::Result<()> {
    let t = Temp::new()?;
    fs::write(t.0.join("alpha.rs"), "aaa")?;
    fs::write(t.0.join("beta.txt"), "bb")?;
    fs::write(t.0.join(".hidden.rs"), "h")?;
    fs::create_dir(t.0.join("child"))?;
    let mut app = t.app()?;
    key(&mut app, KeyCode::Delete, KeyModifiers::ALT);
    navigation_tools::paste(&mut app, "*.rs");
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.panels[0].filter.is_empty());
    navigation_tools::open_filter(&mut app);
    navigation_tools::paste(&mut app, "*.rs");
    draw(&mut app, 80, 25)?;
    click(&mut app, Target::Navigation(Nav::ApplyFilter));
    assert_eq!(app.panels[0].filter, "*.rs");
    app.panels[0].set_sort(SortMode::Size);
    app.panels[0].refresh(false);
    wait(&mut app, |a| !a.panels[0].loading);
    assert_eq!(app.panels[0].filter, "*.rs");
    assert!(!app.panels[0].entries.iter().any(|e| e.name == ".hidden.rs"));
    app.panels[0].selected.insert("beta.txt".into());
    fs::remove_file(t.0.join("beta.txt"))?;
    app.panels[0].refresh(false);
    wait(&mut app, |a| !a.panels[0].loading);
    assert!(!app.panels[0]
        .selected
        .contains(std::ffi::OsStr::new("beta.txt")));
    app.panels[0].go_to(t.0.join("missing"), false);
    wait(&mut app, |a| !a.panels[0].loading);
    assert_eq!(app.panels[0].filter, "*.rs");
    app.dialogs.clear();
    app.panels[0].go_to(t.0.join("child"), false);
    wait(&mut app, |a| !a.panels[0].loading);
    assert!(app.panels[0].filter.is_empty());
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        app.panels[0].go_to(t.0.clone(), true);
        wait(&mut app, |a| !a.panels[0].loading);
        let raw = std::ffi::OsString::from_vec(vec![b'n', 255]);
        let entry = app.panels[0]
            .entries
            .iter_mut()
            .find(|e| !e.parent && !e.directory)
            .unwrap();
        entry.name = raw.clone();
        entry.path = t.0.join(&raw);
        app.panels[0].selected.insert(raw.clone());
        app.panels[0].set_filter("*.rs".into());
        assert!(app.panels[0].filtered_marks() > 0);
        app.panels[0].set_filter("*".into());
        assert!(app.panels[0].entries.iter().any(|e| e.name == raw));
    }
    Ok(())
}
#[test]
fn selection_commands_and_compare_preserve_excluded_marks() -> io::Result<()> {
    let t = Temp::new()?;
    fs::write(t.0.join("a.rs"), "a")?;
    fs::write(t.0.join("b.txt"), "b")?;
    let mut app = t.app()?;
    app.panels[0].selected.insert("b.txt".into());
    app.panels[0].set_filter("*.rs".into());
    key(&mut app, KeyCode::Char('a'), KeyModifiers::CONTROL);
    assert_eq!(app.panels[0].selected.len(), 2);
    menu::execute(&mut app, menu::Action::InvertSelection);
    assert_eq!(app.panels[0].selected.len(), 1);
    assert_eq!(app.panels[0].filtered_marks(), 1);
    dn::file_tools::compare::apply(&mut app.panels, Default::default());
    assert!(app.panels[0]
        .selected
        .contains(std::ffi::OsStr::new("b.txt")));
    key(&mut app, KeyCode::Char('u'), KeyModifiers::CONTROL);
    assert!(app.panels[0].selected.is_empty());
    Ok(())
}
fn loaded_viewer(path: PathBuf) -> io::Result<Viewer> {
    let mut v = Viewer::open(path)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while v.contents.is_none() {
        if let Some(result) = v.poll() {
            result.map_err(io::Error::other)?;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    v.layout(80);
    Ok(v)
}
#[test]
fn viewer_search_forward_reverse_wrap_unicode_and_case_mapping() -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("text");
    fs::write(&path, "Alpha 子 alpha\nİstanbul\nEND alpha\n")?;
    let mut v = loaded_viewer(path)?;
    let mut memory = SearchMemory {
        query: "alpha".into(),
        case_sensitive: true,
    };
    assert!(v.find(&memory, false, false).unwrap());
    assert_eq!(v.current.as_ref().unwrap().range, 10..15);
    v.find(&memory, false, true).unwrap();
    assert_eq!(v.current.as_ref().unwrap().line, 2);
    v.find(&memory, false, true).unwrap();
    assert_eq!(v.current.as_ref().unwrap().line, 0);
    v.find(&memory, true, true).unwrap();
    assert_eq!(v.current.as_ref().unwrap().line, 2);
    v.offset = 0;
    memory.case_sensitive = false;
    v.find(&memory, false, false).unwrap();
    assert_eq!(v.current.as_ref().unwrap().range, 0..5);
    memory.query = "i\u{307}stanbul".into();
    v.find(&memory, false, false).unwrap();
    assert_eq!(v.current.as_ref().unwrap().line, 1);
    assert_eq!(v.current.as_ref().unwrap().range, 0..9);
    let old = v.offset;
    memory.query = "missing".into();
    assert!(!v.find(&memory, false, false).unwrap());
    assert_eq!(v.offset, old);
    assert_eq!(v.notice, "No match");
    v.contents.as_mut().unwrap().truncated = true;
    assert!(!v.find(&memory, false, false).unwrap());
    assert_eq!(v.notice, "No match in loaded preview");
    memory.query.clear();
    assert!(v.find(&memory, false, false).is_err());
    Ok(())
}
#[test]
fn viewer_wrapping_graphemes_visual_rows_and_anchor_resizing() -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("text");
    fs::write(&path, format!("{}\nsecond\n", "a👩‍💻e\u{301}子".repeat(12)))?;
    let mut v = loaded_viewer(path)?;
    v.layout(10);
    v.toggle_wrap();
    assert!(v.wrap);
    assert!(v.rows.len() > 3);
    for row in &v.rows {
        let text = &v.contents.as_ref().unwrap().lines[row.line][row.range.clone()];
        assert!(ratatui::text::Span::raw(text).width() <= 10);
        assert!(!text.starts_with('\u{200d}'));
        assert!(!text.starts_with('\u{301}'));
    }
    v.step(3);
    let anchor = v.anchor();
    assert_eq!(anchor.0, 0);
    v.layout(13);
    let next = v.anchor();
    assert_eq!(next.0, anchor.0);
    assert!(next.1 <= anchor.1);
    v.toggle_wrap();
    assert!(!v.wrap);
    assert_eq!(v.anchor().0, 0);
    assert!(v.horizontal > 0);
    let before = v.anchor();
    v.toggle_wrap();
    assert_eq!(v.anchor().0, before.0);
    v.step(isize::MAX);
    assert_eq!(v.anchor().0, 1);
    v.step(isize::MIN);
    assert_eq!(v.offset, 0);
    Ok(())
}
#[test]
fn viewer_modal_cancel_mouse_controls_hex_and_session_query() -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("text");
    fs::write(&path, format!("{} find\nfind end\n", "x".repeat(160)))?;
    let mut app = t.app()?;
    app.viewer = Some(loaded_viewer(path.clone())?);
    draw(&mut app, 80, 25)?;
    click(&mut app, Target::Viewer(View::Wrap));
    assert!(app.viewer.as_ref().unwrap().wrap);
    key(&mut app, KeyCode::F(7), KeyModifiers::NONE);
    viewer::paste(&mut app, "find");
    draw(&mut app, 80, 25)?;
    click(&mut app, Target::Viewer(View::Submit));
    draw(&mut app, 80, 25)?;
    assert!(app.viewer.as_ref().unwrap().current.is_some());
    assert_eq!(app.viewer_search.query, "find");
    key(&mut app, KeyCode::F(7), KeyModifiers::SHIFT);
    assert_eq!(
        app.viewer.as_ref().unwrap().current.as_ref().unwrap().line,
        1
    );
    key(&mut app, KeyCode::F(7), KeyModifiers::CONTROL);
    assert_eq!(
        app.viewer.as_ref().unwrap().current.as_ref().unwrap().line,
        0
    );
    let offset = app.viewer.as_ref().unwrap().offset;
    let current = app.viewer.as_ref().unwrap().current.clone();
    key(&mut app, KeyCode::F(7), KeyModifiers::NONE);
    viewer::paste(&mut app, "bad");
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(app.viewer.as_ref().unwrap().offset, offset);
    assert_eq!(app.viewer.as_ref().unwrap().current, current);
    assert_eq!(app.viewer_search.query, "find");
    draw(&mut app, 120, 40)?;
    app.viewer = None;
    app.viewer = Some(loaded_viewer(path)?);
    assert!(!app.viewer.as_ref().unwrap().wrap);
    key(&mut app, KeyCode::F(7), KeyModifiers::NONE);
    assert_eq!(
        app.viewer
            .as_ref()
            .unwrap()
            .search_dialog
            .as_ref()
            .unwrap()
            .memory
            .query,
        "find"
    );
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    app.viewer
        .as_mut()
        .unwrap()
        .contents
        .as_mut()
        .unwrap()
        .binary = true;
    viewer::action(&mut app, View::Wrap);
    assert!(!app.viewer.as_ref().unwrap().wrap);
    viewer::action(&mut app, View::Find);
    assert!(app.viewer.as_ref().unwrap().search_dialog.is_none());
    draw(&mut app, 80, 25)?;
    assert!(!app
        .mouse
        .regions
        .iter()
        .any(|(_, t)| *t == Target::Viewer(View::Find)));
    Ok(())
}
#[test]
fn viewport_search_highlights_only_rendered_rows_and_menu_fit() -> io::Result<()> {
    let t = Temp::new()?;
    let mut app = t.app()?;
    menu::open(&mut app, 3);
    let screen = draw(&mut app, 80, 25)?;
    assert!(screen.contains("Quick directories"));
    assert!(screen.contains("Filename filter"));
    assert!(screen.contains("Quick search"));
    menu::close(&mut app);
    let path = t.0.join("text");
    fs::write(&path, "a")?;
    let mut v = loaded_viewer(path)?;
    v.contents = Some(Contents {
        lines: (0..1000).map(|n| format!("line {n}")).collect(),
        binary: false,
        truncated: false,
    });
    v.layout(79);
    v.offset = 500;
    v.find(
        &SearchMemory {
            query: "line".into(),
            case_sensitive: true,
        },
        false,
        false,
    )
    .unwrap();
    assert_eq!(v.current.as_ref().unwrap().line, 500);
    app.viewer = Some(v);
    draw(&mut app, 80, 25)?;
    assert!(app
        .mouse
        .regions
        .iter()
        .any(|(_, t)| *t == Target::CloseViewer));
    draw(&mut app, 60, 15)?;
    Ok(())
}

#[test]
fn filtered_copy_and_list_export_never_touch_excluded_entries() -> io::Result<()> {
    let t = Temp::new()?;
    fs::write(t.0.join("a.rs"), "a")?;
    fs::write(t.0.join("b.txt"), "b")?;
    fs::create_dir(t.0.join("out"))?;
    let mut app = t.app()?;
    app.panels[1].go_to(t.0.join("out"), true);
    wait(&mut app, |a| !a.panels[1].loading);
    app.panels[0]
        .selected
        .extend(["a.rs".into(), "b.txt".into()]);
    app.panels[0].set_filter("*.rs".into());
    app.prompt_operation(dn::operations::Kind::Copy, false);
    assert_eq!(app.prompt.as_ref().unwrap().sources, [t.0.join("a.rs")]);
    app.submit_prompt();
    wait(&mut app, |a| {
        a.job.is_none() && a.panels.iter().all(|p| !p.loading)
    });
    assert!(app.dialogs.is_empty());
    assert_eq!(fs::read_to_string(t.0.join("out/a.rs"))?, "a");
    assert!(!t.0.join("out/b.txt").exists());
    assert_eq!(app.panels[0].filtered_marks(), 1);
    app.panels[0].selected.insert("a.rs".into());
    dn::file_tools::open_export(&mut app);
    if let Some(dn::file_tools::ToolDialog::Export { paths, .. }) = &app.file_tools.dialog {
        assert_eq!(paths, &[t.0.join("a.rs")]);
    } else {
        panic!("export dialog")
    }
    dn::file_tools::accept(&mut app);
    wait(&mut app, |a| {
        !a.file_tools.busy() && a.panels.iter().all(|p| !p.loading)
    });
    assert_eq!(fs::read_to_string(t.0.join("files.lst"))?, "a.rs\n");
    assert_eq!(app.panels[0].filtered_marks(), 1);
    Ok(())
}
#[test]
fn viewer_search_case_checkbox_modal_scroll_and_match_highlighting() -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("text");
    fs::write(&path, "ALPHA\nalpha\n")?;
    let mut app = t.app()?;
    app.viewer = Some(loaded_viewer(path)?);
    draw(&mut app, 80, 25)?;
    key(&mut app, KeyCode::F(7), KeyModifiers::NONE);
    viewer::paste(&mut app, "alpha");
    draw(&mut app, 80, 25)?;
    click(&mut app, Target::Viewer(View::Case));
    assert!(
        !app.viewer
            .as_ref()
            .unwrap()
            .search_dialog
            .as_ref()
            .unwrap()
            .memory
            .case_sensitive
    );
    let offset = app.viewer.as_ref().unwrap().offset;
    mouse::handle(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 3,
            row: 3,
            modifiers: KeyModifiers::NONE,
        },
    );
    assert_eq!(app.viewer.as_ref().unwrap().offset, offset);
    click(&mut app, Target::Viewer(View::Submit));
    assert_eq!(
        app.viewer.as_ref().unwrap().current.as_ref().unwrap().line,
        0
    );
    let mut terminal = Terminal::new(TestBackend::new(80, 25))?;
    terminal.draw(|f| ui::draw(f, &mut app))?;
    assert_eq!(
        terminal.backend().buffer()[(0, 1)].bg,
        app.config.palette.color(3)
    );
    Ok(())
}

#[test]
fn viewer_clipping_never_cuts_a_grapheme_at_the_existing_character_limit() -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("text");
    fs::write(&path, format!("{}👩‍💻tail", "a".repeat(4095)))?;
    let contents = viewer::load(&path)?;
    assert!(contents.truncated);
    assert!(contents.lines[0].ends_with(" … [line clipped]"));
    assert!(!contents.lines[0].contains('👩'));
    Ok(())
}
