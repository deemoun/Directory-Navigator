use crate::filesystem::{self, Entry};
use std::{
    collections::BTreeSet,
    ffi::OsString,
    io,
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{Duration, Instant},
};

struct Request {
    path: PathBuf,
    restore: Option<OsString>,
    show_hidden: bool,
    sort: SortMode,
    cancel: crate::task::Cancellation,
    background: bool,
    recover: bool,
}

struct Response {
    request: Request,
    result: io::Result<Vec<Entry>>,
    recovered_from: Option<PathBuf>,
}

pub struct Panel {
    pub space: crate::volume_space::Snapshot,
    pub path: PathBuf,
    pub entries: Vec<Entry>,
    filtered_out: Vec<Entry>,
    pub filter: String,
    pub cursor: usize,
    pub offset: usize,
    pub loading: bool,
    pub selected: BTreeSet<OsString>,
    pub sort: SortMode,
    pub focus_after_refresh: Option<OsString>,
    pub total_files: usize,
    pub total_bytes: u128,
    pub unknown_sizes: bool,
    pub viewport_rows: usize,
    pub viewport_columns: usize,
    refresh_pending: Option<bool>,
    refreshing: bool,
    last_refresh: Instant,
    refresh_error: Option<String>,
    pub revision: u64,
    scan_cancel: crate::task::Cancellation,
    requests: Sender<Request>,
    results: Receiver<Response>,
}

