use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use dn::{
    app::App,
    config::Config,
    input, menu,
    mouse::{self, Target},
    screensaver::{self, State},
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
        assert_eq!(app.screensaver.setup.as_ref().map(|s| s.delay), Some(60));
        click(&mut app, Target::Screensaver(3));
        assert_eq!(app.screensaver.setup.as_ref().map(|s| s.delay), Some(5));
        click(&mut app, Target::Screensaver(screensaver::PREVIEW));
        let sky = draw(&mut app, w, h)?;
        assert!(!sky.contains("Screen savers"));
        assert!(!sky.contains("Help"));
        assert!(app.mouse.regions.is_empty());
        key(&mut app, KeyCode::F(10), KeyModifiers::NONE);
        assert!(!app.quit);
        assert!(!app.screensaver.active);
    }
    draw(&mut app, 80, 25)?;
    click(&mut app, Target::Screensaver(screensaver::CANCEL));
    assert_eq!(app.config.saver_minutes, 10);
    screensaver::open(&mut app);
    key(&mut app, KeyCode::Down, KeyModifiers::NONE);
    key(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
    key(&mut app, KeyCode::Char('p'), KeyModifiers::NONE);
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
    wait(&mut app, |a| a.editor.as_ref().is_some_and(|e| !e.busy()));
    draw(&mut app, 80, 25)?;
    dn::editor::paste(&mut app, "unsaved text");
    let idle = Instant::now();
    app.screensaver.activity(idle);
    assert!(screensaver::poll(&mut app, idle + Duration::from_secs(600)));
    assert!(app.screensaver.active);
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app
        .editor
        .as_ref()
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
    key(&mut app, KeyCode::Char('x'), KeyModifiers::ALT);
    assert!(!app.quit);
    assert!(app.dialogs.is_empty());
    assert!(app.editor.as_ref().is_some_and(|e| e.modal.is_none()));
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
    app.screensaver.setup = None;
    assert!(!screensaver::poll(&mut app, now + Duration::from_secs(179)));
    assert!(screensaver::poll(&mut app, now + Duration::from_secs(180)));
    app.screensaver.activity(now);
    dn::tetris::open(&mut app);
    assert!(!screensaver::poll(&mut app, now + Duration::from_secs(300)));
    assert!(!app.screensaver.active);
    Ok(())
}
#[test]
fn config_round_trip_legacy_and_invalid_values() -> io::Result<()> {
    let temp = Temp::new()?;
    let path = temp.0.join("config.conf");
    assert_eq!(Config::read(&path)?.saver_minutes, 10);
    fs::write(&path, "hidden=true\n")?;
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
        fs::write(&path, format!("saver_minutes={value}\n"))?;
        assert_eq!(Config::read(&path)?.saver_minutes, 10);
    }
    Ok(())
}
