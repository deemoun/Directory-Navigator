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
    pub focus: usize,
    last_click: Option<(WindowId, Instant)>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Row(WindowId),
    Switch,
    Cancel,
}
fn ids(app: &App) -> Vec<WindowId> {
    [WindowKind::Manager, WindowKind::Editor, WindowKind::Viewer]
        .into_iter()
        .filter_map(|kind| app.desktop.id(kind))
        .collect()
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
    if let Some(editor) = app.desktop.editor_mut() {
        editor.menu = None;
    }
    app.quick_search = None;
    app.shortcuts.clear();
    app.mouse.reset_click();
    let ids = ids(app);
    let cursor = ids
        .iter()
        .position(|id| *id == app.desktop.active().id)
        .unwrap_or(0);
    app.window_list = Some(WindowList {
        ids,
        cursor,
        focus: 0,
        last_click: None,
    });
}
/// A load can fail while the list is open. Never reuse its old row target.
pub fn poll(app: &mut App) -> bool {
    if app.window_list.is_none() {
        return false;
    }
    let current = ids(app);
    let Some(list) = &mut app.window_list else {
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
    true
}
pub fn action(app: &mut App, action: Action) {
    match action {
        Action::Cancel => {
            app.window_list = None;
        }
        Action::Switch => {
            let selected = app
                .window_list
                .as_ref()
                .and_then(|l| l.ids.get(l.cursor))
                .copied();
            if let Some(id) = selected.filter(|id| app.desktop.contains(*id)) {
                app.desktop.activate(id);
                app.window_list = None;
            } else {
                poll(app);
            }
        }
        Action::Row(id) => {
            if !app.desktop.contains(id) {
                return;
            }
            if let Some(list) = &mut app.window_list {
                if let Some(row) = list.ids.iter().position(|i| *i == id) {
                    list.cursor = row;
                    list.focus = 0;
                }
            }
        }
    }
    app.shortcuts.clear();
    app.mouse.reset_click();
}
pub fn key(app: &mut App, key: KeyEvent) {
    let Some(list) = &mut app.window_list else {
        return;
    };
    list.last_click = None;
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
        KeyCode::Tab => list.focus = (list.focus + 1) % 3,
        KeyCode::BackTab => list.focus = (list.focus + 2) % 3,
        KeyCode::Up => {
            list.cursor = list.cursor.saturating_sub(1);
            list.focus = 0;
        }
        KeyCode::Down => {
            list.cursor = (list.cursor + 1).min(list.ids.len().saturating_sub(1));
            list.focus = 0;
        }
        KeyCode::Home | KeyCode::PageUp => {
            list.cursor = 0;
            list.focus = 0;
        }
        KeyCode::End | KeyCode::PageDown => {
            list.cursor = list.ids.len().saturating_sub(1);
            list.focus = 0;
        }
        _ => {}
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
        let Some(list) = &mut app.window_list else {
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
