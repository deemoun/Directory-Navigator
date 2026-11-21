//! Private native working copies with identity-owned cleanup.
use std::{
    fs, io,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
pub struct Workspace {
    pub root: PathBuf,
    identity: crate::platform_fs::EntryIdentity,
    preserved: bool,
}
impl Workspace {
    pub fn create(parent: PathBuf) -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        for _ in 0..100 {
            let root = parent.join(format!(
                ".dn-remote-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let builder = &mut fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&root) {
                Ok(()) => {
                    let root = fs::canonicalize(root)?;
                    let identity = crate::platform_fs::entry_identity(&root)?;
                    return Ok(Self {
                        root,
                        identity,
                        preserved: false,
                    });
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::other("Cannot create remote working directory"))
    }
    /// Keep resources needed by an external process that outlives DN.
    pub(crate) fn preserve(mut self) {
        self.preserved = true;
    }
    pub fn validate(&self) -> io::Result<()> {
        if crate::platform_fs::entry_identity(&self.root)? != self.identity {
            return Err(io::Error::other("Remote workspace changed"));
        }
        Ok(())
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        if !self.preserved && self.validate().is_ok() {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}
