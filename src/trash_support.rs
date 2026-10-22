//! Recoverable deletion only. No backend ever falls back to permanent deletion.
use crate::operations::Progress;
use std::{
    fs, io,
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

pub fn move_items(
    paths: &[PathBuf],
    cancel: &AtomicBool,
    progress: &mut Progress,
    mut notify: impl FnMut(&Progress),
    mut backend: impl FnMut(&Path) -> io::Result<()>,
) -> io::Result<()> {
    let check_cancel = || {
        if cancel.load(Ordering::Relaxed) {
            Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Trash operation cancelled; completed items remain in Trash",
            ))
        } else {
            Ok(())
        }
    };
    for path in paths {
        check_cancel()?;
        progress.path = path.clone();
        notify(progress);
        if path.file_name().is_none()
            || path
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Cannot trash a root or parent entry",
            ));
        }
        // Resolve ancestors, never the final component: broken links remain links.
        let absolute = std::path::absolute(path)?;
        let parent = absolute
            .parent()
            .ok_or_else(|| io::Error::other("Missing parent"))?;
        let source = fs::canonicalize(parent)?.join(
            absolute
                .file_name()
                .ok_or_else(|| io::Error::other("Missing filename"))?,
        );
        fs::symlink_metadata(&source)?;
        check_cancel()?;
        backend(&source).map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("Trash failed: {e}\nNo permanent-delete fallback was attempted."),
            )
        })?;
        progress.files += 1;
        notify(progress);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn send(path: &Path) -> io::Result<()> {
    use trash::macos::{DeleteMethod, TrashContextExtMacos};
    // The native wrapper accepts UTF-8 URLs. Refuse lossy conversion of native names.
    if path.to_str().is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "This native filename cannot be represented safely by the macOS Trash backend",
        ));
    }
    let mut context = trash::TrashContext::new();
    context.set_delete_method(DeleteMethod::NsFileManager);
    context.delete(path).map_err(io::Error::other)
}

#[cfg(target_os = "linux")]
pub fn send(path: &Path) -> io::Result<()> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(|h| PathBuf::from(h).join(".local/share"))
                .filter(|p| p.is_absolute())
        })
        .ok_or_else(|| io::Error::other("No home Trash location is configured"))?;
    freedesktop(path, &data.join("Trash"))
}

/// Home-volume FreeDesktop Trash. Cross-volume moves fail instead of copy/delete.
#[cfg(any(target_os = "linux", all(unix, test)))]
pub fn freedesktop(source: &Path, root: &Path) -> io::Result<()> {
    use std::{
        io::Write,
        os::unix::{
            ffi::OsStrExt,
            fs::{DirBuilderExt, MetadataExt},
        },
        sync::atomic::AtomicU64,
    };
    fn directory(path: &Path) -> io::Result<()> {
        match fs::DirBuilder::new().mode(0o700).create(path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.is_dir()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o077 != 0
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Trash directory must be a private owned directory, not a symlink",
            ));
        }
        Ok(())
    }
    if let Some(parent) = root.parent() {
        fs::create_dir_all(parent)?;
    }
    directory(root)?;
    directory(&root.join("files"))?;
    directory(&root.join("info"))?;
    if fs::symlink_metadata(source)?.dev() != fs::metadata(root)?.dev() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Trash on another volume is not supported; source retained",
        ));
    }
    if source.starts_with(root) || root.starts_with(source) {
        return Err(io::Error::other(
            "Cannot trash the Trash directory or its contents",
        ));
    }
    let mut encoded = String::new();
    for byte in source.as_os_str().as_bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~".contains(byte) {
            encoded.push(*byte as char);
        } else {
            use std::fmt::Write;
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    static NEXT: AtomicU64 = AtomicU64::new(0);
    for _ in 0..100 {
        let name = format!(
            "dn-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let info = root.join("info").join(format!("{name}.trashinfo"));
        let mut options = fs::OpenOptions::new();
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = match options.write(true).create_new(true).mode(0o600).open(&info) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        };
        let identity = file.metadata()?;
        let result = (|| {
            writeln!(
                file,
                "[Trash Info]\nPath={encoded}\nDeletionDate={}",
                chrono::Local::now().format("%Y-%m-%dT%H:%M:%S")
            )?;
            file.sync_all()?;
            crate::platform_fs::rename_no_replace(source, &root.join("files").join(&name))
        })();
        if result.is_err() && crate::platform_fs::owns(&info, &identity) {
            let _ = fs::remove_file(&info);
        }
        return result;
    }
    Err(io::Error::other("Cannot reserve a unique Trash entry"))
}

