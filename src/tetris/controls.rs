use super::GameMode;
use crate::app::App;
use crossterm::event::KeyEvent;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameAction {
    Help,
    Continue,
    SubmitName,
    SkipName,
    NameField,
    BoardFocus,
    RetrySave,
    New,
    Pause,
    Scores,
    Setup,
    LevelUp,
    LevelDown,
    SetLevel(u8),
    SetMode(GameMode),
    Preview,
    Apply,
    Back,
    Close,
}

impl GameAction {
    pub(crate) fn id(self) -> usize {
        match self {
            Self::Help => 0,
            Self::Continue => 1,
            Self::SubmitName => 2,
            Self::SkipName => 3,
            Self::NameField => 4,
            Self::BoardFocus => 5,
            Self::RetrySave => 6,
            Self::New => 7,
            Self::Pause => 8,
            Self::Scores => 9,
            Self::Setup => 10,
            Self::LevelUp => 11,
            Self::LevelDown => 12,
            Self::Preview => 13,
            Self::Apply => 14,
            Self::Back => 15,
            Self::Close => 16,
            Self::SetMode(GameMode::Tetris) => 200,
            Self::SetMode(GameMode::Pentix) => 201,
            Self::SetLevel(v) => 100 + usize::from(v),
        }
    }
    pub(crate) fn from_id(id: usize) -> Option<Self> {
        Some(match id {
            0 => Self::Help,
            1 => Self::Continue,
            2 => Self::SubmitName,
            3 => Self::SkipName,
            4 => Self::NameField,
            5 => Self::BoardFocus,
            6 => Self::RetrySave,
            7 => Self::New,
            8 => Self::Pause,
            9 => Self::Scores,
            10 => Self::Setup,
            11 => Self::LevelUp,
            12 => Self::LevelDown,
            13 => Self::Preview,
            14 => Self::Apply,
            15 => Self::Back,
            16 => Self::Close,
            200 => Self::SetMode(GameMode::Tetris),
            201 => Self::SetMode(GameMode::Pentix),
            101..=110 => Self::SetLevel((id - 100) as u8),
            _ => return None,
        })
    }
}

pub fn target(a: GameAction) -> crate::mouse::Target {
    crate::mouse::Target::Plugin(crate::plugins::TETRIS, crate::plugins::ActionId(a.id()))
}
pub fn open(app: &mut App) {
    crate::plugins::host::open(app, crate::plugins::TETRIS);
}
pub fn key(app: &mut App, key: KeyEvent) {
    crate::plugins::host::key(app, key);
}
pub fn paste(app: &mut App, text: &str) {
    crate::plugins::host::paste(app, text);
}
pub fn action(app: &mut App, a: GameAction) {
    if let Some(b) = app.desktop.plugins.binding(crate::plugins::TETRIS) {
        crate::plugins::host::dispatch(
            app,
            b,
            crate::plugins::Event::Action(crate::plugins::ActionId(a.id())),
        );
    }
}
