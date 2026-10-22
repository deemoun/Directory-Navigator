//! One input-precedence policy, matching the order in which layers are painted.
use crate::app::App;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Manager,
    Viewer,
    Editor,
    Game,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    CommandHistory,
    Message,
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
    Menu,
    QuickSearch,
    Manager,
    Editor,
    EditorModal,
    EditorMenu,
    Viewer,
    ViewerSearch,
    Game,
}
pub fn screen(app: &App) -> Screen {
    if app.editor.is_some() {
        Screen::Editor
    } else if app.viewer.is_some() {
        Screen::Viewer
    } else if app.tetris.is_some() {
        Screen::Game
    } else {
        Screen::Manager
    }
}
pub fn current(app: &App) -> Focus {
    let screen = screen(app);
    if screen == Screen::Game {
        return Focus::Game;
    }
    if !app.dialogs.is_empty() {
        return Focus::Message;
    }
    if let Some(editor) = app.editor.as_ref().filter(|_| screen == Screen::Editor) {
        return if editor.modal.is_some() {
            Focus::EditorModal
        } else if editor.menu.is_some() {
            Focus::EditorMenu
        } else {
            Focus::Editor
        };
    }
    if screen == Screen::Manager {
        if app.command_line.dialog.is_some() {
            return Focus::CommandHistory;
        }
        if app.screensaver.setup.is_some() {
            return Focus::SaverSetup;
        }
        if app.color_dialog.is_some() {
            return Focus::Colors;
        }
        if app.utilities.dialog.is_some() {
            return Focus::Utility;
        }
        if app.navigation_dialog.is_some() {
            return Focus::Navigation;
        }
        if app.archive.is_some() {
            return Focus::Archive;
        }
        if app.user_menu.modal.is_some() {
            return Focus::UserMenu;
        }
        if app.directory_tree.is_some() {
            return Focus::Tree;
        }
        if app.discovery.modal.is_some() {
            return Focus::Discovery;
        }
        if app.file_tools.dialog.is_some() {
            return Focus::FileTools;
        }
    }
    if app.file_info.is_some() {
        return Focus::FileInfo;
    }
    if app.columns_dialog.is_some() {
        return Focus::Columns;
    }
    if app.pattern.is_some() {
        return Focus::Pattern;
    }
    if screen == Screen::Viewer {
        return if app
            .viewer
            .as_ref()
            .is_some_and(|v| v.search_dialog.is_some())
        {
            Focus::ViewerSearch
        } else {
            Focus::Viewer
        };
    }
    if app.settings.is_some() {
        Focus::Settings
    } else if app.job.is_some() {
        Focus::Job
    } else if app.prompt.is_some() {
        Focus::Prompt
    } else if app.menu.is_some() {
        Focus::Menu
    } else if app.quick_search.is_some() {
        Focus::QuickSearch
    } else {
        Focus::Manager
    }
}
