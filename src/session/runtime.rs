use super::*;
use crate::{
    app::App,
    desktop::WindowId,
    requests::RequestScope,
    task::{Cancellation, Task},
    viewer::ViewMode,
};
use std::{collections::VecDeque, io, path::PathBuf};
type LoadedEditor = (
    crate::editor::buffer::Buffer,
    crate::editor::storage::Snapshot,
    TextFormat,
);
enum Prepared {
    Editor(LoadedEditor),
    Viewer(crate::viewer::Contents),
}
type PanelTask = ([PanelRecord; 2], Option<Task<io::Result<[PathBuf; 2]>>>);
struct Pending {
    index: usize,
    record: DocumentRecord,
    task: Option<Task<Result<(PathBuf, Prepared), String>>>,
}
#[derive(Default)]
pub(crate) struct Runtime {
    scope: RequestScope,
    panels: Option<PanelTask>,
    queue: VecDeque<(usize, DocumentRecord)>,
    pending: Option<Pending>,
    positioned: Vec<(WindowId, DocumentRecord)>,
    panel_positions: [Option<PanelRecord>; 2],
    active_index: Option<usize>,
    active_id: Option<WindowId>,
    pub(crate) interacted: bool,
    started: bool,
    skipped: usize,
    pub(crate) quit_layout: Option<Vec<(WindowId, Option<Geometry>)>>,
    pub(crate) quit_active: Option<WindowId>,
}
impl Runtime {
    pub(crate) fn cancel_restore(&mut self) {
        self.panels = None;
        self.queue.clear();
        self.pending = None;
        self.positioned.clear();
        self.panel_positions = [None, None];
        self.started = false;
    }

