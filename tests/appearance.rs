use chrono::{TimeZone, Utc};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use dn::{
    app::App,
    clock_view::Clock,
    config::Config,
    input, menu,
    mouse::{self, Target},
    palette::{self, PaletteProfile, CLASSIC, TERMINAL},
    settings::{self, Setting, SettingsKind},
    ui,
    viewer::Viewer,
};
use ratatui::{backend::TestBackend, style::Color, Terminal};
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
            "dn-colors-{}-{}",
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
fn classic_lookup_is_nearest_fixed_xterm_vga_and_terminal_is_unchanged() {
    let vga = [
        [0, 0, 0],
        [0, 0, 170],
        [0, 170, 0],
        [0, 170, 170],
        [170, 0, 0],
        [170, 0, 170],
        [170, 85, 0],
        [170, 170, 170],
        [85, 85, 85],
        [85, 85, 255],
        [85, 255, 85],
        [85, 255, 255],
        [255, 85, 85],
        [255, 85, 255],
        [255, 255, 85],
        [255, 255, 255],
    ];
    let rgb = |index: u8| -> [i32; 3] {
        if index >= 232 {
            let gray = 8 + 10 * (i32::from(index) - 232);
            [gray; 3]
        } else {
            let i = index - 16;
            let levels = [0, 95, 135, 175, 215, 255];
            [
                levels[(i / 36) as usize],
                levels[(i / 6 % 6) as usize],
                levels[(i % 6) as usize],
            ]
        }
    };
    for (i, target) in vga.iter().enumerate() {
        let nearest = (16..=255u8)
            .min_by_key(|index| {
                rgb(*index)
                    .iter()
                    .zip(target)
                    .map(|(a, b)| (a - b) * (a - b))
                    .sum::<i32>()
            })
            .unwrap();
        assert_eq!(CLASSIC[i], nearest);
        assert_eq!(
            PaletteProfile::ClassicDn.color(i as u8),
            Color::Indexed(nearest)
        );
        assert_eq!(
            PaletteProfile::Terminal.color(i as u8),
            Color::Indexed(TERMINAL[i])
        );
    }
    assert_eq!(
        PaletteProfile::ClassicDn.dos(0x87).bg,
        Some(Color::Indexed(240))
    );
    assert_eq!(
        PaletteProfile::ClassicDn.dos(0x71).fg,
        Some(Color::Indexed(19))
    );
    assert_eq!(
        PaletteProfile::ClassicDn.dos(0x71).bg,
        Some(Color::Indexed(248))
    );
}
#[test]
fn clock_format_midnight_repeated_seconds_and_backwards_time() {
    let mut clock = Clock::default();
    let before = Utc.with_ymd_and_hms(2026, 10, 20, 23, 59, 59).unwrap();
    assert!(clock.update(before));
    assert_eq!(clock.text, " 23:59:59 ");
    assert!(!clock.update(before));
    assert_eq!(clock.text.len(), 10);
    assert!(clock.update(before + chrono::Duration::seconds(1)));
    assert_eq!(clock.text, " 00:00:00 ");
    assert!(clock.update(before - chrono::Duration::hours(10)));
    assert_eq!(clock.text, " 13:59:59 ");
    assert!(!clock.update(before - chrono::Duration::hours(10) + chrono::Duration::days(1)));
}
#[test]
fn appearance_configuration_legacy_unknown_roundtrip_and_interface_clock() -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("settings");
    fs::write(&path, "hidden=false\n")?;
    let old = Config::read(&path)?;
    assert_eq!(old.palette, PaletteProfile::ClassicDn);
    assert!(old.show_clock);
    fs::write(&path, "palette=unknown\n")?;
    assert_eq!(Config::read(&path)?.palette, PaletteProfile::ClassicDn);
    let config = Config {
        palette: PaletteProfile::Terminal,
        show_clock: false,
        ..Default::default()
    };
    config.save(&path)?;
    let loaded = Config::read(&path)?;
    assert_eq!(loaded.palette, PaletteProfile::Terminal);
    assert!(!loaded.show_clock);
    let mut app = t.app()?;
    app.open_settings(SettingsKind::Interface);
    let index = app
        .settings
        .as_ref()
        .unwrap()
        .rows()
        .iter()
        .position(|s| matches!(s, Setting::Clock))
        .unwrap();
    draw(&mut app, 80, 25)?;
    click(&mut app, Target::Setting(index));
    settings::accept(&mut app);
    assert!(!app.config.show_clock);
    Ok(())
}
#[test]
fn clock_header_reservation_styles_resize_and_no_hit_targets() -> io::Result<()> {
    let t = Temp::new()?;
    let mut app = t.app()?;
    for (w, h) in [(80, 25), (120, 40), (161, 30)] {
        let text = app.clock.text.clone();
        let mut terminal = Terminal::new(TestBackend::new(w, h))?;
        terminal.draw(|f| ui::draw(f, &mut app))?;
        let shown = (w - 10..w)
            .map(|x| terminal.backend().buffer()[(x, 0)].symbol())
            .collect::<String>();
        assert_eq!(shown, text);
        for x in w - 10..w {
            let cell = &terminal.backend().buffer()[(x, 0)];
            assert_eq!(cell.fg, Color::Indexed(19));
            assert_eq!(cell.bg, Color::Indexed(248));
            assert!(!app
                .mouse
                .regions
                .iter()
                .any(|(r, _)| r.contains((x, 0).into())));
        }
        for x in 0..w {
            assert_eq!(
                terminal.backend().buffer()[(x, h - 1)].bg,
                Color::Indexed(248)
            );
        }
    }
    app.config.show_clock = false;
    let screen = draw(&mut app, 80, 25)?;
    assert!(!screen.contains(&app.clock.text));
    app.config.show_clock = true;
    assert!(!draw(&mut app, 60, 15)?.contains(&app.clock.text));
    Ok(())
}
#[test]
fn function_bars_profiles_mouse_and_disabled_editor_actions() -> io::Result<()> {
    let t = Temp::new()?;
    let mut app = t.app()?;
    for profile in [PaletteProfile::ClassicDn, PaletteProfile::Terminal] {
        app.config.palette = profile;
        let mut terminal = Terminal::new(TestBackend::new(80, 25))?;
        terminal.draw(|f| ui::draw(f, &mut app))?;
        let cell = &terminal.backend().buffer()[(0, 24)];
        assert_eq!(
            cell.symbol(),
            if profile == PaletteProfile::ClassicDn {
                "F"
            } else {
                "1"
            }
        );
        assert_eq!(
            cell.fg,
            profile
                .dos(if profile == PaletteProfile::ClassicDn {
                    0x74
                } else {
                    0x07
                })
                .fg
                .unwrap()
        );
        assert!(app
            .mouse
            .regions
            .iter()
            .any(|(_, t)| *t == Target::Function(8)));
        for x in 0..80 {
            if profile == PaletteProfile::ClassicDn {
                assert_eq!(terminal.backend().buffer()[(x, 24)].bg, profile.color(7));
            }
        }
    }
    app.config.palette = PaletteProfile::ClassicDn;
    dn::editor::open(&mut app, true);
    let mut terminal = Terminal::new(TestBackend::new(80, 25))?;
    terminal.draw(|f| ui::draw(f, &mut app))?;
    assert_eq!(terminal.backend().buffer()[(0, 24)].symbol(), "F");
    assert_eq!(
        terminal.backend().buffer()[(24, 24)].fg,
        app.config.palette.color(8)
    );
    assert!(!app
        .mouse
        .regions
        .iter()
        .any(|(rect, _)| rect.contains((25, 24).into())));
    for column in [24, 32, 56, 64] {
        let label: String = (column + 2..column + 8)
            .map(|x| terminal.backend().buffer()[(x, 24)].symbol())
            .collect();
        assert!(
            label.trim().is_empty(),
            "unassigned function label: {label}"
        );
        assert!(!app
            .mouse
            .regions
            .iter()
            .any(|(rect, _)| rect.contains((column + 3, 24).into())));
    }
    Ok(())
}
#[test]
fn palette_preview_mouse_keyboard_cancel_apply_and_separate_highlight() -> io::Result<()> {
    let t = Temp::new()?;
    let mut app = t.app()?;
    menu::execute(&mut app, menu::Action::Colors);
    assert!(app.color_dialog.is_some());
    assert!(app.settings.is_none());
    let entries = app.panels[0].entries.len();
    draw(&mut app, 80, 25)?;
    click(&mut app, Target::ColorChoice(PaletteProfile::Terminal));
    assert_eq!(app.config.palette, PaletteProfile::Terminal);
    assert!(!app.panels[0].loading);
    draw(&mut app, 120, 40)?;
    click(&mut app, Target::CancelColors);
    assert_eq!(app.config.palette, PaletteProfile::ClassicDn);
    assert_eq!(app.panels[0].entries.len(), entries);
    palette::open(&mut app);
    key(&mut app, KeyCode::Char('t'), KeyModifiers::NONE);
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.color_dialog.is_none());
    assert_eq!(app.config.palette, PaletteProfile::Terminal);
    palette::open(&mut app);
    key(&mut app, KeyCode::Char('c'), KeyModifiers::NONE);
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(app.config.palette, PaletteProfile::Terminal);
    app.open_settings(SettingsKind::Highlight);
    assert_eq!(app.settings.as_ref().unwrap().rows().len(), 1);
    Ok(())
}
#[test]
fn clock_editor_viewer_dialog_and_game_precedence() -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("long name ".repeat(15));
    fs::write(&path, "view me")?;
    let mut app = t.app()?;
    app.viewer = Some(Viewer::open(path)?);
    wait(&mut app, |a| {
        a.viewer.as_ref().is_some_and(|v| v.contents.is_some())
    });
    let text = app.clock.text.clone();
    assert!(draw(&mut app, 80, 25)?.contains(&text));
    app.viewer = None;
    dn::editor::open(&mut app, true);
    assert!(draw(&mut app, 80, 25)?.contains(&text));
    dn::editor::action(&mut app, dn::editor::EditorAction::Help);
    let screen = draw(&mut app, 80, 25)?;
    assert!(screen.contains("Editor"));
    app.editor = None;
    dn::tetris::open(&mut app);
    assert!(!draw(&mut app, 80, 25)?.contains(&text));
    Ok(())
}
#[test]
fn clock_poll_changes_no_directory_scans_and_disabled_clock_does_not_tick() -> io::Result<()> {
    let t = Temp::new()?;
    let mut app = t.app()?;
    wait(&mut app, |a| {
        a.panels.iter().all(|p| p.space.result.is_some())
    });
    app.config.show_clock = false;
    app.clock = Clock::default();
    assert!(!app.poll());
    assert_eq!(app.clock.text, " --:--:-- ");
    assert!(app.panels.iter().all(|p| !p.loading));
    app.config.show_clock = true;
    assert!(app.poll());
    assert_ne!(app.clock.text, " --:--:-- ");
    assert!(app.panels.iter().all(|p| !p.loading));
    Ok(())
}

