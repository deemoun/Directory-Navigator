//! Transfer-only fallback for filesystems without exclusive rename or hard links.
//! Existing files stay protected by create_new. A crash can leave a partial new
//! destination; normal errors/cancellation remove only output we still own.
use super::*;
use std::io::Seek;

pub(super) fn publish(
    stage: &crate::publication::StagedFile,
    target: &Path,
    capability: capabilities::Capability,
    cancel: &AtomicBool,
    progress: impl FnMut(u64, Phase) -> io::Result<()>,
) -> io::Result<bool> {
    publish_using(stage, target, cancel, progress, || {
        if capability == capabilities::Capability::HardLink {
            if !crate::platform_fs::owns_file(stage.path(), &stage.file) {
                return Err(io::Error::other("Temporary output was replaced"));
            }
            fs::hard_link(stage.path(), target)
        } else {
            stage.publish_ready(target)
        }
    })
}
fn publish_using(
    stage: &crate::publication::StagedFile,
    target: &Path,
    cancel: &AtomicBool,
    progress: impl FnMut(u64, Phase) -> io::Result<()>,
    atomic: impl FnOnce() -> io::Result<()>,
) -> io::Result<bool> {
    match atomic() {
        Err(error) if unavailable(&error) => {
            copy_exclusive(stage, target, cancel, progress).map(|_| true)
        }
        Ok(()) => crate::platform_fs::sync_parent(target).map(|_| false),
        Err(error) => Err(error),
    }
}
pub(super) fn unavailable(error: &io::Error) -> bool {
    crate::platform_fs::unsupported_operation(error)
}
pub(super) struct Output {
    pub file: fs::File,
    path: PathBuf,
    committed: bool,
}
impl Output {
    pub fn create(target: &Path) -> io::Result<Self> {
        let mut options = fs::OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        Ok(Self {
            file: options.open(target)?,
            path: target.to_owned(),
            committed: false,
        })
    }
    pub fn commit(&mut self) -> io::Result<()> {
        self.commit_using(crate::platform_fs::sync_parent)
    }
    fn commit_using(
        &mut self,
        sync_parent: impl FnOnce(&Path) -> io::Result<()>,
    ) -> io::Result<()> {
        if !crate::platform_fs::owns_file(&self.path, &self.file) {
            return Err(io::Error::other("Copy output changed during publication"));
        }
        self.committed = true;
        sync_parent(&self.path)
    }
}
pub(super) enum Destination {
    Staged(crate::publication::StagedFile),
    Direct(Output),
}
impl Destination {
    pub fn create(target: &Path, direct: bool) -> io::Result<Self> {
        if direct {
            Ok(Self::Direct(Output::create(target)?))
        } else {
            Ok(Self::Staged(crate::publication::StagedFile::create(
                target,
            )?))
        }
    }
    pub fn file(&mut self) -> &mut fs::File {
        match self {
            Self::Staged(stage) => &mut stage.file,
            Self::Direct(output) => &mut output.file,
        }
    }
}
impl Drop for Output {
    fn drop(&mut self) {
        if !self.committed && crate::platform_fs::owns_file(&self.path, &self.file) {
            #[cfg(windows)]
            if let Ok(metadata) = self.file.metadata() {
                let mut permissions = metadata.permissions();
                #[allow(clippy::permissions_set_readonly_false)]
                permissions.set_readonly(false);
                let _ = self.file.set_permissions(permissions);
            }
            let _ = fs::remove_file(&self.path);
        }
    }
}
fn copy_exclusive(
    stage: &crate::publication::StagedFile,
    target: &Path,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64, Phase) -> io::Result<()>,
) -> io::Result<()> {
    check(cancel)?;
    progress(0, Phase::Copying)?;
    if !crate::platform_fs::owns_file(stage.path(), &stage.file) {
        return Err(io::Error::other("Temporary output was replaced"));
    }
    let metadata = stage.file.metadata()?;
    let mut input = stage.file.try_clone()?;
    input.rewind()?;
    let mut output = Output::create(target)?;
    let mut buffer = vec![0; 128 * 1024];
    let mut remaining = metadata.len();
    while remaining > 0 {
        check(cancel)?;
        progress(0, Phase::Copying)?;
        let count = remaining.min(buffer.len() as u64) as usize;
        input.read_exact(&mut buffer[..count])?;
        output.file.write_all(&buffer[..count])?;
        remaining -= count as u64;
        progress(count as u64, Phase::Copying)?;
    }
    progress(0, Phase::Copying)?;
    if !crate::platform_fs::owns_file(stage.path(), &stage.file)
        || !crate::platform_fs::owns_file(target, &output.file)
    {
        return Err(io::Error::other("Copy output changed during publication"));
    }
    crate::platform_fs::copy_extended_metadata(&stage.file, &output.file)?;
    crate::platform_fs::copy_basic_metadata(&output.file, &metadata)?;
    output.file.sync_all()?;
    progress(0, Phase::Verifying)?;
    copy_options::verify(&mut input, &mut output.file, metadata.len(), cancel)?;
    check(cancel)?;
    progress(0, Phase::Verifying)?;
    if !crate::platform_fs::owns_file(target, &output.file) {
        return Err(io::Error::other("Copy output changed during publication"));
    }
    output.commit()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::publication::StagedFile;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> io::Result<Self> {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "dn-compatible-copy-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root)?;
            Ok(Self(root))
        }
        fn stage(&self, bytes: &[u8]) -> io::Result<(StagedFile, PathBuf)> {
            let target = self.0.join("result");
            let mut stage = StagedFile::create(&target)?;
            stage.file.write_all(bytes)?;
            Ok((stage, target))
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn unsupported() -> io::Result<()> {
        #[cfg(unix)]
        return Err(io::Error::from_raw_os_error(libc::ENOTSUP));
        #[cfg(not(unix))]
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "exclusive publication unsupported",
        ))
    }
    #[test]
    fn unsupported_publication_copies_and_verifies_empty_and_multi_chunk_files() -> io::Result<()> {
        for bytes in [Vec::new(), vec![0xA5; 400_001]] {
            let t = Fixture::new()?;
            let (stage, target) = t.stage(&bytes)?;
            let mut written = 0;
            publish_using(
                &stage,
                &target,
                &AtomicBool::new(false),
                |n, _| {
                    written += n;
                    Ok(())
                },
                unsupported,
            )?;
            assert_eq!(written, bytes.len() as u64);
            assert_eq!(fs::read(&target)?, bytes);
            assert!(stage.path().exists());
            drop(stage);
            assert_eq!(fs::read_dir(&t.0)?.count(), 1);
        }
        Ok(())
    }
    #[test]
    fn fallback_never_replaces_an_existing_file_or_dangling_link() -> io::Result<()> {
        let t = Fixture::new()?;
        let (stage, target) = t.stage(b"new")?;
        fs::write(&target, b"existing")?;
        assert_eq!(
            publish_using(
                &stage,
                &target,
                &AtomicBool::new(false),
                |_, _| Ok(()),
                unsupported
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read(&target)?, b"existing");
        #[cfg(unix)]
        {
            fs::remove_file(&target)?;
            std::os::unix::fs::symlink("missing", &target)?;
            assert!(publish_using(
                &stage,
                &target,
                &AtomicBool::new(false),
                |_, _| Ok(()),
                unsupported
            )
            .is_err());
            assert_eq!(fs::read_link(&target)?, PathBuf::from("missing"));
            assert!(!t.0.join("missing").exists());
        }
        Ok(())
    }
    #[test]
    fn cancellation_and_write_failure_clean_only_owned_partial_output() -> io::Result<()> {
        for cancel_copy in [false, true] {
            let t = Fixture::new()?;
            let (stage, target) = t.stage(&vec![7; 400_000])?;
            let cancel = AtomicBool::new(false);
            let result = publish_using(
                &stage,
                &target,
                &cancel,
                |n, _| {
                    if n > 0 {
                        if cancel_copy {
                            cancel.store(true, Ordering::Relaxed);
                        } else {
                            return Err(io::Error::new(
                                io::ErrorKind::StorageFull,
                                "injected device full",
                            ));
                        }
                    }
                    Ok(())
                },
                unsupported,
            );
            assert_eq!(
                result.unwrap_err().kind(),
                if cancel_copy {
                    io::ErrorKind::Interrupted
                } else {
                    io::ErrorKind::StorageFull
                }
            );
            assert!(!target.exists());
            assert_eq!(fs::metadata(stage.path())?.len(), 400_000);
        }
        Ok(())
    }
    #[test]
    fn verification_detects_corrupt_fallback_output_and_retains_stage() -> io::Result<()> {
        let t = Fixture::new()?;
        let (stage, target) = t.stage(b"verified original")?;
        let mut corrupted = false;
        let result = publish_using(
            &stage,
            &target,
            &AtomicBool::new(false),
            |n, _| {
                if n > 0 && !corrupted {
                    fs::OpenOptions::new()
                        .write(true)
                        .open(&target)?
                        .write_all(b"X")?;
                    corrupted = true;
                }
                Ok(())
            },
            unsupported,
        );
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("verification failed"));
        assert!(!target.exists());
        assert_eq!(fs::read(stage.path())?, b"verified original");
        Ok(())
    }

    #[test]
    fn directory_durability_failure_keeps_completed_direct_output() -> io::Result<()> {
        let t = Fixture::new()?;
        let target = t.0.join("direct");
        let mut output = Output::create(&target)?;
        output.file.write_all(b"completed")?;
        output.file.sync_all()?;
        assert!(output
            .commit_using(|_| Err(io::Error::other("injected directory sync failure")))
            .is_err());
        drop(output);
        assert_eq!(fs::read(target)?, b"completed");
        Ok(())
    }

    #[test]
    fn permission_and_io_publication_errors_do_not_trigger_fallback() -> io::Result<()> {
        let t = Fixture::new()?;
        let (stage, target) = t.stage(b"data")?;
        for kind in [
            io::ErrorKind::PermissionDenied,
            io::ErrorKind::StorageFull,
            io::ErrorKind::Other,
        ] {
            let error = publish_using(
                &stage,
                &target,
                &AtomicBool::new(false),
                |_, _| panic!("must not start fallback"),
                || Err(io::Error::new(kind, "real failure")),
            )
            .unwrap_err();
            assert_eq!(error.kind(), kind);
            assert!(!target.exists());
        }
        Ok(())
    }
    #[cfg(unix)]
    #[test]
    fn replaced_destination_is_not_deleted_by_failed_fallback() -> io::Result<()> {
        let t = Fixture::new()?;
        let (stage, target) = t.stage(&vec![7; 400_000])?;
        let result = publish_using(
            &stage,
            &target,
            &AtomicBool::new(false),
            |n, _| {
                if n > 0 {
                    fs::rename(&target, t.0.join("held-output"))?;
                    fs::write(&target, b"replacement")?;
                    return Err(io::Error::other("injected replacement"));
                }
                Ok(())
            },
            unsupported,
        );
        assert!(result.is_err());
        assert_eq!(fs::read(&target)?, b"replacement");
        assert!(stage.path().exists());
        Ok(())
    }
    #[cfg(unix)]
    #[test]
    fn retired_stage_is_rejected_before_creating_destination() -> io::Result<()> {
        let t = Fixture::new()?;
        let (stage, target) = t.stage(b"original")?;
        fs::rename(stage.path(), t.0.join("held-stage"))?;
        fs::write(stage.path(), b"replacement")?;
        assert!(publish_using(
            &stage,
            &target,
            &AtomicBool::new(false),
            |_, _| Ok(()),
            unsupported
        )
        .is_err());
        assert!(!target.exists());
        assert_eq!(fs::read(stage.path())?, b"replacement");
        Ok(())
    }
}

#[cfg(test)]
#[path = "compatible_bench.rs"]
mod benchmarks;
