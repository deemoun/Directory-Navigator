//! Bounded background import. Results are applied atomically by the live owner.
use super::{playlist::Track, tags::Tags};
use std::{
    path::PathBuf,
    sync::{atomic::AtomicUsize, Arc},
    time::Instant,
};
#[derive(Default)]
pub(super) struct Progress {
    pub visited: AtomicUsize,
    pub found: AtomicUsize,
}
#[derive(Default)]
pub(super) struct Result {
    pub tracks: Vec<(PathBuf, Tags)>,
    pub errors: usize,
    pub limited: bool,
}
pub(super) struct Pending {
    pub task: Option<crate::task::Task<Result>>,
    pub progress: Arc<Progress>,
    pub wake: Instant,
    pub draft: bool,
    pub visited: usize,
    pub found: usize,
}
impl Pending {
    pub fn tracks(result: Result, next_id: &mut u64) -> impl Iterator<Item = Track> + '_ {
        result.tracks.into_iter().map(move |(path, tags)| {
            *next_id = next_id.wrapping_add(1);
            Track {
                id: *next_id,
                path,
                tags,
            }
        })
    }
}
#[cfg(target_os = "macos")]
pub(super) fn scan(
    paths: Vec<PathBuf>,
    capacity: usize,
    cancel: &crate::task::Cancellation,
    progress: &Progress,
) -> Result {
    use std::{fs, path::Path, sync::atomic::Ordering};
    const MAX_ENTRIES: usize = 10000;
    const MAX_DEPTH: usize = 64;
    struct Scanner<'a> {
        result: Result,
        capacity: usize,
        entries: usize,
        cancel: &'a crate::task::Cancellation,
        progress: &'a Progress,
    }
    impl Scanner<'_> {
        fn full(&mut self) -> bool {
            if self.result.tracks.len() >= self.capacity {
                self.result.limited = true;
                true
            } else {
                false
            }
        }
        fn visit(&mut self, path: &Path, depth: usize) {
            if self.cancel.cancelled() || self.full() {
                return;
            }
            self.progress.visited.fetch_add(1, Ordering::Relaxed);
            let metadata = match fs::symlink_metadata(path) {
                Ok(m) => m,
                Err(_) => {
                    self.result.errors += 1;
                    return;
                }
            };
            if metadata.is_symlink() {
                return;
            }
            if metadata.is_file() {
                if !path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("mp3"))
                {
                    return;
                }
                let read = (|| -> std::io::Result<_> {
                    let mut file = crate::filesystem::open_regular(path)?;
                    if !crate::platform_fs::same_identity(&metadata, &file.metadata()?) {
                        return Err(std::io::Error::other("File changed while importing"));
                    }
                    self.cancel.check()?;
                    let tags = super::tags::read(&mut file)?;
                    self.cancel.check()?;
                    Ok(tags)
                })();
                match read {
                    Ok(tags) => {
                        self.result.tracks.push((path.into(), tags));
                        self.progress.found.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(_) => self.result.errors += 1,
                }
                return;
            }
            if !metadata.is_dir() {
                return;
            }
            if self.entries >= MAX_ENTRIES {
                self.result.limited = true;
                return;
            }
            if depth >= MAX_DEPTH {
                self.result.limited = true;
                return;
            }
            let entries = (|| -> std::io::Result<_> {
                let handle = crate::platform_fs::open_directory(path)?;
                let identity = handle.metadata()?;
                if !crate::platform_fs::same_identity(&metadata, &identity)
                    || !crate::platform_fs::owns(path, &identity)
                {
                    return Err(std::io::Error::other("Directory changed while importing"));
                }
                let entries = fs::read_dir(path)?;
                if !crate::platform_fs::owns(path, &identity) {
                    return Err(std::io::Error::other("Directory changed while importing"));
                }
                Ok((handle, identity, entries))
            })();
            let Ok((_handle, identity, entries)) = entries else {
                self.result.errors += 1;
                return;
            };
            let mut children = Vec::new();
            for entry in entries {
                if self.cancel.cancelled() {
                    return;
                }
                if self.entries >= MAX_ENTRIES {
                    self.result.limited = true;
                    break;
                }
                self.entries += 1;
                match entry {
                    Ok(entry) => children.push(entry.path()),
                    Err(_) => self.result.errors += 1,
                }
            }
            children.sort();
            for child in children {
                if self.cancel.cancelled() || self.result.tracks.len() >= self.capacity {
                    self.result.limited |= self.result.tracks.len() >= self.capacity;
                    break;
                }
                if !crate::platform_fs::owns(path, &identity) {
                    self.result.errors += 1;
                    break;
                }
                self.visit(&child, depth + 1);
            }
        }
    }
    let mut scanner = Scanner {
        result: Result::default(),
        capacity: capacity.min(super::playlist::MAX_ORDER),
        entries: 0,
        cancel,
        progress,
    };
    for path in paths {
        if cancel.cancelled() || scanner.full() {
            break;
        }
        if scanner.entries >= MAX_ENTRIES {
            scanner.result.limited = true;
            break;
        }
        scanner.entries += 1;
        scanner.visit(&path, 0);
    }
    scanner.result
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use std::{
        fs, io,
        sync::atomic::{AtomicU64, Ordering},
    };
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> io::Result<Self> {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "dn-mp3-library-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path)?;
            Ok(Self(path))
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn recursive_import_is_sorted_bounded_and_never_follows_directory_or_file_links(
    ) -> io::Result<()> {
        use std::os::unix::fs::symlink;
        let f = Fixture::new()?;
        fs::create_dir(f.0.join("album"))?;
        fs::write(f.0.join("album/a.MP3"), b"no tags")?;
        fs::write(f.0.join("z.mp3"), b"no tags")?;
        fs::write(f.0.join("ignore.txt"), b"ignored")?;
        symlink(&f.0, f.0.join("album/loop"))?;
        symlink(f.0.join("z.mp3"), f.0.join("link.mp3"))?;
        let cancel = crate::task::Cancellation::default();
        let result = scan(vec![f.0.clone()], 64, &cancel, &Progress::default());
        assert_eq!(result.errors, 0);
        assert_eq!(
            result
                .tracks
                .iter()
                .map(|(p, _)| p.strip_prefix(&f.0).unwrap().to_path_buf())
                .collect::<Vec<_>>(),
            vec![PathBuf::from("album/a.MP3"), PathBuf::from("z.mp3")]
        );
        let result = scan(vec![f.0.clone()], 1, &cancel, &Progress::default());
        assert_eq!(result.tracks.len(), 1);
        assert!(result.limited);
        cancel.cancel();
        assert!(scan(vec![f.0.clone()], 64, &cancel, &Progress::default())
            .tracks
            .is_empty());
        Ok(())
    }
    #[test]
    fn missing_files_do_not_lose_valid_tracks_or_mutate_sources() -> io::Result<()> {
        let f = Fixture::new()?;
        let path = f.0.join("file.mp3");
        fs::write(&path, b"source")?;
        let result = scan(
            vec![f.0.join("missing.mp3"), path.clone()],
            64,
            &Default::default(),
            &Default::default(),
        );
        assert_eq!(result.errors, 1);
        assert_eq!(result.tracks.len(), 1);
        assert_eq!(fs::read(path)?, b"source");
        Ok(())
    }
    #[test]
    fn entry_and_depth_limits_stop_scanning_without_unbounded_work() -> io::Result<()> {
        let f = Fixture::new()?;
        let plain = f.0.join("plain.txt");
        fs::write(&plain, b"not audio")?;
        let progress = Progress::default();
        let result = scan(vec![plain; 10001], 64, &Default::default(), &progress);
        assert!(result.limited);
        assert!(result.tracks.is_empty());
        assert_eq!(progress.visited.load(Ordering::Relaxed), 10000);
        let mut deep = f.0.clone();
        for _ in 0..65 {
            deep.push("n");
        }
        fs::create_dir_all(&deep)?;
        fs::write(deep.join("too-deep.mp3"), b"not read")?;
        let result = scan(
            vec![f.0.clone()],
            64,
            &Default::default(),
            &Progress::default(),
        );
        assert!(result.limited);
        assert!(result.tracks.is_empty());
        Ok(())
    }
}
