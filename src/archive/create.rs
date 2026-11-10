//! Built-in ZIP packing, inspired by DN's MakeArchive (RIT Research Labs).
//! Sources remain read-only; only a complete sibling staging file is published.
use super::{extract::Destination, reader, Password};
use crate::{operations::Progress, task::Cancellation};
use std::{
    collections::BTreeSet,
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::SystemTime,
};
#[derive(Clone)]
struct Snapshot {
    path: PathBuf,
    identity: crate::platform_fs::EntryIdentity,
    size: u64,
    modified: Option<SystemTime>,
    directory: bool,
}
impl Snapshot {
    fn capture(path: PathBuf) -> io::Result<Self> {
        let m = fs::symlink_metadata(&path)?;
        if redirected(&m) || (!m.is_file() && !m.is_dir()) {
            return Err(io::Error::other(
                "Packing links and special files is not supported",
            ));
        }
        Ok(Self {
            identity: crate::platform_fs::entry_identity(&path)?,
            path,
            size: m.len(),
            modified: m.modified().ok(),
            directory: m.is_dir(),
        })
    }
    fn validate(&self) -> io::Result<()> {
        let m = fs::symlink_metadata(&self.path)?;
        if redirected(&m)
            || m.is_dir() != self.directory
            || m.len() != self.size
            || m.modified().ok() != self.modified
            || crate::platform_fs::entry_identity(&self.path)? != self.identity
        {
            return Err(io::Error::other(
                "Archive source changed; create the archive again",
            ));
        }
        Ok(())
    }
}
fn redirected(m: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        m.is_symlink() || m.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        m.is_symlink()
    }
}
#[derive(Clone)]
pub struct Creation {
    pub sources: Vec<PathBuf>,
    pub destination: PathBuf,
    roots: Vec<Snapshot>,
    parents: Vec<(PathBuf, crate::platform_fs::EntryIdentity)>,
    receipt: Destination,
    password: Option<Password>,
    existing: Option<std::sync::Arc<reader::Catalog>>,
    removed: BTreeSet<usize>,
    prefix: String,
    member_mode: Option<u32>,
}
impl Creation {
    pub fn capture(
        sources: Vec<PathBuf>,
        destination: PathBuf,
        password: Option<Password>,
    ) -> io::Result<Self> {
        Self::capture_impl(
            sources,
            destination,
            password,
            None,
            BTreeSet::new(),
            String::new(),
        )
    }
    pub fn update(
        catalog: std::sync::Arc<reader::Catalog>,
        sources: Vec<PathBuf>,
        directory: usize,
        removed: BTreeSet<usize>,
        password: Option<Password>,
    ) -> io::Result<Self> {
        if catalog.format != reader::Format::Zip || catalog.note.is_some() || catalog.skipped != 0 {
            return Err(io::Error::other(
                "Only complete ZIP catalogs can be updated",
            ));
        }
        catalog.validate()?;
        let node = catalog
            .nodes
            .get(directory)
            .filter(|n| n.kind == reader::Kind::Directory)
            .ok_or_else(|| io::Error::other("Missing ZIP directory"))?;
        let prefix = std::str::from_utf8(&node.path)
            .map_err(io::Error::other)?
            .to_string();
        if removed
            .iter()
            .any(|id| *id == 0 || *id >= catalog.nodes.len())
        {
            return Err(io::Error::other("Invalid ZIP removal selection"));
        }
        Self::capture_impl(
            sources,
            catalog.path.clone(),
            password,
            Some(catalog),
            removed,
            prefix,
        )
    }
    pub(crate) fn preserving_member_mode(mut self, mode: Option<u32>) -> Self {
        self.member_mode = mode;
        self
    }
    fn capture_impl(
        sources: Vec<PathBuf>,
        destination: PathBuf,
        password: Option<Password>,
        existing: Option<std::sync::Arc<reader::Catalog>>,
        removed: BTreeSet<usize>,
        prefix: String,
    ) -> io::Result<Self> {
        if (sources.is_empty() && existing.is_none())
            || reader::Format::detect(&destination) != Some(reader::Format::Zip)
        {
            return Err(io::Error::other(
                "Select files and choose a .zip archive path",
            ));
        }
        if existing.is_none() && fs::symlink_metadata(&destination).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "Archive destination already exists",
            ));
        }
        if destination
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(io::Error::other(
                "Archive destination must not contain parent components",
            ));
        }
        let receipt = Destination::capture(&destination)?;
        let roots = sources
            .iter()
            .cloned()
            .map(Snapshot::capture)
            .collect::<io::Result<Vec<_>>>()?;
        let mut parents = Vec::new();
        for root in &roots {
            if existing.is_some()
                && crate::platform_fs::entry_identity(&destination)? == root.identity
            {
                return Err(io::Error::other("A ZIP cannot be added to itself"));
            }
            if !root.path.is_absolute()
                || root
                    .path
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                return Err(io::Error::other("Choose absolute archive sources"));
            }
            if root.directory {
                // Compare directory identities as well as spelling: Windows
                // drive/path casing must not bypass the self-inclusion guard.
                for parent in destination.parent().into_iter().flat_map(Path::ancestors) {
                    if crate::platform_fs::entry_identity(parent)? == root.identity {
                        return Err(io::Error::other(
                            "Place the archive outside the selected folders",
                        ));
                    }
                }
            }
            for p in root.path.parent().into_iter().flat_map(Path::ancestors) {
                if redirected(&fs::symlink_metadata(p)?) {
                    return Err(io::Error::other("Archive source parents must not be links"));
                }
                parents.push((p.to_path_buf(), crate::platform_fs::entry_identity(p)?));
            }
        }
        Ok(Self {
            sources,
            destination,
            roots,
            parents,
            receipt,
            password,
            existing,
            removed,
            prefix,
            member_mode: None,
        })
    }
    fn validate(&self) -> io::Result<()> {
        self.receipt.validate()?;
        if let Some(catalog) = &self.existing {
            catalog.validate()?;
        }
        for (path, identity) in &self.parents {
            if crate::platform_fs::entry_identity(path)? != *identity
                || redirected(&fs::symlink_metadata(path)?)
            {
                return Err(io::Error::other("Archive source parent changed"));
            }
        }
        for root in &self.roots {
            root.validate()?;
        }
        Ok(())
    }
    pub(crate) fn run(
        &self,
        cancel: &Cancellation,
        notify: impl FnMut(&Progress),
    ) -> io::Result<Progress> {
        let result = self.run_inner(cancel, notify);
        if result.is_err() {
            cancel.check()?;
        }
        result
    }
    fn run_inner(
        &self,
        cancel: &Cancellation,
        mut notify: impl FnMut(&Progress),
    ) -> io::Result<Progress> {
        self.validate()?;
        let mut entries = Vec::new();
        let mut stack = Vec::new();
        for root in self.roots.iter().rev() {
            let name = root
                .path
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or_else(|| io::Error::other("Archive names must be Unicode"))?;
            stack.push((
                root.path.clone(),
                if self.prefix.is_empty() {
                    name.to_string()
                } else {
                    format!("{}/{name}", self.prefix)
                },
            ));
        }
        let mut keys = BTreeSet::new();
        let mut total = 0u64;
        let mut metadata_bytes = 0usize;
        while let Some((path, name)) = stack.pop() {
            cancel.check()?;
            if entries.len() >= reader::MAX_ENTRIES {
                return Err(io::Error::other("Archive exceeds 100,000 entries"));
            }
            reader::normalize(name.as_bytes())?;
            for part in name.split('/') {
                super::extract::component(part.as_bytes())?;
            }
            if !keys.insert(name.to_lowercase()) {
                return Err(io::Error::other("Conflicting archive member names"));
            }
            metadata_bytes += name.len() * 2 + 128;
            if metadata_bytes > reader::MAX_METADATA {
                return Err(io::Error::other("Archive metadata limit reached"));
            }
            let snapshot = Snapshot::capture(path.clone())?;
            if snapshot.directory {
                let mut children = Vec::new();
                for entry in fs::read_dir(&path)? {
                    cancel.check()?;
                    if children.len() + entries.len() + stack.len() >= reader::MAX_ENTRIES {
                        return Err(io::Error::other("Archive exceeds 100,000 entries"));
                    }
                    let e = entry?;
                    let leaf = e
                        .file_name()
                        .into_string()
                        .map_err(|_| io::Error::other("Archive names must be Unicode"))?;
                    children.push((e.path(), format!("{name}/{leaf}")));
                }
                children.sort_by(|a, b| a.1.cmp(&b.1));
                stack.extend(children.into_iter().rev());
            } else {
                total = total
                    .checked_add(snapshot.size)
                    .filter(|n| *n <= reader::MAX_STREAM)
                    .ok_or_else(|| {
                        io::Error::other("Archive input exceeds the 64 GiB creation limit")
                    })?;
            }
            entries.push((snapshot, name));
        }
        self.validate()?;
        let parent = self.destination.parent().unwrap();
        if crate::volume_space::read(parent)?.available
            < u128::from(total)
                + self.existing.as_ref().map_or(0, |c| {
                    fs::metadata(&c.path).map_or(0, |m| u128::from(m.len()))
                })
                + 2 * 1024 * 1024
        {
            return Err(io::Error::other("Insufficient space for archive creation"));
        }
        let replacement = self
            .existing
            .as_ref()
            .map(|_| {
                crate::operations::conflict::Approved::capture(
                    &self.destination,
                    fs::symlink_metadata(&self.destination)?,
                )
            })
            .transpose()?;
        let mut retained = Vec::new();
        let mut existing_names = BTreeSet::new();
        if let Some(catalog) = &self.existing {
            if catalog.nodes.iter().any(|n| n.locked) && !entries.is_empty() {
                let password = self.password.as_ref().ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "Enter the archive password before updating encrypted ZIP contents",
                    )
                })?;
                let ids = catalog
                    .nodes
                    .iter()
                    .enumerate()
                    .filter(|(_, n)| n.locked && n.kind == reader::Kind::File)
                    .take(1)
                    .map(|(id, _)| id)
                    .collect::<Vec<_>>();
                reader::check_password(catalog, &ids, password, cancel)?;
            }
            let added = entries
                .iter()
                .map(|(snapshot, name)| (name.to_lowercase(), (name.as_str(), snapshot.directory)))
                .collect::<std::collections::HashMap<_, _>>();
            for (id, node) in catalog.nodes.iter().enumerate().skip(1) {
                let deleted = self.removed.iter().any(|root| {
                    id == *root
                        || node
                            .path
                            .starts_with(&[catalog.nodes[*root].path.as_slice(), b"/"].concat())
                });
                if deleted {
                    continue;
                }
                let key = std::str::from_utf8(&node.path)
                    .map_err(io::Error::other)?
                    .to_lowercase();
                if !existing_names.insert(key.clone()) {
                    return Err(io::Error::other(
                        "ZIP member names collide across platforms",
                    ));
                }
                if let Some((name, directory)) = added.get(&key) {
                    if name.as_bytes() != node.path {
                        return Err(io::Error::other(
                            "ZIP member names differ only in case; use the original spelling",
                        ));
                    }
                    if !matches!(node.kind, reader::Kind::File | reader::Kind::Directory) {
                        return Err(io::Error::other(
                            "Replacing ZIP links or special members is not supported",
                        ));
                    }
                    if *directory != (node.kind == reader::Kind::Directory) {
                        return Err(io::Error::other("ZIP file/directory conflict"));
                    }
                    continue;
                }
                if let Some(index) = node.source {
                    retained.push(index);
                }
            }
            if retained.len() + entries.len() > reader::MAX_ENTRIES {
                return Err(io::Error::other("Updated ZIP exceeds 100,000 entries"));
            }
        }
        let stage = crate::publication::StagedFile::create(&self.destination)?;
        let mut writer = zip::ZipWriter::new(stage.file.try_clone()?);
        if let Some(catalog) = &self.existing {
            let (file, metadata) = catalog.checked_source()?;
            let mut archive = reader::zip_open_for(file, &metadata, cancel, true)?;
            writer.set_raw_comment(archive.comment().to_vec().into())?;
            for index in retained {
                cancel.check()?;
                writer.raw_copy_file(archive.by_index_raw(index)?)?;
            }
        }
        let mut progress = Progress {
            path: self.destination.clone(),
            total_bytes: Some(total),
            phase: crate::operations::Phase::Copying,
            ..Default::default()
        };
        notify(&progress);
        for (snapshot, name) in &entries {
            cancel.check()?;
            snapshot.validate()?;
            let mut options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
                .large_file(snapshot.size >= u64::from(u32::MAX));
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                options = options.unix_permissions(
                    fs::symlink_metadata(&snapshot.path)?.permissions().mode() & 0o777,
                );
            }
            if let Some(mode) = self.member_mode {
                options = options.unix_permissions(mode & 0o777);
            }
            if let Some(time) = snapshot.modified {
                use chrono::{Datelike, Timelike};
                let t: chrono::DateTime<chrono::Local> = time.into();
                if let Ok(t) = zip::DateTime::from_date_and_time(
                    t.year().clamp(1980, 2107) as u16,
                    t.month() as u8,
                    t.day() as u8,
                    t.hour() as u8,
                    t.minute() as u8,
                    t.second().min(59) as u8,
                ) {
                    options = options.last_modified_time(t);
                }
            }
            progress.member = Some(name.clone());
            if snapshot.directory {
                writer.add_directory(format!("{name}/"), options)?;
            } else {
                if let Some(password) = &self.password {
                    options =
                        options.with_aes_encryption_bytes(zip::AesMode::Aes256, password.bytes());
                }
                writer.start_file(name, options)?;
                let mut file = crate::filesystem::open_regular(&snapshot.path)?;
                if !crate::platform_fs::owns_file(&snapshot.path, &file) {
                    return Err(io::Error::other("Archive source changed during open"));
                }
                snapshot.validate()?;
                let mut read = 0u64;
                let mut buffer = [0; 64 * 1024];
                loop {
                    cancel.check()?;
                    let n = file.read(&mut buffer)?;
                    if n == 0 {
                        break;
                    }
                    read += n as u64;
                    if read > snapshot.size {
                        return Err(io::Error::other("Archive source grew during packing"));
                    }
                    writer.write_all(&buffer[..n])?;
                    progress.bytes += n as u64;
                    progress.written += n as u64;
                    notify(&progress);
                }
                if read != snapshot.size {
                    return Err(io::Error::other("Archive source shrank during packing"));
                }
                snapshot.validate()?;
                progress.files += 1;
            }
        }
        writer.finish()?.sync_all()?;
        if stage.file.metadata()?.len() > 64 * 1024 * 1024 * 1024 {
            return Err(io::Error::other(
                "Created archive exceeds the 64 GiB browser limit",
            ));
        }
        // Validate the complete generated catalog before making the new name
        // visible; combined old/new metadata can exceed a limit independently.
        let prepared = reader::list_zip(stage.path().to_path_buf(), cancel)?;
        if prepared.note.is_some() || prepared.skipped != 0 {
            return Err(io::Error::other(
                "Updated ZIP has an incomplete or unsafe catalog",
            ));
        }
        cancel.check()?;
        self.validate()?;
        for (snapshot, _) in &entries {
            cancel.check()?;
            snapshot.validate()?;
        }
        self.receipt.validate()?;
        if let Some(catalog) = &self.existing {
            let (source, _) = catalog.checked_source()?;
            crate::platform_fs::copy_extended_metadata(&source, &stage.file)?;
            stage
                .file
                .set_permissions(source.metadata()?.permissions())?;
            stage.file.sync_all()?;
        }
        if let Some(approved) = &replacement {
            approved.verify(&self.destination)?;
            if !crate::platform_fs::owns_file(stage.path(), &stage.file) {
                return Err(io::Error::other("Staging file changed"));
            }
            std::fs::rename(stage.path(), &self.destination)?;
        } else {
            stage.publish_unsynced(&self.destination)?;
        }
        progress.member = None;
        // Publication has succeeded. A durability warning must not claim that
        // the archive was never created or invite an unsafe retry.
        if let Err(e) = crate::platform_fs::sync_parent(&self.destination) {
            progress.detail(
                self.destination.clone(),
                format!("Archive created; directory synchronization failed: {e}"),
            );
        }
        Ok(progress)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    };
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> io::Result<Self> {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "dn-pack-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path)?;
            let t = Self(fs::canonicalize(path)?);
            fs::create_dir(t.0.join("docs"))?;
            fs::create_dir(t.0.join("docs/empty"))?;
            fs::write(t.0.join("docs/文.txt"), b"private archive contents\n")?;
            fs::write(t.0.join("docs/.hidden"), b"hidden")?;
            Ok(t)
        }
        fn request(&self, password: Option<Password>) -> io::Result<Creation> {
            Creation::capture(vec![self.0.join("docs")], self.0.join("new.zip"), password)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn normal_and_aes_archives_round_trip_unicode_empty_directories_and_hidden_files(
    ) -> io::Result<()> {
        for encrypted in [false, true] {
            let t = Temp::new()?;
            let password = encrypted.then(|| Password::new("páss 界"));
            let p = t
                .request(password.clone())?
                .run(&Cancellation::default(), |_| {})?;
            assert_eq!(p.files, 2);
            let c = reader::list(t.0.join("new.zip"), &Cancellation::default())?;
            assert_eq!(c.nodes.len(), 5);
            let id = c
                .nodes
                .iter()
                .position(|n| n.path == "docs/文.txt".as_bytes())
                .unwrap();
            assert_eq!(c.nodes[id].locked, encrypted);
            if encrypted {
                assert!(reader::preview(&c, id, &Cancellation::default()).is_err());
                assert!(reader::check_password(
                    &c,
                    &[id],
                    &Password::new("wrong"),
                    &Cancellation::default()
                )
                .is_err());
                reader::check_password(
                    &c,
                    &[id],
                    password.as_ref().unwrap(),
                    &Cancellation::default(),
                )?;
                let bytes = fs::read(&c.path)?;
                assert!(!bytes
                    .windows(b"private archive contents".len())
                    .any(|w| w == b"private archive contents"));
            }
            reader::preview_with_password(&c, id, password.as_ref(), &Cancellation::default())?;
            let c = Arc::new(c);
            fs::create_dir(t.0.join("out"))?;
            let extraction = super::super::Extraction {
                directory: 0,
                members: vec![crate::panel::archive::MemberId {
                    generation: c.generation,
                    index: 1,
                }],
                catalog: c,
                password,
            };
            extraction.run(
                &Destination::capture(&t.0.join("out"))?,
                Default::default(),
                Arc::new(AtomicBool::new(false)),
                |_| {},
            )?;
            assert_eq!(
                fs::read(t.0.join("out/docs/文.txt"))?,
                b"private archive contents\n"
            );
            assert!(t.0.join("out/docs/empty").is_dir());
            assert_eq!(
                fs::read(t.0.join("docs/文.txt"))?,
                b"private archive contents\n"
            );
        }
        Ok(())
    }
    #[test]
    fn cancelled_packing_removes_staging_and_keeps_originals() -> io::Result<()> {
        let t = Temp::new()?;
        let cancel = Cancellation::default();
        let result = t.request(Some(Password::new("secret")))?.run(&cancel, |p| {
            if p.written > 0 {
                cancel.cancel();
            }
        });
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Interrupted);
        assert!(!t.0.join("new.zip").exists());
        assert_eq!(fs::read_dir(&t.0)?.count(), 1);
        assert!(t.0.join("docs/文.txt").is_file());
        Ok(())
    }
    #[test]
    fn source_mutation_or_replacement_and_destination_races_publish_nothing() -> io::Result<()> {
        for change_source in [true, false] {
            let t = Temp::new()?;
            let mut changed = false;
            let result = t.request(None)?.run(&Cancellation::default(), |p| {
                if p.written > 0 && !changed {
                    changed = true;
                    if change_source {
                        fs::write(t.0.join("docs/文.txt"), b"changed").unwrap();
                    } else {
                        fs::write(t.0.join("new.zip"), b"winner").unwrap();
                    }
                }
            });
            assert!(result.is_err());
            if change_source {
                assert!(!t.0.join("new.zip").exists());
            } else {
                assert_eq!(fs::read(t.0.join("new.zip"))?, b"winner");
            }
            assert!(!fs::read_dir(&t.0)?.any(|e| e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".dn-part-")));
        }
        let t = Temp::new()?;
        let request = t.request(None)?;
        fs::rename(t.0.join("docs"), t.0.join("old"))?;
        fs::create_dir(t.0.join("docs"))?;
        assert!(request.run(&Cancellation::default(), |_| {}).is_err());
        Ok(())
    }
    #[test]
    fn output_inside_source_existing_output_and_portable_name_collisions_are_rejected(
    ) -> io::Result<()> {
        let t = Temp::new()?;
        assert!(
            Creation::capture(vec![t.0.join("docs")], t.0.join("docs/self.zip"), None).is_err()
        );
        fs::write(t.0.join("new.zip"), b"keep")?;
        assert!(t.request(None).is_err());
        assert_eq!(fs::read(t.0.join("new.zip"))?, b"keep");
        fs::create_dir(t.0.join("other"))?;
        fs::write(t.0.join("other/文.txt"), b"different")?;
        let r = Creation::capture(
            vec![t.0.join("docs/文.txt"), t.0.join("other/文.txt")],
            t.0.join("pair.zip"),
            None,
        )?;
        assert!(r.run(&Cancellation::default(), |_| {}).is_err());
        assert!(!t.0.join("pair.zip").exists());
        Ok(())
    }
    #[cfg(unix)]
    #[test]
    fn links_and_redirected_source_or_destination_parents_are_rejected() -> io::Result<()> {
        use std::os::unix::fs::symlink;
        let t = Temp::new()?;
        symlink(t.0.join("docs"), t.0.join("alias"))?;
        assert!(
            Creation::capture(vec![t.0.join("alias/文.txt")], t.0.join("new.zip"), None).is_err()
        );
        assert!(Creation::capture(
            vec![t.0.join("docs/文.txt")],
            t.0.join("alias/new.zip"),
            None
        )
        .is_err());
        symlink(t.0.join("docs/文.txt"), t.0.join("docs/link"))?;
        assert!(t
            .request(None)?
            .run(&Cancellation::default(), |_| {})
            .is_err());
        assert!(!t.0.join("new.zip").exists());
        Ok(())
    }
    #[test]
    fn bad_password_or_corrupt_authenticated_payload_never_publishes_extracted_files(
    ) -> io::Result<()> {
        use std::io::{Seek, SeekFrom};
        let t = Temp::new()?;
        let password = Password::new("correct");
        t.request(Some(password.clone()))?
            .run(&Cancellation::default(), |_| {})?;
        let path = t.0.join("new.zip");
        let mut zip = zip::ZipArchive::new(fs::File::open(&path)?)?;
        let index = zip.index_for_name("docs/文.txt").unwrap();
        let raw = zip.by_index_raw(index)?;
        let at = raw.data_start().unwrap();
        let size = raw.compressed_size();
        drop(raw);
        drop(zip);
        let mut file = fs::OpenOptions::new().read(true).write(true).open(&path)?;
        file.seek(SeekFrom::Start(at + size - 1))?;
        let mut b = [0];
        file.read_exact(&mut b)?;
        b[0] ^= 1;
        file.seek(SeekFrom::Start(at + size - 1))?;
        file.write_all(&b)?;
        drop(file);
        let c = Arc::new(reader::list(path, &Cancellation::default())?);
        let id = c
            .nodes
            .iter()
            .position(|n| n.path == "docs/文.txt".as_bytes())
            .unwrap();
        assert!(
            reader::preview_with_password(&c, id, Some(&password), &Cancellation::default())
                .is_err()
        );
        fs::create_dir(t.0.join("out"))?;
        let x = super::super::Extraction {
            directory: 0,
            members: vec![crate::panel::archive::MemberId {
                generation: c.generation,
                index: 1,
            }],
            catalog: c,
            password: Some(password),
        };
        assert!(x
            .run(
                &Destination::capture(&t.0.join("out"))?,
                Default::default(),
                Arc::new(AtomicBool::new(false)),
                |_| {}
            )
            .is_err());
        assert!(fs::read_dir(t.0.join("out"))?.next().is_none());
        Ok(())
    }
}

