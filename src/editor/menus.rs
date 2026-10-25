use super::EditorAction as A;
use crate::app::App;
use crossterm::event::KeyCode;
pub const LABELS: [&str; 7] = [
    "~F~ile",
    "~E~dit",
    "~S~earch",
    "~P~aragraph",
    "~B~lock",
    "~M~isc",
    "~O~ptions",
];
const ROOTS: [usize; 6] = [0, 1, 2, 4, 5, 6];
pub struct Item {
    pub label: &'static str,
    pub shortcut: &'static str,
    pub action: Option<A>,
}
fn item(label: &'static str, shortcut: &'static str, action: A) -> Item {
    Item {
        label,
        shortcut,
        action: Some(action),
    }
}
fn line() -> Item {
    Item {
        label: "",
        shortcut: "",
        action: None,
    }
}

pub fn enabled(root: usize) -> bool {
    ROOTS.contains(&root)
}
pub fn items(root: usize) -> Vec<Item> {
    match root {
        0 => vec![
            item("~O~pen...", "F3", A::Open),
            item("~S~ave", "F2", A::Save),
            item("Save ~a~s...", "Shift+F2", A::SaveAs),
            line(),
            item("~W~indows...", "Alt+0", A::Windows),
            item("~C~lose editor", "Ctrl+F4", A::Close),
            item("E~x~it DN", "Alt+X", A::Quit),
        ],
        1 => vec![
            item("~U~ndo", "Alt+Backspace", A::Undo),
            item("~R~edo", "Ctrl+Y", A::Redo),
            line(),
            item("Cu~t~", "Shift+Del", A::Cut),
            item("~C~opy", "Ctrl+Ins", A::Copy),
            item("~P~aste", "Shift+Ins", A::Paste),
            item("Select ~a~ll", "Ctrl+A", A::SelectAll),
            item("De~l~ete selection", "Ctrl+Del", A::Clear),
        ],
        2 => vec![
            item("~F~ind...", "F7", A::Find),
            item("~R~eplace...", "Ctrl+F7", A::Replace),
            item("~S~earch again", "Shift+F7", A::FindNext),
            item("Re~v~ersed search", "Alt+F7", A::FindPrevious),
            item("~G~o to line...", "Alt+G", A::GoTo),
        ],
        4 => vec![
            item("Select ~a~ll", "Ctrl+A", A::SelectAll),
            item("De~l~ete selection", "Ctrl+Del", A::Clear),
            line(),
            item("Cu~t~", "Shift+Del", A::Cut),
            item("~C~opy", "Ctrl+Ins", A::Copy),
            item("~P~aste", "Shift+Ins", A::Paste),
        ],
        5 => vec![item("~D~uplicate line", "F6", A::Duplicate)],
        6 => vec![
            item("Auto~i~ndent", "", A::AutoIndent),
            item("Syntax ~h~ighlight", "", A::Colors),
            item("~T~ab width...", "", A::TabWidth),
            item("~E~xpand typed tabs to spaces", "", A::ExpandTabs),
            line(),
            item("~S~ave desktop", "", A::SaveDesktop),
        ],
        _ => Vec::new(),
    }
}
pub fn open(app: &mut App, root: usize) {
    if !enabled(root) {
        return;
    }
    if let Some(editor) = app.desktop.active_editor_mut() {
        editor.modals.set_editor_menu(Some((root, 0)));
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
            let i = ROOTS.iter().position(|r| *r == root).unwrap_or(0);
            let reverse = matches!(code, KeyCode::Left | KeyCode::BackTab);
            open(
                app,
                ROOTS[crate::control_focus::cycle(i, ROOTS.len(), reverse)],
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