#[test]
fn classic_selection_tags_keep_file_colors_with_yellow_names() -> io::Result<()> {
    use dn::panel_layout::PanelLayout;
    let temp = Temp::new()?;
    fs::create_dir(temp.0.join("folder"))?;
    for name in [
        "ordinary.bin",
        "source.rs",
        "archive.zip",
        "script.sh",
        "media.png",
        "界e\u{301}.rs",
    ] {
        fs::write(temp.0.join(name), "fixture")?;
    }
    for i in 0..25 {
        fs::write(temp.0.join(format!("file{i:02}.txt")), "fixture")?;
    }
    let mut app = temp.app()?;
    for panel in &mut app.panels {
        panel.selected = panel
            .entries
            .iter()
            .filter(|e| !e.parent)
            .map(|e| e.name.clone())
            .collect();
        panel.cursor = panel
            .entries
            .iter()
            .position(|e| e.name == "archive.zip")
            .expect("archive");
    }
    let marks = app.panels[0].selected.clone();
    let cursor = app.panels[0].cursor;
    for profile in [PaletteProfile::ClassicDn, PaletteProfile::Terminal] {
        app.config.palette = profile;
        for layout in [PanelLayout::Brief, PanelLayout::Details] {
            app.config.layouts = [layout; 2];
            for (width, height) in [(80, 25), (132, 40), (80, 25)] {
                let mut terminal = Terminal::new(TestBackend::new(width, height))?;
                terminal.draw(|f| ui::draw(f, &mut app))?;
                for (rect, target) in &app.mouse.regions {
                    let Target::Entry(panel, index) = target else {
                        continue;
                    };
                    let entry = &app.panels[*panel].entries[*index];
                    if entry.parent {
                        continue;
                    }
                    let focused = *panel == app.active && *index == app.panels[*panel].cursor;
                    let bg = profile.color(if focused { 3 } else { 8 });
                    let cell = &terminal.backend().buffer()[(rect.x, rect.y)];
                    assert_eq!(
                        cell.symbol(),
                        if profile == PaletteProfile::ClassicDn {
                            "√"
                        } else {
                            "✓"
                        }
                    );
                    assert_eq!(
                        cell.fg,
                        profile.color(if profile == PaletteProfile::ClassicDn {
                            ui::entry_color(entry) & 15
                        } else {
                            14
                        })
                    );
                    assert_eq!(cell.bg, bg);
                    let name = &terminal.backend().buffer()[(rect.x + 1, rect.y)];
                    assert_eq!(name.fg, profile.color(14));
                    assert_eq!(name.bg, bg);
                }
                assert_eq!(app.panels[0].selected, marks);
                assert_eq!(app.panels[0].cursor, cursor);
                assert!(app.panels.iter().all(|p| !p.loading));
            }
        }
    }
    app.config.palette = PaletteProfile::ClassicDn;
    app.config.highlight_files = false;
    let mut terminal = Terminal::new(TestBackend::new(80, 25))?;
    terminal.draw(|f| ui::draw(f, &mut app))?;
    for (rect, target) in &app.mouse.regions {
        let Target::Entry(panel, index) = target else {
            continue;
        };
        let entry = &app.panels[*panel].entries[*index];
        if entry.parent {
            continue;
        }
        assert_eq!(
            terminal.backend().buffer()[(rect.x, rect.y)].fg,
            app.config
                .palette
                .color(if entry.directory { 15 } else { 7 })
        );
    }
    app.config.show_tags = false;
    terminal.draw(|f| ui::draw(f, &mut app))?;
    assert!(terminal
        .backend()
        .buffer()
        .content
        .iter()
        .all(|c| c.symbol() != "√" && c.symbol() != "✓"));
    assert_eq!(app.panels[0].selected, marks);
    Ok(())
}

