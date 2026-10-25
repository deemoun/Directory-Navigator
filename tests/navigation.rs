use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
#[cfg(unix)]
use dn::panel;
use dn::testing::{backend::TestBackend, layout::Rect, Terminal};
use dn::{
    app::App,
    columns::ColumnPreset,
    config::Config,
    input,
    menu::{self, Action, MenuId},
    mouse::{self, Target},
    panel::SortMode,
    panel_layout::{PanelGeometry, PanelLayout},
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
            "dn-navigation-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
    fn app(&self) -> io::Result<App> {
        let mut app = App::new(self.0.clone())?;
        loaded(&mut app);
        Ok(app)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn loaded(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.panels.iter().any(|p| p.loading) {
        app.poll();
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    assert!(app.dialogs().is_empty());
}
fn key(app: &mut App, code: KeyCode) {
    input::handle(app, KeyEvent::new(code, KeyModifiers::NONE));
}
fn alt_search(app: &mut App) {
    input::handle(app, KeyEvent::new(KeyCode::F(7), KeyModifiers::ALT));
}
fn paste(app: &mut App, text: &str) {
    let search = app.modals.quick_search_mut().unwrap();
    search.paste(&mut app.panels[search.panel], text);
}
fn focus(app: &mut App, name: &str) {
    let panel = &mut app.panels[app.active];
    panel.cursor = panel
        .entries
        .iter()
        .position(|entry| entry.name == name)
        .unwrap();
}
fn highlighted(app: &App) -> String {
    app.panels[app.active].entries[app.panels[app.active].cursor]
        .name
        .to_string_lossy()
        .into()
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
fn click(app: &mut App, target: Target, modifiers: KeyModifiers) {
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
            modifiers,
        },
    );
}
fn activate(app: &mut App, action: Action) {
    let depth = app.modals.submenus().len();
    let row = menu::level_items(app, depth)
        .iter()
        .position(|item| item.action == action)
        .unwrap();
    menu::activate(app, depth, row);
}

#[test]
fn brief_column_navigation_pages_resize_and_mouse_geometry() -> io::Result<()> {
    let temp = Temp::new()?;
    for i in 0..77 {
        fs::write(temp.0.join(format!("file-{i:03}")), b"x")?;
    }
    let mut app = temp.app()?;
    render(&mut app, 80, 25)?;
    let rows = app.panels[0].viewport_rows;
    assert_eq!(rows, 15);
    assert_eq!(app.panels[0].viewport_columns, 3);
    key(&mut app, KeyCode::Right);
    assert_eq!(app.panels[0].cursor, rows);
    key(&mut app, KeyCode::Right);
    assert_eq!(app.panels[0].cursor, rows * 2);
    key(&mut app, KeyCode::Left);
    assert_eq!(app.panels[0].cursor, rows);
    key(&mut app, KeyCode::Home);
    key(&mut app, KeyCode::Left);
    assert_eq!(app.panels[0].cursor, 0);
    app.panels[0].cursor = rows - 1;
    key(&mut app, KeyCode::Down);
    assert_eq!(app.panels[0].cursor, rows);
    key(&mut app, KeyCode::Up);
    assert_eq!(app.panels[0].cursor, rows - 1);
    key(&mut app, KeyCode::Home);
    key(&mut app, KeyCode::PageDown);
    assert_eq!(app.panels[0].cursor, rows * 3);
    render(&mut app, 80, 25)?;
    assert_eq!(app.panels[0].offset, rows);
    key(&mut app, KeyCode::PageUp);
    assert_eq!(app.panels[0].cursor, 0);
    key(&mut app, KeyCode::End);
    key(&mut app, KeyCode::Right);
    assert_eq!(app.panels[0].cursor, 77);
    let selected_name = app.panels[0].entries[77].name.clone();
    app.panels[0].toggle_selection();
    for (width, height) in [(80, 25), (120, 40), (161, 30), (80, 25)] {
        render(&mut app, width, height)?;
        let geometry = PanelGeometry::new(
            Rect::new(1, 2, width.div_ceil(2) - 2, height - 5),
            PanelLayout::Brief,
        );
        let panel = &app.panels[0];
        assert_eq!(geometry.rows(), panel.viewport_rows);
        assert_eq!(panel.offset % geometry.rows(), 0);
        let mut cells = std::collections::BTreeSet::new();
        for (rect, target) in &app.mouse.regions {
            if let Target::Entry(0, index) = target {
                assert_eq!(Some(*rect), geometry.entry_rect(*index, panel.offset));
                for x in rect.x..rect.right() {
                    assert!(cells.insert((x, rect.y)));
                }
            }
        }
        app.mouse.reset_click();
        click(&mut app, Target::Entry(0, 77), KeyModifiers::NONE);
        assert_eq!(app.panels[0].cursor, 77);
        assert!(app.panels[0].selected.contains(&selected_name));
    }
    // Each of the three visible columns maps to its own entry index.
    key(&mut app, KeyCode::Home);
    render(&mut app, 80, 25)?;
    for index in [1, rows + 1, rows * 2 + 1] {
        click(&mut app, Target::Entry(0, index), KeyModifiers::CONTROL);
        assert!(app.panels[0]
            .selected
            .contains(&app.panels[0].entries[index].name));
    }
    let cursor = app.panels[0].cursor;
    menu::execute(&mut app, Action::Layout(PanelLayout::Details));
    render(&mut app, 80, 25)?;
    assert_eq!(app.panels[0].cursor, cursor);
    assert_eq!(app.panels[0].viewport_columns, 1);
    key(&mut app, KeyCode::Home);
    key(&mut app, KeyCode::PageDown);
    assert_eq!(app.panels[0].cursor, rows);
    render(&mut app, 40, 10)?;
    assert!(app.mouse.regions.is_empty());
    Ok(())
}

#[test]
fn layout_menu_marks_choice_and_keeps_cursor_selection_and_columns() -> io::Result<()> {
    let temp = Temp::new()?;
    fs::write(temp.0.join("source.rs"), b"text")?;
    let mut app = temp.app()?;
    focus(&mut app, "source.rs");
    app.panels[0].toggle_selection();
    app.config.columns[0] = ColumnPreset::NameSizeModified;
    menu::open(&mut app, 3);
    activate(&mut app, Action::Submenu(MenuId::Layout));
    let terminal = render(&mut app, 80, 25)?;
    let rect = app
        .mouse
        .regions
        .iter()
        .find(|(_, target)| *target == Target::MenuItem(1, 0))
        .unwrap()
        .0;
    assert_eq!(terminal.backend().buffer()[(rect.x, rect.y)].symbol(), "•");
    click(&mut app, Target::MenuItem(1, 1), KeyModifiers::NONE);
    assert_eq!(
        app.config.layouts,
        [PanelLayout::Details, PanelLayout::Brief]
    );
    let terminal = render(&mut app, 80, 25)?;
    let header: String = (0..40)
        .map(|x| terminal.backend().buffer()[(x, 2)].symbol())
        .collect();
    assert!(header.contains("Modified"));
    assert_eq!(highlighted(&app), "source.rs");
    assert_eq!(app.panels[0].selected.len(), 1);
    assert!(app.panels.iter().all(|panel| !panel.loading));
    menu::execute(&mut app, Action::Layout(PanelLayout::Brief));
    render(&mut app, 80, 25)?;
    assert_eq!(highlighted(&app), "source.rs");
    assert_eq!(app.config.columns[0], ColumnPreset::NameSizeModified);
    menu::open(&mut app, 3);
    activate(&mut app, Action::QuickSearch);
    assert!(app.modals.quick_search().is_some());
    key(&mut app, KeyCode::Esc);
    menu::open(&mut app, 3);
    activate(&mut app, Action::Submenu(MenuId::Sort));
    activate(&mut app, Action::Sort(SortMode::Extension));
    assert_eq!(app.panels[0].sort, SortMode::Extension);
    Ok(())
}

#[test]
fn quick_search_unicode_edit_cycle_cancel_and_no_match() -> io::Result<()> {
    let temp = Temp::new()?;
    for name in ["Alpha", "alphabet", "Éclair", "école", "beta", ".hidden"] {
        fs::write(temp.0.join(name), b"text")?;
    }
    fs::create_dir(temp.0.join("Alpine"))?;
    let mut app = temp.app()?;
    focus(&mut app, "beta");
    app.panels[0].toggle_selection();
    alt_search(&mut app);
    paste(&mut app, "AL");
    assert_eq!(highlighted(&app), "Alpine");
    key(&mut app, KeyCode::Down);
    assert_eq!(highlighted(&app), "Alpha");
    key(&mut app, KeyCode::Down);
    assert_eq!(highlighted(&app), "alphabet");
    key(&mut app, KeyCode::Down);
    assert_eq!(highlighted(&app), "Alpine");
    key(&mut app, KeyCode::Up);
    assert_eq!(highlighted(&app), "alphabet");
    key(&mut app, KeyCode::Char('z'));
    assert!(app.modals.quick_search().unwrap().no_match);
    assert_eq!(highlighted(&app), "alphabet");
    let terminal = render(&mut app, 80, 25)?;
    let footer: String = (0..40)
        .map(|x| terminal.backend().buffer()[(x, 20)].symbol())
        .collect();
    assert!(footer.contains("No match"));
    key(&mut app, KeyCode::Backspace);
    assert_eq!(highlighted(&app), "Alpine");
    key(&mut app, KeyCode::Esc);
    assert_eq!(highlighted(&app), "beta");
    assert_eq!(app.panels[0].selected.len(), 1);
    alt_search(&mut app);
    paste(&mut app, "É");
    assert_eq!(highlighted(&app), "Éclair");
    key(&mut app, KeyCode::Down);
    assert_eq!(highlighted(&app), "école");
    key(&mut app, KeyCode::Backspace);
    assert!(app.modals.quick_search().unwrap().query.is_empty());
    assert_eq!(highlighted(&app), "école");
    key(&mut app, KeyCode::Enter);
    assert!(app.modals.quick_search().is_none());
    assert_eq!(highlighted(&app), "école");
    assert!(app.panels.iter().all(|panel| !panel.loading));
    Ok(())
}

#[test]
fn quick_search_cached_visibility_command_and_mouse_handoff() -> io::Result<()> {
    let temp = Temp::new()?;
    fs::write(temp.0.join("alpha"), b"text")?;
    fs::write(temp.0.join(".hidden"), b"hidden")?;
    let mut app = temp.app()?;
    app.toggle_hidden();
    loaded(&mut app);
    alt_search(&mut app);
    paste(&mut app, ".");
    assert!(app.modals.quick_search().unwrap().no_match); // excludes parent and hidden entry
    key(&mut app, KeyCode::Esc);
    fs::write(temp.0.join("new-file"), b"new")?;
    alt_search(&mut app);
    paste(&mut app, "new");
    assert!(app.modals.quick_search().unwrap().no_match); // no new scan
    key(&mut app, KeyCode::Esc);
    alt_search(&mut app);
    paste(&mut app, "alpha");
    key(&mut app, KeyCode::F(3));
    assert!(app.modals.quick_search().is_none() && app.desktop.active_viewer().is_some());
    key(&mut app, KeyCode::Esc);
    alt_search(&mut app);
    paste(&mut app, "alpha");
    key(&mut app, KeyCode::Tab);
    assert!(app.modals.quick_search().is_none());
    assert_eq!(app.active, 1);
    alt_search(&mut app);
    paste(&mut app, "alpha");
    render(&mut app, 80, 25)?;
    click(&mut app, Target::Entry(0, 0), KeyModifiers::NONE);
    assert!(app.modals.quick_search().is_none());
    assert_eq!(app.active, 0);
    assert_eq!(app.panels[0].cursor, 0);
    alt_search(&mut app);
    paste(&mut app, "alpha");
    app.panels[0].parent(app.config.show_hidden);
    app.poll();
    assert!(app.modals.quick_search().is_none());
    Ok(())
}

#[test]
fn search_handles_disappearing_original_and_native_names() -> io::Result<()> {
    let temp = Temp::new()?;
    fs::write(temp.0.join("alpha"), b"text")?;
    fs::write(temp.0.join("original"), b"text")?;
    let mut app = temp.app()?;
    focus(&mut app, "original");
    alt_search(&mut app);
    paste(&mut app, "alpha");
    app.panels[0].entries.retain(|e| e.name != "original");
    key(&mut app, KeyCode::Esc);
    assert_eq!(highlighted(&app), "alpha");
    #[cfg(unix)]
    {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};
        let index = app.panels[0].cursor;
        app.panels[0].entries[index].name = OsString::from_vec(vec![b'a', 0xff]);
        let native = app.panels[0].entries[index].name.clone();
        alt_search(&mut app);
        paste(&mut app, "a");
        assert!(app.modals.quick_search().unwrap().no_match);
        key(&mut app, KeyCode::Esc);
        assert_eq!(app.panels[0].entries[index].name, native);
    }
    app.panels[0].entries.clear();
    app.panels[0].cursor = 0;
    alt_search(&mut app);
    paste(&mut app, "missing");
    key(&mut app, KeyCode::Up);
    key(&mut app, KeyCode::Down);
    assert!(app.modals.quick_search().unwrap().no_match);
    key(&mut app, KeyCode::Esc);
    render(&mut app, 80, 25)?;
    Ok(())
}

#[test]
fn extension_sort_native_values_shortcuts_and_cycle() -> io::Result<()> {
    let temp = Temp::new()?;
    for name in ["b.rs", "a.rs", "upper.RS", "plain", ".hidden", "z.txt"] {
        fs::write(temp.0.join(name), b"x")?;
    }
    fs::create_dir(temp.0.join("directory.z"))?;
    let mut app = temp.app()?;
    focus(&mut app, "b.rs");
    app.panels[0].toggle_selection();
    input::handle(
        &mut app,
        KeyEvent::new(KeyCode::F(4), KeyModifiers::CONTROL),
    );
    assert_eq!(app.panels[0].sort, SortMode::Extension);
    let names: Vec<_> = app.panels[0]
        .entries
        .iter()
        .map(|e| e.name.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        [
            "..",
            "directory.z",
            ".hidden",
            "plain",
            "upper.RS",
            "a.rs",
            "b.rs",
            "z.txt"
        ]
    );
    assert_eq!(highlighted(&app), "b.rs");
    assert_eq!(app.panels[0].selected.len(), 1);
    app.panels[0].set_sort(SortMode::Name);
    for expected in [
        SortMode::Extension,
        SortMode::Size,
        SortMode::Date,
        SortMode::Name,
    ] {
        input::handle(&mut app, KeyEvent::new(KeyCode::F(2), KeyModifiers::SHIFT));
        assert_eq!(app.panels[0].sort, expected);
    }
    #[cfg(unix)]
    {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};
        let mut entries = dn::filesystem::scan(&temp.0, true)?;
        entries.retain(|entry| !entry.parent && !entry.directory);
        entries[0].name = OsString::from_vec(vec![b'a', b'.', 0xff]);
        panel::sort_entries(&mut entries, SortMode::Extension);
        assert_eq!(
            entries.last().unwrap().name,
            OsString::from_vec(vec![b'a', b'.', 0xff])
        );
    }
    Ok(())
}

