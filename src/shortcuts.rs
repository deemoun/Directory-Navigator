//! Terminal-safe Mac aliases. DN events remain the common command vocabulary.
use crate::{
    app::App,
    focus::{self, Focus, Screen},
    menu::Action,
};
use crossterm::event::{KeyCode as K, KeyEvent, KeyEventKind, KeyModifiers as M};
use std::borrow::Cow;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Profile {
    Auto,
    #[default]
    ClassicDn,
    Mac,
}
impl Profile {
    pub fn is_mac(self) -> bool {
        self == Self::Mac || self == Self::Auto && cfg!(target_os = "macos")
    }
    pub fn key(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::ClassicDn => "classic_dn",
            Self::Mac => "mac",
        }
    }
    pub fn parse(value: &str) -> Self {
        match value {
            "auto" => Self::Auto,
            "classic_dn" => Self::ClassicDn,
            "mac" => Self::Mac,
            _ => Self::default(),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Auto if cfg!(target_os = "macos") => "Automatic (Mac-friendly)",
            Self::Auto => "Automatic (Classic DN)",
            Self::ClassicDn => "Classic DN",
            Self::Mac => "Mac-friendly",
        }
    }
    pub fn next(self, reverse: bool) -> Self {
        match (self, reverse) {
            (Self::Auto, false) | (Self::Mac, true) => Self::ClassicDn,
            (Self::ClassicDn, false) | (Self::Auto, true) => Self::Mac,
            _ => Self::Auto,
        }
    }
}
#[derive(Default)]
pub struct State {
    armed: Option<Focus>,
    owner: Option<crate::desktop::WindowId>,
}
impl State {
    pub fn clear(&mut self) {
        self.armed = None;
        self.owner = None;
    }
}
fn eligible(app: &App) -> bool {
    matches!(
        focus::current(app),
        Focus::Manager
            | Focus::QuickSearch
            | Focus::Menu
            | Focus::Viewer
            | Focus::Editor
            | Focus::EditorMenu
            | Focus::UserMenu
    ) && !app.screensaver.active
}
pub fn armed(app: &App) -> bool {
    app.config.shortcuts.is_mac()
        && eligible(app)
        && app.shortcuts.owner == Some(app.desktop.active().id)
        && app.shortcuts.armed == Some(focus::current(app))
}
pub fn key(app: &mut App, key: KeyEvent) -> Option<KeyEvent> {
    // Command normally belongs to the terminal. If a protocol forwards it,
    // never turn an unhandled Command chord into document/command text.
    if app.config.shortcuts.is_mac() && key.modifiers.contains(M::SUPER) {
        app.shortcuts.clear();
        return None;
    }
    if !app.config.shortcuts.is_mac() || !eligible(app) {
        app.shortcuts.clear();
        return Some(key);
    }
    let was_armed = armed(app);
    if !was_armed {
        app.shortcuts.clear();
    }
    if key.code == K::Char('o') && key.modifiers == M::CONTROL {
        if key.kind == KeyEventKind::Press
            && was_armed
            && matches!(
                focus::current(app),
                Focus::Manager | Focus::Menu | Focus::QuickSearch
            )
        {
            crate::terminal_output::request(app);
            return None;
        }
        if key.kind != KeyEventKind::Repeat {
            app.shortcuts.owner = Some(app.desktop.active().id);
            app.shortcuts.armed = if was_armed {
                None
            } else {
                Some(focus::current(app))
            };
        }
        return None;
    }
    if !was_armed {
        return Some(key);
    }
    app.shortcuts.clear();
    if key.code == K::Esc {
        return None;
    }
    // Real accelerators still work while the prefix guide is displayed.
    if key.modifiers.intersects(M::CONTROL | M::ALT | M::SUPER)
        || matches!(key.code, K::F(_) | K::Tab | K::BackTab)
    {
        return Some(key);
    }
    let mapped = |code, modifiers| Some(KeyEvent::new(code, modifiers));
    if let K::Char(c @ '0'..='9') = key.code {
        return mapped(K::F(if c == '0' { 10 } else { c as u8 - b'0' }), M::NONE);
    }
    if focus::current(app) == Focus::UserMenu {
        return match key.code {
            K::Char('w' | 'W') => mapped(K::Esc, M::NONE),
            K::Char('r' | 'R') => mapped(K::Char('r'), M::CONTROL),
            K::Up | K::Char('[') => mapped(K::PageUp, M::NONE),
            K::Down | K::Char(']') => mapped(K::PageDown, M::NONE),
            K::Left | K::Char(',') => mapped(K::Home, M::NONE),
            K::Right | K::Char('.') => mapped(K::End, M::NONE),
            _ => None,
        };
    }
    if focus::screen(app) == Screen::Manager
        && matches!(
            key.code,
            K::Char('[' | ']' | ',' | '.' | ' ') | K::Up | K::Down | K::Left | K::Right
        )
    {
        crate::menu::close(app);
        app.modals.set_quick_search(None);
        let brief = app.config.layouts[app.active] == crate::panel_layout::PanelLayout::Brief;
        let panel = &mut app.panels[app.active];
        let capacity = panel.viewport_rows * if brief { 3 } else { 1 };
        match key.code {
            K::Char('[') | K::Up => panel.step(-(capacity as isize)),
            K::Char(']') | K::Down => panel.step(capacity as isize),
            K::Char(',') | K::Left => panel.cursor = 0,
            K::Char('.') | K::Right => panel.cursor = panel.entries.len().saturating_sub(1),
            K::Char(' ') => {
                panel.toggle_selection();
                panel.step(1);
            }
            _ => {}
        }
        return None;
    }
    match key.code {
        K::Char('[') | K::Up => return mapped(K::PageUp, M::NONE),
        K::Char(']') | K::Down => return mapped(K::PageDown, M::NONE),
        K::Char(',') => return mapped(K::Home, M::NONE),
        K::Char('.') => return mapped(K::End, M::NONE),
        _ => {}
    }
    let c = match key.code {
        K::Char(c) => c.to_ascii_lowercase(),
        _ => '\0',
    };
    if c == 'u'
        && matches!(
            focus::current(app),
            Focus::Manager
                | Focus::QuickSearch
                | Focus::Menu
                | Focus::Editor
                | Focus::EditorMenu
                | Focus::Viewer
        )
    {
        return mapped(K::Char('0'), M::ALT);
    }
    match focus::screen(app) {
        Screen::Manager => {
            let action = match c {
                'f' => Some(Action::Find),
                's' => Some(Action::QuickSearch),
                'h' => Some(Action::CommandHistory),
                'i' => Some(Action::FileInfo),
                'b' => Some(Action::Bookmarks),
                'l' => Some(Action::FilenameFilter),
                'g' => Some(Action::GoTo),
                't' => Some(Action::DirectoryTree),
                'q' => Some(Action::QuickView),
                'r' => Some(Action::Refresh),
                'e' => Some(Action::Columns),
                'p' => Some(Action::ExportList),
                'o' => Some(Action::OpenExternal),
                'y' => Some(Action::Game),
                'j' => Some(Action::History(
                    crate::file_tools::history::HistoryKind::Directory,
                )),
                'a' => Some(Action::History(
                    crate::file_tools::history::HistoryKind::Edit,
                )),
                'v' => Some(Action::History(
                    crate::file_tools::history::HistoryKind::View,
                )),
                'k' => Some(Action::Settings(crate::settings::SettingsKind::Keyboard)),
                _ => None,
            };
            if let Some(action) = action {
                crate::menu::execute(app, action);
                return None;
            }
            match c {
                'm' => mapped(K::F(9), M::NONE),
                'x' => mapped(K::F(10), M::NONE),
                'z' => {
                    crate::menu::open(app, 3);
                    if let Some(row) = crate::menu::items(3)
                        .iter()
                        .position(|i| i.action == Action::Submenu(crate::menu::MenuId::Sort))
                    {
                        crate::menu::activate(app, 0, row);
                    }
                    None
                }
                _ => None,
            }
        }
        Screen::Editor => match (c, key.code) {
            ('a', _) => mapped(K::F(2), M::SHIFT),
            ('o', _) => mapped(K::F(3), M::NONE),
            ('w', _) => mapped(K::F(4), M::CONTROL),
            ('f', _) => mapped(K::Char('f'), M::CONTROL),
            ('n', _) => mapped(K::F(7), M::SHIFT),
            ('p', _) => mapped(K::F(7), M::ALT),
            ('r', _) => mapped(K::F(7), M::CONTROL),
            ('g', _) => mapped(K::Char('g'), M::CONTROL),
            ('d', _) => mapped(K::F(6), M::NONE),
            ('s', _) => mapped(K::Char('s'), M::CONTROL),
            ('c', _) => mapped(K::Delete, M::CONTROL),
            ('h', _) => mapped(K::Home, M::CONTROL),
            ('e', _) => mapped(K::End, M::CONTROL),
            ('m', _) => mapped(K::F(10), M::NONE),
            ('x', _) => mapped(K::Char('x'), M::ALT),
            (_, K::Left | K::Right) => mapped(key.code, M::CONTROL | (key.modifiers & M::SHIFT)),
            (_, K::Char('<')) => mapped(K::Home, M::CONTROL),
            (_, K::Char('>')) => mapped(K::End, M::CONTROL),
            _ => None,
        },
        Screen::Viewer => match c {
            'w' => mapped(K::Esc, M::NONE),
            'f' => mapped(K::F(7), M::NONE),
            'n' => mapped(K::F(7), M::SHIFT),
            'p' => mapped(K::F(7), M::CONTROL),
            'x' => mapped(K::F(10), M::NONE),
            _ => match key.code {
                K::Left => mapped(K::Home, M::NONE),
                K::Right => mapped(K::End, M::NONE),
                _ => None,
            },
        },
        Screen::Game => None,
    }
}