#[test]
fn document_headers_use_cyan_close_icons_with_reserved_titles_and_clock() -> io::Result<()> {
    let temp = Temp::new()?;
    let path = temp.0.join(format!("{}.txt", "界e\u{301}".repeat(25)));
    fs::write(&path, "file contents")?;
    let mut app = temp.app()?;
    for profile in [PaletteProfile::ClassicDn, PaletteProfile::Terminal] {
        app.config.palette = profile;
        for editor in [false, true] {
            if editor {
                app.viewer = None;
                dn::editor::open_path(&mut app, path.clone());
                wait(&mut app, |a| a.editor.as_ref().is_some_and(|e| !e.busy()));
            } else {
                app.editor = None;
                app.viewer = Some(Viewer::open(path.clone())?);
                wait(&mut app, |a| {
                    a.viewer.as_ref().is_some_and(|v| v.contents.is_some())
                });
            }
            for (w, h) in [(80, 25), (132, 40), (80, 25)] {
                let mut terminal = Terminal::new(TestBackend::new(w, h))?;
                terminal.draw(|f| ui::draw(f, &mut app))?;
                let target = if editor {
                    Target::Editor(dn::editor::EditorAction::Close)
                } else {
                    Target::CloseViewer
                };
                let r = app
                    .mouse
                    .regions
                    .iter()
                    .find(|(_, t)| *t == target)
                    .expect("close")
                    .0;
                assert_eq!((r.x, r.y, r.width, r.height), (2, u16::from(editor), 3, 1));
                let b = terminal.backend().buffer();
                assert_eq!(b[(2, r.y)].symbol(), "[");
                assert_eq!(b[(3, r.y)].symbol(), "■");
                assert_eq!(b[(4, r.y)].symbol(), "]");
                assert_eq!(b[(3, r.y)].fg, profile.color(11));
                assert_eq!(b[(3, r.y)].bg, profile.color(7));
                let title = (0..w).map(|x| b[(x, r.y)].symbol()).collect::<String>();
                assert!(title.contains(if editor { "F4 Editor" } else { "F3 Viewer" }));
                assert!(title.contains(".txt"));
                assert!(!b
                    .content
                    .iter()
                    .map(|c| c.symbol())
                    .collect::<String>()
                    .contains("[ Close ]"));
                assert_eq!(
                    (w - 10..w).map(|x| b[(x, 0)].symbol()).collect::<String>(),
                    app.clock.text
                );
            }
            if editor {
                app.editor = None;
            } else {
                app.viewer = None;
            }
        }
    }
    Ok(())
}

