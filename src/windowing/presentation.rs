//! One presentation policy for each host window class. Native geometry consumes
//! these declarations; controllers do not infer capabilities from window size.
use super::geometry::{Capabilities, Definition, SurfaceKey};
use crate::{app::App, desktop::WindowKind, presentation::layout::Rect, tool_windows::Kind};

#[derive(Clone, Copy)]
pub(crate) struct Policy {
    pub caps: Capabilities,
    pub maximized: bool,
    pub title_row: u16,
}
impl Policy {
    pub fn definition(
        self,
        key: SurfaceKey,
        bounds: Rect,
        parent: Option<SurfaceKey>,
    ) -> Definition {
        Definition {
            key,
            bounds,
            caps: self.caps,
            maximized: self.maximized,
            title_row: self.title_row,
            parent,
        }
    }
    pub fn initial(kind: WindowKind) -> Self {
        let (caps, maximized, title_row) = match kind {
            WindowKind::Manager => {
                let mut c = Capabilities::fixed(1, 1);
                c.movable = false;
                c.tile = false;
                c.cascade = false;
                (c, true, 0)
            }
            WindowKind::Editor => (Capabilities::document(72, 12), true, 1),
            WindowKind::Viewer => (Capabilities::document(48, 10), true, 0),
            // Unpainted shells acquire a plugin/tool's declared size at preparation.
            WindowKind::Plugin(_) | WindowKind::Tool(_) => (Capabilities::fixed(1, 1), false, 0),
        };
        Self {
            caps,
            maximized,
            title_row,
        }
    }
    pub fn document(app: &App, kind: WindowKind) -> Option<Self> {
        let mut p = Self::initial(kind);
        match kind {
            WindowKind::Editor => {
                use unicode_width::UnicodeWidthStr;
                let menu = crate::editor::menus::LABELS
                    .iter()
                    .map(|label| {
                        crate::menu::plain(app.config.language.text(label)).width() as u16 + 3
                    })
                    .sum::<u16>();
                let clock = if app.config.show_clock {
                    app.clock.text.width() as u16 + 2
                } else {
                    0
                };
                p.caps.minimum.0 = 72.max(menu + clock + 2);
            }
            WindowKind::Viewer => {}
            _ => return None,
        }
        Some(p)
    }
    pub fn find_form() -> Self {
        Self {
            caps: Capabilities::tool(76, 22),
            maximized: false,
            title_row: 0,
        }
    }
    pub fn tool(kind: Kind, bounds: Rect) -> Self {
        let caps = match kind {
            Kind::Tree
            | Kind::Find
            | Kind::Sizes
            | Kind::Archive
            | Kind::EditHistory
            | Kind::ViewHistory
            | Kind::DirectoryHistory
            | Kind::CommandHistory
            | Kind::Help => Capabilities::tool(48, 10),
            _ => Capabilities::fixed(bounds.width, bounds.height),
        };
        Self {
            caps,
            maximized: false,
            title_row: 0,
        }
    }
    pub fn dialog(bounds: Rect, help: bool) -> Self {
        let mut caps = if help {
            Capabilities::document(48, 10)
        } else {
            Capabilities::fixed(bounds.width, bounds.height)
        };
        caps.tile = false;
        caps.cascade = false;
        Self {
            caps,
            maximized: false,
            title_row: 0,
        }
    }
}