/// The same mapping labels menus and viewer controls. ^O means Ctrl+O, release.
pub fn label(profile: Profile, screen: Screen, classic: &str) -> Cow<'_, str> {
    if !profile.is_mac() {
        return Cow::Borrowed(classic);
    }
    if classic == "Alt+0" {
        return Cow::Borrowed("^Ou");
    }
    if let Some(n) = classic
        .strip_prefix('F')
        .and_then(|s| s.parse::<u8>().ok())
        .filter(|n| (1..=10).contains(n))
    {
        return Cow::Owned(format!("^O{}", n % 10));
    }
    let value = match screen {
        Screen::Manager => match classic {
            "Ctrl+O" => "^O^O",
            "Alt+F7" => "^Os",
            "Ctrl+F7" => "^Of",
            "Alt+F8" => "^Oh",
            "Alt+E" => "^Oi",
            "Alt+K" => "^Oe",
            "Alt+Del" => "^Ol",
            "Alt+L" => "^Op",
            "Ctrl+Enter" => "^Oo",
            "Alt+F9" => "^Oy",
            "Alt+X" => "^O0",
            "Alt+Backspace" => "^Oj",
            "Alt+PageUp" => "^Oa",
            "Alt+PageDown" => "^Ov",
            "Ctrl+F3" => "^Oz N",
            "Ctrl+F4" => "^Oz E",
            "Ctrl+F5" => "^Oz D",
            "Ctrl+F6" => "^Oz S",
            _ => classic,
        },
        Screen::Editor => match classic {
            "Shift+F2" => "^Oa",
            "Ctrl+F4" => "^Ow",
            "Alt+X" => "^Ox",
            "Shift+F7" => "^On",
            "Alt+F7" => "^Op",
            "Ctrl+F7" => "^Or",
            "Alt+G" => "Ctrl+G",
            "Alt+Backspace" => "Ctrl+Z",
            "Ctrl+Ins" => "Ctrl+C",
            "Shift+Del" => "Ctrl+X",
            "Shift+Ins" => "Ctrl+V",
            "Ctrl+Del" => "^Oc",
            _ => classic,
        },
        Screen::Viewer => match classic {
            "S-F7" | "Shift+F7" => "^On",
            "C-F7" | "Ctrl+F7" => "^Op",
            _ => classic,
        },
        Screen::Game => classic,
    };
    Cow::Borrowed(value)
}

