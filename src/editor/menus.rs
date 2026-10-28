use crate::app::App;
use crate::menu::definition::EDITOR_ROOTS;
pub use crate::menu::definition::{EditorItem as Item, EditorRootId};
use crossterm::event::KeyCode;
pub const LABELS: [&str; EDITOR_ROOTS.len()] = {
    let mut labels = [""; EDITOR_ROOTS.len()];
    let mut i = 0;
    while i < labels.len() {
        labels[i] = EDITOR_ROOTS[i].label;
        i += 1;
    }
    labels
};
pub fn root_index(id: EditorRootId) -> Option<usize> {
    EDITOR_ROOTS.iter().position(|root| root.id == id)
}
pub fn enabled(root: usize) -> bool {
    EDITOR_ROOTS.get(root).is_some_and(|root| root.enabled)
}
pub fn items(root: usize) -> Vec<Item> {
    EDITOR_ROOTS
        .get(root)
        .map_or_else(Vec::new, |root| root.entries.to_vec())
}
pub fn open_root(app: &mut App, id: EditorRootId) {
    if let Some(index) = root_index(id) {
        open(app, index);
    }
}
pub fn open(app: &mut App, root: usize) {
    if !enabled(root) {
        return;
    }
    if let Some(editor) = app.desktop.active_editor_mut() {
        let first = items(root)
            .iter()
            .position(|item| item.action.is_some())
            .unwrap_or(0);
        editor.modals.set_editor_menu(Some((root, first)));
    }
}
pub fn activate(app: &mut App, row: usize) {
    let Some((root, _)) = app
        .desktop
        .active_editor()
        .and_then(|editor| editor.modals.editor_menu())
    else {
        return;
    };
    if let Some(command) = items(root).get(row).and_then(|item| item.action) {
        if let Some(editor) = app.desktop.active_editor_mut() {
            editor.modals.set_editor_menu(None);
        }
        super::action(app, command);
    }
}
pub fn key(app: &mut App, code: KeyCode) {
    let Some((root, row)) = app
        .desktop
        .active_editor()
        .and_then(|editor| editor.modals.editor_menu())
    else {
        return;
    };
    match code {
        KeyCode::Esc | KeyCode::F(10) => {
            if let Some(editor) = app.desktop.active_editor_mut() {
                editor.modals.set_editor_menu(None);
            }
        }
        KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::BackTab => {
            let roots: Vec<_> = EDITOR_ROOTS
                .iter()
                .enumerate()
                .filter(|(_, r)| r.enabled)
                .map(|(i, _)| i)
                .collect();
            let i = roots.iter().position(|r| *r == root).unwrap_or(0);
            let reverse = matches!(code, KeyCode::Left | KeyCode::BackTab);
            open(
                app,
                roots[crate::control_focus::cycle(i, roots.len(), reverse)],
            );
        }
        KeyCode::Enter => activate(app, row),
        KeyCode::Char(c) => {
            if let Some(index) = items(root).iter().position(|item| {
                item.action.is_some() && app.config.language.matches(item.label, c)
            }) {
                activate(app, index);
            }
        }
        KeyCode::Up | KeyCode::Down | KeyCode::Home | KeyCode::End => {
            let entries = items(root);
            let choices: Vec<_> = entries
                .iter()
                .enumerate()
                .filter(|(_, item)| item.action.is_some())
                .map(|(i, _)| i)
                .collect();
            let next = crate::turbo::widgets::menu_row(row, &choices, code);
            if let (Some(editor), Some(next)) = (app.desktop.active_editor_mut(), next) {
                editor.modals.set_editor_menu(Some((root, next)));
            }
        }
        _ => {}
    }
}
