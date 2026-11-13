//! Small native filesystem boundary: no-replace rename and durability/metadata.
use std::{
    fs::{self, File, Metadata},
    io,
    path::Path,
};
#[cfg(unix)]
fn native(path: &Path) -> io::Result<std::ffi::CString> {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Path contains NUL"))
}
pub fn rename_no_replace(source: &Path, target: &Path) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let a = native(source)?;
        let b = native(target)?;
        if unsafe { libc::renamex_np(a.as_ptr(), b.as_ptr(), libc::RENAME_EXCL) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
    #[cfg(target_os = "linux")]
    {
        let a = native(source)?;
        let b = native(target)?;
        if unsafe {
            libc::syscall(
                libc::SYS_renameat2,
                libc::AT_FDCWD,
                a.as_ptr(),
                libc::AT_FDCWD,
                b.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        } == 0
        {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn MoveFileExW(a: *const u16, b: *const u16, flags: u32) -> i32;
        }
        let a: Vec<_> = source.as_os_str().encode_wide().chain(Some(0)).collect();
        let b: Vec<_> = target.as_os_str().encode_wide().chain(Some(0)).collect();
        if a[..a.len() - 1].contains(&0) || b[..b.len() - 1].contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Path contains NUL",
            ));
        }
        if unsafe { MoveFileExW(a.as_ptr(), b.as_ptr(), 0) } != 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    {
        let _ = (source, target);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Exclusive rename is unavailable",
        ))
    }
}
/// Explicit operation-unavailable responses only; permissions, cross-device,
/// invalid arguments and general I/O failures are not capability evidence.
pub(crate) fn unsupported_operation(error: &io::Error) -> bool {
    if error.kind() == io::ErrorKind::Unsupported {
        return true;
    }
    #[cfg(unix)]
    {
        error.raw_os_error().is_some_and(|code| {
            code == libc::ENOTSUP || code == libc::EOPNOTSUPP || code == libc::ENOSYS
        })
    }
    #[cfg(windows)]
    {
        matches!(error.raw_os_error(), Some(1 | 50))
    }
    #[cfg(not(any(unix, windows)))]
    {
        false
    }
}

