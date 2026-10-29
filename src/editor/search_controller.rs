//! Search draft input returns commands; the host alone executes document operations.
use super::{EditorAction, Modal};
use crate::dialog;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
pub(super) fn key(modal: Option<&mut Modal>, key: KeyEvent) -> Option<EditorAction> {
    match modal {
        Some(Modal::Search(dialog)) => match key.code {
            KeyCode::Char(c) if key.modifiers.contains(KeyModifiers::ALT) => {
                match c.to_ascii_lowercase() {
                    't' => dialog.focus = 0,
                    'n' if dialog.replacing => dialog.focus = 1,
                    'c' => dialog.options.case_sensitive = !dialog.options.case_sensitive,
                    'w' => dialog.options.whole_words = !dialog.options.whole_words,
                    'p' if dialog.replacing => dialog.options.prompt = !dialog.options.prompt,
                    'b' => dialog.options.backward = true,
                    'd' => dialog.options.backward = false,
                    's' if dialog.selection.is_some() => dialog.options.selected_scope = true,
                    'g' => dialog.options.selected_scope = false,
                    'e' => dialog.options.entire_scope = true,
                    'f' => dialog.options.entire_scope = false,
                    _ => {}
                }
                None
            }
            KeyCode::Char('a')
                if key.modifiers.contains(KeyModifiers::CONTROL) && dialog.focus < 2 =>
            {
                if dialog.focus == 0 {
                    dialog.query.clear();
                } else {
                    dialog.replacement.clear();
                }
                dialog.cursors[dialog.focus] = 0;
                None
            }
            _ if dialog.focus < 2
                && !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                let field = if dialog.focus == 0 {
                    &mut dialog.query
                } else {
                    &mut dialog.replacement
                };
                dialog::edit_input(field, &mut dialog.cursors[dialog.focus], key.code);
                None
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down => {
                dialog.toggle(dialog.focus);
                None
            }
            _ => None,
        },
        Some(Modal::Replace(_)) => match key.code {
            KeyCode::Char('y' | 'Y') => Some(EditorAction::ReplaceYes),
            KeyCode::Char('n' | 'N') => Some(EditorAction::ReplaceNo),
            KeyCode::Char('a' | 'A') => Some(EditorAction::ReplaceAll),
            KeyCode::Esc | KeyCode::Char('c' | 'C') => Some(EditorAction::Cancel),
            _ => None,
        },
        _ => None,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::search::{SearchDialog, SearchMemory};
    #[test]
    fn search_draft_input_and_replace_decisions_do_not_touch_document_state() {
        let mut modal = Modal::Search(SearchDialog::new(true, &SearchMemory::default(), 0..0));
        key(
            Some(&mut modal),
            KeyEvent::new(KeyCode::Char('界'), KeyModifiers::NONE),
        );
        key(
            Some(&mut modal),
            KeyEvent::new(KeyCode::Char('n'), KeyModifiers::ALT),
        );
        key(
            Some(&mut modal),
            KeyEvent::new(KeyCode::Char('文'), KeyModifiers::NONE),
        );
        let Modal::Search(d) = &modal else {
            panic!("search");
        };
        assert_eq!((&*d.query, &*d.replacement), ("界", "文"));
        assert!(!d.options.selected_scope);
        key(
            Some(&mut modal),
            KeyEvent::new(KeyCode::Char('s'), KeyModifiers::ALT),
        );
        let Modal::Search(d) = modal else {
            panic!("search");
        };
        assert!(!d.options.selected_scope);
        assert_eq!(
            key(None, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            None
        );
    }
}
