//! Session-only welcome state. Configuration existence determines first-run display.
use crate::{app::App, settings::SettingsKind};
use crossterm::event::{KeyCode as K, KeyEvent, KeyModifiers as M};

#[derive(Default)]
pub struct State {
    pub dialog: Option<Welcome>,
    pub(crate) startup_checked: bool,
}
#[derive(Default)]
pub struct Welcome {
    pub focus: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Start,
    Keyboard,
    Help,
}
pub const BUTTONS: [(&str, Action); 3] = [
    ("~S~tart", Action::Start),
    ("~K~eyboard settings", Action::Keyboard),
    ("~H~elp", Action::Help),
];

pub fn open(app: &mut App) {
    crate::menu::close(app);
    app.quick_search = None;
    app.shortcuts.clear();
    app.mouse.reset_click();
    app.welcome.dialog = Some(Welcome::default());
}
pub fn action(app: &mut App, action: Action) {
    if app.welcome.dialog.is_none() {
        return;
    }
    match action {
        Action::Start => app.welcome.dialog = None,
        Action::Keyboard => app.open_settings(SettingsKind::Keyboard),
        Action::Help => crate::dialog::keyboard_help(app),
    }
}
pub fn key(app: &mut App, key: KeyEvent) {
    let (width, height) = app.utilities.size;
    if width < 80 || height < 25 {
        return;
    }
    let Some(welcome) = &mut app.welcome.dialog else {
        return;
    };
    if key.modifiers.intersects(M::CONTROL | M::ALT | M::SUPER) {
        return;
    }
    let selected = match key.code {
        K::Esc => Some(Action::Start),
        K::F(1) => Some(Action::Help),
        K::Enter => Some(BUTTONS[welcome.focus].1),
        K::Tab | K::Right | K::Down => {
            welcome.focus = (welcome.focus + 1) % BUTTONS.len();
            None
        }
        K::BackTab | K::Left | K::Up => {
            welcome.focus = (welcome.focus + BUTTONS.len() - 1) % BUTTONS.len();
            None
        }
        K::Char(c) => BUTTONS
            .iter()
            .find(|(label, _)| app.config.language.matches(label, c))
            .map(|(_, action)| *action),
        _ => None,
    };
    if let Some(selected) = selected {
        action(app, selected);
    }
}
