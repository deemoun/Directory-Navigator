//! Bounded portable offers. Native source identities never cross the wire.
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, Metadata, OpenOptions},
    io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
use unicode_normalization::UnicodeNormalization;
pub const ENTRY_LIMIT: usize = 100_000;
pub const BYTE_LIMIT: u64 = 64 * 1024 * 1024 * 1024;
pub const MANIFEST_LIMIT: usize = 16 * 1024 * 1024;
pub const DEPTH_LIMIT: usize = 512;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub path: String,
    pub directory: bool,
    pub size: u64,
    pub modified: Option<i64>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub entries: Vec<Entry>,
    pub total: u64,
}
#[derive(Clone)]
pub struct Source {
    pub path: PathBuf,
    identity: crate::platform_fs::EntryIdentity,
    metadata: Metadata,
}
#[derive(Clone)]
pub struct Selection {
    pub manifest: Manifest,
    sources: Vec<Source>,
    guards: BTreeMap<PathBuf, crate::platform_fs::EntryIdentity>,
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
pub fn relative(value: &str) -> io::Result<PathBuf> {
    if value.is_empty() || value.len() > 4096 || value.split('/').count() > DEPTH_LIMIT {
        return Err(invalid("Peer path exceeds limits"));
    }
    let mut path = PathBuf::new();
    for part in value.split('/') {
        path.push(crate::archive::extract::component(part.as_bytes())?);
    }
    Ok(path)
}
impl Manifest {
    pub fn validate(&self) -> io::Result<()> {
        if self.entries.is_empty() || self.entries.len() > ENTRY_LIMIT || self.total > BYTE_LIMIT {
            return Err(invalid("Peer offer exceeds transfer limits"));
        }
        let mut paths = BTreeMap::new();
        let mut total = 0_u64;
        let mut budget = 0_usize;
        for entry in &self.entries {
            relative(&entry.path)?;
            budget = budget
                .checked_add(entry.path.len() + 96)
                .ok_or_else(|| invalid("Peer manifest exceeds limits"))?;
            if budget > MANIFEST_LIMIT {
                return Err(invalid("Peer manifest exceeds 16 MiB"));
            }
            let key = entry
                .path
                .nfc()
                .flat_map(char::to_lowercase)
                .nfc()
                .collect::<String>();
            if let Some((parent, _)) = key.rsplit_once('/') {
                if paths.get(parent) != Some(&true) {
                    return Err(invalid(
                        "Peer manifest omits or reorders a parent directory",
                    ));
                }
            }
            if paths.insert(key, entry.directory).is_some() {
                return Err(invalid("Peer output names collide"));
            }
            if entry.directory && entry.size != 0 {
                return Err(invalid("Peer directory has a payload"));
            }
            total = total
                .checked_add(entry.size)
                .ok_or_else(|| invalid("Peer size overflow"))?;
        }
        if total != self.total || total > BYTE_LIMIT {
            return Err(invalid("Peer offer size does not match its entries"));
        }
        Ok(())
    }
}
impl Selection {
    pub fn capture(paths: Vec<PathBuf>) -> io::Result<Self> {
        Self::capture_cancellable(paths, &crate::task::Cancellation::default())
    }
    pub fn capture_cancellable(
        paths: Vec<PathBuf>,
        cancel: &crate::task::Cancellation,
    ) -> io::Result<Self> {
        if paths.len() > ENTRY_LIMIT {
            return Err(invalid("Peer selection exceeds entry limit"));
        }
        if paths.is_empty() {
            return Err(invalid("Select real files or directories first"));
        }
        let mut sources = Vec::new();
        for path in paths {
            cancel.check()?;
            let path = std::path::absolute(path)?;
            let name = path
                .file_name()
                .ok_or_else(|| invalid("Select an entry inside a directory"))?;
            let parent = fs::canonicalize(
                path.parent()
                    .ok_or_else(|| invalid("Missing source parent"))?,
            )?;
            sources.push(Source::capture(parent.join(name))?);
        }
        let mut guards = BTreeMap::new();
        for source in &sources {
            cancel.check()?;
            capture_guards(&mut guards, &source.path)?;
        }
        Ok(Self {
            manifest: Manifest {
                entries: vec![],
                total: 0,
            },
            sources,
            guards,
        })
    }
    pub fn prepare(&self, cancel: &AtomicBool) -> io::Result<Self> {
        let mut stack = Vec::new();
        for source in self.sources.iter().rev() {
            self.verify_parents(&source.path)?;
            source.verify()?;
            let name = source
                .path
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or_else(|| invalid("Nearby transfers require portable Unicode names"))?;
            relative(name)?;
            stack.push((source.path.clone(), name.to_owned(), source.clone()));
        }
        let mut result = Self {
            manifest: Manifest {
                entries: vec![],
                total: 0,
            },
            sources: vec![],
            guards: self.guards.clone(),
        };
        let mut seen = BTreeSet::new();
        let mut budget = 0;
        while let Some((path, relative_path, expected)) = stack.pop() {
            if cancel.load(Ordering::Relaxed) {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "Transfer cancelled",
                ));
            }
            capture_guards(&mut result.guards, &path)?;
            expected.verify()?;
            relative(&relative_path)?;
            if result.sources.len() >= ENTRY_LIMIT || !seen.insert(relative_path.to_lowercase()) {
                return Err(invalid(
                    "Peer selection exceeds limits or contains colliding names",
                ));
            }
            budget += relative_path.len() + 96;
            if budget > MANIFEST_LIMIT {
                return Err(invalid("Peer manifest exceeds 16 MiB"));
            }
            let m = &expected.metadata;
            let size = if m.is_dir() { 0 } else { m.len() };
            result.manifest.total = result
                .manifest
                .total
                .checked_add(size)
                .filter(|n| *n <= BYTE_LIMIT)
                .ok_or_else(|| invalid("Peer selection exceeds 64 GiB"))?;
            result.manifest.entries.push(Entry {
                path: relative_path.clone(),
                directory: m.is_dir(),
                size,
                modified: m.modified().ok().and_then(seconds),
            });
            let directory = m.is_dir();
            let expected_id = expected.identity;
            result.sources.push(expected);
            if directory {
                result.guards.insert(path.clone(), expected_id);
                let mut children = Vec::new();
                for child in fs::read_dir(&path)? {
                    if children.len() + stack.len() + result.sources.len() >= ENTRY_LIMIT {
                        return Err(invalid("Peer selection exceeds entry limit"));
                    }
                    let child = child?;
                    let name = child
                        .file_name()
                        .into_string()
                        .map_err(|_| invalid("Nearby transfers require portable Unicode names"))?;
                    relative(&name)?;
                    let source = Source::capture(child.path())?;
                    children.push((child.path(), format!("{relative_path}/{name}"), source));
                }
                children.sort_by(|a, b| b.1.cmp(&a.1));
                stack.extend(children);
            }
        }
        result.manifest.validate()?;
        Ok(result)
    }
    pub fn open(&self, index: usize) -> io::Result<File> {
        let source = self
            .sources
            .get(index)
            .ok_or_else(|| invalid("Unknown peer member"))?;
        self.verify_parents(&source.path)?;
        source.verify()?;
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.custom_flags(0x00200000);
        }
        let file = options.open(&source.path)?;
        if !file.metadata()?.is_file() || !crate::platform_fs::owns_file(&source.path, &file) {
            return Err(invalid("Peer source changed while opening"));
        }
        source.verify()?;
        Ok(file)
    }
    fn verify_parents(&self, path: &Path) -> io::Result<()> {
        for parent in path.ancestors().skip(1) {
            let expected = self
                .guards
                .get(parent)
                .ok_or_else(|| invalid("Peer source parent was not captured"))?;
            if fs::symlink_metadata(parent)?.is_symlink()
                || crate::platform_fs::entry_identity(parent)? != *expected
            {
                return Err(invalid("Peer source parent changed"));
            }
        }
        Ok(())
    }
    pub fn verify(&self, index: usize) -> io::Result<()> {
        if let Some(source) = self.sources.get(index) {
            self.verify_parents(&source.path)?;
        }
        self.sources
            .get(index)
            .ok_or_else(|| invalid("Unknown peer member"))?
            .verify()
    }
}
impl Source {
    fn capture(path: PathBuf) -> io::Result<Self> {
        // Check every ancestor: file selections cannot escape through a directory link.
        for ancestor in path.ancestors() {
            let m = fs::symlink_metadata(ancestor)?;
            if crate::platform_fs::directory_link(&m) {
                return Err(invalid("Nearby transfers do not follow links"));
            }
        }
        let metadata = fs::symlink_metadata(&path)?;
        if !(metadata.is_file() || metadata.is_dir()) {
            return Err(invalid("Nearby transfers do not support special files"));
        }
        Ok(Self {
            identity: crate::platform_fs::entry_identity(&path)?,
            path,
            metadata,
        })
    }
    fn verify(&self) -> io::Result<()> {
        let current = Self::capture(self.path.clone())?;
        if current.identity != self.identity
            || current.metadata.is_dir() != self.metadata.is_dir()
            || current.metadata.len() != self.metadata.len()
            || current.metadata.modified().ok() != self.metadata.modified().ok()
        {
            return Err(invalid("Peer source changed; select the files again"));
        }
        Ok(())
    }
}
pub fn seconds(time: SystemTime) -> Option<i64> {
    match time.duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_secs()).ok(),
        Err(e) => i64::try_from(e.duration().as_secs()).ok().map(|s| -s),
    }
}
pub fn timestamp(seconds: i64) -> Option<SystemTime> {
    let duration = std::time::Duration::from_secs(seconds.unsigned_abs());
    if seconds >= 0 {
        UNIX_EPOCH.checked_add(duration)
    } else {
        UNIX_EPOCH.checked_sub(duration)
    }
}

