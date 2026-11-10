//! Per-transfer, cancellable conflict rendezvous. No worker owns UI state.
use super::*;
use std::sync::{mpsc, Mutex};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Replace,
    Skip,
    ReplaceAll,
    SkipAll,
    Cancel,
}
#[derive(Clone, Debug)]
pub struct Conflict {
    pub source: PathBuf,
    pub source_label: Option<String>,
    pub destination: PathBuf,
    pub destination_label: Option<String>,
    pub source_size: u64,
    pub destination_size: u64,
    pub source_modified: Option<std::time::SystemTime>,
    pub destination_modified: Option<std::time::SystemTime>,
}
pub(crate) struct Pending {
    pub conflict: Conflict,
    pub answer: mpsc::SyncSender<Decision>,
}
#[derive(Clone, Default)]
pub(crate) struct Mailbox(Arc<Mutex<Option<Pending>>>);
impl Mailbox {
    pub fn pending(&self) -> Option<Conflict> {
        self.0.lock().unwrap().as_ref().map(|p| p.conflict.clone())
    }
    pub fn answer(&self, decision: Decision) {
        if let Some(p) = self.0.lock().unwrap().take() {
            let _ = p.answer.try_send(decision);
        }
    }
    pub fn ask(&self, conflict: &Conflict, cancel: &AtomicBool) -> io::Result<Decision> {
        check(cancel)?;
        let (answer, receive) = mpsc::sync_channel(1);
        *self.0.lock().unwrap() = Some(Pending {
            conflict: conflict.clone(),
            answer,
        });
        let result = loop {
            if let Err(e) = check(cancel) {
                break Err(e);
            }
            match receive.recv_timeout(Duration::from_millis(20)) {
                Ok(d) => break Ok(d),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => break Err(io::Error::other("Conflict prompt closed")),
            }
        };
        self.0.lock().unwrap().take();
        result
    }
}
pub(crate) type Ask<'a> = dyn FnMut(&Conflict) -> io::Result<Decision> + 'a;
pub(crate) fn no_prompt(_: &Conflict) -> io::Result<Decision> {
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "Destination exists; replacement requires confirmation",
    ))
}
#[derive(Clone, Debug)]
pub(crate) struct Approved {
    pub metadata: Metadata,
    identity: crate::platform_fs::EntryIdentity,
}
impl Approved {
    pub fn capture(path: &Path, metadata: Metadata) -> io::Result<Self> {
        let approved = Self {
            identity: crate::platform_fs::entry_identity(path)?,
            metadata,
        };
        approved.verify(path)?;
        Ok(approved)
    }
    pub fn verify(&self, path: &Path) -> io::Result<()> {
        if crate::platform_fs::entry_identity(path)? != self.identity {
            return Err(io::Error::other(
                "Destination changed after confirmation; existing file retained",
            ));
        }
        verify_replacement(path, &self.metadata)
    }
}
pub(crate) struct Resolver<'a> {
    policy: ExistingDestination,
    ask: &'a mut Ask<'a>,
    all: Option<Decision>,
}
impl<'a> Resolver<'a> {
    pub fn new(policy: ExistingDestination, ask: &'a mut Ask<'a>) -> Self {
        Self {
            policy,
            ask,
            all: None,
        }
    }
    /// None skips; Some(None) creates a new name; Some(Some(metadata)) replaces only that revision.
    pub fn file(
        &mut self,
        source: &Path,
        input: &Metadata,
        target: &Path,
    ) -> io::Result<Option<Option<Approved>>> {
        let existing = match fs::symlink_metadata(target) {
            Ok(m) => m,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Some(None)),
            Err(e) => return Err(e),
        };
        if self.policy == ExistingDestination::Skip {
            return Ok(None);
        }
        if self.policy == ExistingDestination::Refuse {
            absent(target)?;
        }
        if !input.is_file() || !existing.is_file() || redirected(&existing) {
            return Err(io::Error::other(
                "Only regular files can be replaced; incompatible types or links were retained",
            ));
        }
        if crate::platform_fs::entry_identity(source)?
            == crate::platform_fs::entry_identity(target)?
        {
            return Err(io::Error::other("Cannot replace a file with itself"));
        }
        let approved = Approved::capture(target, existing.clone())?;
        let conflict = Conflict {
            source_label: None,
            source: source.to_owned(),
            destination: target.to_owned(),
            destination_label: None,
            source_size: input.len(),
            destination_size: existing.len(),
            source_modified: input.modified().ok(),
            destination_modified: existing.modified().ok(),
        };
        let decision = match self.all {
            Some(d) => d,
            None => (self.ask)(&conflict)?,
        };
        match decision {
            Decision::ReplaceAll => {
                self.all = Some(Decision::Replace);
            }
            Decision::SkipAll => {
                self.all = Some(Decision::Skip);
                return Ok(None);
            }
            Decision::Skip => return Ok(None),
            Decision::Cancel => {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "Operation cancelled",
                ))
            }
            Decision::Replace => {}
        }
        approved.verify(target)?;
        Ok(Some(Some(approved)))
    }
}
pub(crate) fn redirected(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.is_symlink()
    }
}
pub(crate) fn verify_replacement(path: &Path, original: &Metadata) -> io::Result<()> {
    let current = fs::symlink_metadata(path)?;
    if !current.is_file() || redirected(&current) || !unchanged_file(original, &current) {
        return Err(io::Error::other(
            "Destination changed after confirmation; existing file retained",
        ));
    }
    Ok(())
}
pub(crate) fn publish(
    stage: &crate::publication::StagedFile,
    target: &Path,
    approved: Option<&Approved>,
) -> io::Result<()> {
    if let Some(original) = approved {
        stage.file.sync_all()?;
        if !crate::platform_fs::owns_file(stage.path(), &stage.file) {
            return Err(io::Error::other("Temporary output was replaced"));
        }
        original.verify(target)?;
        fs::rename(stage.path(), target)?;
        crate::platform_fs::sync_parent(target)
    } else {
        stage.publish(target)
    }
}
