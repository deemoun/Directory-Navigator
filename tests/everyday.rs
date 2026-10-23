use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
#[cfg(unix)]
use dn::pattern;
use dn::{
    app::App,
    columns::ColumnPreset,
    config::Config,
    file_info, formatting, input,
    menu::{self, Action, MenuId},
    mouse::{self, Target},
    pattern::Wildcard,
    ui,
};
use ratatui::{backend::TestBackend, Terminal};
use std::{
    ffi::OsStr,
    fs, io,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant, UNIX_EPOCH},
};

struct Temp(PathBuf);
impl Temp {
    fn new() -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "dn-everyday-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
    fn app(&self) -> io::Result<App> {
        let mut app = App::new(self.0.clone())?;
        wait(&mut app, |a| a.panels.iter().all(|p| !p.loading));
        assert!(app.dialogs.is_empty());
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
        assert!(Instant::now() < deadline, "background task timed out");
        thread::sleep(Duration::from_millis(1));
    }
}
fn key(app: &mut App, code: KeyCode) {
    input::handle(app, KeyEvent::new(code, KeyModifiers::NONE));
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
fn click(app: &mut App, target: Target) {
    let rect = app
        .mouse
        .regions
        .iter()
        .find(|(_, t)| *t == target)
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
fn menu_action(app: &mut App, action: Action) {
    let target = app
        .mouse
        .regions
        .iter()
        .rev()
        .find_map(|(_, target)| {
            if let Target::MenuItem(depth, row) = *target {
                menu::level_items(app, depth)
                    .get(row)
                    .filter(|item| item.action == action)
                    .map(|_| *target)
            } else {
                None
            }
        })
        .unwrap();
    click(app, target);
}
fn focus(app: &mut App, name: &str) {
    app.panels[app.active].cursor = app.panels[app.active]
        .entries
        .iter()
        .position(|e| e.name == name)
        .unwrap();
}

#[test]
fn wildcard_whole_name_unicode_backtracking_and_literal_characters() {
    for (pattern, name, expected) in [
        ("*", "", true),
        ("", "", true),
        ("", "file", false),
        ("*.rs", "file.rs", true),
        ("*.rs", "file.RS", false),
        ("*.rs", "file.rs.bak", false),
        ("*.*", "folder", false),
        ("*.*", ".hidden", true),
        ("?.txt", "文.txt", true),
        ("?.txt", "😀.txt", true),
        ("?.txt", "ab.txt", false),
        ("??", "e\u{301}", true),
        ("a*b?c", "a12b34b5c", true),
        ("**a**?b*", "xxa文bzzz", true),
        ("*aab", "aaaab", true),
        ("*aab", "aaaa", false),
        ("[a]", "a", false),
        ("[a]", "[a]", true),
        ("a?", "a", false),
        ("a*", "a", true),
    ] {
        assert_eq!(
            Wildcard::new(pattern).matches(OsStr::new(name)),
            expected,
            "{pattern:?} / {name:?}"
        );
    }
}

#[test]
fn pattern_selection_uses_visible_cached_entries_and_preserves_other_marks() -> io::Result<()> {
    let temp = Temp::new()?;
    for name in ["one.rs", "two.rs", "keep.txt", ".hidden.rs", "UPPER.RS"] {
        fs::write(temp.0.join(name), b"x")?;
    }
    fs::create_dir(temp.0.join("folder.rs"))?;
    fs::write(temp.0.join("folder.rs/inside.rs"), b"nested")?;
    let mut app = temp.app()?;
    focus(&mut app, "keep.txt");
    app.panels[0].toggle_selection();
    let cursor = app.panels[0].cursor;
    // Remove a listed file: pattern application must not stat or re-scan it.
    fs::remove_file(temp.0.join("two.rs"))?;
    key(&mut app, KeyCode::Char('+'));
    assert_eq!(app.pattern.as_ref().unwrap().field, "*");
    app.pattern.as_mut().unwrap().paste("*.rs");
    render(&mut app, 80, 25)?;
    click(&mut app, Target::AcceptPattern);
    assert!(app.panels[0].selected.contains(OsStr::new("two.rs")));
    assert!(app.panels[0].selected.contains(OsStr::new("folder.rs")));
    assert!(app.panels[0].selected.contains(OsStr::new(".hidden.rs")));
    assert!(app.panels[0].selected.contains(OsStr::new("keep.txt")));
    assert!(!app.panels[0].selected.contains(OsStr::new("UPPER.RS")));
    assert!(!app.panels[0].selected.contains(OsStr::new("..")));
    assert!(!app.panels[0].selected.contains(OsStr::new("inside.rs")));
    assert_eq!(app.panels[0].cursor, cursor);
    assert!(!app.panels[0].loading);
    key(&mut app, KeyCode::Char('-'));
    app.pattern.as_mut().unwrap().paste("*.rs");
    key(&mut app, KeyCode::Enter);
    assert_eq!(
        app.panels[0].selected.iter().collect::<Vec<_>>(),
        [&std::ffi::OsString::from("keep.txt")]
    );
    key(&mut app, KeyCode::Char('+'));
    render(&mut app, 120, 40)?;
    click(&mut app, Target::CancelPattern);
    assert_eq!(app.panels[0].selected.len(), 1);
    app.toggle_hidden();
    wait(&mut app, |a| a.panels.iter().all(|p| !p.loading));
    key(&mut app, KeyCode::Char('+'));
    key(&mut app, KeyCode::Enter);
    assert!(!app.panels[0].selected.contains(OsStr::new(".hidden.rs")));
    Ok(())
}

#[cfg(unix)]
#[test]
fn undecodable_names_are_selected_by_star_without_path_conversion() -> io::Result<()> {
    use std::os::unix::ffi::OsStringExt;
    let temp = Temp::new()?;
    let raw = std::ffi::OsString::from_vec(vec![b'f', 255]);
    fs::write(temp.0.join("placeholder"), b"raw")?;
    let mut app = temp.app()?;
    // Some macOS filesystems/sandboxes reject invalid UTF-8 filenames. Exercise
    // the native cached entry directly: pattern selection performs no I/O.
    let entry = app.panels[0]
        .entries
        .iter_mut()
        .find(|entry| !entry.parent)
        .unwrap();
    entry.name = raw.clone();
    entry.path = temp.0.join(&raw);
    assert!(!Wildcard::new("f?").matches(&raw));
    assert_eq!(pattern::apply(&mut app.panels[0], "*", true), 1);
    assert_eq!(app.panels[0].targets(), [temp.0.join(&raw)]);
    pattern::apply(&mut app.panels[0], "*", false);
    app.panels[0].cursor = app.panels[0]
        .entries
        .iter()
        .position(|e| e.name == raw)
        .unwrap();
    key(&mut app, KeyCode::Insert);
    assert!(app.panels[0].selected.contains(&raw));
    Ok(())
}

#[test]
fn information_is_fresh_readonly_and_reports_missing_entries() -> io::Result<()> {
    let temp = Temp::new()?;
    fs::write(temp.0.join("file"), b"old")?;
    fs::create_dir(temp.0.join("directory"))?;
    let mut app = temp.app()?;
    focus(&mut app, "file");
    fs::write(temp.0.join("file"), b"new contents")?;
    let original = fs::metadata(temp.0.join("file"))?.modified()?;
    input::handle(
        &mut app,
        KeyEvent::new(KeyCode::Char('e'), KeyModifiers::ALT),
    );
    wait(&mut app, |a| {
        a.file_info.as_ref().unwrap().contents.is_some()
    });
    let contents = app
        .file_info
        .as_ref()
        .unwrap()
        .contents
        .as_ref()
        .unwrap()
        .as_ref()
        .unwrap();
    assert!(contents.lines.iter().any(|line| line == "Size: 12 bytes"));
    let modified = contents
        .lines
        .iter()
        .find(|line| line.starts_with("Modified"))
        .unwrap();
    let offset = modified.split_whitespace().last().unwrap();
    assert_eq!(offset.len(), 6);
    assert!(offset.starts_with(['+', '-']) && offset.contains(':'));
    render(&mut app, 80, 25)?;
    assert_eq!(app.mouse.regions.len(), 2);
    assert!(app
        .mouse
        .regions
        .iter()
        .all(|(_, t)| *t == Target::CloseFileInfo));
    click(&mut app, Target::CloseFileInfo);
    assert!(app.file_info.is_none());
    assert_eq!(fs::metadata(temp.0.join("file"))?.modified()?, original);
    assert_eq!(fs::read(temp.0.join("file"))?, b"new contents");
    let directory = file_info::inspect(&temp.0.join("directory"))?;
    assert!(directory
        .lines
        .iter()
        .any(|line| line.contains("contents not counted")));
    fs::remove_file(temp.0.join("file"))?;
    app.open_file_info();
    wait(&mut app, |a| {
        a.file_info.as_ref().unwrap().contents.is_some()
    });
    assert!(app
        .file_info
        .as_ref()
        .unwrap()
        .contents
        .as_ref()
        .unwrap()
        .is_err());
    let terminal = render(&mut app, 80, 25)?;
    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(text.contains("Cannot read file information"));
    key(&mut app, KeyCode::Enter);
    assert!(app.file_info.is_none());
    Ok(())
}

#[cfg(unix)]
#[test]
fn info_inspects_links_themselves_and_permission_errors() -> io::Result<()> {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let temp = Temp::new()?;
    fs::write(temp.0.join("target"), vec![0; 1024])?;
    symlink("target", temp.0.join("link"))?;
    symlink("absent", temp.0.join("broken"))?;
    for name in ["link", "broken"] {
        let info = file_info::inspect(&temp.0.join(name))?;
        assert!(info
            .lines
            .iter()
            .any(|line| line == "Entry type: Symbolic link"));
        assert!(info.lines.iter().any(|line| line.contains("link entry")));
        assert!(info
            .lines
            .iter()
            .any(|line| line.starts_with("Link target (not followed):")));
        assert!(!info.lines.iter().any(|line| line == "Size: 1024 bytes"));
    }
    fs::create_dir(temp.0.join("blocked"))?;
    fs::write(temp.0.join("blocked/child"), b"x")?;
    fs::set_permissions(temp.0.join("blocked"), fs::Permissions::from_mode(0o000))?;
    let enforced = fs::read_dir(temp.0.join("blocked")).is_err();
    let result = file_info::inspect(&temp.0.join("blocked/child"));
    fs::set_permissions(temp.0.join("blocked"), fs::Permissions::from_mode(0o700))?;
    if enforced {
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    }
    Ok(())
}

#[test]
fn info_long_paths_scroll_and_can_close_during_loading() -> io::Result<()> {
    let temp = Temp::new()?;
    let directory = temp
        .0
        .join("long-文-path".repeat(5))
        .join("another-long-directory".repeat(3));
    fs::create_dir_all(&directory)?;
    let path = directory.join("file.txt");
    fs::write(&path, b"x")?;
    let mut app = temp.app()?;
    app.file_info = Some(file_info::InfoDialog::open(path.clone())?);
    wait(&mut app, |a| {
        a.file_info.as_ref().unwrap().contents.is_some()
    });
    for _ in 0..20 {
        key(&mut app, KeyCode::Right);
    }
    key(&mut app, KeyCode::PageDown);
    assert!(app.file_info.as_ref().unwrap().offset > 0);
    assert!(app.file_info.as_ref().unwrap().horizontal > 0);
    render(&mut app, 80, 25)?;
    key(&mut app, KeyCode::Home);
    assert_eq!(app.file_info.as_ref().unwrap().horizontal, 0);
    assert_eq!(app.file_info.as_ref().unwrap().offset, 0);
    key(&mut app, KeyCode::Esc);
    app.file_info = Some(file_info::InfoDialog::open(path)?);
    key(&mut app, KeyCode::Esc);
    app.poll();
    assert!(app.file_info.is_none());
    Ok(())
}

#[test]
fn column_presets_live_preview_cancel_persistence_and_no_rescan() -> io::Result<()> {
    let temp = Temp::new()?;
    fs::write(temp.0.join("file.rs"), b"source")?;
    let mut app = temp.app()?;
    focus(&mut app, "file.rs");
    app.panels[0].toggle_selection();
    let cursor = app.panels[0].cursor;
    app.config.columns = [ColumnPreset::NameSize; 2];
    app.active = 1;
    input::handle(
        &mut app,
        KeyEvent::new(KeyCode::Char('k'), KeyModifiers::ALT),
    );
    assert_eq!(app.columns_dialog.as_ref().unwrap().cursor, 1);
    render(&mut app, 80, 25)?;
    click(&mut app, Target::ColumnChoice(0, ColumnPreset::Name));
    click(
        &mut app,
        Target::ColumnChoice(1, ColumnPreset::NameSizeModified),
    );
    assert_eq!(
        app.config.columns,
        [ColumnPreset::Name, ColumnPreset::NameSizeModified]
    );
    assert!(app.panels.iter().all(|panel| !panel.loading));
    render(&mut app, 120, 40)?;
    click(&mut app, Target::CancelColumns);
    assert_eq!(app.config.columns, [ColumnPreset::NameSize; 2]);
    assert_eq!(app.panels[0].cursor, cursor);
    assert_eq!(app.panels[0].selected.len(), 1);
    app.open_columns();
    key(&mut app, KeyCode::Right);
    key(&mut app, KeyCode::Enter);
    assert_eq!(
        app.config.columns,
        [ColumnPreset::NameSize, ColumnPreset::NameSizeModified]
    );
    app.config.save(&temp.0.join("config"))?;
    let config = Config::read(&temp.0.join("config"))?;
    assert_eq!(config.columns, app.config.columns);
    fs::write(
        temp.0.join("legacy"),
        "config_version=2\n[file_manager]\nhidden=false\n[panels.left]\nsort=\"size\"\n",
    )?;
    let legacy = Config::read(&temp.0.join("legacy"))?;
    assert_eq!(legacy.columns, [ColumnPreset::Name; 2]);
    assert!(!legacy.show_hidden);
    fs::write(
        temp.0.join("legacy"),
        "config_version=2\n[panels.left]\ncolumns=\"unknown\"\n[panels.right]\ncolumns=\"name\"\n",
    )?;
    assert!(Config::read(&temp.0.join("legacy")).is_err());
    Ok(())
}

#[test]
fn preset_rendering_dates_sizes_and_row_click_geometry() -> io::Result<()> {
    let temp = Temp::new()?;
    fs::write(temp.0.join("file"), b"a")?;
    let file = fs::OpenOptions::new()
        .write(true)
        .open(temp.0.join("file"))?;
    file.set_times(
        fs::FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(1_700_000_000)),
    )?;
    let mut app = temp.app()?;
    let row = app.panels[0]
        .entries
        .iter()
        .position(|e| e.name == "file")
        .unwrap();
    app.config.layouts = [dn::panel_layout::PanelLayout::Details; 2];
    for preset in ColumnPreset::ALL {
        app.config.columns = [preset; 2];
        for (width, height) in [(80, 25), (120, 40)] {
            let terminal = render(&mut app, width, height)?;
            let text = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            assert_eq!(
                text.contains("Modified"),
                preset == ColumnPreset::NameSizeModified
            );
            assert_eq!(text.contains("Size"), preset != ColumnPreset::Name);
            if preset == ColumnPreset::NameSizeModified {
                let expected =
                    formatting::modified(app.panels[0].entries[row].modified, width >= 104);
                assert!(text.contains(&expected));
            }
            app.mouse.reset_click();
            click(&mut app, Target::Entry(0, row));
            assert_eq!(app.panels[0].cursor, row);
        }
    }
    render(&mut app, 40, 10)?;
    assert!(app.mouse.regions.is_empty());
    assert_eq!(formatting::size(999_999_999), "999999999");
    for bytes in [
        1_000_000_000,
        (1_u64 << 30) - 1,
        (1_u64 << 40) - 1,
        u64::MAX,
    ] {
        assert!(formatting::size(bytes).len() <= 9);
    }
    assert_eq!(formatting::modified(None, false), "—");
    let before = formatting::local_timestamp(UNIX_EPOCH - Duration::from_millis(250)).unwrap();
    assert_eq!(before.timestamp(), -1);
    assert_eq!(before.timestamp_subsec_nanos(), 750_000_000);
    Ok(())
}

#[test]
fn new_menu_entries_open_their_mouse_dialogs() -> io::Result<()> {
    let temp = Temp::new()?;
    fs::write(temp.0.join("file"), b"x")?;
    let mut app = temp.app()?;
    focus(&mut app, "file");
    for selecting in [true, false] {
        menu::open(&mut app, 3);
        render(&mut app, 80, 25)?;
        menu_action(
            &mut app,
            if selecting {
                Action::SelectGroup
            } else {
                Action::UnselectGroup
            },
        );
        assert_eq!(app.pattern.as_ref().unwrap().selecting, selecting);
        key(&mut app, KeyCode::Esc);
    }
    menu::open(&mut app, 5);
    render(&mut app, 80, 25)?;
    menu_action(&mut app, Action::Submenu(MenuId::FileManager));
    render(&mut app, 80, 25)?;
    menu_action(&mut app, Action::Columns);
    assert!(app.columns_dialog.is_some());
    key(&mut app, KeyCode::Esc);
    menu::open(&mut app, 1);
    render(&mut app, 80, 25)?;
    menu_action(&mut app, Action::FileInfo);
    assert!(app.file_info.is_some());
    assert!(app.menu.is_none());
    key(&mut app, KeyCode::Esc);
    Ok(())
}

#[test]
fn function_bar_has_continuous_label_background_after_resize() -> io::Result<()> {
    use ratatui::style::Color;
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    app.config.shortcuts = dn::shortcuts::Profile::ClassicDn;
    app.config.palette = dn::palette::PaletteProfile::Terminal;
    let mut terminal = Terminal::new(TestBackend::new(80, 25))?;
    for (width, height) in [(80, 25), (120, 40), (161, 30), (80, 25)] {
        terminal.backend_mut().resize(width, height);
        terminal.draw(|frame| ui::draw(frame, &mut app))?;
        for column in 0..width {
            let cell = &terminal.backend().buffer()[(column, height - 1)];
            let number = cell.symbol().chars().all(|c| c.is_ascii_digit());
            assert_eq!(
                cell.bg,
                Color::Indexed(if number { 0 } else { 6 }),
                "unpainted function-bar cell at {column} in {width}x{height}"
            );
        }
    }
    Ok(())
}

#[test]
fn panel_footer_sizes_totals_paths_and_resized_mouse_rows() -> io::Result<()> {
    let temp = Temp::new()?;
    fs::write(temp.0.join("example.bin"), vec![0_u8; 12_345])?;
    fs::create_dir(temp.0.join("folder"))?;
    // Contents of subdirectories must not contribute to the visible total.
    fs::write(temp.0.join("folder").join("nested"), b"nested")?;
    let mut app = temp.app()?;
    assert_eq!(app.config.columns, [ColumnPreset::Name; 2]);
    assert_eq!(app.panels[0].total_files, 1);
    assert_eq!(app.panels[0].total_bytes, 12_345);
    focus(&mut app, "example.bin");
    for (width, height) in [(80, 25), (120, 40), (161, 30)] {
        let terminal = render(&mut app, width, height)?;
        let buffer = terminal.backend().buffer();
        let line = |y: u16| -> String { (0..width / 2).map(|x| buffer[(x, y)].symbol()).collect() };
        assert!(line(height - 6).contains("12,345"));
        assert!(line(height - 5).contains("No files selected"));
        for y in 2..height - 6 {
            assert!(!line(y).contains("12,345"));
            assert!(!line(y).contains("12345"));
        }
        assert_eq!(app.page_rows, usize::from(height - 10));
        assert!(app
            .mouse
            .regions
            .iter()
            .filter_map(|(rect, target)| { matches!(target, Target::Entry(_, _)).then_some(rect) })
            .all(|rect| rect.y < height - 6));
        let path: String = (0..width)
            .map(|x| buffer[(x, height - 2)].symbol())
            .collect();
        assert!(path.starts_with(&format!("{}>", temp.0.display())));
    }
    focus(&mut app, "folder");
    let terminal = render(&mut app, 80, 25)?;
    let footer: String = (0..40)
        .map(|x| terminal.backend().buffer()[(x, 19)].symbol())
        .collect();
    assert!(footer.contains("<SUB-DIR>"));
    app.panels[0].path = PathBuf::from(format!("/{}終点", "long/".repeat(30)));
    let terminal = render(&mut app, 80, 25)?;
    let path: String = (0..80)
        .map(|x| terminal.backend().buffer()[(x, 23)].symbol())
        .collect();
    assert!(path.starts_with('…'));
    assert!(path.replace(' ', "").ends_with("終点>"));
    assert!(ratatui::text::Span::raw(formatting::tail("abc終点>", 6)).width() <= 6);
    assert_eq!(formatting::grouped_bytes(0), "0");
    assert_eq!(formatting::grouped_bytes(1_234_567_890), "1,234,567,890");
    Ok(())
}