fn capture_guards(
    guards: &mut BTreeMap<PathBuf, crate::platform_fs::EntryIdentity>,
    path: &Path,
) -> io::Result<()> {
    for parent in path
        .ancestors()
        .skip(1)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        let m = fs::symlink_metadata(parent)?;
        if !m.is_dir() || crate::platform_fs::directory_link(&m) {
            return Err(invalid("Peer source parent is not a real directory"));
        }
        let id = crate::platform_fs::entry_identity(parent)?;
        if let Some(original) = guards.get(parent) {
            if *original != id {
                return Err(invalid("Peer source parent changed"));
            }
        } else {
            guards.insert(parent.to_owned(), id);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manifest_rejects_unsafe_paths_collisions_order_and_limits() {
        let file = |path: &str| Entry {
            path: path.into(),
            directory: false,
            size: 1,
            modified: None,
        };
        for path in [
            "../escape",
            "/absolute",
            "C:/drive",
            "a\\b",
            "a//b",
            "a/./b",
            "NUL",
            "trailing.",
            "bad\nname",
        ] {
            assert!(
                Manifest {
                    entries: vec![file(path)],
                    total: 1
                }
                .validate()
                .is_err(),
                "{path}"
            );
        }
        assert!(Manifest {
            entries: vec![file("a"), file("A")],
            total: 2
        }
        .validate()
        .is_err());
        assert!(Manifest {
            entries: vec![file("é.txt"), file("e\u{301}.txt")],
            total: 2
        }
        .validate()
        .is_err());
        assert!(Manifest {
            entries: vec![file("missing/file")],
            total: 1
        }
        .validate()
        .is_err());
        assert!(Manifest {
            entries: vec![file("a")],
            total: 2
        }
        .validate()
        .is_err());
        assert!(Manifest {
            entries: vec![Entry {
                size: BYTE_LIMIT + 1,
                ..file("a")
            }],
            total: BYTE_LIMIT + 1
        }
        .validate()
        .is_err());
        let manifest = Manifest {
            entries: vec![
                Entry {
                    path: "Ёж世界".into(),
                    directory: true,
                    size: 0,
                    modified: None,
                },
                file("Ёж世界/e\u{301}.txt"),
            ],
            total: 1,
        };
        assert!(manifest.validate().is_ok());
        assert_eq!(
            serde_json::from_slice::<Manifest>(&serde_json::to_vec(&manifest).unwrap()).unwrap(),
            manifest
        );
    }
    #[test]
    fn source_change_parent_replacement_links_and_cancellation_are_rejected() -> io::Result<()> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "dn-peer-manifest-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path)?;
        struct Temp(PathBuf);
        impl Drop for Temp {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _temp = Temp(path.clone());
        let root = fs::canonicalize(&path)?;
        let source = root.join("source");
        fs::create_dir(&source)?;
        fs::write(source.join("file"), b"original")?;
        fs::write(source.join(".hidden"), b"hidden")?;
        let selection = Selection::capture(vec![source.clone()])?;
        assert!(selection.prepare(&AtomicBool::new(true)).is_err());
        let prepared = selection.prepare(&AtomicBool::new(false))?;
        assert!(prepared
            .manifest
            .entries
            .iter()
            .any(|e| e.path == "source/.hidden"));
        let index = prepared
            .manifest
            .entries
            .iter()
            .position(|e| e.path == "source/file")
            .unwrap();
        assert_eq!(prepared.open(index)?.metadata()?.len(), 8);
        fs::write(source.join("file"), b"changed length")?;
        assert!(prepared.open(index).is_err());
        fs::rename(&source, root.join("old"))?;
        fs::create_dir(&source)?;
        fs::hard_link(root.join("old/file"), source.join("file"))?;
        assert!(prepared.verify(index).is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("old/file"), root.join("link"))?;
            assert!(Selection::capture(vec![root.join("link")]).is_err());
        }
        Ok(())
    }
    #[test]
    fn timestamps_handle_extremes_without_panics() {
        for time in [i64::MIN, -1, 0, 1, i64::MAX] {
            let _ = timestamp(time);
        }
        assert_eq!(seconds(timestamp(-1).unwrap()), Some(-1));
    }
}
