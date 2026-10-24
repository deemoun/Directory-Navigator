//! Ownership and identity of modeless screens. Lifecycle callers retain their
//! existing save/close guards; modal dialogs stay with their content or the app.
use crate::{editor::Editor, tetris::Game, viewer::Viewer};
use std::io;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// Session-local identity; a closed or replaced window never lends its ID to another.
pub struct WindowId(u64);
impl WindowId {
    pub const MANAGER: Self = Self(0);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowKind {
    Manager,
    Viewer,
    Editor,
    Game,
}
impl WindowKind {
    fn priority(self) -> u8 {
        match self {
            Self::Manager => 0,
            Self::Game => 1,
            Self::Viewer => 2,
            Self::Editor => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowInfo {
    pub id: WindowId,
    pub kind: WindowKind,
}
enum Content {
    Manager,
    Editor(Box<Editor>),
    Viewer(Box<Viewer>),
    Game(Box<Game>),
}
impl Content {
    fn kind(&self) -> WindowKind {
        match self {
            Self::Manager => WindowKind::Manager,
            Self::Editor(_) => WindowKind::Editor,
            Self::Viewer(_) => WindowKind::Viewer,
            Self::Game(_) => WindowKind::Game,
        }
    }
}
struct Window {
    id: WindowId,
    content: Content,
}
impl Window {
    fn info(&self) -> WindowInfo {
        WindowInfo {
            id: self.id,
            kind: self.content.kind(),
        }
    }
}

pub struct Desktop {
    windows: Vec<Window>,
    active: WindowInfo,
    next_id: u64,
}
impl Default for Desktop {
    fn default() -> Self {
        Self {
            windows: vec![Window {
                id: WindowId::MANAGER,
                content: Content::Manager,
            }],
            active: WindowInfo {
                id: WindowId::MANAGER,
                kind: WindowKind::Manager,
            },
            next_id: 1,
        }
    }
}
impl Desktop {
    pub fn active(&self) -> WindowInfo {
        self.active
    }
    pub fn windows(&self) -> impl Iterator<Item = WindowInfo> + '_ {
        self.windows.iter().map(Window::info)
    }
    pub fn contains(&self, id: WindowId) -> bool {
        self.windows.iter().any(|w| w.id == id)
    }
    pub fn id(&self, kind: WindowKind) -> Option<WindowId> {
        self.windows().find(|w| w.kind == kind).map(|w| w.id)
    }
    /// Activate a live window and remember the order for close-time fallback.
    /// Stale identities never select a replacement window.
    pub fn activate(&mut self, id: WindowId) -> bool {
        let Some(index) = self.windows.iter().position(|w| w.id == id) else {
            return false;
        };
        let window = self.windows.remove(index);
        self.active = window.info();
        self.windows.push(window);
        true
    }
    fn install(&mut self, content: Content) -> io::Result<WindowId> {
        let kind = content.kind();
        let next = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| io::Error::other("Window identifiers exhausted"))?;
        let id = WindowId(self.next_id);
        self.next_id = next;
        // Still one screen per kind. Dropping replaced content cancels its tasks.
        self.windows.retain(|w| w.info().kind != kind);
        self.windows.push(Window { id, content });
        // Keep background tool creation from stealing an active document, while
        // allowing a document explicitly opened from the manager to take focus.
        let chosen = if self.active.kind == WindowKind::Manager
            || kind.priority() >= self.active.kind.priority()
        {
            id
        } else {
            self.active.id
        };
        self.activate(chosen);
        Ok(id)
    }
    fn close(&mut self, kind: WindowKind) {
        self.windows.retain(|w| w.info().kind != kind);
        if self.active.kind == kind {
            if let Some(id) = self.windows.last().map(|w| w.id) {
                self.activate(id);
            }
        }
    }
    pub fn open_editor(&mut self, editor: Editor) -> io::Result<WindowId> {
        self.install(Content::Editor(Box::new(editor)))
    }
    pub fn open_viewer(&mut self, viewer: Viewer) -> io::Result<WindowId> {
        self.install(Content::Viewer(Box::new(viewer)))
    }
    pub fn open_game(&mut self, game: Game) -> io::Result<WindowId> {
        self.install(Content::Game(Box::new(game)))
    }
    pub fn close_editor(&mut self) {
        self.close(WindowKind::Editor);
    }
    pub fn close_viewer(&mut self) {
        self.close(WindowKind::Viewer);
    }
    pub fn close_game(&mut self) {
        self.close(WindowKind::Game);
    }
    pub fn editor(&self) -> Option<&Editor> {
        self.windows.iter().find_map(|w| match &w.content {
            Content::Editor(e) => Some(e.as_ref()),
            _ => None,
        })
    }
    pub fn editor_mut(&mut self) -> Option<&mut Editor> {
        self.windows.iter_mut().find_map(|w| match &mut w.content {
            Content::Editor(e) => Some(e.as_mut()),
            _ => None,
        })
    }
    pub fn viewer(&self) -> Option<&Viewer> {
        self.windows.iter().find_map(|w| match &w.content {
            Content::Viewer(v) => Some(v.as_ref()),
            _ => None,
        })
    }
    pub fn viewer_mut(&mut self) -> Option<&mut Viewer> {
        self.windows.iter_mut().find_map(|w| match &mut w.content {
            Content::Viewer(v) => Some(v.as_mut()),
            _ => None,
        })
    }
    pub fn game(&self) -> Option<&Game> {
        self.windows.iter().find_map(|w| match &w.content {
            Content::Game(g) => Some(g.as_ref()),
            _ => None,
        })
    }
    pub fn game_mut(&mut self) -> Option<&mut Game> {
        self.windows.iter_mut().find_map(|w| match &mut w.content {
            Content::Game(g) => Some(g.as_mut()),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exhausted_ids_preserve_existing_content_and_active_window() -> io::Result<()> {
        let mut desktop = Desktop::default();
        let id = desktop.open_game(Game::default())?;
        desktop.next_id = u64::MAX;
        assert!(desktop.open_game(Game::default()).is_err());
        assert_eq!(desktop.active().id, id);
        assert!(desktop.game().is_some());
        assert_eq!(desktop.windows().count(), 2);
        Ok(())
    }
}