#[cfg(test)]
mod upgrade_tests {
    use super::*;
    use crate::archive::test_support::{self, Temp};
    use std::sync::Arc;
    #[test]
    fn update_replaces_adds_and_removes_without_changing_unrelated_members() -> io::Result<()> {
        let t = Temp::new()?;
        let path = t.0.join("fixture.zip");
        test_support::zip(
            &path,
            &[
                ("a.txt", b"old"),
                ("keep.txt", b"keep"),
                ("docs/old.txt", b"remove"),
            ],
            None,
        )?;
        let catalog = Arc::new(reader::list(path.clone(), &Cancellation::default())?);
        let id = catalog
            .nodes
            .iter()
            .position(|n| n.path == b"docs")
            .unwrap();
        fs::write(t.0.join("a.txt"), "new Ёжик")?;
        fs::create_dir(t.0.join("empty"))?;
        let update = Creation::update(
            catalog,
            vec![t.0.join("a.txt"), t.0.join("empty")],
            0,
            BTreeSet::from([id]),
            None,
        )?;
        update.run(&Cancellation::default(), |_| {})?;
        let catalog = reader::list(path.clone(), &Cancellation::default())?;
        assert!(!catalog.nodes.iter().any(|n| n.path.starts_with(b"docs")));
        let id = catalog
            .nodes
            .iter()
            .position(|n| n.path == b"a.txt")
            .unwrap();
        assert_eq!(
            reader::preview(&catalog, id, &Cancellation::default())?
                .source
                .as_deref(),
            Some("new Ёжик")
        );
        assert!(catalog.nodes.iter().any(|n| n.path == b"empty"));
        let mut zip = zip::ZipArchive::new(fs::File::open(path)?)?;
        assert_eq!(zip.comment(), b"fixture comment");
        let mut keep = String::new();
        zip.by_name("keep.txt")?.read_to_string(&mut keep)?;
        assert_eq!(keep, "keep");
        Ok(())
    }
    #[test]
    fn folder_merge_keeps_children_and_case_only_conflicts_keep_original() -> io::Result<()> {
        let t = Temp::new()?;
        let path = t.0.join("merge.zip");
        test_support::zip(&path, &[("docs/keep.txt", b"keep")], None)?;
        fs::create_dir(t.0.join("docs"))?;
        fs::write(t.0.join("docs/new.txt"), "new")?;
        let catalog = Arc::new(reader::list(path.clone(), &Cancellation::default())?);
        Creation::update(catalog, vec![t.0.join("docs")], 0, Default::default(), None)?
            .run(&Cancellation::default(), |_| {})?;
        let catalog = Arc::new(reader::list(path.clone(), &Cancellation::default())?);
        assert!(catalog.nodes.iter().any(|n| n.path == b"docs/keep.txt"));
        assert!(catalog.nodes.iter().any(|n| n.path == b"docs/new.txt"));
        fs::create_dir(t.0.join("case"))?;
        fs::create_dir(t.0.join("case/Docs"))?;
        let bytes = fs::read(&path)?;
        assert!(Creation::update(
            catalog,
            vec![t.0.join("case/Docs")],
            0,
            Default::default(),
            None
        )?
        .run(&Cancellation::default(), |_| {})
        .is_err());
        assert_eq!(bytes, fs::read(path)?);
        Ok(())
    }
    #[test]
    fn cancelled_or_stale_update_keeps_original_and_cleans_staging() -> io::Result<()> {
        let t = Temp::new()?;
        let path = t.0.join("fixture.zip");
        test_support::zip(&path, &[("a.txt", b"old")], None)?;
        let bytes = fs::read(&path)?;
        let catalog = Arc::new(reader::list(path.clone(), &Cancellation::default())?);
        fs::write(t.0.join("a.txt"), "new")?;
        let update = Creation::update(
            catalog,
            vec![t.0.join("a.txt")],
            0,
            Default::default(),
            None,
        )?;
        let token = Cancellation::default();
        token.cancel();
        assert!(update.run(&token, |_| {}).is_err());
        assert_eq!(fs::read(&path)?, bytes);
        test_support::zip(&path, &[("a.txt", b"external change")], None)?;
        let changed = fs::read(&path)?;
        assert!(update.run(&Cancellation::default(), |_| {}).is_err());
        assert_eq!(fs::read(&path)?, changed);
        assert!(!fs::read_dir(&t.0)?.any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".dn-part")));
        Ok(())
    }
    #[test]
    fn removal_keeps_encrypted_members_raw_and_addition_requires_correct_password() -> io::Result<()>
    {
        let t = Temp::new()?;
        let path = t.0.join("locked.zip");
        test_support::zip(
            &path,
            &[("keep.txt", b"secret"), ("remove.txt", b"old")],
            Some("fixture"),
        )?;
        let catalog = Arc::new(reader::list(path.clone(), &Cancellation::default())?);
        let id = catalog
            .nodes
            .iter()
            .position(|n| n.path == b"remove.txt")
            .unwrap();
        Creation::update(catalog, Vec::new(), 0, BTreeSet::from([id]), None)?
            .run(&Cancellation::default(), |_| {})?;
        let catalog = Arc::new(reader::list(path.clone(), &Cancellation::default())?);
        let id = catalog
            .nodes
            .iter()
            .position(|n| n.path == b"keep.txt")
            .unwrap();
        assert!(catalog.nodes[id].locked);
        assert_eq!(
            reader::preview_with_password(
                &catalog,
                id,
                Some(&Password::new("fixture")),
                &Cancellation::default()
            )?
            .source
            .as_deref(),
            Some("secret")
        );
        fs::write(t.0.join("new.txt"), "new")?;
        let before = fs::read(&path)?;
        assert!(Creation::update(
            catalog.clone(),
            vec![t.0.join("new.txt")],
            0,
            Default::default(),
            Some(Password::new("wrong"))
        )?
        .run(&Cancellation::default(), |_| {})
        .is_err());
        assert_eq!(before, fs::read(&path)?);
        Creation::update(
            catalog,
            vec![t.0.join("new.txt")],
            0,
            Default::default(),
            Some(Password::new("fixture")),
        )?
        .run(&Cancellation::default(), |_| {})?;
        assert!(
            reader::list(path, &Cancellation::default())?
                .nodes
                .iter()
                .find(|n| n.path == b"new.txt")
                .unwrap()
                .locked
        );
        Ok(())
    }
}
