//! Archive locations share the ordinary pane's layout and navigation controls.
use super::*;
use crate::{
    archive::reader::{self, Catalog, Kind},
    task::Task,
};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MemberId {
    pub generation: u64,
    pub index: usize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntryIdentity {
    Native(PathBuf),
    Member(MemberId),
    Parent(u64, usize),
}
#[derive(Default)]
pub enum Location {
    #[default]
    Filesystem,
    Archive(Box<Archive>),
}
pub struct Archive {
    pub path: PathBuf,
    pub catalog: Option<Arc<Catalog>>,
    pub directory: usize,
    pub generation: u64,
    pub(crate) password: Option<crate::archive::Password>,
    pub(crate) restore_directory: Vec<u8>,
    pub(crate) index: Option<Task<io::Result<Catalog>>>,
    pub(crate) preview: Option<Task<io::Result<crate::viewer::Contents>>>,
    pub(crate) preview_title: PathBuf,
    pub(crate) preview_epoch: u64,
    pub(crate) preview_member: Option<MemberId>,
    pub(crate) preview_command: String,
    members: HashMap<OsString, MemberId>,
    pub(crate) marks: BTreeSet<MemberId>,
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
impl Archive {
    pub fn preview_pending(&self) -> bool {
        self.preview.is_some()
    }
}
impl Panel {
    pub fn archive(&self) -> Option<&Archive> {
        match &self.location {
            Location::Archive(a) => Some(a),
            _ => None,
        }
    }
    pub(crate) fn archive_mut(&mut self) -> Option<&mut Archive> {
        match &mut self.location {
            Location::Archive(a) => Some(a),
            _ => None,
        }
    }
    pub fn is_archive(&self) -> bool {
        self.archive().is_some()
    }
    pub fn location_key(&self) -> (u64, usize) {
        self.archive()
            .map_or((0, 0), |a| (a.generation, a.directory))
    }
    pub fn heading(&self) -> String {
        self.archive().map_or_else(
            || self.path.to_string_lossy().into_owned(),
            |a| {
                let inner = a
                    .catalog
                    .as_ref()
                    .map(|c| String::from_utf8_lossy(&c.nodes[a.directory].path).into_owned())
                    .unwrap_or_default();
                format!("{} :: /{}", a.path.display(), inner)
            },
        )
    }
    pub fn identity(&self, entry: &Entry) -> EntryIdentity {
        if let Some(a) = self.archive() {
            if let Some(id) = self.member(entry) {
                EntryIdentity::Member(id)
            } else {
                EntryIdentity::Parent(a.generation, a.directory)
            }
        } else {
            EntryIdentity::Native(entry.path.clone())
        }
    }
    pub fn has_marks(&self) -> bool {
        self.archive()
            .map_or(!self.selected.is_empty(), |a| !a.marks.is_empty())
    }
    pub fn marked(&self, entry: &Entry) -> bool {
        self.archive().map_or_else(
            || self.selected.contains(&entry.name),
            |a| self.member(entry).is_some_and(|id| a.marks.contains(&id)),
        )
    }
    pub fn clear_selection(&mut self) {
        self.selected.clear();
        if let Some(a) = self.archive_mut() {
            a.marks.clear();
        }
    }
    pub fn mark_name(&mut self, name: OsString, marked: bool) -> bool {
        if let Some(a) = self.archive_mut() {
            let Some(id) = a.members.get(&name) else {
                return false;
            };
            if marked {
                a.marks.insert(*id)
            } else {
                a.marks.remove(id)
            }
        } else if marked {
            self.selected.insert(name)
        } else {
            self.selected.remove(&name)
        }
    }
    pub fn member(&self, entry: &Entry) -> Option<MemberId> {
        self.archive()?
            .members
            .get(&entry.name)
            .copied()
            .filter(|_| !entry.parent)
    }
    pub fn extraction(&self) -> Option<crate::archive::Extraction> {
        let a = self.archive()?;
        let ids = self
            .entries
            .iter()
            .filter(|e| {
                !e.parent
                    && if !self.has_marks() {
                        self.entries
                            .get(self.cursor)
                            .is_some_and(|current| current.name == e.name)
                    } else {
                        self.marked(e)
                    }
            })
            .filter_map(|e| self.member(e))
            .collect::<Vec<_>>();
        (!ids.is_empty()).then(|| crate::archive::Extraction {
            catalog: a.catalog.as_ref().unwrap().clone(),
            directory: a.directory,
            members: ids,
            password: a.password.clone(),
        })
    }
    pub fn open_archive(&mut self, path: PathBuf) -> io::Result<()> {
        if self.is_archive() {
            return Err(io::Error::other(
                "Extract the member before opening an archive",
            ));
        }
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let generation = NEXT.fetch_add(1, Ordering::Relaxed);
        let source = path.clone();
        let index = self
            .scope
            .spawn("archive-index", move |cancel| reader::list(source, &cancel))?;
        self.scan_cancel.cancel();
        self.scope.start("directory-scan");
        self.scope.start("external-open");
        let origin = Origin {
            entries: std::mem::take(&mut self.entries),
            excluded: std::mem::take(&mut self.filtered_out),
            selected: std::mem::take(&mut self.selected),
            filter: std::mem::take(&mut self.filter),
            cursor: self.cursor,
            offset: self.offset,
        };
        self.location = Location::Archive(Box::new(Archive {
            path,
            catalog: None,
            directory: 0,
            generation,
            restore_directory: Vec::new(),
            password: None,
            index: Some(index),
            preview: None,
            preview_title: PathBuf::new(),
            preview_epoch: 0,
            preview_member: None,
            preview_command: String::new(),
            members: HashMap::new(),
            marks: BTreeSet::new(),
            origin,
        }));
        self.loading = true;
        self.refreshing = false;
        self.refresh_pending = None;
        self.cursor = 0;
        self.offset = 0;
        self.total_files = 0;
        self.total_bytes = 0;
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }
    pub(crate) fn refresh_archive(&mut self) {
        let a = self.archive().unwrap();
        let path = a.path.clone();
        let directory = a
            .catalog
            .as_ref()
            .map(|c| c.nodes[a.directory].path.clone())
            .unwrap_or_default();
        let task = self
            .scope
            .spawn("archive-index", move |cancel| reader::list(path, &cancel));
        if let Ok(task) = task {
            let a = self.archive_mut().unwrap();
            a.index = Some(task);
            a.preview = None;
            a.restore_directory = directory;
            a.password = None;
            self.loading = true;
            self.revision = self.revision.wrapping_add(1);
        }
    }
    pub fn leave_archive(&mut self, hidden: bool) {
        let Location::Archive(a) = std::mem::take(&mut self.location) else {
            return;
        };
        self.entries = a.origin.entries;
        self.filtered_out = a.origin.excluded;
        self.selected = a.origin.selected;
        self.filter = a.origin.filter;
        self.cursor = self
            .entries
            .iter()
            .position(|e| e.path == a.path)
            .unwrap_or(a.origin.cursor);
        self.offset = a.origin.offset;
        self.loading = false;
        self.refresh_pending = None;
        self.revision = self.revision.wrapping_add(1);
        self.refresh(hidden);
    }
    pub(crate) fn archive_entries(&mut self) {
        let Some(a) = self.archive_mut() else {
            return;
        };
        let Some(c) = &a.catalog else {
            return;
        };
        a.members.clear();
        a.marks.clear();
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
        for id in &c.nodes[a.directory].children {
            let n = &c.nodes[*id];
            let raw = n.path.rsplit(|b| *b == b'/').next().unwrap_or_default();
            #[cfg(unix)]
            let name = {
                use std::os::unix::ffi::OsStringExt;
                OsString::from_vec(raw.to_vec())
            };
            #[cfg(not(unix))]
            let name = OsString::from(
                std::str::from_utf8(raw)
                    .map(str::to_owned)
                    .unwrap_or_else(|_| format!("{} [#{}]", String::from_utf8_lossy(raw), id)),
            );
            // A nonrepresentable Windows name remains visible but is never a native path.
            if a.members.contains_key(&name) {
                continue;
            }
            a.members.insert(
                name.clone(),
                MemberId {
                    generation: a.generation,
                    index: *id,
                },
            );
            entries.push(Entry {
                file_type: None,
                name,
                path: PathBuf::new(),
                directory: n.kind == Kind::Directory,
                symlink: n.kind == Kind::Link,
                size: Some(n.size),
                parent: false,
                modified: n.modified_time,
                executable: false,
            });
        }
        sort_entries(&mut entries, self.sort);
        self.entries = entries;
        self.filtered_out.clear();
        self.selected.clear();
        self.cursor = 0;
        self.offset = 0;
        self.loading = false;
        self.apply_filter_current();
        self.revision = self.revision.wrapping_add(1);
    }
    pub(crate) fn archive_enter(&mut self, hidden: bool) {
        let Some(entry) = self.entries.get(self.cursor) else {
            return;
        };
        if entry.parent {
            self.archive_parent(hidden);
            return;
        }
        let Some(id) = self.member(entry) else {
            return;
        };
        if !entry.directory {
            return;
        }
        if let Some(a) = self.archive_mut() {
            a.directory = id.index;
            a.preview = None;
        }
        self.archive_entries();
    }
    pub(crate) fn archive_parent(&mut self, hidden: bool) {
        let Some(a) = self.archive_mut() else {
            return;
        };
        if a.directory == 0 {
            self.leave_archive(hidden);
            return;
        }
        let Some(c) = &a.catalog else {
            return;
        };
        let old = a.directory;
        a.directory = c.nodes[old].parent;
        a.preview = None;
        self.archive_entries();
        self.cursor = self
            .entries
            .iter()
            .position(|e| self.member(e).is_some_and(|id| id.index == old))
            .unwrap_or(0);
    }
}
