//! No-replace file operations. Existing destinations are always refused.
use crate::filesystem::open_regular;
use std::{
    fs::{self, Metadata},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Copy,
    Move,
    Delete,
    Trash,
    Mkdir,
}

#[derive(Clone, Debug)]
pub struct Plan {
    pub kind: Kind,
    pub sources: Vec<PathBuf>,
    pub destination: PathBuf,
}

#[derive(Clone, Debug, Default)]
pub struct Progress {
    pub files: u64,
    pub bytes: u64,
    pub path: PathBuf,
}

pub enum Event {
    Progress(Progress),
    Done(Result<Progress, OperationFailure>),
}

#[derive(Clone, Debug)]
pub struct OperationFailure {
    pub kind: io::ErrorKind,
    pub progress: Progress,
    pub message: String,
}
impl std::fmt::Display for OperationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f,"{}\nAt: {}\n\nCompleted items are retained. Partial destinations may remain.\nMove sources are removed only after copying succeeds.",self.message,self.progress.path.display())
    }
}
impl std::error::Error for OperationFailure {}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveStrategy {
    PreferRename,
    Copy,
}
pub struct Job {
    pub events: Receiver<Event>,
    pub cancel: Arc<AtomicBool>,
    pub progress: Progress,
    pub kind: Kind,
    pub focus: Option<PathBuf>,
}

impl Job {
    pub fn start(plan: Plan) -> io::Result<Self> {
        let (sender, events) = mpsc::sync_channel(8);
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let kind = plan.kind;
        let focus = if plan.kind == Kind::Mkdir {
            Some(plan.destination.clone())
        } else if matches!(plan.kind, Kind::Copy | Kind::Move) && plan.sources.len() == 1 {
            if plan.destination.is_dir() {
                plan.sources[0]
                    .file_name()
                    .map(|name| plan.destination.join(name))
            } else {
                Some(plan.destination.clone())
            }
        } else {
            None
        };
        thread::Builder::new()
            .name("dn-operation".into())
            .spawn(move || {
                let mut last = Instant::now() - Duration::from_secs(1);
                let result = execute(
                    &plan,
                    &worker_cancel,
                    |progress| {
                        if last.elapsed() >= Duration::from_millis(80) {
                            let _ = sender.try_send(Event::Progress(progress.clone()));
                            last = Instant::now();
                        }
                    },
                    MoveStrategy::PreferRename,
                );
                let _ = sender.send(Event::Done(result));
            })?;
        Ok(Self {
            events,
            cancel,
            progress: Progress::default(),
            kind,
            focus,
        })
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        self.cancel();
    }
}

fn check(cancel: &AtomicBool) -> io::Result<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Operation cancelled",
        ))
    } else {
        Ok(())
    }
}

fn absent(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "Destination exists (will not overwrite): {}",
                path.display()
            ),
        )),
    }
}

fn same_file(a: &Metadata, b: &Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if a.dev() != b.dev() || a.ino() != b.ino() {
            return false;
        }
    }
    a.file_type() == b.file_type() && a.len() == b.len() && a.modified().ok() == b.modified().ok()
}

/// Resolve the destination's parent before checking containment or creating it.
fn destination_path(path: &Path) -> io::Result<PathBuf> {
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("Destination needs a filename"))?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    Ok(fs::canonicalize(parent)?.join(name))
}

fn mappings(plan: &Plan) -> io::Result<Vec<(PathBuf, PathBuf)>> {
    let container = fs::metadata(&plan.destination).is_ok_and(|m| m.is_dir());
    if plan.sources.len() > 1 && !container {
        return Err(io::Error::other(
            "Multiple items require an existing destination directory",
        ));
    }
    let mut pairs = Vec::new();
    for source in &plan.sources {
        let metadata = fs::symlink_metadata(source)?;
        let target = if container {
            let name = source
                .file_name()
                .ok_or_else(|| io::Error::other("Cannot operate on a filesystem root"))?;
            fs::canonicalize(&plan.destination)?.join(name)
        } else {
            destination_path(&plan.destination)?
        };
        absent(&target)?;
        if metadata.is_dir() && target.starts_with(fs::canonicalize(source)?) {
            return Err(io::Error::other(
                "Cannot copy or move a directory into itself",
            ));
        }
        if pairs.iter().any(|(_, previous)| previous == &target) {
            return Err(io::Error::other(
                "Two sources have the same destination name",
            ));
        }
        pairs.push((source.clone(), target));
    }
    Ok(pairs)
}

