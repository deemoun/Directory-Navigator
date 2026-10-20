use super::{items, level_items, selected_row, submenu_items, Action};
use crate::app::App;
use crossterm::event::KeyCode;

pub fn close(app: &mut App) {
    app.menu = None;
    app.submenus.clear();
}
pub fn open(app: &mut App, root: usize) {
    let root = root % super::LABELS.len();
    let entries = items(root);
    app.menu = Some((
        root % super::LABELS.len(),
        entries
            .iter()
            .position(|item| item.selectable())
            .unwrap_or(0),
    ));
    app.submenus.clear();
}
pub fn open_mnemonic(app: &mut App, character: char) -> bool {
    if let Some(index) = super::LABELS
        .iter()
        .position(|label| super::mnemonic(label) == Some(character.to_ascii_lowercase()))
    {
        open(app, index);
        true
    } else {
        false
    }
}
fn select(app: &mut App, depth: usize, row: usize) {
    app.submenus.truncate(depth);
    if depth == 0 {
        if let Some((_, selected)) = &mut app.menu {
            *selected = row;
        }
    } else if let Some((_, selected)) = app.submenus.get_mut(depth - 1) {
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
            app.submenus.push((id, first));
        }
        action => super::execute(app, action),
    }
}
pub fn key(app: &mut App, code: KeyCode) {
    let Some((root, _)) = app.menu else {
        return;
    };
    let depth = app.submenus.len();
    let entries = level_items(app, depth);
    let row = selected_row(app, depth);
    match code {
        KeyCode::F(1) => super::execute(app, Action::Key(1)),
        KeyCode::F(9) => close(app),
        KeyCode::Esc | KeyCode::Left if depth > 0 => {
            app.submenus.pop();
        }
        KeyCode::Esc => close(app),
        KeyCode::Left | KeyCode::BackTab => {
            open(app, (root + super::LABELS.len() - 1) % super::LABELS.len())
        }
        KeyCode::Tab => open(app, (root + 1) % super::LABELS.len()),
        KeyCode::Right => {
            if entries
                .get(row)
                .is_some_and(|item| matches!(item.action, Action::Submenu(_)))
            {
                activate(app, depth, row);
            } else if depth == 0 {
                open(app, (root + 1) % super::LABELS.len());
            }
        }
        KeyCode::Enter => activate(app, depth, row),
        KeyCode::Up | KeyCode::Down | KeyCode::Home | KeyCode::End => {
            let mut candidate = match code {
                KeyCode::Home => entries.len().saturating_sub(1),
                KeyCode::End => 0,
                _ => row,
            };
            let backwards = matches!(code, KeyCode::Up | KeyCode::End);
            for _ in 0..entries.len() {
                candidate = if backwards {
                    (candidate + entries.len() - 1) % entries.len()
                } else {
                    (candidate + 1) % entries.len()
                };
                if entries[candidate].selectable() {
                    select(app, depth, candidate);
                    break;
                }
            }
        }
        KeyCode::Char(c) => {
            if let Some(index) = entries.iter().position(|item| {
                item.selectable() && super::mnemonic(item.label) == Some(c.to_ascii_lowercase())
            }) {
                activate(app, depth, index);
            }
        }
        _ => {}
    }
}
