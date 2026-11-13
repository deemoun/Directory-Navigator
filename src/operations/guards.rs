//! Detect changed ancestors before using queued paths. This is a conservative
//! check, not a substitute for descriptor-relative filesystem operations.
use std::{
    collections::HashMap,
    fs::{self, Metadata},
    io,
    path::{Path, PathBuf},
};

#[derive(Clone)]
struct DirectoryIdentity {
    #[cfg(not(windows))]
    metadata: Metadata,
    #[cfg(windows)]
    native: crate::platform_fs::EntryIdentity,
}
impl DirectoryIdentity {
    fn capture(path: &Path, metadata: Metadata) -> io::Result<Self> {
        #[cfg(windows)]
        {
            let _ = metadata;
            Ok(Self {
                native: crate::platform_fs::entry_identity(path)?,
            })
        }
        #[cfg(not(windows))]
        {
            let _ = path;
            Ok(Self { metadata })
        }
    }
    fn matches(&self, path: &Path, metadata: &Metadata) -> io::Result<bool> {
        #[cfg(windows)]
        {
            // NTFS can preserve/reuse creation times; they are not identities.
            Ok(metadata.is_dir()
                && !crate::platform_fs::directory_link(metadata)
                && self.native == crate::platform_fs::entry_identity(path)?)
        }
        #[cfg(not(windows))]
        {
            let _ = path;
            Ok(crate::platform_fs::same_identity(&self.metadata, metadata))
        }
    }
}

#[derive(Clone, Default)]
pub(super) struct DirectoryGuards(HashMap<PathBuf, DirectoryIdentity>);

impl DirectoryGuards {
    pub fn capture_parent(&mut self, path: &Path) -> io::Result<()> {
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::other("Missing parent directory"))?;
        let ancestors: Vec<_> = parent.ancestors().collect();
        for ancestor in ancestors.into_iter().rev() {
            self.capture(ancestor)?;
        }
        Ok(())
    }

    pub fn capture(&mut self, directory: &Path) -> io::Result<()> {
        self.verify_parent(directory)?;
        let metadata = fs::symlink_metadata(directory)?;
        if !metadata.is_dir() || crate::platform_fs::directory_link(&metadata) {
            return Err(io::Error::other(
                "Parent directory was replaced by another entry",
            ));
        }
        if let Some(original) = self.0.get(directory) {
            if !original.matches(directory, &metadata)? {
                return Err(io::Error::other(
                    "Parent directory changed during operation",
                ));
            }
        } else {
            self.0.insert(
                directory.to_owned(),
                DirectoryIdentity::capture(directory, metadata)?,
            );
        }
        Ok(())
    }

    pub fn verify_parent(&self, path: &Path) -> io::Result<()> {
        // Check from the root down, before touching a possibly redirected child.
        let ancestors: Vec<_> = path.ancestors().skip(1).collect();
        for parent in ancestors.into_iter().rev() {
            let original = self.0.get(parent).ok_or_else(|| {
                io::Error::other("Parent directory was not checked before operation")
            })?;
            let current = fs::symlink_metadata(parent)?;
            if !original.matches(parent, &current)? {
                return Err(io::Error::other(
                    "Parent directory changed during operation",
                ));
            }
        }
        Ok(())
    }

    pub fn verify_directory(&self, directory: &Path) -> io::Result<()> {
        self.verify_parent(directory)?;
        let original = self
            .0
            .get(directory)
            .ok_or_else(|| io::Error::other("Directory was not checked before operation"))?;
        if !original.matches(directory, &fs::symlink_metadata(directory)?)? {
            return Err(io::Error::other(
                "Parent directory changed during operation",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> io::Result<Self> {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "dn-directory-identity-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            fs::create_dir(&path)?;
            Ok(Self(fs::canonicalize(path)?))
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn replacement_directory_is_rejected_even_with_matching_windows_creation_time() -> io::Result<()>
    {
        let t = Fixture::new()?;
        let parent = t.0.join("parent");
        let child = parent.join("child");
        fs::create_dir(&parent)?;
        let original = crate::platform_fs::entry_identity(&parent)?;
        let mut guards = DirectoryGuards::default();
        guards.capture_parent(&child)?;
        #[cfg(windows)]
        let original_creation = fs::metadata(&parent)?.created()?;
        #[cfg(windows)]
        let creation = {
            use std::os::windows::io::AsRawHandle;
            use windows::Win32::{
                Foundation::HANDLE,
                Storage::FileSystem::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION},
            };
            let handle = crate::platform_fs::open_directory(&parent)?;
            let mut info = BY_HANDLE_FILE_INFORMATION::default();
            // SAFETY: handle is live and info is writable for the duration of the call.
            unsafe { GetFileInformationByHandle(HANDLE(handle.as_raw_handle()), &mut info) }
                .map_err(io::Error::other)?;
            info.ftCreationTime
        };
        fs::rename(&parent, t.0.join("held-original"))?;
        fs::create_dir(&parent)?;
        #[cfg(windows)]
        {
            use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
            use windows::Win32::{
                Foundation::HANDLE,
                Storage::FileSystem::{
                    SetFileTime, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
                    FILE_READ_ATTRIBUTES, FILE_WRITE_ATTRIBUTES,
                },
            };
            let replacement = fs::OpenOptions::new()
                .access_mode(FILE_WRITE_ATTRIBUTES.0 | FILE_READ_ATTRIBUTES.0)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0)
                .open(&parent)?;
            // SAFETY: replacement is live and creation is initialized. Force the
            // collision rather than relying on clock precision or NTFS tunneling.
            unsafe {
                SetFileTime(
                    HANDLE(replacement.as_raw_handle()),
                    Some(&creation),
                    None,
                    None,
                )
            }
            .map_err(io::Error::other)?;
            assert_eq!(original_creation, replacement.metadata()?.created()?);
        }
        assert_ne!(crate::platform_fs::entry_identity(&parent)?, original);
        assert!(guards.verify_parent(&child).is_err());
        assert!(guards.verify_directory(&parent).is_err());
        assert!(guards.capture(&parent).is_err());
        let mut refreshed = DirectoryGuards::default();
        refreshed.capture_parent(&child)?;
        refreshed.verify_directory(&parent)?;
        Ok(())
    }
}
