//! Native dialog focus and button commands with DN's Unicode field semantics.
//! Native controls use the application's single event loop; workers and terminal
//! restoration remain owned by the session.
use crate::{app::App, welcome};
use crossterm::event::{KeyCode as K, KeyEvent, KeyModifiers as M};
use std::time::Duration;
use turbo_vision::{
    core::{
        event::{Event, EventType, KB_ENTER, KB_SHIFT_TAB, KB_TAB},
        geometry::Rect,
    },
    views::{
        button::{self, Button},
        dialog::{CloseOn, Dialog},
        GroupLike, View, ViewId,
    },
};

const FIRST_BUTTON: u16 = 0xf100;
struct Form {
    dialog: Dialog,
    ids: Vec<(usize, ViewId)>,
}
impl Form {
    fn new() -> Self {
        let mut dialog = Dialog::new(Rect::new(0, 0, 72, 20), "");
        // Native commands are proposals. Controllers decide whether to submit,
        // show help or cancel; only their established save/close guards mutate
        // application state. The toolkit must not remove a view by itself.
        dialog.set_close_on(CloseOn::Commands(Vec::new()));
        dialog.set_state_flag(turbo_vision::core::state::State::MODAL, true);
        Self {
            dialog,
            ids: Vec::new(),
        }
    }
    fn button(&mut self, index: usize, default: bool) {
        let mut button = Button::new(
            Rect::new(1, 3 + index as i16 * 2, 20, 5 + index as i16 * 2),
            "",
            FIRST_BUTTON + index as u16,
            default,
        );
        button.set_selectable(true);
        let id = self.dialog.add(button);
        self.ids.push((index, id));
    }
    fn select(&mut self, index: usize) {
        let position = self.ids.iter().position(|(i, _)| *i == index).unwrap_or(0);
        self.dialog.set_focus_to_child(position);
    }
    fn selected(&self) -> usize {
        let selected = self.dialog.group().focused_view_id();
        self.ids
            .iter()
            .find(|(_, id)| Some(*id) == selected)
            .map_or(0, |(i, _)| *i)
    }
    fn event(&mut self, mut event: Event) -> Option<usize> {
        // DN buttons have immediate activation. Scope TV's optional animation
        // override to this dispatch, so no delayed command can reach a newer
        // dialog after the original one closes.
        struct Animation(Duration);
        impl Drop for Animation {
            fn drop(&mut self) {
                button::set_press_animation(self.0);
            }
        }
        let _animation = Animation(button::press_animation());
        button::set_press_animation(Duration::ZERO);
        self.dialog.handle_event(&mut event);
        (event.what == EventType::Command
            && (FIRST_BUTTON..FIRST_BUTTON + 4).contains(&event.command))
        .then(|| usize::from(event.command - FIRST_BUTTON))
    }
    fn press(&mut self, index: usize) -> Option<usize> {
        let previous = self.selected();
        self.select(index);
        let command = self.event(Event::keyboard(KB_ENTER));
        self.select(previous);
        command
    }
}

pub fn welcome_key(app: &mut App, key: KeyEvent) {
    if app.utilities.size.0 < 80
        || app.utilities.size.1 < 25
        || key.modifiers.intersects(M::CONTROL | M::ALT | M::SUPER)
    {
        return;
    }
    let Some(welcome) = app.welcome.modals.welcome() else {
        return;
    };
    let mut form = Form::new();
    for i in 0..welcome::BUTTONS.len() {
        form.button(i, i == 0);
    }
    form.select(welcome.focus);
    let selected = match key.code {
        K::Esc => form.press(0),
        K::F(1) => form.press(2),
        K::Enter => form.event(Event::keyboard(KB_ENTER)),
        K::Tab | K::Right | K::Down => form.event(Event::keyboard(
            if key.modifiers.contains(M::SHIFT) && key.code == K::Tab {
                KB_SHIFT_TAB
            } else {
                KB_TAB
            },
        )),
        K::BackTab | K::Left | K::Up => form.event(Event::keyboard(KB_SHIFT_TAB)),
        K::Char(c) => welcome::BUTTONS
            .iter()
            .position(|(label, _)| app.config.language.matches(label, c))
            .and_then(|i| form.press(i)),
        _ => None,
    };
    if let Some(welcome) = app.welcome.modals.welcome_mut() {
        welcome.focus = form.selected();
    }
    if let Some(index) = selected {
        welcome::action(app, welcome::BUTTONS[index].1);
    }
}

pub fn prompt_key(app: &mut App, key: KeyEvent) {
    use crate::{mouse::Target, turbo::dialog_spec::input_area};
    let proposal = {
        let Some(spec) = crate::ui::prompt_spec(app, input_area(app)) else {
            return;
        };
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
    };
    if let Some(proposal) = proposal {
        crate::dialog_controls::apply(app, proposal, |app, target| match Some(target) {
            Some(Target::SubmitPrompt) => app.submit_prompt(),
            Some(Target::CancelPrompt) => app.modals.set_prompt(None),
            Some(Target::PromptHelp) => crate::dialog::prompt_help(app),
            Some(Target::PromptField) => {
                if let Some(prompt) = app.modals.prompt_mut() {
                    prompt.focus = 0;
                }
            }
            _ => {}
        });
    } else if !key.modifiers.intersects(M::CONTROL | M::ALT) {
        if let Some(prompt) = app.modals.prompt_mut().filter(|p| p.focus == 0) {
            prompt.edit(key.code);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn welcome_button_commands_remain_synchronous_and_bounded() {
        let previous = button::press_animation();
        let mut form = Form::new();
        for i in 0..3 {
            form.button(i, i == 0);
        }
        form.select(0);
        assert_eq!(form.event(Event::keyboard(KB_ENTER)), Some(0));
        for selected in [1, 2, 0] {
            assert_eq!(form.event(Event::keyboard(KB_TAB)), None);
            assert_eq!(form.selected(), selected);
        }
        form.event(Event::keyboard(KB_SHIFT_TAB));
        assert_eq!(form.selected(), 2);
        assert_eq!(form.event(Event::keyboard(KB_ENTER)), Some(2));
        assert_eq!(button::press_animation(), previous);
    }
}
