//! Model tests never launch a command; terminal transport is covered separately.
use crossterm::event::{
    Event, KeyCode as K, KeyEvent, KeyEventKind, KeyModifiers as M, MouseEvent, MouseEventKind,
};
use dn::{
    app::App,
    focus::{self, Focus, Screen},
    input, menu,
    operations::Kind,
    shortcuts::{self, Profile},
    terminal_output as output,
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
        let p = std::env::temp_dir().join(format!(
            "dn-output-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p)?;
        fs::write(p.join("fixture"), b"keep")?;
        Ok(Self(p))
    }
    fn app(&self) -> io::Result<App> {
        let mut a = App::new(self.0.clone())?;
        let until = Instant::now() + Duration::from_secs(5);
        while a.panels.iter().any(|p| p.loading) {
            a.poll();
            assert!(Instant::now() < until);
            thread::sleep(Duration::from_millis(1));
        }
        a.panels[0].cursor = a.panels[0]
            .entries
            .iter()
            .position(|e| e.name == "fixture")
            .unwrap();
        Ok(a)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn ctrl_o(a: &mut App) {
    input::handle(a, KeyEvent::new(K::Char('o'), M::CONTROL));
}
#[test]
fn classic_toggle_preserves_draft_panels_and_consumes_output_input() -> io::Result<()> {
    let t = Temp::new()?;
    let mut a = t.app()?;
    input::paste(&mut a, "untouched draft");
    a.panels[0].toggle_selection();
    let marks = a.panels[0].selected.clone();
    let paths = a.panels.each_ref().map(|p| p.path.clone());
    let cursors = a.panels.each_ref().map(|p| p.cursor);
    ctrl_o(&mut a);
    assert!(a.output_visible);
    assert_eq!(focus::current(&a), Focus::TerminalOutput);
    assert!(a.mouse.frame.is_none() && a.mouse.regions.is_empty());
    for code in [K::Char('x'), K::F(10), K::F(5), K::Down] {
        input::handle(&mut a, KeyEvent::new(code, M::NONE));
    }
    input::paste(&mut a, "rm -rf fake\n");
    dn::mouse::handle(
        &mut a,
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 1,
            row: 1,
            modifiers: M::NONE,
        },
    );
    assert!(a.output_visible && !a.quit && a.job.is_none() && a.command_line.pending.is_none());
    assert_eq!(a.command_line.text, "untouched draft");
    assert_eq!(a.panels[0].selected, marks);
    assert_eq!(a.panels.each_ref().map(|p| p.path.clone()), paths);
    assert_eq!(a.panels.each_ref().map(|p| p.cursor), cursors);
    ctrl_o(&mut a);
    assert!(!a.output_visible);
    assert_eq!(focus::current(&a), Focus::Manager);
    assert_eq!(fs::read(t.0.join("fixture"))?, b"keep");
    Ok(())
}
#[test]
fn mac_prefix_keeps_commands_and_double_ctrl_o_reveals_output() -> io::Result<()> {
    let t = Temp::new()?;
    let mut a = t.app()?;
    a.config.shortcuts = Profile::Mac;
    ctrl_o(&mut a);
    assert!(shortcuts::armed(&a) && !a.output_visible);
    input::handle(&mut a, KeyEvent::new(K::Esc, M::NONE));
    assert!(!shortcuts::armed(&a) && !a.output_visible);
    ctrl_o(&mut a);
    ctrl_o(&mut a);
    assert!(a.output_visible && !shortcuts::armed(&a));
    ctrl_o(&mut a);
    assert!(!a.output_visible && !shortcuts::armed(&a));
    ctrl_o(&mut a);
    input::handle(&mut a, KeyEvent::new(K::Char('h'), M::NONE));
    assert!(!a.output_visible && focus::current(&a) == Focus::CommandHistory);
    assert_eq!(
        shortcuts::label(Profile::Mac, Screen::Manager, "Ctrl+O"),
        "^O^O"
    );
    Ok(())
}
#[test]
fn modal_and_document_guards_and_pending_commands_block_reveal() -> io::Result<()> {
    let t = Temp::new()?;
    let mut a = t.app()?;
    a.prompt_operation(Kind::Copy, false);
    ctrl_o(&mut a);
    assert!(!a.output_visible && a.modals.prompt().is_some());
    a.modals.set_prompt(None);
    a.command_line.pending = Some(dn::command_line::shell_plan("never executed", t.0.clone()));
    output::request(&mut a);
    assert!(!a.output_visible);
    a.command_line.pending = None;
    dn::editor::open(&mut a, true);
    ctrl_o(&mut a);
    assert!(!a.output_visible);
    a.desktop.activate(dn::desktop::WindowId::MANAGER);
    menu::execute(&mut a, menu::Action::TerminalOutput);
    assert!(a.output_visible);
    a.error("Deferred background error");
    assert_eq!(focus::current(&a), Focus::TerminalOutput);
    input::handle(&mut a, KeyEvent::new(K::Enter, M::NONE));
    assert!(!a.output_visible);
    assert_eq!(focus::current(&a), Focus::Message);
    assert_eq!(
        a.last_dialog().unwrap().message,
        "Deferred background error"
    );
    Ok(())
}
#[test]
fn background_scans_continue_without_output_focus_loss_or_screensaver() -> io::Result<()> {
    let t = Temp::new()?;
    let mut a = App::new(t.0.clone())?;
    output::request(&mut a);
    assert!(a.output_visible);
    let until = Instant::now() + Duration::from_secs(5);
    while a.panels.iter().any(|p| p.loading) {
        a.poll();
        assert!(Instant::now() < until);
        thread::sleep(Duration::from_millis(1));
    }
    a.config.saver_minutes = 1;
    dn::screensaver::poll(&mut a, Instant::now() + Duration::from_secs(3600));
    assert!(a.output_visible && !a.screensaver.active);
    assert!(a.panels[0].entries.iter().any(|e| e.name == "fixture"));
    Ok(())
}
#[test]
fn only_explicit_return_presses_exit_output_and_menu_offers_the_action() -> io::Result<()> {
    for code in [K::Esc, K::Enter] {
        assert!(output::return_key(KeyEvent::new(code, M::NONE)));
        assert!(!output::return_key(KeyEvent::new(code, M::ALT)));
    }
    let mut key = KeyEvent::new(K::Char('o'), M::CONTROL);
    assert!(output::return_key(key));
    key.kind = KeyEventKind::Repeat;
    assert!(!output::return_key(key));
    key.kind = KeyEventKind::Release;
    assert!(!output::return_key(key));
    let t = Temp::new()?;
    let mut a = t.app()?;
    let mut repeat = KeyEvent::new(K::F(5), M::ALT);
    repeat.kind = KeyEventKind::Repeat;
    input::handle(&mut a, repeat);
    assert!(!a.output_visible && a.modals.prompt().is_none());
    input::handle(&mut a, KeyEvent::new(K::F(5), M::ALT));
    assert!(a.output_visible);
    output::event(&mut a, &Event::Paste("\u{f}\n".into()));
    assert!(a.output_visible);
    assert!(menu::items(4)
        .iter()
        .any(|i| i.action == menu::Action::TerminalOutput && i.shortcut == "Ctrl+O"));
    Ok(())
}
