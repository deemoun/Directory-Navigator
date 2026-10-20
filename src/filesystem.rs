use std::{
    ffi::OsString,
    fs, io,
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Debug)]
pub struct Entry {
    pub file_type: Option<fs::FileType>,
    pub name: OsString,
    pub path: PathBuf,
    pub directory: bool,
    pub symlink: bool,
    pub size: Option<u64>,
    pub parent: bool,
    pub modified: Option<SystemTime>,
    pub executable: bool,
}

/// Metadata describes the link itself; directory scans never recurse.
pub fn scan(path: &Path, show_hidden: bool) -> io::Result<Vec<Entry>> {
    let mut entries = scan_unordered(path, show_hidden, &crate::task::Cancellation::default())?;
    entries.sort_by(|a, b| {
        b.parent
            .cmp(&a.parent)
            .then_with(|| b.directory.cmp(&a.directory))
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(entries)
}
pub(crate) fn scan_unordered(
    path: &Path,
    show_hidden: bool,
    cancel: &crate::task::Cancellation,
) -> io::Result<Vec<Entry>> {
    cancel.check()?;
    let mut entries = Vec::new();
    for item in fs::read_dir(path)? {
        cancel.check()?;
        let item = item?;
        let name = item.file_name();
        if !show_hidden && name.to_string_lossy().starts_with('.') {
            continue;
        }
        let metadata = match fs::symlink_metadata(item.path()) {
            Ok(metadata) => Some(metadata),
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(_) => None,
        };
        entries.push(Entry {
            file_type: metadata.as_ref().map(|m| m.file_type()),
            name,
            path: item.path(),
            directory: metadata.as_ref().is_some_and(|m| m.is_dir()),
            symlink: metadata.as_ref().is_some_and(|m| m.is_symlink()),
            size: metadata.as_ref().map(|m| m.len()),
            parent: false,
            modified: metadata.as_ref().and_then(|m| m.modified().ok()),
            executable: metadata.as_ref().is_some_and(executable),
        });
    }
    if let Some(parent) = path.parent() {
        entries.insert(
            0,
            Entry {
                file_type: None,
                name: "..".into(),
                path: parent.into(),
                directory: true,
                symlink: false,
                size: None,
                parent: true,
                modified: None,
                executable: false,
            },
        );
    }
    Ok(entries)
}

fn executable(metadata: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        false
    }
}

/// Open regular files without following a final symlink or blocking on a FIFO.
pub fn open_regular(path: &Path) -> io::Result<fs::File> {
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
        options.custom_flags(0x00200000); // FILE_FLAG_OPEN_REPARSE_POINT
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("Only regular files can be read"));
    }
    Ok(file)
}

/// Keep terminal control characters in filenames from altering the display.
pub fn display_name(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_control() { '\u{fffd}' } else { c })
        .collect()
}
