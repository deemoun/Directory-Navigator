//! Session-owned tool documents and typed controller payloads.
mod access;
mod state;
use crate::{
    app::App,
    desktop::{WindowId, WindowKind},
    focus::Focus,
    presentation::layout::Rect,
    windowing::geometry::{Definition, SurfaceKey},
};
use std::path::PathBuf;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Transfers,
    Tree,
    Find,
    Sizes,
    Archive,
    FileInfo,
    Characters,
    System,
    Memory,
    Bookmarks,
    EditHistory,
    ViewHistory,
    DirectoryHistory,
    CommandHistory,
    Help,
}
impl Kind {
    pub fn focus(self) -> Focus {
        match self {
            Self::Transfers => Focus::Transfers,
            Self::Tree => Focus::Tree,
            Self::Find | Self::Sizes => Focus::Discovery,
            Self::Archive => Focus::Archive,
            Self::FileInfo => Focus::FileInfo,
            Self::Characters | Self::System | Self::Memory => Focus::Utility,
            Self::Bookmarks => Focus::Navigation,
            Self::EditHistory | Self::ViewHistory | Self::DirectoryHistory => Focus::FileTools,
            Self::CommandHistory => Focus::CommandHistory,
            Self::Help => Focus::Message,
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Self::Transfers => "Transfer Center",
            Self::Tree => "Directory tree",
            Self::Find => "Find files",
            Self::Sizes => "Directory sizes",
            Self::Archive => "Archive browser",
            Self::FileInfo => "File information",
            Self::Characters => "ASCII table",
            Self::System => "System information",
            Self::Memory => "Memory information",
            Self::Bookmarks => "Bookmarks",
            Self::EditHistory => "File Edit History",
            Self::ViewHistory => "File View History",
            Self::DirectoryHistory => "Directory history",
            Self::CommandHistory => "Command history",
            Self::Help => "Help",
        }
    }
}
pub struct ToolWindow {
    pub kind: Kind,
    pub panel: usize,
    pub source_directory: PathBuf,
    pub(crate) help_owner: Option<WindowId>,
    pub path: Option<PathBuf>,
    pub(crate) state: state::State,
    pub(crate) utility: crate::utilities::State,
}
impl ToolWindow {
    pub(crate) fn set_help(&mut self, dialog: crate::dialog::Dialog) {
        self.state.started = true;
        self.state.help = Some(state::Help::new(dialog));
    }
    pub fn tree(&self) -> Option<&crate::directory_tree::Tree> {
        self.state.directory_tree()
    }
    pub fn discovery(&self) -> Option<&crate::discovery::Modal> {
        self.state.discovery()
    }
    pub fn archive(&self) -> Option<&crate::archive::Browser> {
        self.state.archive()
    }
    pub fn file_info(&self) -> Option<&crate::file_info::InfoDialog> {
        self.state.file_info()
    }

