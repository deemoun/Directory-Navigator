//! Selected extraction uses private staging and the native guarded publication engine.
use super::{
    reader::{self, Kind},
    Extraction,
};
use crate::{
    operations::{self, CopyOptions, ExistingDestination, Plan, Progress},
    task::Cancellation,
};
use std::{
    collections::BTreeSet,
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
};
#[derive(Clone)]
pub struct Destination {
    path: PathBuf,
    ancestors: Vec<(PathBuf, crate::platform_fs::EntryIdentity)>,
    existed: bool,
}
impl Destination {
    pub fn capture(path: &Path) -> io::Result<Self> {
        if !path.is_absolute() {
            return Err(io::Error::other(
                "Choose an absolute filesystem destination",
            ));
        }
        let mut ancestors = Vec::new();
        for p in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
            match fs::symlink_metadata(p) {
                Ok(m) => {
                    if !m.is_dir() && p != path {
                        return Err(io::Error::other("Destination parent is not a directory"));
                    }
                    if m.is_symlink() || redirected(&m) {
                        return Err(io::Error::other("Destination links are not accepted"));
                    }
                    ancestors.push((p.to_path_buf(), crate::platform_fs::entry_identity(p)?));
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound && p == path => {}
                Err(e) => return Err(e),
            }
        }
        Ok(Self {
            path: path.to_path_buf(),
            ancestors,
            existed: fs::symlink_metadata(path).is_ok(),
        })
    }
    pub fn validate(&self) -> io::Result<()> {
        if !self.existed && fs::symlink_metadata(&self.path).is_ok() {
            return Err(io::Error::other("Queued destination appeared"));
        }
        for (p, id) in &self.ancestors {
            if crate::platform_fs::entry_identity(p)? != *id {
                return Err(io::Error::other("Extraction destination changed"));
            }
        }
        Ok(())
    }
}
struct Stage {
    directories: Vec<(PathBuf, crate::platform_fs::EntryIdentity)>,
    path: PathBuf,
    identity: crate::platform_fs::EntryIdentity,
}
impl Stage {
    fn new(parent: &Path) -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        for _ in 0..100 {
            let path = parent.join(format!(
                ".dn-extract-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let builder = &mut fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&path) {
                Ok(()) => {
                    let metadata = fs::symlink_metadata(&path)?;
                    if !metadata.is_dir() || redirected(&metadata) {
                        return Err(io::Error::other("Extraction staging was replaced"));
                    }
                    return Ok(Self {
                        directories: Vec::new(),
                        identity: crate::platform_fs::entry_identity(&path)?,
                        path,
                    });
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::other("Cannot create private extraction staging"))
    }
}
impl Drop for Stage {
    fn drop(&mut self) {
        if crate::platform_fs::entry_identity(&self.path).ok().as_ref() == Some(&self.identity) {
            for (path, id) in &self.directories {
                if crate::platform_fs::entry_identity(path).ok().as_ref() == Some(id) {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
                    }
                }
            }
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
pub(crate) fn component(raw: &[u8]) -> io::Result<OsString> {
    // Reject Windows aliases on every host, keeping archive output portable and
    // preventing name collisions on case-insensitive filesystems.
    let text = std::str::from_utf8(raw)
        .map_err(|_| io::Error::other("Member name cannot be represented portably"))?;
    let stem = text
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    if text.is_empty()
        || text.ends_with(['.', ' '])
        || text
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
        || matches!(
            stem.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$" | "CLOCK$"
        )
        || stem
            .strip_prefix("COM")
            .or_else(|| stem.strip_prefix("LPT"))
            .is_some_and(|suffix| {
                matches!(
                    suffix,
                    "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
    {
        return Err(io::Error::other("Invalid or reserved archive output name"));
    }
    Ok(text.into())
}
impl Extraction {
    pub fn needs_password(&self) -> bool {
        self.selected().is_ok_and(|nodes| {
            nodes.iter().any(|(id, _)| {
                self.catalog.nodes[*id].locked && self.catalog.nodes[*id].kind == Kind::File
            })
        })
    }

    pub(super) fn selected(&self) -> io::Result<Vec<(usize, PathBuf)>> {
        if self.catalog.note.is_some() || self.catalog.skipped > 0 {
            return Err(io::Error::other(
                "Cannot extract from an incomplete or unsafe archive catalog",
            ));
        }
        let parent = self
            .catalog
            .nodes
            .get(self.directory)
            .ok_or_else(|| io::Error::other("Missing archive directory"))?;
        let mut result = Vec::new();
        let mut keys = BTreeSet::new();
        let generation = self.catalog.generation;
        if self.members.is_empty() {
            return Err(io::Error::other("Select an archive member"));
        }
        for id in &self.members {
            if id.generation != generation {
                return Err(io::Error::other("Stale archive selection"));
            }
            let node = self
                .catalog
                .nodes
                .get(id.index)
                .filter(|n| id.index != 0 && n.parent == self.directory)
                .ok_or_else(|| io::Error::other("Invalid archive selection"))?;
            let prefix = if parent.path.is_empty() {
                0
            } else {
                parent.path.len() + 1
            };
            let mut stack = vec![id.index];
            while let Some(index) = stack.pop() {
                let n = &self.catalog.nodes[index];
                if !matches!(n.kind, Kind::File | Kind::Directory) {
                    return Err(io::Error::other(
                        "Selected tree contains links or special members",
                    ));
                }
                let mut path = PathBuf::new();
                for part in n.path[prefix..].split(|b| *b == b'/') {
                    path.push(component(part)?);
                }
                let key = path.to_string_lossy().to_lowercase();
                if !keys.insert(key) {
                    return Err(io::Error::other("Conflicting archive output names"));
                }
                result.push((index, path));
                stack.extend(n.children.iter().rev());
            }
            let _ = node;
        }
        Ok(result)
    }
    pub fn validate(&self) -> io::Result<()> {
        self.selected()?;
        self.catalog.validate()
    }
    #[cfg(test)]
    pub(crate) fn run(
        &self,
        destination: &Destination,
        options: CopyOptions,
        cancel: Arc<AtomicBool>,
        notify: impl FnMut(&Progress),
    ) -> io::Result<Progress> {
        if options.remove_sources {
            return Err(io::Error::other("Archive members are read-only"));
        }
        self.run_with_conflicts(
            destination,
            options,
            cancel,
            &mut operations::conflict::no_prompt,
            notify,
        )
    }
    pub(crate) fn run_with_conflicts(
        &self,
        destination: &Destination,
        options: CopyOptions,
        cancel: Arc<AtomicBool>,
        ask: &mut operations::conflict::Ask<'_>,
        mut notify: impl FnMut(&Progress),
    ) -> io::Result<Progress> {
        if options.remove_sources {
            return Err(io::Error::other("Archive members are read-only"));
        }
        self.validate()?;
        if self.password.is_none() && self.needs_password() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Archive password required",
            ));
        }
        destination.validate()?;
        let token = Cancellation::from_flag(cancel.clone());
        token.check()?;
        let mut selected = self.selected()?;
        let container = fs::symlink_metadata(&destination.path).is_ok_and(|m| m.is_dir());
        if !container && self.members.len() != 1 {
            return Err(io::Error::other(
                "Multiple members require an existing destination directory",
            ));
        }
        let mut skipped = 0;
        let mut excluded = BTreeSet::new();
        for id in &self.members {
            let relative = &selected
                .iter()
                .find(|(index, _)| *index == id.index)
                .unwrap()
                .1;
            let target = if container {
                destination.path.join(relative)
            } else {
                destination.path.clone()
            };
            if options.existing != ExistingDestination::Ask && fs::symlink_metadata(&target).is_ok()
            {
                if options.existing == ExistingDestination::Refuse {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        format!("Destination exists: {}", target.display()),
                    ));
                }
                excluded.insert(id.index);
                skipped += 1;
            }
        }
        selected.retain(|(id, _)| {
            let mut current = *id;
            loop {
                if excluded.contains(&current) {
                    return false;
                }
                if current == self.directory {
                    return true;
                }
                current = self.catalog.nodes[current].parent;
            }
        });
        if selected.is_empty() {
            return Ok(Progress {
                skipped,
                ..Default::default()
            });
        }
        let required = selected.iter().try_fold(0u64, |sum, (id, _)| {
            sum.checked_add(self.catalog.nodes[*id].size)
                .ok_or_else(|| io::Error::other("Extraction size overflow"))
        })?;
        let parent = if container {
            destination.path.as_path()
        } else {
            destination
                .path
                .parent()
                .ok_or_else(|| io::Error::other("Missing destination parent"))?
        };
        if options.check_free_space
            && u128::from(required) * 2 > crate::volume_space::read(parent)?.available
        {
            return Err(io::Error::other(
                "Insufficient space for extraction staging and publication",
            ));
        }
        destination.validate()?;
        let mut stage = Stage::new(parent)?;
        let mut files = Vec::new();
        let mut roots = Vec::new();
        for (id, relative) in &selected {
            token.check()?;
            let path = stage.path.join(relative);
            if self.catalog.nodes[*id].kind == Kind::Directory {
                fs::create_dir(&path)?;
                stage
                    .directories
                    .push((path.clone(), crate::platform_fs::entry_identity(&path)?));
            } else {
                files.push((*id, path.clone()));
            }
            if self.catalog.nodes[*id].parent == self.directory {
                roots.push(path);
            }
        }
        let mut progress = Progress {
            path: self.catalog.path.clone(),
            total_bytes: Some(required),
            ..Default::default()
        };
        reader::extract_to(
            &self.catalog,
            &files,
            self.password.as_ref(),
            &token,
            |id, bytes| {
                progress.member =
                    Some(String::from_utf8_lossy(&self.catalog.nodes[id].path).into_owned());
                progress.written += bytes;
                notify(&progress)
            },
        )?;
        for (id, relative) in selected.iter().rev() {
            let n = &self.catalog.nodes[*id];
            if n.kind == Kind::Directory {
                let path = stage.path.join(relative);
                let mut open = fs::OpenOptions::new();
                open.read(true);
                #[cfg(windows)]
                {
                    use std::os::windows::fs::OpenOptionsExt;
                    open.write(true).custom_flags(0x02000000);
                }
                let directory = open.open(&path)?;
                if let Some(time) = n.modified_time {
                    directory.set_times(fs::FileTimes::new().set_modified(time))?;
                }
                #[cfg(unix)]
                if let Some(mode) = n.mode {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(&path, fs::Permissions::from_mode(mode & 0o777))?;
                }
            }
        }
        destination.validate()?;
        token.check()?;
        let plan = Plan {
            kind: operations::Kind::Move,
            sources: roots,
            destination: destination.path.clone(),
        };
        let prepared = operations::transfer::Prepared::capture(&plan)?;
        destination.validate()?;
        let mut result = operations::publish_extraction(
            &plan,
            options,
            &cancel,
            |p| {
                let mut shown = p.clone();
                shown.path = destination.path.clone();
                shown.member = None;
                notify(&shown)
            },
            &prepared,
            &mut |c| {
                let mut display = c.clone();
                if let Ok(member) = c.source.strip_prefix(&stage.path) {
                    display.source_label = Some(format!(
                        "{}!{}",
                        self.catalog.path.display(),
                        member.display()
                    ));
                }
                ask(&display)
            },
        )?;
        result.skipped += skipped;
        result.path = destination.path.clone();
        result.member = None;
        Ok(result)
    }
}

fn redirected(m: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        m.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        let _ = m;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    struct Fixture(PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn fixture() -> io::Result<(Fixture, Extraction, Destination)> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "dn-extraction-cancel-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path)?;
        let f = Fixture(fs::canonicalize(path)?);
        fs::create_dir(f.0.join("out"))?;
        let archive = f.0.join("source.zip");
        let mut writer = zip::ZipWriter::new(fs::File::create(&archive)?);
        writer.start_file("large", zip::write::SimpleFileOptions::default())?;
        writer.write_all(&vec![b'x'; 256 * 1024])?;
        writer.finish()?;
        let catalog = Arc::new(reader::list(archive, &Cancellation::default())?);
        let selection = Extraction {
            members: vec![crate::panel::archive::MemberId {
                generation: catalog.generation,
                index: 1,
            }],
            catalog,
            directory: 0,
            password: None,
        };
        let destination = Destination::capture(&f.0.join("out"))?;
        Ok((f, selection, destination))
    }
    #[test]
    fn cancellation_during_decode_removes_staging_and_publishes_nothing() -> io::Result<()> {
        let (fixture, selection, destination) = fixture()?;
        let cancel = Arc::new(AtomicBool::new(false));
        let result = selection.run(&destination, CopyOptions::default(), cancel.clone(), |p| {
            if p.written > 0 {
                cancel.store(true, Ordering::Relaxed);
            }
        });
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Interrupted);
        assert!(fs::read_dir(fixture.0.join("out"))?.next().is_none());
        assert!(selection.catalog.path.is_file());
        Ok(())
    }
    #[test]
    fn destination_replacement_and_stale_member_generation_are_rejected() -> io::Result<()> {
        let (fixture, mut selection, destination) = fixture()?;
        fs::rename(fixture.0.join("out"), fixture.0.join("old"))?;
        fs::create_dir(fixture.0.join("out"))?;
        assert!(destination.validate().is_err());
        selection.members[0].generation += 1;
        assert!(selection.validate().is_err());
        assert!(fs::read_dir(fixture.0.join("out"))?.next().is_none());
        Ok(())
    }
    #[test]
    fn partial_publication_retains_completed_files_and_cleans_owned_staging() -> io::Result<()> {
        let (fixture, _, destination) = fixture()?;
        let path = fixture.0.join("pair.zip");
        let mut writer = zip::ZipWriter::new(fs::File::create(&path)?);
        for name in ["first", "second"] {
            writer.start_file(name, zip::write::SimpleFileOptions::default())?;
            writer.write_all(name.as_bytes())?;
        }
        writer.finish()?;
        let catalog = Arc::new(reader::list(path.clone(), &Cancellation::default())?);
        let selection = Extraction {
            members: catalog.nodes[0]
                .children
                .iter()
                .map(|id| crate::panel::archive::MemberId {
                    generation: catalog.generation,
                    index: *id,
                })
                .collect(),
            catalog,
            directory: 0,
            password: None,
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let result = selection.run(&destination, CopyOptions::default(), cancel.clone(), |p| {
            if p.files > 0 {
                cancel.store(true, Ordering::Relaxed);
            }
        });
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Interrupted);
        assert_eq!(fs::read(destination.path.join("first"))?, b"first");
        let names = fs::read_dir(&destination.path)?
            .map(|e| e.map(|e| e.file_name()))
            .collect::<io::Result<Vec<_>>>()?;
        assert_eq!(names, [OsString::from("first")]);
        assert!(path.is_file());
        Ok(())
    }
    #[test]
    fn portable_components_reject_console_and_legacy_serial_device_aliases() {
        for name in [
            "CON",
            "con.txt",
            "CONIN$",
            "CONOUT$",
            "COM1",
            "COM¹",
            "LPT³.txt",
            "a.",
            "trailing ",
            "a:b",
        ] {
            assert!(component(name.as_bytes()).is_err(), "{name}");
        }
        for name in ["文.txt", "COM10.txt", "normal.file"] {
            assert!(component(name.as_bytes()).is_ok(), "{name}");
        }
    }
}
