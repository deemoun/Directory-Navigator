pub mod buffer;
pub(crate) mod controls;
mod editing;
pub mod highlight;
mod keys;
pub mod menus;
pub mod search;
mod search_controls;
pub mod storage;
use buffer::Buffer;
pub use controls::{
    action, open, open_path, open_path_background, paste, poll, request_close, EditorAction,
};
pub(crate) use controls::{file_key, file_mouse};
use highlight::{Cache, Language};
pub use keys::key;
use std::{path::PathBuf, sync::mpsc};
use storage::Snapshot;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PendingAction {
    Close,
    Quit,
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
    TabWidth,
    GoTo,
}
pub enum Modal {
    File(Box<crate::turbo::file_dialog::Picker>),
    Prompt {
        kind: PromptKind,
        field: String,
        cursor: usize,
        focus: usize,
    },
    Search(search::SearchDialog),
    Replace(search::ReplaceSession),
    ConfirmClose {
        focus: usize,
    },
    Help,
}
enum Reply {
    Loaded(PathBuf, Result<(Buffer, Snapshot, bool), String>),
    Saved(Result<(PathBuf, Snapshot), String>),
}
struct SaveDestination {
    path: PathBuf,
    key: PathBuf,
    save_as: bool,
}
pub struct Editor {
    pub(crate) requests: crate::requests::RequestScope,
    pub modals: crate::modals::ModalStack,
    pub path: Option<PathBuf>,
    pub base: PathBuf,
    pub buffer: Option<Buffer>,
    pub highlight: Cache,
    pub colors: bool,
    pub preferences: Preferences,
    pub search_options: search::SearchOptions,
    pub replacement: String,
    pub search_revision: u64,
    pub search_scope: Option<std::ops::Range<usize>>,
    pub scroll_row: usize,
    pub scroll_column: usize,
    pub rows: usize,
    pub columns: usize,
    pub notice: String,
    pub search: String,
    original: Option<Snapshot>,
    bom: bool,
    worker: Option<crate::task::Task<Reply>>,
    saving: bool,
    closing: Option<PendingAction>,
    save_identity: Option<PathBuf>,
    save_prepare: Option<crate::task::Task<Result<SaveDestination, String>>>,
}
impl Editor {
    fn new(base: PathBuf) -> Result<Self, String> {
        Ok(Self {
            requests: Default::default(),
            modals: Default::default(),
            path: None,
            base,
            buffer: Some(Buffer::new(String::new())?),
            highlight: Cache::new(Language::Plain),
            colors: true,
            preferences: Preferences::default(),
            search_options: search::SearchOptions::default(),
            replacement: String::new(),
            search_scope: None,
            search_revision: 0,
            scroll_row: 0,
            scroll_column: 0,
            rows: 1,
            columns: 1,
            notice: "New UTF-8 document".into(),
            search: String::new(),
            original: None,
            bom: false,
            worker: None,
            saving: false,
            closing: None,
            save_prepare: None,
            save_identity: None,
        })
    }
    fn load(path: PathBuf, base: PathBuf) -> Result<Self, String> {
        let mut editor = Self::new(base)?;
        let file = path.clone();
        let receive = editor
            .requests
            .spawn("dn-editor-load", move |_| {
                Reply::Loaded(file.clone(), storage::load(&file))
            })
            .map_err(|e| e.to_string())?;
        editor.path = Some(path);
        editor.buffer = None;
        editor.worker = Some(receive);
        editor.notice = "Reading file…".into();
        Ok(editor)
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
        self.worker.is_some() || self.save_prepare.is_some()
    }
    pub(crate) fn cancel_quit(&mut self) {
        if self.closing == Some(PendingAction::Quit) {
            self.closing = None;
            if matches!(self.modals.editor_modal(), Some(Modal::ConfirmClose { .. })) {
                self.modals.set_editor_modal(None);
            }
        }
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
        let receive = self
            .requests
            .spawn("dn-editor-save", move |_| {
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
