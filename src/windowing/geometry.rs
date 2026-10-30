//! Session-only native geometry. Controllers retain contents and guarded actions.
use crate::{desktop::WindowId, modals::ModalId, presentation::layout::Rect};
use crossterm::event::{
    KeyCode as K, KeyEvent, KeyModifiers as M, MouseButton, MouseEvent, MouseEventKind,
};
use std::time::{Duration, Instant};
use turbo_vision::{
    core::{
        event::{Event, EventType, KB_DOWN, KB_ENTER, KB_LEFT, KB_RIGHT, KB_UP, MB_LEFT_BUTTON},
        geometry::{Point, Rect as TvRect},
        keys::KeyModifiers as TvModifiers,
    },
    views::{window::Window, View},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SurfaceKey {
    Document(WindowId),
    Modal(ModalId),
    ModalPage(ModalId, u64),
    Page(WindowId, u64),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SurfaceId {
    pub key: SurfaceKey,
    pub generation: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capabilities {
    pub movable: bool,
    pub tile: bool,
    pub cascade: bool,
    pub workspace_only: bool,
    pub resize: bool,
    pub zoom: bool,
    pub minimum: (u16, u16),
}
impl Capabilities {
    pub(crate) fn arranges(self, mode: super::Arrangement) -> bool {
        match mode {
            super::Arrangement::Tile => self.tile,
            super::Arrangement::Cascade => self.cascade,
        }
    }
    pub const fn fixed_zoom(w: u16, h: u16) -> Self {
        let mut c = Self::fixed(w, h);
        c.zoom = true;
        c.workspace_only = true;
        c
    }
    pub const fn tool(w: u16, h: u16) -> Self {
        let mut c = Self::document(w, h);
        c.workspace_only = true;
        c
    }
    pub const fn fixed(w: u16, h: u16) -> Self {
        Self {
            movable: true,
            tile: true,
            cascade: true,
            workspace_only: false,
            resize: false,
            zoom: false,
            minimum: (w, h),
        }
    }
    pub const fn document(w: u16, h: u16) -> Self {
        Self {
            movable: true,
            tile: true,
            cascade: true,
            workspace_only: false,
            resize: true,
            zoom: true,
            minimum: (w, h),
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Definition {
    pub key: SurfaceKey,
    pub bounds: Rect,
    pub caps: Capabilities,
    pub maximized: bool,
    pub title_row: u16,
    pub parent: Option<SurfaceKey>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Surface {
    pub id: SurfaceId,
    pub bounds: Rect,
    pub default: Rect,
    pub caps: Capabilities,
    pub maximized: bool,
    pub title_row: u16,
    pub parent: Option<SurfaceKey>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chrome {
    Activate,
    Move,
    Resize,
    Zoom,
    Close,
}
#[derive(Clone, Copy)]
pub(crate) struct Saved {
    bounds: Rect,
    preferred: Rect,
    maximized: bool,
}
pub(crate) struct Shell {
    pub window: Window,
    pub surface: Surface,
    pub preferred: Rect,
}
pub(crate) fn tv(r: Rect) -> TvRect {
    TvRect::new(r.x as i16, r.y as i16, r.right() as i16, r.bottom() as i16)
}
pub(crate) fn rect(r: TvRect) -> Rect {
    Rect::new(
        r.a.x.max(0) as u16,
        r.a.y.max(0) as u16,
        r.width().max(0) as u16,
        r.height().max(0) as u16,
    )
}
pub fn workspace(size: (u16, u16)) -> Rect {
    Rect::new(0, 1, size.0, size.1.saturating_sub(2))
}
pub(crate) fn clamp(r: Rect, limits: Rect) -> Rect {
    let w = r.width.min(limits.width);
    let h = r.height.min(limits.height);
    Rect::new(
        r.x.clamp(limits.x, limits.right().saturating_sub(w)),
        r.y.clamp(limits.y, limits.bottom().saturating_sub(h)),
        w,
        h,
    )
}
fn maximized_bounds(key: SurfaceKey, size: (u16, u16)) -> Rect {
    if matches!(key, SurfaceKey::Document(_)) {
        Rect::new(0, 0, size.0, size.1)
    } else {
        workspace(size)
    }
}
impl Shell {
    pub fn new(d: Definition, generation: u64, size: (u16, u16)) -> Self {
        let limits = workspace(size);
        let preferred = if d.maximized {
            let w = (limits.width * 4 / 5)
                .max(d.caps.minimum.0)
                .min(limits.width);
            let h = (limits.height * 4 / 5)
                .max(d.caps.minimum.1)
                .min(limits.height);
            Rect::new(
                (limits.width - w) / 2,
                limits.y + (limits.height - h) / 2,
                w,
                h,
            )
        } else {
            clamp(d.bounds, limits)
        };
        let bounds = if d.maximized {
            if d.caps.workspace_only {
                workspace(size)
            } else {
                maximized_bounds(d.key, size)
            }
        } else {
            preferred
        };
        let mut window = Window::new(tv(bounds), "");
        window.set_auto_close(false);
        window.set_state_flag(turbo_vision::core::state::State::SHADOW, false);
        window.set_resizable(d.caps.resize);
        window.set_zoomable(d.caps.zoom);
        window.set_min_size(Point::new(d.caps.minimum.0 as i16, d.caps.minimum.1 as i16));
        window.set_drag_limits(tv(limits));
        Self {
            window,
            surface: Surface {
                id: SurfaceId {
                    key: d.key,
                    generation,
                },
                bounds,
                default: d.bounds,
                caps: d.caps,
                maximized: d.maximized,
                title_row: d.title_row,
                parent: d.parent,
            },
            preferred,
        }
    }
    pub fn sync(&mut self, d: Definition, size: (u16, u16)) {
        if !d.caps.resize
            && (self.surface.default.width, self.surface.default.height)
                != (d.bounds.width, d.bounds.height)
        {
            self.preferred.width = d.bounds.width;
            self.preferred.height = d.bounds.height;
        }
        self.surface.default = d.bounds;
        self.surface.caps = d.caps;
        self.surface.parent = d.parent;
        self.window.set_drag_limits(tv(workspace(size)));
        self.window.set_resizable(d.caps.resize);
        self.window.set_zoomable(d.caps.zoom);
        self.window
            .set_min_size(Point::new(d.caps.minimum.0 as i16, d.caps.minimum.1 as i16));
        let bounds = if self.surface.maximized {
            if self.surface.caps.workspace_only {
                workspace(size)
            } else {
                maximized_bounds(self.surface.id.key, size)
            }
        } else {
            let mut preferred = self.preferred;
            preferred.width = preferred.width.max(d.caps.minimum.0);
            preferred.height = preferred.height.max(d.caps.minimum.1);
            clamp(preferred, workspace(size))
        };
        self.set(bounds);
        self.surface.title_row = if self.surface.maximized {
            d.title_row
        } else {
            0
        };
    }
    fn set(&mut self, r: Rect) {
        self.window.set_bounds(tv(r));
        self.surface.bounds = rect(self.window.bounds());
    }
    pub fn save(&self) -> Saved {
        Saved {
            bounds: self.surface.bounds,
            preferred: self.preferred,
            maximized: self.surface.maximized,
        }
    }
    pub fn rollback(&mut self, s: Saved, size: (u16, u16)) {
        self.preferred = s.preferred;
        self.surface.maximized = s.maximized;
        self.set(if s.maximized {
            if self.surface.caps.workspace_only {
                workspace(size)
            } else {
                maximized_bounds(self.surface.id.key, size)
            }
        } else {
            clamp(s.bounds, workspace(size))
        });
    }
    pub fn restore(&mut self, size: (u16, u16)) {
        self.surface.maximized = false;
        self.surface.title_row = 0;
        self.set(clamp(self.preferred, workspace(size)));
    }
    pub fn zoom(&mut self, size: (u16, u16)) {
        if !self.surface.caps.zoom {
            return;
        }
        if self.surface.maximized {
            self.restore(size);
        } else {
            self.preferred = self.surface.bounds;
            self.surface.maximized = true;
            self.set(if self.surface.caps.workspace_only {
                workspace(size)
            } else {
                maximized_bounds(self.surface.id.key, size)
            });
        }
    }
    pub(crate) fn arrange(&mut self, bounds: Rect) {
        self.surface.maximized = false;
        self.surface.title_row = 0;
        self.preferred = bounds;
        self.set(bounds);
    }
    pub fn commit(&mut self) {
        self.surface.bounds = rect(self.window.bounds());
        if !self.surface.maximized {
            self.preferred = self.surface.bounds;
        }
    }
    pub fn native_mouse(&mut self, e: MouseEvent) {
        if matches!(e.kind, MouseEventKind::Down(_)) {
            self.window.set_focus(true);
        }
        let b = self.surface.bounds;
        let kind = match e.kind {
            MouseEventKind::Down(_) => EventType::MouseDown,
            MouseEventKind::Drag(_) => EventType::MouseMove,
            MouseEventKind::Up(_) => EventType::MouseUp,
            _ => return,
        };
        let pos = Point::new(
            (i32::from(e.column) - i32::from(b.x)) as i16,
            (i32::from(e.row) - i32::from(b.y)) as i16,
        );
        self.window.handle_event(&mut Event::mouse(
            kind,
            pos,
            if matches!(e.kind, MouseEventKind::Up(_)) {
                0
            } else {
                MB_LEFT_BUTTON
            },
            false,
        ));
        self.surface.bounds = rect(self.window.bounds());
    }
    pub fn keyboard_start(&mut self) {
        self.window
            .set_state_flag(turbo_vision::core::state::State::ACTIVE, true);
        self.window
            .handle_event(&mut Event::command(turbo_vision::core::command::CM_RESIZE));
    }
    pub fn keyboard_step(&mut self, key: KeyEvent, size: (u16, u16)) {
        let shift = key.modifiers.contains(M::SHIFT) && !matches!(key.code, K::Tab | K::BackTab);
        if shift && !self.surface.caps.resize {
            return;
        }
        let code = match key.code {
            K::Left | K::BackTab => KB_LEFT,
            K::Tab if key.modifiers.contains(M::SHIFT) => KB_LEFT,
            K::Right | K::Tab => KB_RIGHT,
            K::Up => KB_UP,
            K::Down => KB_DOWN,
            _ => 0,
        };
        if code != 0 {
            let count =
                if key.modifiers.contains(M::CONTROL) && matches!(key.code, K::Left | K::Right) {
                    8
                } else {
                    1
                };
            for _ in 0..count {
                let mut event = Event::keyboard(code);
                event.key_modifiers = if shift {
                    TvModifiers::SHIFT
                } else {
                    TvModifiers::empty()
                };
                self.window.handle_event(&mut event);
            }
        } else {
            let mut b = self.surface.bounds;
            let l = workspace(size);
            match key.code {
                K::Home => b.x = l.x,
                K::End => b.x = l.right().saturating_sub(b.width),
                K::PageUp => b.y = l.y,
                K::PageDown => b.y = l.bottom().saturating_sub(b.height),
                _ => return,
            };
            self.set(b);
        }
        self.set(clamp(rect(self.window.bounds()), workspace(size)));
    }
    pub fn end(&mut self) {
        self.window.handle_event(&mut Event::keyboard(KB_ENTER));
        self.window.handle_event(&mut Event::mouse(
            EventType::MouseUp,
            Point::new(0, 0),
            0,
            false,
        ));
    }
}
pub(crate) struct Capture {
    pub id: SurfaceId,
    pub saved: Saved,
    pub keyboard: bool,
    pub dragged: bool,
}
#[derive(Default)]
pub(crate) struct Interaction {
    pub capture: Option<Capture>,
    pub last_title: Option<(SurfaceId, (u16, u16), Instant)>,
    pub size: (u16, u16),
    pub top: Option<SurfaceKey>,
    pub modal: Option<ModalId>,
    pub revision: u64,
}
impl Interaction {
    pub fn double(&mut self, id: SurfaceId, point: (u16, u16), now: Instant) -> bool {
        let double = self.last_title.is_some_and(|(old, previous, t)| {
            old == id
                && previous.0.abs_diff(point.0) <= 1
                && previous.1 == point.1
                && now.saturating_duration_since(t) < Duration::from_millis(400)
        });
        self.last_title = Some((id, point, now));
        double
    }
}
// All gesture events remain in the host, rather than reaching document fields.
pub(crate) fn left(e: MouseEvent) -> bool {
    matches!(
        e.kind,
        MouseEventKind::Down(MouseButton::Left)
            | MouseEventKind::Drag(MouseButton::Left)
            | MouseEventKind::Up(MouseButton::Left)
    )
}
