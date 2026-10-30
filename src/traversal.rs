//! Bounded, cancellable directory traversal shared by Find and size snapshots.
pub mod find;
use crate::{pattern::Wildcard, task::Cancellation};
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, SyncSender, TrySendError},
    thread,
    time::{Duration, Instant},
};
pub const RESULT_LIMIT: usize = 10_000;
#[derive(Clone, Debug)]
pub struct Hit {
    pub path: PathBuf,
    pub regular: bool,
    pub line: Option<usize>,
}
#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub visited: u64,
    pub skipped: u64,
    pub first_error: Option<String>,
    pub path: PathBuf,
}
impl Stats {
    fn error(&mut self, path: &Path, error: impl std::fmt::Display) {
        self.skipped += 1;
        if self.first_error.is_none() {
            self.first_error = Some(format!("{}: {error}", path.display()));
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct Size {
    pub path: PathBuf,
    pub bytes: u128,
    pub files: u64,
    pub directories: u64,
    pub links: u64,
    pub skipped: u64,
    pub partial: bool,
}
pub enum Request {
    Find {
        root: PathBuf,
        pattern: String,
        recursive: bool,
        hidden: bool,
    },
    AdvancedFind {
        root: PathBuf,
        pattern: String,
        recursive: bool,
        hidden: bool,
        options: find::Options,
    },
    Sizes(Vec<PathBuf>),
}
pub enum Event {
    Found(Vec<Hit>),
    Size(Size),
    Progress(Stats),
    Done {
        stats: Stats,
        cancelled: bool,
        limited: bool,
    },
}
pub struct Job {
    pub(crate) requests: crate::requests::RequestScope,
    token: crate::requests::RequestToken,
    pub events: Receiver<Event>,
    cancel: Cancellation,
}
impl Job {
    pub fn try_recv(&self) -> Result<Event, std::sync::mpsc::TryRecvError> {
        if !self.requests.accepts(self.token) {
            return Err(std::sync::mpsc::TryRecvError::Disconnected);
        }
        self.events.try_recv()
    }

    pub fn start(request: Request) -> io::Result<Self> {
        if let Request::AdvancedFind { options, .. } = &request {
            options.validate()?;
        }
        let requests = crate::requests::RequestScope::default();
        let token = requests.start("traversal");
        let (send, events) = mpsc::sync_channel(8);
        let cancel = Cancellation::default();
        let worker_cancel = cancel.clone();
        thread::Builder::new()
            .name("dn-traversal".into())
            .spawn(move || run(request, &worker_cancel, &send))?;
        Ok(Self {
            events,
            cancel,
            requests,
            token,
        })
    }
    pub fn bind(&mut self, owner: crate::requests::Owner) {
        self.requests.bind(owner);
        self.token = self.requests.start("traversal");
    }
    pub fn stop(&self) {
        self.cancel.cancel();
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        self.stop();
    }
}
fn send(
    sender: &SyncSender<Event>,
    mut event: Event,
    cancel: &Cancellation,
    terminal: bool,
) -> bool {
    loop {
        match sender.try_send(event) {
            Ok(()) => return true,
            Err(TrySendError::Disconnected(_)) => return false,
            Err(TrySendError::Full(value)) => {
                if !terminal && cancel.cancelled() {
                    return false;
                }
                event = value;
                thread::sleep(Duration::from_millis(5));
            }
        }
    }
}
/// Return false from visit to stop. Directory links are always leaves.
fn walk(
    root: &Path,
    recursive: bool,
    hidden: bool,
    cancel: &Cancellation,
    stats: &mut Stats,
    mut visit: impl FnMut(&Path, &fs::Metadata, &mut Stats) -> bool,
) {
    let mut stack = Vec::new();
    match fs::read_dir(root) {
        Ok(entries) => stack.push(entries),
        Err(e) => {
            stats.error(root, e);
            return;
        }
    }
    while !cancel.cancelled() {
        let Some(entries) = stack.last_mut() else {
            break;
        };
        let entry = match entries.next() {
            None => {
                stack.pop();
                continue;
            }
            Some(Err(e)) => {
                stats.error(root, e);
                continue;
            }
            Some(Ok(entry)) => entry,
        };
        if !hidden && entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let path = entry.path();
        stats.path = path.clone();
        stats.visited += 1;
        let metadata = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) => {
                stats.error(&path, e);
                continue;
            }
        };
        if !visit(&path, &metadata, stats) {
            break;
        }
        if recursive && metadata.is_dir() {
            match fs::symlink_metadata(&path) {
                Ok(current)
                    if current.is_dir()
                        && crate::platform_fs::same_identity(&metadata, &current) => {}
                Ok(_) => {
                    stats.error(&path, "Directory changed while scanning");
                    continue;
                }
                Err(e) => {
                    stats.error(&path, e);
                    continue;
                }
            }
            if stack.len() >= 512 {
                stats.error(&path, "Directory nesting limit reached");
                continue;
            }
            match fs::read_dir(&path) {
                Ok(entries) => stack.push(entries),
                Err(e) => stats.error(&path, e),
            }
        }
    }
}
fn run(request: Request, cancel: &Cancellation, sender: &SyncSender<Event>) {
    let mut stats = Stats::default();
    let mut limited = false;
    let mut last = Instant::now();
    let options = match &request {
        Request::AdvancedFind { options, .. } => options.clone(),
        _ => Default::default(),
    };
    match request {
        Request::Find {
            root,
            pattern,
            recursive,
            hidden,
        }
        | Request::AdvancedFind {
            root,
            pattern,
            recursive,
            hidden,
            ..
        } => {
            let matcher = Wildcard::new(&pattern);
            let mut batch = Vec::with_capacity(64);
            let mut count = 0;
            walk(
                &root,
                recursive,
                hidden,
                cancel,
                &mut stats,
                |path, metadata, stats| {
                    let mut line = None;
                    let mut matches = options.accepts(metadata)
                        && path.file_name().is_some_and(|name| matcher.matches(name));
                    if matches && !options.text.is_empty() {
                        matches = metadata.is_file();
                        if matches {
                            match options.content(path, metadata, cancel) {
                                Ok(found) => {
                                    line = found;
                                    matches = line.is_some();
                                }
                                Err(_) if cancel.cancelled() => return false,
                                Err(e) => {
                                    stats.error(path, e);
                                    matches = false;
                                }
                            }
                        }
                    }
                    if matches {
                        batch.push(Hit {
                            path: path.into(),
                            regular: metadata.is_file(),
                            line,
                        });
                        count += 1;
                        if batch.len() == 64
                            && !send(
                                sender,
                                Event::Found(std::mem::replace(&mut batch, Vec::with_capacity(64))),
                                cancel,
                                false,
                            )
                        {
                            return false;
                        }
                        if count == RESULT_LIMIT {
                            limited = true;
                            return false;
                        }
                    }
                    if last.elapsed() >= Duration::from_millis(100) {
                        last = Instant::now();
                        if !send(sender, Event::Progress(stats.clone()), cancel, false) {
                            return false;
                        }
                    }
                    true
                },
            );
            if !batch.is_empty() && !send(sender, Event::Found(batch), cancel, true) {
                return;
            }
        }
        Request::Sizes(paths) => {
            for path in paths {
                if cancel.cancelled() {
                    break;
                }
                let skipped = stats.skipped;
                let mut size = Size {
                    path: path.clone(),
                    ..Size::default()
                };
                match fs::symlink_metadata(&path) {
                    Ok(m) if m.is_dir() => {
                        walk(&path, true, true, cancel, &mut stats, |entry, m, stats| {
                            if m.is_symlink() {
                                size.links += 1;
                                size.bytes += u128::from(m.len());
                            } else if m.is_file() {
                                size.files += 1;
                                size.bytes += u128::from(m.len());
                            } else if m.is_dir() {
                                size.directories += 1;
                            }
                            if last.elapsed() >= Duration::from_millis(100) {
                                last = Instant::now();
                                return send(sender, Event::Progress(stats.clone()), cancel, false);
                            }
                            let _ = entry;
                            true
                        })
                    }
                    Ok(_) => stats.error(&path, "Entry is no longer a directory"),
                    Err(e) => stats.error(&path, e),
                }
                size.skipped = stats.skipped - skipped;
                size.partial = cancel.cancelled() || size.skipped > 0;
                if !send(sender, Event::Size(size), cancel, true) {
                    return;
                }
            }
        }
    }
    send(
        sender,
        Event::Done {
            stats,
            cancelled: cancel.cancelled(),
            limited,
        },
        cancel,
        true,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disappearing_directory_is_reported_without_aborting_scan() -> io::Result<()> {
        let root = std::env::temp_dir().join(format!("dn-walk-vanish-{}", std::process::id()));
        fs::create_dir(&root)?;
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(root.clone());
        let gone = root.join("gone");
        fs::create_dir(&gone)?;
        fs::write(root.join("survives"), "")?;
        let mut stats = Stats::default();
        let mut visited = Vec::new();
        walk(
            &root,
            true,
            true,
            &Cancellation::default(),
            &mut stats,
            |path, _, _| {
                visited.push(path.to_owned());
                if path == gone {
                    fs::remove_dir(path).unwrap();
                }
                true
            },
        );
        assert_eq!(stats.skipped, 1);
        assert!(stats.first_error.is_some());
        assert!(visited.contains(&root.join("survives")));
        Ok(())
    }
}