pub fn can_copy_fallback(error: &io::Error) -> bool {
    if error.kind() == io::ErrorKind::Unsupported {
        return true;
    }
    #[cfg(unix)]
    {
        matches!(
            error.raw_os_error(),
            Some(libc::EXDEV | libc::ENOSYS | libc::EINVAL | libc::ENOTSUP)
        )
    }
    #[cfg(windows)]
    {
        matches!(error.raw_os_error(), Some(17 | 50 | 1))
    }
    #[cfg(not(any(unix, windows)))]
    {
        false
    }
}
pub fn same_identity(a: &Metadata, b: &Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        a.dev() == b.dev() && a.ino() == b.ino() && a.file_type() == b.file_type()
    }
    #[cfg(not(unix))]
    {
        a.file_type() == b.file_type() && a.created().ok() == b.created().ok()
    }
}
pub fn sync_parent(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        sync_directory(&File::open(parent)?)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

/// Directory fsync is unavailable on some filesystems. This exception must
/// never be applied to regular-file synchronization.
pub fn sync_directory(directory: &File) -> io::Result<()> {
    #[cfg(unix)]
    {
        match directory.sync_all() {
            Err(e) if matches!(e.raw_os_error(), Some(libc::EINVAL | libc::ENOTSUP)) => Ok(()),
            result => result,
        }
    }
    #[cfg(not(unix))]
    {
        let _ = directory;
        Ok(())
    }
}
/// Preserve the basic metadata that the destination filesystem can represent.
/// In particular, macOS FAT volumes do not implement Unix permission bits.
pub fn copy_basic_metadata(target: &File, metadata: &Metadata) -> io::Result<()> {
    optional_metadata(target.set_permissions(metadata.permissions()))?;
    if let Ok(modified) = metadata.modified() {
        optional_metadata(target.set_times(fs::FileTimes::new().set_modified(modified)))?;
    }
    Ok(())
}

fn optional_metadata(result: io::Result<()>) -> io::Result<()> {
    #[cfg(unix)]
    if result.as_ref().is_err_and(|error| {
        error.kind() == io::ErrorKind::Unsupported
            || error
                .raw_os_error()
                .is_some_and(|code| code == libc::ENOTSUP || code == libc::EOPNOTSUPP)
    }) {
        return Ok(());
    }
    result
}

pub fn copy_extended_metadata(source: &File, target: &File) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        use std::os::fd::AsRawFd;
        copy_macos_metadata(|flags| {
            if unsafe {
                libc::fcopyfile(
                    source.as_raw_fd(),
                    target.as_raw_fd(),
                    std::ptr::null_mut(),
                    flags,
                )
            } == 0
            {
                Ok(())
            } else {
                Err(io::Error::last_os_error())
            }
        })?;
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;
        let size = unsafe { libc::flistxattr(source.as_raw_fd(), std::ptr::null_mut(), 0) };
        if size < 0 {
            return optional_metadata(Err(io::Error::last_os_error()));
        }
        let mut names = vec![0u8; size as usize];
        let count =
            unsafe { libc::flistxattr(source.as_raw_fd(), names.as_mut_ptr().cast(), names.len()) };
        if count < 0 {
            return optional_metadata(Err(io::Error::last_os_error()));
        }
        for name in names[..count as usize].split_inclusive(|b| *b == 0) {
            if name.last() != Some(&0) {
                continue;
            }
            let size = unsafe {
                libc::fgetxattr(
                    source.as_raw_fd(),
                    name.as_ptr().cast(),
                    std::ptr::null_mut(),
                    0,
                )
            };
            if size < 0 {
                return optional_metadata(Err(io::Error::last_os_error()));
            }
            let mut value = vec![0u8; size as usize];
            let count = unsafe {
                libc::fgetxattr(
                    source.as_raw_fd(),
                    name.as_ptr().cast(),
                    value.as_mut_ptr().cast(),
                    value.len(),
                )
            };
            if count < 0 {
                return optional_metadata(Err(io::Error::last_os_error()));
            }
            if unsafe {
                libc::fsetxattr(
                    target.as_raw_fd(),
                    name.as_ptr().cast(),
                    value.as_ptr().cast(),
                    count as usize,
                    0,
                )
            } != 0
            {
                optional_metadata(Err(io::Error::last_os_error()))?;
            }
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (source, target);
    }
    Ok(())
}
// Keep fs available to callers testing path identities without following links.
pub fn owns(path: &Path, identity: &Metadata) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| same_identity(identity, &m))
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EntryIdentity(u64, u64);
impl EntryIdentity {
    pub(crate) fn same_volume(self, other: Self) -> bool {
        self.0 == other.0
    }
    #[cfg(test)]
    pub(crate) fn on_other_volume(self) -> Self {
        Self(self.0.wrapping_add(1), self.1)
    }
}

/// Directory reparse points (including junctions) must remain traversal leaves.
pub(crate) fn directory_link(metadata: &Metadata) -> bool {
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
/// A short-lived no-follow identity probe for queued roots. Unlike timestamps,
/// Windows volume/file indices distinguish replacement entries on NTFS.
pub(crate) fn entry_identity(path: &Path) -> io::Result<EntryIdentity> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let m = fs::symlink_metadata(path)?;
        Ok(EntryIdentity(m.dev(), m.ino()))
    }
    #[cfg(windows)]
    {
        use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
        use windows::Win32::{
            Foundation::HANDLE,
            Storage::FileSystem::{
                GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS,
                FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
            },
        };
        let file = fs::OpenOptions::new()
            .read(true)
            .access_mode(0)
            .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0 | FILE_SHARE_DELETE.0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0 | FILE_FLAG_BACKUP_SEMANTICS.0)
            .open(path)?;
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: the borrowed file handle and writable information live through the call.
        unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) }
            .map_err(io::Error::other)?;
        Ok(EntryIdentity(
            u64::from(info.dwVolumeSerialNumber),
            (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
        ))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Queued file identities are unavailable",
        ))
    }
}

