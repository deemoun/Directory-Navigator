use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use dn::testing::{backend::TestBackend, Terminal};
use dn::{
    app::App,
    config::Config,
    input, menu,
    mouse::{self, Target},
    screensaver::{self, State},
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
            "dn-saver-{}-{}",
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
fn idle_timeout_never_busy_and_frame_rate() {
    let mut s = State::default();
    let now = Instant::now();
    s.activity(now);
    assert!(!s.tick(now + Duration::from_secs(59), 1, true));
    assert!(s.tick(now + Duration::from_secs(60), 1, true));
    assert!(s.active);
    assert!(!s.tick(now + Duration::from_secs(60), 1, true));
    assert!(s.tick(now + Duration::from_millis(60050), 1, true));
    assert!(s.activity(now + Duration::from_secs(61)));
    assert!(!s.tick(now + Duration::from_secs(200), 0, true));
    assert!(!s.tick(now + Duration::from_secs(201), 1, false));
    assert!(!s.tick(now + Duration::from_secs(260), 1, true));
    assert!(s.tick(now + Duration::from_secs(261), 1, true));
}
#[test]
fn star_glyphs_bounds_resize_and_motion() {
    assert_eq!(screensaver::GLYPHS, [' ', '·', '∙', '•', '♦', '☼']);
    let mut s = State::default();
    let now = Instant::now();
    s.start(now);
    for (w, h) in [(80, 25), (160, 50), (1, 1), (0, 0), (400, 100), (80, 25)] {
        s.resize(w, h);
        for i in 0..100 {
            s.tick(now + Duration::from_millis(i * 50), 0, false);
            let cells: Vec<_> = s.cells().collect();
            assert!(cells.len() <= 200);
            for (x, y, c, a) in cells {
                assert!(x < w && y < h);
                assert!(screensaver::GLYPHS.contains(&c));
                assert!(a == 7 || a == 15);
                assert!(!c.is_control());
            }
        }
    }
}
#[test]
fn setup_preview_cancel_apply_mouse_and_menu() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    let path = app.panels[0].path.clone();
    let cursor = app.panels[0].cursor;
    assert!(menu::submenu_items(menu::MenuId::Configuration)
        .iter()
        .any(|i| i.action == menu::Action::ScreensaverSetup));
    menu::execute(&mut app, menu::Action::ScreensaverSetup);
    for (w, h) in [(80, 25), (132, 40), (80, 25)] {
        let setup = draw(&mut app, w, h)?;
        assert!(setup.contains("Screen savers"));
        assert!(setup.contains("30 minutes"));
        assert!(setup.contains("60 minutes"));
        click(&mut app, Target::Screensaver(6));
        assert_eq!(
            app.screensaver.modals.screensaver().map(|s| s.delay),
            Some(60)
        );
        click(&mut app, Target::Screensaver(3));
        assert_eq!(
            app.screensaver.modals.screensaver().map(|s| s.delay),
            Some(5)
        );
        click(&mut app, Target::Screensaver(screensaver::PREVIEW));
        let sky = draw(&mut app, w, h)?;
        assert!(!sky.contains("Screen savers"));
        assert!(!sky.contains("Help"));
        assert!(app.mouse.regions.is_empty());
        crate::key(&mut app, KeyCode::F(10), KeyModifiers::NONE);
        assert!(!app.quit);
        assert!(!app.screensaver.active);
    }
    draw(&mut app, 80, 25)?;
    click(&mut app, Target::Screensaver(screensaver::CANCEL));
    assert_eq!(app.config.saver_minutes, 10);
    screensaver::open(&mut app);
    draw(&mut app, 80, 25)?;
    click(&mut app, Target::Screensaver(5));
    crate::key(&mut app, KeyCode::Char('p'), KeyModifiers::NONE);
    assert!(app.screensaver.active);
    mouse::handle(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        },
    );
    assert!(!app.screensaver.active);
    screensaver::command(&mut app, screensaver::ACCEPT);
    assert_eq!(app.config.saver_minutes, 30);
    assert_eq!(app.panels[0].path, path);
    assert_eq!(app.panels[0].cursor, cursor);
    Ok(())
}
#[test]
fn wake_events_do_not_edit_exit_or_activate_mouse_targets() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    dn::editor::open(&mut app, true);
    wait(&mut app, |a| {
        a.desktop.active_editor().is_some_and(|e| !e.busy())
    });
    draw(&mut app, 80, 25)?;
    dn::editor::paste(&mut app, "unsaved text");
    let idle = Instant::now();
    app.screensaver.activity(idle);
    assert!(screensaver::poll(&mut app, idle + Duration::from_secs(600)));
    assert!(app.screensaver.active);
    crate::key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app
        .desktop
        .active_editor()
        .and_then(|e| e.buffer.as_ref())
        .is_some_and(|b| b.dirty() && b.text == "unsaved text"));
    for event in [
        Event::Paste("discard this".into()),
        Event::Key(KeyEvent::new(KeyCode::F(10), KeyModifiers::NONE)),
        Event::Mouse(MouseEvent {
            kind: MouseEventKind::Moved,
            column: 4,
            row: 2,
            modifiers: KeyModifiers::NONE,
        }),
    ] {
        app.screensaver.start(Instant::now());
        assert!(draw(&mut app, 80, 25)?
            .chars()
            .all(|c| c == ' ' || screensaver::GLYPHS.contains(&c)));
        assert!(!screensaver::event(
            &mut app,
            &Event::Resize(100, 30),
            Instant::now()
        ));
        assert!(app.screensaver.active);
        assert!(!screensaver::event(
            &mut app,
            &Event::Mouse(MouseEvent {
                kind: MouseEventKind::Up(MouseButton::Left),
                column: 1,
                row: 1,
                modifiers: KeyModifiers::NONE,
            }),
            Instant::now()
        ));
        assert!(app.screensaver.active);
        assert!(screensaver::event(&mut app, &event, Instant::now()));
        assert!(!app.quit);
        assert!(draw(&mut app, 80, 25)?.contains("unsaved text"));
    }
    app.screensaver.start(Instant::now());
    crate::key(&mut app, KeyCode::Char('x'), KeyModifiers::ALT);
    assert!(!app.quit);
    assert!(app.dialogs().is_empty());
    assert!(app
        .desktop
        .active_editor()
        .is_some_and(|e| e.modals.editor_modal().is_none()));
    Ok(())
}
#[test]
fn idle_respects_modals_jobs_games_and_clock_ticks() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    app.config.saver_minutes = 1;
    draw(&mut app, 80, 25)?;
    let now = Instant::now();
    app.screensaver.activity(now);
    app.clock.tick();
    assert!(screensaver::poll(&mut app, now + Duration::from_secs(60)));
    assert!(app.screensaver.active);
    app.screensaver.activity(now);
    screensaver::open(&mut app);
    assert!(!screensaver::poll(&mut app, now + Duration::from_secs(120)));
    assert!(!app.screensaver.active);
    app.screensaver.modals.set_screensaver(None);
    assert!(!screensaver::poll(&mut app, now + Duration::from_secs(179)));
    assert!(screensaver::poll(&mut app, now + Duration::from_secs(180)));
    app.screensaver.activity(now);
    dn::tetris::open(&mut app);
    assert!(!screensaver::poll(&mut app, now + Duration::from_secs(300)));
    assert!(!app.screensaver.active);
    Ok(())
}
#[test]
fn config_round_trip_defaults_and_invalid_values() -> io::Result<()> {
    let temp = Temp::new()?;
    let path = temp.0.join("config.conf");
    assert_eq!(Config::read(&path)?.saver_minutes, 10);
    fs::write(&path, "config_version=2\n[file_manager]\nhidden=true\n")?;
    assert_eq!(Config::read(&path)?.saver_minutes, 10);
    for value in screensaver::DELAYS {
        let c = Config {
            saver_minutes: value,
            ..Config::default()
        };
        c.save(&path)?;
        assert_eq!(Config::read(&path)?.saver_minutes, value);
    }
    for value in ["bad", "-1", "3", "255"] {
        fs::write(
            &path,
            format!("config_version=2\n[screensaver]\nminutes={value}\n"),
        )?;
        assert!(Config::read(&path).is_err());
    }
    Ok(())
}

