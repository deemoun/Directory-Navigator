//! Conservative cache candidates and recoverable publication. No permanent deletion.
use super::*;
use std::{
    fs, io,
    path::Path,
    time::{Duration, SystemTime},
};
const MAX_FILES: usize = 10_000;
const MAX_ENTRIES: usize = 100_000;
const MAX_PATH_BYTES: usize = 4 * 1024 * 1024;
#[derive(Clone, Copy)]
struct Limits {
    files: usize,
    entries: usize,
    directories: usize,
    depth: usize,
    paths: usize,
    mount: fn(&Path) -> io::Result<u64>,
    read: fn(&Path) -> io::Result<fs::ReadDir>,
    progress_interval: Duration,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            files: MAX_FILES,
            entries: MAX_ENTRIES,
            directories: crate::traversal::usage::DIRECTORY_LIMIT,
            depth: crate::traversal::usage::DEPTH_LIMIT,
            paths: MAX_PATH_BYTES,
            mount: mount_id,
            read: read_directory,
            progress_interval: Duration::from_millis(100),
        }
    }
}
const AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);
#[derive(Clone)]
pub(crate) struct Review {
    pub root: PathBuf,
    home: PathBuf,
    root_id: EntryIdentity,
    mount: u64,
    cutoff: SystemTime,
    pub paths: Vec<PathBuf>,
    pub bytes: u128,
    pub stats: ScanStats,
    pub sizes: Vec<u64>,
    prepared: Option<Arc<crate::operations::transfer::Prepared>>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LimitReason {
    Entries,
    Files,
    Directories,
    Depth,
    Paths,
}
impl LimitReason {
    fn label(self) -> &'static str {
        match self {
            Self::Entries => "Entry limit reached.",
            Self::Files => "Candidate file limit reached.",
            Self::Directories => "Directory limit reached.",
            Self::Depth => "Directory depth limit reached.",
            Self::Paths => "Review path-storage limit reached.",
        }
    }
}
#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub(crate) struct ScanStats {
    pub root: PathBuf,
    pub limit: Option<LimitReason>,
    pub scanned: u64,
    pub eligible: u64,
    pub recent: u64,
    pub excluded: u64,
    pub unreadable: u64,
    pub first_error: String,
}
impl ScanStats {
    fn error(&mut self, path: &Path, error: &io::Error) {
        self.unreadable += 1;
        if self.first_error.is_empty() {
            self.first_error = format!("{}: {error}", path.display())
                .chars()
                .take(2048)
                .collect();
        }
    }
    pub fn details(&self, lang: crate::localization::Language) -> String {
        format!(
            "{}\n{}\n{}\n{}",
            crate::filesystem::display_name(&self.root.to_string_lossy()),
            self.summary(lang),
            self.limit.map_or("", |reason| lang.text(reason.label())),
            crate::filesystem::display_name(&self.first_error)
        )
    }
    pub fn progress(&self, lang: crate::localization::Language) -> String {
        format!(
            "{} {} · {} {} · {} {}",
            lang.text("Scanned:"),
            self.scanned,
            lang.text("Eligible:"),
            self.eligible,
            lang.text("Unreadable:"),
            self.unreadable
        )
    }
    pub fn summary(&self, lang: crate::localization::Language) -> String {
        format!(
            "{} {} · {} {} · {} {} · {} {}",
            lang.text("Scanned:"),
            self.scanned,
            lang.text("Recent:"),
            self.recent,
            lang.text("Excluded:"),
            self.excluded,
            lang.text("Unreadable:"),
            self.unreadable
        )
    }
}
#[derive(Clone, Default)]
pub(crate) struct Selection {
    pub mask: Vec<bool>,
    pub count: usize,
    pub bytes: u128,
}
impl Selection {
    pub fn set(&mut self, review: &Review, value: bool) {
        self.mask = vec![value; review.paths.len()];
        self.count = if value { review.paths.len() } else { 0 };
        self.bytes = if value { review.bytes } else { 0 };
    }
    pub fn toggle(&mut self, review: &Review, index: usize) {
        if self.mask.len() != review.paths.len() || index >= self.mask.len() {
            return;
        }
        self.mask[index] = !self.mask[index];
        if self.mask[index] {
            self.count += 1;
            self.bytes += u128::from(review.sizes[index]);
        } else {
            self.count -= 1;
            self.bytes -= u128::from(review.sizes[index]);
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    Running,
    Completed,
    Cancelled,
    Failed,
    Unconfirmed,
}
impl Outcome {
    pub fn label(self) -> &'static str {
        match self {
            Self::Running => "Cleanup running",
            Self::Completed => "Cleanup completed",
            Self::Cancelled => "Cleanup cancelled",
            Self::Failed => "Cleanup failed",
            Self::Unconfirmed => "Cleanup outcome unconfirmed",
        }
    }
}
#[derive(Clone)]
pub(crate) struct Report {
    pub root: PathBuf,
    pub selected: usize,
    pub moved: u64,
    pub not_selected: usize,
    pub outcome: Outcome,
    pub error: String,
}
impl Report {
    pub fn running(&self) -> bool {
        self.outcome == Outcome::Running
    }
    pub fn new(root: PathBuf, selected: usize, not_selected: usize) -> Self {
        Self {
            root,
            selected,
            moved: 0,
            not_selected,
            outcome: Outcome::Running,
            error: String::new(),
        }
    }
    pub fn summary(&self, lang: crate::localization::Language) -> String {
        format!(
            "{} {}/{} · {} {} · {} {}",
            lang.text("Last cleanup:"),
            self.moved,
            self.selected,
            lang.text(if self.outcome == Outcome::Unconfirmed {
                "Unconfirmed:"
            } else {
                "Not moved:"
            }),
            self.selected.saturating_sub(self.moved as usize),
            lang.text("Not selected:"),
            self.not_selected
        )
    }
    pub fn stopped(&mut self, outcome: Outcome, cause: &str) {
        self.outcome = outcome;
        self.error = cause.chars().take(2048).collect();
    }
}
impl Review {
    pub fn partial(&self) -> bool {
        self.stats.limit.is_some() || self.stats.unreadable != 0
    }
    pub fn notice(&self) -> &'static str {
        match (self.paths.is_empty(), self.partial()) {
            (true, true) => "Inspection partial; no eligible files in the inspected portion.",
            (true, false) => "No eligible old cache files were found.",
            (false, true) => "Partial inspection; review the available candidates.",
            (false, false) => "Review ready; Clean… opens the candidate list.",
        }
    }
    pub fn selected(&self, selection: &Selection) -> io::Result<Self> {
        if selection.mask.len() != self.paths.len() || !selection.mask.iter().any(|v| *v) {
            return Err(io::Error::other("Select at least one cache file."));
        }
        let mut output = self.clone();
        output.paths = self
            .paths
            .iter()
            .zip(&selection.mask)
            .filter(|(_, yes)| **yes)
            .map(|(p, _)| p.clone())
            .collect();
        output.sizes = self
            .sizes
            .iter()
            .zip(&selection.mask)
            .filter(|(_, yes)| **yes)
            .map(|(size, _)| *size)
            .collect();
        output.bytes = output.sizes.iter().map(|size| u128::from(*size)).sum();
        Ok(output)
    }
}
fn read_directory(path: &Path) -> io::Result<fs::ReadDir> {
    fs::read_dir(path)
}
struct ScanRequest {
    root: PathBuf,
    home: PathBuf,
    id: EntryIdentity,
    now: SystemTime,
}
fn mount_id(path: &Path) -> io::Result<u64> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::ffi::OsStrExt;
        let path = std::ffi::CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::other("Invalid native path"))?;
        let mut info: libc::statx = unsafe { std::mem::zeroed() };
        if unsafe {
            libc::statx(
                libc::AT_FDCWD,
                path.as_ptr(),
                libc::AT_SYMLINK_NOFOLLOW | libc::AT_NO_AUTOMOUNT,
                libc::STATX_MNT_ID,
                &mut info,
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        if info.stx_mask & libc::STATX_MNT_ID == 0 {
            return Err(io::Error::other(
                "Mount identity is unavailable; cache files retained",
            ));
        }
        Ok(info.stx_mnt_id)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        Ok(0)
    } // Only synthetic/private tests use cleanup on other hosts.
}
fn owned(meta: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        meta.uid() == unsafe { libc::geteuid() } && meta.mode() & 0o022 == 0
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        false
    }
}
fn candidate(meta: &fs::Metadata, cutoff: SystemTime) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.nlink() != 1 {
            return false;
        }
    }
    meta.is_file()
        && !crate::platform_fs::directory_link(meta)
        && owned(meta)
        && meta.modified().is_ok_and(|t| t <= cutoff)
}
fn root_guard(root: &Path, home: &Path, id: EntryIdentity) -> io::Result<()> {
    if !root.starts_with(home) || root == home || root.parent().is_none() || home.parent().is_none()
    {
        return Err(io::Error::other(
            "Cleanup requires a dedicated cache directory beneath your home folder.",
        ));
    }
    if [
        "/home", "/Users", "/var", "/usr", "/etc", "/tmp", "/run", "/opt", "/dev", "/proc", "/sys",
        "/bin", "/sbin", "/lib", "/lib64",
    ]
    .iter()
    .any(|p| home == Path::new(p))
    {
        return Err(io::Error::other(
            "Cleanup requires a dedicated cache directory beneath your home folder.",
        ));
    }
    let mut protected_paths = Vec::new();
    for key in ["XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_STATE_HOME"] {
        if let Some(path) = std::env::var_os(key)
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
        {
            protected_paths.push(path);
        }
    }
    for suffix in [
        ".config",
        ".local/share",
        ".local/state",
        "Documents",
        "Desktop",
        "Downloads",
        "Pictures",
        "Music",
        "Videos",
        "Applications",
        "Library",
    ] {
        protected_paths.push(home.join(suffix));
    }
    for protected in protected_paths {
        let protected = fs::canonicalize(&protected).unwrap_or(protected);
        if root.starts_with(&protected) || protected.starts_with(root) {
            return Err(io::Error::other("This location contains protected application or personal data; cleanup is unavailable."));
        }
    }
    for parent in root.ancestors().take_while(|p| p.starts_with(home)) {
        let metadata = fs::symlink_metadata(parent)?;
        if !metadata.is_dir() || crate::platform_fs::directory_link(&metadata) || !owned(&metadata)
        {
            return Err(io::Error::other(
                "Cache ancestors must be privately controlled real directories.",
            ));
        }
    }
    let meta = fs::symlink_metadata(root)?;
    if !meta.is_dir()
        || crate::platform_fs::directory_link(&meta)
        || !owned(&meta)
        || entry_identity(root)? != id
    {
        return Err(io::Error::other(
            "Cache directory changed or is not privately controlled by the current user.",
        ));
    }
    Ok(())
}
#[cfg(all(test, unix))]
pub(super) fn collect(
    root: PathBuf,
    home: PathBuf,
    id: EntryIdentity,
    now: SystemTime,
    cancel: &Cancellation,
) -> io::Result<Review> {
    collect_with(root, home, id, now, cancel, Limits::default())
}
#[cfg(all(test, unix))]
fn collect_with(
    root: PathBuf,
    home: PathBuf,
    id: EntryIdentity,
    now: SystemTime,
    cancel: &Cancellation,
    limits: Limits,
) -> io::Result<Review> {
    collect_using(
        ScanRequest {
            root,
            home,
            id,
            now,
        },
        cancel,
        limits,
        |_| {},
    )
}
pub(super) fn collect_progress(
    root: PathBuf,
    home: PathBuf,
    id: EntryIdentity,
    now: SystemTime,
    cancel: &Cancellation,
    emit: impl FnMut(&ScanStats),
) -> io::Result<Review> {
    collect_using(
        ScanRequest {
            root,
            home,
            id,
            now,
        },
        cancel,
        Limits::default(),
        emit,
    )
}
fn collect_using(
    request: ScanRequest,
    cancel: &Cancellation,
    limits: Limits,
    mut emit: impl FnMut(&ScanStats),
) -> io::Result<Review> {
    let ScanRequest {
        root,
        home,
        id,
        now,
    } = request;
    if !root.is_absolute() || !home.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Cleanup requires a dedicated cache directory beneath your home folder.",
        ));
    }
    let home = fs::canonicalize(home)?;
    root_guard(&root, &home, id)?;
    let cutoff = now
        .checked_sub(AGE)
        .ok_or_else(|| io::Error::other("Cannot establish cache file age"))?;
    let mount = (limits.mount)(&root)?;
    let mut result = Review {
        root: root.clone(),
        home,
        root_id: id,
        mount,
        cutoff,
        paths: Vec::new(),
        bytes: 0,
        stats: ScanStats {
            root: root.clone(),
            ..Default::default()
        },
        sizes: Vec::new(),
        prepared: None,
    };
    let mut observed = std::collections::HashMap::new();
    let mut stack = vec![(root, 0)];
    let (mut entries, mut directories, mut path_bytes) = (0, 1, 0);
    let mut last = std::time::Instant::now();
    emit(&result.stats);
    while let Some((dir, depth)) = stack.pop() {
        cancel.check()?;
        if last.elapsed() >= limits.progress_interval {
            emit(&result.stats);
            last = std::time::Instant::now();
        }
        root_guard(&result.root, &result.home, result.root_id)?;
        let meta = match fs::symlink_metadata(&dir) {
            Ok(v) => v,
            Err(e) => {
                result.stats.error(&dir, &e);
                continue;
            }
        };
        if !meta.is_dir() || crate::platform_fs::directory_link(&meta) || !owned(&meta) {
            result.stats.excluded += 1;
            continue;
        }
        let directory_id = match entry_identity(&dir) {
            Ok(v) => v,
            Err(e) => {
                result.stats.error(&dir, &e);
                continue;
            }
        };
        let directory_mount = match (limits.mount)(&dir) {
            Ok(v) => v,
            Err(e) => {
                result.stats.error(&dir, &e);
                continue;
            }
        };
        if !directory_id.same_volume(id) || directory_mount != mount {
            result.stats.excluded += 1;
            continue;
        }
        let children = match (limits.read)(&dir) {
            Ok(v) => v,
            Err(e) => {
                result.stats.error(&dir, &e);
                if dir == result.root {
                    emit(&result.stats);
                    return Err(e);
                }
                continue;
            }
        };
        for entry in children {
            cancel.check()?;
            if last.elapsed() >= limits.progress_interval {
                emit(&result.stats);
                last = std::time::Instant::now();
            }
            if entry_identity(&dir)? != directory_id {
                return Err(io::Error::other("Cache directory changed during review"));
            }
            if entries >= limits.entries {
                result.stats.limit.get_or_insert(LimitReason::Entries);
                break;
            }
            entries += 1;
            result.stats.scanned += 1;
            let entry = match entry {
                Ok(v) => v,
                Err(e) => {
                    result.stats.error(&dir, &e);
                    continue;
                }
            };
            let path = entry.path();
            let meta = match fs::symlink_metadata(&path) {
                Ok(v) => v,
                Err(e) => {
                    result.stats.error(&path, &e);
                    continue;
                }
            };
            if crate::platform_fs::directory_link(&meta) {
                result.stats.excluded += 1;
                continue;
            }
            let identity = match entry_identity(&path) {
                Ok(v) => v,
                Err(e) => {
                    result.stats.error(&path, &e);
                    continue;
                }
            };
            let entry_mount = match (limits.mount)(&path) {
                Ok(v) => v,
                Err(e) => {
                    result.stats.error(&path, &e);
                    continue;
                }
            };
            if !identity.same_volume(id) || entry_mount != mount {
                result.stats.excluded += 1;
                continue;
            }
            if meta.is_dir() {
                if directories >= limits.directories || depth + 1 >= limits.depth {
                    result
                        .stats
                        .limit
                        .get_or_insert(if directories >= limits.directories {
                            LimitReason::Directories
                        } else {
                            LimitReason::Depth
                        });
                    continue;
                }
                directories += 1;
                stack.push((path, depth + 1));
            } else if candidate(&meta, cutoff) {
                let length = path.as_os_str().len();
                if result.paths.len() >= limits.files || path_bytes + length > limits.paths {
                    result
                        .stats
                        .limit
                        .get_or_insert(if result.paths.len() >= limits.files {
                            LimitReason::Files
                        } else {
                            LimitReason::Paths
                        });
                    break;
                }
                path_bytes += length;
                result.bytes += u128::from(meta.len());
                result.stats.eligible += 1;
                observed.insert(path.clone(), meta);
                result.paths.push(path);
            } else if meta.is_file() && meta.modified().is_ok_and(|t| t > cutoff) {
                result.stats.recent += 1;
            } else if meta.is_file() && meta.modified().is_err() {
                result.stats.error(
                    &path,
                    &io::Error::other("File modification time is unavailable"),
                );
            } else {
                result.stats.excluded += 1;
            }
        }
        if result.stats.limit.is_some() {
            break;
        }
    }
    cancel.check()?;
    result.paths.sort();
    root_guard(&result.root, &result.home, result.root_id)?;
    if (limits.mount)(&result.root)? != mount {
        return Err(io::Error::other("Cache mount changed during review"));
    }
    if !result.paths.is_empty() {
        result.prepared = Some(Arc::new(crate::operations::transfer::Prepared::capture(
            &crate::operations::Plan {
                kind: crate::operations::Kind::Trash,
                sources: result.paths.clone(),
                destination: result.root.clone(),
            },
        )?));
        for path in &result.paths {
            cancel.check()?;
            let previous = &observed[path];
            let current = fs::symlink_metadata(path)?;
            #[cfg(unix)]
            let same_change = {
                use std::os::unix::fs::MetadataExt;
                previous.ctime() == current.ctime() && previous.ctime_nsec() == current.ctime_nsec()
            };
            #[cfg(not(unix))]
            let same_change = true;
            if !crate::platform_fs::same_identity(previous, &current)
                || previous.len() != current.len()
                || previous.modified().ok() != current.modified().ok()
                || !same_change
                || !candidate(&current, cutoff)
            {
                return Err(io::Error::other(
                    "Cache file changed during review; nothing was moved",
                ));
            }
            result.prepared.as_ref().unwrap().verify_source(path)?;
        }
    }
    result.sizes = result.paths.iter().map(|p| observed[p].len()).collect();
    emit(&result.stats);
    Ok(result)
}
pub(super) fn execute(
    mut view: Snapshot,
    review: Arc<Review>,
    cancel: Cancellation,
    latest: Arc<Mutex<Option<Snapshot>>>,
    mut trash: impl FnMut(&Path) -> io::Result<()>,
) -> Snapshot {
    view.review = None;
    view.cleanup_report = Some(Report::new(
        review.root.clone(),
        review.paths.len(),
        view.cleanup_selection
            .mask
            .len()
            .saturating_sub(review.paths.len()),
    ));
    view.cleanup_selection = Selection::default();
    view.notice = "Moving reviewed cache files to Trash…".into();
    for row in &mut view.rows {
        row.stale |= row.measurement.is_some();
    }
    publish(&latest, &view);
    let mut last = std::time::Instant::now();
    let result = (|| {
        let prepared = review
            .prepared
            .as_ref()
            .ok_or_else(|| io::Error::other("No eligible cache files"))?;
        for path in &review.paths {
            cancel.check()?;
            root_guard(&review.root, &review.home, review.root_id)?;
            prepared.verify_source(path)?;
            if !candidate(&fs::symlink_metadata(path)?, review.cutoff) {
                return Err(io::Error::other(
                    "Cache file is no longer eligible; remaining files retained",
                ));
            }
            if mount_id(path)? != review.mount || mount_id(&review.root)? != review.mount {
                return Err(io::Error::other(
                    "Cache mount changed; remaining files retained",
                ));
            }
            // Recheck every ancestor inside the cache immediately before Trash.
            for parent in path
                .parent()
                .unwrap()
                .ancestors()
                .take_while(|p| p.starts_with(&review.root))
            {
                let meta = fs::symlink_metadata(parent)?;
                if !meta.is_dir()
                    || crate::platform_fs::directory_link(&meta)
                    || !owned(&meta)
                    || mount_id(parent)? != review.mount
                {
                    return Err(io::Error::other(
                        "Cache ancestor changed; remaining files retained",
                    ));
                }
            }
            cancel.check()?;
            trash(path)?;
            view.cleanup_report.as_mut().unwrap().moved += 1;
            if last.elapsed() >= Duration::from_millis(100) {
                publish(&latest, &view);
                last = std::time::Instant::now();
            }
        }
        Ok::<_, io::Error>(())
    })();
    view.busy = false;
    view.stopping = false;
    for row in &mut view.rows {
        row.stale |= row.measurement.is_some();
    }
    match result {
        Ok(()) => {
            view.cleanup_report.as_mut().unwrap().outcome = Outcome::Completed;
            view.notice = "Cleanup completed; files are in Trash. Measurements are stale.".into()
        }
        Err(e) => {
            view.cleanup_report.as_mut().unwrap().stopped(
                if e.kind() == io::ErrorKind::Interrupted {
                    Outcome::Cancelled
                } else {
                    Outcome::Failed
                },
                &e.to_string(),
            );
            view.notice = if e.kind() == io::ErrorKind::Interrupted {
                "Cleanup cancelled; confirmed moves remain in Trash. Check remaining items."
            } else {
                "Cleanup failed; confirmed moves remain in Trash. Check remaining items."
            }
            .into();
            view.error = e.to_string().chars().take(2048).collect();
        }
    }
    view
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::{
        fs::{File, FileTimes},
        sync::atomic::{AtomicU64, Ordering},
    };
    struct Fixture {
        home: PathBuf,
        cache: PathBuf,
        now: SystemTime,
    }
    impl Fixture {
        fn new() -> io::Result<Self> {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let home = std::env::temp_dir().join(format!(
                "dn-clean-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let cache = home.join(".cache");
            fs::create_dir_all(&cache)?;
            let home = fs::canonicalize(home)?;
            let cache = home.join(".cache");
            fs::set_permissions(&home, fs::Permissions::from_mode(0o700))?;
            fs::set_permissions(&cache, fs::Permissions::from_mode(0o700))?;
            Ok(Self {
                home,
                cache,
                now: SystemTime::now(),
            })
        }
        fn old(&self, name: &str) -> io::Result<PathBuf> {
            let path = self.cache.join(name);
            fs::create_dir_all(path.parent().unwrap())?;
            fs::write(&path, b"old cache")?;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
            for directory in path
                .parent()
                .unwrap()
                .ancestors()
                .take_while(|p| p.starts_with(&self.cache))
            {
                fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
            }
            File::options().write(true).open(&path)?.set_times(
                FileTimes::new().set_modified(self.now - Duration::from_secs(9 * 24 * 60 * 60)),
            )?;
            Ok(path)
        }
        fn review(&self) -> io::Result<Review> {
            collect(
                self.cache.clone(),
                self.home.clone(),
                entry_identity(&self.cache)?,
                self.now,
                &Cancellation::default(),
            )
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.home);
        }
    }
    #[test]
    fn old_regular_files_only_preserve_recent_links_hardlinks_and_directories() -> io::Result<()> {
        use std::os::unix::fs::symlink;
        let f = Fixture::new()?;
        let old = f.old("nested/Ё cache")?;
        let hidden = f.old(".hidden")?;
        fs::write(f.cache.join("recent"), b"recent")?;
        let hard = f.old("hard")?;
        fs::hard_link(&hard, f.cache.join("other-hard"))?;
        let outside = f.home.join("outside");
        fs::write(&outside, b"valuable")?;
        symlink(&outside, f.cache.join("link"))?;
        symlink(&f.home, f.cache.join("cycle"))?;
        fs::create_dir(f.cache.join("empty"))?;
        let review = f.review()?;
        assert_eq!(review.paths.len(), 2);
        assert!(review.paths.contains(&old) && review.paths.contains(&hidden));
        assert_eq!(review.bytes, 18);
        assert!(review.stats.recent + review.stats.excluded >= 4);
        let trash = f.home.join("private-trash");
        let view = execute(
            Snapshot::default(),
            Arc::new(review),
            Cancellation::default(),
            Arc::default(),
            |p| crate::trash_support::freedesktop(p, &trash),
        );
        assert_eq!(view.cleanup_report.as_ref().map(|r| r.moved), Some(2));
        assert!(view.error.is_empty());
        assert!(f.cache.join("nested").is_dir() && f.cache.join("empty").is_dir());
        assert_eq!(fs::read(outside)?, b"valuable");
        assert_eq!(fs::read(f.cache.join("recent"))?, b"recent");
        assert!(hard.exists());
        assert_eq!(fs::read_dir(trash.join("files"))?.count(), 2);
        assert_eq!(fs::read_dir(trash.join("info"))?.count(), 2);
        Ok(())
    }
    #[test]
    fn source_and_root_changes_stop_before_trash() -> io::Result<()> {
        for replace_root in [false, true] {
            let f = Fixture::new()?;
            let file = f.old("old")?;
            let review = f.review()?;
            if replace_root {
                fs::rename(&f.cache, f.home.join("original-cache"))?;
                fs::create_dir(&f.cache)?;
                fs::write(&file, b"replacement")?;
            } else {
                fs::write(&file, b"changed source data")?;
            }
            let view = execute(
                Snapshot::default(),
                Arc::new(review),
                Cancellation::default(),
                Arc::default(),
                |_| panic!("changed sources must never reach Trash"),
            );
            assert_eq!(view.cleanup_report.as_ref().map(|r| r.moved), Some(0));
            assert!(!view.error.is_empty());
            assert!(file.exists());
        }
        Ok(())
    }
    #[test]
    fn changed_ancestor_does_not_follow_a_link_outside_cache() -> io::Result<()> {
        use std::os::unix::fs::symlink;
        let f = Fixture::new()?;
        f.old("nested/old")?;
        let review = f.review()?;
        fs::rename(f.cache.join("nested"), f.home.join("original-nested"))?;
        let outside = f.home.join("valuable");
        fs::create_dir(&outside)?;
        fs::write(outside.join("old"), b"keep")?;
        symlink(&outside, f.cache.join("nested"))?;
        let view = execute(
            Snapshot::default(),
            Arc::new(review),
            Cancellation::default(),
            Arc::default(),
            |_| panic!("changed ancestor must not reach Trash"),
        );
        assert_eq!(view.cleanup_report.as_ref().map(|r| r.moved), Some(0));
        assert_eq!(fs::read(outside.join("old"))?, b"keep");
        Ok(())
    }
    #[test]
    fn cancellation_retains_completed_trash_moves_and_never_removes_directories() -> io::Result<()>
    {
        let f = Fixture::new()?;
        f.old("a")?;
        f.old("b")?;
        let review = f.review()?;
        let cancel = Cancellation::default();
        let stop = cancel.clone();
        let trash = f.home.join("private-trash");
        let view = execute(
            Snapshot::default(),
            Arc::new(review),
            cancel,
            Arc::default(),
            |p| {
                crate::trash_support::freedesktop(p, &trash)?;
                stop.cancel();
                Ok(())
            },
        );
        assert_eq!(view.cleanup_report.as_ref().map(|r| r.moved), Some(1));
        assert_eq!(fs::read_dir(&f.cache)?.count(), 1);
        assert_eq!(fs::read_dir(trash.join("files"))?.count(), 1);
        assert!(view.notice.contains("confirmed moves remain in Trash"));
        Ok(())
    }
    #[test]
    fn review_excludes_mount_boundaries_without_requiring_real_mounts() -> io::Result<()> {
        let f = Fixture::new()?;
        let old = f.old("old")?;
        f.old("mounted/keep")?;
        fn probe(path: &Path) -> io::Result<u64> {
            Ok(u64::from(path.file_name().is_some_and(|n| n == "mounted")))
        }
        let review = collect_with(
            f.cache.clone(),
            f.home.clone(),
            entry_identity(&f.cache)?,
            f.now,
            &Cancellation::default(),
            Limits {
                mount: probe,
                ..Default::default()
            },
        )?;
        assert_eq!(review.paths, vec![old]);
        assert!(review.stats.excluded >= 1);
        Ok(())
    }
    #[test]
    fn selected_files_only_are_trashed_and_unchecked_changes_do_not_block_them() -> io::Result<()> {
        let f = Fixture::new()?;
        let a = f.old("a")?;
        let b = f.old("b")?;
        let c = f.old("c")?;
        let review = f.review()?;
        let mut selection = Selection::default();
        selection.set(&review, true);
        assert_eq!(selection.count, 3);
        assert_eq!(selection.bytes, 27);
        selection.toggle(&review, 1);
        assert_eq!(selection.count, 2);
        assert_eq!(selection.bytes, 18);
        fs::write(&b, b"unchecked file changed and must remain")?;
        let selected = review.selected(&selection)?;
        assert_eq!(selected.paths, vec![a.clone(), c.clone()]);
        let trash = f.home.join("private-trash");
        let view = execute(
            Snapshot {
                cleanup_selection: selection,
                ..Default::default()
            },
            Arc::new(selected),
            Cancellation::default(),
            Arc::default(),
            |p| crate::trash_support::freedesktop(p, &trash),
        );
        let report = view.cleanup_report.as_ref().unwrap();
        assert_eq!(report.outcome, Outcome::Completed);
        assert_eq!(report.moved, 2);
        assert_eq!(report.selected, 2);
        assert_eq!(report.not_selected, 1);
        assert!(!a.exists() && !c.exists());
        assert_eq!(fs::read(b)?, b"unchecked file changed and must remain");
        let mut none = Selection::default();
        none.set(&review, false);
        assert!(review.selected(&none).is_err());
        none.mask.push(true);
        assert!(review.selected(&none).is_err());
        Ok(())
    }
    #[test]
    fn empty_partial_and_failed_inspections_are_distinct_and_keep_error_causes() -> io::Result<()> {
        let f = Fixture::new()?;
        let empty = f.review()?;
        assert_eq!(empty.notice(), "No eligible old cache files were found.");
        fs::write(f.cache.join("recent"), b"new")?;
        let partial = collect_with(
            f.cache.clone(),
            f.home.clone(),
            entry_identity(&f.cache)?,
            f.now,
            &Cancellation::default(),
            Limits {
                entries: 0,
                ..Default::default()
            },
        )?;
        assert!(partial.paths.is_empty() && partial.partial());
        assert_eq!(
            partial.notice(),
            "Inspection partial; no eligible files in the inspected portion."
        );
        f.old("denied/old")?;
        let good = f.old("good")?;
        fn denied(path: &Path) -> io::Result<fs::ReadDir> {
            if path.file_name().is_some_and(|n| n == "denied") {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "private inspection denial",
                ))
            } else {
                fs::read_dir(path)
            }
        }
        let review = collect_with(
            f.cache.clone(),
            f.home.clone(),
            entry_identity(&f.cache)?,
            f.now,
            &Cancellation::default(),
            Limits {
                read: denied,
                ..Default::default()
            },
        )?;
        assert_eq!(review.paths, vec![good]);
        assert_eq!(review.stats.recent, 1);
        assert_eq!(review.stats.unreadable, 1);
        assert!(review.partial());
        assert!(review
            .stats
            .first_error
            .contains("private inspection denial"));
        assert_eq!(
            review.notice(),
            "Partial inspection; review the available candidates."
        );
        fn root_denied(_: &Path) -> io::Result<fs::ReadDir> {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "private root denial",
            ))
        }
        let failed = collect_with(
            f.cache.clone(),
            f.home.clone(),
            entry_identity(&f.cache)?,
            f.now,
            &Cancellation::default(),
            Limits {
                read: root_denied,
                ..Default::default()
            },
        );
        assert!(matches!(failed,Err(e) if e.kind()==io::ErrorKind::PermissionDenied));
        Ok(())
    }
    #[test]
    fn preparation_progress_is_monotonic_and_finishes_with_exact_counters() -> io::Result<()> {
        let f = Fixture::new()?;
        f.old("a")?;
        f.old("b")?;
        fs::write(f.cache.join("recent"), b"new")?;
        let mut events = Vec::new();
        let review = collect_using(
            ScanRequest {
                root: f.cache.clone(),
                home: f.home.clone(),
                id: entry_identity(&f.cache)?,
                now: f.now,
            },
            &Cancellation::default(),
            Limits {
                progress_interval: Duration::ZERO,
                ..Default::default()
            },
            |stats| events.push(stats.clone()),
        )?;
        assert_eq!(events.first().unwrap().scanned, 0);
        assert!(events.windows(2).all(|w| w[0].scanned <= w[1].scanned));
        assert_eq!(events.last(), Some(&review.stats));
        assert_eq!(review.stats.scanned, 3);
        assert_eq!(review.stats.eligible, 2);
        assert_eq!(review.stats.recent, 1);
        assert_eq!(review.stats.unreadable, 0);
        Ok(())
    }
    #[test]
    fn each_review_bound_retains_a_specific_partial_reason() -> io::Result<()> {
        let f = Fixture::new()?;
        f.old("nested/old")?;
        f.old("root-old")?;
        for (limits, reason) in [
            (
                Limits {
                    entries: 0,
                    ..Default::default()
                },
                LimitReason::Entries,
            ),
            (
                Limits {
                    files: 0,
                    ..Default::default()
                },
                LimitReason::Files,
            ),
            (
                Limits {
                    directories: 1,
                    ..Default::default()
                },
                LimitReason::Directories,
            ),
            (
                Limits {
                    depth: 1,
                    ..Default::default()
                },
                LimitReason::Depth,
            ),
            (
                Limits {
                    paths: 0,
                    ..Default::default()
                },
                LimitReason::Paths,
            ),
        ] {
            let review = collect_with(
                f.cache.clone(),
                f.home.clone(),
                entry_identity(&f.cache)?,
                f.now,
                &Cancellation::default(),
                limits,
            )?;
            assert_eq!(review.stats.limit, Some(reason));
            assert!(review.partial());
            assert!(review
                .stats
                .details(crate::localization::Language::English)
                .contains(reason.label()));
        }
        Ok(())
    }
    #[test]
    fn relative_home_is_rejected_instead_of_resolving_against_the_working_directory(
    ) -> io::Result<()> {
        let f = Fixture::new()?;
        f.old("old")?;
        let mut relative = PathBuf::new();
        for _ in std::env::current_dir()?.ancestors().skip(1) {
            relative.push("..");
        }
        for component in f.home.components() {
            if let std::path::Component::Normal(name) = component {
                relative.push(name);
            }
        }
        assert!(!relative.is_absolute());
        assert_eq!(fs::canonicalize(&relative)?, f.home);
        assert!(collect(
            f.cache.clone(),
            relative,
            entry_identity(&f.cache)?,
            f.now,
            &Cancellation::default()
        )
        .is_err());
        Ok(())
    }
    #[test]
    fn trash_failure_has_no_permanent_delete_fallback() -> io::Result<()> {
        let f = Fixture::new()?;
        let file = f.old("old")?;
        let view = execute(
            Snapshot::default(),
            Arc::new(f.review()?),
            Cancellation::default(),
            Arc::default(),
            |_| {
                Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "Trash unavailable",
                ))
            },
        );
        assert_eq!(view.cleanup_report.as_ref().map(|r| r.moved), Some(0));
        assert_eq!(fs::read(file)?, b"old cache");
        assert!(view.error.contains("Trash unavailable"));
        Ok(())
    }
    #[test]
    fn review_age_boundary_limits_cancellation_and_protected_roots() -> io::Result<()> {
        let f = Fixture::new()?;
        let file = f.old("a")?;
        f.old("b")?;
        let time = fs::metadata(&file)?.modified()?;
        assert!(candidate(&fs::metadata(file)?, time + AGE));
        let limited = collect_with(
            f.cache.clone(),
            f.home.clone(),
            entry_identity(&f.cache)?,
            f.now,
            &Cancellation::default(),
            Limits {
                files: 1,
                ..Default::default()
            },
        )?;
        assert!(limited.stats.limit.is_some());
        assert_eq!(limited.paths.len(), 1);
        let cancel = Cancellation::default();
        cancel.cancel();
        assert!(collect(
            f.cache.clone(),
            f.home.clone(),
            entry_identity(&f.cache)?,
            f.now,
            &cancel
        )
        .is_err());
        assert!(collect(
            f.home.clone(),
            f.home.clone(),
            entry_identity(&f.home)?,
            f.now,
            &Cancellation::default()
        )
        .is_err());
        let data = f.home.join(".local/share");
        fs::create_dir_all(&data)?;
        assert!(collect(
            data.clone(),
            f.home.clone(),
            entry_identity(&data)?,
            f.now,
            &Cancellation::default()
        )
        .is_err());
        Ok(())
    }
}
