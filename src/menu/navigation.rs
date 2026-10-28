use super::{level_items, selected_row, submenu_items, Action};
use crate::app::App;
use crossterm::event::KeyCode;

pub fn close(app: &mut App) {
    app.modals.set_menu(None);
}
pub fn open(app: &mut App, root: usize) {
    if super::LABELS.is_empty() {
        return;
    }
    let root = root % super::LABELS.len();
    app.modals.set_menu(Some((root, 0)));
    app.modals.submenus_mut().clear();
    // Select from the visible list, including registered/disabled plugins.
    let first = level_items(app, 0)
        .iter()
        .position(|item| item.selectable())
        .unwrap_or(0);
    if let Some((_, row)) = app.modals.menu_mut() {
        *row = first;
    }
}
pub fn open_root(app: &mut App, root: super::RootId) -> bool {
    let Some(index) = super::root_index(root) else {
        return false;
    };
    open(app, index);
    true
}
pub fn open_submenu(app: &mut App, root: super::RootId, submenu: super::MenuId) {
    if open_root(app, root) {
        if let Some(row) = level_items(app, 0)
            .iter()
            .position(|i| i.action == Action::Submenu(submenu))
        {
            activate(app, 0, row);
        }
    }
}
pub fn open_mnemonic(app: &mut App, character: char) -> bool {
    if let Some(index) = super::LABELS
        .iter()
        .position(|label| app.config.language.matches(label, character))
    {
        open(app, index);
        true
    } else {
        false
    }
}
fn select(app: &mut App, depth: usize, row: usize) {
    app.modals.submenus_mut().truncate(depth);
    if depth == 0 {
        if let Some((_, selected)) = app.modals.menu_mut() {
            *selected = row;
        }
    } else if let Some((_, selected)) = app.modals.submenus_mut().get_mut(depth - 1) {
        *selected = row;
    }
}
pub fn activate(app: &mut App, depth: usize, row: usize) {
    let entries = level_items(app, depth);
    let Some(item) = entries.get(row).filter(|item| item.selectable()) else {
        return;
    };
    select(app, depth, row);
    match item.action {
        Action::Submenu(id) => {
            let first = submenu_items(id)
                .iter()
                .position(|item| item.selectable())
                .unwrap_or(0);
            app.modals.submenus_mut().push((id, first));
        }
        action => super::execute(app, action),
    }
}
pub fn key(app: &mut App, code: KeyCode) {
    let Some((root, _)) = app.modals.menu() else {
        return;
    };
    let depth = app.modals.submenus().len();
    let entries = level_items(app, depth);
    let row = selected_row(app, depth);
    match code {
        KeyCode::F(1) => super::execute(app, Action::Key(1)),
        KeyCode::F(9) => close(app),
        KeyCode::Esc | KeyCode::Left if depth > 0 => {
            app.modals.submenus_mut().pop();
        }
        KeyCode::Esc => close(app),
        KeyCode::Left | KeyCode::BackTab => open(
            app,
            crate::control_focus::cycle(root, super::LABELS.len(), true),
        ),
        KeyCode::Tab => open(
            app,
            crate::control_focus::cycle(root, super::LABELS.len(), false),
        ),
        KeyCode::Right => {
            if entries
                .get(row)
                .is_some_and(|item| matches!(item.action, Action::Submenu(_)))
            {
                activate(app, depth, row);
            } else if depth == 0 {
                open(
                    app,
                    crate::control_focus::cycle(root, super::LABELS.len(), false),
                );
            }
        }
        KeyCode::Enter => activate(app, depth, row),
        KeyCode::Up | KeyCode::Down | KeyCode::Home | KeyCode::End => {
            let choices: Vec<_> = entries
                .iter()
                .enumerate()
                .filter(|(_, item)| item.selectable())
                .map(|(i, _)| i)
                .collect();
            let next = crate::turbo::widgets::menu_row(row, &choices, code);
            if let Some(next) = next {
                select(app, depth, next);
            }
        }
        KeyCode::Char(c) => {
            if let Some(index) = entries
                .iter()
                .position(|item| item.selectable() && app.config.language.matches(item.label, c))
            {
                activate(app, depth, index);
            }
        }
        _ => {}
    }
}
