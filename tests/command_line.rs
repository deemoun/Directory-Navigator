use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dn::testing::{backend::TestBackend, Terminal};
use dn::{app::App, input};
use std::{
    fs, io,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
struct Temp(PathBuf);
impl Temp {
    fn new() -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "dn-command-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path)?;
        fs::create_dir(path.join("child space"))?;
        fs::write(path.join("file"), "test")?;
        Ok(Self(path))
    }
    fn app(&self) -> io::Result<App> {
        let mut app = App::new(self.0.clone())?;
        ready(&mut app);
        Ok(app)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn ready(app: &mut App) {
    let limit = Instant::now() + Duration::from_secs(3);
    while app.panels.iter().any(|p| p.loading) {
        app.poll();
        assert!(Instant::now() < limit);
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn key(app: &mut App, code: KeyCode) {
    input::handle(app, KeyEvent::new(code, KeyModifiers::NONE));
}
fn ctrl(app: &mut App, c: char) {
    input::handle(app, KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
}

#[test]
fn typing_editing_and_paste_preserve_graphemes_and_limits() -> io::Result<()> {
    let t = Temp::new()?;
    let mut app = t.app()?;
    input::paste(&mut app, "e\u{301}👩‍💻界");
    key(&mut app, KeyCode::Left);
    key(&mut app, KeyCode::Backspace);
    assert_eq!(app.command_line.text, "e\u{301}界");
    key(&mut app, KeyCode::Delete);
    assert_eq!(app.command_line.text, "e\u{301}");
    key(&mut app, KeyCode::Home);
    key(&mut app, KeyCode::Right);
    key(&mut app, KeyCode::Backspace);
    assert!(app.command_line.text.is_empty());
    input::paste(&mut app, "safe");
    input::paste(&mut app, "\nrm something");
    assert_eq!(app.command_line.text, "safe");
    assert!(app.command_line.pending.is_none());
    app.dialogs.clear();
    input::paste(&mut app, &"x".repeat(16 * 1024));
    assert_eq!(app.command_line.text, "safe");
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.command_line.text, "safe"); // dismiss the error first
    key(&mut app, KeyCode::Esc);
    assert!(app.command_line.text.is_empty());
    Ok(())
}
#[test]
fn typed_shell_commands_queue_with_active_native_directory() -> io::Result<()> {
    let t = Temp::new()?;
    let mut app = t.app()?;
    for c in "printf hello".chars() {
        key(&mut app, KeyCode::Char(c));
    }
    key(&mut app, KeyCode::Enter);
    let plan = app.command_line.pending.take().unwrap();
    assert_eq!(plan.cwd, t.0);
    assert_eq!(plan.commands.len(), 1);
    assert_eq!(plan.commands[0].args.last().unwrap(), "printf hello");
    assert!(app.command_line.text.is_empty());
    Ok(())
}
#[test]
fn submitted_history_wraps_to_draft_and_stays_session_only() -> io::Result<()> {
    let t = Temp::new()?;
    let mut app = t.app()?;
    for command in ["first", "second", "first"] {
        input::paste(&mut app, command);
        key(&mut app, KeyCode::Enter);
        app.command_line.pending.take();
    }
    input::paste(&mut app, "draft");
    ctrl(&mut app, 'e');
    assert_eq!(app.command_line.text, "first");
    ctrl(&mut app, 'e');
    assert_eq!(app.command_line.text, "second");
    ctrl(&mut app, 'e');
    assert_eq!(app.command_line.text, "second");
    ctrl(&mut app, 'x');
    assert_eq!(app.command_line.text, "first");
    ctrl(&mut app, 'x');
    assert_eq!(app.command_line.text, "draft");
    let mut other = t.app()?;
    ctrl(&mut other, 'e');
    assert!(other.command_line.text.is_empty());
    Ok(())
}
#[test]
fn cd_navigates_and_missing_directories_preserve_panel() -> io::Result<()> {
    let t = Temp::new()?;
    let mut app = t.app()?;
    input::paste(&mut app, "cd \"child space\"");
    key(&mut app, KeyCode::Enter);
    ready(&mut app);
    assert_eq!(app.panels[0].path, t.0.join("child space"));
    assert!(app.command_line.pending.is_none());
    input::paste(&mut app, "cd absent");
    key(&mut app, KeyCode::Enter);
    ready(&mut app);
    assert_eq!(app.panels[0].path, t.0.join("child space"));
    assert!(!app.dialogs.is_empty());
    Ok(())
}
#[test]
fn empty_prompt_keeps_panel_shortcuts_and_nonempty_prompt_keeps_navigation() -> io::Result<()> {
    let t = Temp::new()?;
    let mut app = t.app()?;
    key(&mut app, KeyCode::Char('+'));
    assert!(app.pattern.is_some());
    app.pattern = None;
    input::paste(&mut app, "echo ");
    key(&mut app, KeyCode::Char('*'));
    assert_eq!(app.command_line.text, "echo *");
    key(&mut app, KeyCode::Down);
    assert_eq!(app.panels[0].cursor, 1);
    key(&mut app, KeyCode::Tab);
    assert_eq!(app.active, 1);
    assert_eq!(app.command_line.text, "echo *");
    key(&mut app, KeyCode::F(7));
    assert!(app.prompt.is_some());
    input::paste(&mut app, "directory");
    assert_eq!(app.command_line.text, "echo *");
    Ok(())
}
#[test]
fn prompt_rendering_resizes_and_keeps_cursor_visible_without_statuses() -> io::Result<()> {
    let t = Temp::new()?;
    let mut app = t.app()?;
    input::paste(&mut app, &"👩‍💻界".repeat(80));
    app.notice = "ODD STATUS".into();
    for (w, h) in [(80, 25), (132, 40), (80, 25)] {
        let mut terminal = Terminal::new(TestBackend::new(w, h))?;
        terminal.draw(|f| dn::ui::draw(f, &mut app))?;
        let cursor = terminal.get_cursor_position()?;
        assert_eq!(cursor.y, h - 2);
        assert!(cursor.x < w);
        let line: String = (0..w)
            .map(|x| terminal.backend().buffer()[(x, h - 2)].symbol())
            .collect();
        assert!(!line.contains("ODD STATUS"));
        assert!(line.contains('>'));
        assert!(app
            .mouse
            .regions
            .iter()
            .any(|(r, t)| *t == dn::mouse::Target::CommandLine && r.y == h - 2));
    }
    Ok(())
}
#[cfg(unix)]
#[test]
fn command_runner_executes_in_disposable_directory_and_reports_spawn_error() -> io::Result<()> {
    use dn::user_menu::runner;
    let t = Temp::new()?;
    let plan = dn::command_line::shell_plan("printf 'hello' > result", t.0.clone());
    assert!(runner::run(&plan)?.success());
    assert_eq!(fs::read_to_string(t.0.join("result"))?, "hello");
    let failed = runner::Plan {
        cwd: t.0.clone(),
        commands: vec![runner::CommandSpec {
            program: t.0.join("missing-executable").into_os_string(),
            args: Vec::new(),
        }],
        env: Vec::new(),
    };
    assert!(runner::run(&failed).is_err());
    Ok(())
}