#[test]
fn document_close_icons_respect_modal_precedence_and_unsaved_guard() -> io::Result<()> {
    let temp = Temp::new()?;
    let path = temp.0.join("sample.txt");
    fs::write(&path, "original file")?;
    let mut app = temp.app()?;
    app.viewer = Some(Viewer::open(path.clone())?);
    wait(&mut app, |a| {
        a.viewer.as_ref().is_some_and(|v| v.contents.is_some())
    });
    draw(&mut app, 80, 25)?;
    click(&mut app, Target::CloseViewer);
    assert!(app.viewer.is_none());
    dn::editor::open_path(&mut app, path.clone());
    wait(&mut app, |a| a.editor.as_ref().is_some_and(|e| !e.busy()));
    draw(&mut app, 80, 25)?;
    dn::editor::paste(&mut app, "unsaved ");
    click(&mut app, Target::Editor(dn::editor::EditorAction::Close));
    assert!(matches!(
        app.editor.as_ref().and_then(|e| e.modal.as_ref()),
        Some(dn::editor::Modal::ConfirmClose)
    ));
    draw(&mut app, 80, 25)?;
    assert!(app
        .mouse
        .regions
        .iter()
        .all(|(_, t)| *t != Target::Editor(dn::editor::EditorAction::Close)));
    click(&mut app, Target::Editor(dn::editor::EditorAction::Cancel));
    assert!(app
        .editor
        .as_ref()
        .is_some_and(|e| e.dirty() && e.modal.is_none()));
    draw(&mut app, 80, 25)?;
    click(&mut app, Target::Editor(dn::editor::EditorAction::Close));
    draw(&mut app, 80, 25)?;
    click(&mut app, Target::Editor(dn::editor::EditorAction::Discard));
    assert!(app.editor.is_none());
    assert_eq!(fs::read_to_string(path)?, "original file");
    Ok(())
}
