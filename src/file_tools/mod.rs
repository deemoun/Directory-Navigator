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
use std::{
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};

pub enum ToolDialog {
    History {
        kind: HistoryKind,
        paths: Vec<PathBuf>,
        cursor: usize,
        offset: usize,
        rows: usize,
        last_click: Option<(usize, Instant)>,
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
    pub histories: Histories,
    pub dialog: Option<ToolDialog>,
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
        }) = &mut self.dialog
        {
            dialog::paste_input(field, cursor, text);
        }
    }
}
fn prepare(app: &mut App) {
    menu::close(app);
    app.quick_search = None;
}
pub fn open_history(app: &mut App, kind: HistoryKind) {
    prepare(app);
    let paths = app.file_tools.histories.get(kind).to_vec();
    app.file_tools.dialog = Some(ToolDialog::History {
        kind,
        paths,
        cursor: 0,
        offset: 0,
        rows: 10,
        last_click: None,
    });
}
pub fn open_compare(app: &mut App) {
    prepare(app);
    if app.panels.iter().any(|p| p.loading) {
        app.error("Wait for both directory listings to finish loading.");
        return;
    }
    app.file_tools.dialog = Some(ToolDialog::Compare { focus: 0 });
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
    app.file_tools.dialog = Some(ToolDialog::Export {
        cursor: field.len(),
        field,
        focus: 0,
        base: panel.path.clone(),
        paths,
    });
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
    app.file_tools.dialog = None;
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
    match app.file_tools.dialog.take() {
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
                app.file_tools.dialog = Some(ToolDialog::Compare { focus });
                app.error("Wait for both directory listings to finish loading.");
                return;
            }
            let result = compare::apply(&mut app.panels, app.file_tools.comparison);
            app.dialogs.push(dialog::Dialog { title: " Compare directories ", message: format!("Left only: {}\nRight only: {}\nDifferent: {}\nIndeterminate: {}\n\nVisible entries only; contents were not compared.", result.left_only, result.right_only, result.different, result.indeterminate) });
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
                app.file_tools.dialog = Some(modal);
                app.error("Enter a destination filename.");
                return;
            }
            let destination = base.join(field);
            if let Err(e) = export::encode(paths, app.file_tools.full_paths) {
                app.file_tools.dialog = Some(modal);
                app.error(e.to_string());
                return;
            }
            let paths = paths.clone();
            let full_paths = app.file_tools.full_paths;
            match crate::task::Task::spawn("dn-list-export", move |_| {
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
                    app.file_tools.dialog = Some(ToolDialog::Busy);
                }
                Err(e) => {
                    app.file_tools.dialog = Some(modal);
                    app.error(e.to_string());
                }
            }
        }
        other => app.file_tools.dialog = other,
    }
}
pub fn action(app: &mut App, action: ToolAction) {
    match action {
        ToolAction::Accept => accept(app),
        ToolAction::Cancel => {
            if !app.file_tools.busy() {
                app.file_tools.dialog = None;
            }
        }
        ToolAction::Choice(index) => match &mut app.file_tools.dialog {
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
            if let Some(ToolDialog::Export { focus, .. }) = &mut app.file_tools.dialog {
                *focus = 0;
            }
        }
        ToolAction::HistoryRow(index) => {
            let mut activate = false;
            if let Some(ToolDialog::History {
                cursor,
                paths,
                last_click,
                ..
            }) = &mut app.file_tools.dialog
            {
                if index < paths.len() {
                    let now = Instant::now();
                    activate = last_click.is_some_and(|(old, time)| {
                        old == index && now.duration_since(time) <= Duration::from_millis(500)
                    });
                    *cursor = index;
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
    if key.code == KeyCode::Esc {
        action(app, ToolAction::Cancel);
        return;
    }
    if key.code == KeyCode::Enter {
        let cancel = match &app.file_tools.dialog {
            Some(ToolDialog::Compare { focus }) => *focus == 5,
            Some(ToolDialog::Export { focus, .. }) => *focus == 4,
            _ => false,
        };
        action(
            app,
            if cancel {
                ToolAction::Cancel
            } else {
                ToolAction::Accept
            },
        );
        return;
    }
    let mut choice = None;
    match &mut app.file_tools.dialog {
        Some(ToolDialog::History {
            cursor,
            paths,
            rows,
            last_click,
            ..
        }) => {
            *last_click = None;
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
        Some(ToolDialog::Compare { focus }) => match key.code {
            KeyCode::Tab | KeyCode::Down => *focus = (*focus + 1) % 6,
            KeyCode::BackTab | KeyCode::Up => *focus = (*focus + 5) % 6,
            KeyCode::Char(' ') if *focus < 4 => choice = Some(*focus),
            KeyCode::Char(c) => {
                choice = match c.to_ascii_lowercase() {
                    's' => Some(0),
                    't' => Some(1),
                    'e' => Some(2),
                    'u' => Some(3),
                    _ => None,
                }
            }
            _ => {}
        },
        Some(ToolDialog::Export {
            field,
            cursor,
            focus,
            ..
        }) => match key.code {
            KeyCode::Tab => *focus = (*focus + 1) % 5,
            KeyCode::BackTab => *focus = (*focus + 4) % 5,
            KeyCode::Char(' ') if *focus == 1 || *focus == 2 => choice = Some(*focus - 1),
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
