//! Native desktop ownership. Application models own their data; TV owns the
//! selected window, z-order and the modal input scope. Commands return to the
//! existing controllers before any close/save/filesystem action is performed.
use crate::windowing::geometry::{
    self, Capabilities, Capture, Definition, Interaction, Shell, Surface, SurfaceId, SurfaceKey,
};
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
    views::{desktop::Desktop, View, ViewCore, ViewId},
};

pub(super) const INPUT: u16 = 0xff00;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    Content,
    Layer(Focus),
    Modal(crate::modals::ModalInfo),
    Screensaver,
}
impl Scope {
    pub fn for_app(app: &crate::app::App) -> Self {
        if app.output_visible {
            return Self::Layer(Focus::TerminalOutput);
        }
        if app.screensaver.active {
            return Self::Screensaver;
        }
        if let Some(modal) = app.modal_top() {
            return Self::Modal(modal);
        }
        match crate::focus::current(app) {
            Focus::Manager | Focus::Editor | Focus::Viewer | Focus::Plugin => Self::Content,
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
    shell: Shell,
    info: WindowInfo,
    scope: Scope,
    mailbox: Mailbox,
    native: Vec<(u16, u16, Cell)>,
}
impl Document {
    fn new(bounds: Rect, info: WindowInfo, scope: Scope, mailbox: Mailbox) -> Self {
        let size = (bounds.width() as u16, bounds.height() as u16);
        let maximal = matches!(
            info.kind,
            crate::desktop::WindowKind::Manager
                | crate::desktop::WindowKind::Editor
                | crate::desktop::WindowKind::Viewer
        ) || scope != Scope::Content;
        let caps = match info.kind {
            crate::desktop::WindowKind::Editor => Capabilities::document(72, 12),
            crate::desktop::WindowKind::Viewer => Capabilities::document(48, 10),
            _ => Capabilities::fixed(54, 23),
        };
        let default = if maximal {
            geometry::rect(bounds)
        } else {
            crate::presentation::layout::Rect::new(
                size.0.saturating_sub(54) / 2,
                size.1.saturating_sub(23) / 2,
                54,
                23,
            )
        };
        let mut shell = Shell::new(
            Definition {
                key: SurfaceKey::Document(info.id),
                bounds: default,
                caps,
                maximized: maximal,
                title_row: u16::from(info.kind == crate::desktop::WindowKind::Editor),
                parent: None,
            },
            0,
            size,
        );
        if scope != Scope::Content {
            shell.window.set_state_flag(State::MODAL, true);
        }
        Self {
            shell,
            info,
            scope,
            mailbox,
            native: Vec::new(),
        }
    }
}
impl View for Document {
    fn core(&self) -> &ViewCore {
        self.shell.window.core()
    }
    fn core_mut(&mut self) -> &mut ViewCore {
        self.shell.window.core_mut()
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
        self.shell.window.set_focus(focused);
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
    auxiliary: std::collections::BTreeMap<SurfaceKey, Shell>,
    interaction: Interaction,
    next_surface: u64,
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
            auxiliary: Default::default(),
            interaction: Interaction {
                size: (80, 25),
                ..Default::default()
            },
            next_surface: 1,
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
        regions: &[(crate::presentation::layout::Rect, crate::mouse::Target)],
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
        host.focus_content(info.id);
        if let Some((_, modal)) = host.modal {
            host.document_mut(modal).info = info;
        }
        host.raise_modal();
    }
    pub(crate) fn remove(&self, id: WindowId) {
        let mut host = self.0.borrow_mut();
        let active = host.active().id;
        if let Some(index) = host.ids.iter().position(|(info, _)| info.id == id) {
            let (_, view) = host.ids.remove(index);
            host.desktop.remove_child_by_id(view);
            if active != id {
                host.focus_content(active);
            }
            host.interaction.revision += 1;
            host.auxiliary
                .retain(|key, _| !matches!(key,SurfaceKey::Page(owner,_) if *owner==id));
            if host.interaction.capture.as_ref().is_some_and(|c|matches!(c.id.key,SurfaceKey::Document(owner)|SurfaceKey::Page(owner,_) if owner==id)){host.interaction.capture=None;}
            host.raise_modal();
        }
    }
    pub(crate) fn activate(&self, id: WindowId) -> bool {
        let mut host = self.0.borrow_mut();
        let Some((_, view)) = host.ids.iter().find(|(info, _)| info.id == id).copied() else {
            return false;
        };
        let info = host.ids.iter().find(|(_, v)| *v == view).unwrap().0;
        let changed = if id == WindowId::MANAGER {
            host.select_manager();
            true
        } else {
            host.desktop.bring_to_front(view)
        };
        host.focus_content(id);
        if let Some((_, modal)) = host.modal {
            host.document_mut(modal).info = info;
        }
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
            if active.id == WindowId::MANAGER {
                host.select_manager();
            } else {
                host.desktop.bring_to_front(id);
                host.focus_content(active.id);
            }
        }
    }
    /// Dispatch through TV's focused child/modal scope before handing the full
    /// original event to a controller. An unhandled event is never sent behind
    /// the top view as a fallback.
    pub fn dispatch(&self) -> Option<Receipt> {
        let mut host = self.0.borrow_mut();
        *host.mailbox.borrow_mut() = None;
        let id = host.modal.map(|(_, id)| id).unwrap_or_else(|| {
            let active = host.active();
            host.ids.iter().find(|(w, _)| *w == active).unwrap().1
        });
        host.document_mut(id)
            .handle_event(&mut Event::command(INPUT));
        let receipt = host.mailbox.borrow_mut().take();
        receipt
    }
    pub(super) fn draw(&self, terminal: &mut Terminal, bounds: Rect, native: &[(u16, u16, Cell)]) {
        let mut host = self.0.borrow_mut();
        if host.bounds != bounds {
            host.bounds = bounds;
            host.desktop.set_bounds(bounds);
        }
        // The compositor encodes all visible layers together. Native window
        // buffers cannot retain glyph tokens from an older frame generation.
        for i in 0..host.desktop.child_count() {
            if let Some(document) = host
                .desktop
                .child_at_mut(i)
                .as_any_mut()
                .downcast_mut::<Document>()
            {
                document.native.clear();
            }
        }
        for &(x, y, cell) in native {
            terminal.write_cell(x as i16, y as i16, cell);
        }
    }
}
impl Host {
    fn select_manager(&mut self) {
        self.focus_content(WindowId::MANAGER);
    }
    fn focus_content(&mut self, id: WindowId) {
        for i in 0..self.desktop.child_count() {
            let child = self.desktop.child_at_mut(i);
            if let Some(d) = child.as_any_mut().downcast_mut::<Document>() {
                let selected = d.info.id == id && d.scope == Scope::Content;
                d.set_focus(selected);
                d.set_state_flag(State::ACTIVE, selected);
            }
        }
    }
    fn shell_mut(&mut self, key: SurfaceKey) -> Option<&mut Shell> {
        if let SurfaceKey::Document(id) = key {
            let view = self.ids.iter().find(|(w, _)| w.id == id)?.1;
            Some(&mut self.document_mut(view).shell)
        } else {
            self.auxiliary.get_mut(&key)
        }
    }
    fn active(&self) -> WindowInfo {
        if let Some((_, id)) = self.modal {
            return self
                .desktop
                .child_by_id(id)
                .unwrap()
                .as_any()
                .downcast_ref::<Document>()
                .unwrap()
                .info;
        }
        for i in (0..self.desktop.child_count()).rev() {
            let Some(document) = self.desktop.child_at(i).as_any().downcast_ref::<Document>()
            else {
                continue;
            };
            if document.scope == Scope::Content && document.state().contains(State::ACTIVE) {
                return document.info;
            }
        }
        for i in (0..self.desktop.child_count()).rev() {
            if let Some(document) = self
                .desktop
                .child_at(i)
                .as_any()
                .downcast_ref::<Document>()
                .filter(|d| d.scope == Scope::Content)
            {
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

impl NativeDesktop {
    pub(crate) fn geometry_sync(
        &self,
        size: (u16, u16),
        definitions: &[Definition],
        modal: Option<crate::modals::ModalId>,
    ) -> Vec<Surface> {
        let mut h = self.0.borrow_mut();
        let active = h.active().id;
        let previous_surfaces = definitions
            .iter()
            .filter_map(|d| h.shell_mut(d.key).map(|s| s.surface))
            .collect::<Vec<_>>();
        if h.interaction.modal != modal {
            if let Some(c) = h.interaction.capture.take() {
                if let Some(s) = h.shell_mut(c.id.key) {
                    s.end();
                    s.rollback(c.saved, size);
                }
            }
            h.interaction.modal = modal;
            h.interaction.last_title = None;
        }
        let top = definitions
            .iter()
            .rev()
            .find(|d| match d.key {
                SurfaceKey::Modal(_) | SurfaceKey::ModalPage(..) => true,
                SurfaceKey::Page(id, _) => id == active,
                _ => false,
            })
            .map(|d| d.key)
            .or(Some(SurfaceKey::Document(active)));
        if h.interaction.top != top {
            if let Some(c) = h.interaction.capture.take() {
                if top != Some(c.id.key) {
                    if let Some(s) = h.shell_mut(c.id.key) {
                        s.end();
                        s.rollback(c.saved, size);
                    }
                } else {
                    h.interaction.capture = Some(c);
                }
            }
            h.interaction.top = top;
        }
        let resized = h.interaction.size != size;
        if resized {
            if let Some(c) = h.interaction.capture.take() {
                if let Some(s) = h.shell_mut(c.id.key) {
                    s.end();
                    s.rollback(c.saved, size);
                }
            }
            h.interaction.size = size;
            h.interaction.revision += 1;
        }
        let root = Rect::new(0, 0, size.0 as i16, size.1 as i16);
        if h.bounds != root {
            h.bounds = root;
            h.desktop.set_bounds(root);
        }
        let previous = h.auxiliary.len();
        h.auxiliary
            .retain(|key, _| definitions.iter().any(|d| d.key == *key));
        if previous != h.auxiliary.len() {
            h.interaction.revision += 1;
        }
        for d in definitions {
            if !matches!(d.key, SurfaceKey::Document(_)) && !h.auxiliary.contains_key(&d.key) {
                let generation = h.next_surface;
                h.next_surface += 1;
                let mut shell = Shell::new(*d, generation, size);
                if let Some(parent) = d.parent.and_then(|key| h.shell_mut(key).map(|s| s.surface)) {
                    let w = d.bounds.width;
                    let height = d.bounds.height;
                    shell.preferred = geometry::clamp(
                        if parent.maximized {
                            d.bounds
                        } else {
                            crate::presentation::layout::Rect::new(
                                (parent.bounds.x + parent.bounds.width / 2).saturating_sub(w / 2),
                                (parent.bounds.y + parent.bounds.height / 2)
                                    .saturating_sub(height / 2),
                                w,
                                height,
                            )
                        },
                        geometry::workspace(size),
                    );
                    shell.sync(*d, size);
                }
                h.auxiliary.insert(d.key, shell);
                h.interaction.revision += 1;
            }
            let captured = h
                .interaction
                .capture
                .as_ref()
                .is_some_and(|c| c.id.key == d.key);
            if !captured {
                if let Some(shell) = h.shell_mut(d.key) {
                    shell.sync(*d, size);
                }
            }
        }
        if h.interaction.capture.as_ref().is_some_and(|c| {
            !h.auxiliary.contains_key(&c.id.key) && !matches!(c.id.key, SurfaceKey::Document(_))
        }) {
            h.interaction.capture = None;
        }
        let surfaces = definitions
            .iter()
            .filter_map(|d| h.shell_mut(d.key).map(|s| s.surface))
            .collect::<Vec<_>>();
        if surfaces != previous_surfaces {
            h.interaction.revision += 1;
        }
        surfaces
    }
    pub(crate) fn surface(&self, key: SurfaceKey) -> Option<Surface> {
        let mut h = self.0.borrow_mut();
        h.shell_mut(key).map(|s| s.surface)
    }
    pub(crate) fn z_order(&self) -> Vec<WindowId> {
        let h = self.0.borrow();
        (0..h.desktop.child_count())
            .filter_map(|i| h.desktop.child_at(i).as_any().downcast_ref::<Document>())
            .filter(|d| d.scope == Scope::Content)
            .map(|d| d.info.id)
            .collect()
    }
    pub(crate) fn top_surface(&self) -> Option<Surface> {
        let mut h = self.0.borrow_mut();
        let key = h.interaction.top?;
        if key == SurfaceKey::Document(WindowId::MANAGER) {
            return None;
        }
        h.shell_mut(key).map(|s| s.surface)
    }
    pub(crate) fn geometry_revision(&self) -> u64 {
        self.0.borrow().interaction.revision
    }
    pub(crate) fn manipulating(&self) -> bool {
        self.0.borrow().interaction.capture.is_some()
    }
    pub(crate) fn zoom_surface(&self, id: SurfaceId) -> bool {
        let mut h = self.0.borrow_mut();
        let size = h.interaction.size;
        if let Some(s) = h
            .shell_mut(id.key)
            .filter(|s| s.surface.id == id && s.surface.caps.zoom)
        {
            s.zoom(size);
            h.interaction.revision += 1;
            true
        } else {
            false
        }
    }
    pub(crate) fn begin_move(
        &self,
        id: SurfaceId,
        mouse: Option<crossterm::event::MouseEvent>,
    ) -> bool {
        let mut h = self.0.borrow_mut();
        let size = h.interaction.size;
        let Some(shell) = h.shell_mut(id.key).filter(|s| s.surface.id == id) else {
            return false;
        };
        let saved = shell.save();
        let restored = shell.surface.maximized;
        if restored {
            shell.restore(size);
            if let Some(e) = mouse {
                let r = shell.surface.bounds;
                let x = e.column.saturating_sub(r.width / 2);
                shell.preferred = geometry::clamp(
                    crate::presentation::layout::Rect::new(x, e.row, r.width, r.height),
                    geometry::workspace(size),
                );
                shell.restore(size);
            }
        }
        if let Some(mut e) = mouse {
            if e.row < shell.surface.bounds.y {
                e.row = shell.surface.bounds.y;
            }
            // The host classified this press using the maximized title.
            // Restoring may place a different native zoom icon under the pointer;
            // arm a drag rather than reinterpreting that press as a new control.
            if restored {
                shell.window.set_zoomable(false);
            }
            shell.native_mouse(e);
            if restored {
                shell.window.set_zoomable(shell.surface.caps.zoom);
            }
        } else {
            shell.keyboard_start();
        }
        h.interaction.capture = Some(Capture {
            id,
            saved,
            keyboard: mouse.is_none(),
            dragged: false,
        });
        h.interaction.revision += 1;
        true
    }
    pub(crate) fn geometry_key(&self, key: crossterm::event::KeyEvent) -> bool {
        use crossterm::event::KeyCode;
        let mut h = self.0.borrow_mut();
        let Some(c) = h.interaction.capture.take() else {
            return false;
        };
        let size = h.interaction.size;
        if let Some(shell) = h.shell_mut(c.id.key).filter(|s| s.surface.id == c.id) {
            match key.code {
                KeyCode::Esc => {
                    shell.end();
                    shell.rollback(c.saved, size);
                }
                KeyCode::Enter => {
                    shell.end();
                    shell.commit();
                }
                _ => {
                    if c.keyboard {
                        shell.keyboard_step(key, size);
                    }
                    h.interaction.capture = Some(c);
                }
            }
        }
        h.interaction.revision += 1;
        true
    }
    pub(crate) fn geometry_mouse(&self, e: crossterm::event::MouseEvent) -> bool {
        let mut h = self.0.borrow_mut();
        let Some(mut c) = h.interaction.capture.take() else {
            return false;
        };
        if matches!(e.kind, crossterm::event::MouseEventKind::Drag(_)) {
            h.interaction.last_title = None;
            c.dragged = true;
        }
        let size = h.interaction.size;
        if let Some(shell) = h.shell_mut(c.id.key).filter(|s| s.surface.id == c.id) {
            if !c.keyboard && geometry::left(e) {
                shell.native_mouse(e);
                shell.commit();
            }
            if matches!(
                e.kind,
                crossterm::event::MouseEventKind::Up(crossterm::event::MouseButton::Left)
            ) && !c.keyboard
            {
                shell.end();
                if !c.dragged {
                    shell.rollback(c.saved, size);
                }
            } else {
                h.interaction.capture = Some(c);
            }
        }
        h.interaction.revision += 1;
        true
    }
    pub(crate) fn title_double(
        &self,
        id: SurfaceId,
        point: (u16, u16),
        now: std::time::Instant,
    ) -> bool {
        self.0.borrow_mut().interaction.double(id, point, now)
    }
}
