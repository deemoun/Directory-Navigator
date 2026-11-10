//! Internal 7z/RAR adapters. Paths are interpreted by DN, never by an extractor.
//!
//! RAR engine notice (Alexander Roshal):
//! UnRAR source code may be used in any software to handle RAR archives without
//! limitations free of charge, but cannot be used to develop RAR (WinRAR)
//! compatible archiver and to re-create RAR compression algorithm, which is
//! proprietary. Distribution of modified UnRAR source code in separate form or
//! as a part of other software is permitted, provided that full text of this
//! paragraph, starting from "UnRAR source code" words, is included in license,
//! or in documentation if license is not available, and in source code comments
//! of resulting package.
use super::{
    reader::{Catalog, Format, Kind, Member, MAX_ENTRIES, MAX_METADATA, MAX_STREAM},
    Password,
};
use crate::{
    task::Cancellation,
    viewer::{Contents, ViewMode},
};
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom, Write},
    path::PathBuf,
    time::{Duration, Instant},
};

mod preflight;

struct Input {
    file: File,
    cancel: Cancellation,
    remaining: u64,
    deadline: Instant,
}
impl Input {
    fn new(file: File, cancel: &Cancellation) -> Self {
        Self {
            file,
            cancel: cancel.clone(),
            remaining: MAX_STREAM + 2 * MAX_METADATA as u64,
            deadline: Instant::now() + Duration::from_secs(3600),
        }
    }
}
impl Read for Input {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        // Read::read_exact/write_all retry Interrupted. Cancellation is
        // permanent, so decoder-facing adapters must return a terminal error.
        self.cancel.check().map_err(io::Error::other)?;
        if Instant::now() > self.deadline || self.remaining == 0 {
            return Err(io::Error::other("Archive read budget exceeded"));
        }
        let length = bytes.len().min(65536).min(self.remaining as usize);
        let n = self.file.read(&mut bytes[..length])?;
        self.remaining -= n as u64;
        Ok(n)
    }
}
impl Seek for Input {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        // Read::read_exact/write_all retry Interrupted. Cancellation is
        // permanent, so decoder-facing adapters must return a terminal error.
        self.cancel.check().map_err(io::Error::other)?;
        self.file.seek(position)
    }
}
fn password(value: Option<&Password>) -> &str {
    value
        .map(|p| std::str::from_utf8(p.bytes()).expect("UTF-8 password"))
        .unwrap_or("")
}
fn error(e: impl std::fmt::Display) -> io::Error {
    let message = e.to_string();
    if message.to_ascii_lowercase().contains("password") {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Archive password required or incorrect",
        )
    } else {
        io::Error::other(message)
    }
}
fn seven(
    file: File,
    value: Option<&Password>,
    cancel: &Cancellation,
) -> io::Result<sevenz_rust2::ArchiveReader<preflight::Overlay>> {
    let reader = preflight::open(file, value, cancel)?;
    if reader.archive().files.len() > MAX_ENTRIES {
        return Err(io::Error::other("Archive exceeds 100,000 members"));
    }
    let mut total = 0u64;
    for entry in &reader.archive().files {
        total = total
            .checked_add(entry.size)
            .filter(|n| *n <= MAX_STREAM)
            .ok_or_else(|| io::Error::other("7z unpacked size exceeds 64 GiB"))?;
    }
    // LZMA/LZMA2 dictionary properties are checked before member decoding.
    for block in &reader.archive().blocks {
        for coder in &block.coders {
            let props = coder.properties();
            let dictionary = match coder.encoder_method_id() {
                [0x21] if !props.is_empty() => {
                    let p = props[0];
                    if p > 37 {
                        return Err(io::Error::other("7z dictionary exceeds 256 MiB"));
                    }
                    (2u64 | u64::from(p & 1)) << (u32::from(p / 2) + 11)
                }
                [3, 1, 1] if props.len() >= 5 => {
                    u64::from(u32::from_le_bytes(props[1..5].try_into().unwrap()))
                }
                _ => 0,
            };
            if dictionary > 256 * 1024 * 1024 {
                return Err(io::Error::other("7z dictionary exceeds 256 MiB"));
            }
        }
    }
    Ok(reader)
}
fn rar(
    mut file: File,
    value: Option<&Password>,
    cancel: &Cancellation,
) -> io::Result<unrar_rs::RarArchive> {
    let mut signature = [0; 8];
    let count = file.read(&mut signature)?;
    if !(count >= 7 && signature[..7] == *b"Rar!\x1a\x07\x00")
        && !(count == 8 && signature == *b"Rar!\x1a\x07\x01\x00")
    {
        return Err(io::Error::other("Invalid RAR signature"));
    }
    file.seek(SeekFrom::Start(0))?;
    let mut reader = if let Some(value) = value {
        unrar_rs::RarArchive::open_with_password(Input::new(file, cancel), password(Some(value)))
    } else {
        unrar_rs::RarArchive::open(Input::new(file, cancel))
    }
    .map_err(error)?;
    reader.set_limits(unrar_rs::Limits {
        max_unpacked_size: MAX_STREAM,
        max_data_segment: MAX_STREAM,
        ..Default::default()
    });
    if reader.more_volumes()
        || reader.present_volumes().iter().any(|n| *n != 0)
        || reader.entries().any(|entry| entry.volumes.is_split())
    {
        return Err(io::Error::other("Multi-volume RAR is not supported"));
    }
    if reader.len() > MAX_ENTRIES {
        return Err(io::Error::other("Archive exceeds 100,000 members"));
    }
    Ok(reader)
}
fn nt_time(value: sevenz_rust2::NtTime) -> Option<std::time::SystemTime> {
    let ticks = u64::from(value);
    let epoch = u64::from(sevenz_rust2::NtTime::UNIX_EPOCH);
    let delta = ticks.abs_diff(epoch);
    let duration = Duration::new(delta / 10_000_000, ((delta % 10_000_000) * 100) as u32);
    if ticks >= epoch {
        std::time::UNIX_EPOCH.checked_add(duration)
    } else {
        std::time::UNIX_EPOCH.checked_sub(duration)
    }
}
fn member(path: Vec<u8>, kind: Kind, size: u64, source: usize, locked: bool) -> Member {
    Member {
        raw: path.clone(),
        path,
        parent: 0,
        kind,
        size,
        source: Some(source),
        locked,
        mode: None,
        modified: None,
        modified_time: None,
        children: Vec::new(),
    }
}
pub(super) fn list(
    catalog: &mut Catalog,
    file: File,
    password: Option<&Password>,
    cancel: &Cancellation,
) -> io::Result<()> {
    match catalog.format {
        Format::SevenZ => {
            let reader = seven(file, password, cancel)?;
            let locked = reader.archive().blocks.iter().any(|b| {
                b.coders
                    .iter()
                    .any(|c| c.encoder_method_id() == [6, 0xf1, 7, 1])
            });
            for (index, entry) in reader.archive().files.iter().enumerate() {
                cancel.check()?;
                let mode = entry.windows_attributes >> 16;
                let kind = if entry.is_anti_item {
                    Kind::Special
                } else if entry.windows_attributes & 0x400 != 0 || mode & 0o170000 == 0o120000 {
                    Kind::Link
                } else if !matches!(mode & 0o170000, 0 | 0o100000 | 0o040000) {
                    Kind::Special
                } else if entry.is_directory {
                    Kind::Directory
                } else {
                    Kind::File
                };
                let mut item = member(
                    entry
                        .name
                        .as_bytes()
                        .iter()
                        .map(|b| if *b == b'\\' { b'/' } else { *b })
                        .collect(),
                    kind,
                    entry.size,
                    index,
                    locked && entry.has_stream,
                );
                item.raw = entry.name.as_bytes().to_vec();
                item.mode = (mode != 0).then_some(mode);
                item.modified_time = entry
                    .has_last_modified_date
                    .then(|| nt_time(entry.last_modified_date))
                    .flatten();
                catalog.add(item);
            }
        }
        Format::Rar => {
            let reader = rar(file, password, cancel)?;
            for (index, entry) in reader.entries().enumerate() {
                cancel.check()?;
                let kind = if entry.is_symlink || entry.is_hardlink || entry.is_file_copy {
                    Kind::Link
                } else if entry.is_directory {
                    Kind::Directory
                } else {
                    Kind::File
                };
                let size = entry
                    .unpacked_size
                    .ok_or_else(|| io::Error::other("Incomplete RAR member size"))?;
                let raw = entry
                    .raw_name_bytes
                    .unwrap_or_else(|| entry.raw_name.into_bytes());
                let path = raw
                    .iter()
                    .map(|b| if *b == b'\\' { b'/' } else { *b })
                    .collect();
                let mut item = member(path, kind, size, index, entry.is_encrypted);
                item.raw = raw;
                item.mode = Some(entry.attributes.unix_mode());
                item.modified_time = entry.mtime;
                catalog.add(item);
            }
        }
        _ => unreachable!(),
    }
    Ok(())
}
struct Sink<'a, W> {
    writer: &'a mut W,
    cancel: &'a Cancellation,
    count: u64,
    expected: u64,
}
impl<W: Write> Write for Sink<'_, W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        // Read::read_exact/write_all retry Interrupted. Cancellation is
        // permanent, so decoder-facing adapters must return a terminal error.
        self.cancel.check().map_err(io::Error::other)?;
        if self.count.saturating_add(bytes.len() as u64) > self.expected {
            return Err(io::Error::other("Member exceeds declared size"));
        }
        let n = self.writer.write(bytes)?;
        self.count += n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}
