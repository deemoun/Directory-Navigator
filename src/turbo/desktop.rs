//! Native desktop ownership. Application models own their data; TV owns the
//! selected window, z-order and the modal input scope. Commands return to the
//! existing controllers before any close/save/filesystem action is performed.
use crate::{
    desktop::{WindowId, WindowInfo},
    focus::Focus,
};
use std::{cell::RefCell, rc::Rc};
use turbo_vision::{
    core::{
        draw::Cell,
        event::{Event, EventType},
        geometry::Rect,
        palette::Palette,
        state::State,
    },
    terminal::Terminal,
    views::{desktop::Desktop, window::Window, View, ViewCore, ViewId},
};

pub(super) const INPUT: u16 = 0xff00;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Content,
    Layer(Focus),
    Screensaver,
}
impl Scope {
    pub fn for_app(app: &crate::app::App) -> Self {
        if app.screensaver.active {
            return Self::Screensaver;
        }
        match crate::focus::current(app) {
            Focus::Manager | Focus::Editor | Focus::Viewer | Focus::Game => Self::Content,
            focus => Self::Layer(focus),
        }
    }
}

/// An input receipt is created by the view TV actually selected. Raw payloads
/// stay outside TV's 16-bit command/info fields (paste and native paths cannot
/// be truncated, and window identities must never be narrowed to u16).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Receipt {
    pub window: WindowInfo,
    pub scope: Scope,
}

