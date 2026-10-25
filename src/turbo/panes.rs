//! Persistent native pane controls. TV Group owns Tab focus and sends navigation
//! commands from the focused pane; workers and filesystem policy stay in models.
use crate::{
    app::App,
    panel_command::{self, Command},
};
use crossterm::event::{KeyCode as K, KeyEvent, KeyModifiers as M};
use std::{cell::RefCell, rc::Rc};
use turbo_vision::{
    core::{event::*, geometry::Rect},
    terminal::Terminal,
    views::{group::Group, GroupLike, View, ViewCore},
};
const RECEIPT: u16 = 0xff01;
type Mailbox = Rc<RefCell<Option<(usize, Command)>>>;
struct Pane {
    core: ViewCore,
    index: usize,
    mailbox: Mailbox,
}
impl View for Pane {
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
    fn get_palette(&self) -> Option<turbo_vision::core::palette::Palette> {
        None
    }
    fn can_focus(&self) -> bool {
        true
    }
    fn draw(&mut self, _: &mut Terminal) {} // The shared native scene supplies the skin.
    fn handle_event(&mut self, event: &mut Event) {
        let command = if event.what == EventType::Command && event.command == RECEIPT {
            Some(Command::Focus)
        } else if event.what == EventType::Keyboard {
            match event.key_code {
                KB_UP => Some(Command::Step(-1)),
                KB_DOWN => Some(Command::Step(1)),
                KB_LEFT => Some(Command::Column(false)),
                KB_RIGHT => Some(Command::Column(true)),
                KB_PGUP => Some(Command::Page(false)),
                KB_PGDN => Some(Command::Page(true)),
                KB_HOME => Some(Command::First),
                KB_END => Some(Command::Last),
                KB_ENTER => Some(Command::Open),
                KB_BACKSPACE => Some(Command::Parent),
                KB_INS | 0x20 => Some(Command::Mark),
                _ => None,
            }
        } else {
            None
        };
        if let Some(command) = command {
            *self.mailbox.borrow_mut() = Some((self.index, command));
            event.clear();
        }
    }
}
pub(super) struct Controls {
    group: Group,
    mailbox: Mailbox,
}
impl Default for Controls {
    fn default() -> Self {
        let mailbox = Rc::new(RefCell::new(None));
        let mut group = Group::new(Rect::new(0, 0, 80, 25));
        for index in 0..2 {
            group.add(Pane {
                core: ViewCore::new(Rect::new(index as i16 * 40, 1, (index as i16 + 1) * 40, 23)),
                index,
                mailbox: mailbox.clone(),
            });
        }
        group.set_initial_focus();
        Self { group, mailbox }
    }
}
impl Controls {
    pub(super) fn sync(&mut self, active: usize, size: (u16, u16)) {
        let w = size.0.min(i16::MAX as u16) as i16;
        let h = size.1.min(i16::MAX as u16) as i16;
        self.group.set_bounds(Rect::new(0, 0, w, h));
        for i in 0..2 {
            self.group.child_at_mut(i).set_bounds(Rect::new(
                i as i16 * (w / 2),
                1,
                if i == 0 { w / 2 } else { w },
                (h - 2).max(1),
            ));
        }
        // External activation (mouse/menu/controller commands) enters through the
        // same native focus boundary before the next draw or keyboard event.
        self.group.set_focus_to(active.min(1));
    }
    pub(super) fn key(
        &mut self,
        active: usize,
        size: (u16, u16),
        key: KeyEvent,
    ) -> Option<(usize, Command)> {
        let code = match key.code {
            K::Tab if key.modifiers.contains(M::SHIFT) => KB_SHIFT_TAB,
            K::Tab => KB_TAB,
            K::BackTab => KB_SHIFT_TAB,
            K::Up => KB_UP,
            K::Down => KB_DOWN,
            K::Left => KB_LEFT,
            K::Right => KB_RIGHT,
            K::PageUp => KB_PGUP,
            K::PageDown => KB_PGDN,
            K::Home => KB_HOME,
            K::End => KB_END,
            K::Enter => KB_ENTER,
            K::Backspace => KB_BACKSPACE,
            K::Insert => KB_INS,
            K::Char(' ') => 0x20,
            _ => return None,
        };
        self.sync(active, size);
        *self.mailbox.borrow_mut() = None;
        self.group.handle_event(&mut Event::keyboard(code));
        if matches!(code, KB_TAB | KB_SHIFT_TAB) {
            self.group.handle_event(&mut Event::command(RECEIPT));
        }
        self.mailbox.borrow_mut().take()
    }
}
pub(crate) fn key(app: &mut App, key: KeyEvent) -> bool {
    if key.code == K::Char(' ') && !app.config.space_select {
        return false;
    }
    if key.code == K::Backspace && !app.config.backspace_parent {
        return false;
    }
    let command = app
        .desktop
        .native()
        .pane_key(app.active, app.utilities.size, key);
    if let Some((index, command)) = command {
        panel_command::apply(app, index, command);
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_focus_cycles_and_commands_name_the_selected_pane() {
        let mut controls = Controls::default();
        let key = |code| KeyEvent::new(code, M::NONE);
        assert_eq!(
            controls.key(0, (80, 25), key(K::Tab)),
            Some((1, Command::Focus))
        );
        assert_eq!(
            controls.key(1, (132, 40), key(K::BackTab)),
            Some((0, Command::Focus))
        );
        assert_eq!(
            controls.key(1, (132, 40), key(K::Down)),
            Some((1, Command::Step(1)))
        );
        assert_eq!(
            controls.key(0, (50, 12), key(K::PageUp)),
            Some((0, Command::Page(false)))
        );
        assert_eq!(controls.key(0, (80, 25), key(K::F(5))), None);
        assert_eq!(
            controls.key(1, (80, 25), key(K::Insert)),
            Some((1, Command::Mark))
        );
    }
}