struct Callback<'a, F> {
    id: usize,
    emit: &'a mut F,
}
impl<F: FnMut(usize, &[u8]) -> io::Result<()>> Write for Callback<'_, F> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        (self.emit)(self.id, bytes)?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn decode_many(
    catalog: &Catalog,
    ids: &[usize],
    value: Option<&Password>,
    cancel: &Cancellation,
    mut emit: impl FnMut(usize, &[u8]) -> io::Result<()>,
) -> io::Result<()> {
    cancel.check()?;
    let (file, _) = catalog.checked_source()?;
    let mut ids = ids.to_vec();
    ids.sort_by_key(|id| catalog.nodes[*id].source);
    ids.dedup();
    let mut found = 0;
    match catalog.format {
        Format::Rar => {
            let mut reader = rar(file, value, cancel)?;
            for id in &ids {
                cancel.check()?;
                let node = &catalog.nodes[*id];
                let entry = reader
                    .by_index(node.source.ok_or_else(|| error("Missing member data"))?)
                    .map_err(error)?;
                if entry
                    .info()
                    .raw_name_bytes
                    .as_deref()
                    .unwrap_or_else(|| entry.info().raw_name.as_bytes())
                    != node.raw
                    || entry.info().is_symlink
                    || entry.info().is_hardlink
                    || entry.size() != Some(node.size)
                {
                    return Err(error("RAR member changed"));
                }
                emit(*id, &[])?;
                let mut output = Callback {
                    id: *id,
                    emit: &mut emit,
                };
                let mut sink = Sink {
                    writer: &mut output,
                    cancel,
                    count: 0,
                    expected: node.size,
                };
                entry.copy_to(&mut sink).map_err(error)?;
                if sink.count != node.size {
                    return Err(error("Truncated archive member"));
                }
                found += 1;
            }
        }
        Format::SevenZ => {
            let mut reader = seven(file, value, cancel)?;
            let mut wanted = std::collections::HashMap::new();
            for id in &ids {
                let node = &catalog.nodes[*id];
                let entry = node
                    .source
                    .and_then(|index| reader.archive().files.get(index))
                    .ok_or_else(|| error("7z member disappeared"))?;
                if entry.name.as_bytes() != node.raw || entry.size != node.size {
                    return Err(error("7z member changed"));
                }
                wanted.insert(node.raw.as_slice(), *id);
            }
            reader
                .for_each_entries(|entry, input| {
                    cancel.check().map_err(io::Error::other)?;
                    if let Some(id) = wanted.remove(entry.name.as_bytes()) {
                        let node = &catalog.nodes[id];
                        emit(id, &[])?;
                        let mut output = Callback {
                            id,
                            emit: &mut emit,
                        };
                        let mut sink = Sink {
                            writer: &mut output,
                            cancel,
                            count: 0,
                            expected: node.size,
                        };
                        io::copy(input, &mut sink)?;
                        if sink.count != node.size {
                            return Err(io::Error::other("Truncated archive member").into());
                        }
                        found += 1;
                    }
                    Ok(true)
                })
                .map_err(error)?;
        }
        _ => unreachable!(),
    }
    cancel.check()?;
    if found != ids.len() {
        return Err(error("Archive members disappeared"));
    }
    catalog.validate()
}
fn decode(
    catalog: &Catalog,
    id: usize,
    value: Option<&Password>,
    cancel: &Cancellation,
    writer: &mut impl Write,
) -> io::Result<()> {
    decode_many(catalog, &[id], value, cancel, |_, bytes| {
        writer.write_all(bytes)
    })
}
pub(super) fn authenticate(
    catalog: &Catalog,
    ids: &[usize],
    value: &Password,
    cancel: &Cancellation,
) -> io::Result<()> {
    let ids = ids
        .iter()
        .copied()
        .filter(|id| catalog.nodes[*id].locked && catalog.nodes[*id].kind == Kind::File)
        .collect::<Vec<_>>();
    decode_many(catalog, &ids, Some(value), cancel, |_, _| Ok(()))
}
struct Preview(Vec<u8>);
impl Write for Preview {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let keep = bytes
            .len()
            .min((crate::viewer::MAX_BYTES + 1).saturating_sub(self.0.len()));
        self.0.extend_from_slice(&bytes[..keep]);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
pub(super) fn preview(
    catalog: &Catalog,
    id: usize,
    value: Option<&Password>,
    mode: ViewMode,
    cancel: &Cancellation,
) -> io::Result<Contents> {
    let mut output = Preview(Vec::new());
    decode(catalog, id, value, cancel, &mut output)?;
    crate::viewer::pdf::document_bytes(
        output.0,
        mode,
        crate::viewer::pdf::is_path(std::path::Path::new(
            std::str::from_utf8(&catalog.nodes[id].path).unwrap_or(""),
        )),
        cancel,
    )
}
pub(super) fn extract(
    catalog: &Catalog,
    files: &[(usize, PathBuf)],
    value: Option<&Password>,
    cancel: &Cancellation,
    mut progress: impl FnMut(usize, u64),
) -> io::Result<()> {
    fn finish(file: &File, node: &Member) -> io::Result<()> {
        #[cfg(unix)]
        if let Some(mode) = node.mode {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(mode & 0o777))?;
        }
        if let Some(time) = node.modified_time {
            file.set_times(std::fs::FileTimes::new().set_modified(time))?;
        }
        file.sync_all()
    }
    let destinations = files
        .iter()
        .map(|(id, path)| (*id, path))
        .collect::<std::collections::HashMap<_, _>>();
    let ids = files.iter().map(|(id, _)| *id).collect::<Vec<_>>();
    let mut current: Option<(usize, File)> = None;
    let result = decode_many(catalog, &ids, value, cancel, |id, bytes| {
        if current.as_ref().is_none_or(|(previous, _)| *previous != id) {
            if let Some((previous, file)) = current.take() {
                finish(&file, &catalog.nodes[previous])?;
            }
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            current = Some((id, options.open(destinations[&id])?));
        }
        current.as_mut().unwrap().1.write_all(bytes)?;
        progress(id, bytes.len() as u64);
        Ok(())
    });
    if result.is_err() {
        cancel.check()?;
    }
    result?;
    if let Some((id, file)) = current {
        finish(&file, &catalog.nodes[id])?;
    }
    catalog.validate()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::test_support::{self, Temp};
    #[test]
    fn file_times_convert_without_panics_at_epoch_and_extreme_values() {
        let epoch = sevenz_rust2::NtTime::UNIX_EPOCH;
        assert_eq!(nt_time(epoch), Some(std::time::UNIX_EPOCH));
        assert_eq!(
            nt_time(sevenz_rust2::NtTime::from(u64::from(epoch) + 12_345_678)),
            std::time::UNIX_EPOCH.checked_add(Duration::new(1, 234_567_800))
        );
        let _ = nt_time(sevenz_rust2::NtTime::from(u64::MAX));
        #[cfg(windows)]
        assert!(nt_time(sevenz_rust2::NtTime::from(u64::MAX)).is_none());
    }
    #[test]
    fn internal_rar_and_seven_preview_extract_and_reject_source_changes() -> io::Result<()> {
        let t = Temp::new()?;
        for (name, seven) in [("fixture.rar", false), ("fixture.7z", true)] {
            let path = t.0.join(name);
            let text = "Hello Ёжик\n";
            if seven {
                test_support::seven(&path, "docs/hello.txt", text.as_bytes())?;
            } else {
                test_support::rar(&path, "docs\\hello.txt", text.as_bytes())?;
            }
            let c = super::super::reader::list(path.clone(), &Cancellation::default())?;
            assert_eq!(c.skipped, 0);
            assert!(c.nodes.iter().any(|n| n.path == b"docs/hello.txt"));
            let id = c.nodes.iter().position(|n| n.kind == Kind::File).unwrap();
            assert_eq!(
                preview(&c, id, None, ViewMode::Auto, &Cancellation::default())?
                    .source
                    .as_deref(),
                Some(text)
            );
            let out = t.0.join(format!("{name}.txt"));
            extract(
                &c,
                &[(id, out.clone())],
                None,
                &Cancellation::default(),
                |_, _| {},
            )?;
            assert_eq!(std::fs::read_to_string(out)?, text);
            std::fs::write(&path, "replaced")?;
            assert!(preview(&c, id, None, ViewMode::Auto, &Cancellation::default()).is_err());
        }
        Ok(())
    }
    #[test]
    fn encrypted_seven_headers_and_members_require_the_correct_password() -> io::Result<()> {
        use sevenz_rust2::{
            encoder_options::AesEncoderOptions, ArchiveEntry, ArchiveWriter, EncoderConfiguration,
            EncoderMethod,
        };
        let t = Temp::new()?;
        let path = t.0.join("secret.7z");
        let mut writer = ArchiveWriter::new(File::create(&path)?).map_err(error)?;
        writer.set_content_methods(vec![
            EncoderConfiguration::new(EncoderMethod::LZMA2),
            AesEncoderOptions::new(sevenz_rust2::Password::from("fixture")).into(),
        ]);
        writer
            .push_archive_entry(
                ArchiveEntry {
                    name: "secret.txt".into(),
                    has_stream: true,
                    ..Default::default()
                },
                Some(&b"secret content"[..]),
            )
            .map_err(error)?;
        writer.finish()?;
        assert_eq!(
            super::super::reader::list(path.clone(), &Cancellation::default())
                .err()
                .unwrap()
                .kind(),
            io::ErrorKind::PermissionDenied
        );
        assert!(super::super::reader::list_with_password(
            path.clone(),
            &Password::new("wrong"),
            &Cancellation::default()
        )
        .is_err());
        let catalog = super::super::reader::list_with_password(
            path,
            &Password::new("fixture"),
            &Cancellation::default(),
        )?;
        let id = catalog
            .nodes
            .iter()
            .position(|n| n.kind == Kind::File)
            .unwrap();
        assert!(catalog.nodes[id].locked);
        assert_eq!(
            preview(
                &catalog,
                id,
                Some(&Password::new("fixture")),
                ViewMode::Auto,
                &Cancellation::default()
            )?
            .source
            .as_deref(),
            Some("secret content")
        );
        Ok(())
    }
    #[test]
    fn empty_seven_archive_has_an_empty_catalog() -> io::Result<()> {
        let t = Temp::new()?;
        let path = t.0.join("empty.7z");
        let mut bytes = [0u8; 32];
        bytes[..8].copy_from_slice(b"7z\xbc\xaf\x27\x1c\0\x04");
        let crc = crc32fast::hash(&bytes[12..]);
        bytes[8..12].copy_from_slice(&crc.to_le_bytes());
        std::fs::write(&path, bytes)?;
        let catalog = super::super::reader::list(path, &Cancellation::default())?;
        assert_eq!(catalog.nodes.len(), 1);
        assert_eq!(catalog.nodes[0].kind, Kind::Directory);
        assert!(catalog.nodes[0].path.is_empty());
        Ok(())
    }
    #[test]
    fn malformed_and_oversized_headers_and_cancelled_decodes_are_errors() -> io::Result<()> {
        let t = Temp::new()?;
        for name in ["bad.rar", "bad.7z"] {
            let path = t.0.join(name);
            std::fs::write(&path, [0; 40])?;
            assert!(super::super::reader::list(path, &Cancellation::default()).is_err());
        }
        let path = t.0.join("fixture.7z");
        test_support::seven(&path, "hello.txt", b"hello")?;
        let c = super::super::reader::list(path, &Cancellation::default())?;
        let id = c.nodes.iter().position(|n| n.kind == Kind::File).unwrap();
        let token = Cancellation::default();
        token.cancel();
        assert!(preview(&c, id, None, ViewMode::Auto, &token).is_err());
        Ok(())
    }
}
