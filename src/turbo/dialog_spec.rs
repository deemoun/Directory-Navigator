//! One definition for pilot dialog painting, native traversal and semantic hits.
//! Values remain controller-owned; native events only propose focus/actions.
use crate::{
    app::App,
    focus::Focus,
    modals::ModalInfo,
    mouse::Target,
    painting::{Frame, Paragraph},
    presentation::layout::Rect,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use turbo_vision::{
    core::{
        event::{Event, KB_SHIFT_TAB, KB_TAB},
        geometry::Rect as TvRect,
        state::{Options, State},
    },
    views::{group::Group, View},
};

pub(crate) enum ControlKind {
    Button(String),
    Input(String),
    ScrolledInput { value: String, scroll: usize },
    Checkbox { caption: String, checked: bool },
    Choice(Vec<String>),
}
pub(crate) struct Control {
    pub id: usize,
    pub bounds: Rect,
    pub kind: ControlKind,
    pub enabled: bool,
    pub focus_visible: bool,
    pub action: Target,
    pub hits: Vec<(Rect, Target)>,
    pub mnemonic: Option<char>,
    pub fallback_mnemonic: Option<char>,
}
impl Control {
    pub fn new(id: usize, bounds: Rect, kind: ControlKind, action: Target) -> Self {
        Self {
            id,
            bounds,
            kind,
            enabled: true,
            focus_visible: true,
            action,
            hits: vec![(bounds, action)],
            mnemonic: None,
            fallback_mnemonic: None,
        }
    }
    fn native(&self, focused: bool, default: bool) -> super::widgets::Native {
        let focused = focused && self.focus_visible;
        let mut widget = match &self.kind {
            ControlKind::Button(label) => {
                if self.enabled {
                    super::widgets::button_with_default(label, focused, default)
                } else {
                    super::widgets::disabled_button(label)
                }
            }
            ControlKind::Input(text) => super::widgets::input(text, focused),
            ControlKind::ScrolledInput { value, scroll } => {
                super::widgets::input_scrolled(value, *scroll, focused)
            }
            ControlKind::Checkbox { caption, checked } => {
                super::widgets::checkbox(caption, *checked, focused, self.enabled)
            }
            ControlKind::Choice(items) => super::widgets::combo(items, 0, focused),
        };
        widget.view.set_state_flag(State::DISABLED, !self.enabled);
        widget
    }
}
enum Part<'a> {
    Caption(Paragraph<'a>, Rect),
    Memo(String, Rect),
    Control(usize),
}
pub(crate) struct DialogSpec<'a> {
    pub bounds: Rect,
    pub scope: ModalInfo,
    pub focus: usize,
    pub default: Target,
    pub cancel: Target,
    title: String,
    pub controls: Vec<Control>,
    parts: Vec<Part<'a>>,
    pub cursor: Option<(u16, u16)>,
}
#[derive(Debug)]
pub(crate) struct Proposal {
    pub scope: ModalInfo,
    pub focus: usize,
    pub action: Option<Target>,
}
impl Proposal {
    pub fn current(&self, app: &App) -> bool {
        app.modal_top()
            .is_some_and(|m| m.id == self.scope.id && m.owner == self.scope.owner)
    }
}
pub(crate) fn scope(app: &App, focus: Focus) -> Option<ModalInfo> {
    // Settings' choice child uses the same input scope. The first entry is the
    // parent whose controls this definition describes, not its owned child.
    app.visible_modals().into_iter().find(|m| m.focus == focus)
}
pub(crate) fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}
pub(crate) fn input_area(app: &App) -> Rect {
    // Keyboard input can arrive before the first frame. Geometry is also valid
    // then; small-terminal presentation remains suppressed by the screen builder.
    Rect::new(
        0,
        0,
        app.utilities.size.0.max(80),
        app.utilities.size.1.max(25),
    )
}
impl<'a> DialogSpec<'a> {
    pub fn new(
        bounds: Rect,
        scope: ModalInfo,
        focus: usize,
        title: &str,
        default: Target,
        cancel: Target,
    ) -> Self {
        Self {
            bounds,
            scope,
            focus,
            title: title.into(),
            default,
            cancel,
            controls: vec![],
            parts: vec![],
            cursor: None,
        }
    }
    pub fn caption(&mut self, paragraph: Paragraph<'a>, bounds: Rect) {
        self.parts.push(Part::Caption(paragraph, bounds));
    }
    pub fn memo(&mut self, text: &str, bounds: Rect) {
        self.parts.push(Part::Memo(text.into(), bounds));
    }
    pub fn control(&mut self, control: Control) {
        assert!(
            !self.controls.iter().any(|c| c.id == control.id),
            "duplicate dialog control"
        );
        self.parts.push(Part::Control(self.controls.len()));
        self.controls.push(control);
    }
    fn selected(&self) -> Option<&Control> {
        self.controls
            .iter()
            .find(|c| c.id == self.focus && c.enabled)
            // Corrupt/stale focus must retain the declared default, especially
            // Cancel in unsaved/replacement guards, rather than choosing Save.
            .or_else(|| {
                self.controls.iter().find(|c| {
                    c.enabled
                        && c.action == self.default
                        && matches!(c.kind, ControlKind::Button(_))
                })
            })
            .or_else(|| {
                self.controls
                    .iter()
                    .find(|c| c.enabled && c.action == self.cancel)
            })
            .or_else(|| self.controls.iter().find(|c| c.enabled))
    }
    fn action(&self, target: Target) -> Option<Target> {
        self.controls
            .iter()
            .find(|c| c.enabled && c.action == target)
            .map(|c| c.action)
    }
    pub fn mnemonic(&self, ch: char) -> Option<usize> {
        self.controls
            .iter()
            .find(|c| {
                c.enabled
                    && (c.mnemonic == ch.to_lowercase().next()
                        || c.fallback_mnemonic == ch.to_lowercase().next())
            })
            .map(|c| c.id)
    }
    pub fn proposal(&self, action: Option<Target>) -> Proposal {
        Proposal {
            scope: self.scope,
            focus: self.selected().map_or(self.focus, |c| c.id),
            action: action.and_then(|a| self.action(a)),
        }
    }
    pub fn activate(&self, id: usize) -> Proposal {
        let action = self
            .controls
            .iter()
            .find(|c| c.id == id && c.enabled)
            .map(|c| c.action);
        self.proposal(action)
    }
    pub fn key(&self, key: KeyEvent) -> Option<Proposal> {
        let selected = self.selected()?;
        let mut result = self.proposal(None);
        match key.code {
            KeyCode::Tab | KeyCode::BackTab => {
                let mut group = Group::new(TvRect::new(
                    0,
                    0,
                    self.bounds.width as i16,
                    self.bounds.height as i16,
                ));
                let mut ids = Vec::new();
                for control in &self.controls {
                    let mut widget = control.native(control.id == selected.id, false);
                    if control.enabled {
                        widget.view.core_mut().options |= Options::SELECTABLE;
                    } else {
                        widget.view.core_mut().options &= !Options::SELECTABLE;
                    }
                    let r = control.bounds;
                    widget.view.set_bounds(TvRect::new(
                        (r.x - self.bounds.x) as i16,
                        (r.y - self.bounds.y) as i16,
                        (r.right() - self.bounds.x) as i16,
                        (r.bottom() - self.bounds.y) as i16,
                    ));
                    ids.push(group.add_boxed(widget.view));
                }
                group.set_focus_to(
                    self.controls
                        .iter()
                        .position(|c| c.id == selected.id)
                        .unwrap(),
                );
                group.handle_event(&mut Event::keyboard(
                    if key.code == KeyCode::BackTab || key.modifiers.contains(KeyModifiers::SHIFT) {
                        KB_SHIFT_TAB
                    } else {
                        KB_TAB
                    },
                ));
                if let Some(i) = ids
                    .iter()
                    .position(|id| Some(*id) == group.focused_view_id())
                {
                    result.focus = self.controls[i].id;
                }
            }
            KeyCode::Enter => {
                result.action = if matches!(selected.kind, ControlKind::Button(_)) {
                    Some(selected.action)
                } else {
                    self.action(self.default)
                }
            }
            KeyCode::Esc => result.action = self.action(self.cancel),
            KeyCode::Char(' ')
                if !matches!(
                    selected.kind,
                    ControlKind::Input(_) | ControlKind::ScrolledInput { .. }
                ) =>
            {
                result.action = Some(selected.action)
            }
            _ => return None,
        }
        Some(result)
    }
    pub fn paint(self, frame: &mut impl Frame, app: &App) {
        frame.modal_scope(Some(self.scope));
        frame.render_widget(super::widgets::dialog(&self.title), self.bounds);
        frame.regions().clear();
        // Existing native title chrome is drawn by the standard close helper.
        crate::ui::paint_spec_close(frame, app, self.bounds, self.cancel);
        let focused = self.selected().map(|c| c.id);
        let button_focus = self
            .selected()
            .is_some_and(|c| matches!(c.kind, ControlKind::Button(_)));
        for part in self.parts {
            match part {
                Part::Caption(text, bounds) => frame.render_widget(text, bounds),
                Part::Memo(text, bounds) => {
                    frame.render_widget(super::widgets::memo(&text), bounds)
                }
                Part::Control(index) => {
                    let control = &self.controls[index];
                    let default =
                        control.enabled && control.action == self.default && !button_focus;
                    frame.render_widget(
                        control.native(focused == Some(control.id), default),
                        control.bounds,
                    );
                    if control.enabled {
                        frame.regions().extend(control.hits.iter().copied());
                    }
                }
            }
        }
        if let Some(cursor) = self.cursor {
            frame.set_cursor_position(cursor);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        desktop::WindowId,
        modals::{ModalId, ModalOwner},
    };
    fn mixed() -> DialogSpec<'static> {
        let mut spec = DialogSpec::new(
            Rect::new(7, 3, 60, 15),
            ModalInfo {
                id: ModalId(1),
                owner: ModalOwner::Workspace,
                parent: None,
                return_document: Some(WindowId::MANAGER),
                focus: Focus::Settings,
            },
            0,
            "Test",
            Target::AcceptSettings,
            Target::CancelSettings,
        );
        for (id, kind, target) in [
            (
                0,
                ControlKind::Input("界e\u{301}".into()),
                Target::PromptField,
            ),
            (
                1,
                ControlKind::Checkbox {
                    caption: "~O~ption".into(),
                    checked: true,
                },
                Target::Setting(0),
            ),
            (
                2,
                ControlKind::Choice(vec!["Choice".into()]),
                Target::ShortcutProfile,
            ),
            (
                3,
                ControlKind::Button("Unavailable".into()),
                Target::PromptHelp,
            ),
            (4, ControlKind::Button("OK".into()), Target::AcceptSettings),
            (
                5,
                ControlKind::Button("Cancel".into()),
                Target::CancelSettings,
            ),
        ] {
            spec.control(Control::new(
                id,
                Rect::new(9, 5 + id as u16, 20, 1),
                kind,
                target,
            ));
        }
        spec.controls[3].enabled = false;
        spec
    }
    #[test]
    fn mixed_native_controls_wrap_skip_disabled_and_keep_enter_space_policy() {
        let mut spec = mixed();
        for expected in [1, 2, 4, 5, 0] {
            spec.focus = spec
                .key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
                .unwrap()
                .focus;
            assert_eq!(spec.focus, expected);
        }
        for expected in [5, 4, 2, 1, 0] {
            spec.focus = spec
                .key(KeyEvent::new(KeyCode::Tab, KeyModifiers::SHIFT))
                .unwrap()
                .focus;
            assert_eq!(spec.focus, expected);
        }
        let event = |code| KeyEvent::new(code, KeyModifiers::NONE);
        assert_eq!(
            spec.key(event(KeyCode::Enter)).unwrap().action,
            Some(Target::AcceptSettings)
        );
        assert!(spec.key(event(KeyCode::Char(' '))).is_none());
        spec.focus = 1;
        assert_eq!(
            spec.key(event(KeyCode::Char(' '))).unwrap().action,
            Some(Target::Setting(0))
        );
        assert_eq!(
            spec.key(event(KeyCode::Enter)).unwrap().action,
            Some(Target::AcceptSettings)
        );
        spec.focus = 5;
        assert_eq!(
            spec.key(event(KeyCode::Enter)).unwrap().action,
            Some(Target::CancelSettings)
        );
        spec.controls[4].enabled = false;
        spec.focus = 0;
        assert_eq!(spec.key(event(KeyCode::Enter)).unwrap().action, None);
        assert_eq!(spec.activate(4).action, None);
        assert_eq!(
            spec.key(event(KeyCode::Esc)).unwrap().action,
            Some(Target::CancelSettings)
        );
        spec.focus = 99;
        assert!(spec
            .controls
            .iter()
            .any(|c| c.enabled && c.id == spec.key(event(KeyCode::Tab)).unwrap().focus));
    }
    #[test]
    fn descriptor_factories_keep_real_control_types_and_disabled_state() {
        let spec = mixed();
        assert!(spec.controls[0]
            .native(true, false)
            .view
            .as_any()
            .is::<turbo_vision::views::input_line::InputLine>());
        assert!(spec.controls[1]
            .native(true, false)
            .view
            .as_any()
            .is::<turbo_vision::views::checkbox::CheckBox>());
        assert!(spec.controls[2]
            .native(true, false)
            .view
            .as_any()
            .is::<turbo_vision::views::ComboBox>());
        assert!(spec.controls[3]
            .native(false, false)
            .view
            .get_state_flag(State::DISABLED));
        assert!(spec.controls[4]
            .native(false, true)
            .view
            .as_any()
            .is::<turbo_vision::views::button::Button>());
    }
    #[test]
    fn proposals_reject_nested_replaced_and_retired_modal_owners() -> std::io::Result<()> {
        let root = std::env::temp_dir().join(format!("dn-spec-owner-{}", std::process::id()));
        std::fs::create_dir(&root)?;
        let result = (|| {
            let mut app = App::new(root.clone())?;
            app.prompt_goto();
            let proposal = crate::ui::prompt_spec(&app, input_area(&app))
                .unwrap()
                .proposal(Some(Target::SubmitPrompt));
            assert!(proposal.current(&app));
            app.error("Child blocks parent");
            assert!(!proposal.current(&app));
            app.pop_dialog();
            assert!(proposal.current(&app));
            app.modals.set_prompt(None);
            app.prompt_goto();
            assert!(!proposal.current(&app));
            let next = crate::ui::prompt_spec(&app, input_area(&app))
                .unwrap()
                .proposal(Some(Target::SubmitPrompt));
            app.modals.retire(next.scope.owner);
            assert!(!next.current(&app));
            Ok(())
        })();
        std::fs::remove_dir_all(root)?;
        result
    }
}
