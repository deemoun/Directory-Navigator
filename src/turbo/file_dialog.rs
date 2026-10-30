//! Owned native Open/Save As picker, driven by the application's single loop.
//! FileDialog chooses paths; DN controllers alone load and publish documents.
use super::scene::{Glyph, Scene};
use crate::{
    editor::EditorAction,
    localization::Language,
    mouse::Target,
    palette::PaletteProfile,
    presentation::{
        layout::{Rect, Size},
        style::{Modifier, Style},
    },
};
use crossterm::event::{
    KeyCode as K, KeyEvent, KeyModifiers as M, MouseButton, MouseEvent, MouseEventKind,
};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    time::{Duration, Instant},
};
use turbo_vision::{
    core::{
        command::{CM_CANCEL, CM_CLOSE, CM_OK},
        event::*,
        geometry::{Point, Rect as TvRect},
        palette::Style as TvStyle,
        state::State,
    },
    views::{
        button::{self, Button},
        checkbox::CheckBox,
        file_dialog::FileDialog,
        input_line::InputLine,
        label::Label,
        GroupLike, View, WindowLike,
    },
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Open,
    SaveAs,
}
pub(crate) enum Outcome {
    Selected(Mode, PathBuf),
    Cancelled,
}
pub struct Picker {
    pub mode: Mode,
    language: Language,
    widget: FileDialog,
    scene: Option<Scene>,
    size: Option<Size>,
    placement: Option<Rect>,
    pub(crate) revision: u64,
    last_click: Option<(Point, Instant)>,
}
impl Picker {
    pub(crate) fn new(mode: Mode, base: PathBuf, language: Language, hidden: bool) -> Self {
        let title = language.text(if mode == Mode::Open {
            "Open file"
        } else {
            "Save As — new destination"
        });
        let label = if mode == Mode::Open {
            "~O~pen"
        } else {
            "~S~ave"
        };
        let mut widget = FileDialog::new(TvRect::new(0, 0, 76, 20), title, "*", Some(base))
            .with_button_label(language.text(label))
            .with_show_hidden(hidden)
            .with_hidden_toggle(true)
            .build();
        for index in 0..widget.child_count() {
            let child = widget.child_at_mut(index);
            if let Some(label) = child.as_any_mut().downcast_mut::<Label>() {
                if label.text() == "~N~ame:" || label.text() == "~F~iles:" {
                    let text = language.text(label.text()).to_owned();
                    label.set_text(&text);
                }
            }
            if let Some(button) = child.as_any_mut().downcast_mut::<Button>() {
                let command = button.command();
                if command == CM_CANCEL || command == CM_OK {
                    let core = button.core().clone();
                    let default = button.is_default();
                    let text = language.text(if command == CM_CANCEL {
                        "~C~ancel"
                    } else {
                        label
                    });
                    *button = Button::new(button.bounds(), &format!(" {text}"), command, default);
                    *button.core_mut() = core;
                }
            }
            if let Some(checkbox) = child.as_any_mut().downcast_mut::<CheckBox>() {
                let mut core = checkbox.core().clone();
                core.bounds.b.x = 61;
                let checked = checkbox.is_checked();
                *checkbox = CheckBox::new(core.bounds, language.text("Show ~h~idden"));
                checkbox.set_embedded(true);
                checkbox.set_checked(checked);
                *checkbox.core_mut() = core;
            }
        }
        widget.set_state_flag(State::MODAL, true);
        widget.window_mut().set_resizable(false);
        let mut picker = Self {
            mode,
            language,
            widget,
            scene: None,
            size: None,
            placement: None,
            revision: 0,
            last_click: None,
        };
        picker.set_filename(String::new());
        picker
    }
    fn input_index(&self) -> usize {
        (0..self.widget.child_count())
            .find(|i| self.widget.child_at(*i).as_any().is::<InputLine>())
            .expect("FileDialog filename input")
    }
    pub fn filename(&self) -> String {
        self.widget
            .child_at(self.input_index())
            .as_any()
            .downcast_ref::<InputLine>()
            .unwrap()
            .get_text()
    }
    /// Set the native filename field; useful for callers preselecting a path.
    pub fn set_filename(&mut self, text: String) {
        let index = self.input_index();
        self.widget.group_mut().set_focus_to(index);
        let input = self
            .widget
            .child_at_mut(index)
            .as_any_mut()
            .downcast_mut::<InputLine>()
            .unwrap();
        input.set_max_length(16_384);
        input.set_text(text);
        self.update_buttons();
        self.changed();
    }
    pub fn directory(&self) -> PathBuf {
        self.widget.get_current_directory()
    }
    fn changed(&mut self) {
        self.revision = self
            .revision
            .checked_add(1)
            .expect("picker revision exhausted");
        self.scene = None;
    }
    fn update_buttons(&mut self) {
        let empty = self.filename().is_empty();
        for index in 0..self.widget.child_count() {
            if let Some(button) = self
                .widget
                .child_at_mut(index)
                .as_any_mut()
                .downcast_mut::<Button>()
            {
                if button.command() == CM_OK {
                    button.set_disabled(empty);
                }
            }
        }
    }
    fn dispatch(&mut self, mut event: Event) -> Option<Outcome> {
        struct Animation(Duration);
        impl Drop for Animation {
            fn drop(&mut self) {
                button::set_press_animation(self.0);
            }
        }
        let _animation = Animation(button::press_animation());
        button::set_press_animation(Duration::ZERO);
        self.widget.handle_event(&mut event);
        if event.what == EventType::Command {
            self.widget.handle_event(&mut event);
        }
        self.update_buttons();
        self.changed();
        match self.widget.end_state() {
            CM_OK => self
                .widget
                .get_selected_file()
                .map(|path| Outcome::Selected(self.mode, path)),
            CM_CANCEL | CM_CLOSE => Some(Outcome::Cancelled),
            _ => None,
        }
    }
    pub(crate) fn reset(&mut self) {
        self.widget.end_modal(0);
        self.changed();
    }
    pub(crate) fn submit(&mut self) -> Option<Outcome> {
        self.dispatch(Event::command(CM_OK))
    }
    pub(crate) fn key(&mut self, key: KeyEvent) -> Option<Outcome> {
        if let K::Char(c) = key.code {
            if !key.modifiers.intersects(M::CONTROL | M::ALT | M::SUPER)
                && !c.is_control()
                && Event::text(c).typed_char().is_none()
            {
                self.paste(&c.to_string());
                return None;
            }
        }
        let mut event = if key.modifiers.contains(M::ALT) {
            let K::Char(c) = key.code else {
                return None;
            };
            let action = if self.mode == Mode::Open {
                "~O~pen"
            } else {
                "~S~ave"
            };
            if self.language.matches(action, c) {
                Event::command(CM_OK)
            } else if self.language.matches("~C~ancel", c) {
                Event::command(CM_CANCEL)
            } else {
                let target = if self.language.matches("~N~ame:", c) {
                    Some(self.input_index())
                } else if self.language.matches("~F~iles:", c) {
                    (0..self.widget.child_count()).find(|i| {
                        self.widget
                            .child_at(*i)
                            .as_any()
                            .is::<turbo_vision::views::listbox::ListBox>()
                    })
                } else if self.language.matches("Show ~h~idden", c) {
                    (0..self.widget.child_count())
                        .find(|i| self.widget.child_at(*i).as_any().is::<CheckBox>())
                } else {
                    None
                };
                if let Some(index) = target {
                    self.widget.group_mut().set_focus_to(index);
                    self.changed();
                    if self.widget.child_at(index).as_any().is::<CheckBox>() {
                        return self.dispatch(Event::text(' '));
                    }
                    return None;
                }
                Event::keyboard(alt_code(c)?)
            }
        } else if key.modifiers.contains(M::CONTROL) {
            Event::keyboard(match key.code {
                K::Char('a') => KB_CTRL_A,
                _ => return None,
            })
        } else {
            match key.code {
                K::Char(c) if !c.is_control() => Event::text(c),
                code => Event::keyboard(match code {
                    K::Esc => KB_ESC,
                    K::Enter => KB_ENTER,
                    K::Tab if key.modifiers.contains(M::SHIFT) => KB_SHIFT_TAB,
                    K::Tab => KB_TAB,
                    K::BackTab => KB_SHIFT_TAB,
                    K::Backspace => KB_BACKSPACE,
                    K::Delete => KB_DEL,
                    K::Insert => KB_INS,
                    K::Up => KB_UP,
                    K::Down => KB_DOWN,
                    K::Left => KB_LEFT,
                    K::Right => KB_RIGHT,
                    K::PageUp => KB_PGUP,
                    K::PageDown => KB_PGDN,
                    K::Home => KB_HOME,
                    K::End => KB_END,
                    _ => return None,
                }),
            }
        };
        if key.modifiers.contains(M::SHIFT) {
            event
                .key_modifiers
                .insert(turbo_vision::core::keys::KeyModifiers::SHIFT);
        }
        self.dispatch(event)
    }
    pub(crate) fn paste(&mut self, text: &str) {
        if self
            .widget
            .group()
            .focused_child()
            .is_none_or(|v| !v.as_any().is::<InputLine>())
        {
            return;
        }
        let clean = text
            .chars()
            .filter(|c| !c.is_control())
            .take(4096)
            .collect::<String>();
        super::widgets::paste_native(&mut self.widget, &clean);
        self.update_buttons();
        self.changed();
    }

