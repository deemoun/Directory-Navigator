use std::{
    fs::{self},
    io::{self, Write},
    path::{Path, PathBuf},
};

/// Validate every native name before any filesystem mutation.
pub fn encode(paths: &[PathBuf], full_paths: bool) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    for path in paths {
        let native = path.to_str().ok_or_else(|| {
            io::Error::other(
                "List export requires UTF-8 paths; an entry cannot be represented without loss.",
            )
        })?;
        if native.contains(['\r', '\n']) {
            return Err(io::Error::other(
                "List export cannot represent a path containing a newline.",
            ));
        }
        let value = if full_paths {
            path.as_os_str()
        } else {
            path.file_name()
                .ok_or_else(|| io::Error::other("Entry has no filename"))?
        };
        let text = value.to_str().ok_or_else(|| {
            io::Error::other(
                "List export requires UTF-8 paths; an entry cannot be represented without loss.",
            )
        })?;
        if text.contains(['\r', '\n']) {
            return Err(io::Error::other(
                "List export cannot represent a filename containing a newline.",
            ));
        }
        bytes.extend_from_slice(text.as_bytes());
        bytes.push(b'\n');
    }
    Ok(bytes)
}
/// Publish complete output exclusively. Hard-link publication never replaces an
/// existing destination, including dangling symlinks. Failure leaves no partial list.
pub fn write_list(destination: &Path, paths: &[PathBuf], full_paths: bool) -> io::Result<()> {
    let bytes = encode(paths, full_paths)?;
    publish(destination, |file| {
        file.write_all(&bytes)?;
        file.sync_all()
    })
}
fn publish(
    destination: &Path,
    write: impl FnOnce(&mut fs::File) -> io::Result<()>,
) -> io::Result<()> {
    if fs::symlink_metadata(destination).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "Destination already exists",
        ));
    }
    let mut staged = crate::publication::StagedFile::create(destination)?;
    write(&mut staged.file)?;
    staged.publish(destination)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_write_and_racing_destination_preserve_existing_files() -> io::Result<()> {
        let root = std::env::temp_dir().join(format!("dn-export-failure-{}", std::process::id()));
        fs::create_dir(&root)?;
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(root.clone());
        let dest = root.join("list");
        let result = publish(&dest, |file| {
            file.write_all(b"partial")?;
            Err(io::Error::other("injected write failure"))
        });
        assert!(result.is_err());
        assert!(!dest.exists());
        assert_eq!(fs::read_dir(&root)?.count(), 0);
        let result = publish(&dest, |file| {
            file.write_all(b"new")?;
            fs::write(&dest, b"racing file")?;
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(fs::read(&dest)?, b"racing file");
        assert_eq!(fs::read_dir(&root)?.count(), 1);
        Ok(())
    }
}