    pub(crate) fn new(kind: Kind, panel: usize, path: Option<PathBuf>) -> Self {
        Self {
            kind,
            panel,
            source_directory: PathBuf::new(),
            help_owner: None,
            path,
            state: Default::default(),
            utility: Default::default(),
        }
    }
    pub(crate) fn bind(&mut self, id: WindowId) {
        self.state.bind(id);
    }
    fn root(&self) -> bool {
        self.kind == Kind::Transfers || self.state.alive()
    }
}
impl App {
    fn document_tool(&self, focus: Focus, workspace_present: bool) -> bool {
        !(!self.desktop.contextual() && workspace_present)
            && self
                .desktop
                .tool(self.desktop.target())
                .is_some_and(|t| t.kind.focus() == focus)
    }
    pub fn utility_dialog(&self) -> Option<crate::utilities::Kind> {
        if self.document_tool(
            Focus::Utility,
            self.utilities.modals.utility_dialog().is_some(),
        ) {
            self.desktop.tool(self.desktop.target())?.state.utility
        } else {
            self.utilities.modals.utility_dialog()
        }
    }
    pub fn set_utility_dialog(&mut self, value: Option<crate::utilities::Kind>) {
        let id = self.desktop.target();
        if self.document_tool(
            Focus::Utility,
            self.utilities.modals.utility_dialog().is_some(),
        ) {
            let t = self.desktop.tool_mut(id).unwrap();
            t.state.started |= value.is_some();
            t.state.utility = value;
        } else {
            self.utilities.modals.set_utility_dialog(value);
        }
    }
    pub fn utility_state(&self) -> &crate::utilities::State {
        self.desktop
            .tool(self.desktop.target())
            .filter(|t| t.kind.focus() == Focus::Utility)
            .map_or(&self.utilities.state, |t| &t.utility)
    }
    pub fn utility_state_mut(&mut self) -> &mut crate::utilities::State {
        let id = self.desktop.target();
        if self
            .desktop
            .tool(id)
            .is_some_and(|t| t.kind.focus() == Focus::Utility)
        {
            &mut self.desktop.tool_mut(id).unwrap().utility
        } else {
            &mut self.utilities.state
        }
    }
    pub(crate) fn help_document(&self) -> Option<&state::Help> {
        if self.modal_top().is_none() {
            self.desktop
                .tool(self.desktop.target())?
                .state
                .help
                .as_ref()
        } else {
            None
        }
    }
    pub(crate) fn help_document_mut(&mut self) -> Option<&mut state::Help> {
        let id = self.desktop.target();
        if self.modal_top().is_none() {
            self.desktop.tool_mut(id)?.state.help.as_mut()
        } else {
            None
        }
    }
    pub fn native_help(&self) -> Option<&crate::turbo::help::Browser> {
        self.help_document()
            .map(|h| &h.browser)
            .or_else(|| self.modals.native_help(self.desktop.target()))
    }
    pub fn native_help_mut(&mut self) -> Option<&mut crate::turbo::help::Browser> {
        let id = self.desktop.target();
        if self.help_document().is_some() {
            Some(&mut self.help_document_mut()?.browser)
        } else {
            self.modals.native_help_mut(id)
        }
    }
}
/// Returns true only for a newly allocated document; repeated opens activate.
pub(crate) fn open(app: &mut App, kind: Kind, path: Option<PathBuf>) -> bool {
    app.sync_modals();
    if crate::windowing::blocked(app) {
        return false;
    }
    crate::menu::close(app);
    app.modals.set_quick_search(None);
    let path = path.map(|p| crate::requests::absolute_key(&p));
    let existing = path
        .as_ref()
        .and_then(|path| app.desktop.find_path(WindowKind::Tool(kind), path))
        .or_else(|| {
            app.desktop.windows().find_map(|w| {
                app.desktop
                    .tool(w.id)
                    .filter(|t| t.kind == kind && t.path == path)
                    .map(|_| w.id)
            })
        });
    if let Some(id) = existing {
        crate::windowing::activate(app, id);
        return false;
    }
    let mut tool = ToolWindow::new(kind, app.active, path);
    tool.source_directory = app.panels[app.active].path.clone();
    match app.install_document(|desktop| desktop.open_tool(tool)) {
        Ok(_) => true,
        Err(e) => {
            app.error(e.to_string());
            false
        }
    }
}
/// Existing singleton tools do not depend on the manager's current selection
/// or a fresh directory scan being ready.
pub(crate) fn reopen(app: &mut App, kind: Kind) -> bool {
    app.sync_modals();
    let existing = app.desktop.windows().find_map(|w| {
        app.desktop
            .tool(w.id)
            .filter(|t| t.kind == kind && t.path.is_none())
            .map(|_| w.id)
    });
    if let Some(id) = existing {
        crate::windowing::activate(app, id);
        true
    } else {
        false
    }
}
pub(crate) fn reconcile(app: &mut App) {
    let ids = app
        .desktop
        .windows()
        .filter(|w| matches!(w.kind, WindowKind::Tool(_)))
        .map(|w| w.id)
        .collect::<Vec<_>>();
    for id in ids {
        if app
            .desktop
            .tool(id)
            .and_then(|t| t.help_owner)
            .is_some_and(|owner| !app.desktop.contains(owner))
        {
            app.desktop.close(id);
            continue;
        }
        let t = app.desktop.tool_mut(id).unwrap();
        if t.kind == Kind::Characters && !t.root() {
            app.utilities.chart.page = t.utility.chart.page;
            app.utilities.chart.cells = t.utility.chart.cells;
        }
        if !t.root() && t.state.started {
            app.desktop.close(id);
        }
    }
}
pub(crate) fn bounds(app: &App, area: Rect, width: u16, height: u16) -> Rect {
    if !app.desktop.contextual() && app.modal_top().is_some() {
        return crate::dialog_theme::centered(area, width, height);
    }
    if let Some(s) = app.desktop.tool(app.desktop.target()).and_then(|_| {
        app.desktop
            .native()
            .surface(SurfaceKey::Document(app.desktop.target()))
            .filter(|s| s.bounds.width >= 48 && s.bounds.height >= 10)
    }) {
        Rect::new(
            s.bounds.x,
            s.bounds.y,
            s.bounds.width,
            s.bounds.height.saturating_sub(1),
        )
    } else {
        crate::dialog_theme::centered(area, width, height)
    }
}
pub(crate) fn spec(app: &App, area: Rect) -> Option<crate::turbo::dialog_spec::DialogSpec<'_>> {
    crate::ui::remaining_spec(
        app,
        area,
        app.desktop.tool(app.desktop.target())?.kind.focus(),
    )
}
pub(crate) fn definitions(app: &App, area: Rect) -> Vec<Definition> {
    app.desktop
        .windows()
        .filter_map(|w| {
            let t = app.desktop.tool(w.id)?;
            let bounds = app.desktop.render_as(w.id, || {
                if t.kind == Kind::Help {
                    Some(crate::dialog_theme::centered(
                        area,
                        76.min(area.width),
                        22.min(area.height.saturating_sub(2)),
                    ))
                } else {
                    spec(app, area).map(|s| s.native_bounds(area))
                }
            })?;
            let policy = if t
                .discovery()
                .is_some_and(|d| d.kind == crate::discovery::Kind::FindForm)
            {
                crate::windowing::presentation::Policy::find_form()
            } else {
                crate::windowing::presentation::Policy::tool(t.kind, bounds)
            };
            Some(policy.definition(SurfaceKey::Document(w.id), bounds, None))
        })
        .collect()
}
/// Preparation and worker delivery use explicit document contexts, never native
/// selection. Rendering and completion of background tools cannot steal focus.
pub(crate) fn prepare(app: &mut App, area: Rect) {
    let profile = app.config.palette;
    let language = app.config.language;
    let shortcuts = app.config.shortcuts;
    let ids = app
        .desktop
        .windows()
        .filter(|w| matches!(w.kind, WindowKind::Tool(_)))
        .map(|w| w.id)
        .collect::<Vec<_>>();
    for id in ids {
        let Some(s) = app.desktop.native().surface(SurfaceKey::Document(id)) else {
            continue;
        };
        if app
            .desktop
            .tool(id)
            .is_some_and(|t| t.kind == Kind::Transfers)
        {
            app.transfers.rows = usize::from(s.bounds.height.saturating_sub(13)).max(1);
        }
        let t = app.desktop.tool_mut(id).unwrap();
        if let Some(tree) = t.state.directory_tree_mut() {
            tree.page_rows = usize::from(s.bounds.height.saturating_sub(10)).max(1);
            tree.keep_visible();
        }
        if let Some(b) = t.state.archive_mut() {
            b.rows = usize::from(s.bounds.height.saturating_sub(10)).max(1);
            b.keep_visible();
        }
        if let Some(d) = t.state.discovery_mut() {
            d.rows = usize::from(s.bounds.height.saturating_sub(12)).max(1);
            d.keep_visible();
        }
        if let Some(crate::file_tools::ToolDialog::History {
            rows,
            offset,
            cursor,
            paths,
            ..
        }) = t.state.file_tools_mut()
        {
            *rows = usize::from(s.bounds.height.saturating_sub(7)).max(1);
            crate::list_state::ListViewport::fit(
                cursor,
                offset,
                *rows,
                Some(paths.len().saturating_sub(1)),
                None,
            );
        }
        if let Some(info) = &mut t.utility.information {
            info.rows = usize::from(s.bounds.height.saturating_sub(7)).max(1);
            info.step(0);
        }
        if let Some(h) = t.state.command_history_mut() {
            h.rows = usize::from(s.bounds.height.saturating_sub(9)).max(1);
            h.keep_visible();
        }
        if let Some(h) = t.state.help.as_mut().map(|h| &mut h.browser) {
            h.place(s.bounds);
            h.prepare(
                crate::presentation::layout::Size::new(area.width, area.height),
                profile,
                language,
                shortcuts,
            );
        }
    }
}
pub(crate) fn poll(app: &mut App) -> bool {
    let ids = app
        .desktop
        .windows()
        .filter(|w| matches!(w.kind, WindowKind::Tool(_)))
        .map(|w| w.id)
        .collect::<Vec<_>>();
    let mut changed = false;
    for id in ids {
        let previous = app.desktop.set_context(Some(id));
        let kind = app.desktop.tool(id).unwrap().kind;
        changed |= match kind {
            Kind::Tree => app.directory_tree_mut().is_some_and(|t| t.poll()),
            Kind::Find | Kind::Sizes => crate::discovery::poll(app),
            Kind::Archive => crate::archive::poll(app),
            Kind::FileInfo => app.file_info_mut().is_some_and(|i| i.poll()),
            Kind::Bookmarks => crate::navigation_tools::poll(app),
            Kind::System | Kind::Memory => crate::utilities::poll(app),
            Kind::CommandHistory => {
                let history = app.command_line.history().to_vec();
                app.command_history_mut()
                    .is_some_and(|h| h.refresh(&history))
            }
            Kind::DirectoryHistory | Kind::EditHistory | Kind::ViewHistory => {
                let kind = match kind {
                    Kind::DirectoryHistory => crate::file_tools::history::HistoryKind::Directory,
                    Kind::EditHistory => crate::file_tools::history::HistoryKind::Edit,
                    _ => crate::file_tools::history::HistoryKind::View,
                };
                let next = app.file_tools.histories.get(kind).to_vec();
                if let Some(crate::file_tools::ToolDialog::History {
                    paths,
                    cursor,
                    offset,
                    ..
                }) = app.file_tools_mut()
                {
                    if *paths != next {
                        let selected = paths.get(*cursor).cloned();
                        *paths = next;
                        *cursor = selected
                            .and_then(|p| paths.iter().position(|v| *v == p))
                            .unwrap_or(0);
                        *offset = (*offset).min(*cursor);
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            _ => false,
        };
        app.desktop.set_context(previous);
    }
    if changed {
        app.layout_revision = app
            .layout_revision
            .checked_add(1)
            .expect("layout revision exhausted");
    }
    changed
}

pub(crate) fn failed(app: &mut App, message: String) {
    let id = app.desktop.target();
    if app
        .desktop
        .tool(id)
        .is_some_and(|t| !t.state.started && !t.root())
    {
        app.desktop.close(id);
    }
    app.error(message);
}

/// Tool origins follow the same logical pane when the manager swaps its panes.
pub(crate) fn swap_panels(app: &mut App) {
    let ids = app
        .desktop
        .windows()
        .filter(|w| matches!(w.kind, WindowKind::Tool(_)))
        .map(|w| w.id)
        .collect::<Vec<_>>();
    for id in ids {
        let t = app.desktop.tool_mut(id).unwrap();
        t.panel ^= 1;
        if let Some(tree) = t.state.directory_tree_mut() {
            tree.panel ^= 1;
        }
        if let Some(find) = t.state.discovery_mut() {
            find.panel ^= 1;
        }
    }
    app.panels.swap(0, 1);
    app.active ^= 1;
    app.layout_revision = app
        .layout_revision
        .checked_add(1)
        .expect("layout revision exhausted");
    app.mouse.reset_click();
}