type Mailbox = Rc<RefCell<Option<Receipt>>>;
struct Document {
    window: Window,
    info: WindowInfo,
    scope: Scope,
    mailbox: Mailbox,
    pixels: Vec<Cell>,
    native: Vec<(u16, u16, Cell)>,
}
impl Document {
    fn new(bounds: Rect, info: WindowInfo, scope: Scope, mailbox: Mailbox) -> Self {
        let mut window = Window::new(bounds, "");
        window.set_auto_close(false);
        if scope != Scope::Content {
            window.set_state_flag(State::MODAL, true);
        }
        Self {
            window,
            info,
            scope,
            mailbox,
            pixels: Vec::new(),
            native: Vec::new(),
        }
    }
}
impl View for Document {
    fn core(&self) -> &ViewCore {
        self.window.core()
    }
    fn core_mut(&mut self) -> &mut ViewCore {
        self.window.core_mut()
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
    fn set_focus(&mut self, focused: bool) {
        self.window.set_focus(focused);
        self.set_state_flag(State::FOCUSED, focused);
    }
    // DN's fixed full-screen views retain their tested skin. They are custom
    // TV views, not TV's stock blue frame. Native controls can replace pieces
    // without taking ownership of the editor buffer or operation guards.
    fn draw(&mut self, terminal: &mut Terminal) {
        let width = usize::try_from(self.bounds().width()).unwrap_or(0);
        if width == 0 {
            return;
        }
        for (i, cell) in self.pixels.iter().copied().enumerate() {
            terminal.write_cell((i % width) as i16, (i / width) as i16, cell);
        }
        for &(x, y, cell) in &self.native {
            terminal.write_cell(x as i16, y as i16, cell);
        }
    }
    fn handle_event(&mut self, event: &mut Event) {
        if event.what == EventType::Command && event.command == INPUT {
            *self.mailbox.borrow_mut() = Some(Receipt {
                window: self.info,
                scope: self.scope,
            });
            event.clear();
        }
    }
}

/// Shared only on the UI thread. No worker can mutate focus or paint a window.
#[derive(Clone)]
pub struct NativeDesktop(Rc<RefCell<Host>>);
struct Host {
    desktop: Desktop,
    ids: Vec<(WindowInfo, ViewId)>,
    modal: Option<(Scope, ViewId)>,
    mailbox: Mailbox,
    bounds: Rect,
    scene: Option<Rc<super::scene::Scene>>,
    panes: super::panes::Controls,
    mouse: super::mouse::Router,
}
impl NativeDesktop {
    pub fn new(manager: WindowInfo) -> Self {
        let bounds = Rect::new(0, 0, 80, 25);
        let mailbox = Rc::new(RefCell::new(None));
        let mut desktop = Desktop::new(bounds);
        let id = desktop.add(Document::new(
            bounds,
            manager,
            Scope::Content,
            mailbox.clone(),
        ));
        Self(Rc::new(RefCell::new(Host {
            desktop,
            ids: vec![(manager, id)],
            modal: None,
            mailbox,
            bounds,
            scene: None,
            panes: super::panes::Controls::default(),
            mouse: super::mouse::Router::default(),
        })))
    }
    pub(crate) fn set_scene(&self, scene: Option<super::scene::Scene>) {
        self.0.borrow_mut().scene = scene.map(Rc::new);
    }
    pub(super) fn scene(&self) -> Option<Rc<super::scene::Scene>> {
        self.0.borrow().scene.clone()
    }
    pub(crate) fn sync_panes(&self, active: usize, size: (u16, u16)) {
        self.0.borrow_mut().panes.sync(active, size);
    }
    pub(crate) fn pane_key(
        &self,
        active: usize,
        size: (u16, u16),
        key: crossterm::event::KeyEvent,
    ) -> Option<(usize, crate::panel_command::Command)> {
        self.0.borrow_mut().panes.key(active, size, key)
    }
    pub(crate) fn mouse_target(
        &self,
        regions: &[(ratatui::layout::Rect, crate::mouse::Target)],
        x: u16,
        y: u16,
    ) -> Option<crate::mouse::Target> {
        self.0.borrow_mut().mouse.target(regions, x, y)
    }
    pub fn active(&self) -> WindowInfo {
        self.0.borrow().active()
    }
    pub(crate) fn add(&self, info: WindowInfo) {
        let mut host = self.0.borrow_mut();
        let document = Document::new(host.bounds, info, Scope::Content, host.mailbox.clone());
        let id = host.desktop.add(document);
        host.ids.push((info, id));
        host.raise_modal();
    }
    pub(crate) fn remove(&self, id: WindowId) {
        let mut host = self.0.borrow_mut();
        if let Some(index) = host.ids.iter().position(|(info, _)| info.id == id) {
            let (_, view) = host.ids.remove(index);
            host.desktop.remove_child_by_id(view);
            host.raise_modal();
        }
    }
    pub(crate) fn activate(&self, id: WindowId) -> bool {
        let mut host = self.0.borrow_mut();
        let Some((_, view)) = host.ids.iter().find(|(info, _)| info.id == id).copied() else {
            return false;
        };
        let changed = host.desktop.bring_to_front(view);
        host.raise_modal();
        changed
    }
    pub fn set_scope(&self, scope: Scope) {
        let mut host = self.0.borrow_mut();
        let current = host.modal.map_or(Scope::Content, |(scope, _)| scope);
        let active = host.active();
        if current == scope {
            if let Some((_, id)) = host.modal {
                host.document_mut(id).info = active;
            }
            return;
        }
        if let Some((_, id)) = host.modal.take() {
            host.desktop.remove_child_by_id(id);
        }
        if scope != Scope::Content {
            let document = Document::new(host.bounds, active, scope, host.mailbox.clone());
            let id = host.desktop.add(document);
            host.modal = Some((scope, id));
        } else {
            let id = host
                .ids
                .iter()
                .find(|(info, _)| *info == active)
                .expect("live selected window")
                .1;
            host.desktop.bring_to_front(id);
        }
    }
    /// Dispatch through TV's focused child/modal scope before handing the full
    /// original event to a controller. An unhandled event is never sent behind
    /// the top view as a fallback.
    pub fn dispatch(&self) -> Option<Receipt> {
        let mut host = self.0.borrow_mut();
        *host.mailbox.borrow_mut() = None;
        host.desktop.handle_event(&mut Event::command(INPUT));
        let receipt = host.mailbox.borrow_mut().take();
        receipt
    }
    pub(super) fn draw(
        &self,
        terminal: &mut Terminal,
        bounds: Rect,
        pixels: &[Cell],
        native: &[(u16, u16, Cell)],
    ) {
        let mut host = self.0.borrow_mut();
        if host.bounds != bounds {
            host.bounds = bounds;
            host.desktop.set_bounds(bounds);
        }
        // Only the foreground full-screen surface is visible. Retired token
        // cells must not survive in background windows after codec reclamation.
        let count = host.desktop.child_count();
        for i in 0..count {
            let child = host.desktop.child_at_mut(i);
            child.set_bounds(bounds);
            let document = child
                .as_any_mut()
                .downcast_mut::<Document>()
                .expect("DN native window");
            document.pixels.clear();
            document.native.clear();
            if i + 1 == count {
                document.pixels.extend_from_slice(pixels);
                document.native.extend_from_slice(native);
            }
        }
        terminal.draw_view(&mut host.desktop);
    }
}
impl Host {
    fn active(&self) -> WindowInfo {
        // The modal is a child scope of this document, not a second document.
        for i in (0..self.desktop.child_count()).rev() {
            let document = self
                .desktop
                .child_at(i)
                .as_any()
                .downcast_ref::<Document>()
                .expect("DN native window");
            if document.scope == Scope::Content {
                return document.info;
            }
        }
        unreachable!("manager is permanent")
    }
    fn document_mut(&mut self, id: ViewId) -> &mut Document {
        self.desktop
            .child_by_id_mut(id)
            .expect("live native view")
            .as_any_mut()
            .downcast_mut()
            .expect("DN native window")
    }
    fn raise_modal(&mut self) {
        if let Some((_, id)) = self.modal {
            let active = self.active();
            self.document_mut(id).info = active;
            self.desktop.bring_to_front(id);
        }
    }
}
