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
        match File::open(parent)?.sync_all() {
            Err(e) if matches!(e.raw_os_error(), Some(libc::EINVAL | libc::ENOTSUP)) => Ok(()),
            result => result,
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}
pub fn copy_extended_metadata(source: &File, target: &File) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        use std::os::fd::AsRawFd;
        if unsafe {
            libc::fcopyfile(
                source.as_raw_fd(),
                target.as_raw_fd(),
                std::ptr::null_mut(),
                libc::COPYFILE_ACL | libc::COPYFILE_XATTR,
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;
        let size = unsafe { libc::flistxattr(source.as_raw_fd(), std::ptr::null_mut(), 0) };
        if size < 0 {
            let e = io::Error::last_os_error();
            return if e.raw_os_error() == Some(libc::ENOTSUP) {
                Ok(())
            } else {
                Err(e)
            };
        }
        let mut names = vec![0u8; size as usize];
        let count =
            unsafe { libc::flistxattr(source.as_raw_fd(), names.as_mut_ptr().cast(), names.len()) };
        if count < 0 {
            return Err(io::Error::last_os_error());
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
                return Err(io::Error::last_os_error());
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
                return Err(io::Error::last_os_error());
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
                return Err(io::Error::last_os_error());
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
