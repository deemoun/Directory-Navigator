//! Viewer search draft only. Search execution and shared defaults remain host-owned.
use super::navigation::{Action, SearchDialog};
use crossterm::event::{KeyEvent, KeyModifiers};
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Effect {
    None,
    Cancel,
}
pub(super) fn action(dialog: &mut SearchDialog, action: Action) -> Effect {
    match action {
        Action::Cancel => return Effect::Cancel,
        Action::Case => {
            dialog.memory.case_sensitive ^= true;
            dialog.focus = 1;
        }
        Action::Field => dialog.focus = 0,
        _ => {}
    }
    Effect::None
}
pub(super) fn paste(dialog: &mut SearchDialog, text: &str) {
    if dialog.focus == 0 {
        crate::dialog::paste_input(&mut dialog.memory.query, &mut dialog.cursor, text);
    }
}
pub(super) fn edit(dialog: &mut SearchDialog, key: KeyEvent) {
    if dialog.focus == 0
        && !key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
    {
        crate::dialog::edit_input(&mut dialog.memory.query, &mut dialog.cursor, key.code);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer::SearchMemory;
    #[test]
    fn cancel_and_paste_isolation_preserve_shared_search_defaults() {
        let shared = SearchMemory::default();
        let mut d = SearchDialog {
            memory: shared.clone(),
            cursor: 0,
            focus: 0,
        };
        paste(&mut d, "界 e\u{301}");
        let text = d.memory.query.clone();
        action(&mut d, Action::Case);
        paste(&mut d, "ignored");
        assert_eq!(d.memory.query, text);
        assert_eq!(action(&mut d, Action::Cancel), Effect::Cancel);
        assert!(!d.memory.case_sensitive);
        assert!(shared.case_sensitive && shared.query.is_empty());
    }
}
