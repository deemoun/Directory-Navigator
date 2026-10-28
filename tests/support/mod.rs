//! Private filesystem fixtures and production input/render helpers for UI tests.
#![allow(dead_code)]
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use dn::{
    app::App,
    input,
    mouse::{self, Target},
    testing::{backend::TestBackend, Buffer, Terminal},
    ui,
};
use std::{
    fs, io,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};
pub struct Fixture(pub PathBuf);
impl Fixture {
    pub fn new(prefix: &str) -> io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "dn-{prefix}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
    pub fn canonicalized(mut self) -> io::Result<Self> {
        self.0 = fs::canonicalize(&self.0)?;
        Ok(self)
    }
    pub fn app(&self) -> io::Result<App> {
        let mut app = App::new(self.0.clone())?;
        app.config.save_on_exit = false;
        app.config_store = dn::config::Store::new(Some(self.0.join("config.conf")));
        wait(&mut app);
        Ok(app)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
pub fn wait(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.panels.iter().any(|p| p.loading)
        || app.desktop.active_editor().is_some_and(|e| e.busy())
        || app
            .desktop
            .active_viewer()
            .is_some_and(|v| v.contents.is_none())
    {
        app.poll();
        assert!(
            Instant::now() < deadline,
            "background UI work did not finish"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}
pub fn key(app: &mut App, code: KeyCode) {
    modified(app, code, KeyModifiers::NONE);
}
pub fn modified(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    input::handle(app, KeyEvent::new(code, modifiers));
}
pub fn render(app: &mut App, w: u16, h: u16) -> io::Result<Terminal<TestBackend>> {
    let mut terminal = Terminal::new(TestBackend::new(w, h))?;
    terminal.draw(|f| ui::draw(f, app))?;
    assert!(app
        .mouse
        .regions
        .iter()
        .all(|(r, _)| r.right() <= w && r.bottom() <= h));
    Ok(terminal)
}
pub fn draw(app: &mut App, w: u16, h: u16) -> io::Result<Buffer> {
    Ok(render(app, w, h)?.backend().buffer().clone())
}
pub fn click(app: &mut App, target: Target) {
    let r = app
        .mouse
        .regions
        .iter()
        .rev()
        .find(|(_, t)| *t == target)
        .expect("visible mouse target")
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

pub fn wait_until(app: &mut App, timeout: Duration, ready: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + timeout;
    loop {
        app.poll();
        if ready(app) {
            return;
        }
        assert!(Instant::now() < deadline, "worker timeout");
        std::thread::sleep(Duration::from_millis(1));
    }
}
pub fn text(app: &mut App, w: u16, h: u16) -> io::Result<String> {
    Ok(draw(app, w, h)?
        .content
        .iter()
        .map(|c| c.symbol())
        .collect())
}
pub fn click_at(app: &mut App, bounds: dn::presentation::layout::Rect) {
    mouse::handle(
        app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: bounds.x,
            row: bounds.y,
            modifiers: KeyModifiers::NONE,
        },
    );
}
