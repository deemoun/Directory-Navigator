//! Destination guard for externally generated files; publication uses Copy's
//! replacement approvals and compatibility-volume verification.
use super::*;
use crate::publication::StagedFile;

pub(crate) struct Output {
    target: PathBuf,
    guards: DirectoryGuards,
    approved: Option<conflict::Approved>,
}
impl Output {
    pub fn new(
        target: PathBuf,
        cancel: &AtomicBool,
        mailbox: &conflict::Mailbox,
    ) -> io::Result<Self> {
        Self::new_named(target, cancel, mailbox, "Generated video output", 0)
    }
    pub(crate) fn new_named(
        target: PathBuf,
        cancel: &AtomicBool,
        mailbox: &conflict::Mailbox,
        source_label: &str,
        source_size: u64,
    ) -> io::Result<Self> {
        Self::new_named_pumping(
            target,
            cancel,
            mailbox,
            source_label,
            source_size,
            || Ok(()),
        )
    }
    pub(crate) fn new_named_pumping(
        target: PathBuf,
        cancel: &AtomicBool,
        mailbox: &conflict::Mailbox,
        source_label: &str,
        source_size: u64,
        pump: impl FnMut() -> io::Result<()>,
    ) -> io::Result<Self> {
        let mut guards = DirectoryGuards::default();
        guards.capture_parent(&target)?;
        let approved = match fs::symlink_metadata(&target) {
            Ok(metadata) => {
                if !metadata.is_file() || conflict::redirected(&metadata) {
                    return Err(io::Error::other(
                        "Output must be a regular file, not a directory or link",
                    ));
                }
                let approved = conflict::Approved::capture(&target, metadata.clone())?;
                let decision = mailbox.ask_pumping(
                    &conflict::Conflict {
                        source: PathBuf::new(),
                        source_label: Some(source_label.into()),
                        destination: target.clone(),
                        destination_label: None,
                        source_size,
                        destination_size: metadata.len(),
                        source_modified: None,
                        destination_modified: metadata.modified().ok(),
                    },
                    cancel,
                    pump,
                )?;
                if !matches!(
                    decision,
                    conflict::Decision::Replace | conflict::Decision::ReplaceAll
                ) {
                    return Err(io::Error::new(
                        io::ErrorKind::Interrupted,
                        "Output replacement cancelled; existing file retained",
                    ));
                }
                Some(approved)
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
        };
        guards.verify_parent(&target)?;
        Ok(Self {
            target,
            guards,
            approved,
        })
    }
    pub fn stage(&self) -> io::Result<StagedFile> {
        self.guards.verify_parent(&self.target)?;
        StagedFile::create(&self.target)
    }
    /// A warning means publication succeeded but directory durability is unknown.
    pub fn publish(&self, stage: &StagedFile, cancel: &AtomicBool) -> io::Result<Option<String>> {
        check(cancel)?;
        stage.file.sync_all()?;
        self.guards.verify_parent(&self.target)?;
        let capability = capabilities::Cache::default().get(&self.target, &self.guards)?;
        match conflict::publish(
            stage,
            &self.target,
            self.approved.as_ref(),
            capability,
            cancel,
            |_, _| check(cancel),
        ) {
            Ok(_) => Ok(None),
            Err(e)
                if crate::platform_fs::owns_file(&self.target, &stage.file)
                    || e.get_ref()
                        .is_some_and(|e| e.is::<compatible::PublishedWarning>()) =>
            {
                Ok(Some(format!(
                    "File published; directory synchronization failed: {e}"
                )))
            }
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> io::Result<PathBuf> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "dn-generated-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p)?;
        fs::canonicalize(p)
    }
    #[test]
    fn completed_stage_is_exclusive_and_cancelled_stage_cleans_up() -> io::Result<()> {
        let root = fixture()?;
        let target = root.join("out.mp4");
        let cancel = AtomicBool::new(false);
        let output = Output::new(target.clone(), &cancel, &Default::default())?;
        let mut stage = output.stage()?;
        stage.file.write_all(b"complete")?;
        let path = stage.path().to_owned();
        output.publish(&stage, &cancel)?;
        assert_eq!(fs::read(&target)?, b"complete");
        drop(stage);
        assert!(!path.exists());
        let other = Output::new(root.join("cancelled.mp4"), &cancel, &Default::default())?;
        let mut stage = other.stage()?;
        stage.file.write_all(b"partial")?;
        let path = stage.path().to_owned();
        cancel.store(true, Ordering::Relaxed);
        assert!(other.publish(&stage, &cancel).is_err());
        drop(stage);
        assert!(!path.exists());
        assert!(!root.join("cancelled.mp4").exists());
        fs::remove_dir_all(root)
    }
    #[test]
    fn replacement_requires_approval_and_rejects_changed_destination() -> io::Result<()> {
        let root = fixture()?;
        let target = root.join("out.mp4");
        fs::write(&target, b"old")?;
        let mailbox = conflict::Mailbox::default();
        let m = mailbox.clone();
        let p = target.clone();
        let thread = std::thread::spawn(move || Output::new(p, &AtomicBool::new(false), &m));
        let deadline = Instant::now() + Duration::from_secs(5);
        while mailbox.pending().is_none() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(fs::read(&target)?, b"old");
        mailbox.answer(conflict::Decision::Replace);
        let output = thread.join().unwrap()?;
        let mut stage = output.stage()?;
        stage.file.write_all(b"new")?;
        fs::write(&target, b"externally changed, longer")?;
        assert!(output.publish(&stage, &AtomicBool::new(false)).is_err());
        assert_eq!(fs::read(&target)?, b"externally changed, longer");
        drop(stage);
        fs::remove_dir_all(root)
    }
}
