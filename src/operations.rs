//! No-replace file operations. Existing destinations are refused or explicitly skipped.
mod copy_options;
mod guards;
pub(crate) mod transfer;
use crate::filesystem::open_regular;
pub use copy_options::{CopyOptions, ExistingDestination};
use guards::DirectoryGuards;
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
    pub transfer: Option<crate::transfers::TransferId>,
    pub phase: Phase,
    pub written: u64,
    pub total_bytes: Option<u64>,
    pub details: Vec<Detail>,
    pub omitted_details: u64,
    pub files: u64,
    pub skipped: u64,
    pub bytes: u64,
    pub path: PathBuf,
}

pub enum Event {
    Progress(Progress),
    Done(Result<Progress, OperationFailure>),
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    Preparing,
    Calculating,
    #[default]
    Copying,
    Verifying,
    Renaming,
    RemovingSources,
}
impl Phase {
    pub fn caption(self) -> &'static str {
        match self {
            Self::Preparing => "Preparing",
            Self::Calculating => "Calculating totals",
            Self::Copying => "Copying",
            Self::Verifying => "Verifying",
            Self::Renaming => "Renaming",
            Self::RemovingSources => "Removing sources",
        }
    }
}
#[derive(Clone, Debug)]
pub struct Detail {
    pub path: PathBuf,
    pub message: String,
}
impl Progress {
    pub(crate) fn detail(&mut self, path: PathBuf, message: String) {
        if self.details.len() < 100 {
            self.details.push(Detail { path, message });
        } else {
            self.omitted_details = self.omitted_details.saturating_add(1);
        }
    }
}

#[derive(Clone, Debug)]
pub struct OperationFailure {
    pub kind: io::ErrorKind,
    pub progress: Box<Progress>,
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
    pub transfer: Option<crate::transfers::TransferId>,
    pub(crate) requests: crate::requests::RequestScope,
    token: crate::requests::RequestToken,
    pub events: Receiver<Event>,
    pub cancel: Arc<AtomicBool>,
    pub progress: Progress,
    pub kind: Kind,
    pub focus: Option<PathBuf>,
}

impl Job {
    pub fn try_recv(&self) -> Result<Event, std::sync::mpsc::TryRecvError> {
        if !self.requests.accepts(self.token) {
            return Err(std::sync::mpsc::TryRecvError::Disconnected);
        }
        self.events.try_recv()
    }

