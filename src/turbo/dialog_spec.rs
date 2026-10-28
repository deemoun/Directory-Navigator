//! One definition for dialog painting, native traversal and semantic hits.
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
    SelectedInput(String),
    Surface,
    Outline,
    Memo(String),
    Menu {
        items: Vec<super::widgets::MenuEntry>,
        cursor: usize,
    },
    ScrolledInput {
        value: String,
        scroll: usize,
    },
    Checkbox {
        caption: String,
        checked: bool,
    },
    Radio {
        caption: String,
        checked: bool,
    },
    History,
    CharacterGrid {
        page: crate::utilities::characters::Page,
        cells: [u8; 2],
    },
    List {
        items: Vec<String>,
        cursor: usize,
        offset: usize,
    },
    Choice(Vec<String>),
}
pub(crate) struct Control {
    pub id: usize,
    pub bounds: Rect,
    pub kind: ControlKind,
    pub enabled: bool,
    pub paint: bool,
    pub enter_action: bool,
    pub space_default: bool,
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
            paint: true,
            enter_action: false,
            space_default: false,
            focus_visible: true,
            action,
            hits: vec![(bounds, action)],
            mnemonic: None,
            fallback_mnemonic: None,
        }
    }
    pub fn label_keys(&mut self, language: crate::localization::Language, source: &str) {
        self.mnemonic = crate::menu::mnemonic(language.text(source));
        self.fallback_mnemonic = crate::menu::mnemonic(source);
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
            ControlKind::SelectedInput(text) => super::widgets::selected_input(text, focused),
            ControlKind::Surface => super::widgets::surface(),
            ControlKind::Memo(text) => super::widgets::memo(text),
            ControlKind::Outline => super::widgets::outline_focus(),
            ControlKind::Menu { items, cursor } => super::widgets::menu(items.clone(), *cursor),
            ControlKind::ScrolledInput { value, scroll } => {
                super::widgets::input_scrolled(value, *scroll, focused)
            }
            ControlKind::Checkbox { caption, checked } => {
                super::widgets::checkbox(caption, *checked, focused, self.enabled)
            }
            ControlKind::Radio { caption, checked } => {
                super::widgets::radio_enabled(caption, *checked, focused, self.enabled)
            }
            ControlKind::CharacterGrid { page, cells } => super::widgets::native(
                super::characters::table(&crate::utilities::characters::Chart {
                    page: *page,
                    cells: *cells,
                    focus: 0,
                }),
                super::widgets::TextCells::default(),
            ),
            ControlKind::History => super::widgets::history_button(),
            ControlKind::List {
                items,
                cursor,
                offset,
            } => super::widgets::list(items.clone(), *cursor, *offset, focused),
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
    Native(super::widgets::Native, Rect),
    Hit(Rect, Target),
    Scene(super::scene::Scene),
    Overlay(Box<DialogSpec<'a>>),
}
#[derive(Clone, Copy, Debug)]
pub(crate) enum ControlScope {
    Modal(ModalInfo),
    Document {
        window: crate::desktop::WindowId,
        revision: u64,
    },
}
impl ControlScope {
    fn modal(self) -> Option<ModalInfo> {
        if let Self::Modal(m) = self {
            Some(m)
        } else {
            None
        }
    }
    #[cfg(test)]
    fn owner(self) -> crate::modals::ModalOwner {
        match self {
            Self::Modal(m) => m.owner,
            Self::Document { window, .. } => crate::modals::ModalOwner::Document(window),
        }
    }
}
pub(crate) struct DialogSpec<'a> {
    pub bounds: Rect,
    pub scope: ControlScope,
    pub focus: usize,
    pub default: Target,
    pub cancel: Target,
    pub close_action: Target,
    title: String,
    pub controls: Vec<Control>,
    parts: Vec<Part<'a>>,
    pub cursor: Option<(u16, u16)>,
    pub close_visible: bool,
    pub tab_order: Vec<usize>,
}
#[derive(Debug)]
pub(crate) struct Proposal {
    pub scope: ControlScope,
    pub focus: usize,
    pub action: Option<Target>,
}
impl Proposal {
    /// The shared ownership gate: focus and action are delivered together only
    /// while the originating modal/document is still the foreground owner.
    pub fn deliver(
        self,
        app: &mut App,
        apply: impl FnOnce(&mut App, usize, Option<Target>),
    ) -> bool {
        if !self.current(app) {
            return false;
        }
        apply(app, self.focus, self.action);
        true
    }
    pub fn current(&self, app: &App) -> bool {
        match self.scope {
            ControlScope::Modal(scope) => app
                .modal_top()
                .is_some_and(|m| m.id == scope.id && m.owner == scope.owner),
            ControlScope::Document { window, revision } => {
                app.desktop.contains(window)
                    && app.desktop.active().id == window
                    && app.layout_revision == revision
                    && app.modal_top().is_none()
            }
        }
    }
}
pub(crate) fn scope(app: &App, focus: Focus) -> Option<ModalInfo> {
    // Settings' choice child uses the same input scope. The first entry is the
    // parent whose controls this definition describes, not its owned child.
    app.visible_modals().into_iter().find(|m| m.focus == focus)
}
pub(crate) fn centered(area: Rect, width: u16, height: u16) -> Rect {
    crate::dialog_theme::centered(area, width, height)
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
        Self::owned(
            bounds,
            ControlScope::Modal(scope),
            focus,
            title,
            default,
            cancel,
        )
    }
    pub(crate) fn owned(
        bounds: Rect,
        scope: ControlScope,
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
            close_action: cancel,
            controls: vec![],
            parts: vec![],
            cursor: None,
            close_visible: true,
            tab_order: vec![],
        }
    }
    pub fn caption(&mut self, paragraph: Paragraph<'a>, bounds: Rect) {
        self.parts.push(Part::Caption(paragraph, bounds));
    }
    pub fn memo(&mut self, text: &str, bounds: Rect) {
        self.parts.push(Part::Memo(text.into(), bounds));
    }
    pub fn native(&mut self, widget: super::widgets::Native, bounds: Rect) {
        self.parts.push(Part::Native(widget, bounds));
    }
    pub fn overlay(&mut self, child: DialogSpec<'a>) {
        self.parts.push(Part::Overlay(Box::new(child)));
    }
    pub fn scene(&mut self, scene: super::scene::Scene) {
        self.parts.push(Part::Scene(scene));
    }
    pub fn hit(&mut self, bounds: Rect, target: Target) {
        self.parts.push(Part::Hit(bounds, target));
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
        if self
            .controls
            .iter()
            .any(|c| c.id == self.focus && !c.enabled)
        {
            return None;
        }
        self.controls
            .iter()
            .find(|c| c.id == self.focus && c.enabled)
            // Corrupt/stale focus must retain the declared default, especially
            // Cancel in unsaved/replacement guards, rather than choosing Save.
            .or_else(|| {
                self.controls.iter().find(|c| {
                    c.enabled
                        && c.action == self.default
                        && matches!(c.kind, ControlKind::Button(_) | ControlKind::History)
                })
            })
            .or_else(|| {
                self.controls
                    .iter()
                    .find(|c| c.enabled && c.action == self.cancel)
            })
            .or_else(|| self.controls.iter().find(|c| c.enabled))
    }
    fn ordered(&self) -> Vec<&Control> {
        if self.tab_order.is_empty() {
            self.controls.iter().collect()
        } else {
            self.tab_order
                .iter()
                .filter_map(|id| self.controls.iter().find(|c| c.id == *id))
                .collect()
        }
    }
    fn default_action(&self) -> Option<Target> {
        self.controls
            .iter()
            .find(|c| {
                c.enabled
                    && c.action == self.default
                    && matches!(c.kind, ControlKind::Button(_) | ControlKind::History)
            })
            .map(|c| c.action)
    }
    fn action(&self, target: Target) -> Option<Target> {
        self.controls
            .iter()
            .find(|c| c.enabled && c.action == target)
            .map(|c| c.action)
    }
    /// Capabilities and hit geometry come from the same definition as painting.
    /// Async enable/disable changes retire the previously published frame.
    pub fn interaction_state(&self) -> Vec<(bool, Rect, Target)> {
        let mut state = self
            .controls
            .iter()
            .flat_map(|c| c.hits.iter().map(|(r, t)| (c.enabled, *r, *t)))
            .collect::<Vec<_>>();
        for part in &self.parts {
            if let Part::Hit(r, t) = part {
                state.push((true, *r, *t));
            }
        }
        // Radio groups move their native focus widget between selected rows.
        // Hit semantics stay the same: ignore descriptor/decorator insertion order.
        state.sort_by_key(|(enabled, r, _)| (r.x, r.y, r.width, r.height, *enabled));
        state
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
        let mut result = self.proposal(None);
        if key.code == KeyCode::Esc {
            result.action = if self.close_visible {
                Some(self.cancel)
            } else {
                self.action(self.cancel)
            };
            return Some(result);
        }
        let selected = match self.selected() {
            Some(c) => c,
            None => {
                if matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
                    let ordered = self.ordered();
                    let mut choices = ordered.iter().filter(|c| c.enabled);
                    let selected = if key.code == KeyCode::BackTab
                        || key.modifiers.contains(KeyModifiers::SHIFT)
                    {
                        choices.next_back()
                    } else {
                        choices.next()
                    };
                    result.focus = selected.map_or(self.focus, |c| c.id);
                    return Some(result);
                }
                return matches!(key.code, KeyCode::Enter | KeyCode::Char(' ')).then_some(result);
            }
        };
        match key.code {
            KeyCode::Tab | KeyCode::BackTab => {
                let mut group = Group::new(TvRect::new(
                    0,
                    0,
                    self.bounds.width as i16,
                    self.bounds.height as i16,
                ));
                let mut ids = Vec::new();
                let ordered = self.ordered();
                for control in &ordered {
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
                group.set_focus_to(ordered.iter().position(|c| c.id == selected.id).unwrap());
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
                    result.focus = ordered[i].id;
                }
            }
            KeyCode::Enter => {
                result.action = if selected.enter_action
                    || matches!(selected.kind, ControlKind::Button(_) | ControlKind::History)
                {
                    Some(selected.action)
                } else {
                    self.default_action()
                }
            }
            KeyCode::Esc => result.action = self.action(self.cancel),
            KeyCode::Char(' ')
                if selected.space_default
                    || !matches!(
                        selected.kind,
                        ControlKind::Input(_)
                            | ControlKind::SelectedInput(_)
                            | ControlKind::ScrolledInput { .. }
                            | ControlKind::Memo(_)
                            | ControlKind::List { .. }
                            | ControlKind::Menu { .. }
                            | ControlKind::Outline
                            | ControlKind::Surface
                    ) =>
            {
                result.action = if selected.space_default {
                    self.default_action()
                } else {
                    Some(selected.action)
                }
            }
            _ => return None,
        }
        Some(result)
    }
    pub fn paint(self, frame: &mut impl Frame, app: &App) {
        frame.modal_scope(self.scope.modal());
        frame.render_widget(super::widgets::dialog(&self.title), self.bounds);
        frame.regions().clear();
        // Existing native title chrome is drawn by the standard close helper.
        if self.close_visible {
            crate::ui::paint_spec_close(frame, app, self.bounds, self.close_action);
        }
        self.append(frame);
    }
    /// Paint controls over a custom document surface without replacing its board.
    pub fn append(self, frame: &mut impl Frame) {
        let focused = self.selected().map(|c| c.id);
        let button_focus = self
            .selected()
            .is_some_and(|c| matches!(c.kind, ControlKind::Button(_) | ControlKind::History));
        for part in self.parts {
            match part {
                Part::Caption(text, bounds) => frame.render_widget(text, bounds),
                Part::Memo(text, bounds) => {
                    frame.render_widget(super::widgets::memo(&text), bounds)
                }
                Part::Native(widget, bounds) => frame.render_widget(widget, bounds),
                Part::Scene(scene) => frame.compose(scene),
                Part::Overlay(child) => {
                    frame.render_widget(super::widgets::dialog(&child.title), child.bounds);
                    frame.regions().clear();
                    if child.close_visible {
                        frame.regions().push((
                            Rect::new(child.bounds.x + 2, child.bounds.y, 3, 1),
                            child.close_action,
                        ));
                    }
                    child.append(frame);
                }
                Part::Hit(bounds, target) => frame.regions().push((bounds, target)),
                Part::Control(index) => {
                    let control = &self.controls[index];
                    let default = control.enabled
                        && control.action == self.default
                        && focused.is_some()
                        && !button_focus;
                    if control.paint {
                        frame.render_widget(
                            control.native(focused == Some(control.id), default),
                            control.bounds,
                        );
                    }
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
    fn disabled_defaults_cannot_be_reenabled_by_a_list_with_the_same_action() {
        let mut s = mixed();
        s.controls[0].kind = ControlKind::List {
            items: vec![],
            cursor: 0,
            offset: 0,
        };
        s.controls[0].action = Target::AcceptSettings;
        s.controls[4].enabled = false;
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
        assert_eq!(s.key(key(KeyCode::Enter)).unwrap().action, None);
        s.focus = 4;
        assert_eq!(s.key(key(KeyCode::Enter)).unwrap().action, None);
        assert_eq!(s.key(key(KeyCode::Tab)).unwrap().focus, 0);
        assert_eq!(s.key(key(KeyCode::BackTab)).unwrap().focus, 5);
    }
    #[test]
    fn document_control_proposals_reject_page_revisions_switches_and_retirement(
    ) -> std::io::Result<()> {
        let root =
            std::env::temp_dir().join(format!("dn-document-controls-{}", std::process::id()));
        std::fs::create_dir(&root)?;
        let result = (|| {
            let mut app = App::new(root.clone())?;
            crate::tetris::open(&mut app);
            let proposal = crate::plugins::host::controls(&app, input_area(&app))
                .unwrap()
                .proposal(None);
            assert!(proposal.current(&app));
            app.layout_revision += 1;
            assert!(!proposal.current(&app));
            let proposal = crate::plugins::host::controls(&app, input_area(&app))
                .unwrap()
                .proposal(None);
            app.error("Owned child");
            assert!(!proposal.current(&app));
            app.pop_dialog();
            assert!(proposal.current(&app));
            let id = app.desktop.active().id;
            app.desktop.activate(WindowId::MANAGER);
            assert!(!proposal.current(&app));
            app.desktop.activate(id);
            assert!(proposal.current(&app));
            app.desktop.close(id);
            assert!(!proposal.current(&app));
            crate::tetris::open(&mut app);
            assert_ne!(app.desktop.active().id, id);
            assert!(!proposal.current(&app));
            Ok(())
        })();
        std::fs::remove_dir_all(root)?;
        result
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
            assert!(!proposal.deliver(&mut app, |_, _, _| panic!("retired proposal applied")));
            let live = crate::ui::prompt_spec(&app, input_area(&app))
                .unwrap()
                .proposal(Some(Target::PromptHelp));
            assert!(live.deliver(&mut app, |app, _, action| {
                assert_eq!(action, Some(Target::PromptHelp));
                app.notice = "live proposal".into();
            }));
            assert_eq!(app.notice, "live proposal");
            let next = crate::ui::prompt_spec(&app, input_area(&app))
                .unwrap()
                .proposal(Some(Target::SubmitPrompt));
            app.modals.retire(next.scope.owner());
            assert!(!next.current(&app));
            assert!(!next.deliver(&mut app, |_, _, _| panic!(
                "retired owner received an action"
            )));
            Ok(())
        })();
        std::fs::remove_dir_all(root)?;
        result
    }
}