/// Compare a directory entry with a live file handle, without following links.
/// Windows creation timestamps are not file identities (NTFS may reuse them).
pub fn owns_file(path: &Path, file: &File) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
        use windows::Win32::{
            Foundation::HANDLE,
            Storage::FileSystem::{
                GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS,
                FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
            },
        };
        fn identity(file: &File) -> io::Result<(u32, u32, u32)> {
            let mut info = BY_HANDLE_FILE_INFORMATION::default();
            // SAFETY: the borrowed handle remains open and info is writable.
            unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) }
                .map_err(io::Error::other)?;
            Ok((
                info.dwVolumeSerialNumber,
                info.nFileIndexHigh,
                info.nFileIndexLow,
            ))
        }
        let entry = fs::OpenOptions::new()
            .read(true)
            .access_mode(0)
            .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0 | FILE_SHARE_DELETE.0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0 | FILE_FLAG_BACKUP_SEMANTICS.0)
            .open(path);
        let Ok(entry) = entry else { return false };
        matches!((identity(file), identity(&entry)), (Ok(a), Ok(b)) if a == b)
    }
    #[cfg(unix)]
    {
        file.metadata().is_ok_and(|metadata| owns(path, &metadata))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (path, file);
        false
    }
}

pub fn open_directory(path: &Path) -> io::Result<File> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x02000000 | 0x00200000);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_dir() {
        return Err(io::Error::other("Directory changed before opening"));
    }
    Ok(file)
}

// Copy the two independent metadata classes separately: a destination may
// support extended attributes while lacking ACLs, or vice versa.
#[cfg(target_os = "macos")]
fn copy_macos_metadata(
    mut copy: impl FnMut(libc::copyfile_flags_t) -> io::Result<()>,
) -> io::Result<()> {
    for flags in [libc::COPYFILE_ACL, libc::COPYFILE_XATTR] {
        optional_metadata(copy(flags))?;
    }
    Ok(())
}

#[cfg(all(test, target_os = "macos"))]
mod metadata_tests {
    use super::*;

    #[test]
    fn unsupported_usb_metadata_does_not_prevent_copying_other_metadata() {
        for unsupported in [libc::COPYFILE_ACL, libc::COPYFILE_XATTR] {
            let mut calls = Vec::new();
            copy_macos_metadata(|flags| {
                calls.push(flags);
                if flags == unsupported {
                    Err(io::Error::from_raw_os_error(libc::ENOTSUP))
                } else {
                    Ok(())
                }
            })
            .expect("unsupported optional metadata must not fail the data copy");
            assert_eq!(calls, [libc::COPYFILE_ACL, libc::COPYFILE_XATTR]);
        }
        copy_macos_metadata(|_| Err(io::Error::from_raw_os_error(libc::ENOTSUP)))
            .expect("neither metadata class is required on a USB filesystem");
    }

    #[test]
    fn unsupported_basic_permissions_and_times_are_optional_but_real_errors_are_not() {
        optional_metadata(Err(io::Error::from_raw_os_error(libc::ENOTSUP)))
            .expect("FAT permissions must not prevent data publication");
        for errno in [libc::EACCES, libc::EIO, libc::ENOSPC, libc::EROFS] {
            assert_eq!(
                optional_metadata(Err(io::Error::from_raw_os_error(errno)))
                    .unwrap_err()
                    .raw_os_error(),
                Some(errno)
            );
        }
    }

    #[test]
    fn metadata_permission_io_and_space_errors_are_not_ignored() {
        for errno in [
            libc::EACCES,
            libc::EPERM,
            libc::EIO,
            libc::ENOSPC,
            libc::EROFS,
        ] {
            let error = copy_macos_metadata(|_| Err(io::Error::from_raw_os_error(errno)))
                .expect_err("real failures must remain visible");
            assert_eq!(error.raw_os_error(), Some(errno));
        }
    }
}

#[cfg(all(test, unix))]
mod optional_metadata_policy_tests {
    use super::*;
    #[test]
    fn unsupported_metadata_is_optional_on_both_unix_hosts_but_real_errors_remain() {
        for code in [libc::ENOTSUP, libc::EOPNOTSUPP] {
            assert!(optional_metadata(Err(io::Error::from_raw_os_error(code))).is_ok());
        }
        for code in [
            libc::EACCES,
            libc::EPERM,
            libc::ENOSPC,
            libc::EIO,
            libc::EROFS,
        ] {
            assert_eq!(
                optional_metadata(Err(io::Error::from_raw_os_error(code)))
                    .unwrap_err()
                    .raw_os_error(),
                Some(code)
            );
        }
    }
}
