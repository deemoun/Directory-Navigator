//! Session-only switching; content and asynchronous work remain owned by Desktop.
pub(crate) mod arrangement;
pub use arrangement::Arrangement;
pub(crate) use arrangement::Error as ArrangementError;
pub mod geometry;
pub(crate) mod presentation;
pub(crate) mod workspace;
use crate::{
    app::App,
    desktop::WindowId,
    focus::{self, Focus},
    mouse::Target,
};
use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use std::time::Instant;

/// Transient menus are dismissible; decisions and plugin pages remain blocking.
pub(crate) fn blocked(app: &App) -> bool {
    app.exit_pending
        || app.desktop.native().manipulating()
        || app.modal_top().is_some_and(|m| {
            !matches!(
                m.focus,
                Focus::Menu | Focus::EditorMenu | Focus::QuickSearch
            )
        })
        || app
            .desktop
            .native()
            .top_surface()
            .is_some_and(|s| matches!(s.id.key, geometry::SurfaceKey::Page(..)))
}
#[derive(Default)]
pub(crate) struct ActivationState {
    revision: u64,
}
/// Reconcile every native selection transition, including opening, reuse,
/// retirement fallback and identity coalescing. Never select from rendering.
pub(crate) fn reconcile_activation(app: &mut App) {
    let revision = app.desktop.selection_revision();
    if app.activation.revision == revision {
        return;
    }
    app.activation.revision = revision;
    crate::menu::close(app);
    app.modals.set_quick_search(None);
    for id in app.desktop.document_ids(crate::desktop::WindowKind::Editor) {
        if let Some(e) = app.desktop.editor_mut(id) {
            e.modals.set_editor_menu(None);
        }
    }
    crate::button_feedback::cancel(app);
    app.shortcuts.clear();
    app.mouse.reset_click();
    app.layout_revision = app
        .layout_revision
        .checked_add(1)
        .expect("layout revision exhausted");
    crate::plugins::host::update_activity(app, Instant::now());
}
pub(crate) fn activate(app: &mut App, id: WindowId) -> bool {
    if blocked(app) {
        return false;
    }
    restore_focus(app, id)
}
/// Only lifecycle guards and modal-parent restoration bypass blocking policy.
pub(crate) fn restore_focus(app: &mut App, id: WindowId) -> bool {
    let changed = app.desktop.activate(id);
    reconcile_activation(app);
    changed
}
pub(crate) fn cycle(app: &mut App, back: bool) {
    if blocked(app) {
        return;
    }
    let ids = ids(app);
    let current = ids
        .iter()
        .position(|id| *id == app.desktop.active().id)
        .unwrap_or(0);
    if let Some(id) = ids.get(crate::control_focus::cycle(current, ids.len(), back)) {
        activate(app, *id);
    }
}
pub struct WindowList {
    pub ids: Vec<WindowId>,
    pub cursor: usize,
    pub offset: usize,
    pub rows: usize,
    pub focus: usize,
    last_click: Option<(WindowId, Instant)>,
}
impl WindowList {
    pub fn keep_visible(&mut self) {
        crate::list_state::ListViewport::fit(
            &mut self.cursor,
            &mut self.offset,
            self.rows.max(1),
            Some(self.ids.len().saturating_sub(1)),
            None,
        );
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    List,
    Next,
    Previous,
    Row(WindowId),
    Switch,
    Move,
    Zoom,
    Cancel,
    Tile,
    Cascade,
}
fn ids(app: &App) -> Vec<WindowId> {
    let mut windows = app.desktop.windows().map(|w| w.id).collect::<Vec<_>>();
    windows.sort();
    windows
}
pub fn open(app: &mut App) {
    if blocked(app) {
        return;
    }
    if app.exit_pending
        || !matches!(
            focus::current(app),
            Focus::Manager
                | Focus::QuickSearch
                | Focus::Menu
                | Focus::Editor
                | Focus::EditorMenu
                | Focus::Viewer
                | Focus::Plugin
                | Focus::Tree
                | Focus::Discovery
                | Focus::Archive
                | Focus::FileInfo
                | Focus::Utility
                | Focus::Navigation
                | Focus::FileTools
                | Focus::CommandHistory
                | Focus::Message
        )
    {
        return;
    }
    crate::menu::close(app);
    if let Some(editor) = app.desktop.active_editor_mut() {
        editor.modals.set_editor_menu(None);
    }
    app.modals.set_quick_search(None);
    app.shortcuts.clear();
    app.mouse.reset_click();
    let ids = ids(app);
    let cursor = ids
        .iter()
        .position(|id| *id == app.desktop.active().id)
        .unwrap_or(0);
    app.modals.set_window_list(Some(WindowList {
        ids,
        cursor,
        offset: 0,
        rows: 6,
        focus: 0,
        last_click: None,
    }));
}
/// A load can fail while the list is open. Never reuse its old row target.
pub fn poll(app: &mut App) -> bool {
    if app.modals.window_list().is_none() {
        return false;
    }
    let current = ids(app);
    let Some(list) = app.modals.window_list_mut() else {
        return false;
    };
    if list.ids == current {
        return false;
    }
    let selected = list.ids.get(list.cursor).copied();
    list.cursor = selected
        .and_then(|id| current.iter().position(|i| *i == id))
        .unwrap_or(0);
    list.ids = current;
    list.last_click = None;
    list.keep_visible();
    true
}
pub fn action(app: &mut App, action: Action) {
    match action {
        Action::List => open(app),
        Action::Next => cycle(app, false),
        Action::Previous => cycle(app, true),
        Action::Tile => arrange(app, Arrangement::Tile),
        Action::Cascade => arrange(app, Arrangement::Cascade),
        Action::Move | Action::Zoom => {
            let id = app
                .modals
                .window_list()
                .and_then(|l| l.ids.get(l.cursor))
                .copied()
                .unwrap_or_else(|| app.desktop.active().id);
            if id == WindowId::MANAGER || !app.desktop.contains(id) {
                return;
            }
            app.modals.set_window_list(None);
            activate(app, id);
            crate::layout::update(app);
            if let Some(surface) = app
                .desktop
                .native()
                .surface(geometry::SurfaceKey::Document(id))
            {
                if action == Action::Move {
                    app.desktop.native().begin_move(surface.id, None);
                } else {
                    workspace::zoom(app, surface);
                }
            }
        }
        Action::Cancel => {
            app.modals.set_window_list(None);
        }
        Action::Switch => {
            let selected = app
                .modals
                .window_list()
                .and_then(|l| l.ids.get(l.cursor))
                .copied();
            if let Some(id) = selected.filter(|id| app.desktop.contains(*id)) {
                app.modals.set_window_list(None);
                activate(app, id);
            } else {
                poll(app);
            }
        }
        Action::Row(id) => {
            if !app.desktop.contains(id) {
                return;
            }
            if let Some(list) = app.modals.window_list_mut() {
                if let Some(row) = list.ids.iter().position(|i| *i == id) {
                    list.cursor = row;
                    list.focus = 0;
                }
            }
        }
    }
    if let Some(list) = app.modals.window_list_mut() {
        list.keep_visible();
    }
    app.shortcuts.clear();
    app.mouse.reset_click();
}
pub fn key(app: &mut App, key: KeyEvent) {
    if let Some(m) = app.modals.window_list_mut() {
        m.last_click = None;
    }
    if crate::dialog_controls::key(app, key, crate::focus::Focus::WindowList) {
        return;
    }
    let Some(list) = app.modals.window_list_mut() else {
        return;
    };
    list.last_click = None;
    let navigation = match key.code {
        KeyCode::PageUp => KeyCode::Home,
        KeyCode::PageDown => KeyCode::End,
        code => code,
    };
    if let Some(row) =
        crate::turbo::widgets::list_key(list.cursor, list.ids.len(), list.rows, navigation)
    {
        list.cursor = row;
        list.focus = 0;
        list.keep_visible();
        return;
    }
    if let Some(list) = app.modals.window_list_mut() {
        list.keep_visible();
    }
}
pub fn mouse(app: &mut App, event: MouseEvent, target: Option<Target>) {
    if event.kind != MouseEventKind::Down(MouseButton::Left) {
        return;
    }
    let Some(Target::Window(command)) = target else {
        return;
    };
    let double = if let Action::Row(id) = command {
        if !app.desktop.contains(id) {
            return;
        }
        let Some(list) = app.modals.window_list_mut() else {
            return;
        };
        let now = Instant::now();
        let double =
            crate::list_state::ClickTracker::STRICT.matches(list.last_click.as_ref(), &id, now);
        list.last_click = Some((id, now));
        double
    } else {
        false
    };
    action(app, command);
    if double {
        action(app, Action::Switch);
    }
}

pub(crate) fn move_active(app: &mut App, zoom: bool) {
    crate::menu::close(app);
    if let Some(editor) = app.desktop.active_editor_mut() {
        editor.modals.set_editor_menu(None);
    }
    app.sync_modals();
    crate::layout::update(app);
    if let Some(s) = workspace::foreground(app) {
        if zoom {
            workspace::zoom(app, s);
        } else {
            app.desktop.native().begin_move(s.id, None);
        }
    }
}

/// Arrangement changes only presentation, never activation, MRU or document state.
pub fn arrange(app: &mut App, mode: Arrangement) {
    if app.exit_pending
        || app.output_visible
        || app.screensaver.active
        || app.desktop.native().manipulating()
        || !matches!(
            focus::current(app),
            Focus::Manager
                | Focus::QuickSearch
                | Focus::Menu
                | Focus::Editor
                | Focus::EditorMenu
                | Focus::Viewer
                | Focus::Plugin
                | Focus::Tree
                | Focus::Discovery
                | Focus::Archive
                | Focus::FileInfo
                | Focus::Utility
                | Focus::Navigation
                | Focus::FileTools
                | Focus::CommandHistory
                | Focus::Message
                | Focus::WindowList
        )
    {
        return;
    }
    crate::menu::close(app);
    if let Some(editor) = app.desktop.active_editor_mut() {
        editor.modals.set_editor_menu(None);
    }
    app.modals.set_quick_search(None);
    app.modals.set_window_list(None);
    app.sync_modals();
    if app.layout_key.is_some() {
        crate::layout::update(app);
    } else {
        let (w, h) = app.utilities.size;
        crate::layout::prepare(app, crate::presentation::layout::Rect::new(0, 0, w, h));
    }
    match app.desktop.native().arrange_documents(mode) {
        Ok(_) => {
            crate::layout::update(app);
        }
        Err(ArrangementError::NoRoom) => {
            app.error("Not enough desktop space to arrange windows. Enlarge the terminal or close some document windows.");
        }
        Err(ArrangementError::NoWindows) => {
            app.notice = "No document windows are open.".into();
        }
        Err(ArrangementError::Blocked) => {
            app.notice = "Close the current dialog before arranging windows.".into();
        }
    }
    app.shortcuts.clear();
    app.mouse.reset_click();
}
pub(crate) fn can_arrange(app: &App) -> bool {
    app.desktop.native().arrangeable_count() > 0
}
