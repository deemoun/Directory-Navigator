//! Semantic bridge for shared tool definitions. Existing controllers own actions.
use crate::{app::App, focus::Focus, mouse::Target, turbo::dialog_spec::input_area};
use crossterm::event::{KeyCode, KeyEvent};
pub(crate) fn key(app: &mut App, event: KeyEvent, scope: Focus) -> bool {
    if scope == Focus::Columns
        && event.code == KeyCode::Char(' ')
        && app.modals.columns_dialog().is_some_and(|m| m.cursor < 2)
    {
        return false;
    }
    let code = match (scope, event.code) {
        (Focus::Colors | Focus::SaverSetup, KeyCode::Down) => KeyCode::Tab,
        (Focus::Colors | Focus::SaverSetup, KeyCode::Up) => KeyCode::BackTab,
        (Focus::SaverSetup, KeyCode::Right) => KeyCode::Tab,
        (Focus::SaverSetup, KeyCode::Left) => KeyCode::BackTab,
        (Focus::FileTools, KeyCode::Down)
            if matches!(
                app.file_tools.modals.file_tools(),
                Some(crate::file_tools::ToolDialog::Compare { .. })
            ) =>
        {
            KeyCode::Tab
        }
        (Focus::FileTools, KeyCode::Up)
            if matches!(
                app.file_tools.modals.file_tools(),
                Some(crate::file_tools::ToolDialog::Compare { .. })
            ) =>
        {
            KeyCode::BackTab
        }
        _ => event.code,
    };
    let event = KeyEvent::new(code, event.modifiers);
    if !matches!(
        event.code,
        KeyCode::Tab | KeyCode::BackTab | KeyCode::Enter | KeyCode::Esc | KeyCode::Char(' ')
    ) {
        return false;
    }
    let proposal = {
        let Some(spec) = crate::ui::remaining_spec(app, input_area(app), scope) else {
            return false;
        };
        spec.key(event)
    };
    let Some(p) = proposal else {
        return false;
    };
    apply(app, p, tool_action);
    true
}

/// Common proposal application. State and semantic actions stay controller-owned.
pub(crate) fn apply(
    app: &mut App,
    proposal: crate::turbo::dialog_spec::Proposal,
    action: impl FnOnce(&mut App, Target),
) {
    proposal.deliver(app, |app, focus, target| {
        set_focus(app, crate::focus::current(app), focus);
        if let Some(target) = target {
            action(app, target);
        }
    });
}
fn set_focus(app: &mut App, scope: Focus, next: usize) {
    if let Some(adapter) = crate::dialog_adapter::get(scope) {
        (adapter.set_focus)(app, next);
        return;
    }
    match scope {
        Focus::Settings => {
            if let Some(m) = app.modals.settings_mut() {
                m.cursor = next;
            }
        }
        Focus::Prompt => {
            if let Some(m) = app.modals.prompt_mut() {
                m.focus = next;
            }
        }
        Focus::EditorModal => {
            if let Some(e) = app.desktop.active_editor_mut() {
                match e.modals.editor_modal_mut() {
                    Some(crate::editor::Modal::Search(d)) => d.focus = next,
                    Some(crate::editor::Modal::Replace(d)) => d.focus = next,
                    Some(
                        crate::editor::Modal::Prompt { focus: f, .. }
                        | crate::editor::Modal::ConfirmClose { focus: f },
                    ) => *f = next,
                    _ => {}
                }
            }
        }
        Focus::ViewerSearch => {
            if let Some(d) = app
                .desktop
                .active_viewer_mut()
                .and_then(|v| v.modals.viewer_search_mut())
            {
                d.focus = next;
            }
        }
        Focus::Pattern => {
            if let Some(m) = app.modals.pattern_mut() {
                m.focus = next;
            }
        }
        Focus::Columns => {
            if let Some(m) = app.modals.columns_dialog_mut() {
                m.cursor = next;
            }
        }
        Focus::Colors => {
            if let Some(m) = app.modals.color_dialog_mut() {
                m.cursor = next;
            }
        }
        Focus::SaverSetup => {
            if let Some(m) = app.screensaver.modals.screensaver_mut() {
                m.cursor = next;
            }
        }
        _ => {}
    }
}

pub(crate) fn tool_action(app: &mut App, target: Target) {
    match Some(target) {
        Some(Target::CloseDialog) => {
            app.pop_dialog();
        }
        Some(Target::CancelJob) => {
            if let Some(job) = app.job.as_ref() {
                job.cancel();
                app.notice = "Cancelling…".into();
            }
        }
        Some(Target::CloseCopyHistory) => app.modals.set_copy_history(None),
        Some(Target::CopyHistory(row)) => crate::copy_dialog::select_history(app, row),
        Some(Target::ApplyImport) => crate::config_import::apply(app),
        Some(Target::CancelImport) => app.modals.set_config_import(None),
        Some(Target::AcceptPattern) => crate::pattern::accept(app),
        Some(Target::CancelPattern) => app.modals.set_pattern(None),
        Some(Target::ColumnChoice(panel, preset)) => crate::columns::choose(app, panel, preset),
        Some(Target::AcceptColumns) => crate::columns::accept(app),
        Some(Target::CancelColumns) => crate::columns::cancel(app),
        Some(Target::CloseFileInfo) => app.modals.set_file_info(None),
        Some(Target::Tree(a)) => crate::directory_tree::action(app, a),
        Some(Target::UserMenu(a)) => crate::user_menu::action(app, a),
        Some(Target::Utility(a)) => crate::utilities::action(app, a),
        Some(Target::Window(a)) => crate::windowing::action(app, a),
        Some(Target::CommandHistory(a)) => crate::command_line::history::action(app, a),
        Some(Target::ColorChoice(profile)) => crate::palette::choose(app, profile),
        Some(Target::AcceptColors) => app.modals.set_color_dialog(None),
        Some(Target::CancelColors) => crate::palette::cancel(app),
        Some(Target::Screensaver(i)) => crate::screensaver::command(app, i),
        Some(Target::Tool(a)) => crate::file_tools::action(app, a),
        Some(Target::Archive(a)) => crate::archive::action(app, a),
        Some(Target::Discovery(a)) => crate::discovery::action(app, a),
        Some(Target::Navigation(a)) => crate::navigation_tools::action(app, a),
        _ => {}
    }
}
