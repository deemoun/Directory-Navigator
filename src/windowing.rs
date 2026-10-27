//! Session-only switching; content and asynchronous work remain owned by Desktop.
use crate::{
    app::App,
    desktop::{WindowId, WindowKind},
    focus::{self, Focus},
    mouse::Target,
};
use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use std::time::{Duration, Instant};

pub struct WindowList {
    pub ids: Vec<WindowId>,
    pub cursor: usize,
    pub offset: usize,
    pub rows: usize,
    pub focus: usize,
    last_click: Option<(WindowId, Instant)>,
}
impl WindowList {
    pub fn keep_visible(&mut self) {
        self.cursor = self.cursor.min(self.ids.len().saturating_sub(1));
        if self.cursor < self.offset {
            self.offset = self.cursor;
        }
        if self.cursor >= self.offset + self.rows.max(1) {
            self.offset = self.cursor + 1 - self.rows.max(1);
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Row(WindowId),
    Switch,
    Cancel,
}
fn ids(app: &App) -> Vec<WindowId> {
    let mut windows = app
        .desktop
        .windows()
        .filter(|w| w.kind != WindowKind::Game)
        .map(|w| w.id)
        .collect::<Vec<_>>();
    windows.sort();
    windows
}
pub fn open(app: &mut App) {
    if app.exit_pending
        || !matches!(
            focus::current(app),
            Focus::Manager
                | Focus::QuickSearch
                | Focus::Menu
                | Focus::Editor
                | Focus::EditorMenu
                | Focus::Viewer
        )
    {
        return;
    }
    crate::menu::close(app);
    if let Some(editor) = app.desktop.active_editor_mut() {
        editor.modals.set_editor_menu(None);
    }
    app.modals.set_quick_search(None);
    app.shortcuts.clear();
    app.mouse.reset_click();
    let ids = ids(app);
    let cursor = ids
        .iter()
        .position(|id| *id == app.desktop.active().id)
        .unwrap_or(0);
    app.modals.set_window_list(Some(WindowList {
        ids,
        cursor,
        offset: 0,
        rows: 6,
        focus: 0,
        last_click: None,
    }));
}
/// A load can fail while the list is open. Never reuse its old row target.
pub fn poll(app: &mut App) -> bool {
    if app.modals.window_list().is_none() {
        return false;
    }
    let current = ids(app);
    let Some(list) = app.modals.window_list_mut() else {
        return false;
    };
    if list.ids == current {
        return false;
    }
    let selected = list.ids.get(list.cursor).copied();
    list.cursor = selected
        .and_then(|id| current.iter().position(|i| *i == id))
        .unwrap_or(0);
    list.ids = current;
    list.last_click = None;
    list.keep_visible();
    true
}
pub fn action(app: &mut App, action: Action) {
    match action {
        Action::Cancel => {
            app.modals.set_window_list(None);
        }
        Action::Switch => {
            let selected = app
                .modals
                .window_list()
                .and_then(|l| l.ids.get(l.cursor))
                .copied();
            if let Some(id) = selected.filter(|id| app.desktop.contains(*id)) {
                app.desktop.activate(id);
                app.modals.set_window_list(None);
            } else {
                poll(app);
            }
        }
        Action::Row(id) => {
            if !app.desktop.contains(id) {
                return;
            }
            if let Some(list) = app.modals.window_list_mut() {
                if let Some(row) = list.ids.iter().position(|i| *i == id) {
                    list.cursor = row;
                    list.focus = 0;
                }
            }
        }
    }
    if let Some(list) = app.modals.window_list_mut() {
        list.keep_visible();
    }
    app.shortcuts.clear();
    app.mouse.reset_click();
}
pub fn key(app: &mut App, key: KeyEvent) {
    let Some(list) = app.modals.window_list_mut() else {
        return;
    };
    list.last_click = None;
    let navigation = match key.code {
        KeyCode::PageUp => KeyCode::Home,
        KeyCode::PageDown => KeyCode::End,
        code => code,
    };
    if let Some(row) =
        crate::turbo::widgets::list_key(list.cursor, list.ids.len(), list.rows, navigation)
    {
        list.cursor = row;
        list.focus = 0;
        list.keep_visible();
        return;
    }
    match key.code {
        KeyCode::Esc => action(app, Action::Cancel),
        KeyCode::Enter => {
            let command = if list.focus == 2 {
                Action::Cancel
            } else {
                Action::Switch
            };
            action(app, command);
        }
        KeyCode::Tab => list.focus = crate::control_focus::cycle(list.focus, 3, false),
        KeyCode::BackTab => list.focus = crate::control_focus::cycle(list.focus, 3, true),
        _ => {}
    }
    if let Some(list) = app.modals.window_list_mut() {
        list.keep_visible();
    }
}
pub fn mouse(app: &mut App, event: MouseEvent, target: Option<Target>) {
    if event.kind != MouseEventKind::Down(MouseButton::Left) {
        return;
    }
    let Some(Target::Window(command)) = target else {
        return;
    };
    let double = if let Action::Row(id) = command {
        if !app.desktop.contains(id) {
            return;
        }
        let Some(list) = app.modals.window_list_mut() else {
            return;
        };
        let now = Instant::now();
        let double = list.last_click.is_some_and(|(last, time)| {
            last == id && now.duration_since(time) < Duration::from_millis(400)
        });
        list.last_click = Some((id, now));
        double
    } else {
        false
    };
    action(app, command);
    if double {
        action(app, Action::Switch);
    }
}
