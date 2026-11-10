//! One-pass, bounded apparent-size analysis. No content reads or link traversal.
use super::{send, Event, Stats};
use crate::{
    platform_fs::{directory_link, entry_identity, EntryIdentity},
    task::Cancellation,
};
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::mpsc::SyncSender,
    time::{Duration, Instant},
};

pub const DIRECTORY_LIMIT: usize = 20_000;
pub const DEPTH_LIMIT: usize = 512;
const BATCH: usize = 64;
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Measurement {
    pub bytes: u128,
    pub direct_bytes: u128,
    pub files: u64,
    pub directories: u64,
    pub links: u64,
    pub skipped: u64,
    pub excluded: u64,
    pub partial: bool,
}
impl Measurement {
    fn add(&mut self, child: &Self) {
        self.bytes += child.bytes;
        self.files += child.files;
        self.directories += child.directories;
        self.links += child.links;
        self.skipped += child.skipped;
        self.excluded += child.excluded;
        self.partial |= child.partial;
    }
}
#[derive(Clone, Debug, Default)]
pub struct Update {
    pub records: Vec<(PathBuf, Measurement)>,
    pub files: u64,
    pub bytes: u128,
    pub excluded: u64,
    pub root_changed: bool,
    pub skipped: u64,
    pub first_error: Option<String>,
}
struct Frame {
    path: PathBuf,
    identity: EntryIdentity,
    entries: fs::ReadDir,
    value: Measurement,
}
struct Scanner<'a, P, E> {
    cancel: &'a Cancellation,
    probe: P,
    emit: E,
    limit: usize,
    depth: usize,
    stats: Stats,
    update: Update,
    count: usize,
    limited: bool,
    last: Instant,
}
impl<P, E> Scanner<'_, P, E>
where
    P: FnMut(&Path) -> io::Result<EntryIdentity>,
    E: FnMut(Update) -> bool,
{
    fn flush(&mut self) -> bool {
        self.update.skipped = self.stats.skipped;
        self.update.first_error.clone_from(&self.stats.first_error);
        let next = Update {
            files: self.update.files,
            bytes: self.update.bytes,
            excluded: self.update.excluded,
            root_changed: self.update.root_changed,
            ..Default::default()
        };
        self.last = Instant::now();
        (self.emit)(std::mem::replace(&mut self.update, next))
    }
    fn record(&mut self, path: PathBuf, value: Measurement) -> bool {
        self.update.records.push((path, value));
        if self.update.records.len() >= BATCH || self.last.elapsed() >= Duration::from_millis(100) {
            self.flush()
        } else {
            true
        }
    }
    fn error(&mut self, frame: &mut Frame, path: &Path, error: impl std::fmt::Display) {
        frame.value.skipped += 1;
        frame.value.partial = true;
        self.stats.error(path, error);
    }
    fn scan(&mut self, root: &Path) {
        let initial = (|| {
            let meta = fs::symlink_metadata(root)?;
            if !meta.is_dir() || directory_link(&meta) {
                return Err(io::Error::other("Select a real directory for analysis"));
            }
            let identity = (self.probe)(root)?;
            let entries = fs::read_dir(root)?;
            if (self.probe)(root)? != identity {
                return Err(io::Error::other("Directory changed while scanning"));
            }
            Ok(Frame {
                path: root.to_owned(),
                identity,
                entries,
                value: Measurement::default(),
            })
        })();
        let first = match initial {
            Ok(f) => f,
            Err(e) => {
                self.stats.error(root, e);
                self.record(
                    root.to_owned(),
                    Measurement {
                        skipped: 1,
                        partial: true,
                        ..Default::default()
                    },
                );
                return;
            }
        };
        let volume = first.identity;
        self.count = 1;
        let mut stack = vec![first];
        if !self.record(
            root.to_owned(),
            Measurement {
                partial: true,
                ..Default::default()
            },
        ) {
            return;
        }
        while let Some(mut frame) = stack.pop() {
            if self.cancel.cancelled() || self.limited {
                frame.value.partial = true;
            } else if let Some(entry) = frame.entries.next() {
                let (path, metadata) =
                    match entry.and_then(|e| Ok((e.path(), fs::symlink_metadata(e.path())?))) {
                        Ok(v) => v,
                        Err(e) => {
                            let path = frame.path.clone();
                            self.error(&mut frame, &path, e);
                            stack.push(frame);
                            continue;
                        }
                    };
                self.stats.visited += 1;
                self.stats.path = path.clone();
                if directory_link(&metadata) {
                    frame.value.links += 1;
                    frame.value.bytes += u128::from(metadata.len());
                    frame.value.direct_bytes += u128::from(metadata.len());
                    self.update.bytes += u128::from(metadata.len());
                } else if metadata.is_file() {
                    frame.value.files += 1;
                    frame.value.bytes += u128::from(metadata.len());
                    frame.value.direct_bytes += u128::from(metadata.len());
                    self.update.files += 1;
                    self.update.bytes += u128::from(metadata.len());
                } else if metadata.is_dir() {
                    if self.count >= self.limit {
                        self.limited = true;
                        self.error(&mut frame, &path, "Directory analysis record limit reached");
                    } else {
                        self.count += 1;
                        frame.value.directories += 1;
                        let child = (|| {
                            let identity = (self.probe)(&path)?;
                            if !identity.same_volume(volume) {
                                return Ok(None);
                            }
                            if stack.len() + 1 >= self.depth {
                                return Err(io::Error::other("Directory nesting limit reached"));
                            }
                            if identity == frame.identity
                                || stack.iter().any(|f| f.identity == identity)
                            {
                                return Err(io::Error::other("Directory cycle detected"));
                            }
                            let entries = fs::read_dir(&path)?;
                            if (self.probe)(&path)? != identity {
                                return Err(io::Error::other("Directory changed while scanning"));
                            }
                            Ok(Some(Frame {
                                path: path.clone(),
                                identity,
                                entries,
                                value: Measurement::default(),
                            }))
                        })();
                        match child {
                            Ok(Some(child)) => {
                                if !self.record(
                                    path,
                                    Measurement {
                                        partial: true,
                                        ..Default::default()
                                    },
                                ) {
                                    return;
                                }
                                stack.push(frame);
                                stack.push(child);
                                continue;
                            }
                            Ok(None) => {
                                frame.value.excluded += 1;
                                frame.value.partial = true;
                                self.update.excluded += 1;
                                if !self.record(
                                    path,
                                    Measurement {
                                        excluded: 1,
                                        partial: true,
                                        ..Default::default()
                                    },
                                ) {
                                    return;
                                }
                            }
                            Err(e) => {
                                self.error(&mut frame, &path, &e);
                                if !self.record(
                                    path,
                                    Measurement {
                                        skipped: 1,
                                        partial: true,
                                        ..Default::default()
                                    },
                                ) {
                                    return;
                                }
                            }
                        }
                    }
                } else {
                    self.error(&mut frame, &path, "Special file omitted from analysis");
                }
                stack.push(frame);
                if self.last.elapsed() >= Duration::from_millis(100) && !self.flush() {
                    return;
                }
                continue;
            }
            match (self.probe)(&frame.path) {
                Ok(identity) if identity == frame.identity => {}
                _ => {
                    let path = frame.path.clone();
                    self.error(&mut frame, &path, "Directory changed while scanning");
                    if path == root {
                        self.update.root_changed = true;
                    }
                }
            }
            if let Some(parent) = stack.last_mut() {
                parent.value.add(&frame.value);
            }
            if !self.record(frame.path, frame.value) {
                return;
            }
        }
    }
}

