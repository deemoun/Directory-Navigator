use crossterm::event::{KeyCode as K, KeyEvent, KeyModifiers as M};
use dn::{
    app::App,
    input,
    mouse::Target,
    settings::{Settings, SettingsKind},
    testing::Buffer,
    tetris::{self, GameAction, Screen},
};
use std::{fs, io};
mod support;
use support::{draw, key, Fixture as Temp};
fn fixture() -> io::Result<(Temp, App)> {
    let t = Temp::new("dialog-navigation")?.canonicalized()?;
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
fn shift_tab(app: &mut App) {
    input::handle(app, KeyEvent::new(K::Tab, M::SHIFT));
}
fn wait(app: &mut App, done: impl Fn(&App) -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        app.poll();
        if done(app) {
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}
fn ended(app: &mut App) {
    let g = app.desktop.game_mut().unwrap();
    g.screen = Screen::GameOver;
    g.over = true;
    g.score = 3650;
    g.submitted = false;
    g.page_focus = 0;
}
#[test]
fn tetris_name_cycles_both_directions_and_cancel_keeps_unsaved_score_and_draft() -> io::Result<()> {
    let (t, mut app) = fixture()?;
    tetris::open(&mut app);
    ended(&mut app);
    draw(&mut app, 80, 25)?;
    key(&mut app, K::Tab);
    assert_eq!(app.desktop.game().unwrap().page_focus, 0);
    key(&mut app, K::Enter);
    input::paste(&mut app, "Kept draft");
    key(&mut app, K::Tab);
    assert_eq!(app.desktop.game().unwrap().page_focus, 1);
    input::paste(&mut app, "ignored");
    assert_eq!(app.desktop.game().unwrap().name, "Kept draft");
    key(&mut app, K::Tab);
    let b = draw(&mut app, 80, 25)?;
    assert!(marked(&app, &b, dn::tetris::target(GameAction::Back)));
    assert!(!marked(
        &app,
        &b,
        dn::tetris::target(GameAction::SubmitName)
    ));
    shift_tab(&mut app);
    assert_eq!(app.desktop.game().unwrap().page_focus, 1);
    key(&mut app, K::BackTab);
    assert_eq!(app.desktop.game().unwrap().page_focus, 0);
    key(&mut app, K::Esc);
    assert_eq!(app.desktop.game().unwrap().screen, Screen::GameOver);
    assert_eq!(app.desktop.game().unwrap().name, "Kept draft");
    assert!(app.desktop.plugins.scores_mut().board.records.is_empty());
    assert!(!t.0.join("tetris.conf").exists());
    key(&mut app, K::Esc);
    assert_eq!(app.desktop.game().unwrap().screen, Screen::Play);
    assert!(app.desktop.game().unwrap().over);
    assert!(app.desktop.plugins.scores_mut().board.records.is_empty());
    ended(&mut app);
    key(&mut app, K::Enter);
    shift_tab(&mut app);
    assert_eq!(app.desktop.game().unwrap().page_focus, 4);
    key(&mut app, K::Enter);
    assert_eq!(app.desktop.game().unwrap().screen, Screen::Scores);
    assert_eq!(
        app.desktop.plugins.scores_mut().board.records[0].name,
        "Anonymous"
    );
    assert!(t.0.join("tetris.conf").exists());
    Ok(())
}
#[test]
fn tetris_main_buttons_setup_and_retry_have_real_focus_and_pause_gravity() -> io::Result<()> {
    let (_t, mut app) = fixture()?;
    tetris::open(&mut app);
    let board = app.desktop.game().unwrap().board;
    shift_tab(&mut app);
    assert_eq!(app.desktop.game().unwrap().play_focus, 4);
    let b = draw(&mut app, 80, 25)?;
    assert!(marked(&app, &b, dn::tetris::target(GameAction::Pause)));
    let y = app.desktop.game().unwrap().piece.y;
    assert!(!app
        .desktop
        .game_mut()
        .unwrap()
        .tick(std::time::Instant::now() + std::time::Duration::from_secs(30)));
    assert_eq!(app.desktop.game().unwrap().piece.y, y);
    key(&mut app, K::Esc);
    assert_eq!(app.desktop.game().unwrap().play_focus, 0);
    key(&mut app, K::Tab);
    key(&mut app, K::Tab);
    key(&mut app, K::Enter);
    assert!(matches!(
        app.desktop.game().unwrap().screen,
        Screen::Setup { .. }
    ));
    shift_tab(&mut app);
    assert_eq!(app.desktop.game().unwrap().setup_focus, 4);
    let b = draw(&mut app, 80, 25)?;
    assert!(marked(&app, &b, dn::tetris::target(GameAction::Back)));
    key(&mut app, K::Enter);
    assert_eq!(app.desktop.game().unwrap().screen, Screen::Play);
    assert_eq!(app.desktop.game().unwrap().board, board);
    app.desktop
        .plugins
        .scores_mut()
        .add(tetris::Record {
            name: "Pending".into(),
            score: 1,
            lines: Some(0),
            start_level: 1,
            level: 1,
        })
        .unwrap();
    tetris::action(&mut app, GameAction::Scores);
    key(&mut app, K::Tab);
    assert_eq!(app.desktop.game().unwrap().page_focus, 1);
    let b = draw(&mut app, 80, 25)?;
    assert!(marked(&app, &b, dn::tetris::target(GameAction::RetrySave)));
    key(&mut app, K::Enter);
    assert!(!app.desktop.plugins.scores_mut().has_pending());
    key(&mut app, K::Tab);
    assert_eq!(app.desktop.game().unwrap().page_focus, 0);
    key(&mut app, K::Esc);
    Ok(())
}
#[test]
fn editor_prompt_and_unsaved_guard_enter_follow_the_focused_button() -> io::Result<()> {
    use dn::editor::{self, EditorAction as A, Modal};
    let (_t, mut app) = fixture()?;
    editor::open(&mut app, true);
    editor::paste(&mut app, "dirty text");
    editor::action(&mut app, A::GoTo);
    draw(&mut app, 80, 25)?;
    shift_tab(&mut app);
    assert!(matches!(
        app.desktop.active_editor().unwrap().modals.editor_modal(),
        Some(Modal::Prompt { focus: 2, .. })
    ));
    input::paste(&mut app, "ignored");
    assert!(
        matches!(app.desktop.active_editor().unwrap().modals.editor_modal(),Some(Modal::Prompt{field,..})if field.is_empty())
    );
    key(&mut app, K::Enter);
    assert!(app
        .desktop
        .active_editor()
        .unwrap()
        .modals
        .editor_modal()
        .is_none());
    key(&mut app, K::Esc);
    assert!(matches!(
        app.desktop.active_editor().unwrap().modals.editor_modal(),
        Some(Modal::ConfirmClose { focus: 2 })
    ));
    key(&mut app, K::Tab);
    let b = draw(&mut app, 80, 25)?;
    assert!(marked(&app, &b, Target::Editor(A::Save)));
    shift_tab(&mut app);
    assert!(matches!(
        app.desktop.active_editor().unwrap().modals.editor_modal(),
        Some(Modal::ConfirmClose { focus: 2 })
    ));
    key(&mut app, K::Esc);
    assert!(app.desktop.active_editor().is_some_and(|e| e.dirty()));
    key(&mut app, K::Esc);
    shift_tab(&mut app);
    let b = draw(&mut app, 80, 25)?;
    assert!(marked(&app, &b, Target::Editor(A::Discard)));
    key(&mut app, K::Enter);
    assert!(app.desktop.active_editor().is_none());
    Ok(())
}
#[test]
fn replacement_confirmation_starts_on_cancel_then_enter_uses_selected_choice() -> io::Result<()> {
    use dn::editor::{self, search::ReplaceSession, Modal};
    let (_t, mut app) = fixture()?;
    editor::open(&mut app, true);
    editor::paste(&mut app, "a a");
    app.desktop
        .active_editor_mut()
        .unwrap()
        .modals
        .set_editor_modal(Some(Modal::Replace(ReplaceSession::new(
            vec![0..1, 2..3],
            "b".into(),
        ))));
    let b = draw(&mut app, 80, 25)?;
    assert!(marked(
        &app,
        &b,
        Target::Editor(editor::EditorAction::Cancel)
    ));
    key(&mut app, K::Esc);
    assert_eq!(
        app.desktop
            .active_editor()
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .text,
        "a a"
    );
    app.desktop
        .active_editor_mut()
        .unwrap()
        .modals
        .set_editor_modal(Some(Modal::Replace(ReplaceSession::new(
            vec![0..1, 2..3],
            "b".into(),
        ))));
    shift_tab(&mut app);
    let b = draw(&mut app, 80, 25)?;
    assert!(marked(
        &app,
        &b,
        Target::Editor(editor::EditorAction::ReplaceAll)
    ));
    key(&mut app, K::Enter);
    assert_eq!(
        app.desktop
            .active_editor()
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .text,
        "b b"
    );
    Ok(())
}
#[test]
fn pattern_and_history_skip_disabled_actions_and_cancel_without_side_effects() -> io::Result<()> {
    use dn::file_tools::{self, history::HistoryKind, ToolDialog};
    let (_t, mut app) = fixture()?;
    app.open_pattern(true);
    draw(&mut app, 80, 25)?;
    shift_tab(&mut app);
    assert_eq!(app.modals.pattern().unwrap().focus, 2);
    let b = draw(&mut app, 80, 25)?;
    assert!(marked(&app, &b, Target::CancelPattern));
    key(&mut app, K::Enter);
    assert!(app.modals.pattern().is_none());
    assert!(app.panels[0].selected.is_empty());
    app.file_tools.histories.lists[HistoryKind::Directory.index()].clear();
    file_tools::open_history(&mut app, HistoryKind::Directory);
    draw(&mut app, 80, 25)?;
    key(&mut app, K::Tab);
    assert!(matches!(
        app.file_tools(),
        Some(ToolDialog::History { focus: 2, .. })
    ));
    key(&mut app, K::Enter);
    assert!(app.file_tools().is_none());
    app.file_tools
        .histories
        .record(HistoryKind::Directory, app.panels[0].path.clone());
    file_tools::open_history(&mut app, HistoryKind::Directory);
    key(&mut app, K::Tab);
    assert!(matches!(
        app.file_tools(),
        Some(ToolDialog::History { focus: 1, .. })
    ));
    shift_tab(&mut app);
    assert!(matches!(
        app.file_tools(),
        Some(ToolDialog::History { focus: 0, .. })
    ));
    key(&mut app, K::Esc);
    Ok(())
}
#[test]
fn settings_calculator_characters_and_screensaver_share_reverse_tab_and_enter_default(
) -> io::Result<()> {
    use dn::{
        screensaver,
        utilities::{self, Kind},
    };
    let (_t, mut app) = fixture()?;
    app.modals
        .set_settings(Some(Settings::new(SettingsKind::Interface, &app.config)));
    draw(&mut app, 80, 25)?;
    shift_tab(&mut app);
    assert_eq!(
        app.modals.settings().unwrap().cursor,
        app.modals.settings().unwrap().rows().len() + 1
    );
    key(&mut app, K::Enter);
    assert!(app.modals.settings().is_none());
    utilities::open(&mut app, Kind::Calculator);
    input::paste(&mut app, "2+3");
    key(&mut app, K::Tab);
    assert_eq!(app.desktop.plugins.calculator_mut().focus, 1);
    key(&mut app, K::Enter);
    assert!(app.desktop.plugins.calculator_mut().result.is_some());
    shift_tab(&mut app);
    assert_eq!(app.desktop.plugins.calculator_mut().focus, 0);
    key(&mut app, K::Esc);
    utilities::open(&mut app, Kind::Characters);
    app.utility_state_mut().chart.select(0);
    for expected in [1, 2, 4, 0] {
        key(&mut app, K::Tab);
        assert_eq!(app.utility_state().chart.focus, expected);
    }
    shift_tab(&mut app);
    assert_eq!(app.utility_state().chart.focus, 4);
    key(&mut app, K::Enter);
    assert!(app.utility_dialog().is_none());
    app.config.saver_minutes = 0;
    screensaver::open(&mut app);
    draw(&mut app, 80, 25)?;
    shift_tab(&mut app);
    assert_eq!(
        app.screensaver.modals.screensaver().unwrap().cursor,
        screensaver::CANCEL
    );
    key(&mut app, K::Enter);
    assert!(app.screensaver.modals.screensaver().is_none());
    screensaver::open(&mut app);
    key(&mut app, K::Enter);
    assert!(app.screensaver.modals.screensaver().is_none());
    Ok(())
}
#[test]
fn archive_and_discovery_skip_unavailable_buttons_in_both_directions() -> io::Result<()> {
    let (t, mut app) = fixture()?;
    let archive = t.0.join("empty.tar");
    fs::write(&archive, vec![0; 1024])?;
    dn::archive::open(&mut app, archive);
    key(&mut app, K::Tab);
    assert_eq!(app.archive().unwrap().focus, 4);
    shift_tab(&mut app);
    assert_eq!(app.archive().unwrap().focus, 0);
    wait(&mut app, |a| a.archive().is_some_and(|b| !b.loading()));
    app.archive_mut().unwrap().focus = 4;
    key(&mut app, K::Tab);
    assert_eq!(app.archive().unwrap().focus, 6);
    shift_tab(&mut app);
    assert_eq!(app.archive().unwrap().focus, 4);
    key(&mut app, K::Esc);
    dn::discovery::open_find(&mut app);
    let original = app.discovery().unwrap().prefs.recursive;
    key(&mut app, K::Tab);
    assert_eq!(app.discovery().unwrap().focus, 5);
    key(&mut app, K::Enter);
    assert_eq!(app.discovery().unwrap().prefs.recursive, original);
    wait(&mut app, |a| a.discovery().is_some_and(|m| m.job.is_none()));
    app.discovery_mut().unwrap().hits.clear();
    app.discovery_mut().unwrap().focus = 0;
    key(&mut app, K::Tab);
    assert_eq!(app.discovery().unwrap().focus, 5);
    shift_tab(&mut app);
    assert_eq!(app.discovery().unwrap().focus, 0);
    key(&mut app, K::Esc);
    Ok(())
}
