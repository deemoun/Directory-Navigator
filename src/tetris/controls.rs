use super::{Game, Screen};
use crate::{app::App, menu};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameAction {
    Help,
    Continue,
    SubmitName,
    SkipName,
    RetrySave,
    New,
    Pause,
    Scores,
    Setup,
    LevelUp,
    LevelDown,
    Preview,
    Apply,
    Back,
    Close,
}

pub fn open(app: &mut App) {
    if app.job.is_some()
        || !app.dialogs.is_empty()
        || app.viewer.is_some()
        || app.settings.is_some()
        || app.prompt.is_some()
        || app.pattern.is_some()
        || app.columns_dialog.is_some()
    {
        return;
    }
    menu::close(app);
    app.quick_search = None;
    if app.tetris.is_none() {
        let path = app
            .config_store
            .path()
            .ok()
            .map(|p| p.with_file_name("tetris.conf"));
        let error = app.game_scores.load(path).err();
        app.tetris = Some(Game {
            records: app.game_scores.board.records.clone(),
            name: app.game_scores.board.last_name.clone(),
            name_cursor: app.game_scores.board.last_name.len(),
            notice: error.map_or_else(
                || {
                    if app.game_scores.has_pending() {
                        "Scores not saved. Retry save.".into()
                    } else {
                        String::new()
                    }
                },
                |e| format!("Cannot load scores: {e}"),
            ),
            ..Game::default()
        });
    }
}

pub fn action(app: &mut App, action: GameAction) {
    if action == GameAction::Close {
        app.tetris = None;
        return;
    }
    if action == GameAction::RetrySave {
        persist(app);
        return;
    }
    if matches!(action, GameAction::SubmitName | GameAction::SkipName) {
        submit(app, action == GameAction::SkipName);
        return;
    }
    let Some(game) = &mut app.tetris else {
        return;
    };
    let now = Instant::now();
    if game.screen == Screen::Name {
        return;
    }
    if game.screen == Screen::GameOver && action != GameAction::Continue {
        return;
    }
    match action {
        GameAction::Continue => {
            if qualifies(game) {
                game.screen = Screen::Name;
                game.replace_name = true;
                game.name_cursor = game.name.len();
            } else {
                game.screen = Screen::Scores;
            }
        }
        GameAction::Help => game.screen = Screen::Help,
        GameAction::Scores => game.screen = Screen::Scores,
        GameAction::Setup => {
            game.screen = Screen::Setup {
                level: game.start_level,
                preview: game.preview,
            }
        }
        GameAction::Back => game.screen = Screen::Play,
        GameAction::New => game.restart(now),
        GameAction::Pause if game.screen == Screen::Play && !game.over => {
            game.paused = !game.paused
        }
        GameAction::LevelUp => {
            if let Screen::Setup { level, .. } = &mut game.screen {
                *level = (*level + 1).min(10);
            } else if game.screen == Screen::Play {
                game.level = (game.level + 1).min(10);
            }
        }
        GameAction::LevelDown => {
            if let Screen::Setup { level, .. } = &mut game.screen {
                *level = level.saturating_sub(1).max(1);
            }
        }
        GameAction::Preview => {
            if let Screen::Setup { preview, .. } = &mut game.screen {
                *preview = !*preview;
            } else if game.screen == Screen::Play {
                game.preview = !game.preview;
            }
        }
        GameAction::Apply => {
            if let Screen::Setup { level, preview } = game.screen {
                game.start_level = level;
                game.preview = preview;
                game.restart(now);
            }
        }
        _ => {}
    }
    game.resume_clock(now);
}

pub fn key(app: &mut App, key: KeyEvent) {
    let screen = app.tetris.as_ref().map(|game| game.screen);
    if screen == Some(Screen::Name) {
        match key.code {
            KeyCode::Enter => action(app, GameAction::SubmitName),
            KeyCode::Esc => action(app, GameAction::SkipName),
            _ => edit_name(app, key),
        }
        return;
    }
    if screen == Some(Screen::GameOver) {
        if matches!(key.code, KeyCode::Enter | KeyCode::Esc) {
            action(app, GameAction::Continue);
        }
        return;
    }
    if key.code == KeyCode::Esc {
        action(
            app,
            if screen == Some(Screen::Play) {
                GameAction::Close
            } else {
                GameAction::Back
            },
        );
        return;
    }
    if matches!(screen, Some(Screen::Setup { .. })) {
        match key.code {
            KeyCode::Up | KeyCode::Right | KeyCode::Char('+') => action(app, GameAction::LevelUp),
            KeyCode::Down | KeyCode::Left | KeyCode::Char('-') => {
                action(app, GameAction::LevelDown)
            }
            KeyCode::Char(' ') | KeyCode::Char('*') => action(app, GameAction::Preview),
            KeyCode::Enter => action(app, GameAction::Apply),
            _ => {}
        }
        return;
    }
    if screen != Some(Screen::Play) {
        if key.code == KeyCode::Enter {
            action(app, GameAction::Back);
        }
        return;
    }
    let command = if key.modifiers.contains(KeyModifiers::ALT) {
        match key.code {
            KeyCode::Char('n') => Some(GameAction::New),
            KeyCode::Char('p') => Some(GameAction::Pause),
            KeyCode::Char('t') => Some(GameAction::Scores),
            KeyCode::Char('s') => Some(GameAction::Setup),
            _ => None,
        }
    } else {
        match key.code {
            KeyCode::F(1) => Some(GameAction::Help),
            KeyCode::F(2) => Some(GameAction::New),
            KeyCode::F(3) => Some(GameAction::Pause),
            KeyCode::F(4) => Some(GameAction::Scores),
            KeyCode::F(5) => Some(GameAction::Setup),
            KeyCode::Char('+') => Some(GameAction::LevelUp),
            KeyCode::Char('*') => Some(GameAction::Preview),
            _ => None,
        }
    };
    if let Some(command) = command {
        action(app, command);
        return;
    }
    if key
        .modifiers
        .intersects(KeyModifiers::ALT | KeyModifiers::CONTROL)
    {
        return;
    }
    if let Some(game) = &mut app.tetris {
        match key.code {
            KeyCode::Left | KeyCode::Home => {
                game.translate(-1, 0);
            }
            KeyCode::Right | KeyCode::PageUp => {
                game.translate(1, 0);
            }
            KeyCode::Up => game.rotate(),
            KeyCode::Down | KeyCode::Char(' ') => game.drop(Instant::now()),
            _ => {}
        }
    }
}