    pub(crate) fn busy(&self) -> bool {
        self.panels.is_some()
            || !self.queue.is_empty()
            || self.pending.is_some()
            || !self.positioned.is_empty()
            || self.panel_positions.iter().any(Option::is_some)
    }
}
pub(crate) fn capture(app: &App) -> io::Result<Option<Snapshot>> {
    if app.config.restore_session == RestoreLevel::Off {
        return Ok(None);
    }
    let panels = std::array::from_fn(|i| {
        let p = &app.panels[i];
        if let Some(a) = p.archive() {
            PanelRecord {
                path: a.path.parent().unwrap_or(&p.path).to_owned(),
                cursor: a.path.file_name().map(PathBuf::from),
                filter: String::new(),
                offset: 0,
            }
        } else {
            PanelRecord {
                path: p.path.clone(),
                cursor: p
                    .entries
                    .get(p.cursor)
                    .filter(|e| !e.parent)
                    .map(|e| PathBuf::from(&e.name)),
                filter: p.filter.clone(),
                offset: p.offset,
            }
        }
    });
    let order = app
        .session
        .quit_layout
        .clone()
        .unwrap_or_else(|| layout(app));
    let active = app.session.quit_active.unwrap_or(app.desktop.active().id);
    let mut documents = Vec::new();
    let mut active_document = None;
    if app.config.restore_session == RestoreLevel::Documents {
        for (id, geometry) in order {
            let record = if let Some(e) = app.desktop.editor(id) {
                if e.is_smartpad() || e.archive_origin.is_some() || e.recovery_source.is_some() {
                    continue;
                }
                let (Some(path), Some(b)) = (&e.path, &e.buffer) else {
                    continue;
                };
                DocumentRecord {
                    path: path.clone(),
                    kind: Kind::Editor,
                    geometry,
                    format: Some(e.text_format()),
                    position: b.row(),
                    column: b.column(),
                    scroll: e.scroll_row,
                    horizontal: u16::try_from(e.scroll_column).unwrap_or(u16::MAX),
                    mode: ViewMode::Text,
                    markdown: false,
                    wrap: false,
                }
            } else if let Some(v) = app.desktop.viewer(id) {
                let Some(path) = v.native_path() else {
                    continue;
                };
                let Some(c) = &v.contents else {
                    continue;
                };
                let position = if c.binary {
                    v.source_line().saturating_mul(16)
                } else if let Some(pdf) = &c.pdf {
                    pdf.page_lines
                        .partition_point(|line| *line <= v.source_line())
                        .saturating_sub(1)
                } else {
                    v.source_line()
                };
                DocumentRecord {
                    path: path.to_owned(),
                    kind: Kind::Viewer,
                    geometry,
                    format: c.format,
                    position,
                    column: 0,
                    scroll: 0,
                    horizontal: v.horizontal,
                    mode: v.requested_mode,
                    markdown: v.markdown.rendered,
                    wrap: v.wrap,
                }
            } else {
                continue;
            };
            if documents.len() == MAX_DOCUMENTS {
                return Err(io::Error::other(
                    "Session exceeds 128 documents; close documents or choose Panels only",
                ));
            }
            if id == active {
                active_document = Some(documents.len());
            }
            documents.push(record);
        }
    }
    Ok(Some(Snapshot {
        panels,
        active_panel: app.active,
        documents,
        active_document,
    }))
}
pub(crate) fn layout(app: &App) -> Vec<(WindowId, Option<Geometry>)> {
    app.desktop
        .native()
        .z_order()
        .into_iter()
        .map(|id| (id, app.desktop.native().session_geometry(id)))
        .collect()
}
pub(crate) fn interaction(app: &mut App) {
    app.session.interacted = true;
}
pub(crate) fn begin(app: &mut App, bypass: bool) {
    if bypass || app.config.restore_session == RestoreLevel::Off {
        return;
    }
    let Some(snapshot) = app.config.session.clone() else {
        if app.config.invalid_session {
            app.notice = app
                .config
                .language
                .text("Saved session is invalid; started a fresh session. Check Restore previous session in System Setup.")
                .into();
        }
        return;
    };
    let mut state = Runtime {
        started: true,
        active_index: snapshot.active_document,
        ..Default::default()
    };
    let records = snapshot.panels;
    let paths = records.clone();
    let fallback = app.panels[0].path.clone();
    match state.scope.spawn("session-panels", move |cancel| {
        let mut result = [fallback.clone(), fallback.clone()];
        for i in 0..2 {
            let mut p = paths[i].path.clone();
            loop {
                cancel.check()?;
                if std::fs::read_dir(&p).is_ok() {
                    result[i] = p;
                    break;
                }
                if !p.pop() {
                    break;
                }
            }
        }
        Ok(result)
    }) {
        Ok(task) => state.panels = Some((records, Some(task))),
        Err(_) => state.skipped += 2,
    }
    app.active = snapshot.active_panel;
    if app.config.restore_session == RestoreLevel::Documents {
        state.queue = snapshot.documents.into_iter().enumerate().collect();
    }
    app.session = state;
}
fn prepare(record: &DocumentRecord, cancel: &Cancellation) -> Result<(PathBuf, Prepared), String> {
    let key = std::fs::canonicalize(&record.path).map_err(|e| e.to_string())?;
    let prepared = match record.kind {
        Kind::Editor => {
            crate::editor::storage::load_format_cancelled(&record.path, record.format, cancel)
                .map(Prepared::Editor)
        }
        Kind::Viewer => {
            crate::viewer::session_load(&record.path, record.mode, record.format, cancel)
                .map(Prepared::Viewer)
                .map_err(|e| e.to_string())
        }
    }?;
    Ok((key, prepared))
}
pub(crate) fn poll(app: &mut App) -> bool {
    let mut state = std::mem::take(&mut app.session);
    let mut changed = false;
    // Only a foreground user action, never background opening, releases prompts.
    if state.interacted && app.modals.infos().next().is_none() {
        if let Some(v) = app.desktop.active_viewer_mut() {
            changed |= v.session_release_prompts();
        }
    }
    if app.panels.iter().all(|p| !p.loading) {
        if let Some(reply) = state
            .panels
            .as_mut()
            .and_then(|(_, task)| crate::task::take_reply(task))
        {
            let (records, _) = state.panels.take().unwrap();
            match reply {
                Ok(Ok(paths)) => {
                    for (i, (r, path)) in records.into_iter().zip(paths).enumerate() {
                        if state.interacted {
                            state.skipped += 1;
                            continue;
                        }
                        if r.path != path {
                            state.skipped += 1;
                        }
                        app.panels[i].go_to(path, app.config.show_hidden);
                        state.panel_positions[i] = Some(r);
                    }
                    changed = true;
                }
                _ => state.skipped += 2,
            }
        }
    }
    for i in 0..2 {
        if !app.panels[i].loading {
            if let Some(r) = state.panel_positions[i].take() {
                if !state.interacted {
                    let p = &mut app.panels[i];
                    p.set_filter(r.filter);
                    p.cursor = r
                        .cursor
                        .as_ref()
                        .and_then(|name| {
                            p.entries
                                .iter()
                                .position(|e| Some(e.name.as_os_str()) == name.file_name())
                        })
                        .unwrap_or(0);
                    p.offset = r.offset.min(p.entries.len().saturating_sub(1));
                    p.selected.clear();
                    p.keep_visible_grid(p.viewport_rows.max(1), p.viewport_columns.max(1));
                    changed = true;
                }
            }
        }
    }
    if let Some(reply) = state
        .pending
        .as_mut()
        .and_then(|p| crate::task::take_reply(&mut p.task))
    {
        let pending = state.pending.take().unwrap();
        let Pending { index, record, .. } = pending;
        match reply {
            Ok(Ok((key, prepared))) => {
                let kind = if record.kind == Kind::Editor {
                    crate::desktop::WindowKind::Editor
                } else {
                    crate::desktop::WindowKind::Viewer
                };
                if app.desktop.find_path(kind, &key).is_some()
                    || app.desktop.find_path(kind, &record.path).is_some()
                {
                    state.skipped += 1;
                } else {
                    let installed = match prepared {
                        Prepared::Editor(loaded) => crate::editor::Editor::session_document(
                            record.path.clone(),
                            loaded,
                            app.config.editor,
                        )
                        .and_then(|e| {
                            app.install_document(|d| d.open_editor_background(e))
                                .map_err(|e| e.to_string())
                        }),
                        Prepared::Viewer(contents) => {
                            let v = crate::viewer::Viewer::session_document(
                                record.path.clone(),
                                contents,
                                &record,
                            );
                            app.install_document(|d| d.open_viewer_background(v))
                                .map_err(|e| e.to_string())
                        }
                    };
                    if let Ok(id) = installed {
                        if Some(index) == state.active_index {
                            state.active_id = Some(id);
                        }
                        state.positioned.push((id, record));
                    } else {
                        state.skipped += 1;
                    }
                }
            }
            _ => state.skipped += 1,
        }
        changed = true;
    }
    if state.pending.is_none() && state.positioned.is_empty() {
        if let Some((index, record)) = state.queue.pop_front() {
            let request = record.clone();
            match state
                .scope
                .spawn("session-document", move |cancel| prepare(&request, &cancel))
            {
                Ok(task) => {
                    state.pending = Some(Pending {
                        index,
                        record,
                        task: Some(task),
                    })
                }
                Err(_) => state.skipped += 1,
            }
            changed = true;
        }
    }
    state.positioned.retain(|(id, r)| {
        if !app.desktop.contains(*id) || (state.interacted && app.desktop.active().id == *id) {
            return false;
        }
        if app
            .desktop
            .native()
            .surface(crate::windowing::geometry::SurfaceKey::Document(*id))
            .is_none()
        {
            return true;
        }
        if let Some(g) = r.geometry {
            app.desktop.native().restore_session_geometry(*id, g);
        }
        if let Some(e) = app.desktop.editor_mut(*id) {
            if let Some(b) = &mut e.buffer {
                let row = r.position.min(b.lines.len().saturating_sub(1));
                b.cursor = b.position_at(row, r.column);
                b.anchor = None;
                e.scroll_row = r.scroll.min(b.lines.len().saturating_sub(1));
                e.scroll_column = r.horizontal as usize;
            }
        }
        if let Some(v) = app.desktop.viewer_mut(*id) {
            if !v.session_ready() {
                return true;
            }
            v.session_position(r);
        }
        changed = true;
        false
    });
    if state.started && !state.busy() {
        if !state.interacted && app.modals.infos().next().is_none() {
            if let Some(id) = state.active_id {
                crate::windowing::activate(app, id);
            }
        }
        if state.skipped > 0 {
            app.notice = format!(
                "{} {} · {}",
                app.config
                    .language
                    .text("Session restored; unavailable items skipped:"),
                state.skipped,
                app.config
                    .language
                    .text("Reopen unavailable files from the panels.")
            );
        }
        state.started = false;
        changed = true;
    }
    app.session = state;
    changed
}

pub(crate) fn remember_quit(app: &mut App) {
    app.session.quit_layout = Some(layout(app));
    app.session.quit_active = Some(app.desktop.active().id);
}
