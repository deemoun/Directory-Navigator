use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
#[cfg(not(windows))]
use dn::launcher;
#[cfg(unix)]
use dn::launcher::{Launched, Launcher};
use dn::{
    app::App,
    input,
    mouse::{self, Target},
    ui,
};
use ratatui::{backend::TestBackend, Terminal};
use std::{
    fs, io,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
struct Temp(PathBuf);
impl Temp {
    fn new() -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "dn-launch-{}-{}",
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
    let end = Instant::now() + Duration::from_secs(5);
    while !done(app) {
        app.poll();
        assert!(Instant::now() < end);
        thread::sleep(Duration::from_millis(1));
    }
}
fn focus(app: &mut App, name: &str) {
    app.panels[0].cursor = app.panels[0]
        .entries
        .iter()
        .position(|e| e.name == name)
        .unwrap();
}
fn key(app: &mut App, code: KeyCode) {
    input::handle(app, KeyEvent::new(code, KeyModifiers::NONE));
}

#[test]
fn enter_missing_file_reports_error_instead_of_opening_internal_tools_and_keys_remain(
) -> io::Result<()> {
    let temp = Temp::new()?;
    let file = temp.0.join("file.txt");
    fs::write(&file, "text")?;
    let mut app = temp.app()?;
    focus(&mut app, "file.txt");
    fs::remove_file(&file)?;
    key(&mut app, KeyCode::Enter);
    assert!(app.launcher.loading());
    assert!(app.viewer.is_none() && app.editor.is_none());
    wait(&mut app, |a| !a.launcher.loading());
    assert!(app.dialogs.last().unwrap().message.contains("Cannot open"));
    key(&mut app, KeyCode::Enter);
    fs::write(&file, "restored")?;
    key(&mut app, KeyCode::F(3));
    wait(&mut app, |a| {
        a.viewer.as_ref().is_some_and(|v| v.contents.is_some())
    });
    key(&mut app, KeyCode::Esc);
    key(&mut app, KeyCode::F(4));
    wait(&mut app, |a| a.editor.as_ref().is_some_and(|e| !e.busy()));
    assert!(app.editor.is_some());
    Ok(())
}

#[test]
fn directory_enter_and_missing_file_double_click_share_correct_actions() -> io::Result<()> {
    let temp = Temp::new()?;
    fs::create_dir(temp.0.join("folder"))?;
    let file = temp.0.join("file.txt");
    fs::write(&file, "")?;
    let mut app = temp.app()?;
    focus(&mut app, "folder");
    key(&mut app, KeyCode::Enter);
    assert!(app.panels[0].loading);
    wait(&mut app, |a| !a.panels[0].loading);
    assert_eq!(app.panels[0].path, temp.0.join("folder"));
    key(&mut app, KeyCode::Backspace);
    wait(&mut app, |a| !a.panels[0].loading);
    focus(&mut app, "file.txt");
    fs::remove_file(&file)?;
    let mut terminal = Terminal::new(TestBackend::new(80, 25))?;
    terminal.draw(|f| ui::draw(f, &mut app))?;
    let rect = app
        .mouse
        .regions
        .iter()
        .find(|(_, t)| *t == Target::Entry(0, app.panels[0].cursor))
        .unwrap()
        .0;
    let event = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: rect.x,
        row: rect.y,
        modifiers: KeyModifiers::NONE,
    };
    mouse::handle(&mut app, event);
    mouse::handle(&mut app, event);
    assert!(app.launcher.loading());
    assert!(app.viewer.is_none());
    wait(&mut app, |a| !a.launcher.loading());
    assert!(!app.dialogs.is_empty());
    Ok(())
}

#[cfg(unix)]
#[test]
fn file_and_directory_symlinks_and_broken_links_are_classified_fresh() -> io::Result<()> {
    use std::os::unix::fs::symlink;
    let temp = Temp::new()?;
    fs::write(temp.0.join("file"), "")?;
    fs::create_dir(temp.0.join("dir"))?;
    symlink("file", temp.0.join("file-link"))?;
    symlink("dir", temp.0.join("dir-link"))?;
    symlink("missing", temp.0.join("broken"))?;
    assert!(!launcher::is_directory(&temp.0.join("file-link"))?);
    assert!(launcher::is_directory(&temp.0.join("dir-link"))?);
    assert!(launcher::is_directory(&temp.0.join("broken")).is_err());
    let mut app = temp.app()?;
    focus(&mut app, "dir-link");
    key(&mut app, KeyCode::Enter);
    wait(&mut app, |a| !a.launcher.loading() && !a.panels[0].loading);
    assert_eq!(app.panels[0].path, temp.0.join("dir-link"));
    #[cfg(target_os = "macos")]
    {
        fs::create_dir(temp.0.join("Example.app"))?;
        assert!(!launcher::is_directory(&temp.0.join("Example.app"))?);
        symlink("Example.app", temp.0.join("app-link"))?;
        assert!(!launcher::is_directory(&temp.0.join("app-link"))?);
    }
    Ok(())
}

#[cfg(not(windows))]
#[test]
fn opener_passes_one_absolute_native_argument_without_shell_parsing() -> io::Result<()> {
    let temp = Temp::new()?;
    let path = temp.0.join("-文 ' \" ; $(touch NEVER) &.txt");
    let command = launcher::opener_command(&path)?;
    assert_eq!(
        command.get_args().collect::<Vec<_>>(),
        vec![path.as_os_str()]
    );
    assert_eq!(command.get_current_dir(), Some(temp.0.as_path()));
    #[cfg(target_os = "macos")]
    assert_eq!(command.get_program(), "/usr/bin/open");
    #[cfg(target_os = "linux")]
    assert_eq!(command.get_program(), "xdg-open");
    use std::os::unix::ffi::OsStringExt;
    let native = temp
        .0
        .join(std::ffi::OsString::from_vec(b"native-\xff".to_vec()));
    assert_eq!(
        launcher::opener_command(&native)?.get_args().next(),
        Some(native.as_os_str())
    );
    Ok(())
}

#[test]
fn background_launch_preserves_native_request_panel_and_reports_backend_errors() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    let path = temp.0.join("document.pdf");
    let original = path.clone();
    let (send, receive) = mpsc::channel();
    app.launcher.request_with(1, path, move |path| {
        send.send(path.to_owned()).unwrap();
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "No default application",
        ))
    })?;
    wait(&mut app, |a| !a.launcher.loading());
    assert_eq!(receive.recv().unwrap(), original);
    assert!(app
        .dialogs
        .last()
        .unwrap()
        .message
        .contains("No default application"));
    assert!(app.viewer.is_none() && app.editor.is_none());
    Ok(())
}

#[cfg(unix)]
#[test]
fn opener_processes_do_not_block_further_requests_and_failures_are_reaped() -> io::Result<()> {
    let temp = Temp::new()?;
    let path = temp.0.join("file");
    let mut launcher = Launcher::default();
    launcher.request_with(1, path.clone(), |_| {
        std::process::Command::new("/bin/sh")
            .args(["-c", "sleep 0.2; exit 7"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map(Launched::Process)
    })?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while launcher.loading() {
        launcher.poll();
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    launcher.request_with(0, path.clone(), |_| Ok(Launched::Submitted))?;
    loop {
        let replies = launcher.poll();
        if let Some(reply) = replies.into_iter().find(|r| r.result.is_err()) {
            assert_eq!(reply.path, path);
            assert_eq!(reply.panel, 1);
            assert!(reply
                .result
                .err()
                .unwrap()
                .to_string()
                .contains("default application"));
            break;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    Ok(())
}