fn qualifies(game: &Game) -> bool {
    game.score > 0
        && (game.records.len() < 10 || game.records.last().is_some_and(|r| game.score > r.score))
}
fn persist(app: &mut App) {
    let result = app.game_scores.save();
    if let Some(g) = &mut app.tetris {
        g.records = app.game_scores.board.records.clone();
        if g.submitted {
            g.highlighted = g
                .records
                .iter()
                .rposition(|r| r.score == g.score && r.name == g.name && r.lines == g.lines);
        }
        if result.is_ok() {
            g.notice = "Scores saved".into();
        } else if let Err(e) = result {
            g.notice = format!("Scores not saved: {e}. Retry save.");
        }
    }
}
fn submit(app: &mut App, anonymous: bool) {
    let Some(g) = &mut app.tetris else {
        return;
    };
    if g.screen != Screen::Name || g.submitted {
        return;
    }
    let name = if anonymous || g.name.trim().is_empty() {
        super::scores::ANONYMOUS.to_owned()
    } else {
        g.name.trim().to_owned()
    };
    if !super::scores::validate_name(&name) {
        g.notice = "Name must fit 30 characters / 240 bytes, without controls".into();
        return;
    }
    let record = super::Record {
        name: name.clone(),
        score: g.score,
        lines: g.lines,
        start_level: g.start_level,
        level: g.level,
    };
    if let Err(e) = app.game_scores.add(record) {
        g.notice = e.to_string();
        return;
    }
    g.name = name.clone();
    g.submitted = true;
    g.highlighted = Some(
        app.game_scores
            .board
            .records
            .iter()
            .rposition(|r| r.score == g.score && r.name == name)
            .unwrap_or(0),
    );
    g.screen = Screen::Scores;
    persist(app);
}
pub fn paste(app: &mut App, text: &str) {
    if text.is_empty() {
        return;
    }
    let Some(g) = &mut app.tetris else {
        return;
    };
    if g.screen != Screen::Name {
        return;
    }
    let mut value = if g.replace_name {
        String::new()
    } else {
        g.name.clone()
    };
    let cursor = if g.replace_name { 0 } else { g.name_cursor };
    value.insert_str(cursor, text);
    if !super::scores::validate_name(&value) {
        g.notice = "Name must fit 30 characters / 240 bytes, without controls".into();
        return;
    }
    g.name = value;
    g.name_cursor = cursor + text.len();
    use unicode_segmentation::UnicodeSegmentation;
    g.name_cursor = g
        .name
        .grapheme_indices(true)
        .map(|(i, _)| i)
        .find(|i| *i >= g.name_cursor)
        .unwrap_or(g.name.len());
    g.replace_name = false;
    g.name_cursor = g
        .name
        .grapheme_indices(true)
        .map(|(i, _)| i)
        .find(|i| *i >= g.name_cursor)
        .unwrap_or(g.name.len());
    if g.notice.starts_with("Name must fit") {
        g.notice.clear();
    }
}
fn edit_name(app: &mut App, key: KeyEvent) {
    use unicode_segmentation::UnicodeSegmentation;
    if key
        .modifiers
        .intersects(KeyModifiers::ALT | KeyModifiers::CONTROL)
    {
        return;
    }
    if let KeyCode::Char(c) = key.code {
        paste(app, &c.to_string());
        return;
    }
    if !matches!(
        key.code,
        KeyCode::Left
            | KeyCode::Right
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::Backspace
            | KeyCode::Delete
    ) {
        return;
    }
    let Some(g) = &mut app.tetris else {
        return;
    };
    let previous = g.name[..g.name_cursor]
        .grapheme_indices(true)
        .next_back()
        .map_or(0, |(i, _)| i);
    let next = g.name[g.name_cursor..]
        .grapheme_indices(true)
        .nth(1)
        .map_or(g.name.len(), |(i, _)| g.name_cursor + i);
    match key.code {
        KeyCode::Left => g.name_cursor = previous,
        KeyCode::Right => g.name_cursor = next,
        KeyCode::Home => g.name_cursor = 0,
        KeyCode::End => g.name_cursor = g.name.len(),
        KeyCode::Backspace | KeyCode::Delete if g.replace_name => {
            g.name.clear();
            g.name_cursor = 0;
        }
        KeyCode::Backspace => {
            g.name.replace_range(previous..g.name_cursor, "");
            g.name_cursor = previous;
        }
        KeyCode::Delete => {
            g.name.replace_range(g.name_cursor..next, "");
        }
        _ => {}
    }
    g.replace_name = false;
    g.name_cursor = g
        .name
        .grapheme_indices(true)
        .map(|(i, _)| i)
        .find(|i| *i >= g.name_cursor)
        .unwrap_or(g.name.len());
    if g.notice.starts_with("Name must fit") {
        g.notice.clear();
    }
}
