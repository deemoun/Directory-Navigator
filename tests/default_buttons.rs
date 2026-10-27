use crossterm::event::{KeyCode as K, KeyEvent, KeyModifiers as M};
use dn::{
    app::App,
    input,
    mouse::Target,
    settings::{Settings, SettingsKind},
    testing::{backend::TestBackend, Buffer, Terminal},
    tetris::{self, GameAction, Screen},
    ui,
};
use std::{
    fs, io,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
struct Temp(PathBuf);
impl Temp {
    fn app() -> io::Result<(Self, App)> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "dn-default-buttons-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p)?;
        let t = Self(p);
        let mut app = App::new(t.0.clone())?;
        app.config.save_on_exit = false;
        app.config_store = dn::config::Store::new(Some(t.0.join("config.conf")));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while app.panels.iter().any(|p| p.loading) {
            app.poll();
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        Ok((t, app))
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn draw(app: &mut App, w: u16, h: u16) -> io::Result<Buffer> {
    let mut term = Terminal::new(TestBackend::new(w, h))?;
    term.draw(|f| ui::draw(f, app))?;
    Ok(term.backend().buffer().clone())
}
fn key(app: &mut App, code: K) {
    input::handle(app, KeyEvent::new(code, M::NONE));
}
fn marked(app: &App, b: &Buffer, target: Target) -> bool {
    let r = app
        .mouse
        .regions
        .iter()
        .find(|(r, t)| *t == target && r.width > 3)
        .expect("button target")
        .0;
    let marker = (r.x..r.right())
        .any(|x| b[(x, r.y)].symbol() == "►" && b[(x, r.y)].fg == app.config.palette.color(11));
    let caption = (r.x..r.right())
        .filter(|x| !matches!(b[(*x, r.y)].symbol(), " " | "►" | "◄"))
        .collect::<Vec<_>>();
    marker
        || !caption.is_empty()
            && caption
                .iter()
                .all(|x| b[(*x, r.y)].fg == app.config.palette.color(14))
}
#[test]
fn game_over_name_and_help_show_their_default_on_first_paint() -> io::Result<()> {
    let (_t, mut app) = Temp::app()?;
    tetris::open(&mut app);
    {
        let g = app.desktop.game_mut().unwrap();
        g.screen = Screen::GameOver;
        g.over = true;
        g.score = 3650;
        g.submitted = false;
    }
    for (w, h) in [(80, 25), (132, 40)] {
        let b = draw(&mut app, w, h)?;
        assert!(marked(&app, &b, Target::Game(GameAction::Continue)));
        assert_eq!(app.desktop.game().unwrap().screen, Screen::GameOver);
    }
    key(&mut app, K::Enter);
    assert_eq!(app.desktop.game().unwrap().screen, Screen::Name);
    let b = draw(&mut app, 80, 25)?;
    assert!(marked(&app, &b, Target::Game(GameAction::SubmitName)));
    assert!(!marked(&app, &b, Target::Game(GameAction::SkipName)));
    input::paste(&mut app, "Still typing");
    assert!(app.desktop.game().unwrap().name.contains("Still typing"));
    key(&mut app, K::Esc);
    key(&mut app, K::Esc);
    key(&mut app, K::F(1));
    let b = draw(&mut app, 80, 25)?;
    assert!(marked(&app, &b, Target::Game(GameAction::Back)));
    Ok(())
}
#[test]
fn settings_default_remains_visible_until_another_button_takes_focus() -> io::Result<()> {
    let (_t, mut app) = Temp::app()?;
    app.modals
        .set_settings(Some(Settings::new(SettingsKind::Interface, &app.config)));
    let b = draw(&mut app, 80, 25)?;
    assert!(marked(&app, &b, Target::AcceptSettings));
    assert!(!marked(&app, &b, Target::CancelSettings));
    assert_eq!(app.modals.settings().unwrap().cursor, 0);
    let count = app.modals.settings().unwrap().rows().len();
    for _ in 0..count + 1 {
        key(&mut app, K::Tab);
    }
    let b = draw(&mut app, 80, 25)?;
    assert!(!marked(&app, &b, Target::AcceptSettings));
    assert!(marked(&app, &b, Target::CancelSettings));
    key(&mut app, K::Enter);
    assert!(app.modals.settings().is_none());
    Ok(())
}
#[test]
fn editor_defaults_keep_text_input_active_and_unsaved_cancel_safe() -> io::Result<()> {
    use dn::editor::{self, EditorAction as A, Modal};
    let (_t, mut app) = Temp::app()?;
    editor::open(&mut app, true);
    editor::paste(&mut app, "retained edits");
    editor::action(&mut app, A::GoTo);
    let b = draw(&mut app, 80, 25)?;
    assert!(marked(&app, &b, Target::Editor(A::Submit)));
    assert!(!marked(&app, &b, Target::Editor(A::Cancel)));
    input::paste(&mut app, "12");
    assert!(
        matches!(app.desktop.active_editor().unwrap().modals.editor_modal(),Some(Modal::Prompt{field,..}) if field=="12")
    );
    key(&mut app, K::Esc);
    key(&mut app, K::Esc);
    let b = draw(&mut app, 80, 25)?;
    assert!(marked(&app, &b, Target::Editor(A::Cancel)));
    assert!(!marked(&app, &b, Target::Editor(A::Save)));
    assert!(!marked(&app, &b, Target::Editor(A::Discard)));
    key(&mut app, K::Enter);
    assert!(app.desktop.active_editor().is_some_and(|e| e.dirty()));
    Ok(())
}
#[test]
fn list_default_and_pattern_default_are_unique_without_moving_model_focus() -> io::Result<()> {
    let (_t, mut app) = Temp::app()?;
    dn::windowing::open(&mut app);
    let b = draw(&mut app, 80, 25)?;
    assert!(marked(
        &app,
        &b,
        Target::Window(dn::windowing::Action::Switch)
    ));
    assert!(!marked(
        &app,
        &b,
        Target::Window(dn::windowing::Action::Cancel)
    ));
    assert_eq!(app.modals.window_list().unwrap().focus, 0);
    key(&mut app, K::Esc);
    app.open_pattern(true);
    let b = draw(&mut app, 80, 25)?;
    assert!(marked(&app, &b, Target::AcceptPattern));
    assert!(!marked(&app, &b, Target::CancelPattern));
    input::paste(&mut app, "*.rs");
    assert_eq!(app.modals.pattern().unwrap().field, "*.rs");
    Ok(())
}
