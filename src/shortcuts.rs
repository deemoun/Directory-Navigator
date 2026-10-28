//! Terminal-safe Mac aliases. DN events remain the common command vocabulary.
use crate::{
    app::App,
    focus::{self, Focus, Screen},
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
    crate::commands::mac_key(app, c, key)
}

/// The same mapping labels menus and viewer controls. ^O means Ctrl+O, release.
pub fn label(profile: Profile, screen: Screen, classic: &str) -> Cow<'_, str> {
    crate::commands::label(profile, screen, classic)
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

pub use crate::commands::help::{EDITOR_HELP, MANAGER_HELP, USER_MENU_HELP, VIEWER_HELP};
