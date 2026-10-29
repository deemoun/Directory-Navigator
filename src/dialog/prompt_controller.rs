//! Common prompt input and intent. Submission, help and cancellation are host effects.
use super::Prompt;
use crate::{
    mouse::Target,
    turbo::dialog_spec::{DialogSpec, Proposal},
};
use crossterm::event::{KeyCode as K, KeyEvent, KeyModifiers as M};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Effect {
    None,
    Submit,
    Cancel,
    Help,
}
pub(crate) fn key(spec: &DialogSpec<'_>, key: KeyEvent) -> Option<Proposal> {
    if key.code == K::F(1) {
        Some(spec.proposal(Some(Target::PromptHelp)))
    } else if let K::Char(c) = key.code {
        if key.modifiers.contains(M::ALT) {
            spec.mnemonic(c).map(|id| spec.activate(id))
        } else {
            spec.key(key)
        }
    } else {
        spec.key(key)
    }
}
pub(crate) fn apply(prompt: &mut Prompt, focus: Option<usize>, target: Option<Target>) -> Effect {
    if let Some(focus) = focus {
        prompt.focus = focus;
    }
    match target {
        Some(Target::SubmitPrompt) => Effect::Submit,
        Some(Target::CancelPrompt) => Effect::Cancel,
        Some(Target::PromptHelp) => Effect::Help,
        Some(Target::PromptField) => {
            prompt.focus = 0;
            Effect::None
        }
        _ => Effect::None,
    }
}
pub(crate) fn edit(prompt: &mut Prompt, key: KeyEvent) {
    if prompt.focus == 0 && !key.modifiers.intersects(M::CONTROL | M::ALT) {
        prompt.edit(key.code);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn prompt() -> Prompt {
        Prompt {
            kind: super::super::PromptKind::Operation(crate::operations::Kind::Mkdir),
            sources: vec![],
            base: std::env::temp_dir(),
            field: String::new(),
            cursor: 0,
            focus: 0,
            copy: None,
        }
    }
    #[test]
    fn unicode_input_and_effects_are_isolated_from_filesystem_and_modal_lifecycle() {
        let mut p = prompt();
        p.paste("界 e\u{301}");
        let text = p.field.clone();
        assert_eq!(
            apply(&mut p, Some(2), Some(Target::CancelPrompt)),
            Effect::Cancel
        );
        edit(&mut p, KeyEvent::new(K::Char('x'), M::NONE));
        p.paste("ignored");
        assert_eq!(p.field, text);
        assert_eq!(apply(&mut p, None, Some(Target::PromptHelp)), Effect::Help);
        assert_eq!(p.focus, 2);
        assert_eq!(apply(&mut p, None, Some(Target::PromptField)), Effect::None);
        assert_eq!(p.focus, 0);
        edit(&mut p, KeyEvent::new(K::Char('x'), M::CONTROL));
        assert_eq!(p.field, text);
        assert_eq!(
            apply(&mut p, Some(1), Some(Target::SubmitPrompt)),
            Effect::Submit
        );
        assert_eq!(p.field, text);
    }
}