    pub(crate) fn mouse(&mut self, event: MouseEvent, offset: (u16, u16)) -> Option<Outcome> {
        let bounds = self.widget.bounds();
        let pos = Point::new(
            event.column as i16 - offset.0 as i16 - bounds.a.x,
            event.row as i16 - offset.1 as i16 - bounds.a.y,
        );
        let (kind, buttons) = match event.kind {
            MouseEventKind::Down(MouseButton::Left) => (EventType::MouseDown, MB_LEFT_BUTTON),
            MouseEventKind::Up(MouseButton::Left) => (EventType::MouseUp, 0),
            MouseEventKind::Drag(MouseButton::Left) => (EventType::MouseMove, MB_LEFT_BUTTON),
            MouseEventKind::ScrollUp => (EventType::MouseWheelUp, 0),
            MouseEventKind::ScrollDown => (EventType::MouseWheelDown, 0),
            _ => return None,
        };
        let now = Instant::now();
        let double = kind == EventType::MouseDown
            && self.last_click.is_some_and(|(last, time)| {
                last == pos && now.duration_since(time) < Duration::from_millis(400)
            });
        if kind == EventType::MouseDown {
            self.last_click = Some((pos, now));
        }
        self.dispatch(Event::mouse(kind, pos, buttons, double))
    }
    /// Native drawing happens during preparation; paint only clones this scene.
    pub(crate) fn prepare(&mut self, size: Size, profile: PaletteProfile) {
        if size.width < 80 || size.height < 25 {
            return;
        }
        if self.size != Some(size)
            || self.placement.is_some_and(|r| {
                self.widget.bounds()
                    != TvRect::new(r.x as i16, r.y as i16, r.right() as i16, r.bottom() as i16)
            })
        {
            let r = self.placement.unwrap_or(Rect::new(
                (size.width - 76) / 2,
                (size.height - 20) / 2,
                76,
                20,
            ));
            let x = r.x;
            let y = r.y;
            self.widget.set_bounds(TvRect::new(
                x as i16,
                y as i16,
                (x + r.width) as i16,
                (y + r.height) as i16,
            ));
            self.widget.window_mut().set_drag_limits(TvRect::new(
                0,
                0,
                size.width.min(i16::MAX as u16) as i16,
                size.height.min(i16::MAX as u16) as i16,
            ));
            self.size = Some(size);
            self.changed();
        }
        let mut terminal = super::surface::terminal(size.width, size.height);
        terminal.draw_view(&mut self.widget);
        terminal.push_origin(self.widget.bounds().a);
        self.widget.update_cursor(&mut terminal);
        terminal.pop_origin();
        let bounds = self.widget.bounds();
        let mut scene = Scene {
            width: size.width,
            height: size.height,
            profile,
            cells: BTreeMap::new(),
            regions: vec![(
                Rect::new(0, 0, size.width, size.height),
                Target::Editor(EditorAction::FileDialog),
            )],
            cursor: terminal.cursor(),
        };
        let rect = Rect::new(
            bounds.a.x.max(0) as u16,
            bounds.a.y.max(0) as u16,
            bounds.width_clamped() as u16,
            bounds.height_clamped() as u16,
        );
        for y in rect.y..rect.bottom().min(size.height) {
            for x in rect.x..rect.right().min(size.width) {
                let cell = terminal.read_cell(x as i16, y as i16).unwrap();
                let mut style = Style::default();
                for (tv, own) in [
                    (TvStyle::BOLD, Modifier::BOLD),
                    (TvStyle::ITALIC, Modifier::ITALIC),
                    (TvStyle::UNDERLINE, Modifier::UNDERLINED),
                    (TvStyle::REVERSE, Modifier::REVERSED),
                    (TvStyle::DIM, Modifier::DIM),
                    (TvStyle::STRIKETHROUGH, Modifier::CROSSED_OUT),
                ] {
                    if cell.attr.style.contains(tv) {
                        style = style.add_modifier(own);
                    }
                }
                scene.cells.insert(
                    (x, y),
                    Glyph {
                        text: if cell.ch == '\0' {
                            String::new()
                        } else if cell.ch.is_control() {
                            "�".into()
                        } else {
                            cell.ch.to_string()
                        },
                        attribute: cell.attr.to_u8(),
                        style: Some(style),
                    },
                );
            }
        }
        let focused_button = (0..self.widget.child_count()).any(|i| {
            let child = self.widget.child_at(i);
            child.as_any().is::<Button>() && child.is_focused()
        });
        for i in 0..self.widget.child_count() {
            let child = self.widget.child_at(i);
            if let Some(button) = child.as_any().downcast_ref::<Button>() {
                let b = child.bounds();
                for y in 0..b.height_clamped() as u16 {
                    for x in 0..b.width_clamped() as u16 {
                        let position = (
                            rect.x + 1 + b.a.x.max(0) as u16 + x,
                            rect.y + 1 + b.a.y.max(0) as u16 + y,
                        );
                        if let Some(glyph) = scene.cells.get_mut(&position) {
                            super::widgets::button_cell(
                                &mut glyph.text,
                                &mut glyph.attribute,
                                x,
                                y,
                                b.width_clamped() as u16,
                                b.height_clamped() as u16,
                                child.is_focused() || !focused_button && button.is_default(),
                            );
                        }
                    }
                }
            }
        }
        for glyph in scene.cells.values_mut() {
            if let Some(style) =
                profile.themed(crate::palette::theme::Group::Dialogs, glyph.attribute)
            {
                glyph.style = Some(style.patch(glyph.style.unwrap_or_default()));
            }
        }
        self.scene = Some(scene);
    }
    pub(crate) fn place(&mut self, r: Rect) {
        self.placement = Some(r);
    }
    pub(crate) fn bounds(&self) -> Rect {
        let b = self.widget.bounds();
        Rect::new(
            b.a.x.max(0) as u16,
            b.a.y.max(0) as u16,
            b.width_clamped() as u16,
            b.height_clamped() as u16,
        )
    }
    pub(crate) fn scene(&self) -> Option<&Scene> {
        self.scene.as_ref()
    }
}
