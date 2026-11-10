//! Session-owned analysis state; layout and drawing only inspect snapshots.
use crate::{
    localization::Language,
    traversal::{self, usage::Measurement, Event, Job, Request},
};
use std::{
    collections::HashMap,
    io,
    path::{Path, PathBuf},
    sync::mpsc::TryRecvError,
};

#[derive(Default)]
pub struct Usage {
    pub root: Option<PathBuf>,
    pub records: HashMap<PathBuf, Measurement>,
    pub size_order: bool,
    pub stale: bool,
    pub stats: traversal::Stats,
    pub files: u64,
    pub bytes: u128,
    pub excluded: u64,
    pub state: &'static str,
    job: Option<Job>,
    stopping: bool,
}
impl Usage {
    pub fn running(&self) -> bool {
        self.job.is_some()
    }
    pub fn stopping(&self) -> bool {
        self.stopping
    }
    pub fn start(&mut self, root: PathBuf, owner: crate::requests::Owner) -> io::Result<()> {
        self.job = None;
        let order = self.size_order;
        *self = Self {
            root: Some(root.clone()),
            size_order: order,
            state: "Analyzing",
            ..Self::default()
        };
        match Job::start(Request::Usage(root)) {
            Ok(mut job) => {
                job.bind(owner);
                self.job = Some(job);
                Ok(())
            }
            Err(e) => {
                self.state = "Failed — partial";
                self.stats.first_error = Some(e.to_string());
                Err(e)
            }
        }
    }
    pub fn stop(&mut self) {
        if let Some(job) = &self.job {
            job.stop();
            self.stopping = true;
            self.state = "Stopping — partial";
        }
    }
    pub fn invalidate(&mut self, path: &Path) {
        if self
            .root
            .as_ref()
            .is_some_and(|root| root.starts_with(path) || path.starts_with(root))
        {
            self.stale = true;
            self.stop();
        }
    }
    pub fn poll(&mut self) -> (bool, bool) {
        let mut changed = false;
        let mut finished = false;
        if let Some(job) = &self.job {
            for _ in 0..8 {
                match job.try_recv() {
                    Ok(Event::Usage(update)) => {
                        for (path, value) in update.records {
                            if self.records.contains_key(&path)
                                || self.records.len() < traversal::usage::DIRECTORY_LIMIT
                            {
                                self.records.insert(path, value);
                            }
                        }
                        self.files = update.files;
                        self.bytes = update.bytes;
                        self.excluded = update.excluded;
                        self.stale |= update.root_changed;
                        self.stats.skipped = update.skipped;
                        self.stats.first_error = update.first_error;
                        changed = true;
                    }
                    Ok(Event::Done {
                        stats,
                        cancelled,
                        limited,
                    }) => {
                        let stopped = cancelled || self.stopping;
                        if stopped {
                            if let Some(value) = self
                                .root
                                .as_ref()
                                .and_then(|root| self.records.get_mut(root))
                            {
                                value.partial = true;
                            }
                        }
                        self.state = if stopped {
                            "Stopped — partial"
                        } else if limited {
                            "Limited — partial"
                        } else if stats.skipped > 0 || self.excluded > 0 || self.stale {
                            "Complete — partial"
                        } else {
                            "Complete"
                        };
                        self.stats = stats;
                        changed = true;
                        finished = true;
                        break;
                    }
                    Ok(_) => {}
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        self.state = "Failed — partial";
                        self.stats.first_error = Some("Directory analysis worker stopped".into());
                        changed = true;
                        finished = true;
                        break;
                    }
                }
            }
        }
        if finished {
            self.job = None;
            self.stopping = false;
        }
        (changed, finished)
    }
    pub fn status(&self, language: Language) -> String {
        if self.root.is_none() {
            return language
                .text("Analyze: apparent sizes, including hidden files")
                .into();
        }
        format!(
            "{}{} · {} {} · {} {} · {} {}",
            language.text(self.state),
            if self.stale {
                format!(" · {}", language.text("Stale"))
            } else {
                String::new()
            },
            self.files,
            language.text("files"),
            self.stats.skipped,
            language.text("skipped"),
            self.excluded,
            language.text("excluded mounts")
        )
    }
    pub fn summary(&self, path: &Path, language: Language) -> String {
        match self.records.get(path) {
            Some(value) => format!(
                "{}{}: {} · {}: {}",
                language.text(if value.partial {
                    "Partial apparent size"
                } else {
                    "Apparent size"
                }),
                if self.stale {
                    format!(" ({})", language.text("Stale"))
                } else {
                    String::new()
                },
                bytes(value.bytes),
                language.text("Direct files/links"),
                bytes(value.direct_bytes)
            ),
            None => language.text("No size snapshot for this directory").into(),
        }
    }
}
pub fn bytes(value: u128) -> String {
    for (scale, unit) in [
        (1_u128 << 60, "EiB"),
        (1 << 50, "PiB"),
        (1 << 40, "TiB"),
        (1 << 30, "GiB"),
        (1 << 20, "MiB"),
        (1 << 10, "KiB"),
    ] {
        if value >= scale {
            return format!(
                "{}.{} {}",
                value / scale,
                (value % scale) * 10 / scale,
                unit
            );
        }
    }
    format!("{value} B")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    #[test]
    fn accepted_stop_is_not_overwritten_by_a_queued_successful_completion() {
        let root = std::env::temp_dir().join("dn-usage-queued-completion");
        let (job, sender) = Job::controlled();
        let mut usage = Usage {
            root: Some(root.clone()),
            job: Some(job),
            state: "Analyzing",
            ..Default::default()
        };
        usage.records.insert(
            root.clone(),
            Measurement {
                bytes: 123,
                ..Default::default()
            },
        );
        sender
            .send(Event::Done {
                stats: traversal::Stats::default(),
                cancelled: false,
                limited: false,
            })
            .unwrap();
        usage.stop();
        assert!(usage.stopping());
        assert_eq!(usage.poll(), (true, true));
        assert_eq!(usage.state, "Stopped — partial");
        assert!(usage.records[&root].partial);
        assert!(!usage.running());
        assert!(!usage.stopping());
    }
    #[test]
    fn superseded_jobs_never_deliver_into_new_snapshot() -> io::Result<()> {
        let root = std::env::temp_dir().join(format!("dn-usage-replace-{}", std::process::id()));
        std::fs::create_dir(&root)?;
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(root.clone());
        for n in ["a", "b"] {
            std::fs::create_dir(root.join(n))?;
            std::fs::write(root.join(n).join("f"), n)?;
        }
        let mut usage = Usage::default();
        let owner = crate::requests::Owner::Workspace;
        usage.start(root.join("a"), owner)?;
        usage.start(root.join("b"), owner)?;
        let end = Instant::now() + Duration::from_secs(5);
        while usage.running() {
            usage.poll();
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(!usage.records.contains_key(&root.join("a")));
        assert_eq!(usage.records[&root.join("b")].bytes, 1);
        usage.invalidate(&root.join("unrelated"));
        assert!(!usage.stale);
        usage.invalidate(&root);
        assert!(usage.stale);
        Ok(())
    }
}