    pub fn start(plan: Plan) -> io::Result<Self> {
        Self::start_with_options(plan, CopyOptions::default())
    }
    pub fn start_with_options(plan: Plan, options: CopyOptions) -> io::Result<Self> {
        let requests = crate::requests::RequestScope::new(crate::requests::Owner::Workspace);
        let token = requests.start("file-operation");
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
                let result = execute_with_options(
                    &plan,
                    options,
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
            transfer: None,
            requests,
            token,
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
    a.file_type() == b.file_type()
        && a.len() == b.len()
        && a.permissions() == b.permissions()
        && a.modified().ok() == b.modified().ok()
}

fn unchanged_file(a: &Metadata, b: &Metadata) -> bool {
    // ctime also detects same-length rewrites whose mtime was restored.
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if a.ctime() != b.ctime() || a.ctime_nsec() != b.ctime_nsec() {
            return false;
        }
    }
    same_file(a, b)
}

#[derive(Clone)]
struct Source {
    path: PathBuf,
    metadata: Metadata,
}

fn sources(plan: &Plan, guards: &mut DirectoryGuards) -> io::Result<Vec<Source>> {
    let mut sources = Vec::new();
    let mut selected = std::collections::HashMap::new();
    for path in &plan.sources {
        // Pin ancestor symlinks once; the final component is never followed.
        let path = destination_path(path)?;
        guards.capture_parent(&path)?;
        let metadata = fs::symlink_metadata(&path)?;
        if selected.insert(path.clone(), metadata.is_dir()).is_some() {
            return Err(io::Error::other(
                "The same source was selected more than once",
            ));
        }
        sources.push(Source { path, metadata });
    }
    for source in &sources {
        if source
            .path
            .ancestors()
            .skip(1)
            .any(|p| selected.get(p) == Some(&true))
        {
            return Err(io::Error::other(
                "Cannot operate on a directory and its contents in the same batch",
            ));
        }
    }
    Ok(sources)
}

fn verify_entry(path: &Path, original: &Metadata) -> io::Result<Metadata> {
    let current = fs::symlink_metadata(path)?;
    let unchanged = if original.is_dir() {
        crate::platform_fs::same_identity(original, &current)
    } else {
        unchanged_file(original, &current)
    };
    if !unchanged {
        return Err(io::Error::other("Entry changed during operation; retained"));
    }
    Ok(current)
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

fn mappings(
    plan: &Plan,
    sources: Vec<Source>,
    policy: ExistingDestination,
    progress: &mut Progress,
) -> io::Result<Vec<(Source, PathBuf)>> {
    let container = fs::metadata(&plan.destination).is_ok_and(|m| m.is_dir());
    let container_path = if container {
        Some(fs::canonicalize(&plan.destination)?)
    } else {
        None
    };
    if plan.sources.len() > 1 && !container {
        return Err(io::Error::other(
            "Multiple items require an existing destination directory",
        ));
    }
    let mut pairs = Vec::new();
    let mut targets = std::collections::HashSet::new();
    for source in sources {
        let metadata = &source.metadata;
        let target = if let Some(container_path) = &container_path {
            let name = source
                .path
                .file_name()
                .ok_or_else(|| io::Error::other("Cannot operate on a filesystem root"))?;
            container_path.join(name)
        } else {
            destination_path(&plan.destination)?
        };
        if metadata.is_dir() && target.starts_with(&source.path) {
            return Err(io::Error::other(
                "Cannot copy or move a directory into itself",
            ));
        }
        if !targets.insert(target.clone()) {
            return Err(io::Error::other(
                "Two sources have the same destination name",
            ));
        }
        match absent(&target) {
            Err(e)
                if e.kind() == io::ErrorKind::AlreadyExists
                    && policy == ExistingDestination::Skip =>
            {
                progress.skipped = progress.skipped.saturating_add(1);
                progress.detail(source.path.clone(), "Skipped existing destination".into());
                continue;
            }
            result => result?,
        }
        pairs.push((source, target));
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
    Visit(Source, PathBuf),
    Permissions(PathBuf, PathBuf, fs::File, fs::File, Metadata),
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
    notify: impl FnMut(&Progress),
    strategy: MoveStrategy,
) -> Result<Progress, OperationFailure> {
    execute_with_options(plan, CopyOptions::default(), cancel, notify, strategy)
}
pub fn execute_with_options(
    plan: &Plan,
    options: CopyOptions,
    cancel: &AtomicBool,
    mut notify: impl FnMut(&Progress),
    strategy: MoveStrategy,
) -> Result<Progress, OperationFailure> {
    let mut progress = Progress {
        path: plan.destination.clone(),
        ..Progress::default()
    };
    run_with_options(
        plan,
        options,
        cancel,
        |p| {
            progress = p.clone();
            notify(p);
        },
        strategy,
    )
    .map_err(|e| OperationFailure {
        kind: e.kind(),
        progress: Box::new(progress),
        message: e.to_string(),
    })
}
pub fn run_with_strategy(
    plan: &Plan,
    cancel: &AtomicBool,
    notify: impl FnMut(&Progress),
    strategy: MoveStrategy,
) -> io::Result<Progress> {
    run_with_options(plan, CopyOptions::default(), cancel, notify, strategy)
}
pub fn run_with_options(
    plan: &Plan,
    options: CopyOptions,
    cancel: &AtomicBool,
    notify: impl FnMut(&Progress),
    strategy: MoveStrategy,
) -> io::Result<Progress> {
    run_checked(plan, options, cancel, notify, strategy, |path| {
        crate::volume_space::read(path).map(|s| s.available)
    })
}
fn run_checked(
    plan: &Plan,
    options: CopyOptions,
    cancel: &AtomicBool,
    notify: impl FnMut(&Progress),
    strategy: MoveStrategy,
    space: impl Fn(&Path) -> io::Result<u128>,
) -> io::Result<Progress> {
    run_checked_expected(plan, options, cancel, notify, strategy, space, None)
}
fn run_checked_expected(
    plan: &Plan,
    options: CopyOptions,
    cancel: &AtomicBool,
    mut notify: impl FnMut(&Progress),
    strategy: MoveStrategy,
    space: impl Fn(&Path) -> io::Result<u128>,
    expected: Option<&transfer::Prepared>,
) -> io::Result<Progress> {
    let mut effective = plan.clone();
    if let Some(expected) = expected {
        expected.validate(plan)?;
        effective.destination = expected.destination.clone();
    }
    if options.remove_sources && effective.kind == Kind::Copy {
        effective.kind = Kind::Move;
    }
    let plan = &effective;
    let strategy = if options.verify_writes || options.remove_sources {
        MoveStrategy::Copy
    } else {
        strategy
    };
    let mut progress = Progress::default();
    check(cancel)?;
    if plan.kind == Kind::Mkdir {
        let destination = destination_path(&plan.destination)?;
        fs::create_dir(&destination)?;
        crate::platform_fs::sync_parent(&destination)?;
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
    let mut guards = expected.map_or_else(DirectoryGuards::default, |p| p.guards.clone());
    let sources = if let Some(p) = expected {
        p.sources.clone()
    } else {
        sources(plan, &mut guards)?
    };
    let display_roots: std::collections::HashMap<_, _> = sources
        .iter()
        .zip(&plan.sources)
        .map(|(source, display)| (source.path.clone(), display))
        .collect();
    let display_path = |path: &Path| {
        path.ancestors()
            .find_map(|root| {
                display_roots.get(root).and_then(|display| {
                    path.strip_prefix(root).ok().map(|suffix| {
                        if suffix.as_os_str().is_empty() {
                            (**display).clone()
                        } else {
                            display.join(suffix)
                        }
                    })
                })
            })
            .unwrap_or_else(|| path.to_owned())
    };
    if plan.kind == Kind::Delete {
        for source in sources {
            let root = source.path.clone();
            let mut stack = vec![(source, false)];
            while let Some((source, leave)) = stack.pop() {
                check(cancel)?;
                let path = source.path;
                progress.path = display_path(&path);
                notify(&progress);
                check(cancel)?;
                guards.verify_parent(&path)?;
                let metadata = verify_entry(&path, &source.metadata)?;
                if metadata.is_dir() {
                    if leave {
                        fs::remove_dir(&path)?;
                        progress.files += 1;
                    } else {
                        guards.capture(&path)?;
                        stack.push((
                            Source {
                                path: path.clone(),
                                metadata,
                            },
                            true,
                        ));
                        for child in fs::read_dir(&path)? {
                            check(cancel)?;
                            guards.verify_directory(&path)?;
                            let path = child?.path();
                            let metadata = fs::symlink_metadata(&path)?;
                            stack.push((Source { path, metadata }, false));
                        }
                    }
                } else {
                    remove_leaf(&path, &metadata)?;
                    progress.files += 1;
                }
            }
            crate::platform_fs::sync_parent(&root)?;
        }
        return Ok(progress);
    }
    let pairs = mappings(plan, sources, options.existing, &mut progress)?; // Check all top-level collisions before writing.
    for (_, target) in &pairs {
        guards.capture_parent(target)?;
    }
    let prefer_rename = plan.kind == Kind::Move && strategy == MoveStrategy::PreferRename;
    if options.check_free_space && !prefer_rename && !pairs.is_empty() {
        copy_options::preflight(&pairs, &mut guards, cancel, &space)?;
    }
    let mut copied = Vec::new();
    let mut buffer = vec![0_u8; 128 * 1024];
    for (source, target) in pairs {
        if prefer_rename {
            progress.phase = Phase::Renaming;
            check(cancel)?;
            let original = &source.metadata;
            progress.path = display_path(&source.path);
            notify(&progress);
            check(cancel)?;
            guards.verify_parent(&source.path)?;
            guards.verify_parent(&target)?;
            // Rename transfers the current inode itself. Renaming a previous
            // hard link can legitimately change this inode's ctime.
            if !same_file(original, &fs::symlink_metadata(&source.path)?) {
                return Err(io::Error::other("Source changed before move; retained"));
            }
            match crate::platform_fs::rename_no_replace(&source.path, &target) {
                Ok(()) => {
                    progress.files += 1;
                    if original.is_file() {
                        progress.bytes = progress.bytes.saturating_add(original.len());
                    }
                    notify(&progress);
                    crate::platform_fs::sync_parent(&target)?;
                    crate::platform_fs::sync_parent(&source.path)?;
                    continue;
                }
                Err(e) if crate::platform_fs::can_copy_fallback(&e) => {}
                Err(e) => return Err(e),
            }
            // A successful native rename needs no copy capacity. Check only
            // after rename actually requests fallback, before copying this tree.
            if options.check_free_space {
                copy_options::preflight(
                    &[(source.clone(), target.clone())],
                    &mut guards,
                    cancel,
                    &space,
                )?;
            }
        }
        let mut stack = vec![Work::Visit(source, target)];
        while let Some(work) = stack.pop() {
            check(cancel)?;
            let (source, target) = match work {
                Work::Permissions(source, target, input, output, metadata) => {
                    guards.verify_parent(&source)?;
                    guards.verify_parent(&target)?;
                    verify_entry(&source, &metadata)?;
                    if !crate::platform_fs::owns_file(&target, &output) {
                        return Err(io::Error::other(
                            "Destination directory changed during copy",
                        ));
                    }
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
                    crate::platform_fs::sync_parent(&target)?;
                    continue;
                }
                Work::Visit(source, target) => (source, target),
            };
            let Source {
                path: source,
                metadata: original,
            } = source;
            progress.path = display_path(&source);
            notify(&progress);
            check(cancel)?;
            guards.verify_parent(&source)?;
            guards.verify_parent(&target)?;
            let metadata = verify_entry(&source, &original)?;
            if metadata.is_symlink() {
                copy_link(&source, &target)?;
                guards.verify_parent(&source)?;
                verify_entry(&source, &metadata)?;
                crate::platform_fs::sync_parent(&target)?;
            } else if metadata.is_dir() {
                let source_directory = crate::platform_fs::open_directory(&source)?;
                if !crate::platform_fs::same_identity(&metadata, &source_directory.metadata()?) {
                    return Err(io::Error::other("Source directory changed before copy"));
                }
                fs::create_dir(&target)?;
                let target_directory = crate::platform_fs::open_directory(&target)?;
                guards.capture(&source)?;
                guards.capture(&target)?;
                stack.push(Work::Permissions(
                    source.clone(),
                    target.clone(),
                    source_directory,
                    target_directory,
                    metadata.clone(),
                ));
                for child in fs::read_dir(&source)? {
                    check(cancel)?;
                    guards.verify_directory(&source)?;
                    let child = child?;
                    let path = child.path();
                    let metadata = fs::symlink_metadata(&path)?;
                    stack.push(Work::Visit(
                        Source { path, metadata },
                        target.join(child.file_name()),
                    ));
                }
            } else if metadata.is_file() {
                progress.phase = Phase::Copying;
                let mut input = open_regular(&source)?;
                if !unchanged_file(&metadata, &input.metadata()?) {
                    return Err(io::Error::other("Source changed before copy"));
                }
                absent(&target)?;
                let mut staged = crate::publication::StagedFile::create(&target)?;
                let output = &mut staged.file;
                let mut remaining = metadata.len();
                while remaining > 0 {
                    check(cancel)?;
                    let limit = remaining.min(buffer.len() as u64) as usize;
                    let count = input.read(&mut buffer[..limit])?;
                    if count == 0 {
                        return Err(io::Error::other(
                            "Source shrank during copy; original retained",
                        ));
                    }
                    output.write_all(&buffer[..count])?;
                    remaining -= count as u64;
                    progress.bytes = progress.bytes.saturating_add(count as u64);
                    progress.written = progress.written.saturating_add(count as u64);
                    notify(&progress);
                }
                check(cancel)?;
                if input.read(&mut buffer[..1])? != 0 {
                    return Err(io::Error::other(
                        "Source grew during copy; original retained",
                    ));
                }
                output.sync_all()?;
                if options.verify_writes || options.remove_sources {
                    progress.phase = Phase::Verifying;
                    notify(&progress);
                    copy_options::verify(&mut input, output, metadata.len(), cancel)?;
                }
                check(cancel)?;
                guards.verify_parent(&source)?;
                guards.verify_parent(&target)?;
                if !unchanged_file(&metadata, &input.metadata()?)
                    || !unchanged_file(&metadata, &fs::symlink_metadata(&source)?)
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
                check(cancel)?;
                guards.verify_parent(&source)?;
                guards.verify_parent(&target)?;
                verify_entry(&source, &metadata)?;
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
            notify(&progress);
        }
    }
    if plan.kind == Kind::Move {
        // Validate the whole copied batch before removing its first source.
        progress.phase = Phase::RemovingSources;
        // Later unlinks can change ctime on other source hard links, so cleanup
        // below rechecks identity/content metadata without comparing that field.
        for (source, target, original, destination_metadata) in &copied {
            check(cancel)?;
            guards.verify_parent(source)?;
            guards.verify_parent(target)?;
            verify_entry(source, original)?;
            verify_entry(target, destination_metadata)?;
        }
        // Copy everything successfully before removing any source. Never recursively
        // delete a moved directory: new children must survive, causing remove_dir to fail.
        #[cfg(unix)]
        let mut unlinked = std::collections::HashMap::<(u64, u64), u64>::new();
        for (source, target, original, destination_metadata) in copied.into_iter().rev() {
            check(cancel)?;
            progress.path = display_path(&source);
            notify(&progress);
            check(cancel)?;
            guards.verify_parent(&source)?;
            guards.verify_parent(&target)?;
            let current = fs::symlink_metadata(&source)?;
            let destination_current = fs::symlink_metadata(&target)?;
            if (original.is_dir()
                && !crate::platform_fs::same_identity(&destination_metadata, &destination_current))
                || (!original.is_dir()
                    && !unchanged_file(&destination_metadata, &destination_current))
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
                #[cfg(unix)]
                let unchanged = {
                    use std::os::unix::fs::MetadataExt;
                    let removals = unlinked
                        .get(&(original.dev(), original.ino()))
                        .copied()
                        .unwrap_or(0);
                    if removals == 0 {
                        unchanged_file(&original, &current)
                    } else {
                        // Our previous unlink changes ctime on remaining hard links.
                        same_file(&original, &current)
                            && original.nlink().checked_sub(removals) == Some(current.nlink())
                    }
                };
                #[cfg(not(unix))]
                let unchanged = unchanged_file(&original, &current);
                if !unchanged {
                    return Err(io::Error::other("Source changed after copy; retained"));
                }
                remove_leaf(&source, &current)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    *unlinked
                        .entry((original.dev(), original.ino()))
                        .or_default() += 1;
                }
            }
            crate::platform_fs::sync_parent(&source)?;
        }
    }
    Ok(progress)
}
