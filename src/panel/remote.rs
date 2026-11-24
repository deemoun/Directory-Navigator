//! A pane's remote directory and owned, generation-checked listing request.
use super::*;
use crate::network::{
    remote::{Connection, Member, RemotePath},
    task::Task,
};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
pub struct Remote {
    pub connection: Arc<Connection>,
    pub directory: RemotePath,
    pub generation: u64,
    pub revision: u64,
    pub hidden: bool,
    pub members: HashMap<OsString, Member>,
    pub status: crate::network::status::Snapshot,
    pub issue: Option<crate::network::status::Failure>,
    pub failed_path: Option<RemotePath>,
    pending: Option<Task<io::Result<Vec<Member>>>>,
    target: RemotePath,
    origin: Origin,
}
struct Origin {
    entries: Vec<Entry>,
    excluded: Vec<Entry>,
    selected: BTreeSet<OsString>,
    filter: String,
    types_filter: Option<filter::TypesFilter>,
    cursor: usize,
    offset: usize,
}
impl Panel {
    pub fn remote(&self) -> Option<&Remote> {
        if let Location::Remote(r) = &self.location {
            Some(r)
        } else {
            None
        }
    }
    pub(crate) fn remote_mut(&mut self) -> Option<&mut Remote> {
        if let Location::Remote(r) = &mut self.location {
            Some(r)
        } else {
            None
        }
    }
    pub fn is_remote(&self) -> bool {
        self.remote().is_some()
    }
    pub fn is_virtual(&self) -> bool {
        self.is_archive() || self.is_remote()
    }
    pub(crate) fn open_remote(
        &mut self,
        connection: Arc<Connection>,
        hidden: bool,
    ) -> io::Result<()> {
        if self.is_archive() {
            self.leave_archive(hidden);
        }
        if self.is_remote() {
            self.leave_remote(hidden);
        }
        self.scan_cancel.cancel();
        self.scope.start("directory-scan");
        let directory = connection.profile.directory.clone();
        let origin = Origin {
            entries: std::mem::take(&mut self.entries),
            excluded: std::mem::take(&mut self.filtered_out),
            selected: std::mem::take(&mut self.selected),
            filter: std::mem::take(&mut self.filter),
            types_filter: self.types_filter.take(),
            cursor: self.cursor,
            offset: self.offset,
        };
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let generation = NEXT.fetch_add(1, Ordering::Relaxed);
        self.location = Location::Remote(Box::new(Remote {
            status: connection.snapshot(),
            issue: None,
            failed_path: None,
            connection,
            directory: directory.clone(),
            target: directory.clone(),
            generation,
            revision: 0,
            hidden,
            members: HashMap::new(),
            pending: None,
            origin,
        }));
        self.cursor = 0;
        self.offset = 0;
        match self.remote_request(directory) {
            Ok(()) => Ok(()),
            Err(e) => {
                self.leave_remote(hidden);
                Err(e)
            }
        }
    }
    fn remote_request(&mut self, path: RemotePath) -> io::Result<()> {
        let connection = self.remote().expect("remote pane").connection.clone();
        let target = path.clone();
        let task = match Task::spawn(&self.scope, "dn-remote-directory", move |cancel| {
            connection.with(|backend| backend.list(&target, &cancel))
        }) {
            Ok(task) => task,
            Err(e) => {
                let r = self.remote_mut().unwrap();
                r.issue = Some(crate::network::status::Failure::new(&e));
                r.failed_path = Some(path);
                self.revision = self.revision.wrapping_add(1);
                return Err(e);
            }
        };
        let remote = self.remote_mut().unwrap();
        remote.pending = Some(task);
        remote.target = path;
        remote.revision += 1;
        self.loading = true;
        Ok(())
    }
    pub(crate) fn remote_poll(&mut self) -> Option<String> {
        let status = self.remote()?.connection.snapshot();
        if self.remote()?.status != status {
            self.remote_mut()?.status = status;
            self.revision = self.revision.wrapping_add(1);
        }
        if let Some(error) = self.refresh_error.take() {
            return Some(error);
        }
        let reply = {
            let remote = self.remote_mut()?;
            let task = remote.pending.as_ref()?;
            match task.try_recv() {
                Ok(r) => Some(r),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                Err(_) => Some(Err(io::Error::other(
                    "Remote listing cancelled or worker stopped",
                ))),
            }
        }?;
        self.loading = false;
        let remote = self.remote_mut().unwrap();
        remote.pending = None;
        // The worker may finish between the initial snapshot and receiving its
        // result. Publish the matching connection state in this same update.
        remote.status = remote.connection.snapshot();
        match reply {
            Err(e) => {
                remote.issue = Some(crate::network::status::Failure::new(&e));
                remote.failed_path = Some(remote.target.clone());
                let label = remote.connection.label(&remote.target);
                if self.entries.is_empty() {
                    self.entries.push(parent_entry());
                }
                self.revision = self.revision.wrapping_add(1);
                Some(format!("{label}: {e}"))
            }
            Ok(members) => {
                let mut unique = std::collections::HashSet::new();
                for member in &members {
                    if member.path.parent() != remote.target
                        || !unique.insert(member.path.name().to_owned())
                    {
                        let e = io::Error::new(
                            io::ErrorKind::InvalidData,
                            "Remote listing contains duplicate or redirected names",
                        );
                        remote.issue = Some(crate::network::status::Failure::new(&e));
                        remote.failed_path = Some(remote.target.clone());
                        self.revision = self.revision.wrapping_add(1);
                        return Some(e.to_string());
                    }
                }
                remote.issue = None;
                remote.failed_path = None;
                let same_directory = remote.directory == remote.target;
                let selected_name = self.entries.get(self.cursor).map(|e| e.name.clone());
                let old_offset = self.offset;
                let remote = self.remote_mut().unwrap();
                remote.directory = remote.target.clone();
                remote.revision += 1;
                remote.members.clear();
                let mut entries = vec![parent_entry()];
                for member in members {
                    if !remote.hidden && member.path.name().starts_with('.') {
                        continue;
                    }
                    let name = OsString::from(member.path.name());
                    if remote
                        .members
                        .insert(name.clone(), member.clone())
                        .is_some()
                    {
                        return Some("Remote directory contains duplicate names".into());
                    }
                    entries.push(Entry {
                        file_type: None,
                        name,
                        path: PathBuf::new(),
                        directory: member.directory,
                        symlink: member.link,
                        size: member.size,
                        parent: false,
                        modified: member.modified,
                        executable: false,
                    });
                }
                self.entries = entries;
                self.filtered_out.clear();
                if same_directory {
                    let names = self
                        .remote()
                        .unwrap()
                        .members
                        .keys()
                        .cloned()
                        .collect::<std::collections::HashSet<_>>();
                    self.selected.retain(|name| names.contains(name));
                } else {
                    self.selected.clear();
                }
                sort_entries(&mut self.entries, self.sort);
                self.cursor = if same_directory {
                    selected_name
                        .and_then(|n| self.entries.iter().position(|e| e.name == n))
                        .unwrap_or(0)
                } else {
                    0
                };
                self.offset = 0;
                self.apply_filter_current();
                if same_directory {
                    self.offset = old_offset;
                }
                self.revision += 1;
                self.refresh_error = None;
                None
            }
        }
    }
    pub(crate) fn remote_enter(&mut self, hidden: bool) {
        if self.loading {
            return;
        }
        let Some(entry) = self.entries.get(self.cursor) else {
            return;
        };
        if entry.parent {
            self.remote_parent(hidden);
            return;
        }
        if entry.directory && !entry.symlink {
            let path = self
                .remote()
                .unwrap()
                .members
                .get(&entry.name)
                .map(|m| m.path.clone());
            if let Some(path) = path {
                if let Err(e) = self.remote_request(path) {
                    self.refresh_error = Some(e.to_string());
                }
            }
        }
    }
    pub(crate) fn remote_parent(&mut self, hidden: bool) {
        if self.loading {
            return;
        }
        let Some(remote) = self.remote() else {
            return;
        };
        if remote.directory.as_str() == "/" {
            self.leave_remote(hidden);
        } else {
            let path = remote.directory.parent();
            if let Err(e) = self.remote_request(path) {
                self.refresh_error = Some(e.to_string());
            }
        }
    }
    pub(crate) fn remote_refresh(&mut self) {
        let Some(remote) = self.remote() else {
            return;
        };
        let path = remote.directory.clone();
        if let Err(e) = self.remote_request(path) {
            self.refresh_error = Some(e.to_string());
        }
    }
    pub(crate) fn remote_cancel(&mut self) {
        let Some(remote) = self.remote_mut() else {
            return;
        };
        remote.pending = None;
        remote.target = remote.directory.clone();
        remote.revision = remote.revision.wrapping_add(1);
        remote.issue = Some(crate::network::status::Failure::new(&io::Error::new(
            io::ErrorKind::Interrupted,
            "Remote directory request cancelled",
        )));
        self.loading = false;
        self.revision = self.revision.wrapping_add(1);
        if self.entries.is_empty() {
            self.entries.push(parent_entry());
        }
    }
    pub(crate) fn reconnect_remote(&mut self, connection: Arc<Connection>) -> io::Result<()> {
        let Some(remote) = self.remote_mut() else {
            return Err(io::Error::other("Remote pane is no longer open"));
        };
        let directory = remote.directory.clone();
        remote.pending = None;
        remote.connection = connection;
        remote.status = remote.connection.snapshot();
        remote.issue = None;
        remote.failed_path = None;
        // Preserve this pane's identity/view; old tasks are invalidated by a new lease.
        self.remote_request(directory)
    }
    pub(crate) fn remote_error_report(&self, language: crate::localization::Language) -> String {
        let r = self.remote().expect("remote error");
        let issue = r.issue.as_ref();
        crate::error_report::Report::new(
            "Cannot load the remote directory. The remote location is retained.",
            issue.map_or("", |i| i.cause.as_str()),
            issue
                .map_or(crate::network::status::Problem::Other, |i| i.problem)
                .advice(),
        )
        .text(
            "Remote directory",
            &r.connection
                .label(r.failed_path.as_ref().unwrap_or(&r.directory)),
        )
        .render(language)
    }
    pub fn leave_remote(&mut self, hidden: bool) {
        if !self.is_remote() {
            return;
        }
        let Location::Remote(remote) = std::mem::take(&mut self.location) else {
            return;
        };
        self.entries = remote.origin.entries;
        self.filtered_out = remote.origin.excluded;
        self.selected = remote.origin.selected;
        self.filter = remote.origin.filter;
        self.types_filter = remote.origin.types_filter;
        self.cursor = remote.origin.cursor;
        self.offset = remote.origin.offset;
        self.loading = false;
        self.refresh(hidden);
    }
    pub fn remote_targets(&self) -> Vec<Member> {
        let Some(remote) = self.remote() else {
            return vec![];
        };
        self.entries
            .iter()
            .filter(|e| {
                !e.parent
                    && if self.selected.is_empty() {
                        self.entries
                            .get(self.cursor)
                            .is_some_and(|c| c.name == e.name)
                    } else {
                        self.selected.contains(&e.name)
                    }
            })
            .filter_map(|e| remote.members.get(&e.name).cloned())
            .collect()
    }
}

impl Panel {
    pub(crate) fn remote_session_panel(&self) -> Option<crate::session::PanelRecord> {
        let r = self.remote()?;
        Some(crate::session::PanelRecord {
            path: self.path.clone(),
            cursor: r
                .origin
                .entries
                .get(r.origin.cursor)
                .filter(|e| !e.parent)
                .map(|e| PathBuf::from(&e.name)),
            filter: r.origin.filter.clone(),
            types_filter: r.origin.types_filter.clone(),
            offset: r.origin.offset,
        })
    }
}

fn parent_entry() -> Entry {
    Entry {
        file_type: None,
        name: "..".into(),
        path: PathBuf::new(),
        directory: true,
        symlink: false,
        size: None,
        parent: true,
        modified: None,
        executable: false,
    }
}
