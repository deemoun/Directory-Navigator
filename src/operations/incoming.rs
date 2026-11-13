//! A bounded network stream publishes through the same guards as local Copy.
use super::*;
use crate::{
    peer::manifest::{relative, timestamp, Entry},
    publication::StagedFile,
};
use std::io::{Read, Seek, SeekFrom};
pub(crate) struct Sink<'a> {
    root: PathBuf,
    guards: DirectoryGuards,
    resolver: conflict::Resolver<'a>,
    policy: ExistingDestination,
    excluded: Vec<PathBuf>,
    capabilities: capabilities::Cache,
}
pub(crate) struct IncomingFile {
    pub stage: StagedFile,
    target: PathBuf,
    approved: Option<conflict::Approved>,
    entry: Entry,
}
impl<'a> Sink<'a> {
    pub fn new(
        root: PathBuf,
        policy: ExistingDestination,
        ask: &'a mut conflict::Ask<'a>,
    ) -> io::Result<Self> {
        if crate::platform_fs::directory_link(&fs::symlink_metadata(&root)?) {
            return Err(io::Error::other("Receive directory cannot be a link"));
        }
        let root = fs::canonicalize(root)?;
        let mut guards = DirectoryGuards::default();
        guards.capture_parent(&root)?;
        guards.capture(&root)?;
        Ok(Self {
            root,
            guards,
            resolver: conflict::Resolver::new(policy, ask),
            policy,
            excluded: vec![],
            capabilities: Default::default(),
        })
    }
    fn target(&self, entry: &Entry) -> io::Result<Option<PathBuf>> {
        let rel = relative(&entry.path)?;
        if self.excluded.iter().any(|p| rel.starts_with(p)) {
            return Ok(None);
        }
        let target = self.root.join(rel);
        self.guards.verify_parent(&target)?;
        Ok(Some(target))
    }
    pub fn directory(&mut self, entry: &Entry) -> io::Result<bool> {
        let Some(target) = self.target(entry)? else {
            return Ok(false);
        };
        match fs::symlink_metadata(&target) {
            Ok(m) => {
                if !m.is_dir() || conflict::redirected(&m) {
                    return Err(io::Error::other(
                        "Peer destination has incompatible types or links",
                    ));
                }
                match self.policy {
                    ExistingDestination::Skip => {
                        self.excluded.push(relative(&entry.path)?);
                        return Ok(false);
                    }
                    ExistingDestination::Refuse => {
                        return Err(io::Error::new(
                            io::ErrorKind::AlreadyExists,
                            "Peer destination directory exists",
                        ))
                    }
                    ExistingDestination::Ask => {}
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let builder = fs::DirBuilder::new();
                #[cfg(unix)]
                let mut builder = builder;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::DirBuilderExt;
                    builder.mode(0o700);
                }
                builder.create(&target)?;
                crate::platform_fs::sync_parent(&target)?;
            }
            Err(e) => return Err(e),
        }
        self.guards.capture(&target)?;
        Ok(true)
    }
    pub fn complete_directories(&self, entries: &[Entry], cancel: &AtomicBool) -> io::Result<()> {
        self.guards.verify_directory(&self.root)?;
        for entry in entries.iter().rev().filter(|e| e.directory) {
            check(cancel)?;
            let Some(target) = self.target(entry)? else {
                continue;
            };
            let Some(time) = entry.modified.and_then(timestamp) else {
                continue;
            };
            self.guards.verify_directory(&target)?;
            let mut options = fs::OpenOptions::new();
            options.read(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                options
                    .access_mode(0x80 | 0x100)
                    .custom_flags(0x02000000 | 0x00200000);
            }
            let directory = options.open(&target)?;
            self.guards.verify_directory(&target)?;
            if !crate::platform_fs::owns_file(&target, &directory) {
                return Err(io::Error::other(
                    "Receive directory changed before setting its time",
                ));
            }
            if let Err(error) = directory.set_times(fs::FileTimes::new().set_modified(time)) {
                if !crate::platform_fs::unsupported_operation(&error) {
                    return Err(error);
                }
            }
        }
        Ok(())
    }
    pub fn begin(&mut self, entry: &Entry) -> io::Result<Option<IncomingFile>> {
        let Some(target) = self.target(entry)? else {
            return Ok(None);
        };
        let Some(approved) = self.resolver.remote_file(
            &entry.path,
            entry.size,
            entry.modified.and_then(timestamp),
            &target,
        )?
        else {
            return Ok(None);
        };
        self.guards.verify_parent(&target)?;
        Ok(Some(IncomingFile {
            stage: StagedFile::create(&target)?,
            target,
            approved,
            entry: entry.clone(),
        }))
    }
    pub fn finish(
        &mut self,
        mut file: IncomingFile,
        expected: [u8; 32],
        options: CopyOptions,
        cancel: &AtomicBool,
        progress: &mut Progress,
    ) -> io::Result<()> {
        check(cancel)?;
        if file.stage.file.metadata()?.len() != file.entry.size {
            return Err(io::Error::other(
                "Peer file length does not match its offer",
            ));
        }
        if options.verify_writes {
            progress.phase = Phase::Verifying;
            file.stage.file.seek(SeekFrom::Start(0))?;
            let mut hash = blake3::Hasher::new();
            let mut bytes = [0; 128 * 1024];
            loop {
                check(cancel)?;
                let n = file.stage.file.read(&mut bytes)?;
                if n == 0 {
                    break;
                }
                hash.update(&bytes[..n]);
            }
            if hash.finalize().as_bytes() != &expected {
                return Err(io::Error::other("Peer file read-back checksum failed"));
            }
        }
        if let Some(original) = &file.approved {
            file.stage
                .file
                .set_permissions(original.metadata.permissions())?;
        }
        if let Some(time) = file.entry.modified.and_then(timestamp) {
            if let Err(e) = file
                .stage
                .file
                .set_times(fs::FileTimes::new().set_modified(time))
            {
                if !crate::platform_fs::unsupported_operation(&e) {
                    return Err(e);
                }
            }
        }
        file.stage.file.sync_all()?;
        self.guards.verify_parent(&file.target)?;
        let capability = self.capabilities.get(&file.target, &self.guards)?;
        let publication = conflict::publish(
            &file.stage,
            &file.target,
            file.approved.as_ref(),
            capability,
            cancel,
            |_, _| check(cancel),
        );
        if let Err(e) = publication {
            if crate::platform_fs::owns_file(&file.target, &file.stage.file) {
                progress.detail(
                    file.target.clone(),
                    format!("File published; directory synchronization failed: {e}"),
                );
            } else {
                return Err(e);
            }
        }
        progress.files += 1;
        progress.path = file.target;
        progress.phase = Phase::Copying;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> io::Result<Self> {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "dn-incoming-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path)?;
            Ok(Self(fs::canonicalize(path)?))
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn entry(name: &str) -> Entry {
        Entry {
            path: name.into(),
            directory: false,
            size: 4,
            modified: None,
        }
    }
    #[test]
    fn checksum_and_destination_changes_preserve_original_and_clean_owned_staging() -> io::Result<()>
    {
        let t = Temp::new()?;
        fs::write(t.0.join("old.txt"), b"old!")?;
        let mut ask = |_: &conflict::Conflict| Ok(conflict::Decision::Replace);
        let mut sink = Sink::new(t.0.clone(), ExistingDestination::Ask, &mut ask)?;
        let cancel = AtomicBool::new(false);
        let mut progress = Progress::default();
        let mut output = sink.begin(&entry("old.txt"))?.unwrap();
        output.stage.file.write_all(b"new!")?;
        let options = CopyOptions {
            verify_writes: true,
            ..Default::default()
        };
        assert!(sink
            .finish(output, [0; 32], options, &cancel, &mut progress)
            .is_err());
        assert_eq!(fs::read(t.0.join("old.txt"))?, b"old!");
        assert_eq!(fs::read_dir(&t.0)?.count(), 1);
        let mut output = sink.begin(&entry("old.txt"))?.unwrap();
        output.stage.file.write_all(b"new!")?;
        fs::write(t.0.join("old.txt"), b"changed destination")?;
        assert!(sink
            .finish(
                output,
                *blake3::hash(b"new!").as_bytes(),
                options,
                &cancel,
                &mut progress
            )
            .is_err());
        assert_eq!(fs::read(t.0.join("old.txt"))?, b"changed destination");
        assert_eq!(fs::read_dir(&t.0)?.count(), 1);
        Ok(())
    }
    #[test]
    fn cancellation_keeps_completed_publications_and_never_follows_links() -> io::Result<()> {
        let t = Temp::new()?;
        let mut ask = |_: &conflict::Conflict| Ok(conflict::Decision::Skip);
        let mut sink = Sink::new(t.0.clone(), ExistingDestination::Ask, &mut ask)?;
        let cancel = AtomicBool::new(false);
        let mut progress = Progress::default();
        let mut first = sink.begin(&entry("first"))?.unwrap();
        first.stage.file.write_all(b"done")?;
        sink.finish(
            first,
            *blake3::hash(b"done").as_bytes(),
            CopyOptions::default(),
            &cancel,
            &mut progress,
        )?;
        let mut second = sink.begin(&entry("second"))?.unwrap();
        second.stage.file.write_all(b"part")?;
        cancel.store(true, Ordering::Release);
        assert!(sink
            .finish(
                second,
                *blake3::hash(b"part").as_bytes(),
                CopyOptions::default(),
                &cancel,
                &mut progress
            )
            .is_err());
        assert_eq!(fs::read(t.0.join("first"))?, b"done");
        assert!(!t.0.join("second").exists());
        assert_eq!(fs::read_dir(&t.0)?.count(), 1);
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(t.0.join("first"), t.0.join("link"))?;
            assert!(sink.begin(&entry("link")).is_err());
            assert_eq!(fs::read(t.0.join("first"))?, b"done");
        }
        Ok(())
    }
}
