//! Registration of ordinary owned dialogs. Global input precedence and special
//! native file/help behavior stay in the host; registered hooks are owner-local.
use crate::{
    app::App, focus::Focus, mouse::Target, painting::Frame, presentation::layout::Rect,
    turbo::dialog_spec::DialogSpec,
};
use crossterm::event::{KeyEvent, MouseEvent};
pub(crate) struct Adapter {
    pub focus: Focus,
    pub key: fn(&mut App, KeyEvent),
    pub mouse: fn(&mut App, MouseEvent, Option<Target>),
    pub spec: for<'a> fn(&'a App, Rect) -> Option<DialogSpec<'a>>,
    pub set_focus: fn(&mut App, usize),
}
pub(crate) fn get(focus: Focus) -> Option<&'static Adapter> {
    ADAPTERS.iter().find(|a| a.focus == focus)
}
pub(crate) fn key(app: &mut App, event: KeyEvent, focus: Focus) -> bool {
    if let Some(a) = get(focus) {
        (a.key)(app, event);
        true
    } else {
        false
    }
}
pub(crate) fn mouse(
    app: &mut App,
    event: MouseEvent,
    target: Option<Target>,
    focus: Focus,
) -> bool {
    if let Some(a) = get(focus) {
        (a.mouse)(app, event, target);
        true
    } else {
        false
    }
}
pub(crate) fn paint(frame: &mut impl Frame, app: &App, area: Rect, focus: Focus) -> bool {
    if let Some(a) = get(focus) {
        if let Some(spec) = (a.spec)(app, area) {
            spec.paint(frame, app);
        }
        true
    } else {
        false
    }
}

const ADAPTERS: &[Adapter] = &[
    Adapter {
        focus: Focus::WindowList,
        key: crate::windowing::key,
        mouse: crate::windowing::mouse,
        spec: crate::ui::windows_spec,
        set_focus: |app, next| {
            if let Some(m) = app.modals.window_list_mut() {
                m.focus = next;
            }
        },
    },
    Adapter {
        focus: Focus::CommandHistory,
        key: crate::command_line::history::key,
        mouse: crate::command_line::history::mouse,
        spec: crate::ui::command_history_spec,
        set_focus: |app, next| {
            if let Some(m) = app.command_line.modals.command_history_mut() {
                m.focus = next;
            }
        },
    },
    Adapter {
        focus: Focus::ConfigImport,
        key: crate::config_import::key,
        mouse: crate::config_import::mouse,
        spec: crate::ui::import_spec,
        set_focus: |app, next| {
            if let Some(m) = app.modals.config_import_mut() {
                m.focus = next;
            }
        },
    },
    Adapter {
        focus: Focus::Archive,
        key: crate::archive::key,
        mouse: crate::archive::mouse,
        spec: crate::ui::archive_spec,
        set_focus: |app, next| {
            if let Some(m) = app.modals.archive_mut() {
                m.focus = next;
            }
        },
    },
    Adapter {
        focus: Focus::UserMenu,
        key: crate::user_menu::key,
        mouse: crate::user_menu::mouse,
        spec: crate::ui::user_menu_spec,
        set_focus: |app, next| {
            if let Some(m) = app.user_menu.modals.user_menu_mut() {
                m.focus = next;
            }
        },
    },
    Adapter {
        focus: Focus::Tree,
        key: crate::directory_tree::key,
        mouse: crate::directory_tree::mouse,
        spec: crate::ui::tree_spec,
        set_focus: |app, next| {
            if let Some(m) = app.modals.directory_tree_mut() {
                m.focus = next;
            }
        },
    },
    Adapter {
        focus: Focus::Discovery,
        key: crate::discovery::key,
        mouse: crate::discovery::mouse,
        spec: crate::ui::discovery_spec,
        set_focus: |app, next| {
            if let Some(m) = app.discovery.modals.discovery_mut() {
                m.focus = next;
            }
        },
    },
    Adapter {
        focus: Focus::Navigation,
        key: crate::navigation_tools::key,
        mouse: crate::navigation_tools::mouse,
        spec: crate::ui::navigation_spec,
        set_focus: |app, next| {
            if let Some(
                crate::navigation_tools::Modal::Bookmarks { focus, .. }
                | crate::navigation_tools::Modal::Filter { focus, .. },
            ) = app.modals.navigation_dialog_mut()
            {
                *focus = next;
            }
        },
    },
    Adapter {
        focus: Focus::Utility,
        key: crate::utilities::key,
        mouse: crate::utilities::mouse,
        spec: crate::ui::utility_spec,
        set_focus: |app, next| match app.utilities.modals.utility_dialog() {
            Some(crate::utilities::Kind::Characters) => app.utilities.chart.focus = next,
            _ => app.utilities.focus = next,
        },
    },
    Adapter {
        focus: Focus::FileTools,
        key: crate::file_tools::key,
        mouse: crate::file_tools::mouse,
        spec: crate::ui::file_tools_spec,
        set_focus: |app, next| {
            if let Some(
                crate::file_tools::ToolDialog::History { focus, .. }
                | crate::file_tools::ToolDialog::Compare { focus }
                | crate::file_tools::ToolDialog::Export { focus, .. },
            ) = app.file_tools.modals.file_tools_mut()
            {
                *focus = next;
            }
        },
    },
];
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registrations_are_unique_and_special_native_hosts_remain_explicit() {
        for (i, a) in ADAPTERS.iter().enumerate() {
            assert!(!ADAPTERS[..i].iter().any(|b| b.focus == a.focus));
        }
        for focus in [
            Focus::EditorModal,
            Focus::Message,
            Focus::Welcome,
            Focus::Plugin,
        ] {
            assert!(get(focus).is_none());
        }
    }
}
