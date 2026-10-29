use crossterm::event::KeyCode as K;
use dn::{
    app::App,
    input,
    mouse::Target,
    settings::{Settings, SettingsKind},
    testing::Buffer,
    tetris::{self, GameAction, Screen},
};
use std::io;
mod support;
use support::{draw, key, Fixture as Temp};
fn fixture() -> io::Result<(Temp, App)> {
    let t = Temp::new("default-buttons")?.canonicalized()?;
    let a = t.app()?;
    Ok((t, a))
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
    let (_t, mut app) = fixture()?;
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
        assert!(marked(&app, &b, dn::tetris::target(GameAction::Continue)));
        assert_eq!(app.desktop.game().unwrap().screen, Screen::GameOver);
    }
    key(&mut app, K::Enter);
    assert_eq!(app.desktop.game().unwrap().screen, Screen::Name);
    let b = draw(&mut app, 80, 25)?;
    assert!(marked(&app, &b, dn::tetris::target(GameAction::SubmitName)));
    assert!(!marked(&app, &b, dn::tetris::target(GameAction::SkipName)));
    input::paste(&mut app, "Still typing");
    assert!(app.desktop.game().unwrap().name.contains("Still typing"));
    key(&mut app, K::Esc);
    key(&mut app, K::Esc);
    key(&mut app, K::F(1));
    let b = draw(&mut app, 80, 25)?;
    assert!(marked(&app, &b, dn::tetris::target(GameAction::Back)));
    Ok(())
}
#[test]
fn settings_default_remains_visible_until_another_button_takes_focus() -> io::Result<()> {
    let (_t, mut app) = fixture()?;
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
    let (_t, mut app) = fixture()?;
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
    let (_t, mut app) = fixture()?;
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