#[cfg(windows)]
pub fn send(path: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        core::PCWSTR,
        Win32::{System::Com::*, UI::Shell::*},
    };
    fn perform(path: &Path) -> windows::core::Result<()> {
        struct Com;
        impl Drop for Com {
            fn drop(&mut self) {
                unsafe {
                    CoUninitialize();
                }
            }
        }
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
            let _com = Com;
            let operation: IFileOperation =
                CoCreateInstance(&FileOperation, None, CLSCTX_INPROC_SERVER)?;
            operation.SetOperationFlags(
                FOF_SILENT
                    | FOF_NOERRORUI
                    | FOF_WANTNUKEWARNING
                    | FOFX_EARLYFAILURE
                    | FOFX_RECYCLEONDELETE
                    | FOF_NO_CONNECTED_ELEMENTS,
            )?;
            // Preserve native UTF-16, including unpaired surrogates.
            use std::{ffi::OsString, path::Prefix};
            let mut parts = path.components();
            let mut shell_path = OsString::new();
            if let Some(first) = parts.next() {
                match first {
                    Component::Prefix(prefix) => match prefix.kind() {
                        Prefix::VerbatimDisk(letter) => {
                            shell_path.push(format!("{}:", letter as char))
                        }
                        Prefix::VerbatimUNC(host, share) => {
                            shell_path.push(r"\\");
                            shell_path.push(host);
                            shell_path.push(r"\");
                            shell_path.push(share);
                        }
                        _ => shell_path.push(first.as_os_str()),
                    },
                    _ => shell_path.push(first.as_os_str()),
                }
            }
            let mut shell_path = PathBuf::from(shell_path);
            for part in parts {
                shell_path.push(part.as_os_str());
            }
            let wide: Vec<u16> = shell_path
                .as_os_str()
                .encode_wide()
                .chain(Some(0))
                .collect();
            let item: IShellItem = SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None)?;
            operation.DeleteItem(&item, None)?;
            operation.PerformOperations()?;
            if operation.GetAnyOperationsAborted()?.as_bool() {
                return Err(windows::core::Error::from_hresult(windows::core::HRESULT(
                    0x800704C7u32 as i32,
                )));
            }
            Ok(())
        }
    }
    perform(path).map_err(io::Error::other)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
pub fn send(_path: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "System Trash is unavailable on this platform",
    ))
}

#[cfg(all(unix, test))]
mod tests {
    use super::*;
    use std::{os::unix::fs::symlink, sync::atomic::AtomicU64};
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> io::Result<Self> {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let p = std::env::temp_dir().join(format!(
                "dn-fdo-trash-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&p)?;
            Ok(Self(p))
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn freedesktop_retains_content_links_native_names_and_restore_information() -> io::Result<()> {
        let temp = Temp::new()?;
        let root = temp.0.join("Trash");
        let outside = temp.0.join("outside");
        fs::write(&outside, "retained")?;
        #[cfg(target_os = "linux")]
        let name = {
            use std::os::unix::ffi::OsStringExt;
            std::ffi::OsString::from_vec(b"folder-\xff\n".to_vec())
        };
        #[cfg(not(target_os = "linux"))]
        let name = std::ffi::OsString::from("folder-文\n");
        let source = temp.0.join(name);
        fs::create_dir(&source)
            .map_err(|e| io::Error::new(e.kind(), format!("create native source: {e}")))?;
        fs::write(source.join("文"), "content")
            .map_err(|e| io::Error::new(e.kind(), format!("write child: {e}")))?;
        symlink(&outside, source.join("link"))?;
        symlink("absent", source.join("broken"))?;
        freedesktop(&source, &root)
            .map_err(|e| io::Error::new(e.kind(), format!("trash source: {e}")))?;
        assert!(!source.exists());
        assert_eq!(fs::read(&outside)?, b"retained");
        let entry = fs::read_dir(root.join("files"))?.next().unwrap()?;
        assert_eq!(fs::read(entry.path().join("文"))?, b"content");
        assert!(fs::symlink_metadata(entry.path().join("link"))?.is_symlink());
        assert_eq!(
            fs::read_link(entry.path().join("broken"))?,
            PathBuf::from("absent")
        );
        let info = fs::read_to_string(
            root.join("info")
                .join(format!("{}.trashinfo", entry.file_name().to_str().unwrap())),
        )?;
        assert!(info.starts_with("[Trash Info]\nPath=/"));
        #[cfg(target_os = "linux")]
        assert!(info.contains("%FF%0A\nDeletionDate="));
        #[cfg(not(target_os = "linux"))]
        assert!(info.contains("%E6%96%87%0A\nDeletionDate="));
        Ok(())
    }
    #[test]
    fn freedesktop_rejects_unsafe_trash_without_removing_source() -> io::Result<()> {
        let temp = Temp::new()?;
        let root = temp.0.join("Trash");
        let other = temp.0.join("other");
        fs::create_dir(&other)?;
        let source = temp.0.join("file");
        fs::write(&source, "safe")?;
        symlink(&other, &root)?;
        assert!(freedesktop(&source, &root).is_err());
        assert_eq!(fs::read(&source)?, b"safe");
        assert_eq!(fs::read_dir(&other)?.count(), 0);
        Ok(())
    }
    #[test]
    fn freedesktop_same_names_never_overwrite_and_failures_leave_no_info() -> io::Result<()> {
        let temp = Temp::new()?;
        let root = temp.0.join("Trash");
        for (dir, text) in [("a", "one"), ("b", "two")] {
            fs::create_dir(temp.0.join(dir))?;
            let source = temp.0.join(dir).join("same");
            fs::write(&source, text)?;
            freedesktop(&source, &root)?;
        }
        let mut contents = fs::read_dir(root.join("files"))?
            .map(|e| fs::read_to_string(e?.path()))
            .collect::<io::Result<Vec<_>>>()?;
        contents.sort();
        assert_eq!(contents, ["one", "two"]);
        assert_eq!(fs::read_dir(root.join("info"))?.count(), 2);
        assert!(freedesktop(&temp.0.join("missing"), &root).is_err());
        assert_eq!(fs::read_dir(root.join("info"))?.count(), 2);
        Ok(())
    }
}