#[test]
fn original_lists_draft_preview_help_and_cancel_preserve_configuration() -> io::Result<()> {
    use screensaver::*;
    let t = Temp::new()?;
    let mut app = t.app()?;
    let original = app.config.clone();
    open(&mut app);
    let title = draw(&mut app, 80, 25)?;
    for label in [
        "Selected savers",
        "Available savers",
        "Star flight",
        "Flash-light",
        "Clock",
        "Blackness",
    ] {
        assert!(title.contains(label), "{label}");
    }
    click(&mut app, Target::Screensaver(AVAILABLE_ROW + 1));
    crate::key(&mut app, KeyCode::Insert, KeyModifiers::NONE);
    command(&mut app, ADD);
    assert_eq!(
        app.screensaver.modals.screensaver().unwrap().selected,
        [Kind::StarFlight, Kind::FlashLight]
    );
    command(&mut app, PREVIEW);
    assert_eq!(app.screensaver.kind(), Kind::FlashLight);
    draw(&mut app, 80, 25)?;
    crate::key(&mut app, KeyCode::F(10), KeyModifiers::NONE);
    assert!(!app.quit && !app.screensaver.active);
    assert!(app.screensaver.modals.screensaver().is_some());
    assert_eq!(app.config, original);
    draw(&mut app, 80, 25)?;
    crate::key(&mut app, KeyCode::F(1), KeyModifiers::NONE);
    assert!(app.last_dialog().is_some());
    crate::key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.screensaver.modals.screensaver().is_some());
    assert!(app.last_dialog().is_none());
    crate::key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.screensaver.modals.screensaver().is_none());
    assert_eq!(app.config, original);
    Ok(())
}
#[test]
fn remove_keeps_nearest_row_empty_selection_disables_auto_and_manual_launch() -> io::Result<()> {
    use screensaver::*;
    let t = Temp::new()?;
    let mut app = t.app()?;
    open(&mut app);
    for i in 0..Kind::ALL.len() {
        command(&mut app, AVAILABLE_ROW + i);
        command(&mut app, ADD);
    }
    assert_eq!(
        app.screensaver.modals.screensaver().unwrap().selected,
        Kind::ALL
    );
    command(&mut app, SELECTED_ROW + 3);
    command(&mut app, REMOVE);
    assert_eq!(
        app.screensaver.modals.screensaver().unwrap().selected_row,
        2
    );
    for _ in 0..3 {
        command(&mut app, REMOVE);
    }
    assert!(app
        .screensaver
        .modals
        .screensaver()
        .unwrap()
        .selected
        .is_empty());
    command(&mut app, ACCEPT);
    assert!(app.config.savers.is_empty());
    assert!(!poll(&mut app, Instant::now() + Duration::from_secs(3600)));
    menu::execute(&mut app, menu::Action::ScreenRest);
    assert!(!app.screensaver.active);
    assert!(app
        .last_dialog()
        .unwrap()
        .message
        .contains("Select at least one"));
    Ok(())
}
#[test]
fn native_selection_array_retains_old_defaults_order_empty_and_unknown_fields() -> io::Result<()> {
    use screensaver::Kind;
    let t = Temp::new()?;
    let path = t.0.join("savers.conf");
    fs::write(&path,"config_version=2\n[screensaver]\nminutes=30\nselected=['clock','blackness','clock']\nfuture='retained'\n[extra]\nflag=true\n")?;
    let mut c = Config::read(&path)?;
    assert_eq!(c.savers, [Kind::Clock, Kind::Blackness]);
    c.save(&path)?;
    assert_eq!(Config::read(&path)?, c);
    let table = fs::read_to_string(&path)?.parse::<toml::Table>().unwrap();
    assert_eq!(table["screensaver"]["future"].as_str(), Some("retained"));
    assert_eq!(table["extra"]["flag"].as_bool(), Some(true));
    c.savers.clear();
    c.save(&path)?;
    assert!(Config::read(&path)?.savers.is_empty());
    assert_eq!(
        Config::parse(b"config_version=2\n[screensaver]\nminutes=5")?.savers,
        [Kind::StarFlight]
    );
    for value in ["'clock'", "[true]", "['unknown']", "42", "{}"] {
        assert!(Config::parse(
            format!("config_version=2\n[screensaver]\nselected={value}").as_bytes()
        )
        .is_err());
    }
    Ok(())
}
#[test]
fn all_effects_paint_immutably_resize_localize_and_consume_wake_input() -> io::Result<()> {
    use dn::localization::Language;
    use screensaver::*;
    let t = Temp::new()?;
    let mut app = t.app()?;
    for language in [Language::English, Language::Russian] {
        app.config.language = language;
        for custom in [false, true] {
            app.config.palette = Default::default();
            if custom {
                app.config.palette = app
                    .config
                    .palette
                    .with_slot(dn::palette::theme::Slot::DialogText, 0x1e);
            }
            for kind in Kind::ALL {
                app.screensaver.start_kind(Instant::now(), kind);
                for (width, height) in [(80, 25), (132, 40), (40, 15), (80, 25)] {
                    let area = dn::testing::layout::Rect::new(0, 0, width, height);
                    let snapshot = dn::layout::prepare(&mut app, area);
                    let first = dn::testing::Buffer::rendered(ui::render(&app, &snapshot));
                    for _ in 0..3 {
                        assert_eq!(
                            first,
                            dn::testing::Buffer::rendered(ui::render(&app, &snapshot))
                        );
                    }
                    let rendered = ui::render(&app, &snapshot);
                    assert!(rendered.hit_regions().is_empty());
                    assert!(rendered.cursor().is_none());
                    if kind == Kind::Blackness {
                        assert!(first
                            .content
                            .iter()
                            .all(|c| c.symbol() == " " && c.bg == app.config.palette.color(0)));
                    }
                }
                crate::key(&mut app, KeyCode::F(10), KeyModifiers::NONE);
                assert!(!app.quit && !app.screensaver.active);
            }
        }
    }
    Ok(())
}
#[test]
fn selection_form_keyboard_highlights_both_lists_and_survives_small_terminal() -> io::Result<()> {
    use screensaver::*;
    let t = Temp::new()?;
    let mut app = t.app()?;
    for language in [
        dn::localization::Language::English,
        dn::localization::Language::Russian,
    ] {
        app.config.language = language;
        open(&mut app);
        crate::key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        assert_eq!(
            app.screensaver.modals.screensaver().unwrap().cursor,
            AVAILABLE
        );
        crate::key(&mut app, KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(
            app.screensaver.modals.screensaver().unwrap().preview,
            Kind::FlashLight
        );
        crate::key(&mut app, KeyCode::Insert, KeyModifiers::NONE);
        for (w, h) in [(80, 25), (120, 40), (40, 15), (80, 25)] {
            draw(&mut app, w, h)?;
        }
        crate::key(&mut app, KeyCode::BackTab, KeyModifiers::SHIFT);
        crate::key(&mut app, KeyCode::End, KeyModifiers::NONE);
        assert_eq!(
            app.screensaver.modals.screensaver().unwrap().preview,
            Kind::FlashLight
        );
        crate::key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(app.config.savers, [Kind::StarFlight]);
    }
    Ok(())
}

#[test]
fn flashlight_captures_prepared_workspace_once_before_projection() -> io::Result<()> {
    use dn::{
        panel_layout::PanelLayout,
        testing::{layout::Rect, Buffer},
    };
    let t = Temp::new()?;
    for i in 0..30 {
        fs::write(t.0.join(format!("entry-{i:02}")), "fixture")?;
    }
    let mut app = t.app()?;
    app.config.show_clock = false;
    app.config.layouts = [PanelLayout::Details; 2];
    let bounds = Rect::new(7, 3, 80, 25);
    let screen = Rect::new(0, 0, 100, 35);
    let now = Instant::now();
    let background = dn::layout::prepare_window(&mut app, bounds, screen, now);
    let expected = Buffer::rendered(ui::render(&app, &background));
    app.screensaver
        .start_kind(now, screensaver::Kind::FlashLight);
    let prepared = dn::layout::prepare_window(&mut app, bounds, screen, now);
    let frozen = Buffer::rendered(ui::render(&app, &prepared));
    assert_eq!(frozen[(47, 15)], expected[(47, 15)]);
    app.panels[1].entries.clear();
    let again = dn::layout::prepare_window(&mut app, bounds, screen, now);
    assert_eq!(frozen, Buffer::rendered(ui::render(&app, &again)));
    Ok(())
}
