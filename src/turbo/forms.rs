//! Native dialog focus and button commands with DN's Unicode field semantics.
//! The existing canvas is the skin during the renderer conversion. No nested
//! modal loop: workers and terminal restoration remain owned by the main loop.
use crate::{
    app::App,
    dialog::{Prompt, PromptKind},
    operations::Kind,
    welcome,
};
use crossterm::event::{KeyCode as K, KeyEvent, KeyModifiers as M};
use std::{cell::RefCell, rc::Rc, time::Duration};
use turbo_vision::{
    core::{
        event::{Event, EventType, KB_ENTER, KB_SHIFT_TAB, KB_TAB},
        geometry::Rect,
        palette::Palette,
    },
    terminal::Terminal,
    views::{
        button::{self, Button},
        dialog::{CloseOn, Dialog},
        GroupLike, View, ViewCore, ViewId,
    },
};

const FIRST_BUTTON: u16 = 0xf100;
#[derive(Default)]
struct FieldData {
    text: String,
    cursor: usize,
}
struct Field {
    core: ViewCore,
    data: Rc<RefCell<FieldData>>,
    key: KeyEvent,
}
impl View for Field {
    fn core(&self) -> &ViewCore {
        &self.core
    }
    fn core_mut(&mut self) -> &mut ViewCore {
        &mut self.core
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn get_palette(&self) -> Option<Palette> {
        None
    }
    fn can_focus(&self) -> bool {
        true
    }
    fn draw(&mut self, _: &mut Terminal) {}
    fn handle_event(&mut self, event: &mut Event) {
        if event.what != EventType::Keyboard
            || matches!(event.key_code, KB_ENTER | KB_TAB | KB_SHIFT_TAB)
        {
            return;
        }
        if !self.key.modifiers.intersects(M::CONTROL | M::ALT) {
            let mut field = self.data.borrow_mut();
            let FieldData { text, cursor } = &mut *field;
            crate::dialog::edit_input(text, cursor, self.key.code);
            // Even a non-editing printable key belongs to the field. Never
            // let a label mnemonic steal text from a focused path input.
            event.clear();
        }
    }
}

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
    fn field(&mut self, data: Rc<RefCell<FieldData>>, key: KeyEvent) {
        let id = self.dialog.add(Field {
            core: ViewCore::new(Rect::new(1, 1, 60, 2)),
            data,
            key,
        });
        self.ids.push((0, id));
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
    let Some(prompt) = app.modals.prompt() else {
        return;
    };
    let input = !matches!(
        prompt.kind,
        PromptKind::Operation(Kind::Delete | Kind::Trash)
    );
    let data = Rc::new(RefCell::new(FieldData {
        text: prompt.field.clone(),
        cursor: prompt.cursor,
    }));
    let mut form = Form::new();
    if input {
        form.field(data.clone(), key);
    }
    for i in 1..4 {
        form.button(i, i == 1);
    }
    form.select(prompt.focus);
    let selected = match key.code {
        K::Esc => form.press(2),
        K::F(1) => form.press(3),
        K::Char('d' | 'D') if key.modifiers.contains(M::ALT) => {
            if input {
                form.select(0);
            }
            None
        }
        K::Char('k' | 'K') if key.modifiers.contains(M::ALT) => form.press(1),
        K::Char('c' | 'C') if key.modifiers.contains(M::ALT) => form.press(2),
        K::Char('h' | 'H') if key.modifiers.contains(M::ALT) => form.press(3),
        K::Tab | K::BackTab => form.event(Event::keyboard(
            if key.code == K::BackTab || key.modifiers.contains(M::SHIFT) {
                KB_SHIFT_TAB
            } else {
                KB_TAB
            },
        )),
        K::Enter => form.event(Event::keyboard(KB_ENTER)),
        _ if prompt.focus == 0 => form.event(Event::text(match key.code {
            K::Char(c) => c,
            _ => '\0',
        })),
        _ => None,
    };
    if let Some(Prompt {
        field,
        cursor,
        focus,
        ..
    }) = app.modals.prompt_mut()
    {
        let mut data = data.borrow_mut();
        *field = std::mem::take(&mut data.text);
        *cursor = data.cursor;
        *focus = form.selected();
    }
    match selected {
        Some(1) => app.submit_prompt(),
        Some(2) => app.modals.set_prompt(None),
        Some(3) => crate::dialog::prompt_help(app),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_form_focus_and_default_commands_are_synchronous_and_bounded() {
        let previous = button::press_animation();
        let mut form = Form::new();
        let data = Rc::new(RefCell::new(FieldData::default()));
        form.field(data, KeyEvent::new(K::Char('x'), M::NONE));
        for i in 1..4 {
            form.button(i, i == 1);
        }
        form.select(0);
        assert_eq!(form.event(Event::keyboard(KB_ENTER)), Some(1));
        assert_eq!(form.selected(), 0);
        for selected in [1, 2, 3, 0] {
            assert_eq!(form.event(Event::keyboard(KB_TAB)), None);
            assert_eq!(form.selected(), selected);
        }
        assert_eq!(form.event(Event::keyboard(KB_SHIFT_TAB)), None);
        assert_eq!(form.selected(), 3);
        assert_eq!(form.event(Event::keyboard(KB_ENTER)), Some(3));
        assert_eq!(button::press_animation(), previous);
    }
    #[test]
    fn native_field_accepts_wide_combining_private_and_non_latin_text() {
        let data = Rc::new(RefCell::new(FieldData::default()));
        let mut field = Field {
            core: ViewCore::new(Rect::new(0, 0, 20, 1)),
            data: data.clone(),
            key: KeyEvent::new(K::Null, M::NONE),
        };
        for ch in "界е\u{301}👩\u{200d}💻\u{f0000}".chars() {
            field.key = KeyEvent::new(K::Char(ch), M::NONE);
            let mut event = Event::text(ch);
            field.handle_event(&mut event);
            assert_eq!(event.what, EventType::Nothing);
        }
        assert_eq!(data.borrow().text, "界е\u{301}👩\u{200d}💻\u{f0000}");
        assert_eq!(data.borrow().cursor, data.borrow().text.len());
        field.key = KeyEvent::new(K::Backspace, M::NONE);
        field.handle_event(&mut Event::text('\0'));
        assert_eq!(data.borrow().text, "界е\u{301}👩\u{200d}💻");
        field.key = KeyEvent::new(K::Home, M::NONE);
        field.handle_event(&mut Event::text('\0'));
        field.key = KeyEvent::new(K::Delete, M::NONE);
        field.handle_event(&mut Event::text('\0'));
        assert_eq!(data.borrow().text, "е\u{301}👩\u{200d}💻");
        assert_eq!(data.borrow().cursor, 0);
        field.key = KeyEvent::new(K::Char('q'), M::CONTROL);
        field.handle_event(&mut Event::text('q'));
        assert_eq!(data.borrow().cursor, 0);
    }
}
