//! Session-only welcome state. Configuration existence determines first-run display.
use crate::{app::App, settings::SettingsKind};
use crossterm::event::KeyEvent;

#[derive(Default)]
pub struct State {
    pub modals: crate::modals::ModalStack,
    pub(crate) startup_checked: bool,
}
pub struct Welcome {
    pub focus: usize,
    pub(crate) page: crate::turbo::help::Browser,
    pub(crate) links: Vec<(crate::presentation::layout::Rect, crate::mouse::Target)>,
}
impl Default for Welcome {
    fn default() -> Self {
        Self {
            focus: 0,
            page: crate::turbo::help::Browser::landing(
                " Welcome to Directory Navigator ",
                BUTTONS
                    .iter()
                    .zip(["start", "keyboard", "help"])
                    .map(|((label, _), target)| (*label, target))
                    .collect(),
            ),
            links: Vec::new(),
        }
    }
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
    app.modals.set_quick_search(None);
    app.shortcuts.clear();
    app.mouse.reset_click();
    app.welcome.modals.set_welcome(Some(Welcome::default()));
}
pub fn action(app: &mut App, action: Action) {
    if app.welcome.modals.welcome().is_none() {
        return;
    }
    match action {
        Action::Start => app.welcome.modals.set_welcome(None),
        Action::Keyboard => app.open_settings(SettingsKind::Keyboard),
        Action::Help => crate::dialog::keyboard_help(app),
    }
}
pub fn key(app: &mut App, key: KeyEvent) {
    use crossterm::event::KeyCode as K;
    if app.utilities.size.0 < 80 || app.utilities.size.1 < 25 {
        return;
    }
    if matches!(key.code, K::PageUp | K::PageDown | K::Home | K::End) {
        if let Some(welcome) = app.welcome.modals.welcome_mut() {
            welcome.page.key(key);
        }
    } else {
        crate::turbo::forms::welcome_key(app, key);
    }
}

pub(crate) fn prepare(app: &mut App, size: crate::presentation::layout::Size) {
    use crate::{focus::Screen, shortcuts};
    let language = app.config.language;
    let key = |classic| shortcuts::label(app.config.shortcuts, Screen::Manager, classic);
    let body = [
        format!("{} {}", crate::APPLICATION_NAME, crate::DISPLAY_VERSION),
        language.text("Native Rust terminal file manager").into(),
        String::new(),
        language
            .text("Arrows navigate; Tab switches panels.")
            .into(),
        language
            .text("Enter opens folders, archives or associated applications.")
            .into(),
        language
            .text(if app.config.space_select {
                "Space / Insert marks entries."
            } else {
                "Insert marks entries; Space marking is disabled."
            })
            .into(),
        format!(
            "{} {} · {} {}",
            key("F3"),
            language.text("Viewer"),
            key("F4"),
            language.text("Editor")
        ),
        String::new(),
        format!(
            "{} {} · {} {} · {} {}",
            key("F5"),
            language.text("Copy"),
            key("F6"),
            language.text("Move"),
            key("F7"),
            language.text("Make directory")
        ),
        format!(
            "{} {} · {}",
            key("F8"),
            language.text(if app.config.use_trash {
                "Trash"
            } else {
                "Delete"
            }),
            language.text(if app.config.use_trash {
                "Deletion mode: Trash"
            } else {
                "Deletion mode: permanent deletion"
            })
        ),
        language
            .text("Destinations and deletion require confirmation.")
            .into(),
        String::new(),
        language
            .text(if app.config.shortcuts.is_mac() {
                "Mac: press Ctrl+O, release, then another key."
            } else {
                "Classic keys: F1–F10 and Alt/Ctrl combinations."
            })
            .into(),
        language
            .text(if app.config.shortcuts.is_mac() {
                "Ctrl+O displays a guide; Esc cancels the prefix."
            } else {
                "F1 opens help; keyboard settings offer a Mac-friendly profile."
            })
            .into(),
    ]
    .join("\n");
    let Some(welcome) = app.welcome.modals.welcome_mut() else {
        return;
    };
    welcome.page.set_body(body);
    welcome.page.select_link(welcome.focus);
    welcome
        .page
        .prepare(size, app.config.palette, language, app.config.shortcuts);
    welcome.links = welcome
        .page
        .link_regions()
        .into_iter()
        .filter_map(|(target, rect)| {
            let action = match target.as_str() {
                "start" => Action::Start,
                "keyboard" => Action::Keyboard,
                "help" => Action::Help,
                _ => return None,
            };
            Some((rect, crate::mouse::Target::Welcome(action)))
        })
        .collect();
}
