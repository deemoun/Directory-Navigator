pub mod clipboard;
pub mod compare;
pub mod export;
pub mod history;
use crate::{
    app::App,
    dialog, menu,
    viewer::{ViewMode, Viewer},
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use history::{Histories, HistoryKind};
use std::{path::PathBuf, sync::mpsc, time::Instant};

pub enum ToolDialog {
    History {
        kind: HistoryKind,
        paths: Vec<PathBuf>,
        cursor: usize,
        offset: usize,
        rows: usize,
        last_click: Option<(usize, Instant)>,
        focus: usize,
    },
    Compare {
        focus: usize,
    },
    Export {
        field: String,
        cursor: usize,
        focus: usize,
        base: PathBuf,
        paths: Vec<PathBuf>,
    },
    Busy,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolAction {
    Accept,
    Cancel,
    Choice(usize),
    HistoryRow(usize),
    Field,
}
#[derive(Default)]
pub struct Tools {
    pub(crate) requests: crate::requests::RequestScope,
    pub modals: crate::modals::ModalStack,
    pub histories: Histories,
    pub comparison: compare::CompareOptions,
    pub full_paths: bool,
    worker: Option<crate::task::Task<Result<PathBuf, String>>>,
}
impl Tools {
    pub fn busy(&self) -> bool {
        self.worker.is_some()
    }
    pub fn paste(&mut self, text: &str) {
        if let Some(ToolDialog::Export {
            field,
            cursor,
            focus: 0,
            ..
        }) = self.modals.file_tools_mut()
        {
            dialog::paste_input(field, cursor, text);
        }
    }
}
fn prepare(app: &mut App) {
    menu::close(app);
    app.modals.set_quick_search(None);
}
pub fn open_history(app: &mut App, kind: HistoryKind) {
    prepare(app);
    let paths = app.file_tools.histories.get(kind).to_vec();
    app.file_tools
        .modals
        .set_file_tools(Some(ToolDialog::History {
            kind,
            paths,
            cursor: 0,
            offset: 0,
            rows: 10,
            last_click: None,
            focus: 0,
        }));
}
pub fn open_compare(app: &mut App) {
    prepare(app);
    if app.panels.iter().any(|p| p.loading) {
        app.error("Wait for both directory listings to finish loading.");
        return;
    }
    app.file_tools
        .modals
        .set_file_tools(Some(ToolDialog::Compare { focus: 0 }));
}
pub fn open_export(app: &mut App) {
    prepare(app);
    let panel = &app.panels[app.active];
    if panel.loading {
        return;
    }
    let paths = panel.targets();
    if paths.is_empty() {
        app.error(if panel.only_filtered_marks() {
            "All marked entries are filtered out. Clear the filter or marks first."
        } else {
            "Select an entry first; the parent directory cannot be exported."
        });
        return;
    }
    let field = "files.lst".to_string();
    app.file_tools
        .modals
        .set_file_tools(Some(ToolDialog::Export {
            cursor: field.len(),
            field,
            focus: 0,
            base: panel.path.clone(),
            paths,
        }));
}
pub fn poll(app: &mut App) -> bool {
    let result = app
        .file_tools
        .worker
        .as_ref()
        .and_then(|worker| match worker.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err("List export worker stopped unexpectedly.".into()))
            }
        });
    let Some(result) = result else {
        return false;
    };
    app.file_tools.worker = None;
    app.file_tools.modals.set_file_tools(None);
    match result {
        Ok(path) => {
            app.notice = format!("File list saved: {}", path.display());
            if let Some(parent) = path.parent() {
                for panel in &mut app.panels {
                    if panel.path == parent
                        || std::fs::canonicalize(&panel.path).ok().as_deref() == Some(parent)
                    {
                        panel.refresh(app.config.show_hidden);
                    }
                }
            }
        }
        Err(message) => app.error(message),
    }
    if app.exit_pending {
        app.request_exit();
    }
    true
}
pub fn accept(app: &mut App) {
    match app.file_tools.modals.take_file_tools() {
        Some(ToolDialog::History {
            kind,
            paths,
            cursor,
            ..
        }) => {
            let Some(path) = paths.get(cursor).cloned() else {
                return;
            };
            match kind {
                HistoryKind::Directory => {
                    app.panels[app.active].go_to(path, app.config.show_hidden)
                }
                HistoryKind::Edit => crate::editor::open_path(app, path),
                HistoryKind::View => match Viewer::open_mode(path, ViewMode::Auto) {
                    Ok(viewer) => app.show_viewer(viewer),
                    Err(e) => app.error(e.to_string()),
                },
            }
        }
        Some(ToolDialog::Compare { focus }) => {
            if app.panels.iter().any(|p| p.loading) {
                app.file_tools
                    .modals
                    .set_file_tools(Some(ToolDialog::Compare { focus }));
                app.error("Wait for both directory listings to finish loading.");
                return;
            }
            let result = compare::apply(&mut app.panels, app.file_tools.comparison);
            app.push_dialog(dialog::Dialog { title: " Compare directories ", message: format!("Left only: {}\nRight only: {}\nDifferent: {}\nIndeterminate: {}\n\nVisible entries only; contents were not compared.", result.left_only, result.right_only, result.different, result.indeterminate) });
        }
        Some(modal @ ToolDialog::Export { .. }) => {
            let ToolDialog::Export {
                ref field,
                ref base,
                ref paths,
                ..
            } = modal
            else {
                return;
            };
            if field.trim().is_empty() {
                app.file_tools.modals.set_file_tools(Some(modal));
                app.error("Enter a destination filename.");
                return;
            }
            let destination = base.join(field);
            if let Err(e) = export::encode(paths, app.file_tools.full_paths) {
                app.file_tools.modals.set_file_tools(Some(modal));
                app.error(e.to_string());
                return;
            }
            let paths = paths.clone();
            let full_paths = app.file_tools.full_paths;
            match app.file_tools.requests.spawn("dn-list-export", move |_| {
                let result = export::write_list(&destination, &paths, full_paths)
                    .map(|()| {
                        destination
                            .parent()
                            .and_then(|p| std::fs::canonicalize(p).ok())
                            .and_then(|p| destination.file_name().map(|n| p.join(n)))
                            .unwrap_or_else(|| destination.clone())
                    })
                    .map_err(|e| format!("{}: {e}", destination.display()));
                result
            }) {
                Ok(receive) => {
                    app.file_tools.worker = Some(receive);
                    app.file_tools.modals.set_file_tools(Some(ToolDialog::Busy));
                }
                Err(e) => {
                    app.file_tools.modals.set_file_tools(Some(modal));
                    app.error(e.to_string());
                }
            }
        }
        other => app.file_tools.modals.set_file_tools(other),
    }
}
pub fn action(app: &mut App, action: ToolAction) {
    match action {
        ToolAction::Accept => accept(app),
        ToolAction::Cancel => {
            if !app.file_tools.busy() {
                app.file_tools.modals.set_file_tools(None);
            }
        }
        ToolAction::Choice(index) => match app.file_tools.modals.file_tools_mut() {
            Some(ToolDialog::Compare { focus }) => {
                *focus = index;
                match index {
                    0 => app.file_tools.comparison.size ^= true,
                    1 => app.file_tools.comparison.time ^= true,
                    2 => app.file_tools.comparison.unselect = false,
                    3 => app.file_tools.comparison.unselect = true,
                    _ => {}
                }
            }
            Some(ToolDialog::Export { focus, .. }) => {
                *focus = index + 1;
                app.file_tools.full_paths = index == 1;
            }
            _ => {}
        },
        ToolAction::Field => {
            if let Some(ToolDialog::Export { focus, .. }) = app.file_tools.modals.file_tools_mut() {
                *focus = 0;
            }
        }
        ToolAction::HistoryRow(index) => {
            let mut activate = false;
            if let Some(ToolDialog::History {
                cursor,
                paths,
                last_click,
                focus,
                ..
            }) = app.file_tools.modals.file_tools_mut()
            {
                if index < paths.len() {
                    let now = Instant::now();
                    activate = crate::list_state::ClickTracker::FILE_HISTORY.matches(
                        last_click.as_ref(),
                        &index,
                        now,
                    );
                    *cursor = index;
                    *focus = 0;
                    *last_click = Some((index, now));
                }
            }
            if activate {
                accept(app);
            }
        }
    }
}
pub fn key(app: &mut App, key: KeyEvent) {
    if let Some(ToolDialog::History { last_click, .. }) = app.file_tools.modals.file_tools_mut() {
        *last_click = None;
    }
    if crate::dialog_controls::key(app, key, crate::focus::Focus::FileTools) {
        return;
    }
    let mut choice = None;
    match app.file_tools.modals.file_tools_mut() {
        Some(ToolDialog::History {
            cursor,
            paths,
            rows,
            last_click,
            focus,
            ..
        }) => {
            *last_click = None;
            if *focus != 0 {
                return;
            }
            let amount = match key.code {
                KeyCode::Up => -1,
                KeyCode::Down => 1,
                KeyCode::PageUp => -(*rows as isize),
                KeyCode::PageDown => *rows as isize,
                KeyCode::Home => isize::MIN,
                KeyCode::End => isize::MAX,
                _ => 0,
            };
            *cursor = cursor
                .saturating_add_signed(amount)
                .min(paths.len().saturating_sub(1));
        }
        Some(ToolDialog::Compare { .. }) => {
            if let KeyCode::Char(c) = key.code {
                choice = match c.to_ascii_lowercase() {
                    's' => Some(0),
                    't' => Some(1),
                    'e' => Some(2),
                    'u' => Some(3),
                    _ => None,
                };
            }
        }
        Some(ToolDialog::Export {
            field,
            cursor,
            focus,
            ..
        }) => match key.code {
            code if *focus == 0
                && !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                dialog::edit_input(field, cursor, code)
            }
            _ => {}
        },
        _ => {}
    }
    if let Some(index) = choice {
        action(app, ToolAction::Choice(index));
    }
}
pub fn mouse(app: &mut App, event: MouseEvent, target: Option<crate::mouse::Target>) {
    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if let Some(crate::mouse::Target::Tool(action_id)) = target {
                action(app, action_id);
            }
        }
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
            let code = if event.kind == MouseEventKind::ScrollUp {
                KeyCode::Up
            } else {
                KeyCode::Down
            };
            for _ in 0..3 {
                key(app, KeyEvent::new(code, KeyModifiers::NONE));
            }
        }
        _ => {}
    }
}
