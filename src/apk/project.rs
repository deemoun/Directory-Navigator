//! Persistent APK working copies and no-overwrite export. Editable files are
//! separate from the frozen source; protected Android structures never use them.
use super::{manifest::Changes, Package};
use crate::{
    archive::{extract::Destination, reader},
    filesystem::open_regular,
    platform_fs,
    task::Cancellation,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};
#[derive(Clone)]
pub struct Project {
    pub root: PathBuf,
    pub changes: Changes,
    pub catalog: Arc<reader::Catalog>,
}
pub fn editable(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    !name.ends_with('/')
        && lower != "androidmanifest.xml"
        && !lower.starts_with("meta-inf/")
        && !lower.starts_with("lib/")
        && !lower.starts_with("res/")
        && lower != "resources.arsc"
        && !lower.ends_with(".dex")
        && !lower.starts_with(".dn-")
}
pub(super) struct Stage {
    pub path: PathBuf,
    identity: platform_fs::EntryIdentity,
}
impl Stage {
    pub fn new(parent: &Path) -> io::Result<Self> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        guard(parent)?;
        for _ in 0..100 {
            let path = parent.join(format!(
                ".dn-apk-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            let builder = &mut fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&path) {
                Ok(()) => {
                    return Ok(Self {
                        identity: platform_fs::entry_identity(&path)?,
                        path,
                    })
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::other("Cannot create private APK stage"))
    }
    pub fn validate(&self) -> io::Result<()> {
        if platform_fs::entry_identity(&self.path)? != self.identity {
            return Err(io::Error::other("APK stage changed"));
        }
        Ok(())
    }
}
impl Drop for Stage {
    fn drop(&mut self) {
        if self.validate().is_ok() {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
fn write_new(path: &Path, data: &[u8]) -> io::Result<()> {
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    f.write_all(data)?;
    f.sync_all()
}
pub(super) fn copy(
    input: &mut impl Read,
    output: &mut impl Write,
    cancel: &Cancellation,
    limit: u64,
) -> io::Result<u64> {
    let mut buffer = [0; 65536];
    let mut size = 0;
    loop {
        cancel.check()?;
        let n = input.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        size += n as u64;
        if size > limit {
            return Err(io::Error::other("APK data exceeds limit"));
        }
        output.write_all(&buffer[..n])?;
    }
    Ok(size)
}
fn path(root: &Path, name: &str) -> io::Result<PathBuf> {
    let normalized = reader::normalize(name.as_bytes())?;
    if normalized != name.as_bytes() || name.is_empty() {
        return Err(io::Error::other("Invalid project member path"));
    }
    let mut target = root.to_owned();
    for part in name.split('/') {
        crate::archive::extract::component(part.as_bytes())?;
        target.push(part);
    }
    Ok(target)
}
fn workspace_files(root: &Path, cancel: &Cancellation) -> io::Result<BTreeMap<String, PathBuf>> {
    guard(root)?;
    let mut result = BTreeMap::new();
    let mut pending = vec![(root.to_owned(), String::new(), 0)];
    let mut total = 0;
    while let Some((directory, prefix, depth)) = pending.pop() {
        cancel.check()?;
        if depth > 64 {
            return Err(io::Error::other("APK workspace too deep"));
        }
        for entry in fs::read_dir(directory)? {
            cancel.check()?;
            let entry = entry?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| io::Error::other("APK member names must be Unicode"))?;
            crate::archive::extract::component(name.as_bytes())?;
            let name = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            let m = fs::symlink_metadata(entry.path())?;
            if m.is_symlink() {
                return Err(io::Error::other("Workspace links are not accepted"));
            }
            guard(&entry.path())?;
            if m.is_dir() {
                pending.push((entry.path(), name, depth + 1));
            } else if m.is_file() && editable(&name) {
                total += m.len();
                result.insert(name, entry.path());
            } else {
                return Err(io::Error::other(
                    "Protected or special file in APK workspace",
                ));
            }
            if result.len() > reader::MAX_ENTRIES || total > reader::MAX_STREAM {
                return Err(io::Error::other("APK workspace exceeds limits"));
            }
        }
    }
    Ok(result)
}
impl Project {
    pub fn create(package: &Package, root: PathBuf, cancel: &Cancellation) -> io::Result<Self> {
        let receipt = guard(&root)?;
        if root.exists() {
            return Err(io::Error::other("Choose a new project directory"));
        }
        let stage = Stage::new(
            root.parent()
                .ok_or_else(|| io::Error::other("Missing project parent"))?,
        )?;
        let mut source = open_regular(&package.catalog.path)?;
        let mut target = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(stage.path.join("source.apk"))?;
        copy(&mut source, &mut target, cancel, 512 * 1024 * 1024)?;
        target.sync_all()?;
        drop(target);
        drop(source);
        package.catalog.validate()?;
        fs::create_dir(stage.path.join("files"))?;
        let mut files = Vec::new();
        for (id, n) in package.catalog.nodes.iter().enumerate() {
            cancel.check()?;
            if n.kind == reader::Kind::File {
                let name = std::str::from_utf8(&n.path)
                    .map_err(|_| io::Error::other("APK names must be Unicode"))?;
                if editable(name) {
                    let target = path(&stage.path.join("files"), name)?;
                    fs::create_dir_all(target.parent().unwrap())?;
                    files.push((id, target));
                }
            }
        }
        reader::extract_to(&package.catalog, &files, None, cancel, |_, _| {})?;
        write_new(
            &stage.path.join("apk-project.toml"),
            b"format_version = 1\n",
        )?;
        cancel.check()?;
        stage.validate()?;
        receipt.validate()?;
        platform_fs::rename_no_replace(&stage.path, &root)?;
        platform_fs::sync_parent(&root)?;
        Self::open(root, cancel)
    }
    pub fn open(root: PathBuf, cancel: &Cancellation) -> io::Result<Self> {
        guard(&root)?;
        let mut data = String::new();
        open_regular(&root.join("apk-project.toml"))?
            .take(65537)
            .read_to_string(&mut data)?;
        if data.len() > 65536 {
            return Err(io::Error::other("Project metadata exceeds limit"));
        }
        let t = data.parse::<toml::Table>().map_err(io::Error::other)?;
        if t.get("format_version").and_then(toml::Value::as_integer) != Some(1) {
            return Err(io::Error::other("Unsupported APK project version"));
        }
        let changes = Changes {
            debuggable: t
                .get("debuggable")
                .map(|v| {
                    v.as_bool()
                        .ok_or_else(|| io::Error::other("Invalid debuggable value"))
                })
                .transpose()?,
            version_code: t
                .get("version_code")
                .map(|v| {
                    v.as_integer()
                        .and_then(|v| u32::try_from(v).ok())
                        .ok_or_else(|| io::Error::other("Invalid version code"))
                })
                .transpose()?,
            version_name: t
                .get("version_name")
                .map(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| io::Error::other("Invalid version name"))
                })
                .transpose()?,
            permissions: t
                .get("permissions")
                .map(|v| {
                    v.as_array()
                        .ok_or_else(|| io::Error::other("Invalid permissions"))?
                        .iter()
                        .map(|v| {
                            v.as_str()
                                .map(str::to_owned)
                                .ok_or_else(|| io::Error::other("Invalid permission"))
                        })
                        .collect::<io::Result<Vec<_>>>()
                })
                .transpose()?,
        };
        let catalog = Arc::new(reader::list_zip(root.join("source.apk"), cancel)?);
        if catalog.note.is_some() || catalog.skipped != 0 {
            return Err(io::Error::other("Incomplete source APK"));
        }
        workspace_files(&root.join("files"), cancel)?;
        let project = Self {
            root,
            changes,
            catalog,
        };
        if project.manifest(cancel)?.split {
            return Err(io::Error::other("Split APK projects are not supported"));
        }
        Ok(project)
    }
    pub fn manifest(&self, cancel: &Cancellation) -> io::Result<super::manifest::Manifest> {
        let id = self
            .catalog
            .nodes
            .iter()
            .position(|n| n.path == b"AndroidManifest.xml")
            .ok_or_else(|| io::Error::other("Missing manifest"))?;
        let m = super::manifest::Manifest::parse(
            &reader::zip_bytes(&self.catalog, id, 8 * 1024 * 1024, cancel)?,
            cancel,
        )?;
        if self.changes == Changes::default() {
            Ok(m)
        } else {
            super::manifest::Manifest::parse(&m.patch(&self.changes, cancel)?, cancel)
        }
    }
    pub fn save_changes(&mut self, changes: Changes, cancel: &Cancellation) -> io::Result<()> {
        // Validate binary output before committing the project's settings.
        let id = self
            .catalog
            .nodes
            .iter()
            .position(|n| n.path == b"AndroidManifest.xml")
            .ok_or_else(|| io::Error::other("Missing manifest"))?;
        super::manifest::Manifest::parse(
            &reader::zip_bytes(&self.catalog, id, 8 * 1024 * 1024, cancel)?,
            cancel,
        )?
        .patch(&changes, cancel)?;
        guard(&self.root)?;
        let mut table = toml::Table::new();
        table.insert("format_version".into(), 1.into());
        if let Some(v) = changes.debuggable {
            table.insert("debuggable".into(), v.into());
        }
        if let Some(v) = changes.version_code {
            table.insert("version_code".into(), i64::from(v).into());
        }
        if let Some(v) = &changes.version_name {
            table.insert("version_name".into(), v.clone().into());
        }
        if let Some(v) = &changes.permissions {
            table.insert(
                "permissions".into(),
                toml::Value::Array(v.iter().cloned().map(toml::Value::String).collect()),
            );
        }
        let stage = Stage::new(&self.root)?;
        let temp = stage.path.join("metadata");
        write_new(&temp, table.to_string().as_bytes())?;
        cancel.check()?;
        let destination = self.root.join("apk-project.toml");
        let file = open_regular(&destination)?;
        if !platform_fs::owns_file(&destination, &file) {
            return Err(io::Error::other("Project metadata changed"));
        }
        fs::rename(temp, destination)?;
        self.changes = changes;
        Ok(())
    }
    pub fn changes(&self, cancel: &Cancellation) -> io::Result<Vec<String>> {
        let mut working = workspace_files(&self.root.join("files"), cancel)?;
        let mut rows = Vec::new();
        for (id, n) in self.catalog.nodes.iter().enumerate() {
            if n.kind != reader::Kind::File {
                continue;
            }
            let name = std::str::from_utf8(&n.path).map_err(io::Error::other)?;
            if !editable(name) {
                continue;
            }
            if let Some(p) = working.remove(name) {
                let a = reader::zip_bytes(&self.catalog, id, reader::MAX_STREAM, cancel)?;
                let mut f = open_regular(&p)?;
                let mut different = f.metadata()?.len() != a.len() as u64;
                if !different {
                    let mut offset = 0;
                    let mut b = [0; 65536];
                    loop {
                        cancel.check()?;
                        let n = f.read(&mut b)?;
                        if n == 0 {
                            break;
                        }
                        if a.get(offset..offset + n) != Some(&b[..n]) {
                            different = true;
                            break;
                        }
                        offset += n;
                    }
                }
                if different {
                    rows.push(format!("Modified: {name}"));
                }
            } else {
                rows.push(format!("Deleted: {name}"));
            }
        }
        rows.extend(working.keys().map(|n| format!("Added: {n}")));
        if self.changes != Changes::default() {
            rows.push("Modified: AndroidManifest.xml".into());
        }
        Ok(rows)
    }
    pub fn restore(&self, name: &str, cancel: &Cancellation) -> io::Result<()> {
        if name == "AndroidManifest.xml" {
            return Err(io::Error::other(
                "Use Reset manifest to discard manifest changes",
            ));
        }
        if !editable(name) {
            return Err(io::Error::other("Protected APK member"));
        }
        let target = path(&self.root.join("files"), name)?;
        if let Some((id, _)) = self
            .catalog
            .nodes
            .iter()
            .enumerate()
            .find(|(_, n)| n.path == name.as_bytes())
        {
            let data = reader::zip_bytes(&self.catalog, id, reader::MAX_STREAM, cancel)?;
            let stage = Stage::new(&self.root)?;
            let temp = stage.path.join("restore");
            write_new(&temp, &data)?;
            fs::create_dir_all(target.parent().unwrap())?;
            guard(&target)?;
            if target.exists() {
                let _ = open_regular(&target)?;
                fs::rename(temp, target)?;
            } else {
                platform_fs::rename_no_replace(&temp, &target)?;
            }
        } else {
            guard(&target)?;
            let _ = open_regular(&target)?;
            fs::remove_file(target)?;
        }
        Ok(())
    }
    pub fn export(&self, destination: &Path, cancel: &Cancellation) -> io::Result<()> {
        let receipt = guard(destination)?;
        if destination.exists() {
            return Err(io::Error::other("APK destination already exists"));
        }
        let stage = Stage::new(
            destination
                .parent()
                .ok_or_else(|| io::Error::other("Missing export parent"))?,
        )?;
        let output = stage.path.join("unsigned.apk");
        self.build(&output, cancel)?;
        cancel.check()?;
        receipt.validate()?;
        stage.validate()?;
        platform_fs::rename_no_replace(&output, destination)?;
        platform_fs::sync_parent(destination)
    }
    pub(super) fn build(&self, output: &Path, cancel: &Cancellation) -> io::Result<()> {
        self.catalog.validate()?;
        let mut working = workspace_files(&self.root.join("files"), cancel)?;
        let source = open_regular(&self.catalog.path)?;
        let mut zip = zip::ZipArchive::new(source).map_err(io::Error::other)?;
        let target = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output)?;
        let mut writer = zip::ZipWriter::new(target);
        let mut names = BTreeSet::new();
        let mut total = 0;
        for i in 0..zip.len() {
            cancel.check()?;
            let mut entry = zip.by_index(i).map_err(io::Error::other)?;
            let name = entry.name().map_err(io::Error::other)?.into_owned();
            path(&self.root, name.trim_end_matches('/'))?;
            if !names.insert(name.to_ascii_lowercase()) {
                return Err(io::Error::other("Conflicting APK output names"));
            }
            if signature(&name) {
                continue;
            }
            let replacement = working.remove(&name);
            if editable(&name) && !entry.is_dir() && replacement.is_none() {
                continue;
            }
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(entry.compression())
                .unix_permissions(entry.unix_mode().unwrap_or(0o644) & 0o777)
                .last_modified_time(entry.last_modified().unwrap_or_default());
            if entry.is_dir() {
                writer
                    .add_directory(&name, options)
                    .map_err(io::Error::other)?;
                continue;
            }
            writer
                .start_file(&name, options)
                .map_err(io::Error::other)?;
            if name == "AndroidManifest.xml" && self.changes != Changes::default() {
                let mut data = Vec::new();
                entry
                    .by_ref()
                    .take(8 * 1024 * 1024 + 1)
                    .read_to_end(&mut data)?;
                let data = super::manifest::Manifest::parse(&data, cancel)?
                    .patch(&self.changes, cancel)?;
                total += data.len() as u64;
                writer.write_all(&data)?;
            } else if let Some(p) = replacement {
                let mut file = open_regular(&p)?;
                let before = file.metadata()?;
                total += copy(&mut file, &mut writer, cancel, reader::MAX_STREAM)?;
                let after = file.metadata()?;
                if before.len() != after.len()
                    || before.modified().ok() != after.modified().ok()
                    || !platform_fs::owns_file(&p, &file)
                {
                    return Err(io::Error::other("Working file changed during export"));
                }
            } else {
                total += copy(&mut entry, &mut writer, cancel, reader::MAX_STREAM)?;
            }
            if total > reader::MAX_STREAM {
                return Err(io::Error::other("Export exceeds APK size limit"));
            }
        }
        for (name, p) in working {
            cancel.check()?;
            if !names.insert(name.to_ascii_lowercase()) {
                return Err(io::Error::other("Conflicting APK names"));
            }
            writer
                .start_file(
                    name,
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated),
                )
                .map_err(io::Error::other)?;
            total += copy(
                &mut open_regular(&p)?,
                &mut writer,
                cancel,
                reader::MAX_STREAM,
            )?;
            if total > reader::MAX_STREAM {
                return Err(io::Error::other("APK export exceeds limit"));
            }
        }
        writer.finish().map_err(io::Error::other)?.sync_all()?;
        self.catalog.validate()?;
        Ok(())
    }
    pub fn working_path(&self, name: &str) -> io::Result<PathBuf> {
        if !editable(name) {
            return Err(io::Error::other(
                "Extract protected APK files for viewing only",
            ));
        }
        let p = path(&self.root.join("files"), name)?;
        guard(&p)?;
        open_regular(&p)?;
        Ok(p)
    }
}

fn signature(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    let Some(tail) = upper.strip_prefix("META-INF/") else {
        return false;
    };
    !tail.contains('/')
        && (tail == "MANIFEST.MF"
            || tail.starts_with("SIG-")
            || [".SF", ".RSA", ".DSA", ".EC"]
                .iter()
                .any(|ext| tail.ends_with(ext)))
}

fn guard(path: &Path) -> io::Result<Destination> {
    Destination::capture(path)
        .map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", path.display())))
}
