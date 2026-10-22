use super::Config;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
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
    legacy: Option<Vec<u8>>,
}
impl Store {
    pub fn new(path: Option<PathBuf>) -> Self {
        Self {
            path,
            disk: None,
            loaded: None,
            legacy: None,
        }
    }
    pub fn path(&self) -> io::Result<&Path> {
        self.path
            .as_deref()
            .ok_or_else(|| io::Error::other("Cannot determine the configuration directory"))
    }
    pub fn load(&mut self) -> io::Result<Config> {
        let disk = read_bytes(self.path()?)?;
        let legacy = if disk.is_none()
            && self.path()?.file_name() == Some(std::ffi::OsStr::new("config.conf"))
        {
            read_bytes(&self.path()?.with_file_name("config.txt"))?
        } else {
            None
        };
        let config = match disk.as_ref().or(legacy.as_ref()) {
            Some(bytes) => Config::parse(bytes)?,
            None => Config::default(),
        };
        self.disk = disk;
        self.legacy = legacy;
        self.loaded = Some(config.clone());
        Ok(config)
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
        if let Some(legacy) = &self.legacy {
            if read_bytes(&path.with_file_name("config.txt"))?.as_ref() != Some(legacy) {
                return Err(io::Error::other("Legacy settings changed outside this instance. Load desktop before saving again"));
            }
        }
        config.save_preserving(
            &path,
            self.disk
                .as_deref()
                .or(self.legacy.as_deref())
                .unwrap_or_default(),
        )?;
        self.disk = read_bytes(&path)?;
        self.legacy = None;
        self.loaded = Some(config.clone());
        Ok(true)
    }
}
pub(crate) struct SaveLock {
    file: fs::File,
    path: PathBuf,
}
impl SaveLock {
    pub(crate) fn acquire(path: &Path) -> io::Result<Self> {
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
        let file = options.open(&path).map_err(|e| {
            if e.kind() == io::ErrorKind::AlreadyExists {
                io::Error::other(format!(
                    "Settings are locked: {}. If no other copy is saving, remove this stale lock",
                    path.display()
                ))
            } else {
                e
            }
        })?;
        Ok(Self { file, path })
    }
}
impl Drop for SaveLock {
    fn drop(&mut self) {
        if let Ok(metadata) = self.file.metadata() {
            if crate::platform_fs::owns(&self.path, &metadata) {
                let _ = fs::remove_file(&self.path);
            }
        }
    }
}
