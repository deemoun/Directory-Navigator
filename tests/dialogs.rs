use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use dn::testing::{backend::TestBackend, layout::Rect, Terminal};
use dn::{
    app::App,
    input,
    mouse::{self, Target},
    operations::Kind,
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
            "dn-dialogs-{}-{}",
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
fn mkdir_original_dimensions_colors_buttons_and_resize() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    app.prompt_operation(Kind::Mkdir, false);
    for (w, h) in [(80, 25), (132, 40), (80, 25)] {
        let mut terminal = Terminal::new(TestBackend::new(w, h))?;
        terminal.draw(|f| ui::draw(f, &mut app))?;
        let field = app
            .mouse
            .regions
            .iter()
            .find(|(_, t)| *t == Target::PromptField)
            .expect("input")
            .0;
        let rect = Rect::new(field.x - 2, field.y - 2, field.width + 4, 7);
        assert_eq!(rect.width, 49);
        assert_eq!(rect.x, (w - 49) / 2);
        assert_eq!(rect.y, (h - 7) / 2);
        let b = terminal.backend().buffer();
        assert_eq!(b[(rect.x, rect.y)].symbol(), "╔");
        assert_eq!(b[(rect.right() - 1, rect.bottom() - 1)].symbol(), "╝");
        assert_eq!(b[(rect.x, rect.y)].fg, app.config.palette.color(15));
        assert_eq!(b[(field.x, field.y)].bg, app.config.palette.color(0));
        for (r, t) in &app.mouse.regions {
            assert!(
                r.x > rect.x
                    && r.y >= rect.y
                    && r.right() < rect.right()
                    && r.bottom() < rect.bottom()
            );
            if *t == Target::SubmitPrompt {
                assert_eq!(b[(r.x, r.y)].symbol(), " ");
                assert_eq!(b[(r.right() - 1, r.y)].symbol(), " ");
                assert_eq!(b[(r.right() - 1, r.y)].bg, app.config.palette.color(8));
                assert_eq!(b[(r.x, r.y)].fg, app.config.palette.color(15));
                assert_eq!(b[(r.x, r.y)].bg, app.config.palette.color(2));
            }
        }
    }
    assert!(draw(&mut app, 50, 12)?.contains("Terminal too small"));
    assert!(app.mouse.regions.is_empty());
    assert!(draw(&mut app, 80, 25)?.contains("Make directory"));
    Ok(())
}
#[test]
fn prompt_tab_cancel_help_mouse_and_unicode_creation() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    app.prompt_operation(Kind::Mkdir, false);
    app.modals
        .prompt_mut()
        .expect("prompt")
        .paste("新しい e\u{301}");
    key(&mut app, KeyCode::F(1), KeyModifiers::NONE);
    assert!(draw(&mut app, 80, 25)?.contains("never overwritten"));
    assert!(app
        .mouse
        .regions
        .iter()
        .all(|(_, t)| *t == Target::CloseDialog));
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(
        app.modals.prompt().expect("prompt").field,
        "新しい e\u{301}"
    );
    key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
    key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
    assert_eq!(app.modals.prompt().expect("prompt").focus, 2);
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.modals.prompt().is_none());
    assert!(!temp.0.join("新しい e\u{301}").exists());
    app.prompt_operation(Kind::Mkdir, false);
    key(&mut app, KeyCode::BackTab, KeyModifiers::SHIFT);
    assert_eq!(app.modals.prompt().expect("prompt").focus, 3);
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    draw(&mut app, 80, 25)?;
    click(&mut app, Target::CloseDialog);
    draw(&mut app, 80, 25)?;
    click(&mut app, Target::PromptField);
    app.modals
        .prompt_mut()
        .expect("prompt")
        .paste("新しい e\u{301}");
    click(&mut app, Target::PromptHelp);
    draw(&mut app, 80, 25)?;
    click(&mut app, Target::CloseDialog);
    draw(&mut app, 80, 25)?;
    click(&mut app, Target::SubmitPrompt);
    wait(&mut app, |a| {
        a.job.is_none() && a.panels.iter().all(|p| !p.loading)
    });
    assert!(temp.0.join("新しい e\u{301}").is_dir());
    assert!(app.dialogs().is_empty());
    Ok(())
}
#[test]
fn compact_messages_scroll_and_restore_underlying_prompt() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    app.prompt_goto();
    app.error("Short error.");
    let mut terminal = Terminal::new(TestBackend::new(80, 25))?;
    terminal.draw(|f| ui::draw(f, &mut app))?;
    // A short message occupies a small 36×6 window, not a fixed 66×17 one.
    let b = terminal.backend().buffer();
    assert_eq!(b[(22, 9)].symbol(), "╔");
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    app.error(
        (0..100)
            .map(|i| format!("Line {i:03}: Unicode 界e\u{301}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    let initial = draw(&mut app, 80, 25)?;
    assert!(initial.contains("Line 000"));
    assert!(initial.contains("scroll"));
    key(&mut app, KeyCode::End, KeyModifiers::NONE);
    assert!(draw(&mut app, 80, 25)?.contains("Line 099"));
    assert!(!draw(&mut app, 80, 25)?.contains("Line 000"));
    key(&mut app, KeyCode::Home, KeyModifiers::NONE);
    assert!(draw(&mut app, 80, 25)?.contains("Line 000"));
    mouse::handle(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 40,
            row: 12,
            modifiers: KeyModifiers::NONE,
        },
    );
    assert_eq!(app.message_offset(), 3);
    draw(&mut app, 80, 25)?;
    click(&mut app, Target::CloseDialog);
    assert_eq!(app.message_offset(), 0);
    assert!(app.modals.prompt().is_some());
    Ok(())
}
#[test]
fn editor_small_prompts_and_unsaved_guard_remain_usable() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    dn::editor::open(&mut app, true);
    draw(&mut app, 80, 25)?;
    dn::editor::paste(&mut app, "unchanged text");
    dn::editor::action(&mut app, dn::editor::EditorAction::GoTo);
    for (w, h) in [(80, 25), (132, 40)] {
        let screen = draw(&mut app, w, h)?;
        assert!(screen.contains("Line number"));
        let ok = app
            .mouse
            .regions
            .iter()
            .find(|(_, t)| *t == Target::Editor(dn::editor::EditorAction::Submit))
            .expect("OK")
            .0;
        assert_eq!(ok.y, (h - 7) / 2 + 4);
    }
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    let screen = draw(&mut app, 80, 25)?;
    assert!(screen.contains("unsaved changes"));
    click(&mut app, Target::Editor(dn::editor::EditorAction::Cancel));
    assert!(draw(&mut app, 80, 25)?.contains("unchanged text"));
    Ok(())
}

