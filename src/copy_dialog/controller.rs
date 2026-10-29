//! Draft-only Copy/Move transitions. The host owns children and filesystem submission.
use super::Action;
use crate::{
    dialog::{Prompt, PromptKind},
    operations::{ExistingDestination, Kind},
};
use crossterm::event::{KeyCode as K, KeyEvent, KeyModifiers as M};
use std::path::PathBuf;
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Effect {
    None,
    History,
    Tree(PathBuf),
}
pub(super) fn reduce(prompt: &mut Prompt, action: Action) -> Effect {
    let moving = matches!(prompt.kind, PromptKind::Operation(Kind::Move));
    let Some(copy) = &mut prompt.copy else {
        return Effect::None;
    };
    match action {
        Action::Field => prompt.focus = 0,
        Action::Refuse => {
            copy.options.existing = ExistingDestination::Refuse;
            prompt.focus = 1;
        }
        Action::Skip => {
            copy.options.existing = ExistingDestination::Skip;
            prompt.focus = 2;
        }
        Action::CheckSpace => {
            copy.options.check_free_space ^= true;
            prompt.focus = 3;
        }
        Action::Verify => {
            if !copy.options.remove_sources {
                copy.options.verify_writes ^= true;
                prompt.focus = 4;
            }
        }
        Action::RemoveSources => {
            if moving {
                return Effect::None;
            }
            copy.options.remove_sources ^= true;
            if copy.options.remove_sources {
                copy.options.verify_writes = true;
            }
            prompt.focus = 5;
        }
        Action::History => {
            prompt.focus = 10;
            return Effect::History;
        }
        Action::Tree => {
            prompt.focus = 8;
            return Effect::Tree(prompt.base.clone());
        }
    }
    Effect::None
}
pub(super) fn edit(prompt: &mut Prompt, key: KeyEvent) {
    if key.modifiers.contains(M::ALT) {
        return;
    }
    if (prompt.focus == 1 || prompt.focus == 2)
        && matches!(key.code, K::Up | K::Down | K::Left | K::Right)
    {
        reduce(
            prompt,
            if prompt.focus == 1 {
                Action::Skip
            } else {
                Action::Refuse
            },
        );
    } else if prompt.focus == 0 && !key.modifiers.intersects(M::CONTROL | M::SUPER) {
        prompt.edit(key.code);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn prompt(kind: Kind) -> Prompt {
        Prompt {
            kind: PromptKind::Operation(kind),
            sources: vec![],
            base: PathBuf::from("fixture"),
            field: String::new(),
            cursor: 0,
            focus: 0,
            copy: Some(super::super::Draft::default()),
        }
    }
    #[test]
    fn remove_sources_requires_verification_and_move_cannot_toggle_it() {
        let mut copy = prompt(Kind::Copy);
        reduce(&mut copy, Action::RemoveSources);
        reduce(&mut copy, Action::Verify);
        let options = &copy.copy.as_ref().unwrap().options;
        assert!(options.remove_sources && options.verify_writes);
        assert_eq!(options.existing, ExistingDestination::Refuse);
        let mut moving = prompt(Kind::Move);
        assert_eq!(reduce(&mut moving, Action::RemoveSources), Effect::None);
        assert!(!moving.copy.as_ref().unwrap().options.remove_sources);
        assert_eq!(moving.focus, 0);
    }
    #[test]
    fn child_requests_and_keyboard_are_draft_only() {
        let mut p = prompt(Kind::Copy);
        assert_eq!(reduce(&mut p, Action::Tree), Effect::Tree(p.base.clone()));
        assert_eq!(p.focus, 8);
        assert_eq!(reduce(&mut p, Action::History), Effect::History);
        reduce(&mut p, Action::Refuse);
        edit(&mut p, KeyEvent::new(K::Down, M::NONE));
        assert_eq!(
            p.copy.as_ref().unwrap().options.existing,
            ExistingDestination::Skip
        );
        p.paste("ignored");
        assert!(p.field.is_empty());
        reduce(&mut p, Action::Field);
        p.paste("界 e\u{301}");
        let text = p.field.clone();
        edit(&mut p, KeyEvent::new(K::Char('x'), M::CONTROL));
        assert_eq!(p.field, text);
    }
}