#[cfg(unix)]
fn copy_link(source: &Path, destination: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(fs::read_link(source)?, destination)
}

#[cfg(windows)]
fn copy_link(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::fs::{symlink_dir, symlink_file, MetadataExt};
    let target = fs::read_link(source)?;
    if fs::symlink_metadata(source)?.file_attributes() & 0x10 != 0 {
        symlink_dir(target, destination)
    } else {
        symlink_file(target, destination)
    }
}

#[cfg(not(any(unix, windows)))]
fn copy_link(_: &Path, _: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Symlink copy is unsupported on this platform",
    ))
}

fn remove_leaf(path: &Path, metadata: &Metadata) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.is_symlink() && metadata.file_attributes() & 0x10 != 0 {
            return fs::remove_dir(path);
        }
    }
    let _ = metadata;
    fs::remove_file(path)
}

enum Work {
    Visit(PathBuf, PathBuf),
    Permissions(fs::File, fs::File, Metadata),
}

pub fn run(
    plan: &Plan,
    cancel: &AtomicBool,
    notify: impl FnMut(&Progress),
) -> io::Result<Progress> {
    run_with_strategy(plan, cancel, notify, MoveStrategy::PreferRename)
}
pub fn execute(
    plan: &Plan,
    cancel: &AtomicBool,
    mut notify: impl FnMut(&Progress),
    strategy: MoveStrategy,
) -> Result<Progress, OperationFailure> {
    let mut progress = Progress {
        path: plan.destination.clone(),
        ..Progress::default()
    };
    run_with_strategy(
        plan,
        cancel,
        |p| {
            progress = p.clone();
            notify(p);
        },
        strategy,
    )
    .map_err(|e| OperationFailure {
        kind: e.kind(),
        progress,
        message: e.to_string(),
    })
}
pub fn run_with_strategy(
    plan: &Plan,
    cancel: &AtomicBool,
    mut notify: impl FnMut(&Progress),
    strategy: MoveStrategy,
) -> io::Result<Progress> {
    let mut progress = Progress::default();
    check(cancel)?;
    if plan.kind == Kind::Mkdir {
        fs::create_dir(destination_path(&plan.destination)?)?;
        progress.files = 1;
        return Ok(progress);
    }
    if plan.sources.is_empty() {
        return Err(io::Error::other("No files selected"));
    }
    // The UI supplies sibling entries; reject roots and dot/parent paths here too.
    for source in &plan.sources {
        if source.file_name().is_none() {
            return Err(io::Error::other(
                "Cannot operate on a filesystem root or parent entry",
            ));
        }
        fs::symlink_metadata(source)?;
    }
    if plan.kind == Kind::Trash {
        crate::trash_support::move_items(
            &plan.sources,
            cancel,
            &mut progress,
            &mut notify,
            crate::trash_support::send,
        )?;
        return Ok(progress);
    }
    if plan.kind == Kind::Delete {
        for source in &plan.sources {
            let mut stack = vec![(source.clone(), false)];
            while let Some((path, leave)) = stack.pop() {
                check(cancel)?;
                progress.path = path.clone();
                notify(&progress);
                let metadata = fs::symlink_metadata(&path)?;
                if metadata.is_dir() {
                    if leave {
                        fs::remove_dir(&path)?;
                        progress.files += 1;
                    } else {
                        stack.push((path.clone(), true));
                        for child in fs::read_dir(&path)? {
                            check(cancel)?;
                            stack.push((child?.path(), false));
                        }
                    }
                } else {
                    remove_leaf(&path, &metadata)?;
                    progress.files += 1;
                }
            }
        }
        return Ok(progress);
    }
    let pairs = mappings(plan)?; // Check all top-level collisions before writing.
    let mut copied = Vec::new();
    let mut buffer = vec![0_u8; 128 * 1024];
    for (source, target) in pairs {
        if plan.kind == Kind::Move && strategy == MoveStrategy::PreferRename {
            check(cancel)?;
            let original = fs::symlink_metadata(&source)?;
            progress.path = source.clone();
            notify(&progress);
            check(cancel)?;
            if !same_file(&original, &fs::symlink_metadata(&source)?) {
                return Err(io::Error::other("Source changed before move"));
            }
            match crate::platform_fs::rename_no_replace(&source, &target) {
                Ok(()) => {
                    progress.files += 1;
                    if original.is_file() {
                        progress.bytes = progress.bytes.saturating_add(original.len());
                    }
                    notify(&progress);
                    crate::platform_fs::sync_parent(&target)?;
                    crate::platform_fs::sync_parent(&source)?;
                    continue;
                }
                Err(e) if crate::platform_fs::can_copy_fallback(&e) => {}
                Err(e) => return Err(e),
            }
        }
        let mut stack = vec![Work::Visit(source, target)];
        while let Some(work) = stack.pop() {
            check(cancel)?;
            let (source, target) = match work {
                Work::Permissions(input, output, metadata) => {
                    crate::platform_fs::copy_extended_metadata(&input, &output)?;
                    #[cfg(unix)]
                    {
                        output.set_permissions(metadata.permissions())?;
                        if let Ok(modified) = metadata.modified() {
                            output.set_times(fs::FileTimes::new().set_modified(modified))?;
                        }
                        output.sync_all()?;
                    }
                    #[cfg(not(unix))]
                    {
                        let _ = metadata;
                    }
                    continue;
                }
                Work::Visit(source, target) => (source, target),
            };
            progress.path = source.clone();
            notify(&progress);
            let metadata = fs::symlink_metadata(&source)?;
            if metadata.is_symlink() {
                copy_link(&source, &target)?;
            } else if metadata.is_dir() {
                fs::create_dir(&target)?;
                let source_directory = crate::platform_fs::open_directory(&source)?;
                if !crate::platform_fs::same_identity(&metadata, &source_directory.metadata()?) {
                    return Err(io::Error::other("Source directory changed before copy"));
                }
                let target_directory = crate::platform_fs::open_directory(&target)?;
                stack.push(Work::Permissions(
                    source_directory,
                    target_directory,
                    metadata.clone(),
                ));
                for child in fs::read_dir(&source)? {
                    check(cancel)?;
                    let child = child?;
                    stack.push(Work::Visit(child.path(), target.join(child.file_name())));
                }
            } else if metadata.is_file() {
                let mut input = open_regular(&source)?;
                if !same_file(&metadata, &input.metadata()?) {
                    return Err(io::Error::other("Source changed before copy"));
                }
                absent(&target)?;
                let mut staged = crate::publication::StagedFile::create(&target)?;
                let output = &mut staged.file;
                loop {
                    check(cancel)?;
                    let count = input.read(&mut buffer)?;
                    if count == 0 {
                        break;
                    }
                    output.write_all(&buffer[..count])?;
                    progress.bytes = progress.bytes.saturating_add(count as u64);
                    notify(&progress);
                }
                output.sync_all()?;
                if !same_file(&metadata, &input.metadata()?)
                    || !same_file(&metadata, &fs::symlink_metadata(&source)?)
                {
                    return Err(io::Error::other(
                        "Source changed during copy; original retained",
                    ));
                }
                crate::platform_fs::copy_extended_metadata(&input, output)?;
                output.set_permissions(metadata.permissions())?;
                if let Ok(modified) = metadata.modified() {
                    output.set_times(fs::FileTimes::new().set_modified(modified))?;
                }
                output.sync_all()?;
                staged.publish(&target)?;
            } else {
                return Err(io::Error::other(format!(
                    "Unsupported special file: {}",
                    source.display()
                )));
            }
            progress.files += 1;
            if plan.kind == Kind::Move {
                let destination_metadata = fs::symlink_metadata(&target)?;
                copied.push((source, target, metadata, destination_metadata));
            }
        }
    }
    if plan.kind == Kind::Move {
        // Copy everything successfully before removing any source. Never recursively
        // delete a moved directory: new children must survive, causing remove_dir to fail.
        for (source, target, original, destination_metadata) in copied.into_iter().rev() {
            check(cancel)?;
            progress.path = source.clone();
            notify(&progress);
            let current = fs::symlink_metadata(&source)?;
            let destination_current = fs::symlink_metadata(&target)?;
            if (original.is_dir()
                && !crate::platform_fs::same_identity(&destination_metadata, &destination_current))
                || (!original.is_dir() && !same_file(&destination_metadata, &destination_current))
            {
                return Err(io::Error::other(
                    "Destination changed after copy; original retained",
                ));
            }
            if original.is_dir() {
                if !current.is_dir() || !crate::platform_fs::same_identity(&original, &current) {
                    return Err(io::Error::other("Source directory changed; retained"));
                }
                fs::remove_dir(&source)?;
            } else {
                if !same_file(&original, &current) {
                    return Err(io::Error::other("Source changed after copy; retained"));
                }
                remove_leaf(&source, &current)?;
            }
        }
    }
    Ok(progress)
}
