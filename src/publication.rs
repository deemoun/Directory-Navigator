//! A newly created sibling owns its cleanup; final names appear only when complete.
use std::{
    fs::{self, File, Metadata, OpenOptions},
    io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
pub struct StagedFile {
    pub file: File,
    path: PathBuf,
    identity: Metadata,
}
impl StagedFile {
    pub fn create(destination: &Path) -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let parent = destination
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        for _ in 0..100 {
            let path = parent.join(format!(
                ".dn-part-{}-{}.tmp",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let mut options = OpenOptions::new();
            options.read(true).write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&path) {
                Ok(file) => {
                    let identity = file.metadata()?;
                    return Ok(Self {
                        file,
                        path,
                        identity,
                    });
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::other("Cannot create temporary output"))
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn publish(&self, destination: &Path) -> io::Result<()> {
        self.file.sync_all()?;
        if !crate::platform_fs::owns(&self.path, &self.identity) {
            return Err(io::Error::other("Temporary output was replaced"));
        }
        match crate::platform_fs::rename_no_replace(&self.path, destination) {
            Ok(()) => {}
            Err(e) if crate::platform_fs::can_copy_fallback(&e) => {
                fs::hard_link(&self.path, destination)?;
            }
            Err(e) => return Err(e),
        }
        crate::platform_fs::sync_parent(destination)
    }
}
impl Drop for StagedFile {
    fn drop(&mut self) {
        if crate::platform_fs::owns(&self.path, &self.identity) {
            #[cfg(windows)]
            if let Ok(metadata) = self.file.metadata() {
                let mut permissions = metadata.permissions();
                // Windows needs the read-only attribute cleared to delete our
                // abandoned staging file. This code cannot run on Unix, where
                // set_readonly(false) would broaden filesystem permissions.
                #[allow(clippy::permissions_set_readonly_false)]
                permissions.set_readonly(false);
                let _ = self.file.set_permissions(permissions);
            }
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> io::Result<Self> {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let p = std::env::temp_dir().join(format!(
                "dn-publication-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&p)?;
            Ok(Self(p))
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn abandoned_staging_removes_only_its_own_partial_output() -> io::Result<()> {
        let t = Temp::new()?;
        let target = t.0.join("target");
        fs::write(&target, "existing")?;
        let staging = {
            let mut stage = StagedFile::create(&target)?;
            stage.file.write_all(b"partial")?;
            stage.path().to_owned()
        };
        assert!(!staging.exists());
        assert_eq!(fs::read_to_string(&target)?, "existing");
        assert_eq!(fs::read_dir(&t.0)?.count(), 1);
        Ok(())
    }
    #[test]
    fn publication_refuses_a_destination_created_after_staging() -> io::Result<()> {
        let t = Temp::new()?;
        let target = t.0.join("target");
        let mut stage = StagedFile::create(&target)?;
        stage.file.write_all(b"new")?;
        fs::write(&target, "winner")?;
        assert!(stage.publish(&target).is_err());
        drop(stage);
        assert_eq!(fs::read_to_string(&target)?, "winner");
        assert_eq!(fs::read_dir(&t.0)?.count(), 1);
        Ok(())
    }
    #[test]
    fn successful_publication_survives_staging_drop() -> io::Result<()> {
        let t = Temp::new()?;
        let target = t.0.join("target");
        let mut stage = StagedFile::create(&target)?;
        stage.file.write_all(b"complete")?;
        stage.publish(&target)?;
        drop(stage);
        assert_eq!(fs::read(&target)?, b"complete");
        assert_eq!(fs::read_dir(&t.0)?.count(), 1);
        Ok(())
    }
    #[cfg(windows)]
    #[test]
    fn abandoned_readonly_staging_is_removed_without_touching_destination() -> io::Result<()> {
        let t = Temp::new()?;
        let target = t.0.join("target");
        fs::write(&target, b"existing")?;
        let mut stage = StagedFile::create(&target)?;
        stage.file.write_all(b"partial")?;
        let path = stage.path().to_owned();
        let mut permissions = stage.file.metadata()?.permissions();
        permissions.set_readonly(true);
        stage.file.set_permissions(permissions)?;
        drop(stage);
        assert!(!path.exists());
        assert_eq!(fs::read(&target)?, b"existing");
        assert!(!fs::metadata(&target)?.permissions().readonly());
        assert_eq!(fs::read_dir(&t.0)?.count(), 1);
        Ok(())
    }
    #[cfg(unix)]
    #[test]
    fn replaced_staging_cannot_be_published_or_deleted_by_the_old_owner() -> io::Result<()> {
        let t = Temp::new()?;
        let target = t.0.join("target");
        let stage = StagedFile::create(&target)?;
        let path = stage.path().to_owned();
        fs::rename(&path, t.0.join("held-original"))?;
        fs::write(&path, "replacement")?;
        assert!(stage.publish(&target).is_err());
        drop(stage);
        assert_eq!(fs::read_to_string(&path)?, "replacement");
        assert!(!target.exists());
        Ok(())
    }
    #[cfg(unix)]
    #[test]
    fn dangling_symlink_destination_is_never_overwritten() -> io::Result<()> {
        let t = Temp::new()?;
        let target = t.0.join("link");
        std::os::unix::fs::symlink("missing", &target)?;
        let mut stage = StagedFile::create(&target)?;
        stage.file.write_all(b"new")?;
        assert!(stage.publish(&target).is_err());
        drop(stage);
        assert_eq!(fs::read_link(&target)?, PathBuf::from("missing"));
        assert!(!t.0.join("missing").exists());
        Ok(())
    }
}
