pub(crate) mod markdown;
mod navigation;
pub mod pdf;
mod search_controller;
pub mod streaming;
use crate::filesystem::open_regular;
pub use navigation::{
    action, key, paste, Action, GoToDialog, Match, SearchDialog, SearchMemory, VisualRow,
};
use std::{
    io::{self, Read},
    path::{Path, PathBuf},
};
use unicode_segmentation::UnicodeSegmentation;

pub const MAX_BYTES: usize = crate::limits::VIEWER_BYTES;
const MAX_LINES: usize = crate::limits::VIEWER_LINES;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    Auto,
    Text,
    Hex,
}

#[derive(Clone)]
pub struct Contents {
    pub stream: Option<std::sync::Arc<streaming::Window>>,
    pub pdf: Option<std::sync::Arc<pdf::Info>>,
    pub source: Option<std::sync::Arc<str>>,
    pub raw: Option<std::sync::Arc<[u8]>>,
    pub format: Option<crate::text_encoding::TextFormat>,
    pub encoding_needed: bool,
    pub raw_truncated: bool,
    pub loaded_bytes: usize,
    pub lines: Vec<String>,
    pub binary: bool,
    pub truncated: bool,
}

pub fn load(path: &Path) -> io::Result<Contents> {
    load_mode(path, ViewMode::Auto)
}

pub fn load_mode(path: &Path, mode: ViewMode) -> io::Result<Contents> {
    load_mode_cancelled(path, mode, &crate::task::Cancellation::default(), true)
}
fn load_mode_cancelled(
    path: &Path,
    mode: ViewMode,
    cancel: &crate::task::Cancellation,
    resolve_link: bool,
) -> io::Result<Contents> {
    cancel.check()?;
    // Viewing is an explicit read action; resolve a chosen link, then reject
    // replacement links and special files at the actual open.
    let resolved = if resolve_link {
        std::fs::canonicalize(path)?
    } else {
        path.to_owned()
    };
    let input = open_regular(&resolved)?;
    if resolve_link
        && input.metadata()?.len() > MAX_BYTES as u64
        && !(mode == ViewMode::Auto && pdf::is_path(path))
    {
        return streaming::Source::open(&resolved, mode, cancel);
    }
    let mut bytes = Vec::new();
    let mut input = input.take(MAX_BYTES as u64 + 1);
    let mut chunk = [0u8; 64 * 1024];
    loop {
        cancel.check()?;
        let n = input.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..n]);
    }
    if resolve_link {
        pdf::document_bytes(bytes, mode, pdf::is_path(path), cancel)
    } else {
        from_bytes_cancelled(bytes, mode, cancel)
    }
}

