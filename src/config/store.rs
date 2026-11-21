use super::Config;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub(super) const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
pub(crate) fn read_bytes(path: &Path) -> io::Result<Option<Vec<u8>>> {
    use std::io::Read;
    let file = match crate::filesystem::open_regular(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let mut bytes = Vec::new();
    file.take(MAX_CONFIG_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_CONFIG_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Configuration exceeds 1 MiB",
        ));
    }
    Ok(Some(bytes))
}

/// Persistence for one running instance. Explicit loads establish the disk baseline.
/// Writes are locked and checked against it so other instances cannot be overwritten.
pub struct Store {
    path: Option<PathBuf>,
    disk: Option<Vec<u8>>,
    loaded: Option<Config>,
    recovered: bool,
}
impl Store {
    pub fn new(path: Option<PathBuf>) -> Self {
        Self {
            path,
            disk: None,
            loaded: None,
            recovered: false,
        }
    }
    pub fn path(&self) -> io::Result<&Path> {
        self.path
            .as_deref()
            .ok_or_else(|| io::Error::other("Cannot determine the configuration directory"))
    }
    /// True only after a successful load with no settings file or recovery.
    pub fn is_new_installation(&self) -> bool {
        self.loaded.is_some() && self.disk.is_none() && !self.recovered
    }
    pub fn load(&mut self) -> io::Result<Config> {
        let path = self.path()?.to_path_buf();
        let read = || -> io::Result<_> {
            let disk = read_bytes(&path)?;
            let config = match disk.as_ref() {
                Some(bytes) => Config::parse(bytes)?,
                None => Config::default(),
            };
            Ok((disk, config))
        };
        let (disk, config) = match read() {
            Ok(value) => value,
            Err(error) if error.kind() == io::ErrorKind::InvalidData => {
                let _lock = SaveLock::acquire(&path)?;
                // Another instance may have repaired the file before we acquired the lock.
                match read() {
                    Ok(value) => value,
                    Err(error) if error.kind() == io::ErrorKind::InvalidData => {
                        preserve_invalid(&path)?;
                        self.recovered = true;
                        (None, Config::default())
                    }
                    Err(error) => return Err(error),
                }
            }
            Err(error) => return Err(error),
        };
        self.disk = disk;
        self.loaded = Some(config.clone());
        Ok(config)
    }
    /// Publish an explicit, narrowly scoped preference update against the loaded
    /// disk baseline. Unrelated live drafts and the desktop snapshot stay untouched.
    pub(crate) fn update_preferences(
        &mut self,
        update: impl FnOnce(&mut Config) -> io::Result<()>,
    ) -> io::Result<Option<io::Error>> {
        if self.loaded.is_none() {
            self.load()?;
        }
        let mut saved = self.loaded.clone().expect("loaded preferences");
        update(&mut saved)?;
        match self.save(&saved, true) {
            Ok(_) => Ok(None),
            Err(error) => {
                // Publication may have succeeded before parent synchronization
                // failed. Only report retained data when a safe read confirms it.
                if let Ok(Some(bytes)) = read_bytes(self.path()?) {
                    if Config::parse(&bytes).is_ok_and(|present| present == saved) {
                        self.disk = Some(bytes);
                        self.loaded = Some(saved);
                        return Ok(Some(error));
                    }
                }
                Err(error)
            }
        }
    }
    pub fn save(&mut self, config: &Config, force: bool) -> io::Result<bool> {
        if self.loaded.is_none() {
            self.load()?;
        }
        if !force && self.disk.is_some() && self.loaded.as_ref() == Some(config) {
            return Ok(false);
        }
        let path = self.path()?.to_path_buf();
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        let _lock = SaveLock::acquire(&path)?;
        if read_bytes(&path)? != self.disk {
            return Err(io::Error::other(
                "Settings changed outside this instance. Load desktop before saving again",
            ));
        }
        config.save_preserving(&path, self.disk.as_deref())?;
        self.disk = read_bytes(&path)?;
        self.loaded = Some(config.clone());
        Ok(true)
    }
}
/// Move invalid contents aside without overwriting any earlier recovery file.
fn preserve_invalid(path: &Path) -> io::Result<()> {
    let file = crate::filesystem::open_regular(path)?;
    if file.metadata()?.permissions().readonly() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Configuration file is read-only",
        ));
    }
    // Validate the same opened file whose identity we guard before moving it.
    use std::io::Read;
    let mut contents = Vec::new();
    (&file)
        .take(MAX_CONFIG_BYTES + 1)
        .read_to_end(&mut contents)?;
    if Config::parse(&contents).is_ok() {
        return Err(io::Error::other(
            "Settings changed during recovery; reload settings",
        ));
    }
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("Missing settings filename"))?;
    for number in 0..1000 {
        let mut backup = std::ffi::OsString::from(if number == 0 {
            "_old_".to_owned()
        } else {
            format!("_old_{number}_")
        });
        backup.push(name);
        if !crate::platform_fs::owns_file(path, &file) {
            return Err(io::Error::other("Settings changed during recovery"));
        }
        match crate::platform_fs::rename_no_replace(path, &path.with_file_name(backup)) {
            Ok(()) => return crate::platform_fs::sync_parent(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::other("No free settings recovery filename"))
}

pub(crate) struct SaveLock {
    file: fs::File,
    path: PathBuf,
}
impl SaveLock {
    pub(crate) fn acquire(path: &Path) -> io::Result<Self> {
        Self::try_acquire(path).map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                io::Error::other(format!("Settings are locked: {}.lock. If no other copy is saving, remove this stale lock", path.display()))
            } else { error }
        })
    }
    pub(crate) fn try_acquire(path: &Path) -> io::Result<Self> {
        let mut lock_path = path.as_os_str().to_os_string();
        lock_path.push(".lock");
        let path = PathBuf::from(lock_path);
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(&path)?;
        Ok(Self { file, path })
    }
}
impl Drop for SaveLock {
    fn drop(&mut self) {
        if crate::platform_fs::owns_file(&self.path, &self.file) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod scoped_update_tests {
    use super::*;
    #[test]
    fn failed_scoped_save_distinguishes_confirmed_disk_data_from_unwritten_edits() -> io::Result<()>
    {
        let w = crate::network::workspace::Workspace::create(std::env::temp_dir())?;
        let path = w.root.join("config.conf");
        Config::default().save(&path)?;
        let mut store = Store::new(Some(path.clone()));
        store.load()?;
        struct Restore(PathBuf, fs::Permissions);
        impl Drop for Restore {
            fn drop(&mut self) {
                let _ = fs::set_permissions(&self.0, self.1.clone());
            }
        }
        let permissions = fs::metadata(&path)?.permissions();
        let _restore = Restore(path.clone(), permissions.clone());
        let mut readonly = permissions;
        readonly.set_readonly(true);
        fs::set_permissions(&path, readonly)?;
        assert!(store.update_preferences(|_| Ok(()))?.is_some());
        let before = read_bytes(&path)?;
        assert!(store
            .update_preferences(|c| {
                c.show_clock = !c.show_clock;
                Ok(())
            })
            .is_err());
        assert_eq!(read_bytes(&path)?, before);
        Ok(())
    }
}