#[test]
fn configuration_defaults_invalid_and_independent_roundtrip() -> io::Result<()> {
    let temp = Temp::new()?;
    let file = temp.0.join("config");
    assert_eq!(Config::read(&file)?.layouts, [PanelLayout::Brief; 2]);
    fs::write(
        &file,
        "config_version=2\n[file_manager]\nhidden=false\n[panels.left]\ncolumns=\"name_size_modified\"\n[panels.right]\ncolumns=\"name_size\"\n",
    )?;
    let legacy = Config::read(&file)?;
    assert_eq!(legacy.layouts, [PanelLayout::Brief; 2]);
    assert_eq!(
        legacy.columns,
        [ColumnPreset::NameSizeModified, ColumnPreset::NameSize]
    );
    assert!(!legacy.show_hidden);
    fs::write(
        &file,
        "config_version=2\n[panels.left]\nlayout=\"brief\"\nsort=\"extension\"\n[panels.right]\nlayout=\"unknown\"\n",
    )?;
    assert!(Config::read(&file).is_err());
    fs::write(&file,"config_version=2\n[panels.left]\nlayout=\"brief\"\nsort=\"extension\"\n[panels.right]\nlayout=\"details\"\n")?;
    let mut config = Config::read(&file)?;
    assert_eq!(config.layouts, [PanelLayout::Brief, PanelLayout::Details]);
    assert_eq!(config.sorts[0], SortMode::Extension);
    config.sorts[1] = SortMode::Date;
    config.save(&file)?;
    let saved = Config::read(&file)?;
    assert_eq!(saved.layouts, config.layouts);
    assert_eq!(saved.sorts, config.sorts);
    let mut app = temp.app()?;
    app.apply_config(saved);
    assert_eq!(app.config.layouts, config.layouts);
    assert_eq!(app.panels[0].sort, SortMode::Extension);
    Ok(())
}

