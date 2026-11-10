//! Typed remote transfers. Local publication delegates to the guarded native engine.
use super::{
    remote::{Backend, Connection, Member, RemotePath},
    workspace::Workspace,
};
use crate::{
    operations::{
        self,
        conflict::{Conflict, Decision},
        CopyOptions, ExistingDestination, Kind, Plan, Progress,
    },
    task::Cancellation,
};
use std::{
    collections::HashSet,
    io,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
};
const FILES: usize = 100_000;
const BYTES: u64 = 64 * 1024 * 1024 * 1024;
#[derive(Clone)]
pub struct NativeSource {
    pub path: PathBuf,
    identity: crate::platform_fs::EntryIdentity,
    metadata: std::fs::Metadata,
}
#[derive(Clone)]
pub enum Transfer {
    Mutation(super::controller::Mutation),
    Download {
        connection: Arc<Connection>,
        members: Vec<Member>,
        destination: PathBuf,
    },
    Upload {
        connection: Arc<Connection>,
        sources: Vec<NativeSource>,
        directory: RemotePath,
    },
}
impl Transfer {
    pub fn upload(
        connection: Arc<Connection>,
        paths: Vec<PathBuf>,
        directory: RemotePath,
    ) -> io::Result<Self> {
        let mut sources = Vec::new();
        for path in paths {
            let metadata = std::fs::symlink_metadata(&path)?;
            if crate::platform_fs::directory_link(&metadata)
                || (!metadata.is_file() && !metadata.is_dir())
            {
                return Err(io::Error::other(
                    "Upload real files and directories; links and special files are not supported",
                ));
            }
            sources.push(NativeSource {
                identity: crate::platform_fs::entry_identity(&path)?,
                metadata,
                path,
            });
        }
        Ok(Self::Upload {
            connection,
            sources,
            directory,
        })
    }
    pub fn uploading(&self) -> bool {
        matches!(self, Self::Upload { .. })
    }
    pub fn summary(&self) -> String {
        match self {
            Self::Mutation(m) => m.summary(),
            Self::Download {
                connection,
                members,
                ..
            } => members.first().map_or_else(
                || "Remote download".into(),
                |m| {
                    format!(
                        "{} (+{})",
                        connection.label(&m.path),
                        members.len().saturating_sub(1)
                    )
                },
            ),
            Self::Upload {
                connection,
                directory,
                sources,
            } => format!("{} → {}", sources.len(), connection.label(directory)),
        }
    }
    pub(crate) fn run(
        &self,
        options: CopyOptions,
        cancel: &AtomicBool,
        mut notify: impl FnMut(&Progress),
        ask: &mut impl FnMut(&Conflict) -> io::Result<Decision>,
    ) -> io::Result<Progress> {
        check(cancel)?;
        if options.remove_sources {
            return Err(io::Error::other("Remote Copy does not remove sources"));
        }
        let token = Cancellation::default(); // Every loop also observes the workspace cancellation flag.
        let mut progress = Progress::default();
        match self {
            Self::Mutation(m) => m.run(cancel, notify),
            Self::Download {
                connection,
                members,
                destination,
            } => {
                let receipt = crate::archive::extract::Destination::capture(destination)?;
                let parent = if destination.is_dir() {
                    destination.as_path()
                } else {
                    destination
                        .parent()
                        .ok_or_else(|| io::Error::other("Missing destination parent"))?
                };
                let stage = Workspace::create(parent.to_owned())?;
                let mut selected = Vec::new();
                let mut seen = HashSet::new();
                let mut stack = members
                    .iter()
                    .rev()
                    .cloned()
                    .map(|m| {
                        let name = crate::archive::extract::component(m.path.name().as_bytes())?;
                        Ok((m, PathBuf::from(name), 0))
                    })
                    .collect::<io::Result<Vec<_>>>()?;
                connection.with(|backend| {
                    while let Some((member, relative, depth)) = stack.pop() {
                        check(cancel)?;
                        if depth >= 512 || seen.len() >= FILES {
                            return Err(io::Error::other(
                                "Remote selection exceeds traversal limits",
                            ));
                        }
                        if member.link || (!member.regular && !member.directory) {
                            return Err(io::Error::other(
                                "Remote links and special files cannot be transferred",
                            ));
                        }
                        if !seen.insert(relative.to_string_lossy().to_lowercase()) {
                            return Err(io::Error::other("Remote output names collide"));
                        }
                        if member.directory {
                            for child in backend.list(&member.path, &token)? {
                                let name = crate::archive::extract::component(
                                    child.path.name().as_bytes(),
                                )?;
                                if stack.len() + selected.len() >= FILES {
                                    return Err(io::Error::other(
                                        "Remote selection exceeds traversal limits",
                                    ));
                                }
                                stack.push((child, relative.join(name), depth + 1));
                            }
                        }
                        selected.push((member, relative));
                    }
                    selected.sort_by_key(|(_, relative)| relative.components().count());
                    for (member, relative) in &selected {
                        check(cancel)?;
                        receipt.validate()?;
                        stage.validate()?;
                        let target = stage.root.join(relative);
                        if member.directory {
                            std::fs::create_dir_all(&target)?;
                        } else {
                            if let Some(parent) = target.parent() {
                                std::fs::create_dir_all(parent)?;
                            }
                            let current = backend
                                .stat(&member.path, &token)?
                                .ok_or_else(|| io::Error::other("Remote source disappeared"))?;
                            if &current != member {
                                return Err(io::Error::other("Remote source changed while queued"));
                            }
                            let mut output = std::fs::OpenOptions::new()
                                .write(true)
                                .create_new(true)
                                .open(&target)?;
                            let mut writer = Counting {
                                inner: &mut output,
                                cancel,
                                written: 0,
                                notify: &mut notify,
                                progress: &mut progress,
                                label: connection.label(&member.path),
                            };
                            let n = backend.read(&member.path, &mut writer, &token, BYTES)?;
                            drop(writer);
                            output.sync_all()?;
                            if member.size.is_some_and(|size| size != n)
                                || backend.stat(&member.path, &token)?.as_ref() != Some(member)
                            {
                                return Err(io::Error::other(
                                    "Remote source changed during download",
                                ));
                            }
                        }
                    }
                    Ok(())
                })?;
                check(cancel)?;
                receipt.validate()?;
                let mut roots = members
                    .iter()
                    .map(|m| {
                        crate::archive::extract::component(m.path.name().as_bytes())
                            .map(|name| stage.root.join(name))
                    })
                    .collect::<io::Result<Vec<_>>>()?;
                roots.sort();
                roots.dedup();
                operations::run_with_conflicts(
                    &Plan {
                        kind: Kind::Copy,
                        sources: roots,
                        destination: destination.clone(),
                    },
                    options,
                    cancel,
                    notify,
                    ask,
                )
            }
            Self::Upload {
                connection,
                sources,
                directory,
            } => connection.with(|backend| {
                let mut all = None;
                let mut count = 0;
                for source in sources {
                    check(cancel)?;
                    if crate::platform_fs::entry_identity(&source.path)? != source.identity {
                        return Err(io::Error::other("Upload source changed while queued"));
                    }
                    let now = std::fs::symlink_metadata(&source.path)?;
                    if now.len() != source.metadata.len()
                        || now.modified().ok() != source.metadata.modified().ok()
                    {
                        return Err(io::Error::other("Upload source changed while queued"));
                    }
                    let root_name = source
                        .path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .ok_or_else(|| {
                            io::Error::other("Remote upload requires representable UTF-8 names")
                        })?;
                    let mut stack = vec![(source.path.clone(), directory.child(root_name)?, 0)];
                    while let Some((path, target, depth)) = stack.pop() {
                        check(cancel)?;
                        count += 1;
                        if count > FILES || depth >= 512 {
                            return Err(io::Error::other("Upload exceeds traversal limits"));
                        }
                        let metadata = std::fs::symlink_metadata(&path)?;
                        if crate::platform_fs::directory_link(&metadata)
                            || (!metadata.is_file() && !metadata.is_dir())
                        {
                            return Err(io::Error::other("Upload contains a link or special file"));
                        }
                        let existing = backend.stat(&target, &token)?;
                        if metadata.is_dir() {
                            match existing {
                                Some(ref m) if m.directory && !m.link => {
                                    if options.existing == ExistingDestination::Skip {
                                        progress.skipped += 1;
                                        continue;
                                    }
                                    if options.existing == ExistingDestination::Refuse {
                                        return Err(io::Error::new(
                                            io::ErrorKind::AlreadyExists,
                                            "Remote destination directory already exists",
                                        ));
                                    }
                                }
                                Some(_) => {
                                    return Err(io::Error::other(
                                        "Remote destination has an incompatible type",
                                    ))
                                }
                                None => backend.mkdir(&target)?,
                            }
                            for entry in std::fs::read_dir(&path)? {
                                let entry = entry?;
                                let name = entry.file_name().into_string().map_err(|_| {
                                    io::Error::other(
                                        "Remote upload requires representable UTF-8 names",
                                    )
                                })?;
                                if stack.len() + count >= FILES {
                                    return Err(io::Error::other(
                                        "Upload exceeds traversal limits",
                                    ));
                                }
                                stack.push((entry.path(), target.child(&name)?, depth + 1));
                            }
                            continue;
                        }
                        let mut replace = false;
                        if let Some(remote) = &existing {
                            if !remote.regular || remote.link {
                                return Err(io::Error::other(
                                    "Remote destination is not a regular file",
                                ));
                            }
                            let decision = match options.existing {
                                ExistingDestination::Skip => Decision::Skip,
                                ExistingDestination::Refuse => {
                                    return Err(io::Error::new(
                                        io::ErrorKind::AlreadyExists,
                                        "Remote destination exists",
                                    ))
                                }
                                ExistingDestination::Ask => {
                                    if let Some(d) = all {
                                        d
                                    } else {
                                        ask(&Conflict {
                                            source: path.clone(),
                                            source_label: None,
                                            destination: PathBuf::new(),
                                            destination_label: Some(connection.label(&target)),
                                            source_size: metadata.len(),
                                            destination_size: remote.size.unwrap_or(0),
                                            source_modified: metadata.modified().ok(),
                                            destination_modified: remote.modified,
                                        })?
                                    }
                                }
                            };
                            match decision {
                                Decision::Cancel => {
                                    return Err(io::Error::new(
                                        io::ErrorKind::Interrupted,
                                        "Remote transfer cancelled",
                                    ))
                                }
                                Decision::Skip | Decision::SkipAll => {
                                    if decision == Decision::SkipAll {
                                        all = Some(Decision::Skip);
                                    }
                                    progress.skipped += 1;
                                    continue;
                                }
                                Decision::ReplaceAll => {
                                    all = Some(Decision::Replace);
                                    replace = true;
                                }
                                Decision::Replace => replace = true,
                            }
                        }
                        let identity = crate::platform_fs::entry_identity(&path)?;
                        let mut input = crate::filesystem::open_regular(&path)?;
                        let stage = stage_directory(backend, &target.parent())?;
                        let staged = stage.child("content")?;
                        let result = (|| {
                            let mut reader = CheckedReader {
                                inner: &mut input,
                                cancel,
                                bytes: 0,
                                progress: &mut progress,
                                notify: &mut notify,
                                label: connection.label(&target),
                            };
                            let n = backend.write_new(&staged, &mut reader, &token)?;
                            if n != metadata.len()
                                || crate::platform_fs::entry_identity(&path)? != identity
                                || input.metadata()?.modified().ok() != metadata.modified().ok()
                            {
                                return Err(io::Error::other(
                                    "Upload source changed during reading",
                                ));
                            }
                            check(cancel)?;
                            if backend.stat(&target, &token)? != existing {
                                return Err(io::Error::other(
                                    "Remote destination changed after confirmation",
                                ));
                            }
                            backend.publish(&staged, &target, replace)?;
                            progress.files += 1;
                            progress.bytes += n;
                            progress.member = Some(connection.label(&target));
                            notify(&progress);
                            Ok(())
                        })();
                        let _ = backend.remove(&staged, false);
                        let _ = backend.remove(&stage, true);
                        result?;
                    }
                }
                Ok(progress)
            }),
        }
    }
}
pub fn stage_directory(backend: &mut dyn Backend, parent: &RemotePath) -> io::Result<RemotePath> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = parent.child(&format!(
        ".dn-upload-{}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ))?;
    backend.mkdir(&path)?;
    Ok(path)
}
fn check(cancel: &AtomicBool) -> io::Result<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Remote transfer cancelled",
        ))
    } else {
        Ok(())
    }
}
struct CheckedReader<'a, F> {
    inner: &'a mut dyn io::Read,
    cancel: &'a AtomicBool,
    bytes: u64,
    progress: &'a mut Progress,
    notify: &'a mut F,
    label: String,
}
impl<F: FnMut(&Progress)> io::Read for CheckedReader<'_, F> {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        check(self.cancel)?;
        let n = self.inner.read(b)?;
        self.bytes += n as u64;
        if self.bytes > BYTES {
            return Err(io::Error::other("Remote upload exceeds 64 GiB"));
        }
        self.progress.written += n as u64;
        self.progress.member = Some(self.label.clone());
        (self.notify)(self.progress);
        Ok(n)
    }
}
struct Counting<'a, F> {
    inner: &'a mut dyn io::Write,
    cancel: &'a AtomicBool,
    written: u64,
    notify: &'a mut F,
    progress: &'a mut Progress,
    label: String,
}
impl<F: FnMut(&Progress)> io::Write for Counting<'_, F> {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        check(self.cancel)?;
        let n = self.inner.write(b)?;
        self.written += n as u64;
        self.progress.written += n as u64;
        self.progress.member = Some(self.label.clone());
        (self.notify)(self.progress);
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
