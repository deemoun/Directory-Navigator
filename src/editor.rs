mod block;
pub mod buffer;
pub(crate) mod controls;
mod dialog_controls;
mod editing;
pub use block::Case;
pub mod highlight;
mod keys;
pub mod menus;
pub mod recovery;
pub mod search;
mod search_controller;
mod search_controls;
pub mod storage;
use crate::text_encoding::{EncodingDialog, TextFormat};
use buffer::Buffer;
pub use controls::{
    action, open, open_path, open_path_background, open_smartpad, paste, poll, request_close,
    EditorAction,
};
pub(crate) use controls::{file_key, file_mouse, open_path_with_focus};
use highlight::{Cache, Language};
pub use keys::key;
use std::{path::PathBuf, sync::mpsc};
use storage::Snapshot;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PendingAction {
    Close,
    Quit,
    ReopenEncoding,
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
    ConfirmOverwrite {
        path: PathBuf,
        key: PathBuf,
        approved: Box<storage::Replacement>,
        focus: usize,
    },
    ConfirmClose {
        focus: usize,
    },
    Help,
    Encoding(EncodingDialog),
}
enum Reply {
    Loaded(PathBuf, Result<(Buffer, Snapshot, TextFormat), String>),
    Saved(Result<(PathBuf, storage::SaveOutcome, TextFormat), String>),
    EncodingBytes(Result<Vec<u8>, String>),
    ArchiveSaved {
        path: PathBuf,
        cache: storage::SaveOutcome,
        format: TextFormat,
        result: Result<Option<String>, String>,
    },
}
struct SaveDestination {
    path: PathBuf,
    key: PathBuf,
    save_as: bool,
    replacement: Option<storage::Replacement>,
}
pub struct Editor {
    pub(crate) requests: crate::requests::RequestScope,
    pub modals: crate::modals::ModalStack,
    smartpad: bool,
    pub(crate) remote_origin: Option<std::sync::Arc<crate::network::editor::Origin>>,
    pub(crate) archive_origin: Option<std::sync::Arc<crate::archive::edit::Origin>>,
    smartpad_return: Option<crate::desktop::WindowId>,
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
    pub(crate) recovery_source: Option<PathBuf>,
    recovery_generation: u64,
    pub(crate) durability_pending: Option<u64>,
    reveal_line: Option<usize>,
    format: TextFormat,
    save_format: Option<TextFormat>,
    reopen_position: Option<(usize, usize)>,
    worker: Option<crate::task::Task<Reply>>,
    saving: bool,
    closing: Option<PendingAction>,
    save_identity: Option<PathBuf>,
    save_prepare: Option<crate::task::Task<Result<SaveDestination, String>>>,
}
impl Editor {
    pub fn text_format(&self) -> TextFormat {
        self.format
    }
    pub fn is_smartpad(&self) -> bool {
        self.smartpad
    }
    pub fn label(&self) -> &'static str {
        if self.smartpad {
            "SmartPad"
        } else {
            "Editor"
        }
    }

    pub fn go_to_line(&mut self, line: usize) {
        if let Some(buffer) = &mut self.buffer {
            let row = line
                .saturating_sub(1)
                .min(buffer.lines.len().saturating_sub(1));
            let position = buffer.position_at(row, 0);
            buffer.move_to(position, false);
            self.scroll_row = row;
            self.keep_visible();
        } else {
            self.reveal_line = Some(line);
        }
    }

    fn new(base: PathBuf) -> Result<Self, String> {
        Ok(Self {
            requests: Default::default(),
            modals: Default::default(),
            smartpad: false,
            archive_origin: None,
            remote_origin: None,
            smartpad_return: None,
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
            recovery_source: None,
            recovery_generation: 0,
            durability_pending: None,
            reveal_line: None,
            format: TextFormat::default(),
            save_format: None,
            reopen_position: None,
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
            .spawn("dn-editor-load", move |cancel| {
                Reply::Loaded(
                    file.clone(),
                    storage::load_format_cancelled(&file, None, &cancel),
                )
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
    pub(crate) fn saving(&self) -> bool {
        self.saving
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
        self.durability_pending.is_some() || self.buffer.as_ref().is_some_and(Buffer::dirty)
    }
    fn save(&mut self, path: PathBuf, save_as: bool) -> Result<(), String> {
        self.save_approved(path, save_as, None)
    }
    fn save_approved(
        &mut self,
        path: PathBuf,
        save_as: bool,
        replacement: Option<storage::Replacement>,
    ) -> Result<(), String> {
        if self.busy() {
            return Ok(());
        }
        let Some(buffer) = &self.buffer else {
            return Ok(());
        };
        let text = buffer.text.clone();
        let original = if save_as { None } else { self.original.clone() };
        let format = self.save_format.unwrap_or(self.format);
        let confirm =
            !save_as && format == self.format && self.durability_pending == Some(buffer.revision());
        let archive_origin = (!save_as).then(|| self.archive_origin.clone()).flatten();
        let remote_origin = (!save_as).then(|| self.remote_origin.clone()).flatten();
        let receive = self
            .requests
            .spawn("dn-editor-save", move |cancel| {
                if let Some(origin) = remote_origin {
                    let cache = storage::save(&path, &text, format, original.as_ref());
                    return match cache {
                        Ok(cache) => Reply::ArchiveSaved {
                            path,
                            cache,
                            format,
                            result: origin.save(&cancel),
                        },
                        Err(error) => Reply::Saved(Err(error)),
                    };
                }
                if let Some(origin) = archive_origin {
                    let cache = if confirm {
                        storage::confirm_durability(
                            &path,
                            original.as_ref().expect("published snapshot"),
                        )
                    } else {
                        storage::save(&path, &text, format, original.as_ref())
                    };
                    return match cache {
                        Ok(cache) => Reply::ArchiveSaved {
                            path,
                            cache,
                            format,
                            result: if confirm {
                                origin.confirm_durability()
                            } else {
                                origin.save(&cancel)
                            },
                        },
                        Err(error) => Reply::Saved(Err(error)),
                    };
                }
                Reply::Saved(
                    (if confirm {
                        storage::confirm_durability(
                            &path,
                            original.as_ref().expect("published snapshot"),
                        )
                    } else if let Some(approved) = &replacement {
                        storage::save_replacing(&path, &text, format, approved)
                    } else {
                        storage::save(&path, &text, format, original.as_ref())
                    })
                    .map(|snapshot| (path, snapshot, format)),
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
            self.highlight.schedule_markdown(buffer, &self.requests);
        }
    }
    pub(crate) fn poll_markdown(&mut self) -> bool {
        if let Some(buffer) = &self.buffer {
            self.highlight.schedule_markdown(buffer, &self.requests);
            let changed = self.highlight.poll_markdown(buffer);
            if let Some(notice) = self.highlight.md_notice.take() {
                self.notice = notice;
            }
            changed
        } else {
            false
        }
    }
    pub(crate) fn markdown_pending(&self) -> bool {
        self.highlight.markdown_pending()
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

impl Editor {
    pub(crate) fn session_document(
        path: PathBuf,
        loaded: (Buffer, Snapshot, TextFormat),
        preferences: Preferences,
    ) -> Result<Self, String> {
        let mut editor = Self::new(
            path.parent()
                .ok_or("Missing document directory")?
                .to_owned(),
        )?;
        let (buffer, snapshot, format) = loaded;
        editor.highlight = Cache::new(Language::for_path(Some(&path)));
        editor.path = Some(path);
        editor.buffer = Some(buffer);
        editor.original = Some(snapshot);
        editor.format = format;
        editor.apply_preferences(preferences);
        editor.notice.clear();
        Ok(editor)
    }
}
