//! Private, versioned checkpoints. Source paths are descriptive, never write targets.
use super::Record;
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};
const LIMIT: u64 = 10 * 1024 * 1024;
fn invalid() -> io::Error {
    io::Error::other("Invalid or unsafe editor recovery data")
}
fn private(path: &Path, directory: bool) -> io::Result<()> {
    let m = fs::symlink_metadata(path)?;
    if m.file_type().is_symlink() || if directory { !m.is_dir() } else { !m.is_file() } {
        return Err(invalid());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if m.uid() != unsafe { libc::geteuid() }
            || m.mode() & 0o077 != 0
            || (!directory && m.nlink() != 1)
        {
            return Err(invalid());
        }
    }
    Ok(())
}
fn directory(path: &Path) -> io::Result<()> {
    let b = &mut fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        b.mode(0o700);
    }
    match b.create(path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    private(path, true)
}
pub struct Lease {
    pub dir: PathBuf,
    _file: File,
}
pub struct Store {
    pub current: Arc<Lease>,
    pub pending: std::collections::VecDeque<PathBuf>,
    _claims: Vec<Arc<Lease>>,
    pub warnings: usize,
}
impl Store {
    pub fn open(root: &Path) -> io::Result<Self> {
        fs::create_dir_all(root.parent().ok_or_else(invalid)?)?;
        directory(root)?;
        let mut claims = Vec::new();
        let mut pending = Vec::new();
        let mut warnings = 0;
        // Bound startup work; unvisited sessions remain untouched for a later run.
        for entry in fs::read_dir(root)?.take(1024) {
            let entry = entry?;
            let dir = entry.path();
            if !entry.file_name().to_string_lossy().starts_with("session-") {
                continue;
            }
            let claim = || -> io::Result<Arc<Lease>> {
                private(&dir, true)?;
                let path = dir.join("lease");
                private(&path, false)?;
                let file = crate::filesystem::open_regular(&path)?;
                file.try_lock()
                    .map_err(|e| io::Error::other(e.to_string()))?;
                Ok(Arc::new(Lease {
                    dir: dir.clone(),
                    _file: file,
                }))
            };
            let Ok(lease) = claim() else {
                continue;
            }; // Live sessions are never offered.
            for file in fs::read_dir(&dir)?.take(1024) {
                let file = file?;
                if file.path().extension().is_some_and(|s| s == "rec") {
                    if private(&file.path(), false).is_ok() {
                        pending.push(file.path());
                    } else {
                        warnings += 1;
                    }
                }
            }
            claims.push(lease);
        }
        pending.sort();
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let dir = root.join(format!(
            "session-{stamp}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        directory(&dir)?;
        let mut o = OpenOptions::new();
        o.read(true).write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(0o600);
        }
        let file = o.open(dir.join("lease"))?;
        file.try_lock()
            .map_err(|e| io::Error::other(e.to_string()))?;
        crate::platform_fs::sync_parent(&dir.join("lease"))?;
        crate::platform_fs::sync_parent(&dir)?;
        Ok(Self {
            current: Arc::new(Lease { dir, _file: file }),
            pending: pending.into(),
            _claims: claims,
            warnings,
        })
    }
    pub fn leases(&self) -> Vec<Arc<Lease>> {
        self._claims
            .iter()
            .cloned()
            .chain(std::iter::once(self.current.clone()))
            .collect()
    }
    pub fn path(&self, id: crate::desktop::WindowId) -> PathBuf {
        self.current.dir.join(format!("{id}.rec"))
    }
    pub fn clear_current(&self) -> io::Result<()> {
        for e in fs::read_dir(&self.current.dir)? {
            let p = e?.path();
            if p.extension().is_some_and(|s| s == "rec") {
                remove(&p)?;
            }
        }
        Ok(())
    }
}
fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    })
}
pub fn write(path: &Path, record: &Record) -> io::Result<()> {
    if record.text.len() > super::super::buffer::MAX_BYTES
        || !record.base.is_absolute()
        || record.path.as_ref().is_some_and(|p| !p.is_absolute())
        || record.cursor > record.text.len()
        || !record.text.is_char_boundary(record.cursor)
        || record
            .anchor
            .is_some_and(|a| a > record.text.len() || !record.text.is_char_boundary(a))
    {
        return Err(invalid());
    }
    private(path.parent().ok_or_else(invalid)?, true)?;
    if path.try_exists()? {
        private(path, false)?;
    }
    let mut t = toml::Table::new();
    t.insert("text".into(), record.text.clone().into());
    t.insert("base".into(), crate::bookmarks::encode(&record.base).into());
    if let Some(p) = &record.path {
        t.insert("source".into(), crate::bookmarks::encode(p).into());
    }
    t.insert("cursor".into(), (record.cursor as i64).into());
    if let Some(a) = record.anchor {
        t.insert("anchor".into(), (a as i64).into());
    }
    t.insert("bom".into(), record.bom.into());
    t.insert("time".into(), record.time.into());
    let body = toml::to_string(&t).map_err(io::Error::other)?;
    if body.len() as u64 > LIMIT {
        return Err(invalid());
    }
    let mut staged = crate::publication::StagedFile::create(path)?;
    write!(
        staged.file,
        "DNREC1\n{:016x}\n{body}",
        hash(body.as_bytes())
    )?;
    staged.file.sync_all()?;
    if !crate::platform_fs::owns_file(staged.path(), &staged.file) {
        return Err(invalid());
    }
    fs::rename(staged.path(), path)?;
    crate::platform_fs::sync_parent(path)
}
pub fn read(path: &Path) -> io::Result<Record> {
    private(path.parent().ok_or_else(invalid)?, true)?;
    private(path, false)?;
    let mut body = String::new();
    crate::filesystem::open_regular(path)?
        .take(LIMIT + 1)
        .read_to_string(&mut body)?;
    if body.len() as u64 > LIMIT {
        return Err(invalid());
    }
    let mut parts = body.splitn(3, '\n');
    if parts.next() != Some("DNREC1") {
        return Err(invalid());
    }
    let sum = parts
        .next()
        .and_then(|s| u64::from_str_radix(s, 16).ok())
        .ok_or_else(invalid)?;
    let body = parts.next().ok_or_else(invalid)?;
    if hash(body.as_bytes()) != sum {
        return Err(invalid());
    }
    let t = body.parse::<toml::Table>().map_err(io::Error::other)?;
    let string = |key| t.get(key).and_then(toml::Value::as_str).ok_or_else(invalid);
    let text = string("text")?.to_owned();
    super::super::buffer::Buffer::new(text.clone()).map_err(io::Error::other)?;
    let base = crate::bookmarks::decode(string("base")?)
        .filter(|p| p.is_absolute())
        .ok_or_else(invalid)?;
    let path = t
        .get("source")
        .map(|v| {
            v.as_str()
                .and_then(crate::bookmarks::decode)
                .filter(|p| p.is_absolute())
                .ok_or_else(invalid)
        })
        .transpose()?;
    let position = |key| {
        t.get(key)
            .and_then(toml::Value::as_integer)
            .and_then(|v| usize::try_from(v).ok())
            .filter(|v| *v <= text.len() && text.is_char_boundary(*v))
            .ok_or_else(invalid)
    };
    let cursor = position("cursor")?;
    let anchor = t
        .contains_key("anchor")
        .then(|| position("anchor"))
        .transpose()?;
    let bom = t
        .get("bom")
        .and_then(toml::Value::as_bool)
        .ok_or_else(invalid)?;
    let time = t
        .get("time")
        .and_then(toml::Value::as_integer)
        .ok_or_else(invalid)?;
    Ok(Record {
        text,
        base,
        path,
        cursor,
        anchor,
        bom,
        time,
    })
}
pub fn remove(path: &Path) -> io::Result<()> {
    match private(path, false) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    }
    fs::remove_file(path)?;
    crate::platform_fs::sync_parent(path)
}

impl Drop for Lease {
    fn drop(&mut self) {
        // Retain every checkpoint and unknown entry. Only empty sessions are tidied.
        if fs::read_dir(&self.dir)
            .is_ok_and(|mut entries| entries.all(|e| e.is_ok_and(|e| e.file_name() == "lease")))
            && crate::platform_fs::owns_file(&self.dir.join("lease"), &self._file)
        {
            let _ = fs::remove_file(self.dir.join("lease"));
            let _ = fs::remove_dir(&self.dir);
        }
    }
}
