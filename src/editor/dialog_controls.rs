//! Shared dialog proposals retain editor-owned state and guarded actions.
use super::{action, Modal};
use crate::{app::App, mouse::Target, turbo::dialog_spec::input_area};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
pub(super) fn key(app: &mut App, key: KeyEvent) -> bool {
    let proposal = {
        let Some(spec) = crate::ui::editor_spec(app, input_area(app)) else {
            return false;
        };
        let search = app
            .desktop
            .active_editor()
            .is_some_and(|e| matches!(e.modals.editor_modal(), Some(Modal::Search(_))));
        // Preserve existing text/shortcut semantics; Space toggles search options.
        if !matches!(
            key.code,
            KeyCode::Tab | KeyCode::BackTab | KeyCode::Enter | KeyCode::Esc
        ) && !(search
            && key.code == KeyCode::Char(' ')
            && !key.modifiers.contains(KeyModifiers::ALT))
        {
            return false;
        }
        spec.key(key)
    };
    let Some(proposal) = proposal else {
        return false;
    };
    crate::dialog_controls::apply(app, proposal, |app, target| {
        if let Target::Editor(command) = target {
            action(app, command);
        }
    });
    true
}
