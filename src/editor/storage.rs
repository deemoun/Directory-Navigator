use super::buffer::{Buffer, MAX_BYTES};
use crate::filesystem;
use crate::text_encoding::{self, TextFormat};
use std::{
    fs::{self, Metadata, Permissions},
    io::{self, Read, Write},
    path::Path,
    time::SystemTime,
};

#[derive(Clone, Debug)]
pub struct Snapshot {
    bytes: Vec<u8>,
    modified: Option<SystemTime>,
    permissions: Permissions,
    #[cfg(unix)]
    identity: (u64, u64),
}
impl Snapshot {
    fn new(bytes: Vec<u8>, metadata: &Metadata) -> Self {
        Self {
            bytes,
            modified: metadata.modified().ok(),
            permissions: metadata.permissions(),
            #[cfg(unix)]
            identity: {
                use std::os::unix::fs::MetadataExt;
                (metadata.dev(), metadata.ino())
            },
        }
    }
    fn matches(&self, other: &Self) -> bool {
        #[cfg(unix)]
        if self.identity != other.identity {
            return false;
        }
        self.modified == other.modified
            && self.permissions == other.permissions
            && self.bytes == other.bytes
    }
}
fn read(path: &Path) -> io::Result<(Vec<u8>, Metadata)> {
    read_cancelled(path, &crate::task::Cancellation::default())
}
fn read_cancelled(
    path: &Path,
    cancel: &crate::task::Cancellation,
) -> io::Result<(Vec<u8>, Metadata)> {
    cancel.check()?;
    let entry = fs::symlink_metadata(path)?;
    if !entry.is_file() {
        return Err(io::Error::other(
            "Only regular files can be edited; links are not followed.",
        ));
    }
    let mut file = filesystem::open_regular(path)?;
    let metadata = file.metadata()?;
    if metadata.len() > MAX_BYTES as u64 {
        return Err(io::Error::other(
            "Editor limit is 64 MiB. Use F3 for a bounded preview.",
        ));
    }
    let mut bytes = Vec::new();
    let mut limited = (&mut file).take(MAX_BYTES as u64 + 1);
    let mut chunk = [0; 65536];
    loop {
        cancel.check()?;
        let n = limited.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..n]);
    }
    if bytes.len() > MAX_BYTES {
        return Err(io::Error::other(
            "File grew beyond the 64 MiB editor limit.",
        ));
    }
    let after = file.metadata()?;
    if metadata.len() != after.len() || metadata.modified().ok() != after.modified().ok() {
        return Err(io::Error::other(
            "File changed while being read; reopen it.",
        ));
    }
    Ok((bytes, metadata))
}
pub fn load(path: &Path) -> Result<(Buffer, Snapshot, TextFormat), String> {
    load_format(path, None)
}
pub fn load_format(
    path: &Path,
    chosen: Option<TextFormat>,
) -> Result<(Buffer, Snapshot, TextFormat), String> {
    load_format_cancelled(path, chosen, &crate::task::Cancellation::default())
}
pub(crate) fn load_format_cancelled(
    path: &Path,
    chosen: Option<TextFormat>,
    cancel: &crate::task::Cancellation,
) -> Result<(Buffer, Snapshot, TextFormat), String> {
    let (bytes, metadata) = read_cancelled(path, cancel).map_err(|e| e.to_string())?;
    let format = match chosen {
        Some(format) => format,
        None => text_encoding::detect(&bytes, false)?.ok_or("Choose a text encoding with F8.")?,
    };
    cancel.check().map_err(|e| e.to_string())?;
    let buffer = Buffer::new(format.decode(&bytes, false)?)?;
    cancel.check().map_err(|e| e.to_string())?;
    Ok((buffer, Snapshot::new(bytes, &metadata), format))
}
pub(crate) fn encoding_bytes_cancelled(
    path: &Path,
    cancel: &crate::task::Cancellation,
) -> Result<Vec<u8>, String> {
    read_cancelled(path, cancel)
        .map(|(bytes, _)| bytes)
        .map_err(|e| e.to_string())
}
fn verify(path: &Path, original: &Snapshot) -> io::Result<()> {
    let (bytes, metadata) = read(path)?;
    if metadata.permissions().readonly() {
        return Err(io::Error::other("File is read-only. Use Save As."));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            return Err(io::Error::other(
                "File has hard links. Use Save As to avoid splitting linked contents.",
            ));
        }
    }
    if !original.matches(&Snapshot::new(bytes, &metadata)) {
        return Err(io::Error::other(
            "File changed outside the editor. Original was not replaced; use Save As or reopen.",
        ));
    }
    Ok(())
}
/// Identity and revision approved by the Save As replacement prompt.
#[derive(Clone, Debug)]
pub struct Replacement(crate::operations::conflict::Approved);
pub(crate) fn replacement(path: &Path) -> Result<Option<Replacement>, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.is_file() || crate::operations::conflict::redirected(&metadata) {
                return Err(
                    "Only regular files can be replaced; links and directories are retained".into(),
                );
            }
            let file = filesystem::open_regular(path).map_err(|e| e.to_string())?;
            crate::operations::conflict::verify_replacement(path, &metadata)
                .map_err(|e| e.to_string())?;
            if !crate::platform_fs::same_identity(
                &metadata,
                &file.metadata().map_err(|e| e.to_string())?,
            ) {
                return Err("Destination changed before confirmation".into());
            }
            Ok(Some(Replacement(
                crate::operations::conflict::Approved::capture(path, metadata)
                    .map_err(|e| e.to_string())?,
            )))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}
