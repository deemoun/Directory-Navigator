use crate::{app::App, dialog, filesystem::Entry, panel::Panel};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::{ffi::OsString, path::PathBuf};

pub struct QuickSearch {
    pub panel: usize,
    pub path: PathBuf,
    pub query: String,
    pub no_match: bool,
    original: Option<OsString>,
    original_offset: usize,
}
impl QuickSearch {
    pub fn new(panel: usize, listing: &Panel) -> Self {
        Self {
            panel,
            path: listing.path.clone(),
            query: String::new(),
            no_match: false,
            original: listing.entries.get(listing.cursor).map(|e| e.name.clone()),
            original_offset: listing.offset,
        }
    }
    fn matches(entry: &Entry, prefix: &str) -> bool {
        !entry.parent
            && entry
                .name
                .to_str()
                .is_some_and(|name| name.to_lowercase().starts_with(prefix))
    }
    pub fn update(&mut self, panel: &mut Panel) {
        if self.query.is_empty() {
            self.no_match = false;
            return;
        }
        let prefix = self.query.to_lowercase();
        let found = panel
            .entries
            .iter()
            .position(|entry| Self::matches(entry, &prefix));
        self.no_match = found.is_none();
        if let Some(index) = found {
            panel.cursor = index;
        }
    }
    fn cycle(&mut self, panel: &mut Panel, forward: bool) {
        if self.query.is_empty() {
            return;
        }
        let prefix = self.query.to_lowercase();
        let len = panel.entries.len();
        for distance in 1..=len {
            let index = if forward {
                (panel.cursor + distance) % len
            } else {
                (panel.cursor + len - distance) % len
            };
            if Self::matches(&panel.entries[index], &prefix) {
                panel.cursor = index;
                self.no_match = false;
                return;
            }
        }
        self.no_match = true;
    }
    pub fn paste(&mut self, panel: &mut Panel, text: &str) {
        let mut cursor = self.query.len();
        dialog::paste_input(&mut self.query, &mut cursor, text);
        self.update(panel);
    }
}

pub fn open(app: &mut App) {
    if !app.panels[app.active].loading
        && !app.blocking_operation()
        && app.dialogs().is_empty()
        && crate::focus::screen(app) == crate::focus::Screen::Manager
        && app.modals.prompt().is_none()
            & app.modals.settings().is_none()
            & app.modals.columns_dialog().is_none()
            & app.modals.pattern().is_none()
    {
        app.modals
            .set_quick_search(Some(QuickSearch::new(app.active, &app.panels[app.active])));
    }
}

/// Return false to hand a non-search command back to the normal input handler.
pub fn key(app: &mut App, key: KeyEvent) -> bool {
    let Some(mut search) = app.modals.take_quick_search() else {
        return false;
    };
    let panel = &mut app.panels[search.panel];
    if panel.loading || panel.path != search.path || search.panel != app.active {
        return false;
    }
    match key.code {
        KeyCode::Esc => {
            if let Some(index) = search
                .original
                .as_ref()
                .and_then(|name| panel.entries.iter().position(|e| &e.name == name))
            {
                panel.cursor = index;
                panel.offset = search.original_offset;
            }
            return true;
        }
        KeyCode::Enter => return true,
        KeyCode::Up if key.modifiers.is_empty() => search.cycle(panel, false),
        KeyCode::Down if key.modifiers.is_empty() => search.cycle(panel, true),
        KeyCode::Char('u') if key.modifiers == KeyModifiers::CONTROL => {
            search.query.clear();
            search.update(panel);
        }
        KeyCode::Char(_) | KeyCode::Backspace
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            let mut cursor = search.query.len();
            dialog::edit_input(&mut search.query, &mut cursor, key.code);
            search.update(panel);
        }
        _ => return false,
    }
    app.modals.set_quick_search(Some(search));
    true
}