pub fn function(
    profile: Profile,
    palette: crate::palette::PaletteProfile,
    number: u8,
    label: &str,
    enabled: bool,
) -> crate::presentation::text::Line<'static> {
    if profile.is_mac() {
        palette.function_key(
            format!("^O{} ", number % 10),
            if label == "MkDir" { "Dir" } else { label },
            enabled,
        )
    } else {
        palette.function(number, label, enabled)
    }
}

pub const MANAGER_HELP: &str="Mac-friendly: press Ctrl+O, release, then choose\n1 Help · 2 User · 3 View · 4 Edit · 5 Copy\n6 Move · 7 MkDir · 8 Trash/delete · 9 Menu · 0 Quit\ns Quick search · f Recursive Find · i Info · z Sort\nb Bookmarks · l Filter · e Columns · r Refresh\nh Commands · j Directories · a Edits · v Views\no OS default app · p List file · y Tetris\nk Keyboard settings · u Windows · m Menu\nUp/Down Page · Left/Right First/last entry\nCtrl+A all · Ctrl+U clear · Ctrl+O then Space marks\nCtrl+O twice: terminal output; Esc returns\nEsc or click cancels the prefix; nothing is typed.\nClassic DN keys remain available.\nCommand shortcuts belong to your terminal.\nTrash/delete still requires confirmation.";
pub const EDITOR_HELP: &str="Mac-friendly: Ctrl+O, release, then digit or letter\n1 Help · 2 Save · 3 Open · 6 Duplicate · 7 Find · 0 Menu\na Save As · w Close · x Quit DN · m Menu · u Windows\nn Next match · p Reverse · r Replace · g Go to line\nCtrl+S Save · Ctrl+F Find · Ctrl+G Go to line\nCtrl+Z/Y Undo/Redo · Ctrl+C/X/V internal clipboard\nCtrl+O then Left/Right: word movement\nAdd Shift to the arrow to extend selection\nUp/Down Page · , / . Line start/end\nh / e Document start/end · c Clear selection\nEsc or click cancels the prefix. Classic keys still work.\nOpen/Close/Quit protect unsaved changes.\nUTF-8, 2 MiB / 100k lines. Line endings preserved.\nSave As never overwrites an existing destination.";
pub const VIEWER_HELP: &str="Mac-friendly: press Ctrl+O, release, then choose\n2 Wrap · 3 Close · 7 Find · 0 Quit DN\nf Find · n Next match · p Reverse · w Close · u Windows\nUp/Down Page · Left/Right First/last row\nEsc or click cancels the prefix.\nClassic DN keys remain available.";
pub const USER_MENU_HELP: &str="Ctrl+O: choose a user-menu command\n1 Help · 2 Global/Local · 4 Edit · other digits: F-keys\nr Reload · w Back/Cancel · Up/Down Page\nLeft/Right First/last item\nEsc or click cancels the prefix.\nCommands still require explicit Run confirmation.";
