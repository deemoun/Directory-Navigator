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
                permissions.set_readonly(false);
                let _ = self.file.set_permissions(permissions);
            }
            let _ = fs::remove_file(&self.path);
        }
    }
}