pub(crate) fn save_replacing(
    path: &Path,
    text: &str,
    format: TextFormat,
    approved: &Replacement,
) -> Result<SaveOutcome, String> {
    save_prepared_with(path, text, format, None, Some(approved), |_| Ok(()))
}
/// An error means this attempt did not publish. A successful result always
/// carries the published identity, even when its durability is uncertain.
#[derive(Debug)]
pub struct SaveOutcome {
    pub snapshot: Snapshot,
    pub warning: Option<String>,
}
#[derive(Clone, Copy)]
enum Stage {
    Write,
    FileSync,
    Publish,
    ParentSync,
}
pub fn save(
    path: &Path,
    text: &str,
    format: impl Into<TextFormat>,
    original: Option<&Snapshot>,
) -> Result<SaveOutcome, String> {
    save_with(path, text, format, original, |_| Ok(()))
}
fn save_with(
    path: &Path,
    text: &str,
    format: impl Into<TextFormat>,
    original: Option<&Snapshot>,
    check: impl FnMut(Stage) -> io::Result<()>,
) -> Result<SaveOutcome, String> {
    save_prepared_with(path, text, format, original, None, check)
}
fn save_prepared_with(
    path: &Path,
    text: &str,
    format: impl Into<TextFormat>,
    original: Option<&Snapshot>,
    replacement: Option<&Replacement>,
    mut check: impl FnMut(Stage) -> io::Result<()>,
) -> Result<SaveOutcome, String> {
    let bytes = format.into().encode(text)?;
    let prepare = || -> io::Result<_> {
        if text.len() > MAX_BYTES || bytes.len() > MAX_BYTES {
            return Err(io::Error::other(
                "Saved file exceeds the 64 MiB editor limit.",
            ));
        }
        if let Some(original) = original {
            verify(path, original)?;
        } else if let Some(replacement) = replacement {
            replacement.0.verify(path)?;
        } else if fs::symlink_metadata(path).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "Destination exists; choose a new name.",
            ));
        }
        crate::publication::StagedFile::create(path)
    };
    let mut staged = prepare().map_err(|e| e.to_string())?;
    let publish = || -> io::Result<_> {
        check(Stage::Write)?;
        staged.file.write_all(&bytes)?;
        if original.is_some() || replacement.is_some() {
            let input = filesystem::open_regular(path)?;
            crate::platform_fs::copy_extended_metadata(&input, &staged.file)?;
            let permissions = original
                .map(|o| o.permissions.clone())
                .unwrap_or_else(|| replacement.unwrap().0.metadata.permissions());
            staged.file.set_permissions(permissions)?;
        }
        check(Stage::FileSync)?;
        staged.file.sync_all()?;
        let metadata = staged.file.metadata()?;
        check(Stage::Publish)?;
        if let Some(original) = original {
            verify(path, original)?;
            if !crate::platform_fs::owns_file(staged.path(), &staged.file) {
                return Err(io::Error::other("Temporary save was replaced"));
            }
            fs::rename(staged.path(), path)?;
        } else if let Some(replacement) = replacement {
            replacement.0.verify(path)?;
            if !crate::platform_fs::owns_file(staged.path(), &staged.file) {
                return Err(io::Error::other("Temporary save was replaced"));
            }
            fs::rename(staged.path(), path)?;
        } else {
            staged.publish_unsynced(path)?;
        }
        Ok(Snapshot::new(bytes, &metadata))
    };
    let snapshot = publish().map_err(|e| e.to_string())?;
    // No ordinary error may escape after publication: callers need this snapshot
    // to retry safely and must retain dirty/recovery state until confirmed.
    let warning = check(Stage::ParentSync)
        .and_then(|_| crate::platform_fs::sync_parent(path))
        .err()
        .map(|e| e.to_string());
    Ok(SaveOutcome { snapshot, warning })
}
/// Retry an already published revision without replacing the file again.
pub fn confirm_durability(path: &Path, snapshot: &Snapshot) -> Result<SaveOutcome, String> {
    verify(path, snapshot).map_err(|e| e.to_string())?;
    // FlushFileBuffers on Windows requires a handle opened for writing.
    let warning = filesystem::open_regular_writable(path)
        .and_then(|f| f.sync_all())
        .and_then(|_| crate::platform_fs::sync_parent(path))
        .err()
        .map(|e| e.to_string());
    Ok(SaveOutcome {
        snapshot: snapshot.clone(),
        warning,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) struct Temp(pub(super) std::path::PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    pub(super) fn fixture(label: &str) -> Temp {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "dn-save-outcome-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Temp(p)
    }
    #[test]
    fn failures_before_publication_leave_original_or_absent_destination_untouched() {
        for fault in [Stage::Write, Stage::FileSync, Stage::Publish] {
            let f = fixture("early");
            let p = f.0.join("source");
            fs::write(&p, "original").unwrap();
            let (_, snapshot, _) = load(&p).unwrap();
            let inject = |s: Stage| {
                if std::mem::discriminant(&s) == std::mem::discriminant(&fault) {
                    Err(io::Error::other("injected save failure"))
                } else {
                    Ok(())
                }
            };
            assert!(save_with(&p, "edited", false, Some(&snapshot), inject).is_err());
            assert_eq!(fs::read(&p).unwrap(), b"original");
            let new = f.0.join("new");
            assert!(save_with(&new, "edited", false, None, inject).is_err());
            assert!(!new.exists());
            assert_eq!(fs::read_dir(&f.0).unwrap().count(), 1);
        }
    }
    #[test]
    fn late_failure_returns_published_identity_and_retry_does_not_replace_it() {
        for existing in [false, true] {
            let f = fixture("late");
            let p = f.0.join("file");
            let original = if existing {
                fs::write(&p, "old").unwrap();
                Some(load(&p).unwrap().1)
            } else {
                None
            };
            let out = save_with(&p, "new\r\n界", true, original.as_ref(), |s| {
                if matches!(s, Stage::ParentSync) {
                    Err(io::Error::other("injected directory sync failure"))
                } else {
                    Ok(())
                }
            })
            .unwrap();
            assert!(out.warning.is_some());
            assert_eq!(fs::read(&p).unwrap(), "\u{feff}new\r\n界".as_bytes());
            #[cfg(unix)]
            let inode = {
                use std::os::unix::fs::MetadataExt;
                fs::metadata(&p).unwrap().ino()
            };
            let retried = confirm_durability(&p, &out.snapshot).unwrap();
            assert!(
                retried.warning.is_none(),
                "Retry warning: {:?}",
                retried.warning
            );
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                assert_eq!(fs::metadata(&p).unwrap().ino(), inode);
            }
            fs::write(&p, "external change").unwrap();
            assert!(confirm_durability(&p, &retried.snapshot).is_err());
            assert_eq!(fs::read(&p).unwrap(), b"external change");
        }
    }
    #[test]
    fn racing_new_destination_and_replaced_open_file_are_not_overwritten() {
        let f = fixture("race");
        let p = f.0.join("new");
        assert!(save_with(&p, "ours", false, None, |s| {
            if matches!(s, Stage::Publish) {
                fs::write(&p, "winner")?;
            }
            Ok(())
        })
        .is_err());
        assert_eq!(fs::read(&p).unwrap(), b"winner");
        let original = load(&p).unwrap().1;
        assert!(save_with(&p, "ours", false, Some(&original), |s| {
            if matches!(s, Stage::Publish) {
                fs::write(&p, "later")?;
            }
            Ok(())
        })
        .is_err());
        assert_eq!(fs::read(&p).unwrap(), b"later");
    }
}

#[cfg(test)]
mod replacement_tests {
    use super::*;
    #[test]
    fn replacement_failure_keeps_old_bytes_and_changed_revision_is_rejected() {
        let f = super::tests::fixture("replace");
        let path = f.0.join("old");
        fs::write(&path, b"old").unwrap();
        let approved = replacement(&path).unwrap().unwrap();
        let r = save_prepared_with(
            &path,
            "new",
            TextFormat::default(),
            None,
            Some(&approved),
            |stage| {
                if matches!(stage, Stage::Publish) {
                    return Err(io::Error::other("Injected publication failure"));
                }
                Ok(())
            },
        );
        assert!(r.is_err());
        assert_eq!(fs::read(&path).unwrap(), b"old");
        assert_eq!(fs::read_dir(&f.0).unwrap().count(), 1);
        fs::write(&path, b"externally changed").unwrap();
        assert!(save_replacing(&path, "new", TextFormat::default(), &approved).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"externally changed");
    }
}
