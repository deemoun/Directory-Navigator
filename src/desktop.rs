//! Ownership and identity of modeless screens. Lifecycle callers retain their
//! existing save/close guards; modal dialogs stay with their content or the app.
use crate::{editor::Editor, tetris::Game, viewer::Viewer};
use std::io;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
/// Session-local identity; a closed or replaced window never lends its ID to another.
pub struct WindowId(u64);
impl std::fmt::Display for WindowId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl WindowId {
    pub const MANAGER: Self = Self(0);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowKind {
    Manager,
    Viewer,
    Editor,
    Plugin(crate::plugins::PluginId),
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
    Plugin(crate::plugins::PluginId),
}
impl Content {
    fn kind(&self) -> WindowKind {
        match self {
            Self::Manager => WindowKind::Manager,
            Self::Editor(_) => WindowKind::Editor,
            Self::Viewer(_) => WindowKind::Viewer,
            Self::Plugin(id) => WindowKind::Plugin(*id),
        }
    }
}
struct Window {
    id: WindowId,
    open_revision: u64,
    content: Content,
    key: Option<std::path::PathBuf>,
    aliases: std::collections::BTreeSet<std::path::PathBuf>,
    _requests: crate::requests::RequestScope,
    identity: Option<crate::task::Task<std::io::Result<std::path::PathBuf>>>,
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
    pub plugins: crate::plugins::Runtime,
    windows: Vec<Window>,

