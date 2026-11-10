//! Windowed file viewing: sparse source offsets and bounded decoded windows.
use super::{Contents, Match, SearchMemory, ViewMode};
use crate::{
    task::Cancellation,
    text_encoding::{TextEncoding, TextFormat},
};
use std::{
    collections::VecDeque,
    fs::{File, Metadata},
    io::{self, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
const WINDOW_LINES: usize = 4096;
const WINDOW_BYTES: usize = 4 * 1024 * 1024;
const LINE_BYTES: usize = 64 * 1024;
const CHECKPOINT: usize = 4096;
struct Index {
    checkpoints: Vec<(usize, u64)>,
    total: Option<usize>,
}
pub struct Source {
    path: PathBuf,
    metadata: Metadata,
    identity: crate::platform_fs::EntryIdentity,
    pub bytes: u64,
    pub format: Option<TextFormat>,
    sample: Arc<[u8]>,
    choose_encoding: bool,
    index: Mutex<Index>,
}
#[derive(Clone)]
pub struct Window {
    pub source: Arc<Source>,
    pub first: usize,
    pub target: usize,
    pub eof: bool,
    pub found: Option<Match>,
    pub notice: String,
}
#[derive(Clone)]
pub(super) enum Request {
    Line(usize),
    End,
    Search {
        memory: SearchMemory,
        backward: bool,
        origin: (usize, usize),
    },
}
fn error(e: impl ToString) -> io::Error {
    io::Error::other(e.to_string())
}
impl Source {
    pub(super) fn open(path: &Path, mode: ViewMode, cancel: &Cancellation) -> io::Result<Contents> {
        let file = crate::filesystem::open_regular(path)?;
        let metadata = file.metadata()?;
        let identity = crate::platform_fs::entry_identity(path)?;
        if !crate::platform_fs::owns_file(path, &file) {
            return Err(error("Viewer source changed"));
        }
        let mut sample = Vec::new();
        file.take(16 * 1024).read_to_end(&mut sample)?;
        let detected = crate::text_encoding::detect(&sample, true)
            .ok()
            .flatten()
            .or_else(|| {
                (mode == ViewMode::Text && TextFormat::default().decode(&sample, true).is_ok())
                    .then_some(TextFormat::default())
            });
        let choose_encoding = detected.is_none()
            && mode != ViewMode::Hex
            && crate::text_encoding::plausible_text(&sample);
        let format = (mode != ViewMode::Hex).then_some(detected).flatten();
        let source = Arc::new(Self {
            bytes: metadata.len(),
            path: path.to_owned(),
            metadata,
            identity,
            format,
            sample: sample.into(),
            choose_encoding,
            index: Mutex::new(Index {
                checkpoints: vec![(0, 0)],
                total: None,
            }),
        });
        source.load(Request::Line(0), cancel)
    }
    pub(super) fn with_format(&self, format: Option<TextFormat>) -> Arc<Self> {
        Arc::new(Self {
            path: self.path.clone(),
            metadata: self.metadata.clone(),
            identity: self.identity,
            bytes: self.bytes,
            format,
            sample: self.sample.clone(),
            choose_encoding: false,
            index: Mutex::new(Index {
                checkpoints: vec![(0, 0)],
                total: None,
            }),
        })
    }
    fn checked(&self) -> io::Result<File> {
        let file = crate::filesystem::open_regular(&self.path)?;
        let metadata = file.metadata()?;
        if self.identity != crate::platform_fs::entry_identity(&self.path)?
            || !crate::platform_fs::owns_file(&self.path, &file)
            || !crate::platform_fs::same_identity(&metadata, &self.metadata)
            || metadata.len() != self.bytes
            || metadata.modified().ok() != self.metadata.modified().ok()
        {
            return Err(error(
                "File changed while streaming; reopen it to read the new contents",
            ));
        }
        Ok(file)
    }
    pub fn total_lines(&self) -> Option<usize> {
        if self.format.is_none() {
            Some(self.bytes.div_ceil(16) as usize)
        } else {
            self.index.lock().unwrap().total
        }
    }
    fn checkpoint(&self, line: usize, byte: u64) {
        if line.is_multiple_of(CHECKPOINT) {
            let mut index = self.index.lock().unwrap();
            let at = index.checkpoints.partition_point(|(n, _)| *n < line);
            if index.checkpoints.len() < 65536
                && index.checkpoints.get(at).is_none_or(|(n, _)| *n != line)
            {
                index.checkpoints.insert(at, (line, byte));
            }
        }
    }
    pub(super) fn load(
        self: &Arc<Self>,
        request: Request,
        cancel: &Cancellation,
    ) -> io::Result<Contents> {
        cancel.check()?;
        if self.format.is_none() {
            return self.hex(request, cancel);
        }
        let mut target = match &request {
            Request::Line(n) => *n,
            _ => 0,
        };
        let mut found = None;
        let mut notice = String::new();
        if let Request::Search {
            memory,
            backward,
            origin,
        } = &request
        {
            if memory.query.is_empty() {
                return Err(error("Enter text to find"));
            }
            let mut reader = Lines::new(self.checked()?, self.format.unwrap(), 0, cancel)?;
            let mut line = 0;
            let mut wrap = None;
            while let Some(text) = reader.next()? {
                self.checkpoint(line, reader.start);
                let options = crate::editor::search::SearchOptions {
                    case_sensitive: memory.case_sensitive,
                    backward: *backward,
                    ..Default::default()
                };
                let ranges =
                    crate::editor::search::matches(&text, &memory.query, 0..text.len(), options)
                        .map_err(error)?;
                for range in ranges {
                    let item = Match { line, range };
                    let position = (
                        line,
                        if *backward {
                            item.range.end
                        } else {
                            item.range.start
                        },
                    );
                    if *backward {
                        if position <= *origin {
                            found = Some(item.clone());
                        }
                        wrap = Some(item);
                    } else if position >= *origin {
                        found = Some(item);
                        break;
                    } else if wrap.is_none() {
                        wrap = Some(item);
                    }
                }
                if found.is_some() && !*backward {
                    break;
                }
                line += 1;
            }
            found = found.or(wrap);
            if let Some(item) = &found {
                target = item.line;
            } else {
                target = origin.0;
                notice = "No match".into();
            }
        }
        let end = matches!(request, Request::End);
        let start = if end {
            self.index
                .lock()
                .unwrap()
                .checkpoints
                .last()
                .copied()
                .unwrap_or((0, 0))
        } else {
            let index = self.index.lock().unwrap();
            index
                .checkpoints
                .iter()
                .rev()
                .find(|(n, _)| *n <= target.saturating_sub(128))
                .copied()
                .unwrap_or((0, 0))
        };
        let mut reader = Lines::new(self.checked()?, self.format.unwrap(), start.1, cancel)?;
        let mut line = start.0;
        let mut kept = VecDeque::new();
        let mut bytes = 0usize;
        let mut eof = false;
        loop {
            let Some(text) = reader.next()? else {
                eof = true;
                self.index.lock().unwrap().total = Some(line.max(1));
                break;
            };
            self.checkpoint(line, reader.start);
            if end || line >= target.saturating_sub(128) {
                bytes += text.len();
                kept.push_back((line, text));
                if end {
                    while kept.len() > WINDOW_LINES || bytes > WINDOW_BYTES {
                        if let Some((_, text)) = kept.pop_front() {
                            bytes -= text.len();
                        }
                    }
                } else if kept.len() >= WINDOW_LINES || bytes >= WINDOW_BYTES {
                    break;
                }
            }
            line += 1;
        }
        if end {
            target = line.saturating_sub(1);
        }
        if kept.is_empty() {
            if target > 0 {
                return Err(error("Line is outside the file"));
            }
            kept.push_back((0, String::new()));
        }
        let first = kept.front().unwrap().0;
        let lines = kept.into_iter().map(|(_, text)| text).collect::<Vec<_>>();
        let mut contents = super::text_contents("");
        contents.source = None;
        contents.lines = lines;
        contents.raw = Some(self.sample.clone());
        contents.format = self.format;
        contents.raw_truncated = true;
        contents.loaded_bytes = self.bytes as usize;
        contents.truncated = false;
        contents.stream = Some(Arc::new(Window {
            source: self.clone(),
            first,
            target,
            eof,
            found,
            notice,
        }));
        self.checked()?;
        Ok(contents)
    }
    fn hex(self: &Arc<Self>, request: Request, cancel: &Cancellation) -> io::Result<Contents> {
        let count = self.bytes.div_ceil(16) as usize;
        let target = match request {
            Request::End => count.saturating_sub(1),
            Request::Line(line) => line,
            Request::Search { .. } => return Err(error("Text search is unavailable in Hex")),
        };
        if target >= count.max(1) {
            return Err(error("Offset is outside the file"));
        }
        let first = target.saturating_sub(128);
        let mut file = self.checked()?;
        file.seek(SeekFrom::Start(first as u64 * 16))?;
        let mut bytes = Vec::new();
        file.take((WINDOW_LINES * 16) as u64)
            .read_to_end(&mut bytes)?;
        cancel.check()?;
        let eof = first as u64 * 16 + bytes.len() as u64 >= self.bytes;
        let mut contents = super::from_bytes_cancelled(bytes, ViewMode::Hex, cancel)?;
        for (index, line) in contents.lines.iter_mut().enumerate() {
            let address = format!("{:08X}", (first + index) as u64 * 16);
            line.replace_range(..8, &address);
        }
        contents.raw = Some(self.sample.clone());
        contents.raw_truncated = true;
        contents.loaded_bytes = self.bytes as usize;
        contents.encoding_needed = self.choose_encoding;
        contents.stream = Some(Arc::new(Window {
            source: self.clone(),
            first,
            target,
            eof,
            found: None,
            notice: String::new(),
        }));
        self.checked()?;
        Ok(contents)
    }
}
struct Lines<'a> {
    input: BufReader<File>,
    format: TextFormat,
    pos: u64,
    start: u64,
    pending: Option<u16>,
    cancel: &'a Cancellation,
}
impl<'a> Lines<'a> {
    fn new(
        mut file: File,
        format: TextFormat,
        offset: u64,
        cancel: &'a Cancellation,
    ) -> io::Result<Self> {
        file.seek(SeekFrom::Start(offset))?;
        Ok(Self {
            input: BufReader::with_capacity(65536, file),
            format,
            pos: offset,
            start: offset,
            pending: None,
            cancel,
        })
    }
    fn unit(&mut self) -> io::Result<Option<u16>> {
        if self.pos.is_multiple_of(65536) {
            self.cancel.check()?;
        }
        let wide = matches!(
            self.format.encoding,
            TextEncoding::Utf16Le | TextEncoding::Utf16Be
        );
        if let Some(unit) = self.pending.take() {
            self.pos += if wide { 2 } else { 1 };
            return Ok(Some(unit));
        }
        let mut b = [0; 2];
        if self.input.read(&mut b[..1])? == 0 {
            return Ok(None);
        }
        self.pos += 1;
        if !wide {
            return Ok(Some(u16::from(b[0])));
        }
        self.input.read_exact(&mut b[1..])?;
        self.pos += 1;
        Ok(Some(if self.format.encoding == TextEncoding::Utf16Le {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        }))
    }
    fn next(&mut self) -> io::Result<Option<String>> {
        self.cancel.check()?;
        self.start = self.pos;
        let mut bytes = Vec::new();
        let mut seen = false;
        let mut clipped = false;
        while let Some(unit) = self.unit()? {
            seen = true;
            if unit == 10 {
                break;
            }
            if unit == 13 {
                if let Some(next) = self.unit()? {
                    if next != 10 {
                        self.pending = Some(next);
                        self.pos -= if matches!(
                            self.format.encoding,
                            TextEncoding::Utf16Le | TextEncoding::Utf16Be
                        ) {
                            2
                        } else {
                            1
                        };
                    }
                }
                break;
            }
            if bytes.len() < LINE_BYTES {
                match self.format.encoding {
                    TextEncoding::Utf16Le => bytes.extend_from_slice(&unit.to_le_bytes()),
                    TextEncoding::Utf16Be => bytes.extend_from_slice(&unit.to_be_bytes()),
                    _ => bytes.push(unit as u8),
                }
            } else {
                clipped = true;
            }
        }
        if !seen {
            return Ok(None);
        }
        let format = TextFormat {
            bom: self.format.bom && self.start == 0,
            ..self.format
        };
        let text = format.decode(&bytes, clipped).map_err(error)?;
        let mut displayed = super::text_contents(&text)
            .lines
            .into_iter()
            .next()
            .unwrap_or_default();
        if clipped {
            displayed.push_str(" … [line clipped]");
        }
        Ok(Some(displayed))
    }
}
impl super::Viewer {
    pub(super) fn stream_request(
        &mut self,
        source: Arc<Source>,
        request: Request,
    ) -> Result<(), String> {
        self.stream_pending = Some(
            self.requests
                .spawn("dn-viewer-stream", move |cancel| {
                    source.load(request, &cancel).map_err(|e| e.to_string())
                })
                .map_err(|e| e.to_string())?,
        );
        self.notice = "Reading file…".into();
        Ok(())
    }
    pub(super) fn poll_stream(&mut self) -> bool {
        let Some(reply) = crate::task::take_reply(&mut self.stream_pending) else {
            return false;
        };
        match reply {
            Ok(Ok(contents)) => {
                let window = contents.stream.as_ref().unwrap();
                let target = window.target - window.first;
                self.current = window.found.clone().map(|mut m| {
                    m.line -= window.first;
                    m
                });
                self.notice = window.notice.clone();
                self.offset = 0;
                self.rows.clear();
                self.layout_width = 0;
                self.reveal_line = Some(target);
                self.contents = Some(contents);
            }
            Ok(Err(error)) => self.notice = error,
            Err(_) => self.notice = "Viewer worker stopped".into(),
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> io::Result<Self> {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "dn-streaming-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path)?;
            Ok(Self(path))
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn windows_end_goto_and_search_reach_beyond_first_window() -> io::Result<()> {
        let t = Temp::new()?;
        let path = t.0.join("large.txt");
        let text = (0..12000)
            .map(|n| {
                format!(
                    "row {n} {}\n",
                    if [2, 9000].contains(&n) {
                        "needle Ёжик"
                    } else {
                        "ordinary"
                    }
                )
            })
            .collect::<String>();
        std::fs::write(&path, text)?;
        let c = Source::open(&path, ViewMode::Auto, &Cancellation::default())?;
        assert_eq!(c.lines.len(), WINDOW_LINES);
        let source = c.stream.as_ref().unwrap().source.clone();
        let c = source.load(Request::Line(9000), &Cancellation::default())?;
        let w = c.stream.as_ref().unwrap();
        assert!(c.lines[9000 - w.first].contains("needle Ёжик"));
        let c = source.load(Request::End, &Cancellation::default())?;
        let w = c.stream.as_ref().unwrap();
        assert_eq!(w.target, 11999);
        assert!(w.eof);
        assert_eq!(c.lines.last().unwrap(), "row 11999 ordinary");
        assert_eq!(source.total_lines(), Some(12000));
        let memory = SearchMemory {
            query: "needle Ёжик".into(),
            case_sensitive: true,
        };
        for (backward, origin, expected) in [
            (false, (3, 0), 9000),
            (true, (8999, 0), 2),
            (false, (10000, 0), 2),
            (true, (0, 0), 9000),
        ] {
            let c = source.load(
                Request::Search {
                    memory: memory.clone(),
                    backward,
                    origin,
                },
                &Cancellation::default(),
            )?;
            assert_eq!(
                c.stream.as_ref().unwrap().found.as_ref().unwrap().line,
                expected
            );
        }
        Ok(())
    }
    #[test]
    fn all_encodings_mixed_endings_and_long_lines_keep_window_bounds() -> io::Result<()> {
        let t = Temp::new()?;
        for encoding in TextEncoding::ALL {
            let text = if encoding == TextEncoding::Windows1252 {
                "Café\r\nsecond\rthird\nfourth"
            } else {
                "Ёжик\r\nsecond\rthird\nfourth"
            };
            let format = encoding.default_format();
            let path = t.0.join(encoding.name());
            std::fs::write(&path, format.encode(text).map_err(error)?)?;
            let c = Source::open(&path, ViewMode::Auto, &Cancellation::default())?;
            let source = c.stream.unwrap().source.with_format(Some(format));
            let c = source.load(Request::Line(0), &Cancellation::default())?;
            assert_eq!(
                c.lines,
                text.replace("\r\n", "\n")
                    .replace('\r', "\n")
                    .lines()
                    .collect::<Vec<_>>()
            );
        }
        let path = t.0.join("long.txt");
        std::fs::write(&path, format!("{}\nlast", "a".repeat(WINDOW_BYTES * 2)))?;
        let c = Source::open(&path, ViewMode::Auto, &Cancellation::default())?;
        assert!(c.lines[0].contains("line clipped"));
        assert_eq!(c.lines[1], "last");
        assert!(c.lines.iter().map(String::len).sum::<usize>() < LINE_BYTES);
        Ok(())
    }
    #[test]
    fn hex_offsets_file_changes_and_cancellation_do_not_publish_stale_windows() -> io::Result<()> {
        let t = Temp::new()?;
        let path = t.0.join("data.bin");
        std::fs::write(&path, vec![0; 200000])?;
        let c = Source::open(&path, ViewMode::Hex, &Cancellation::default())?;
        let source = c.stream.as_ref().unwrap().source.clone();
        let c = source.load(Request::Line(10000), &Cancellation::default())?;
        assert!(c.lines[128].starts_with(&format!("{:08X}", 160000)));
        let token = Cancellation::default();
        token.cancel();
        assert_eq!(
            source.load(Request::End, &token).err().unwrap().kind(),
            io::ErrorKind::Interrupted
        );
        std::fs::write(&path, "changed")?;
        assert!(source.load(Request::End, &Cancellation::default()).is_err());
        assert_eq!(c.stream.as_ref().unwrap().target, 10000);
        Ok(())
    }
}
