//! One input-precedence policy, matching the order in which layers are painted.
use crate::app::App;
pub use crate::desktop::WindowKind as Screen;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Recovery,
    TerminalOutput,
    ConfigImport,
    Welcome,
    CommandHistory,
    Message,
    WindowList,
    SaverSetup,
    Colors,
    Utility,
    Navigation,
    FileTools,
    Discovery,
    Tree,
    UserMenu,
    Archive,
    FileInfo,
    Columns,
    Pattern,
    Settings,
    Job,
    Prompt,
    CopyHistory,
    Menu,
    QuickSearch,
    Manager,
    Editor,
    EditorModal,
    EditorMenu,
    Viewer,
    ViewerSearch,
    Plugin,
}
pub fn screen(app: &App) -> Screen {
    app.desktop.active().kind
}
pub fn current(app: &App) -> Focus {
    if app.output_visible {
        return Focus::TerminalOutput;
    }
    if let Some(modal) = app.modal_top() {
        return modal.focus;
    }
    if app.job.is_some() && screen(app) == Screen::Manager {
        return Focus::Job;
    }
    match screen(app) {
        Screen::Manager => Focus::Manager,
        Screen::Editor => Focus::Editor,
        Screen::Viewer => Focus::Viewer,
        Screen::Plugin(_) => Focus::Plugin,
        Screen::Tool(kind) => kind.focus(),
    }
}