fn title_close(app: &mut App, target: Target) -> io::Result<()> {
    let r = app
        .mouse
        .regions
        .iter()
        .find(|(r, t)| *t == target && r.width == 3)
        .expect("title close target")
        .0;
    mouse::handle(
        app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: r.x + 1,
            row: r.y,
            modifiers: KeyModifiers::NONE,
        },
    );
    Ok(())
}
#[test]
fn green_title_close_matches_dn_and_cancels_without_creating_directory() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    app.prompt_operation(Kind::Mkdir, false);
    app.modals
        .prompt_mut()
        .expect("prompt")
        .paste("must-not-exist");
    for (w, h) in [(80, 25), (132, 40), (80, 25)] {
        let mut terminal = Terminal::new(TestBackend::new(w, h))?;
        terminal.draw(|f| ui::draw(f, &mut app))?;
        let r = app
            .mouse
            .regions
            .iter()
            .find(|(r, t)| *t == Target::CancelPrompt && r.width == 3)
            .expect("close")
            .0;
        let b = terminal.backend().buffer();
        assert_eq!(r.x, (w - 49) / 2 + 2);
        assert_eq!(r.y, (h - 7) / 2);
        for (dx, symbol, fg) in [(0, "[", 15), (1, "■", 10), (2, "]", 15)] {
            assert_eq!(b[(r.x + dx, r.y)].symbol(), symbol);
            assert_eq!(b[(r.x + dx, r.y)].fg, app.config.palette.color(fg));
            assert_eq!(b[(r.x + dx, r.y)].bg, app.config.palette.color(7));
        }
    }
    app.error("A message above the directory prompt.");
    draw(&mut app, 80, 25)?;
    title_close(&mut app, Target::CloseDialog)?;
    assert!(app.dialogs().is_empty());
    assert!(app.modals.prompt().is_some());
    draw(&mut app, 80, 25)?;
    title_close(&mut app, Target::CancelPrompt)?;
    assert!(app.modals.prompt().is_none());
    assert!(!temp.0.join("must-not-exist").exists());
    Ok(())
}
#[test]
fn title_close_restores_previews_and_keeps_unsaved_document() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    let original = app.config.palette;
    dn::palette::open(&mut app);
    dn::palette::choose(&mut app, dn::palette::PaletteProfile::Terminal);
    draw(&mut app, 80, 25)?;
    title_close(&mut app, Target::CancelColors)?;
    assert_eq!(app.config.palette, original);
    assert!(app.modals.color_dialog().is_none());
    app.open_columns();
    let original = app.config.columns;
    dn::columns::choose(&mut app, 0, dn::columns::ColumnPreset::NameSizeModified);
    draw(&mut app, 80, 25)?;
    title_close(&mut app, Target::CancelColumns)?;
    assert_eq!(app.config.columns, original);
    dn::editor::open(&mut app, true);
    draw(&mut app, 80, 25)?;
    dn::editor::paste(&mut app, "unsaved content");
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    draw(&mut app, 80, 25)?;
    title_close(&mut app, Target::Editor(dn::editor::EditorAction::Cancel))?;
    assert!(app
        .desktop
        .active_editor()
        .is_some_and(|e| e.modals.editor_modal().is_none()));
    assert!(draw(&mut app, 80, 25)?.contains("unsaved content"));
    Ok(())
}
