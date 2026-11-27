//! Read-only, session-only cache inventory. Filesystem work belongs to workers.
use crate::{
    app::App,
    platform_fs::{entry_identity, EntryIdentity},
    plugins::{Binding, Owner, CACHE_LOCATIONS},
    task::Cancellation,
    traversal::usage::Measurement,
};
use std::{
    path::PathBuf,
    sync::{mpsc::TryRecvError, Arc, Mutex},
};
mod catalog;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Status {
    #[default]
    Unresolved,
    Missing,
    Inaccessible,
    Unavailable,
    NotAnalyzed,
    Queued,
    Scanning,
    Complete,
    Partial,
    Cancelled,
    Limited,
    Changed,
}
impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unresolved => "Unresolved",
            Self::Missing => "Missing",
            Self::Inaccessible => "Access denied",
            Self::Unavailable => "Unavailable",
            Self::NotAnalyzed => "Not analyzed",
            Self::Queued => "Queued",
            Self::Scanning => "Scanning",
            Self::Complete => "Complete",
            Self::Partial => "Partial",
            Self::Cancelled => "Cancelled — partial",
            Self::Limited => "Limited — partial",
            Self::Changed => "Directory changed",
        }
    }
}
#[derive(Clone, Debug)]
pub(crate) struct Location {
    pub name: &'static str,
    pub purpose: &'static str,
    pub path: Option<PathBuf>,
    pub identity: Option<EntryIdentity>,
    pub status: Status,
    pub measurement: Option<Measurement>,
    pub stale: bool,
    pub cause: String,
}
impl Location {
    pub fn can_open(&self) -> bool {
        self.path.is_some() && self.identity.is_some() && self.status != Status::Changed
    }
    fn new(name: &'static str, purpose: &'static str, path: Option<PathBuf>) -> Self {
        Self {
            name,
            purpose,
            path,
            identity: None,
            status: Status::Unresolved,
            measurement: None,
            stale: false,
            cause: String::new(),
        }
    }
}
#[derive(Clone, Default)]
pub(crate) struct Snapshot {
    pub generation: u64,
    pub rows: Vec<Location>,
    pub busy: bool,
    pub stopping: bool,
    pub notice: String,
    pub error: String,
}
#[derive(Clone, Copy)]
pub(crate) enum Action {
    Refresh,
    Analyze(Option<usize>),
    Stop,
    Open(usize),
    Copy(usize),
}
struct Service {
    binding: Binding,
    refreshing: bool,
    worker: crate::workers::Worker<Snapshot>,
    latest: Arc<Mutex<Option<Snapshot>>>,
}
#[derive(Default)]
pub(crate) struct State {
    pub view: Snapshot,
    requests: crate::requests::RequestScope,
    service: Option<Service>,
    panel: usize,
}
fn alive(app: &App, binding: Binding) -> bool {
    binding.plugin == CACHE_LOCATIONS
        && app.config.plugin_enabled(CACHE_LOCATIONS)
        && app.desktop.plugins.binding(CACHE_LOCATIONS) == Some(binding)
        && matches!(binding.owner, Owner::Document(id) if app.desktop.contains(id))
}
pub(crate) fn opened(app: &mut App, b: Binding) {
    app.cache_locations = State {
        panel: app.active,
        ..Default::default()
    };
    action(app, b, 0, Action::Refresh);
}
fn publish(latest: &Mutex<Option<Snapshot>>, view: &Snapshot) {
    if let Ok(mut slot) = latest.lock() {
        *slot = Some(view.clone());
    }
}
fn cancel_refresh(view: &mut Snapshot) {
    view.busy = false;
    view.stopping = false;
    view.error.clear();
    for row in &mut view.rows {
        if row.measurement.is_some() {
            row.stale = true;
        }
    }
    view.notice = "Refresh cancelled; previous locations retained. Measurements are stale.".into();
}
fn refresh(
    mut view: Snapshot,
    cancel: &Cancellation,
    discover: impl FnOnce(&Cancellation) -> Vec<Location>,
) -> Snapshot {
    if cancel.cancelled() {
        cancel_refresh(&mut view);
        return view;
    }
    let mut rows = discover(cancel);
    if cancel.cancelled() {
        cancel_refresh(&mut view);
        return view;
    }
    for row in &mut rows {
        if let Some(previous) = view
            .rows
            .iter()
            .find(|p| p.identity.is_some() && p.identity == row.identity)
        {
            row.measurement = previous.measurement.clone();
            row.stale = row.measurement.is_some();
            if row.stale {
                row.status = previous.status;
                row.cause.clone_from(&previous.cause);
            }
        }
    }
    view.rows = rows;
    view.busy = false;
    view.stopping = false;
    view.notice = "Select a location and Analyze; no files are deleted.".into();
    view
}
fn scan(
    mut view: Snapshot,
    selected: Option<usize>,
    cancel: Cancellation,
    latest: Arc<Mutex<Option<Snapshot>>>,
) -> Snapshot {
    let indices: Vec<_> = (0..view.rows.len())
        .filter(|i| selected.is_none_or(|s| s == *i) && view.rows[*i].identity.is_some())
        .collect();
    for &i in &indices {
        view.rows[i].status = Status::Queued;
        view.rows[i].stale = view.rows[i].measurement.is_some();
    }
    publish(&latest, &view);
    for i in indices {
        if cancel.cancelled() {
            break;
        }
        let row = &mut view.rows[i];
        let root = row.path.clone().expect("resolved directory");
        if entry_identity(&root).ok() != row.identity
            || std::fs::symlink_metadata(&root)
                .is_ok_and(|m| crate::platform_fs::directory_link(&m))
        {
            row.status = Status::Changed;
            row.stale = true;
            continue;
        }
        row.status = Status::Scanning;
        row.measurement = None;
        row.stale = false;
        row.cause.clear();
        publish(&latest, &view);
        let mut root_value = None;
        let (stats, limited) = crate::traversal::usage::collect(&root, &cancel, |update| {
            let row = &mut view.rows[i];
            for (path, value) in update.records {
                if path == root {
                    root_value = Some(value);
                }
            }
            row.measurement = Some(Measurement {
                bytes: update.bytes,
                files: update.files,
                skipped: update.skipped,
                excluded: update.excluded,
                partial: true,
                ..Default::default()
            });
            publish(&latest, &view);
            true
        });
        let row = &mut view.rows[i];
        row.measurement = root_value;
        if stats.visited == 0 && stats.skipped > 0 {
            row.measurement = None;
        }
        row.cause = stats
            .first_error
            .unwrap_or_default()
            .chars()
            .take(2048)
            .collect();
        row.status = if entry_identity(&root).ok() != row.identity {
            row.stale = true;
            Status::Changed
        } else if cancel.cancelled() {
            Status::Cancelled
        } else if limited {
            Status::Limited
        } else if row.measurement.as_ref().is_none_or(|m| m.partial) {
            Status::Partial
        } else {
            Status::Complete
        };
        publish(&latest, &view);
    }
    for row in &mut view.rows {
        if matches!(row.status, Status::Queued | Status::Scanning) {
            row.status = Status::Cancelled;
        }
    }
    view.busy = false;
    view.stopping = false;
    view.notice = if cancel.cancelled() {
        "Analysis stopped; incomplete measurements are partial."
    } else {
        "Snapshot ready; locations can overlap. No grand total."
    }
    .into();
    view
}
pub(crate) fn action(app: &mut App, b: Binding, generation: u64, a: Action) {
    if !alive(app, b) || generation != app.cache_locations.view.generation {
        return;
    }
    if matches!(a, Action::Stop) {
        if let Some(service) = &app.cache_locations.service {
            service.worker.cancel();
            app.cache_locations.view.stopping = true;
            app.cache_locations.view.notice = "Stopping…".into();
        }
        return;
    }
    if let Action::Open(i) | Action::Copy(i) = a {
        let Some(row) = app.cache_locations.view.rows.get(i) else {
            return;
        };
        let Some(path) = row.path.clone() else {
            return;
        };
        if matches!(a, Action::Copy(_)) {
            if let Some(text) = path.to_str() {
                crate::clipboard::copy_plugin(app, b, generation, text.to_owned());
            } else {
                app.cache_locations.view.notice =
                    "This native path cannot be represented as clipboard text.".into();
            }
        } else {
            if !row.can_open() {
                return;
            }
            let panel = app.cache_locations.panel;
            if app.panels[panel].loading {
                app.cache_locations.view.notice =
                    "Wait for directory navigation to finish, then Open again.".into();
                return;
            }
            app.panels[panel].go_to(path, app.config.show_hidden);
            app.active = panel;
            crate::windowing::activate(app, crate::desktop::WindowId::MANAGER);
        }
        return;
    }
    if app.cache_locations.view.busy {
        return;
    }
    let Owner::Document(owner) = b.owner else {
        return;
    };
    let generation = generation.wrapping_add(1);
    let mut view = app.cache_locations.view.clone();
    view.generation = generation;
    view.busy = true;
    view.stopping = false;
    view.error.clear();
    view.notice = if matches!(a, Action::Refresh) {
        "Discovering standard locations…"
    } else {
        "Analyzing apparent bytes…"
    }
    .into();
    if matches!(a, Action::Refresh) {
        for row in &mut view.rows {
            if row.measurement.is_some() {
                row.stale = true;
            }
        }
    }
    let latest = Arc::new(Mutex::new(None));
    let output = latest.clone();
    let input = view.clone();
    let result = app.workers.spawn(
        "dn-cache-locations",
        &app.cache_locations.requests,
        crate::requests::Owner::Document(owner),
        "cache-locations",
        move |cancel| {
            if let Action::Analyze(selected) = a {
                return Ok(scan(input, selected, cancel, output));
            }
            Ok(refresh(input, &cancel, catalog::native))
        },
    );
    match result {
        Ok(worker) => {
            app.cache_locations.view = view;
            app.cache_locations.service = Some(Service {
                binding: b,
                refreshing: matches!(a, Action::Refresh),
                worker,
                latest,
            });
        }
        Err(error) => {
            app.cache_locations.view.error = error.to_string().chars().take(2048).collect();
            app.cache_locations.view.notice =
                "Could not start inspection; see Help for the original error.".into();
        }
    }
}
pub(crate) fn poll(app: &mut App) -> bool {
    let Some(service) = &app.cache_locations.service else {
        return false;
    };
    if !alive(app, service.binding) || app.quit {
        app.cache_locations = State::default();
        return true;
    }
    let mut changed = false;
    if let Some(mut view) = service.latest.try_lock().ok().and_then(|mut s| s.take()) {
        if view.generation == app.cache_locations.view.generation {
            view.stopping = app.cache_locations.view.stopping;
            app.cache_locations.view = view;
            changed = true;
        }
    }
    match service.worker.try_recv() {
        Err(TryRecvError::Empty) => changed,
        result => {
            match result {
                Ok(Ok(view))
                    if view.generation == app.cache_locations.view.generation
                        && service.refreshing
                        && app.cache_locations.view.stopping =>
                {
                    cancel_refresh(&mut app.cache_locations.view);
                    app.cache_locations.view.generation = view.generation.wrapping_add(1);
                }
                Ok(Ok(mut view)) if view.generation == app.cache_locations.view.generation => {
                    // A refreshed catalog can reorder rows. Retire actions prepared
                    // for the previous snapshot before publishing any completion.
                    view.generation = view.generation.wrapping_add(1);
                    app.cache_locations.view = view
                }
                Ok(Err(error))
                    if service.refreshing && error.kind() == std::io::ErrorKind::Interrupted =>
                {
                    cancel_refresh(&mut app.cache_locations.view);
                    app.cache_locations.view.generation =
                        app.cache_locations.view.generation.wrapping_add(1);
                }
                Ok(Err(error)) => failed(
                    &mut app.cache_locations.view,
                    error.kind() == std::io::ErrorKind::Interrupted,
                    &error.to_string(),
                ),
                Err(TryRecvError::Disconnected) => failed(
                    &mut app.cache_locations.view,
                    false,
                    "Background inspection ended unexpectedly",
                ),
                _ => {}
            }
            app.cache_locations.view.busy = false;
            app.cache_locations.service = None;
            true
        }
    }
}
fn failed(view: &mut Snapshot, cancelled: bool, cause: &str) {
    view.generation = view.generation.wrapping_add(1);
    view.stopping = false;
    view.error = cause.chars().take(2048).collect();
    view.notice = if cancelled {
        "Analysis stopped; incomplete measurements are partial."
    } else {
        "Inspection failed; incomplete measurements are partial."
    }
    .into();
    for row in &mut view.rows {
        if matches!(row.status, Status::Scanning | Status::Queued) {
            row.status = if cancelled {
                Status::Cancelled
            } else {
                Status::Partial
            };
            if let Some(value) = &mut row.measurement {
                value.partial = true;
            }
            row.cause = cause.chars().take(2048).collect();
        }
    }
}
#[cfg(test)]
mod tests;