pub(super) fn run(root: PathBuf, cancel: &Cancellation, sender: &SyncSender<Event>) {
    let mut scanner = Scanner {
        cancel,
        probe: entry_identity,
        emit: |u| send(sender, Event::Usage(u), cancel, true),
        limit: DIRECTORY_LIMIT,
        depth: DEPTH_LIMIT,
        stats: Stats::default(),
        update: Update::default(),
        count: 0,
        limited: false,
        last: Instant::now(),
    };
    scanner.scan(&root);
    scanner.flush();
    send(
        sender,
        Event::Done {
            stats: scanner.stats,
            cancelled: cancel.cancelled(),
            limited: scanner.limited,
        },
        cancel,
        true,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::HashMap,
        sync::atomic::{AtomicU64, Ordering},
    };
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> io::Result<Self> {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "dn-usage-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path)?;
            Ok(Self(path))
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn analyze(
        root: &Path,
        limit: usize,
        depth: usize,
        cancel: &Cancellation,
        probe: impl FnMut(&Path) -> io::Result<EntryIdentity>,
    ) -> (HashMap<PathBuf, Measurement>, Stats, bool, Update) {
        let mut values = HashMap::new();
        let mut last = Update::default();
        let (stats, limited) = {
            let mut scanner = Scanner {
                cancel,
                probe,
                emit: |update: Update| {
                    assert!(update.records.len() <= BATCH);
                    values.extend(update.records.iter().cloned());
                    last = update;
                    true
                },
                limit,
                depth,
                stats: Stats::default(),
                update: Update::default(),
                count: 0,
                limited: false,
                last: Instant::now(),
            };
            scanner.scan(root);
            scanner.flush();
            (scanner.stats, scanner.limited)
        };
        (values, stats, limited, last)
    }
    #[test]
    fn one_pass_totals_include_hidden_direct_files_empty_folders_and_hard_links() -> io::Result<()>
    {
        let t = Temp::new()?;
        fs::create_dir_all(t.0.join("Ёж/empty"))?;
        fs::create_dir(t.0.join(".hidden"))?;
        fs::write(t.0.join("direct"), b"123")?;
        fs::write(t.0.join("Ёж/file"), b"12345")?;
        fs::hard_link(t.0.join("Ёж/file"), t.0.join("Ёж/alias"))?;
        fs::write(t.0.join(".hidden/file"), b"12")?;
        let (values, stats, limited, update) =
            analyze(&t.0, 20, 512, &Cancellation::default(), entry_identity);
        assert!(!limited);
        assert_eq!(stats.visited, 7);
        assert_eq!(stats.skipped, 0);
        let root = &values[&t.0];
        assert_eq!(
            (root.bytes, root.direct_bytes, root.files, root.directories),
            (15, 3, 4, 3)
        );
        assert!(!root.partial);
        assert_eq!(values[&t.0.join("Ёж")].bytes, 10);
        assert_eq!(values[&t.0.join("Ёж/empty")], Measurement::default());
        assert_eq!((update.files, update.bytes), (4, 15));
        Ok(())
    }
    #[test]
    fn injected_mount_and_access_error_propagate_distinct_partial_totals() -> io::Result<()> {
        let t = Temp::new()?;
        for name in ["mount", "denied", "good"] {
            fs::create_dir(t.0.join(name))?;
            fs::write(t.0.join(name).join("data"), b"data")?;
        }
        let (values, stats, _, update) = analyze(&t.0, 20, 512, &Cancellation::default(), |path| {
            let id = entry_identity(path)?;
            if path == t.0.join("mount") {
                return Ok(id.on_other_volume());
            }
            if path == t.0.join("denied") {
                return Err(io::Error::from(io::ErrorKind::PermissionDenied));
            }
            Ok(id)
        });
        let root = &values[&t.0];
        assert_eq!((root.bytes, root.skipped, root.excluded), (4, 1, 1));
        assert!(root.partial);
        assert_eq!((stats.skipped, update.excluded), (1, 1));
        assert_eq!(values[&t.0.join("mount")].excluded, 1);
        assert!(!values.contains_key(&t.0.join("mount/data")));
        Ok(())
    }
    #[test]
    fn limits_cancellation_and_replaced_root_are_never_complete() -> io::Result<()> {
        let t = Temp::new()?;
        fs::create_dir_all(t.0.join("a/b/c"))?;
        let (v, _, limited, _) = analyze(&t.0, 2, 512, &Cancellation::default(), entry_identity);
        assert!(limited);
        assert!(v.len() <= 2);
        assert!(v[&t.0].partial);
        let (v, stats, _, _) = analyze(&t.0, 20, 2, &Cancellation::default(), entry_identity);
        assert!(v[&t.0].partial);
        assert_eq!(stats.skipped, 1);
        let cancel = Cancellation::default();
        cancel.cancel();
        let (v, _, _, _) = analyze(&t.0, 20, 512, &cancel, entry_identity);
        assert!(v[&t.0].partial);
        let mut probes = 0;
        let (v, stats, _, update) = analyze(&t.0, 20, 512, &Cancellation::default(), |p| {
            let id = entry_identity(p)?;
            if p == t.0 {
                probes += 1;
                if probes > 2 {
                    return Ok(id.on_other_volume());
                }
            }
            Ok(id)
        });
        assert!(v[&t.0].partial);
        assert!(update.root_changed);
        assert!(stats.skipped > 0);
        Ok(())
    }
    #[test]
    fn missing_and_file_roots_report_partial_errors() -> io::Result<()> {
        let t = Temp::new()?;
        fs::write(t.0.join("file"), b"data")?;
        for path in [t.0.join("missing"), t.0.join("file")] {
            let (v, stats, _, _) =
                analyze(&path, 20, 512, &Cancellation::default(), entry_identity);
            assert!(v[&path].partial);
            assert_eq!(stats.skipped, 1);
        }
        Ok(())
    }
    #[cfg(unix)]
    #[test]
    fn symlink_cycles_and_special_files_are_not_traversed_or_read() -> io::Result<()> {
        use std::os::unix::{fs::symlink, net::UnixListener};
        let t = Temp::new()?;
        fs::create_dir(t.0.join("folder"))?;
        symlink(&t.0, t.0.join("folder/cycle"))?;
        symlink("missing", t.0.join("broken"))?;
        let socket = UnixListener::bind(t.0.join("socket"))?;
        let link_bytes = fs::symlink_metadata(t.0.join("folder/cycle"))?.len()
            + fs::symlink_metadata(t.0.join("broken"))?.len();
        let (v, stats, _, _) = analyze(&t.0, 20, 512, &Cancellation::default(), entry_identity);
        assert_eq!(v[&t.0].bytes, u128::from(link_bytes));
        assert_eq!(v[&t.0].links, 2);
        assert_eq!(stats.visited, 4);
        assert_eq!(stats.skipped, 1);
        assert!(v[&t.0].partial);
        drop(socket);
        Ok(())
    }
    #[test]
    fn job_cancellation_delivers_partial_terminal_event() -> io::Result<()> {
        let t = Temp::new()?;
        for i in 0..600 {
            fs::create_dir(t.0.join(format!("d{i}")))?;
        }
        let job = super::super::Job::start(super::super::Request::Usage(t.0.clone()))?;
        job.stop();
        loop {
            match job
                .events
                .recv_timeout(Duration::from_secs(5))
                .expect("bounded cancellation")
            {
                Event::Done { cancelled, .. } => {
                    assert!(cancelled);
                    break;
                }
                Event::Usage(u) => assert!(u.records.len() <= BATCH),
                _ => panic!("unexpected usage event"),
            }
        }
        Ok(())
    }
}
