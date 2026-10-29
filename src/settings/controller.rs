//! Settings draft transitions. No application, desktop, storage or modal lifecycle access.
use super::{ChoiceDialog, Settings, SettingsKind};
use crate::{localization::Language, mouse::Target};
use crossterm::event::KeyCode as K;
#[derive(Clone, Copy)]
pub(super) enum Action {
    OpenChoice(usize),
    Keyboard,
    Regional(usize),
    Toggle(usize),
    Accept,
    Cancel,
}
pub(super) enum Effect {
    None,
    Choice(ChoiceDialog),
    Accept,
    Cancel,
}
pub(super) enum Key {
    Handled,
    Choice(usize),
    Controls(K),
}
pub(super) enum ChoiceKey {
    Handled,
    Accept,
    Cancel,
}
pub(super) fn command(target: Target) -> Option<Action> {
    Some(match target {
        Target::OpenSettingsChoice(i) => Action::OpenChoice(i),
        Target::ShortcutProfile => Action::Keyboard,
        Target::RegionalChoice(i) => Action::Regional(i),
        Target::Setting(i) => Action::Toggle(i),
        Target::AcceptSettings => Action::Accept,
        Target::CancelSettings => Action::Cancel,
        _ => return None,
    })
}
pub(super) fn reduce(state: &mut Settings, action: Action, language: Language) -> Effect {
    match action {
        Action::OpenChoice(row) => {
            if !matches!(state.kind, SettingsKind::Keyboard | SettingsKind::Regional)
                || row >= state.control_count()
            {
                return Effect::None;
            }
            return Effect::Choice(ChoiceDialog {
                kind: state.kind,
                row,
                items: super::choice_items(state, row, language),
                cursor: 0,
            });
        }
        Action::Keyboard => state.keyboard_choice(false),
        Action::Regional(i) => state.regional_choice(i, false),
        Action::Toggle(i) => state.toggle(i),
        Action::Accept => return Effect::Accept,
        Action::Cancel => return Effect::Cancel,
    }
    Effect::None
}
pub(super) fn key(state: &mut Settings, code: K) -> Key {
    if code == K::F(4) {
        return Key::Choice(state.cursor);
    }
    if state.kind == SettingsKind::Keyboard
        && state.cursor == 0
        && matches!(code, K::Left | K::Right | K::Char(' ' | 'p' | 'P'))
    {
        state.keyboard_choice(code == K::Left);
        return Key::Handled;
    }
    if state.kind == SettingsKind::Regional
        && state.cursor < state.control_count()
        && matches!(code, K::Left | K::Right | K::Char(' '))
    {
        state.regional_choice(state.cursor, code == K::Left);
        return Key::Handled;
    }
    Key::Controls(match code {
        K::Up => K::BackTab,
        K::Down => K::Tab,
        _ => code,
    })
}
pub(super) fn choice_key(state: &mut ChoiceDialog, code: K) -> ChoiceKey {
    if let Some(row) =
        crate::turbo::widgets::list_key(state.cursor, state.items.len(), state.items.len(), code)
    {
        state.cursor = row;
        return ChoiceKey::Handled;
    }
    match code {
        K::Esc | K::F(4) => ChoiceKey::Cancel,
        K::Enter | K::Char(' ') => ChoiceKey::Accept,
        _ => ChoiceKey::Handled,
    }
}
pub(super) fn apply_choice(state: &mut Settings, choice: ChoiceDialog) {
    if state.kind != choice.kind {
        return;
    }
    for _ in 0..choice.cursor {
        if state.kind == SettingsKind::Keyboard {
            state.keyboard_choice(false);
        } else {
            state.regional_choice(choice.row, false);
        }
    }
    state.cursor = choice.row;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    #[test]
    fn draft_actions_do_not_commit_or_own_the_presentation() {
        let original = Config::default();
        let mut s = Settings::new(SettingsKind::Interface, &original);
        let field = s.rows()[0];
        let before = s.value(field);
        assert!(matches!(
            reduce(&mut s, Action::Toggle(0), Language::English),
            Effect::None
        ));
        assert_eq!(s.value(field), !before);
        assert_eq!((field.definition().get)(&original), before);
        assert!(matches!(
            reduce(&mut s, Action::Cancel, Language::English),
            Effect::Cancel
        ));
        assert!(matches!(
            reduce(&mut s, Action::Accept, Language::English),
            Effect::Accept
        ));
        assert_eq!((field.definition().get)(&original), before);
    }
    #[test]
    fn child_choices_update_only_the_matching_parent_draft_and_restore_its_row() {
        let config = Config::default();
        let mut parent = Settings::new(SettingsKind::Keyboard, &config);
        let Effect::Choice(mut child) =
            reduce(&mut parent, Action::OpenChoice(0), Language::English)
        else {
            panic!("choice");
        };
        assert_eq!(parent.draft.shortcuts, config.shortcuts);
        assert!(matches!(
            choice_key(&mut child, K::Down),
            ChoiceKey::Handled
        ));
        assert!(matches!(
            choice_key(&mut child, K::Enter),
            ChoiceKey::Accept
        ));
        apply_choice(&mut parent, child);
        assert_ne!(parent.draft.shortcuts, config.shortcuts);
        assert_eq!(parent.cursor, 0);
        let mut wrong = Settings::new(SettingsKind::Regional, &config);
        let previous = wrong.draft.shortcuts;
        apply_choice(
            &mut wrong,
            ChoiceDialog {
                kind: SettingsKind::Keyboard,
                row: 0,
                items: vec![],
                cursor: 1,
            },
        );
        assert_eq!(wrong.draft.shortcuts, previous);
        assert!(matches!(
            reduce(
                &mut parent,
                Action::OpenChoice(usize::MAX),
                Language::English
            ),
            Effect::None
        ));
    }
}
