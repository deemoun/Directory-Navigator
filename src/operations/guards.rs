//! Detect changed ancestors before using queued paths. This is a conservative
//! check, not a substitute for descriptor-relative filesystem operations.
use std::{
    collections::HashMap,
    fs::{self, Metadata},
    io,
    path::{Path, PathBuf},
};

#[derive(Default)]
pub(super) struct DirectoryGuards(HashMap<PathBuf, Metadata>);

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
        if !metadata.is_dir() {
            return Err(io::Error::other(
                "Parent directory was replaced by another entry",
            ));
        }
        if let Some(original) = self.0.get(directory) {
            if !crate::platform_fs::same_identity(original, &metadata) {
                return Err(io::Error::other(
                    "Parent directory changed during operation",
                ));
            }
        } else {
            self.0.insert(directory.to_owned(), metadata);
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
            if !crate::platform_fs::same_identity(original, &current) {
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
        if !crate::platform_fs::same_identity(original, &fs::symlink_metadata(directory)?) {
            return Err(io::Error::other(
                "Parent directory changed during operation",
            ));
        }
        Ok(())
    }
}
