//! Session-only welcome state. Configuration existence determines first-run display.
use crate::{app::App, settings::SettingsKind};
use crossterm::event::KeyEvent;

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
    crate::turbo::forms::welcome_key(app, key);
}