    native: crate::turbo::desktop::NativeDesktop,
    next_id: u64,
    context: Option<WindowId>,
    activation_revision: u64,
    reservations: std::collections::BTreeMap<std::path::PathBuf, WindowId>,
}
impl Default for Desktop {
    fn default() -> Self {
        Self {
            plugins: Default::default(),
            windows: vec![Window {
                id: WindowId::MANAGER,
                open_revision: 0,
                content: Content::Manager,
                key: None,
                aliases: Default::default(),
                identity: None,
                _requests: Default::default(),
            }],
            native: crate::turbo::desktop::NativeDesktop::new(WindowInfo {
                id: WindowId::MANAGER,
                kind: WindowKind::Manager,
            }),

            next_id: 1,
            context: None,
            activation_revision: 0,
            reservations: Default::default(),
        }
    }
}
impl Desktop {
    pub fn active(&self) -> WindowInfo {
        self.native.active()
    }
    pub fn native(&self) -> crate::turbo::desktop::NativeDesktop {
        self.native.clone()
    }
    pub fn windows(&self) -> impl DoubleEndedIterator<Item = WindowInfo> + '_ {
        self.windows.iter().map(Window::info)
    }
    pub fn contains(&self, id: WindowId) -> bool {
        self.windows.iter().any(|w| w.id == id)
    }
    pub fn id(&self, kind: WindowKind) -> Option<WindowId> {
        self.windows().rev().find(|w| w.kind == kind).map(|w| w.id)
    }
    /// Synchronous lookup uses identities already resolved by workers and
    /// lexical spellings of pending requests; it performs no filesystem I/O.
    pub fn find_path(&self, kind: WindowKind, path: &std::path::Path) -> Option<WindowId> {
        let key = crate::requests::absolute_key(path);
        if kind == WindowKind::Editor {
            if let Some(owner) = self.reservations.get(&key) {
                return Some(*owner);
            }
        }
        self.windows
            .iter()
            .find(|w| {
                w.content.kind() == kind
                    && (w.key.as_ref() == Some(&key) || w.aliases.contains(&key))
            })
            .map(|w| w.id)
    }
    /// Activate a live window and remember the order for close-time fallback.
    /// Stale identities never select a replacement window.
    pub fn activation_revision(&self) -> u64 {
        self.activation_revision
    }
    pub fn activate(&mut self, id: WindowId) -> bool {
        self.activate_inner(id, true)
    }
    fn activate_inner(&mut self, id: WindowId, bump: bool) -> bool {
        let Some(index) = self.windows.iter().position(|w| w.id == id) else {
            return false;
        };
        if bump {
            self.activation_revision = self
                .activation_revision
                .checked_add(1)
                .expect("activation revision exhausted");
        }
        let window = self.windows.remove(index);
        assert!(self.native.activate(id));

        self.windows.push(window);
        true
    }
    fn install(&mut self, mut content: Content, foreground: bool) -> io::Result<WindowId> {
        let previous = self.active().id;
        let kind = content.kind();
        let next = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| io::Error::other("Window identifiers exhausted"))?;
        let id = WindowId(self.next_id);
        self.next_id = next;
        let path = match &content {
            Content::Editor(e) => e.path.clone(),
            Content::Viewer(v) => Some(v.path.clone()),
            _ => None,
        };
        let key = path.as_deref().map(crate::requests::absolute_key);
        if kind == WindowKind::Editor {
            if let Some(owner) = key
                .as_ref()
                .and_then(|key| self.reservations.get(key))
                .copied()
            {
                if foreground {
                    self.activate(owner);
                }
                return Ok(owner);
            }
        }
        if let Some(existing) = key
            .as_ref()
            .and_then(|key| {
                self.windows.iter().find(|w| {
                    w.content.kind() == kind
                        && (w.key.as_ref() == Some(key) || w.aliases.contains(key))
                })
            })
            .map(|w| w.id)
        {
            if let Some(window) = self.windows.iter_mut().find(|w| w.id == existing) {
                window.open_revision = id.0;
            }
            if let Content::Viewer(mut next) = content {
                next.requests
                    .bind(crate::requests::Owner::Document(existing));
                next.modals.bind(existing);
                if let Some(current) = self.viewer_mut(existing) {
                    if next.requested_mode != crate::viewer::ViewMode::Auto
                        || current.requested_mode != next.requested_mode
                    {
                        *current = *next;
                    }
                }
            }
            if foreground {
                self.activate(existing);
            }
            return Ok(existing);
        }
        if matches!(kind, WindowKind::Plugin(_)) {
            if let Some(old) = self.id(kind) {
                self.close(old);
            }
        }
        let requests = crate::requests::RequestScope::new(crate::requests::Owner::Document(id));
        let identity = path
            .map(|path| {
                requests.spawn("dn-document-identity", move |_| std::fs::canonicalize(path))
            })
            .transpose()?;
        match &mut content {
            Content::Editor(e) => {
                e.modals.bind(id);
                e.requests.bind(crate::requests::Owner::Document(id));
            }
            Content::Viewer(v) => {
                v.modals.bind(id);
                v.requests.bind(crate::requests::Owner::Document(id));
            }
            _ => {}
        }
        self.windows.push(Window {
            id,
            open_revision: id.0,
            content,
            aliases: key.iter().cloned().collect(),
            key,
            identity,
            _requests: requests,
        });
        self.native.add(WindowInfo { id, kind });
        if foreground {
            self.activate(id);
        } else {
            self.activate_inner(previous, false);
        }
        Ok(id)
    }
    pub fn close(&mut self, id: WindowId) {
        if id == WindowId::MANAGER {
            return;
        }
        let active = self.active().id;
        if let Some(binding) = self
            .plugins
            .bound()
            .into_iter()
            .find(|b| b.owner == crate::plugins::Owner::Document(id))
        {
            self.plugins.retire(binding);
        }
        self.native.remove(id);
        self.windows.retain(|w| w.id != id);
        self.reservations.retain(|_, owner| *owner != id);
        if active == id {
            if let Some(next) = self.windows.last().map(|w| w.id) {
                self.activate(next);
            }
        }
    }
    pub(crate) fn set_context(&mut self, id: Option<WindowId>) -> Option<WindowId> {
        std::mem::replace(&mut self.context, id)
    }
    pub fn target(&self) -> WindowId {
        self.context.unwrap_or_else(|| self.active().id)
    }
    pub fn document_ids(&self, kind: WindowKind) -> Vec<WindowId> {
        self.windows()
            .filter(|w| w.kind == kind)
            .map(|w| w.id)
            .collect()
    }
    pub fn identity_pending(&self, id: WindowId) -> bool {
        self.windows
            .iter()
            .any(|w| w.id == id && w.identity.is_some())
    }
    pub fn poll_identities(&mut self) -> bool {
        let mut resolved = Vec::new();
        for window in &mut self.windows {
            if let Some(task) = &window.identity {
                match task.try_recv() {
                    Ok(key) => {
                        resolved.push((window.id, window.content.kind(), key.ok()));
                        window.identity = None;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        resolved.push((window.id, window.content.kind(), None));
                        window.identity = None;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                }
            }
        }
        let changed = !resolved.is_empty();
        for (id, kind, key) in resolved {
            if let Some(key) = key {
                if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
                    w.aliases.insert(key.clone());
                    w.key = Some(key.clone());
                }
                let mut matching = self
                    .windows
                    .iter()
                    .filter(|w| w.content.kind() == kind && w.key.as_ref() == Some(&key))
                    .map(|w| w.id)
                    .collect::<Vec<_>>();
                matching.sort();
                let reserved = (kind == WindowKind::Editor)
                    .then(|| self.reservations.get(&key).copied())
                    .flatten();
                if let Some(existing) = reserved.or_else(|| matching.first().copied()) {
                    for duplicate in matching.into_iter().filter(|other| *other != existing) {
                        let aliases = self
                            .windows
                            .iter()
                            .find(|w| w.id == duplicate)
                            .map(|w| w.aliases.clone())
                            .unwrap_or_default();
                        if let Some(w) = self.windows.iter_mut().find(|w| w.id == existing) {
                            // A destination reservation is not file ownership until
                            // publication succeeds. Failed saves must release it.
                            if w.key.as_ref() == Some(&key) {
                                w.aliases.extend(aliases);
                            }
                        }
                        // The newest explicit viewer request selects its mode, even
                        // when an alias resolves after the initial open.
                        if kind == WindowKind::Viewer {
                            let reload = self
                                .viewer(duplicate)
                                .zip(self.viewer(existing))
                                .and_then(|(next, current)| {
                                    ((next.requested_mode != crate::viewer::ViewMode::Auto
                                        || next.requested_mode != current.requested_mode)
                                        && self
                                            .windows
                                            .iter()
                                            .find(|w| w.id == duplicate)
                                            .unwrap()
                                            .open_revision
                                            > self
                                                .windows
                                                .iter()
                                                .find(|w| w.id == existing)
                                                .unwrap()
                                                .open_revision)
                                        .then(|| (next.path.clone(), next.requested_mode))
                                });
                            if let Some((path, mode)) = reload {
                                if let Ok(mut viewer) = Viewer::open_mode(path, mode) {
                                    viewer
                                        .requests
                                        .bind(crate::requests::Owner::Document(existing));
                                    viewer.modals.bind(existing);
                                    if let Some(current) = self.viewer_mut(existing) {
                                        *current = viewer;
                                    }
                                    let revision = self
                                        .windows
                                        .iter()
                                        .find(|w| w.id == duplicate)
                                        .unwrap()
                                        .open_revision;
                                    if let Some(window) =
                                        self.windows.iter_mut().find(|w| w.id == existing)
                                    {
                                        window.open_revision = revision;
                                    }
                                }
                            }
                        }
                        let active = self.active().id == duplicate;
                        // Canonicalization is bookkeeping, not a new focus intent.
                        let epoch = self.activation_revision;
                        self.close(duplicate);
                        if active {
                            self.activate_inner(existing, false);
                        }
                        self.activation_revision = epoch;
                    }
                }
            }
        }
        changed
    }
    pub fn reserve(&mut self, id: WindowId, key: std::path::PathBuf) -> Result<(), String> {
        if self.windows.iter().any(|w| {
            w.id != id
                && w.content.kind() == WindowKind::Editor
                && (w.key.as_ref() == Some(&key) || w.aliases.contains(&key))
        }) || self
            .reservations
            .get(&key)
            .is_some_and(|owner| *owner != id)
        {
            return Err("That file is already open or being saved in another editor.".into());
        }
        self.reservations.insert(key, id);
        Ok(())
    }
    pub fn release_reservations(&mut self, id: WindowId) {
        self.reservations.retain(|_, owner| *owner != id);
    }
    pub fn saved_path(&mut self, id: WindowId, key: std::path::PathBuf) {
        if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
            w.aliases.clear();
            w.aliases.insert(key.clone());
            if let Content::Editor(e) = &w.content {
                if let Some(path) = &e.path {
                    w.aliases.insert(crate::requests::absolute_key(path));
                }
            }
            w.key = Some(key);
        }
        self.release_reservations(id);
    }
    pub fn editor(&self, id: WindowId) -> Option<&Editor> {
        self.windows
            .iter()
            .find(|w| w.id == id)
            .and_then(|w| match &w.content {
                Content::Editor(e) => Some(e.as_ref()),
                _ => None,
            })
    }
    pub fn editor_mut(&mut self, id: WindowId) -> Option<&mut Editor> {
        self.windows
            .iter_mut()
            .find(|w| w.id == id)
            .and_then(|w| match &mut w.content {
                Content::Editor(e) => Some(e.as_mut()),
                _ => None,
            })
    }
    pub fn viewer(&self, id: WindowId) -> Option<&Viewer> {
        self.windows
            .iter()
            .find(|w| w.id == id)
            .and_then(|w| match &w.content {
                Content::Viewer(v) => Some(v.as_ref()),
                _ => None,
            })
    }
    pub fn viewer_mut(&mut self, id: WindowId) -> Option<&mut Viewer> {
        self.windows
            .iter_mut()
            .find(|w| w.id == id)
            .and_then(|w| match &mut w.content {
                Content::Viewer(v) => Some(v.as_mut()),
                _ => None,
            })
    }
    pub fn open_editor_background(&mut self, editor: Editor) -> io::Result<WindowId> {
        self.install(Content::Editor(Box::new(editor)), false)
    }
    pub fn open_editor(&mut self, editor: Editor) -> io::Result<WindowId> {
        self.install(Content::Editor(Box::new(editor)), true)
    }
    pub fn open_viewer_background(&mut self, viewer: Viewer) -> io::Result<WindowId> {
        self.install(Content::Viewer(Box::new(viewer)), false)
    }
    pub fn open_viewer(&mut self, viewer: Viewer) -> io::Result<WindowId> {
        self.install(Content::Viewer(Box::new(viewer)), true)
    }
    pub fn open_game(&mut self, game: Game) -> io::Result<WindowId> {
        let id = self.open_plugin(crate::plugins::TETRIS)?;
        self.plugins.install_game(game);
        self.plugins.bind(crate::plugins::Binding {
            plugin: crate::plugins::TETRIS,
            owner: crate::plugins::Owner::Document(id),
        });
        Ok(id)
    }
    pub fn close_editor(&mut self) {
        let id = self.target();
        if self.editor(id).is_some() {
            self.close(id);
        }
    }
    pub fn close_viewer(&mut self) {
        let id = self.target();
        if self.viewer(id).is_some() {
            self.close(id);
        }
    }
    pub fn close_game(&mut self) {
        if let Some(id) = self.id(WindowKind::Plugin(crate::plugins::TETRIS)) {
            self.close(id);
        }
    }
    pub fn active_editor(&self) -> Option<&Editor> {
        self.editor(self.target())
    }
    pub fn active_editor_mut(&mut self) -> Option<&mut Editor> {
        let id = self.target();
        self.editor_mut(id)
    }
    pub fn active_viewer(&self) -> Option<&Viewer> {
        self.viewer(self.target())
    }
    pub fn active_viewer_mut(&mut self) -> Option<&mut Viewer> {
        let id = self.target();
        self.viewer_mut(id)
    }
    pub(crate) fn open_plugin(&mut self, id: crate::plugins::PluginId) -> io::Result<WindowId> {
        self.install(Content::Plugin(id), true)
    }
    pub fn game(&self) -> Option<&Game> {
        self.plugins.game()
    }
    pub fn game_mut(&mut self) -> Option<&mut Game> {
        self.plugins.game_mut()
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
