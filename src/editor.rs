pub mod buffer;
mod controls;
mod editing;
pub mod highlight;
mod keys;
pub mod menus;
pub mod search;
mod search_controls;
pub mod storage;
use buffer::Buffer;
pub use controls::{action, open, open_path, paste, poll, request_close, EditorAction};
use highlight::{Cache, Language};
pub use keys::key;
use std::{path::PathBuf, sync::mpsc};
use storage::Snapshot;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PendingAction {
    Close,
    Quit,
    Open(PathBuf),
}
// Compatibility for callers asking to close the editor or the application.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseIntent {
    Panel,
    Quit,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Preferences {
    pub auto_indent: bool,
    pub syntax: bool,
    pub tab_size: u8,
    pub expand_tabs: bool,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            auto_indent: true,
            syntax: true,
            tab_size: 8,
            expand_tabs: true,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptKind {
    Open,
    SaveAs,
    TabWidth,
    GoTo,
}
pub enum Modal {
    Prompt {
        kind: PromptKind,
        field: String,
        cursor: usize,
    },
    Search(search::SearchDialog),
    Replace(search::ReplaceSession),
    ConfirmClose,
    Help,
}
enum Reply {
    Loaded(PathBuf, Result<(Buffer, Snapshot, bool), String>),
    Saved(Result<(PathBuf, Snapshot), String>),
}
pub struct Editor {
    pub path: Option<PathBuf>,
    pub base: PathBuf,
    pub buffer: Option<Buffer>,
    pub highlight: Cache,
    pub colors: bool,
    pub preferences: Preferences,
    pub menu: Option<(usize, usize)>,
    pub search_options: search::SearchOptions,
    pub replacement: String,
    pub search_revision: u64,
    pub search_scope: Option<std::ops::Range<usize>>,
    pub scroll_row: usize,
    pub scroll_column: usize,
    pub rows: usize,
    pub columns: usize,
    pub modal: Option<Modal>,
    pub notice: String,
    pub search: String,
    original: Option<Snapshot>,
    bom: bool,
    worker: Option<crate::task::Task<Reply>>,
    saving: bool,
    closing: Option<PendingAction>,
}
impl Editor {
    fn new(base: PathBuf) -> Result<Self, String> {
        Ok(Self {
            path: None,
            base,
            buffer: Some(Buffer::new(String::new())?),
            highlight: Cache::new(Language::Plain),
            colors: true,
            preferences: Preferences::default(),
            menu: None,
            search_options: search::SearchOptions::default(),
            replacement: String::new(),
            search_scope: None,
            search_revision: 0,
            scroll_row: 0,
            scroll_column: 0,
            rows: 1,
            columns: 1,
            modal: None,
            notice: "New UTF-8 document".into(),
            search: String::new(),
            original: None,
            bom: false,
            worker: None,
            saving: false,
            closing: None,
        })
    }
    fn load(path: PathBuf, base: PathBuf) -> Result<Self, String> {
        let mut editor = Self::new(base)?;
        let file = path.clone();
        let receive = crate::task::Task::spawn("dn-editor-load", move |_| {
            Reply::Loaded(file.clone(), storage::load(&file))
        })
        .map_err(|e| e.to_string())?;
        editor.path = Some(path);
        editor.buffer = None;
        editor.worker = Some(receive);
        editor.notice = "Reading file…".into();
        Ok(editor)
    }
    fn load_next(&mut self, path: PathBuf) -> Result<(), String> {
        let receive = crate::task::Task::spawn("dn-editor-load", move |_| {
            Reply::Loaded(path.clone(), storage::load(&path))
        })
        .map_err(|e| e.to_string())?;
        self.worker = Some(receive);
        self.saving = false;
        self.notice = "Reading next file…".into();
        Ok(())
    }
    pub fn apply_preferences(&mut self, mut preferences: Preferences) {
        preferences.tab_size = preferences.tab_size.clamp(1, 16);
        self.preferences = preferences;
        self.colors = preferences.syntax;
        if let Some(buffer) = &mut self.buffer {
            buffer.tab_size = usize::from(preferences.tab_size);
        }
    }
    pub fn busy(&self) -> bool {
        self.worker.is_some()
    }
    pub fn dirty(&self) -> bool {
        self.buffer.as_ref().is_some_and(Buffer::dirty)
    }
    fn save(&mut self, path: PathBuf, save_as: bool) -> Result<(), String> {
        if self.busy() {
            return Ok(());
        }
        let Some(buffer) = &self.buffer else {
            return Ok(());
        };
        let text = buffer.text.clone();
        let original = if save_as { None } else { self.original.clone() };
        let bom = self.bom;
        let receive = crate::task::Task::spawn("dn-editor-save", move |_| {
            Reply::Saved(
                storage::save(&path, &text, bom, original.as_ref())
                    .map(|snapshot| (path, snapshot)),
            )
        })
        .map_err(|e| e.to_string())?;
        self.worker = Some(receive);
        self.saving = true;
        self.notice = "Saving…".into();
        Ok(())
    }
    fn recolor(&mut self) {
        let language = Language::for_path(self.path.as_deref());
        if language != self.highlight.language {
            self.highlight = Cache::new(language);
            if let Some(buffer) = &mut self.buffer {
                buffer.changed_line = 0;
            }
        }
        if let Some(buffer) = &self.buffer {
            self.highlight.refresh(buffer);
        }
    }
    pub fn keep_visible(&mut self) {
        if let Some(buffer) = &self.buffer {
            let row = buffer.row();
            let col = buffer.column();
            if row < self.scroll_row {
                self.scroll_row = row;
            }
            if row >= self.scroll_row + self.rows {
                self.scroll_row = row + 1 - self.rows;
            }
            if col < self.scroll_column {
                self.scroll_column = col;
            }
            if col >= self.scroll_column + self.columns {
                self.scroll_column = col + 1 - self.columns;
            }
        }
    }
}
