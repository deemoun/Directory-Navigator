use super::*;
use crate::{app::App, dialog};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub fn key(app: &mut App, key: KeyEvent) {
    let Some(editor) = app.desktop.active_editor_mut() else {
        return;
    };
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    if key.code == KeyCode::Char('x') && alt {
        request_close(app, CloseIntent::Quit);
        return;
    }
    if editor.busy() {
        if key.code == KeyCode::Esc {
            request_close(app, CloseIntent::Panel);
        }
        return;
    }
    if matches!(editor.modals.editor_modal(), Some(Modal::File(_))) {
        file_key(app, key);
        return;
    }
    if super::dialog_controls::key(app, key) {
        return;
    }
    let editor = app.desktop.active_editor_mut().unwrap();
    if matches!(
        editor.modals.editor_modal(),
        Some(Modal::Search(_) | Modal::Replace(_))
    ) {
        search_controls::key(app, key);
        return;
    }
    if let Some(modal) = editor.modals.editor_modal_mut() {
        let command = match modal {
            Modal::ConfirmClose { .. } => match key.code {
                KeyCode::Char('s' | 'S') => Some(EditorAction::Save),
                KeyCode::Char('d' | 'D') => Some(EditorAction::Discard),
                KeyCode::Esc | KeyCode::Char('c' | 'C') => Some(EditorAction::Cancel),
                _ => None,
            },
            Modal::Help => None,
            Modal::Prompt {
                field,
                cursor,
                focus,
                ..
            } => match key.code {
                KeyCode::Char('a') if ctrl && *focus == 0 => {
                    field.clear();
                    *cursor = 0;
                    None
                }
                _ if *focus == 0 && !alt && !ctrl => {
                    dialog::edit_input(field, cursor, key.code);
                    None
                }
                _ => None,
            },
            _ => None,
        };
        if let Some(command) = command {
            action(app, command);
        }
        return;
    }
    if alt {
        let command = match key.code {
            KeyCode::Backspace => Some(EditorAction::Undo),
            KeyCode::F(7) => Some(EditorAction::FindPrevious),
            KeyCode::Char('g') => Some(EditorAction::GoTo),
            _ => None,
        };
        if let Some(command) = command {
            action(app, command);
            return;
        }
        if let KeyCode::Char(c) = key.code {
            if let Some(root) = menus::LABELS
                .iter()
                .position(|label| app.config.language.matches(label, c))
            {
                menus::open(app, root);
            }
        }
        return;
    }
    if editor.modals.editor_menu().is_some() {
        let accelerator = matches!(key.code, KeyCode::F(1 | 2 | 3 | 6 | 7)) || ctrl;
        if accelerator {
            editor.modals.set_editor_menu(None);
        } else {
            menus::key(app, key.code);
            return;
        }
    }
    use EditorAction as A;
    let command = if ctrl {
        match key.code {
            KeyCode::F(4) => Some(A::Close),
            KeyCode::F(7) => Some(A::Replace),
            KeyCode::Insert => Some(A::Copy),
            KeyCode::Delete => Some(A::Clear),
            KeyCode::Char('s') => Some(A::Save),
            KeyCode::Char('f') => Some(A::Find),
            KeyCode::Char('g') => Some(A::GoTo),
            KeyCode::Char('z') => Some(A::Undo),
            KeyCode::Char('y') => Some(A::Redo),
            KeyCode::Char('c') => Some(A::Copy),
            KeyCode::Char('x') => Some(A::Cut),
            KeyCode::Char('v') => Some(A::Paste),
            KeyCode::Char('a') => Some(A::SelectAll),
            _ => None,
        }
    } else {
        match key.code {
            KeyCode::F(1) => Some(A::Help),
            KeyCode::F(2) if shift => Some(A::SaveAs),
            KeyCode::F(2) => Some(A::Save),
            KeyCode::F(3) if !shift => Some(A::Open),
            KeyCode::F(6) if !shift => Some(A::Duplicate),
            KeyCode::F(7) if shift => Some(A::FindNext),
            KeyCode::F(7) => Some(A::Find),
            KeyCode::F(10) => Some(A::Menu),
            KeyCode::Insert if shift => Some(A::Paste),
            KeyCode::Delete if shift => Some(A::Cut),
            KeyCode::Esc => Some(A::Close),
            _ => None,
        }
    };
    if let Some(command) = command {
        action(app, command);
        return;
    }
    let Some(buffer) = &mut editor.buffer else {
        return;
    };
    let result = match key.code {
        KeyCode::Left if ctrl => {
            buffer.word_move(false, shift);
            Ok(())
        }
        KeyCode::Right if ctrl => {
            buffer.word_move(true, shift);
            Ok(())
        }
        KeyCode::Left => {
            buffer.horizontal(false, shift);
            Ok(())
        }
        KeyCode::Right => {
            buffer.horizontal(true, shift);
            Ok(())
        }
        KeyCode::Up => {
            buffer.vertical(-1, shift);
            Ok(())
        }
        KeyCode::Down => {
            buffer.vertical(1, shift);
            Ok(())
        }
        KeyCode::PageUp => {
            buffer.vertical(-(editor.rows as isize), shift);
            Ok(())
        }
        KeyCode::PageDown => {
            buffer.vertical(editor.rows as isize, shift);
            Ok(())
        }
        KeyCode::Home => {
            buffer.move_to(
                if ctrl {
                    0
                } else {
                    buffer.lines[buffer.row()].start
                },
                shift,
            );
            Ok(())
        }
        KeyCode::End => {
            buffer.move_to(
                if ctrl {
                    buffer.text.len()
                } else {
                    buffer.lines[buffer.row()].end
                },
                shift,
            );
            Ok(())
        }
        KeyCode::Backspace => buffer.delete(true),
        KeyCode::Delete => buffer.delete(false),
        KeyCode::Enter => buffer.enter(editor.preferences.auto_indent),
        KeyCode::Tab if !shift => {
            if editor.preferences.expand_tabs {
                buffer.insert(
                    &" ".repeat(buffer.tab_size - buffer.insertion_column() % buffer.tab_size),
                )
            } else {
                buffer.insert("\t")
            }
        }
        KeyCode::Char(c) if !ctrl => buffer.insert(&c.to_string()),
        _ => Ok(()),
    };
    editor.recolor();
    editor.keep_visible();
    if let Err(error) = result {
        app.error(error);
    }
}
