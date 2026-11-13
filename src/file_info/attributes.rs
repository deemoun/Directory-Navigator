//! Handle-based, non-recursive attribute changes. Never follow entry links.
use std::{
    fs::{self, File, OpenOptions},
    io,
    path::Path,
};

#[derive(Clone, Debug)]
pub struct Attributes {
    file: std::sync::Arc<File>,
    original: u32,
    pub value: u32,
}

#[cfg(unix)]
pub const FIELDS: &[(&str, u32)] = &[
    ("Owner read", 0o400),
    ("Owner write", 0o200),
    ("Owner execute", 0o100),
    ("Group read", 0o040),
    ("Group write", 0o020),
    ("Group execute", 0o010),
    ("Others read", 0o004),
    ("Others write", 0o002),
    ("Others execute", 0o001),
];
#[cfg(windows)]
pub const FIELDS: &[(&str, u32)] = &[
    ("Read-only", 1),
    ("Hidden", 2),
    ("System", 4),
    ("Archive", 32),
];
#[cfg(not(any(unix, windows)))]
pub const FIELDS: &[(&str, u32)] = &[];

impl Attributes {
    pub fn open(path: &Path) -> io::Result<Self> {
        let identity = crate::platform_fs::entry_identity(path)?;
        let before = fs::symlink_metadata(path)?;
        if before.is_symlink()
            || crate::platform_fs::directory_link(&before)
            || !(before.is_file() || before.is_dir())
        {
            return Err(io::Error::other(
                "Links and special files are not editable.",
            ));
        }
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            #[cfg(target_os = "linux")]
            options.custom_flags(libc::O_PATH | libc::O_NOFOLLOW | libc::O_CLOEXEC);
            #[cfg(target_os = "macos")]
            options.custom_flags(libc::O_EVTONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
            #[cfg(not(any(target_os = "linux", target_os = "macos")))]
            options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            // FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES; no data-write access.
            options
                .access_mode(0x80 | 0x100)
                .custom_flags(0x02000000 | 0x00200000);
        }
        let file = options.open(path)?;
        if !crate::platform_fs::owns_file(path, &file)
            || crate::platform_fs::entry_identity(path)? != identity
        {
            return Err(io::Error::other("Entry changed while reading attributes."));
        }
        let opened = file.metadata()?;
        if opened.is_symlink()
            || crate::platform_fs::directory_link(&opened)
            || !(opened.is_file() || opened.is_dir())
        {
            return Err(io::Error::other(
                "Links and special files are not editable.",
            ));
        }
        let original = bits(&file)?;
        Ok(Self {
            file: std::sync::Arc::new(file),
            original,
            value: original,
        })
    }
    pub fn owns(&self, path: &Path) -> bool {
        crate::platform_fs::owns_file(path, &self.file)
    }
    pub fn toggle(&mut self, index: usize) {
        if let Some((_, bit)) = FIELDS.get(index) {
            self.value ^= bit;
        }
    }
    pub fn reset(&mut self) {
        self.value = self.original;
    }
    pub fn changed(&self) -> bool {
        self.value != self.original
    }
    pub fn apply(&mut self, path: &Path) -> io::Result<()> {
        if !crate::platform_fs::owns_file(path, &self.file) || bits(&self.file)? != self.original {
            return Err(io::Error::other(
                "Entry or attributes changed. Close Properties and reopen before applying.",
            ));
        }
        let mask = FIELDS.iter().fold(0, |mask, (_, bit)| mask | bit);
        let value = (self.original & !mask) | (self.value & mask);
        set_bits(&self.file, value)?;
        self.original = bits(&self.file)?;
        self.value = self.original;
        let verification_mask = if cfg!(unix) { mask | 0o7000 } else { mask };
        if self.original & verification_mask != value & verification_mask {
            return Err(io::Error::other("The filesystem did not retain all requested attributes. Reopen Properties to review."));
        }
        Ok(())
    }
}
#[cfg(unix)]
fn bits(file: &File) -> io::Result<u32> {
    use std::os::unix::fs::PermissionsExt;
    Ok(file.metadata()?.permissions().mode() & 0o7777)
}
#[cfg(unix)]
fn set_bits(file: &File, value: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;
        // O_PATH can open unreadable owned files. The proc descriptor link refers
        // to this pinned inode, never a newly substituted user pathname.
        fs::set_permissions(
            format!("/proc/self/fd/{}", file.as_raw_fd()),
            fs::Permissions::from_mode(value),
        )
    }
    #[cfg(not(target_os = "linux"))]
    file.set_permissions(fs::Permissions::from_mode(value))
}
#[cfg(windows)]
fn bits(file: &File) -> io::Result<u32> {
    use std::os::windows::fs::MetadataExt;
    Ok(file.metadata()?.file_attributes())
}
#[cfg(windows)]
fn set_bits(file: &File, value: u32) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{FileBasicInfo, SetFileInformationByHandle, FILE_BASIC_INFO},
    };
    let info = FILE_BASIC_INFO {
        FileAttributes: if value == 0 { 0x80 } else { value & !0x80 },
        ..Default::default()
    };
    unsafe {
        SetFileInformationByHandle(
            HANDLE(file.as_raw_handle()),
            FileBasicInfo,
            &info as *const _ as *const _,
            std::mem::size_of::<FILE_BASIC_INFO>() as u32,
        )
    }
    .map_err(io::Error::other)
}
#[cfg(not(any(unix, windows)))]
fn bits(_: &File) -> io::Result<u32> {
    Err(io::Error::other("Attributes unavailable"))
}
#[cfg(not(any(unix, windows)))]
fn set_bits(_: &File, _: u32) -> io::Result<()> {
    Err(io::Error::other("Attributes unavailable"))
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> io::Result<Self> {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "dn-attributes-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            fs::create_dir(&path)?;
            Ok(Self(path))
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn draft_cancel_apply_and_changed_attribute_refusal() -> io::Result<()> {
        let root = Fixture::new()?;
        let path = root.0.join("Ёж世界.txt");
        fs::write(&path, b"preserve content")?;
        let mut draft = Attributes::open(&path)?;
        let original = draft.value;
        let index = if cfg!(unix) { 2 } else { 1 };
        draft.toggle(index);
        assert!(draft.changed());
        assert_eq!(bits(&draft.file)?, original);
        draft.reset();
        assert!(!draft.changed());
        draft.toggle(index);
        draft.apply(&path)?;
        assert_eq!(bits(&draft.file)?, original ^ FIELDS[index].1);
        assert_eq!(fs::read(&path)?, b"preserve content");
        let mut external = Attributes::open(&path)?;
        draft.toggle(index);
        draft.apply(&path)?;
        external.toggle(index);
        assert!(external.apply(&path).is_err());
        assert_eq!(bits(&draft.file)?, original);
        Ok(())
    }
    #[test]
    fn replaced_entry_cannot_receive_attributes() -> io::Result<()> {
        let root = Fixture::new()?;
        let path = root.0.join("file");
        fs::write(&path, b"original")?;
        let mut draft = Attributes::open(&path)?;
        fs::rename(&path, root.0.join("old"))?;
        fs::write(&path, b"replacement")?;
        let before = fs::metadata(&path)?.permissions();
        draft.toggle(1);
        assert!(draft.apply(&path).is_err());
        assert_eq!(fs::metadata(&path)?.permissions(), before);
        assert_eq!(fs::read(&path)?, b"replacement");
        Ok(())
    }
    #[test]
    #[cfg(unix)]
    fn unreadable_owned_file_respects_platform_handle_access() -> io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let root = Fixture::new()?;
        let path = root.0.join("unreadable");
        fs::write(&path, b"owned")?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o0))?;
        let opened = Attributes::open(&path);
        #[cfg(not(target_os = "linux"))]
        if let Err(error) = &opened {
            // Darwin requires access even for O_EVTONLY. Never fall back to a
            // pathname chmod that could change a substituted entry.
            assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
            assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0);
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
            assert_eq!(fs::read(&path)?, b"owned");
            return Ok(());
        }
        let mut attributes = opened?;
        attributes.toggle(0);
        attributes.apply(&path)?;
        assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o400);
        assert_eq!(fs::read(&path)?, b"owned");
        Ok(())
    }
    #[test]
    #[cfg(unix)]
    fn links_rejected_and_directory_changes_are_non_recursive() -> io::Result<()> {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let root = Fixture::new()?;
        let child = root.0.join("child");
        fs::create_dir(&child)?;
        let file = child.join("file");
        fs::write(&file, b"unchanged")?;
        symlink(&child, root.0.join("link"))?;
        assert!(Attributes::open(&root.0.join("link")).is_err());
        let before = fs::metadata(&file)?.permissions().mode();
        let mut draft = Attributes::open(&child)?;
        draft.toggle(8);
        draft.apply(&child)?;
        assert_eq!(fs::metadata(&file)?.permissions().mode(), before);
        Ok(())
    }
}
