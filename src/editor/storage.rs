use super::buffer::{Buffer, MAX_BYTES};
use crate::filesystem;
use std::{
    fs::{self, Metadata, Permissions},
    io::{self, Read, Write},
    path::Path,
    time::SystemTime,
};

#[derive(Clone)]
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
            "Editor limit is 2 MiB. Use F3 for a bounded preview.",
        ));
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_BYTES {
        return Err(io::Error::other("File grew beyond the 2 MiB editor limit."));
    }
    let after = file.metadata()?;
    if metadata.len() != after.len() || metadata.modified().ok() != after.modified().ok() {
        return Err(io::Error::other(
            "File changed while being read; reopen it.",
        ));
    }
    Ok((bytes, metadata))
}
pub fn load(path: &Path) -> Result<(Buffer, Snapshot, bool), String> {
    let (bytes, metadata) = read(path).map_err(|e| e.to_string())?;
    let bom = bytes.starts_with(&[0xef, 0xbb, 0xbf]);
    let content = std::str::from_utf8(if bom { &bytes[3..] } else { &bytes }).map_err(|_| {
        "File is not UTF-8 text. Use F3; editing would risk changing its encoding.".to_owned()
    })?;
    let buffer = Buffer::new(content.into())?;
    Ok((buffer, Snapshot::new(bytes, &metadata), bom))
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
/// Save As publishes without overwriting. Saving the opened file checks its
/// identity/content before atomic replacement; no target is truncated in place.
pub fn save(
    path: &Path,
    text: &str,
    bom: bool,
    original: Option<&Snapshot>,
) -> Result<Snapshot, String> {
    let save = || -> io::Result<Snapshot> {
        if text.len() + if bom { 3 } else { 0 } > MAX_BYTES {
            return Err(io::Error::other(
                "Saved file exceeds the 2 MiB editor limit.",
            ));
        }
        if let Some(original) = original {
            verify(path, original)?;
        } else if fs::symlink_metadata(path).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "Destination exists; choose a new name.",
            ));
        }
        let mut staged = crate::publication::StagedFile::create(path)?;
        let output = &mut staged.file;
        let mut bytes = Vec::with_capacity(text.len() + 3);
        if bom {
            bytes.extend_from_slice(&[0xef, 0xbb, 0xbf]);
        }
        bytes.extend_from_slice(text.as_bytes());
        output.write_all(&bytes)?;
        if let Some(original) = original {
            let input = filesystem::open_regular(path)?;
            crate::platform_fs::copy_extended_metadata(&input, output)?;
            output.set_permissions(original.permissions.clone())?;
        }
        output.sync_all()?;
        let metadata = output.metadata()?;
        if let Some(original) = original {
            verify(path, original)?;
            if !crate::platform_fs::owns_file(staged.path(), &staged.file) {
                return Err(io::Error::other("Temporary save was replaced"));
            }
            fs::rename(staged.path(), path)?;
        } else {
            // Atomic no-clobber publication, including a racing destination/link.
            staged.publish(path)?;
        }
        crate::platform_fs::sync_parent(path)?;
        Ok(Snapshot::new(bytes, &metadata))
    };
    save().map_err(|error| error.to_string())
}
