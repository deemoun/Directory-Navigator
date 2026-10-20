use super::{Game, Screen};
use crate::{app::App, menu};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameAction {
    Help,
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
        app.tetris = Some(Game {
            records: app.game_records.clone(),
            ..Game::default()
        });
    }
}

pub fn action(app: &mut App, action: GameAction) {
    if action == GameAction::Close {
        if let Some(game) = app.tetris.take() {
            app.game_records = game.records;
        }
        return;
    }
    let Some(game) = &mut app.tetris else {
        return;
    };
    let now = Instant::now();
    match action {
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