/// Decode an in-memory, bounded preview using the ordinary viewer's rules.
pub fn from_bytes(bytes: Vec<u8>, mode: ViewMode) -> Contents {
    from_bytes_cancelled(bytes, mode, &crate::task::Cancellation::default())
        .expect("uncancelled preview")
}
pub(crate) fn from_bytes_cancelled(
    bytes: Vec<u8>,
    mode: ViewMode,
    cancel: &crate::task::Cancellation,
) -> io::Result<Contents> {
    from_remote_bytes(bytes, mode, false, cancel)
}
pub(crate) fn from_remote_bytes(
    mut bytes: Vec<u8>,
    mode: ViewMode,
    source_truncated: bool,
    cancel: &crate::task::Cancellation,
) -> io::Result<Contents> {
    cancel.check()?;
    let mut truncated = source_truncated || bytes.len() > MAX_BYTES;
    bytes.truncate(MAX_BYTES);
    let raw_truncated = truncated;
    let detected = crate::text_encoding::detect(&bytes, truncated)
        .ok()
        .flatten()
        .or_else(|| {
            // Explicit As Text may display valid UTF-8 controls safely as dots;
            // it still never performs lossy decoding of unknown bytes.
            (mode == ViewMode::Text
                && crate::text_encoding::TextFormat::default()
                    .decode(&bytes, raw_truncated)
                    .is_ok())
            .then_some(crate::text_encoding::TextFormat::default())
        });
    let encoding_needed =
        detected.is_none() && crate::text_encoding::plausible_text(&bytes) && mode != ViewMode::Hex;
    let binary = mode == ViewMode::Hex || detected.is_none();
    if !binary {
        if let Some(text) = detected.and_then(|format| format.decode(&bytes, raw_truncated).ok()) {
            let mut c = text_contents_cancelled(&text, cancel)?;
            c.loaded_bytes = bytes.len();
            c.raw = Some(bytes.into());
            c.format = detected;
            c.truncated |= raw_truncated;
            c.raw_truncated = raw_truncated;
            return Ok(c);
        }
    }
    let source = None;
    let mut lines = Vec::new();
    for (index, chunk) in bytes.chunks(16).take(MAX_LINES + 1).enumerate() {
        if index % 1024 == 0 {
            cancel.check()?;
        }
        let hex = chunk
            .iter()
            .map(|b| format!("{b:02X} "))
            .collect::<String>();
        let ascii: String = chunk
            .iter()
            .map(|b| {
                if b.is_ascii_graphic() || *b == b' ' {
                    char::from(*b)
                } else {
                    '.'
                }
            })
            .collect();
        lines.push(format!("{:08X}  {hex:<48} │{ascii}│", index * 16));
    }
    if lines.len() > MAX_LINES {
        truncated = true;
        lines.truncate(MAX_LINES);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    let loaded_bytes = if binary {
        bytes.len().min(MAX_LINES * 16)
    } else {
        bytes.len()
    };
    Ok(Contents {
        stream: None,
        pdf: None,
        raw_truncated,
        raw: Some(bytes.into()),
        format: detected,
        encoding_needed,
        source,
        loaded_bytes,
        lines,
        binary,
        truncated,
    })
}

pub fn from_encoded_bytes(
    bytes: std::sync::Arc<[u8]>,
    format: crate::text_encoding::TextFormat,
    truncated: bool,
) -> Result<Contents, String> {
    if bytes.len() > MAX_BYTES {
        return Err("Encoded preview exceeds the 64 MiB limit.".into());
    }
    let text = format.decode(&bytes, truncated)?;
    let mut contents = text_contents(&text);
    contents.raw = Some(bytes.clone());
    contents.loaded_bytes = bytes.len();
    contents.format = Some(format);
    contents.truncated |= truncated;
    contents.raw_truncated = truncated;
    Ok(contents)
}
/// Already-decoded snapshots never acquire a filesystem encoding or path.
pub fn text_contents(text: &str) -> Contents {
    text_contents_cancelled(text, &crate::task::Cancellation::default()).expect("uncancelled text")
}
fn text_contents_cancelled(text: &str, cancel: &crate::task::Cancellation) -> io::Result<Contents> {
    let mut end = text.len().min(crate::limits::DECODED_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut truncated = end < text.len();
    let text = &text[..end];
    let mut lines = Vec::new();
    for (index, line) in text.lines().take(MAX_LINES + 1).enumerate() {
        if index % 256 == 0 {
            cancel.check()?;
        }
        let mut output = String::new();
        let mut count = 0;
        for grapheme in line.graphemes(true) {
            let chars = grapheme.chars().count();
            if count + chars > 4096 {
                output.push_str(" … [line clipped]");
                truncated = true;
                break;
            }
            count += chars;
            for c in grapheme.chars() {
                match c {
                    '\t' => output.push_str("    "),
                    c if c.is_control() => output.push('·'),
                    c => output.push(c),
                }
            }
        }
        lines.push(output);
    }
    if lines.len() > MAX_LINES {
        lines.truncate(MAX_LINES);
        truncated = true;
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    Ok(Contents {
        stream: None,
        pdf: None,
        source: Some(std::sync::Arc::from(text)),
        raw: None,
        format: None,
        encoding_needed: false,
        raw_truncated: false,
        loaded_bytes: text.len(),
        lines,
        binary: false,
        truncated,
    })
}

pub struct Viewer {
    session_deferred_prompts: bool,
    pub(crate) requests: crate::requests::RequestScope,
    pub modals: crate::modals::ModalStack,
    pub requested_mode: ViewMode,
    pub markdown: markdown::State,
    pub preview_editor: Option<crate::desktop::WindowId>,
    pub path: PathBuf,
    memory_only: bool,
    pub contents: Option<Contents>,
    pub offset: usize,
    pub horizontal: u16,
    pub wrap: bool,
    pub rows: Vec<VisualRow>,
    layout_width: usize,
    mapped_wrap: bool,
    pub current: Option<Match>,
    last_search: SearchMemory,
    pub notice: String,
    pending: Option<crate::task::Task<Result<Contents, String>>>,
    reveal_line: Option<usize>,
    encoding_pending: Option<crate::task::Task<Result<Contents, String>>>,
    encoding_anchor: usize,
    stream_pending: Option<crate::task::Task<Result<Contents, String>>>,
}

impl Viewer {
    pub fn open(path: PathBuf) -> io::Result<Self> {
        Self::open_mode(path, ViewMode::Auto)
    }

    pub fn open_mode(path: PathBuf, mode: ViewMode) -> io::Result<Self> {
        Self::open_internal(path, mode, true)
    }
    pub fn open_preview(path: PathBuf) -> io::Result<Self> {
        Self::open_internal(path, ViewMode::Auto, false)
    }
    fn open_internal(path: PathBuf, mode: ViewMode, resolve_link: bool) -> io::Result<Self> {
        let source = path.clone();
        let requests = crate::requests::RequestScope::default();
        let pending = requests.spawn("dn-viewer", move |cancel| {
            load_mode_cancelled(&source, mode, &cancel, resolve_link)
                .map_err(|e| format!("{}: {e}", source.display()))
        })?;
        let mut viewer = Self::empty(path, Some(pending));
        viewer.requests = requests;
        viewer.requested_mode = mode;
        viewer.markdown.hint = resolve_link && crate::markdown::is_path(&viewer.path);
        viewer.markdown.requested = mode == ViewMode::Auto;
        Ok(viewer)
    }
    pub(crate) fn from_archive_contents(
        label: PathBuf,
        contents: Contents,
        markdown_hint: bool,
    ) -> Self {
        let mut viewer = Self::from_contents(label, contents);
        viewer.memory_only = true;
        viewer.markdown.hint =
            markdown_hint && viewer.contents.as_ref().is_none_or(|c| c.pdf.is_none());
        viewer.markdown.requested = true;
        viewer
    }
    pub(crate) fn native_path(&self) -> Option<&Path> {
        (!self.memory_only).then_some(self.path.as_path())
    }
    pub fn from_contents(path: PathBuf, contents: Contents) -> Self {
        let mut viewer = Self::empty(path, None);
        viewer.requested_mode = if contents.binary {
            ViewMode::Hex
        } else {
            ViewMode::Text
        };
        viewer.wrap = contents.pdf.is_some();
        viewer.contents = Some(contents);
        viewer.offer_encoding();
        viewer
    }
    fn empty(path: PathBuf, pending: Option<crate::task::Task<Result<Contents, String>>>) -> Self {
        Self {
            path,
            memory_only: false,
            modals: Default::default(),
            requested_mode: ViewMode::Auto,
            markdown: Default::default(),
            preview_editor: None,
            requests: Default::default(),
            contents: None,
            offset: 0,
            horizontal: 0,
            wrap: false,
            rows: Vec::new(),
            layout_width: 0,
            mapped_wrap: false,
            current: None,
            last_search: SearchMemory::default(),
            notice: String::new(),
            session_deferred_prompts: false,
            pending,
            reveal_line: None,
            encoding_pending: None,
            encoding_anchor: 0,
            stream_pending: None,
        }
    }

    pub(crate) fn pending_work(&self) -> bool {
        self.pending.is_some()
            || self.stream_pending.is_some()
            || self.encoding_pending.is_some()
            || self.markdown.pending()
    }
    pub fn poll(&mut self) -> Option<Result<(), String>> {
        if self.poll_stream() {
            return Some(Ok(()));
        }
        if self.poll_pdf_password() {
            return Some(Ok(()));
        }
        if self.modals.viewer_encoding_mut().is_some_and(|d| d.poll()) {
            return Some(Ok(()));
        }
        if let Some(reply) = crate::task::take_reply(&mut self.encoding_pending) {
            match reply {
                Ok(Ok(contents)) => {
                    let hint = self.markdown.hint;
                    let requested = self.markdown.requested || self.markdown.rendered;
                    self.markdown = Default::default();
                    self.markdown.hint = hint;
                    self.markdown.requested = requested;
                    self.offset = self
                        .encoding_anchor
                        .min(contents.lines.len().saturating_sub(1));
                    self.contents = Some(contents);
                    self.rows.clear();
                    self.layout_width = 0;
                    self.current = None;
                    self.horizontal = 0;
                    self.notice.clear();
                }
                Ok(Err(error)) => self.notice = error,
                Err(_) => self.notice = "Decoding worker stopped".into(),
            }
            return Some(Ok(()));
        }

        if self.pending.is_none() {
            return self.poll_markdown();
        }
        Some(match crate::task::take_reply(&mut self.pending)? {
            Ok(result) => result.map(|contents| {
                self.wrap = contents.pdf.is_some();
                if contents.pdf.is_some() {
                    self.markdown.hint = false;
                }
                if contents.stream.is_some() {
                    self.markdown.hint = false;
                }
                self.contents = Some(contents);
                self.offer_encoding();
                self.layout_width = 0;
                self.rows.clear();
            }),
            Err(_) => Err("Viewer worker stopped".into()),
        })
    }

    fn offer_encoding(&mut self) {
        if self.session_deferred_prompts {
            return;
        }
        if let Some(c) = &self.contents {
            if let Some(pdf) = &c.pdf {
                if pdf.locked && self.modals.viewer_pdf_password().is_none() {
                    if let Some(bytes) = &c.raw {
                        self.modals
                            .set_viewer_pdf_password(Some(pdf::password::Dialog::new(
                                bytes.clone(),
                            )));
                    }
                }
                if pdf.empty {
                    self.notice = pdf::EMPTY_NOTICE.into();
                }
                return;
            }
            if let Some(raw) = &c.raw {
                if let Err(error) = crate::text_encoding::detect(raw, c.raw_truncated) {
                    self.notice = error;
                }
            }
            if c.encoding_needed {
                if let Some(bytes) = &c.raw {
                    self.modals.set_viewer_encoding(Some(
                        crate::text_encoding::EncodingDialog::new(
                            crate::text_encoding::TextEncoding::Cp866.default_format(),
                            bytes.clone(),
                            c.raw_truncated,
                            false,
                            true,
                        ),
                    ));
                }
            }
        }
    }
    pub fn encoding_available(&self) -> bool {
        self.contents
            .as_ref()
            .is_some_and(|c| c.raw.is_some() && c.pdf.is_none())
    }
    pub(crate) fn apply_encoding(
        &mut self,
        format: Option<crate::text_encoding::TextFormat>,
    ) -> Result<(), String> {
        let c = self.contents.as_ref().ok_or("Wait for the file to load.")?;
        if c.pdf.is_some() {
            return Err(
                "PDF text uses its embedded fonts; a text encoding cannot be selected.".into(),
            );
        }
        if let Some(window) = &c.stream {
            let source = window.source.with_format(format);
            return self.stream_request(source, streaming::Request::Line(0));
        }
        let bytes = c
            .raw
            .clone()
            .ok_or("This is an already-decoded Unicode snapshot.")?;
        self.encoding_anchor = self.source_line();
        let truncated = c.raw_truncated;
        self.encoding_pending = Some(
            self.requests
                .spawn("dn-viewer-encoding", move |cancel| {
                    cancel.check().map_err(|e| e.to_string())?;
                    let result = if let Some(format) = format {
                        from_encoded_bytes(bytes, format, truncated)
                    } else {
                        let mut c = from_bytes(bytes.to_vec(), ViewMode::Hex);
                        c.truncated |= truncated;
                        Ok(c)
                    };
                    cancel.check().map_err(|e| e.to_string())?;
                    result
                })
                .map_err(|e| e.to_string())?,
        );
        self.notice = "Decoding text…".into();
        Ok(())
    }
    pub fn step(&mut self, amount: isize) {
        if let Some(window) = self.contents.as_ref().and_then(|c| c.stream.clone()) {
            let local = self.anchor().0;
            let total = self.contents.as_ref().unwrap().lines.len();
            let request = if amount == isize::MAX {
                Some(streaming::Request::End)
            } else if amount < 0 && self.offset < amount.unsigned_abs() && window.first > 0 {
                Some(streaming::Request::Line(
                    (window.first + local).saturating_add_signed(amount),
                ))
            } else if amount > 0
                && self.offset.saturating_add(amount as usize) >= self.rows.len().max(total)
                && !window.eof
            {
                Some(streaming::Request::Line(window.first + total))
            } else {
                None
            };
            if let Some(request) = request {
                if let Err(error) = self.stream_request(window.source.clone(), request) {
                    self.notice = error;
                }
                return;
            }
        }
        let last = if self.rows.is_empty() {
            self.contents
                .as_ref()
                .map(|c| c.lines.len().saturating_sub(1))
                .unwrap_or(0)
        } else {
            self.rows.len().saturating_sub(1)
        };
        self.offset = self.offset.saturating_add_signed(amount).min(last);
    }
}

pub(crate) fn session_load(
    path: &Path,
    mode: ViewMode,
    format: Option<crate::text_encoding::TextFormat>,
    cancel: &crate::task::Cancellation,
) -> io::Result<Contents> {
    let mut contents = load_mode_cancelled(path, mode, cancel, true)?;
    if let Some(format) = format.filter(|_| !contents.binary && contents.pdf.is_none()) {
        if let Some(window) = &contents.stream {
            contents = window
                .source
                .with_format(Some(format))
                .load(streaming::Request::Line(0), cancel)?;
        } else if let Some(raw) = &contents.raw {
            contents = from_encoded_bytes(raw.clone(), format, contents.raw_truncated)
                .map_err(io::Error::other)?;
        }
    }
    Ok(contents)
}
impl Viewer {
    pub(crate) fn session_document(
        path: PathBuf,
        contents: Contents,
        record: &crate::session::DocumentRecord,
    ) -> Self {
        let mut viewer = Self::empty(path, None);
        viewer.requested_mode = record.mode;
        viewer.markdown.hint = viewer
            .path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.eq_ignore_ascii_case("md") || s.eq_ignore_ascii_case("markdown"));
        viewer.markdown.hint &=
            contents.stream.is_none() && contents.pdf.is_none() && !contents.binary;
        viewer.markdown.requested = record.markdown;
        viewer.wrap = record.wrap;
        viewer.contents = Some(contents);
        viewer.session_deferred_prompts = true;
        viewer
    }
    pub(crate) fn session_release_prompts(&mut self) -> bool {
        if !self.session_deferred_prompts {
            return false;
        }
        self.session_deferred_prompts = false;
        if self
            .contents
            .as_ref()
            .is_some_and(|c| c.pdf.is_some() || c.format.is_none())
        {
            self.offer_encoding();
        }
        true
    }
    pub(crate) fn session_position(&mut self, record: &crate::session::DocumentRecord) {
        let Some(c) = &self.contents else {
            return;
        };
        let position = if let Some(pdf) = &c.pdf {
            record.position.min(pdf.page_lines.len().saturating_sub(1)) + 1
        } else if c.binary {
            record.position.min(
                c.stream
                    .as_ref()
                    .map_or(c.loaded_bytes as u64, |w| w.source.bytes)
                    .saturating_sub(1) as usize,
            )
        } else if c.stream.is_some() {
            record.position.saturating_add(1)
        } else {
            record
                .position
                .min(self.source_line_count().saturating_sub(1))
                + 1
        };
        let _ = self.go_to(&position.to_string());
        self.horizontal = record.horizontal;
        self.wrap = record.wrap;
    }
}
