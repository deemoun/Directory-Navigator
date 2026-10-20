//! Bounded read-only archive I/O. No unpack/extract APIs or temporary files.
use crate::{
    filesystem::open_regular,
    task::Cancellation,
    viewer::{self, Contents, ViewMode},
};
use std::{
    collections::HashMap,
    fs::{File, Metadata},
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
pub const MAX_ENTRIES: usize = 10_000;
pub const MAX_NODES: usize = 20_000;
pub const MAX_METADATA: usize = 16 * 1024 * 1024;
pub const MAX_STREAM: u64 = 256 * 1024 * 1024;
const MAX_SOURCE: u64 = 512 * 1024 * 1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Zip,
    Tar,
    TarGz,
}
impl Format {
    pub fn detect(path: &Path) -> Option<Self> {
        let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
        if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
            Some(Self::TarGz)
        } else if name.ends_with(".tar") {
            Some(Self::Tar)
        } else if name.ends_with(".zip") {
            Some(Self::Zip)
        } else {
            None
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Directory,
    File,
    Link,
    Special,
}
#[derive(Clone, Debug)]
pub struct Member {
    pub path: Vec<u8>,
    pub parent: usize,
    pub kind: Kind,
    pub size: u64,
    pub modified: Option<String>,
    pub children: Vec<usize>,
    pub source: Option<usize>,
    pub raw: Vec<u8>,
    pub locked: bool,
}
impl Member {
    pub fn name(&self) -> String {
        crate::filesystem::display_name(&String::from_utf8_lossy(
            self.path.rsplit(|b| *b == b'/').next().unwrap_or(&[]),
        ))
    }
}
pub struct Catalog {
    pub path: PathBuf,
    pub format: Format,
    pub nodes: Vec<Member>,
    pub skipped: usize,
    pub note: Option<String>,
    identity: Metadata,
    keys: HashMap<Vec<u8>, usize>,
    metadata_bytes: usize,
}
pub fn normalize(raw: &[u8]) -> io::Result<Vec<u8>> {
    if raw.len() > 4096 || raw.contains(&0) || raw.contains(&b'\\') || raw.starts_with(b"/") {
        return Err(io::Error::other("Unsafe or oversized member path"));
    }
    let mut path = Vec::new();
    let mut depth = 0;
    for part in raw.split(|b| *b == b'/') {
        if part.is_empty() || part == b"." {
            continue;
        }
        if part == b".." || part.contains(&b':') {
            return Err(io::Error::other("Traversal or drive-qualified member path"));
        }
        depth += 1;
        if depth > 64 {
            return Err(io::Error::other("Archive nesting exceeds 64 levels"));
        }
        if !path.is_empty() {
            path.push(b'/');
        }
        path.extend_from_slice(part);
    }
    Ok(path)
}
impl Catalog {
    fn new(path: PathBuf, format: Format, identity: Metadata) -> Self {
        Self {
            path,
            format,
            identity,
            nodes: vec![Member {
                path: Vec::new(),
                parent: 0,
                kind: Kind::Directory,
                size: 0,
                modified: None,
                children: Vec::new(),
                source: None,
                raw: Vec::new(),
                locked: false,
            }],
            keys: HashMap::from([(Vec::new(), 0)]),
            metadata_bytes: 0,
            skipped: 0,
            note: None,
        }
    }
    fn insert(&mut self, mut member: Member) -> io::Result<()> {
        let path = normalize(&member.path)?;
        if path.is_empty() {
            return Ok(());
        }
        let mut parent = 0;
        let pieces = path.split(|b| *b == b'/').collect::<Vec<_>>();
        let mut current = Vec::new();
        for (index, piece) in pieces.iter().enumerate() {
            if !current.is_empty() {
                current.push(b'/');
            }
            current.extend_from_slice(piece);
            let final_part = index + 1 == pieces.len();
            if let Some(id) = self.keys.get(&current).copied() {
                if final_part {
                    if self.nodes[id].source.is_none() && member.kind == Kind::Directory {
                        member.path = current;
                        member.parent = parent;
                        member.children = std::mem::take(&mut self.nodes[id].children);
                        self.nodes[id] = member;
                        return Ok(());
                    }
                    return Err(io::Error::other("Duplicate or conflicting archive member"));
                }
                if self.nodes[id].kind != Kind::Directory {
                    return Err(io::Error::other("File/link used as a parent directory"));
                }
                parent = id;
                continue;
            }
            if self.nodes.len() >= MAX_NODES
                || self
                    .metadata_bytes
                    .saturating_add(current.len() + member.raw.len())
                    > MAX_METADATA
            {
                return Err(io::Error::other("Archive tree/metadata limit reached"));
            }
            self.metadata_bytes += current.len() + if final_part { member.raw.len() } else { 0 };
            let id = self.nodes.len();
            self.keys.insert(current.clone(), id);
            self.nodes[parent].children.push(id);
            if final_part {
                member.path = current;
                member.parent = parent;
                self.nodes.push(member);
                return Ok(());
            }
            self.nodes.push(Member {
                path: current.clone(),
                parent,
                kind: Kind::Directory,
                size: 0,
                modified: None,
                children: Vec::new(),
                source: None,
                raw: Vec::new(),
                locked: false,
            });
            parent = id;
        }
        Ok(())
    }
    fn add(&mut self, member: Member) {
        if let Err(e) = self.insert(member) {
            self.skipped += 1;
            if self.note.is_none() {
                self.note = Some(e.to_string());
            }
        }
    }
    fn finish(&mut self) {
        let nodes = &self.nodes;
        let mut children = nodes.iter().map(|n| n.children.clone()).collect::<Vec<_>>();
        for ids in &mut children {
            ids.sort_by(|a, b| {
                let a = &nodes[*a];
                let b = &nodes[*b];
                (b.kind == Kind::Directory)
                    .cmp(&(a.kind == Kind::Directory))
                    .then_with(|| a.path.cmp(&b.path))
            });
        }
        for (n, c) in self.nodes.iter_mut().zip(children) {
            n.children = c;
        }
    }
}
/// Check cancellation, elapsed work and bytes on every bounded read.
struct Guard<R> {
    inner: R,
    cancel: Cancellation,
    remaining: u64,
    deadline: Instant,
}
impl<R> Guard<R> {
    fn new(inner: R, cancel: &Cancellation, limit: u64) -> Self {
        Self {
            inner,
            cancel: cancel.clone(),
            remaining: limit,
            deadline: Instant::now() + Duration::from_secs(10),
        }
    }
}
impl<R: Read> Read for Guard<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.cancel.check()?;
        if Instant::now() > self.deadline {
            return Err(io::Error::other("Archive work exceeded 10 seconds"));
        }
        if buffer.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            return Err(io::Error::other("Archive read/decompression limit reached"));
        }
        let len = buffer.len().min(64 * 1024).min(self.remaining as usize);
        let n = self.inner.read(&mut buffer[..len])?;
        self.remaining -= n as u64;
        Ok(n)
    }
}
impl<R: Seek> Seek for Guard<R> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.cancel.check()?;
        self.inner.seek(position)
    }
}
fn source(path: &Path) -> io::Result<(File, Metadata)> {
    let file = open_regular(path)?;
    let metadata = file.metadata()?;
    if metadata.len() > MAX_SOURCE {
        return Err(io::Error::other("Archive exceeds the 512 MiB source limit"));
    }
    Ok((file, metadata))
}
fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}
fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}
/// Bound ZIP allocation before the library constructs its central-directory index.
fn zip_guard(input: &mut Guard<File>, length: u64) -> io::Result<usize> {
    let tail_len = length.min(65_557) as usize;
    input.seek(SeekFrom::End(-(tail_len as i64)))?;
    let mut tail = vec![0; tail_len];
    input.read_exact(&mut tail)?;
    let end = (0..tail.len().saturating_sub(21))
        .rev()
        .find(|i| {
            tail[*i..].starts_with(b"PK\x05\x06")
                && *i + 22 + usize::from(u16_at(&tail, *i + 20)) == tail.len()
        })
        .ok_or_else(|| io::Error::other("Missing ZIP central directory"))?;
    let count = u16_at(&tail, end + 10);
    let size = u32_at(&tail, end + 12);
    let offset = u32_at(&tail, end + 16);
    let absolute = length - tail_len as u64 + end as u64;
    if u16_at(&tail, end + 4) != 0
        || u16_at(&tail, end + 6) != 0
        || u16_at(&tail, end + 8) != count
        || count == u16::MAX
        || size == u32::MAX
        || offset == u32::MAX
        || count as usize > MAX_ENTRIES
        || size as usize > MAX_METADATA
        || u64::from(offset) + u64::from(size) != absolute
    {
        return Err(io::Error::other(
            "ZIP is oversized, ZIP64, multi-volume or has an unsupported directory layout",
        ));
    }
    if absolute >= 20 {
        input.seek(SeekFrom::Start(absolute - 20))?;
        let mut signature = [0; 4];
        input.read_exact(&mut signature)?;
        if signature == *b"PK\x06\x07" {
            return Err(io::Error::other("ZIP64 is not supported in this browser"));
        }
    }
    input.seek(SeekFrom::Start(u64::from(offset)))?;
    let mut consumed = 0u64;
    for _ in 0..count {
        let mut header = [0; 46];
        input.read_exact(&mut header)?;
        if !header.starts_with(b"PK\x01\x02") {
            return Err(io::Error::other("Invalid ZIP directory entry"));
        }
        let name = u16_at(&header, 28);
        let extra = u16_at(&header, 30);
        let comment = u16_at(&header, 32);
        let record = 46 + u64::from(name) + u64::from(extra) + u64::from(comment);
        consumed += record;
        if name > 4096 || consumed > u64::from(size) {
            return Err(io::Error::other(
                "ZIP metadata limit or invalid directory length",
            ));
        }
        input.seek(SeekFrom::Current((record - 46) as i64))?;
    }
    if consumed != u64::from(size) {
        return Err(io::Error::other("ZIP directory count/size mismatch"));
    }
    input.seek(SeekFrom::Start(0))?;
    Ok(usize::from(count))
}
fn zip_open(
    file: File,
    metadata: &Metadata,
    cancel: &Cancellation,
) -> io::Result<zip::ZipArchive<Guard<File>>> {
    let mut input = Guard::new(file, cancel, 64 * 1024 * 1024);
    let count = zip_guard(&mut input, metadata.len())?;
    let archive = zip::ZipArchive::new(input).map_err(io::Error::other)?;
    if archive.len() != count {
        return Err(io::Error::other(
            "Duplicate ZIP member names are not supported",
        ));
    }
    Ok(archive)
}
fn tar_input(file: File, format: Format, cancel: &Cancellation) -> Box<dyn Read> {
    let input = Guard::new(file, cancel, MAX_SOURCE);
    if format == Format::TarGz {
        Box::new(Guard::new(
            flate2::read::MultiGzDecoder::new(input),
            cancel,
            MAX_STREAM,
        ))
    } else {
        Box::new(Guard::new(input, cancel, MAX_STREAM))
    }
}
pub fn list(path: PathBuf, cancel: &Cancellation) -> io::Result<Catalog> {
    cancel.check()?;
    let format = Format::detect(&path)
        .ok_or_else(|| io::Error::other("Supported formats: ZIP, TAR, TAR.GZ and TGZ"))?;
    let (file, metadata) = source(&path)?;
    let mut result = Catalog::new(path, format, metadata);
    if format == Format::Zip {
        let mut archive = zip_open(file, &result.identity, cancel)?;
        for index in 0..archive.len() {
            cancel.check()?;
            let entry = archive.by_index_raw(index).map_err(io::Error::other)?;
            let kind = if entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) {
                Kind::Link
            } else if entry.is_dir() {
                Kind::Directory
            } else if entry
                .unix_mode()
                .is_some_and(|m| !matches!(m & 0o170000, 0 | 0o100000))
            {
                Kind::Special
            } else {
                Kind::File
            };
            result.add(Member {
                path: entry
                    .name()
                    .map(|n| n.into_owned().into_bytes())
                    .unwrap_or_else(|_| entry.name_raw().to_vec()),
                parent: 0,
                kind,
                size: entry.size(),
                modified: entry.last_modified().map(|t| {
                    format!(
                        "{:04}-{:02}-{:02} {:02}:{:02}",
                        t.year(),
                        t.month(),
                        t.day(),
                        t.hour(),
                        t.minute()
                    )
                }),
                children: Vec::new(),
                source: Some(index),
                raw: entry.name_raw().to_vec(),
                locked: entry.encrypted(),
            });
        }
    } else {
        let mut archive = tar::Archive::new(tar_input(file, format, cancel));
        for (index, item) in archive.entries()?.enumerate() {
            cancel.check()?;
            if index >= MAX_ENTRIES {
                result.note = Some("Partial listing: 10,000 member limit".into());
                break;
            }
            let entry = match item {
                Ok(e) => e,
                Err(e) => {
                    cancel.check()?;
                    if result.nodes.len() == 1 {
                        return Err(e);
                    }
                    result.note = Some(format!("Partial listing: {e}"));
                    break;
                }
            };
            let kind = entry.header().entry_type();
            let kind = if kind.is_dir() {
                Kind::Directory
            } else if kind.is_symlink() || kind.is_hard_link() {
                Kind::Link
            } else if kind.is_file() {
                Kind::File
            } else {
                Kind::Special
            };
            let raw = entry.path_bytes().into_owned();
            result.add(Member {
                path: raw.clone(),
                parent: 0,
                kind,
                size: entry.size(),
                modified: entry
                    .header()
                    .mtime()
                    .ok()
                    .and_then(|s| i64::try_from(s).ok())
                    .and_then(|s| chrono::DateTime::from_timestamp(s, 0))
                    .map(|t| t.format("%Y-%m-%d %H:%M UTC").to_string()),
                children: Vec::new(),
                source: Some(index),
                raw,
                locked: false,
            });
        }
    }
    cancel.check()?;
    result.finish();
    Ok(result)
}
fn read_preview(
    input: &mut impl Read,
    cancel: &Cancellation,
    expected: u64,
) -> io::Result<Contents> {
    let mut bytes = Vec::new();
    let mut input = input.take(viewer::MAX_BYTES as u64 + 1);
    let mut chunk = [0; 64 * 1024];
    loop {
        cancel.check()?;
        let n = input.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..n]);
    }
    if (bytes.len() as u64) < expected.min(viewer::MAX_BYTES as u64 + 1) {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "Archive member ended before its declared length",
        ));
    }
    Ok(viewer::from_bytes(bytes, ViewMode::Auto))
}
pub fn preview(catalog: &Catalog, id: usize, cancel: &Cancellation) -> io::Result<Contents> {
    cancel.check()?;
    let node = catalog
        .nodes
        .get(id)
        .ok_or_else(|| io::Error::other("Missing archive member"))?;
    if node.kind != Kind::File || node.locked {
        return Err(io::Error::other(
            "Only unencrypted regular members can be previewed; links are never followed",
        ));
    }
    let index = node
        .source
        .ok_or_else(|| io::Error::other("Member has no data"))?;
    let (file, metadata) = source(&catalog.path)?;
    if !crate::platform_fs::same_identity(&catalog.identity, &metadata)
        || catalog.identity.len() != metadata.len()
        || catalog.identity.modified().ok() != metadata.modified().ok()
    {
        return Err(io::Error::other(
            "Archive changed since listing; refresh before previewing",
        ));
    }
    if catalog.format == Format::Zip {
        let mut archive = zip_open(file, &metadata, cancel)?;
        let mut entry = archive.by_index(index).map_err(io::Error::other)?;
        if entry.name_raw() != node.raw {
            return Err(io::Error::other("Archive member changed"));
        }
        read_preview(&mut entry, cancel, node.size)
    } else {
        let mut archive = tar::Archive::new(tar_input(file, catalog.format, cancel));
        for (position, item) in archive.entries()?.enumerate() {
            cancel.check()?;
            let mut entry = item?;
            if position == index {
                if entry.path_bytes().as_ref() != node.raw {
                    return Err(io::Error::other("Archive member changed"));
                }
                return read_preview(&mut entry, cancel, node.size);
            }
        }
        Err(io::Error::other("Archive member disappeared"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn guarded_reads_enforce_budgets_cancellation_and_deadlines() {
        let mut input = Guard::new(&b"abcdef"[..], &Cancellation::default(), 3);
        let mut bytes = [0; 8];
        assert_eq!(input.read(&mut bytes).unwrap(), 3);
        assert!(input
            .read(&mut bytes)
            .unwrap_err()
            .to_string()
            .contains("limit"));
        let token = Cancellation::default();
        let mut input = Guard::new(&b"abc"[..], &token, 10);
        token.cancel();
        assert_eq!(
            input.read(&mut bytes).unwrap_err().kind(),
            io::ErrorKind::Interrupted
        );
        let mut input = Guard::new(&b"abc"[..], &Cancellation::default(), 10);
        input.deadline = Instant::now() - Duration::from_secs(1);
        assert!(input
            .read(&mut bytes)
            .unwrap_err()
            .to_string()
            .contains("10 seconds"));
    }
}
