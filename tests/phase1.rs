use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dn::testing::{backend::TestBackend, Terminal};
use dn::{app::App, filesystem, input, panel::Panel, ui};
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
            "dn-phase1-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn loaded(panel: &mut Panel) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while panel.loading {
        assert!(panel.poll().is_none());
        assert!(Instant::now() < deadline, "directory scan timed out");
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn directory_navigation_and_parent_restore() -> io::Result<()> {
    let temp = Temp::new()?;
    fs::create_dir(temp.0.join("子 directory"))?;
    let mut panel = Panel::new(temp.0.clone(), true)?;
    loaded(&mut panel);
    panel.cursor = panel
        .entries
        .iter()
        .position(|e| e.directory && !e.parent)
        .unwrap();
    panel.enter(true);
    loaded(&mut panel);
    assert_eq!(panel.path, temp.0.join("子 directory"));
    panel.parent(true);
    loaded(&mut panel);
    assert_eq!(panel.path, temp.0);
    assert_eq!(panel.entries[panel.cursor].name, "子 directory");
    panel.step(isize::MAX);
    assert_eq!(panel.cursor, panel.entries.len() - 1);
    panel.step(isize::MIN);
    assert_eq!(panel.cursor, 0);
    Ok(())
}

#[test]
fn directories_first_sorting_hidden_files_and_disappearing_entries() -> io::Result<()> {
    let temp = Temp::new()?;
    fs::write(temp.0.join("z.txt"), b"hello")?;
    fs::write(temp.0.join("a.txt"), b"a")?;
    fs::write(temp.0.join(".hidden"), b"a")?;
    fs::create_dir(temp.0.join("z-dir"))?;
    let entries = filesystem::scan(&temp.0, false)?;
    let names: Vec<_> = entries.iter().map(|e| e.name.to_string_lossy()).collect();
    assert_eq!(names, ["..", "z-dir", "a.txt", "z.txt"]);
    assert_eq!(filesystem::scan(&temp.0, true)?.len(), 5);
    assert!(filesystem::scan(&temp.0.join("absent"), true).is_err());
    assert!(filesystem::scan(&temp.0.join("a.txt"), true).is_err());
    let mut panel = Panel::new(temp.0.clone(), true)?;
    loaded(&mut panel);
    panel.cursor = panel
        .entries
        .iter()
        .position(|e| e.name == "z-dir")
        .unwrap();
    fs::remove_dir(temp.0.join("z-dir"))?;
    panel.enter(true);
    let deadline = Instant::now() + Duration::from_secs(5);
    let error = loop {
        if let Some(error) = panel.poll() {
            break error;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    };
    assert!(error.contains("z-dir"));
    assert_eq!(panel.path, temp.0);
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlinks_are_listed_without_following_targets() -> io::Result<()> {
    use std::os::unix::fs::symlink;
    let temp = Temp::new()?;
    fs::create_dir(temp.0.join("directory"))?;
    symlink(temp.0.join("directory"), temp.0.join("link"))?;
    symlink(temp.0.join("missing"), temp.0.join("broken"))?;
    symlink(&temp.0, temp.0.join("loop"))?;
    let entries = filesystem::scan(&temp.0, true)?;
    assert_eq!(entries.iter().filter(|e| e.symlink).count(), 3);
    assert!(entries.iter().filter(|e| e.symlink).all(|e| !e.directory));
    let mut panel = Panel::new(temp.0.clone(), true)?;
    loaded(&mut panel);
    panel.cursor = panel.entries.iter().position(|e| e.name == "link").unwrap();
    panel.enter(true);
    loaded(&mut panel);
    assert_eq!(panel.path, temp.0.join("link"));
    Ok(())
}

#[test]
fn input_and_rendering_across_terminal_sizes() -> io::Result<()> {
    let temp = Temp::new()?;
    for i in 0..250 {
        fs::write(temp.0.join(format!("file-{i:03}")), b"x")?;
    }
    let mut app = App::new(temp.0.clone())?;
    for panel in &mut app.panels {
        loaded(panel);
    }
    input::handle(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(app.active, 1);
    input::handle(&mut app, KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
    for (w, h) in [(80, 25), (120, 40), (40, 10), (1, 1), (80, 25)] {
        let mut terminal = Terminal::new(TestBackend::new(w, h))?;
        terminal.draw(|frame| ui::draw(frame, &mut app))?;
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        if w >= 80 {
            assert!(text.contains("file-249"));
        } else if w == 40 {
            assert!(text.contains("Terminal too small"));
        }
    }
    input::handle(&mut app, KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE));
    assert_eq!(app.dialogs().len(), 1);
    input::handle(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.dialogs().is_empty());
    input::handle(&mut app, KeyEvent::new(KeyCode::F(10), KeyModifiers::NONE));
    assert!(app.quit);
    Ok(())
}

#[test]
fn terminal_controls_are_sanitized() {
    assert_eq!(filesystem::display_name("a\u{1b}[31m\n文"), "a�[31m�文");
}

fn render(app: &mut App, width: u16, height: u16) -> io::Result<()> {
    let mut terminal = Terminal::new(TestBackend::new(width, height))?;
    terminal.draw(|frame| ui::draw(frame, app))?;
    Ok(())
}

fn mouse_at(app: &mut App, target: dn::mouse::Target, kind: crossterm::event::MouseEventKind) {
    let rect = app
        .mouse
        .regions
        .iter()
        .find(|(_, hit)| *hit == target)
        .unwrap()
        .0;
    dn::mouse::handle(
        app,
        crossterm::event::MouseEvent {
            kind,
            column: rect.x,
            row: rect.y,
            modifiers: KeyModifiers::NONE,
        },
    );
}

fn menu_action(app: &mut App, action: dn::menu::Action) {
    let target = app
        .mouse
        .regions
        .iter()
        .rev()
        .find_map(|(_, target)| {
            if let dn::mouse::Target::MenuItem(depth, row) = *target {
                dn::menu::level_items(app, depth)
                    .get(row)
                    .filter(|item| item.action == action)
                    .map(|_| *target)
            } else {
                None
            }
        })
        .unwrap();
    mouse_at(
        app,
        target,
        crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
    );
}

#[test]
fn mouse_menus_function_bar_and_modal_clicks() -> io::Result<()> {
    use crossterm::event::{MouseButton, MouseEventKind};
    use dn::{menu::Action, mouse::Target};
    let click = MouseEventKind::Down(MouseButton::Left);
    let temp = Temp::new()?;
    let mut app = App::new(temp.0.clone())?;
    for panel in &mut app.panels {
        loaded(panel);
    }
    for width in [80, 120] {
        render(&mut app, width, 25)?;
        app.push_owned_dialog(
            dn::modals::ModalOwner::Workspace,
            dn::dialog::Dialog {
                title: "Help",
                message: "Modal help".into(),
            },
        );
        assert_eq!(app.dialogs().len(), 1);
        render(&mut app, width, 25)?;
        assert!(app
            .mouse
            .regions
            .iter()
            .all(|(_, target)| *target == Target::CloseDialog
                || matches!(target, Target::Surface(..))));
        // Clicking the Quit position through the dialog must not exit.
        dn::mouse::handle(
            &mut app,
            crossterm::event::MouseEvent {
                kind: click,
                column: width - 1,
                row: 24,
                modifiers: KeyModifiers::NONE,
            },
        );
        assert!(!app.quit);
        assert_eq!(app.dialogs().len(), 1);
        mouse_at(&mut app, Target::CloseDialog, click);
        assert!(app.dialogs().is_empty());
        render(&mut app, width, 25)?;
        mouse_at(&mut app, Target::Menu(4), click);
        render(&mut app, width, 25)?;
        let previous_active = app.active;
        menu_action(&mut app, Action::SwapPanels);
        assert_eq!(app.active, previous_active ^ 1);
        assert!(app.modals.menu().is_none());
    }
    render(&mut app, 80, 25)?;
    mouse_at(&mut app, Target::Menu(1), click);
    render(&mut app, 80, 25)?;
    dn::menu::execute(&mut app, Action::Disabled);
    assert_eq!(app.modals.menu(), Some((1, 0)));
    input::handle(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    render(&mut app, 80, 25)?;
    mouse_at(&mut app, Target::Function(9), click);
    assert_eq!(app.modals.menu(), Some((1, 0)));
    input::handle(&mut app, KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    assert_eq!(app.modals.menu(), Some((1, 0)));
    assert_eq!(app.modals.submenus().len(), 1);
    input::handle(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.modals.submenus().is_empty());
    input::handle(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    render(&mut app, 40, 10)?;
    assert!(app.mouse.regions.is_empty());
    render(&mut app, 80, 25)?;
    mouse_at(&mut app, Target::Function(10), click);
    assert!(app.quit);
    Ok(())
}

#[test]
fn mouse_panel_focus_wheel_offset_and_double_click() -> io::Result<()> {
    use crossterm::event::{MouseButton, MouseEventKind};
    use dn::mouse::Target;
    let click = MouseEventKind::Down(MouseButton::Left);
    let temp = Temp::new()?;
    fs::create_dir(temp.0.join("directory"))?;
    for i in 0..50 {
        fs::write(temp.0.join(format!("file-{i:03}")), b"x")?;
    }
    let mut app = App::new(temp.0.clone())?;
    for panel in &mut app.panels {
        loaded(panel);
    }
    app.config.layouts = [dn::panel_layout::PanelLayout::Details; 2];
    render(&mut app, 80, 25)?;
    mouse_at(&mut app, Target::Entry(1, 3), click);
    assert_eq!(app.active, 1);
    assert_eq!(app.panels[1].cursor, 3);
    // Release must not count as a second click.
    mouse_at(
        &mut app,
        Target::Entry(1, 3),
        MouseEventKind::Up(MouseButton::Left),
    );
    mouse_at(&mut app, Target::Entry(1, 3), MouseEventKind::ScrollDown);
    assert_eq!(app.panels[1].cursor, 6);
    input::handle(&mut app, KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
    render(&mut app, 120, 40)?;
    let offset = app.panels[1].offset;
    assert!(offset > 0);
    mouse_at(&mut app, Target::Entry(1, offset), click);
    assert_eq!(app.panels[1].cursor, offset);
    input::handle(&mut app, KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
    render(&mut app, 80, 25)?;
    let directory = app.panels[1]
        .entries
        .iter()
        .position(|e| e.name == "directory")
        .unwrap();
    mouse_at(&mut app, Target::Entry(1, directory), click);
    assert!(!app.panels[1].loading);
    render(&mut app, 80, 25)?;
    mouse_at(&mut app, Target::Entry(1, directory), click);
    assert!(app.panels[1].loading);
    loaded(&mut app.panels[1]);
    assert_eq!(app.panels[1].path, temp.0.join("directory"));
    Ok(())
}

#[test]
fn selection_sorting_and_confirmed_background_operations() -> io::Result<()> {
    use dn::{operations::Kind, panel::SortMode};
    let temp = Temp::new()?;
    fs::write(temp.0.join("a"), b"a")?;
    fs::write(temp.0.join("b"), b"bigger")?;
    fs::create_dir(temp.0.join("out"))?;
    let mut app = App::new(temp.0.clone())?;
    for panel in &mut app.panels {
        loaded(panel);
    }
    let panel = &mut app.panels[0];
    panel.cursor = panel.entries.iter().position(|e| e.name == "a").unwrap();
    panel.toggle_selection();
    panel.set_sort(SortMode::Size);
    assert_eq!(panel.entries[panel.cursor].name, "a");
    assert_eq!(panel.targets(), [temp.0.join("a")]);
    let files: Vec<_> = panel
        .entries
        .iter()
        .filter(|e| !e.directory)
        .map(|e| e.name.to_string_lossy())
        .collect();
    assert_eq!(files, ["b", "a"]);
    panel.refresh(true);
    loaded(panel);
    assert_eq!(panel.selected.len(), 1);
    app.prompt_operation(Kind::Delete, false);
    assert!(app.modals.prompt().is_some());
    assert!(app.job.is_none());
    input::handle(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(temp.0.join("a").exists());
    assert!(app.job.is_none());
    app.panels[1].go_to(temp.0.join("out"), true);
    loaded(&mut app.panels[1]);
    app.prompt_operation(Kind::Copy, false);
    app.submit_prompt();
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.job.is_some() || app.panels.iter().any(|p| p.loading) {
        app.poll();
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    assert!(app.dialogs().is_empty());
    assert_eq!(fs::read(temp.0.join("out/a"))?, b"a");
    assert!(app.panels[0].selected.is_empty());
    app.panels[0].cursor = app.panels[0]
        .entries
        .iter()
        .position(|e| e.name == "a")
        .unwrap();
    app.prompt_operation(Kind::Move, true);
    app.modals.prompt_mut().unwrap().paste("renamed");
    app.submit_prompt();
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.job.is_some() || app.panels.iter().any(|p| p.loading) {
        app.poll();
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    assert!(app.dialogs().is_empty());
    assert_eq!(app.panels[0].entries[app.panels[0].cursor].name, "renamed");
    assert_eq!(fs::read(temp.0.join("renamed"))?, b"a");
    app.panels[0].cursor = 0;
    app.panels[0].toggle_selection();
    assert!(app.panels[0].selected.is_empty()); // Never select parent.
    Ok(())
}

#[test]
fn viewer_text_hex_bounds_and_mouse_close() -> io::Result<()> {
    let temp = Temp::new()?;
    let path = temp.0.join("text.txt");
    fs::write(&path, "Unicode 文\ncolumn\tvalue\n\u{1b}[31m\n")?;
    let text = dn::viewer::load(&path)?;
    assert!(!text.binary);
    assert!(text.lines[0].contains('文'));
    assert_eq!(text.lines[1], "column    value");
    assert!(!text.lines.iter().any(|line| line.contains('\u{1b}')));
    fs::write(temp.0.join("binary"), [0_u8, 255, 65])?;
    let binary = dn::viewer::load(&temp.0.join("binary"))?;
    assert!(binary.binary);
    assert!(binary.lines[0].contains("00 FF 41"));
    fs::write(temp.0.join("large"), vec![b'a'; dn::viewer::MAX_BYTES + 1])?;
    let large = dn::viewer::load(&temp.0.join("large"))?;
    assert!(large.truncated);
    assert!(!large.binary);
    assert!(large.lines[0].len() < 4200);
    assert!(dn::viewer::load(&temp.0).is_err());
    let mut app = App::new(temp.0.clone())?;
    for panel in &mut app.panels {
        loaded(panel);
    }
    app.panels[0].cursor = app.panels[0]
        .entries
        .iter()
        .position(|e| e.name == "text.txt")
        .unwrap();
    input::handle(&mut app, KeyEvent::new(KeyCode::F(3), KeyModifiers::NONE));
    let deadline = Instant::now() + Duration::from_secs(5);
    while app
        .desktop
        .active_viewer()
        .is_some_and(|v| v.contents.is_none())
    {
        app.poll();
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    render(&mut app, 80, 25)?;
    input::handle(&mut app, KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
    assert!(app.desktop.active_viewer().unwrap().offset > 0);
    // An asynchronous error must be visible and modal even over the viewer.
    app.error("test error");
    render(&mut app, 80, 25)?;
    assert_eq!(
        app.mouse
            .regions
            .iter()
            .filter(|(_, t)| !matches!(t, dn::mouse::Target::Surface(..)))
            .count(),
        2
    );
    assert!(app
        .mouse
        .regions
        .iter()
        .all(|(_, target)| *target == dn::mouse::Target::CloseDialog
            || matches!(target, dn::mouse::Target::Surface(..))));
    assert_eq!(app.mouse.regions[0].1, dn::mouse::Target::CloseDialog);
    input::handle(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    render(&mut app, 120, 40)?;
    mouse_at(
        &mut app,
        dn::mouse::Target::CloseViewer,
        crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
    );
    assert!(app.desktop.active_viewer().is_none());
    Ok(())
}

#[test]
fn saved_preferences_and_hidden_date_sorting() -> io::Result<()> {
    use dn::{config::Config, panel::SortMode};
    let temp = Temp::new()?;
    let config_path = temp.0.join("settings/config.conf");
    let mut config = Config::read(&config_path)?;
    config.show_hidden = false;
    config.sorts = [SortMode::Date, SortMode::Size];
    config.save(&config_path)?;
    let loaded_config = Config::read(&config_path)?;
    assert!(!loaded_config.show_hidden);
    assert_eq!(loaded_config.sorts, [SortMode::Date, SortMode::Size]);
    config.show_hidden = true;
    config.save(&config_path)?;
    assert!(Config::read(&config_path)?.show_hidden);
    fs::write(temp.0.join("old"), b"old")?;
    fs::write(temp.0.join("new"), b"new")?;
    fs::write(temp.0.join(".hidden"), b"hidden")?;
    for (name, seconds) in [("old", 100), ("new", 200)] {
        let file = fs::OpenOptions::new().write(true).open(temp.0.join(name))?;
        file.set_times(
            fs::FileTimes::new().set_modified(std::time::UNIX_EPOCH + Duration::from_secs(seconds)),
        )?;
    }
    let mut app = App::new(temp.0.clone())?;
    app.apply_config(loaded_config);
    for panel in &mut app.panels {
        loaded(panel);
    }
    assert!(!app.panels[0].entries.iter().any(|e| e.name == ".hidden"));
    let panel = &app.panels[0];
    assert!(
        panel.entries.iter().position(|e| e.name == "new")
            < panel.entries.iter().position(|e| e.name == "old")
    );
    app.toggle_hidden();
    for panel in &mut app.panels {
        loaded(panel);
    }
    assert!(app.panels[0].entries.iter().any(|e| e.name == ".hidden"));
    Ok(())
}

#[test]
fn unicode_prompt_editing_paste_and_delete_confirmation() -> io::Result<()> {
    use dn::operations::Kind;
    let temp = Temp::new()?;
    fs::write(temp.0.join("delete-me"), b"temporary")?;
    let mut app = App::new(temp.0.clone())?;
    // Exercise explicit permanent mode only in this disposable fixture.
    app.config.use_trash = false;
    for panel in &mut app.panels {
        loaded(panel);
    }
    app.prompt_operation(Kind::Mkdir, false);
    let prompt = app.modals.prompt_mut().unwrap();
    prompt.paste("文abc\n");
    prompt.edit(KeyCode::Left);
    prompt.edit(KeyCode::Backspace);
    assert_eq!(prompt.field, "文ac");
    prompt.edit(KeyCode::Home);
    prompt.edit(KeyCode::Delete);
    assert_eq!(prompt.field, "ac");
    assert!(app.job.is_none());
    render(&mut app, 80, 25)?;
    mouse_at(
        &mut app,
        dn::mouse::Target::CancelPrompt,
        crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
    );
    app.panels[0].cursor = app.panels[0]
        .entries
        .iter()
        .position(|e| e.name == "delete-me")
        .unwrap();
    input::handle(&mut app, KeyEvent::new(KeyCode::F(8), KeyModifiers::NONE));
    assert!(temp.0.join("delete-me").exists());
    assert!(app.job.is_none());
    render(&mut app, 80, 25)?;
    mouse_at(
        &mut app,
        dn::mouse::Target::SubmitPrompt,
        crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.job.is_some() {
        app.poll();
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    assert!(!temp.0.join("delete-me").exists());
    assert!(app.dialogs().is_empty());
    Ok(())
}

#[test]
fn file_types_and_marked_rows_have_distinct_colors() -> io::Result<()> {
    let temp = Temp::new()?;
    for name in ["source.rs", "archive.zip", "media.png", "script.sh"] {
        fs::write(temp.0.join(name), b"a")?;
    }
    let mut app = App::new(temp.0.clone())?;
    for panel in &mut app.panels {
        loaded(panel);
    }
    let types: std::collections::BTreeSet<_> = app.panels[0]
        .entries
        .iter()
        .filter(|e| !e.parent)
        .map(ui::entry_color)
        .collect();
    assert_eq!(types.len(), 4);
    let index = app.panels[0]
        .entries
        .iter()
        .position(|e| e.name == "source.rs")
        .unwrap();
    app.panels[0].cursor = index;
    app.panels[0].toggle_selection();
    let mut terminal = Terminal::new(TestBackend::new(80, 25))?;
    terminal.draw(|frame| ui::draw(frame, &mut app))?;
    let cell = &terminal.backend().buffer()[(1, 3 + index as u16)];
    assert_eq!(cell.symbol(), "√");
    assert_eq!(cell.fg, app.config.palette.color(11));
    assert_eq!(
        terminal.backend().buffer()[(2, 3 + index as u16)].fg,
        app.config.palette.color(14)
    );
    Ok(())
}

#[test]
fn original_menu_hierarchy_cascades_alignment_and_keyboard_navigation() -> io::Result<()> {
    use dn::menu::{self, Action, MenuId};
    assert_eq!(
        menu::LABELS.map(menu::plain),
        ["≡", "File", "Utilities", "Panel", "Manager", "Options",]
    );
    let temp = Temp::new()?;
    let mut app = App::new(temp.0.clone())?;
    for panel in &mut app.panels {
        loaded(panel);
    }
    for (width, height) in [(80, 25), (120, 40)] {
        for root in 0..menu::LABELS.len() {
            menu::open(&mut app, root);
            render(&mut app, width, height)?;
            for (row, item) in menu::items(root).iter().enumerate() {
                if matches!(item.action, Action::Submenu(_)) {
                    menu::activate(&mut app, 0, row);
                    render(&mut app, width, height)?;
                    let surfaces: Vec<_> = app
                        .mouse
                        .regions
                        .iter()
                        .filter(|(_, target)| *target == dn::mouse::Target::MenuSurface)
                        .map(|(rect, _)| *rect)
                        .collect();
                    assert_eq!(surfaces.len(), 2);
                    assert!(surfaces
                        .iter()
                        .all(|rect| rect.right() <= width && rect.bottom() <= height));
                    if root == 5 && width == 80 {
                        assert!(surfaces[1].x < surfaces[0].x);
                    }
                    menu::key(&mut app, KeyCode::Esc);
                }
            }
        }
        menu::open(&mut app, 1);
        let mut terminal = Terminal::new(TestBackend::new(width, height))?;
        terminal.draw(|frame| ui::draw(frame, &mut app))?;
        let rect = app
            .mouse
            .regions
            .iter()
            .find(|(_, target)| *target == dn::mouse::Target::MenuSurface)
            .unwrap()
            .0;
        for (row, item) in menu::items(1).iter().enumerate() {
            if let Some(last) = dn::shortcuts::label(
                app.config.shortcuts,
                dn::focus::Screen::Manager,
                item.shortcut,
            )
            .chars()
            .last()
            {
                assert_eq!(
                    terminal.backend().buffer()[(rect.right() - 3, rect.y + 1 + row as u16)]
                        .symbol(),
                    last.to_string()
                );
            }
        }
    }
    menu::open(&mut app, 5);
    menu::key(&mut app, KeyCode::Char('c'));
    assert_eq!(app.modals.submenus()[0].0, MenuId::Configuration);
    menu::key(&mut app, KeyCode::End);
    // Screen savers remains the last Configuration action after placeholder cleanup.
    let row = menu::selected_row(&app, 1);
    assert_eq!(
        menu::level_items(&app, 1)[row].action,
        Action::ScreensaverSetup
    );
    menu::key(&mut app, KeyCode::Esc);
    assert!(app.modals.menu().is_some());
    assert!(app.modals.submenus().is_empty());
    menu::key(&mut app, KeyCode::Esc);
    assert!(app.modals.menu().is_none());
    input::handle(
        &mut app,
        KeyEvent::new(KeyCode::Char('o'), KeyModifiers::ALT),
    );
    assert_eq!(app.modals.menu().map(|(root, _)| root), Some(5));
    menu::key(&mut app, KeyCode::F(1));
    assert!(app.modals.menu().is_none());
    assert_eq!(app.dialogs().len(), 1);
    Ok(())
}

#[test]
fn options_mouse_dialogs_cancel_apply_and_save_real_preferences() -> io::Result<()> {
    use crossterm::event::{MouseButton, MouseEventKind};
    use dn::{
        menu::{self, Action, MenuId},
        mouse::Target,
        settings::SettingsKind,
    };
    let click = MouseEventKind::Down(MouseButton::Left);
    let temp = Temp::new()?;
    fs::write(temp.0.join("file"), b"text")?;
    let mut app = App::new(temp.0.clone())?;
    for panel in &mut app.panels {
        loaded(panel);
    }
    render(&mut app, 80, 25)?;
    mouse_at(&mut app, Target::Menu(5), click);
    render(&mut app, 80, 25)?;
    menu_action(&mut app, Action::Submenu(MenuId::Configuration));
    render(&mut app, 80, 25)?;
    menu_action(&mut app, Action::Settings(SettingsKind::Interface));
    assert!(app.modals.menu().is_none());
    assert!(app.modals.submenus().is_empty());
    render(&mut app, 80, 25)?;
    mouse_at(&mut app, Target::Setting(0), click);
    assert!(!app.modals.settings().unwrap().draft.highlight_files);
    assert!(app.config.highlight_files);
    mouse_at(&mut app, Target::CancelSettings, click);
    assert!(app.config.highlight_files);
    menu::execute(&mut app, Action::Settings(SettingsKind::FileManager));
    render(&mut app, 80, 25)?;
    for index in 0..5 {
        mouse_at(&mut app, Target::Setting(index), click);
    }
    mouse_at(&mut app, Target::AcceptSettings, click);
    assert!(!app.config.space_select && !app.config.delete_key && !app.config.backspace_parent);
    assert!(!app.config.show_tags && !app.config.highlight_files);
    assert!(app.panels.iter().all(|panel| !panel.loading));
    app.panels[0].cursor = app.panels[0]
        .entries
        .iter()
        .position(|e| e.name == "file")
        .unwrap();
    input::handle(
        &mut app,
        KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
    );
    assert!(app.panels[0].selected.is_empty());
    input::handle(&mut app, KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
    assert!(app.modals.prompt().is_none());
    input::handle(&mut app, KeyEvent::new(KeyCode::Insert, KeyModifiers::NONE));
    assert_eq!(app.panels[0].selected.len(), 1);
    input::handle(&mut app, KeyEvent::new(KeyCode::F(8), KeyModifiers::NONE));
    assert!(app.modals.prompt().is_some()); // F8 still always requires confirmation.
    input::handle(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    let config_path = temp.0.join("config.conf");
    app.config.save(&config_path)?;
    let saved = dn::config::Config::read(&config_path)?;
    assert!(
        !saved.space_select
            && !saved.delete_key
            && !saved.backspace_parent
            && !saved.show_tags
            && !saved.highlight_files
    );
    menu::execute(&mut app, Action::Confirmations);
    assert_eq!(
        app.last_dialog().unwrap().title,
        " Confirmation policy — read-only "
    );
    assert!(app.modals.settings().is_none());
    assert!(app
        .dialogs()
        .last()
        .unwrap()
        .message
        .contains("always enabled"));
    assert!(temp.0.join("file").exists());
    Ok(())
}

#[test]
fn file_view_submenu_honors_explicit_text_and_hex_modes() -> io::Result<()> {
    use dn::{
        menu::{self, Action, MenuId},
        viewer::ViewMode,
    };
    let temp = Temp::new()?;
    fs::write(temp.0.join("hello.txt"), b"hello")?;
    let mut app = App::new(temp.0.clone())?;
    for panel in &mut app.panels {
        loaded(panel);
    }
    app.panels[0].cursor = app.panels[0]
        .entries
        .iter()
        .position(|e| e.name == "hello.txt")
        .unwrap();
    for (mode, binary) in [(ViewMode::Hex, true), (ViewMode::Text, false)] {
        menu::open(&mut app, 1);
        render(&mut app, 80, 25)?;
        menu_action(&mut app, Action::Submenu(MenuId::View));
        render(&mut app, 80, 25)?;
        menu_action(&mut app, Action::View(mode));
        let deadline = Instant::now() + Duration::from_secs(5);
        while app
            .desktop
            .active_viewer()
            .is_some_and(|viewer| viewer.contents.is_none())
        {
            app.poll();
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            app.desktop
                .active_viewer()
                .unwrap()
                .contents
                .as_ref()
                .unwrap()
                .binary,
            binary
        );
        input::handle(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    }
    Ok(())
}

#[test]
fn menus_contain_only_working_commands_and_normalized_separators() {
    use dn::menu::{self, Action};
    fn validate(items: &[menu::Item]) {
        assert!(!items.is_empty());
        assert_ne!(items.first().unwrap().action, Action::Separator);
        assert_ne!(items.last().unwrap().action, Action::Separator);
        let mut hotkeys = std::collections::BTreeSet::new();
        for (i, item) in items.iter().enumerate() {
            assert_ne!(item.action, Action::Disabled, "{}", item.label);
            if item.action == Action::Separator {
                assert!(item.label.is_empty());
                assert_ne!(items[i - 1].action, Action::Separator);
            } else {
                assert!(!item.label.is_empty());
                if let Some(key) = menu::mnemonic(item.label) {
                    assert!(hotkeys.insert(key), "Duplicate mnemonic: {}", item.label);
                }
            }
        }
    }
    let mut submenus = Vec::new();
    for root in 0..menu::LABELS.len() {
        let entries = menu::items(root);
        validate(&entries);
        for item in entries {
            if let Action::Submenu(id) = item.action {
                if !submenus.contains(&id) {
                    submenus.push(id);
                }
            }
        }
    }
    for id in submenus {
        validate(&menu::submenu_items(id));
    }
    assert_eq!(menu::LABELS.len(), 6);
    assert_eq!(
        (0..menu::LABELS.len())
            .flat_map(menu::items)
            .filter(|item| item.action == Action::DirectoryTree)
            .count(),
        1
    );
    assert!(menu::items(0)
        .iter()
        .any(|item| menu::plain(item.label) == "Refresh"));
    assert!(menu::items(0)
        .iter()
        .any(|item| menu::plain(item.label) == "Screensaver"));
    assert_eq!(
        menu::items(2)
            .iter()
            .filter(|i| i.action == Action::UserMenu)
            .count(),
        1
    );
    assert_eq!(
        menu::items(5)
            .iter()
            .find(|i| i.action == Action::Colors)
            .unwrap()
            .label,
        "C~o~lors..."
    );
    assert_eq!(
        menu::plain(
            menu::submenu_items(menu::MenuId::Configuration)
                .iter()
                .find(|i| i.action == Action::Confirmations)
                .unwrap()
                .label
        ),
        "Confirmation policy..."
    );
    use dn::editor::menus as editor;
    for root in 0..editor::LABELS.len() {
        let items = editor::items(root);
        assert_eq!(editor::enabled(root), root != 3);
        if root == 3 {
            assert!(items.is_empty());
            continue;
        }
        assert!(items.first().unwrap().action.is_some());
        assert!(items.last().unwrap().action.is_some());
        for (i, item) in items.iter().enumerate() {
            assert_eq!(item.action.is_none(), item.label.is_empty());
            if item.action.is_none() {
                assert!(items[i - 1].action.is_some());
            }
        }
    }
}

#[test]
fn application_identity_about_help_and_errors() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = App::new(temp.0.clone())?;
    dn::menu::execute(&mut app, dn::menu::Action::About);
    let about = app.pop_dialog().unwrap();
    assert_eq!(about.title, " About Directory Navigator ");
    assert!(about.message.contains(&format!(
        "Directory Navigator\nVersion {}",
        env!("CARGO_PKG_VERSION")
    )));
    assert!(!about.message.contains("Dos Navigator"));
    assert!(about.message.contains("Author: Dmitry Yarygin"));
    assert!(!about.message.contains("RIT"));
    input::handle(&mut app, KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE));
    let help = app.pop_dialog().unwrap();
    assert!(help.title.contains(dn::APPLICATION_NAME));
    assert!(!help.message.contains("Dos Navigator"));
    app.error("test error");
    assert_eq!(
        app.last_dialog().unwrap().title,
        " Directory Navigator — error "
    );
    let info = dn::utilities::information::collect(
        dn::utilities::information::Kind::System,
        temp.0.clone(),
        (80, 25),
    );
    assert!(info.iter().any(|(label, value)| label == "Application"
        && value == &format!("Directory Navigator {}", env!("CARGO_PKG_VERSION"))));
    Ok(())
}