impl Panel {
    pub fn new(path: PathBuf, show_hidden: bool) -> io::Result<Self> {
        let (requests, jobs) = mpsc::channel::<Request>();
        let (results, responses) = mpsc::channel();
        thread::Builder::new()
            .name("dn-directory".into())
            .spawn(move || {
                while let Ok(mut request) = jobs.recv() {
                    let original = request.path.clone();
                    let mut recovered_from = None;
                    let result = loop {
                        let result = filesystem::scan_unordered(
                            &request.path,
                            request.show_hidden,
                            &request.cancel,
                        );
                        if request.recover
                            && result.as_ref().is_err_and(|e| {
                                matches!(
                                    e.kind(),
                                    io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                                )
                            })
                        {
                            if let Some(parent) = request.path.parent() {
                                request.path = parent.into();
                                recovered_from = Some(original.clone());
                                continue;
                            }
                        }
                        break result;
                    }
                    .map(|mut entries| {
                        sort_entries(&mut entries, request.sort);
                        entries
                    });
                    if results
                        .send(Response {
                            request,
                            result,
                            recovered_from,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })?;
        let mut panel = Self {
            space: Default::default(),
            path: path.clone(),
            entries: Vec::new(),
            filtered_out: Vec::new(),
            filter: String::new(),
            cursor: 0,
            offset: 0,
            loading: false,
            selected: BTreeSet::new(),
            sort: SortMode::Name,
            focus_after_refresh: None,
            total_files: 0,
            total_bytes: 0,
            unknown_sizes: false,
            viewport_rows: 1,
            viewport_columns: 3,
            refresh_pending: None,
            refreshing: false,
            last_refresh: Instant::now(),
            refresh_error: None,
            revision: 0,
            scan_cancel: Default::default(),
            requests,
            results: responses,
        };
        panel.request(path, None, show_hidden);
        Ok(panel)
    }

    fn request(&mut self, path: PathBuf, restore: Option<OsString>, show_hidden: bool) {
        self.request_scan(path, restore, show_hidden, false);
    }

    fn request_scan(
        &mut self,
        path: PathBuf,
        restore: Option<OsString>,
        show_hidden: bool,
        background: bool,
    ) {
        self.scan_cancel.cancel();
        self.scan_cancel = Default::default();
        if self
            .requests
            .send(Request {
                recover: path == self.path,
                path,
                restore,
                show_hidden,
                sort: self.sort,
                cancel: self.scan_cancel.clone(),
                background,
            })
            .is_ok()
        {
            self.loading = !background;
            self.refreshing = background;
        }
    }

    pub fn poll(&mut self) -> Option<String> {
        match self.results.try_recv() {
            Ok(response) => {
                if response.request.cancel.cancelled() {
                    return None;
                }
                self.loading = false;
                self.refreshing = false;
                self.last_refresh = Instant::now();
                match response.result {
                    Ok(mut entries) => {
                        self.refresh_error = None;
                        let same_path = self.path == response.request.path;
                        let old_offset = self.offset;
                        if self.sort != response.request.sort {
                            sort_entries(&mut entries, self.sort);
                        }
                        if self.path != response.request.path {
                            self.selected.clear();
                            self.filter.clear();
                        } else {
                            let names: BTreeSet<_> =
                                entries.iter().map(|entry| &entry.name).collect();
                            self.selected.retain(|name| names.contains(name));
                        }
                        self.cursor = self
                            .focus_after_refresh
                            .take()
                            .or_else(|| {
                                if response.request.background {
                                    self.entries.get(self.cursor).map(|e| e.name.clone())
                                } else {
                                    response.request.restore
                                }
                            })
                            .as_ref()
                            .and_then(|name| entries.iter().position(|e| &e.name == name))
                            .unwrap_or_else(|| {
                                if self.path == response.request.path {
                                    self.cursor.min(entries.len().saturating_sub(1))
                                } else {
                                    0
                                }
                            });
                        self.entries = entries;
                        self.filtered_out.clear();
                        self.apply_filter_current();
                        self.path = response.request.path;
                        self.offset = if same_path { old_offset } else { 0 };
                        self.keep_visible_grid(self.viewport_rows, self.viewport_columns);
                        self.revision = self.revision.wrapping_add(1);
                        if let Some(hidden) = self.refresh_pending.take() {
                            self.refresh(hidden);
                        }
                        response.recovered_from.map(|path| {
                            format!(
                                "{} is no longer a directory. Showing {} instead.",
                                path.display(),
                                self.path.display()
                            )
                        })
                    }
                    Err(error) => {
                        self.focus_after_refresh = None;
                        let message = format!("{}: {error}", response.request.path.display());
                        let duplicate = response.request.background
                            && self.refresh_error.as_ref() == Some(&message);
                        self.refresh_error = Some(message.clone());
                        if duplicate {
                            None
                        } else {
                            Some(message)
                        }
                    }
                }
            }
            Err(mpsc::TryRecvError::Disconnected) if self.loading || self.refreshing => {
                self.loading = false;
                self.refreshing = false;
                Some("Directory worker stopped. Restart the application.".into())
            }
            Err(_) => None,
        }
    }

    /// Passive refresh never disables navigation or paints a loading indicator.
    pub fn refresh_if_due(&mut self, show_hidden: bool, now: Instant) {
        if !self.loading
            && !self.refreshing
            && now.saturating_duration_since(self.last_refresh) >= Duration::from_secs(2)
        {
            self.request_scan(self.path.clone(), None, show_hidden, true);
        }
    }

    pub fn background_refreshing(&self) -> bool {
        self.refreshing
    }

    /// Move entries between visible and excluded vectors; native paths are never cloned.
    fn apply_filter_current(&mut self) {
        let name = self.entries.get(self.cursor).map(|e| e.name.clone());
        let needs_sort = !self.filtered_out.is_empty();
        self.entries.append(&mut self.filtered_out);
        let matcher = crate::pattern::Wildcard::new(if self.filter.is_empty() {
            "*"
        } else {
            &self.filter
        });
        let (mut visible, excluded): (Vec<_>, Vec<_>) = self
            .entries
            .drain(..)
            .partition(|entry| entry.parent || matcher.matches(&entry.name));
        if needs_sort {
            sort_entries(&mut visible, self.sort);
        }
        self.entries = visible;
        self.filtered_out = excluded;
        self.cursor = name
            .and_then(|name| self.entries.iter().position(|e| e.name == name))
            .unwrap_or(0);
        self.offset = 0;
        self.total_files = 0;
        self.total_bytes = 0;
        self.unknown_sizes = false;
        for entry in self.entries.iter().filter(|e| !e.parent && !e.directory) {
            self.total_files += 1;
            if let Some(size) = entry.size {
                self.total_bytes += u128::from(size);
            } else {
                self.unknown_sizes = true;
            }
        }
    }
    pub fn set_filter(&mut self, pattern: String) {
        if self.loading {
            return;
        }
        self.filter = if pattern.is_empty() || pattern == "*" {
            String::new()
        } else {
            pattern
        };
        self.apply_filter_current();
    }
    pub fn filtered_marks(&self) -> usize {
        self.filtered_out
            .iter()
            .filter(|e| self.selected.contains(&e.name))
            .count()
    }
    pub fn only_filtered_marks(&self) -> bool {
        !self.selected.is_empty()
            && !self
                .entries
                .iter()
                .any(|e| !e.parent && self.selected.contains(&e.name))
    }
    pub fn select_visible(&mut self) {
        self.selected.extend(
            self.entries
                .iter()
                .filter(|e| !e.parent)
                .map(|e| e.name.clone()),
        );
    }
    pub fn invert_visible(&mut self) {
        for entry in self.entries.iter().filter(|e| !e.parent) {
            if !self.selected.remove(&entry.name) {
                self.selected.insert(entry.name.clone());
            }
        }
    }
    pub fn replace_visible_marks(&mut self, marks: &BTreeSet<OsString>) {
        for e in self.entries.iter().filter(|e| !e.parent) {
            self.selected.remove(&e.name);
        }
        self.selected.extend(marks.iter().cloned());
    }

    pub fn step(&mut self, amount: isize) {
        self.cursor = self
            .cursor
            .saturating_add_signed(amount)
            .min(self.entries.len().saturating_sub(1));
    }

    pub fn enter(&mut self, show_hidden: bool) {
        if self.loading {
            return;
        }
        if let Some(entry) = self.entries.get(self.cursor) {
            if entry.parent {
                self.parent(show_hidden);
            } else if entry.directory || entry.symlink {
                self.request(entry.path.clone(), None, show_hidden);
            }
        }
    }

    pub fn parent(&mut self, show_hidden: bool) {
        if self.loading {
            return;
        }
        if let Some(parent) = self.path.parent() {
            self.request(
                parent.into(),
                self.path.file_name().map(OsString::from),
                show_hidden,
            );
        }
    }

    pub fn refresh(&mut self, show_hidden: bool) {
        if self.loading {
            self.refresh_pending = Some(show_hidden);
        } else {
            let name = self.entries.get(self.cursor).map(|e| e.name.clone());
            self.request(self.path.clone(), name, show_hidden);
        }
    }

    pub fn toggle_selection(&mut self) {
        if self.loading {
            return;
        }
        if let Some(entry) = self.entries.get(self.cursor).filter(|e| !e.parent) {
            if !self.selected.remove(&entry.name) {
                self.selected.insert(entry.name.clone());
            }
        }
    }

    pub fn targets(&self) -> Vec<PathBuf> {
        if self.loading {
            return Vec::new();
        }
        self.entries
            .iter()
            .filter(|e| {
                !e.parent
                    && if self.selected.is_empty() {
                        self.entries
                            .get(self.cursor)
                            .is_some_and(|current| current.path == e.path)
                    } else {
                        self.selected.contains(&e.name)
                    }
            })
            .map(|e| e.path.clone())
            .collect()
    }

    pub fn set_sort(&mut self, mode: SortMode) {
        let name = self.entries.get(self.cursor).map(|e| e.name.clone());
        self.sort = mode;
        sort_entries(&mut self.entries, mode);
        self.cursor = name
            .and_then(|name| self.entries.iter().position(|e| e.name == name))
            .unwrap_or(0);
    }

    pub fn go_to(&mut self, path: PathBuf, hidden: bool) {
        if !self.loading {
            self.request(path, None, hidden);
        }
    }

    pub fn move_column(&mut self, right: bool) {
        let next = if right {
            self.cursor.checked_add(self.viewport_rows)
        } else {
            self.cursor.checked_sub(self.viewport_rows)
        };
        if let Some(next) = next.filter(|next| *next < self.entries.len()) {
            self.cursor = next;
        }
    }

    pub fn keep_visible_grid(&mut self, rows: usize, columns: usize) {
        self.viewport_rows = rows.max(1);
        self.viewport_columns = columns.max(1);
        if columns == 1 {
            self.keep_visible(rows);
            return;
        }
        if rows == 0 {
            return;
        }
        self.offset = self.offset / rows * rows;
        if self.cursor < self.offset {
            self.offset = self.cursor / rows * rows;
        }
        if self.cursor >= self.offset.saturating_add(rows * columns) {
            self.offset = (self.cursor / rows + 1 - columns) * rows;
        }
    }

    pub fn keep_visible(&mut self, rows: usize) {
        if self.cursor < self.offset {
            self.offset = self.cursor;
        }
        if rows > 0 && self.cursor >= self.offset.saturating_add(rows) {
            self.offset = self.cursor + 1 - rows;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortMode {
    Name,
    Extension,
    Size,
    Date,
}

impl SortMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Extension => "Extension",
            Self::Size => "Size ↓",
            Self::Date => "Date ↓",
        }
    }
}

pub fn sort_entries(entries: &mut [Entry], mode: SortMode) {
    entries.sort_by(|a, b| {
        b.parent
            .cmp(&a.parent)
            .then_with(|| b.directory.cmp(&a.directory))
            .then_with(|| match mode {
                SortMode::Name => a.name.cmp(&b.name),
                SortMode::Extension => std::path::Path::new(&a.name)
                    .extension()
                    .cmp(&std::path::Path::new(&b.name).extension())
                    .then_with(|| a.name.cmp(&b.name)),
                SortMode::Size => b.size.cmp(&a.size).then_with(|| a.name.cmp(&b.name)),
                SortMode::Date => b
                    .modified
                    .cmp(&a.modified)
                    .then_with(|| a.name.cmp(&b.name)),
            })
    });
}

impl Drop for Panel {
    fn drop(&mut self) {
        self.scan_cancel.cancel();
    }
}
