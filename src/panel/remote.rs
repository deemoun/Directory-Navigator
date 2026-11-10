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
    pending: Option<Task<io::Result<Vec<Member>>>>,
    target: RemotePath,
    origin: Origin,
}
struct Origin {
    entries: Vec<Entry>,
    excluded: Vec<Entry>,
    selected: BTreeSet<OsString>,
    filter: String,
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
            cursor: self.cursor,
            offset: self.offset,
        };
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let generation = NEXT.fetch_add(1, Ordering::Relaxed);
        self.location = Location::Remote(Box::new(Remote {
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
        let task = Task::spawn(&self.scope, "dn-remote-directory", move |cancel| {
            connection.with(|backend| backend.list(&target, &cancel))
        })?;
        let remote = self.remote_mut().unwrap();
        remote.pending = Some(task);
        remote.target = path;
        remote.revision += 1;
        self.loading = true;
        Ok(())
    }
    pub(crate) fn remote_poll(&mut self) -> Option<String> {
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
        let no_entries = self.entries.is_empty();
        let remote = self.remote_mut().unwrap();
        remote.pending = None;
        match reply {
            Err(e) => {
                let label = remote.connection.label(&remote.target);
                let first = remote.members.is_empty() && no_entries;
                let hidden = remote.hidden;
                if first {
                    self.leave_remote(hidden);
                }
                Some(format!("{label}: {e}"))
            }
            Ok(members) => {
                let mut unique = std::collections::HashSet::new();
                for member in &members {
                    if member.path.parent() != remote.target
                        || !unique.insert(member.path.name().to_owned())
                    {
                        return Some(
                            "Remote listing contains duplicate or redirected names".into(),
                        );
                    }
                }
                let same_directory = remote.directory == remote.target;
                let selected_name = self.entries.get(self.cursor).map(|e| e.name.clone());
                let old_offset = self.offset;
                let remote = self.remote_mut().unwrap();
                remote.directory = remote.target.clone();
                remote.revision += 1;
                remote.members.clear();
                let mut entries = vec![Entry {
                    file_type: None,
                    name: "..".into(),
                    path: PathBuf::new(),
                    directory: true,
                    symlink: false,
                    size: None,
                    parent: true,
                    modified: None,
                    executable: false,
                }];
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
            offset: r.origin.offset,
        })
    }
}