#[test]
fn brief_mouse_wheel_and_double_click_in_later_columns() -> io::Result<()> {
    let temp = Temp::new()?;
    for index in 0..40 {
        fs::create_dir(temp.0.join(format!("dir-{index:03}")))?;
    }
    let mut app = temp.app()?;
    render(&mut app, 80, 25)?;
    let index = app.panels[1].viewport_rows * 2 + 1;
    let rect = app
        .mouse
        .regions
        .iter()
        .find(|(_, target)| *target == Target::Entry(1, index))
        .unwrap()
        .0;
    click(&mut app, Target::Entry(1, index), KeyModifiers::CONTROL);
    assert_eq!(app.active, 1);
    mouse::handle(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: rect.x,
            row: rect.y,
            modifiers: KeyModifiers::NONE,
        },
    );
    assert_eq!(app.panels[1].cursor, index + 3);
    app.mouse.reset_click();
    let destination = app.panels[1].entries[index].path.clone();
    click(&mut app, Target::Entry(1, index), KeyModifiers::NONE);
    assert!(!app.panels[1].loading);
    render(&mut app, 80, 25)?;
    click(&mut app, Target::Entry(1, index), KeyModifiers::NONE);
    assert!(app.panels[1].loading);
    loaded(&mut app);
    assert_eq!(app.panels[1].path, destination);
    Ok(())
}
