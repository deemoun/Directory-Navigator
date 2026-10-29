mod navigation;
use crate::filesystem::open_regular;
pub use navigation::{action, key, paste, Action, Match, SearchDialog, SearchMemory, VisualRow};
use std::{
    io::{self, Read},
    path::{Path, PathBuf},
};
use unicode_segmentation::UnicodeSegmentation;

pub const MAX_BYTES: usize = 2 * 1024 * 1024;
const MAX_LINES: usize = 100_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    Auto,
    Text,
    Hex,
}

pub struct Contents {
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
    Ok(from_bytes(bytes, mode))
}

/// Decode an in-memory, bounded preview using the ordinary viewer's rules.
pub fn from_bytes(mut bytes: Vec<u8>, mode: ViewMode) -> Contents {
    let mut truncated = bytes.len() > MAX_BYTES;
    bytes.truncate(MAX_BYTES);
    let detected_binary = bytes.contains(&0)
        || std::str::from_utf8(&bytes)
            .is_err_and(|error| error.error_len().is_some() || !truncated);
    let binary = match mode {
        ViewMode::Auto => detected_binary,
        ViewMode::Text => false,
        ViewMode::Hex => true,
    };
    let mut lines = Vec::new();
    if binary {
        for (index, chunk) in bytes.chunks(16).take(MAX_LINES + 1).enumerate() {
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
    } else {
        let text = String::from_utf8_lossy(&bytes);
        for line in text.lines().take(MAX_LINES + 1) {
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
    }
    if lines.len() > MAX_LINES {
        truncated = true;
        lines.truncate(MAX_LINES);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    Contents {
        lines,
        binary,
        truncated,
    }
}

pub struct Viewer {
    pub(crate) requests: crate::requests::RequestScope,
    pub modals: crate::modals::ModalStack,
    pub requested_mode: ViewMode,
    pub path: PathBuf,
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
        Ok(viewer)
    }
    pub fn from_contents(path: PathBuf, contents: Contents) -> Self {
        let mut viewer = Self::empty(path, None);
        viewer.requested_mode = if contents.binary {
            ViewMode::Hex
        } else {
            ViewMode::Text
        };
        viewer.contents = Some(contents);
        viewer
    }
    fn empty(path: PathBuf, pending: Option<crate::task::Task<Result<Contents, String>>>) -> Self {
        Self {
            path,
            modals: Default::default(),
            requested_mode: ViewMode::Auto,
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
            pending,
        }
    }

    pub fn poll(&mut self) -> Option<Result<(), String>> {
        Some(match crate::task::take_reply(&mut self.pending)? {
            Ok(result) => result.map(|contents| {
                self.contents = Some(contents);
                self.layout_width = 0;
                self.rows.clear();
            }),
            Err(_) => Err("Viewer worker stopped".into()),
        })
    }

    pub fn step(&mut self, amount: isize) {
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
