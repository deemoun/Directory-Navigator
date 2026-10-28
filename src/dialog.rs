pub struct Dialog {
    pub title: &'static str,
    pub message: String,
}

use crate::operations::Kind;
use std::path::PathBuf;

pub enum PromptKind {
    ImportDos,
    Operation(Kind),
    GoTo,
}

pub struct Prompt {
    pub kind: PromptKind,
    pub sources: Vec<PathBuf>,
    pub base: PathBuf,
    pub field: String,
    /// UTF-8 byte position, always on a character boundary.
    pub cursor: usize,
    /// Input, OK, Cancel, Help.
    pub focus: usize,
    /// Shared Copy/Rename-move draft, including native destination receipts.
    pub copy: Option<crate::copy_dialog::Draft>,
}

impl Prompt {
    pub fn paste(&mut self, text: &str) {
        if self.focus == 0 {
            let previous = self.field.clone();
            paste_input(&mut self.field, &mut self.cursor, text);
            if self.field != previous {
                if let Some(copy) = &mut self.copy {
                    copy.picked = None;
                }
            }
        }
    }
    pub fn title(&self) -> &'static str {
        match self.kind {
            PromptKind::Operation(Kind::Copy) => " Copy to ",
            PromptKind::Operation(Kind::Move) => " Move / rename to ",
            PromptKind::Operation(Kind::Delete) => " Confirm permanent deletion ",
            PromptKind::Operation(Kind::Trash) => " Move to Trash ",
            PromptKind::Operation(Kind::Mkdir) => " Make directory ",
            PromptKind::GoTo => " Go to directory ",
            PromptKind::ImportDos => " Import DOS settings ",
        }
    }

    pub fn edit(&mut self, code: crossterm::event::KeyCode) {
        let previous = self.field.clone();
        edit_input(&mut self.field, &mut self.cursor, code);
        if self.field != previous {
            if let Some(copy) = &mut self.copy {
                copy.picked = None;
            }
        }
    }
}

pub fn paste_input(field: &mut String, cursor: &mut usize, text: &str) {
    let clean: String = text
        .chars()
        .filter(|c| !c.is_control())
        .take(4096)
        .collect();
    if field.len() + clean.len() <= 16_384 {
        crate::turbo::widgets::edit_input(
            field,
            cursor,
            crossterm::event::KeyCode::Null,
            Some(&clean),
        );
    }
}

pub fn edit_input(field: &mut String, cursor: &mut usize, code: crossterm::event::KeyCode) {
    crate::turbo::widgets::edit_input(field, cursor, code, None);
}

pub fn prompt_help(app: &mut crate::app::App) {
    let Some(prompt) = app.modals.prompt() else {
        return;
    };
    let message = match prompt.kind {
        PromptKind::Operation(Kind::Mkdir) => "Enter a directory name or path.\nRelative paths use the active panel directory.\nExisting files and directories are never overwritten.",
        PromptKind::ImportDos => "Choose an original DN.CFG file. A read-only preview shows supported preferences before Apply. DOS paths, commands and desktop windows are not imported.",
        PromptKind::GoTo => "Enter an absolute or relative directory path.\nRelative paths use the active panel directory.",
        PromptKind::Operation(Kind::Delete) => "Permanently deletes the listed files and directories.\nSymbolic links are removed without following targets.\nCancel leaves everything unchanged.",
        PromptKind::Operation(Kind::Trash) => "Moves the listed items to the system Trash.\nRestore items using your desktop file manager.\nLinks themselves are moved; targets are not followed.\nIf Trash is unavailable, the operation stops without\nfalling back to permanent deletion.",
        PromptKind::Operation(Kind::Copy) => "Copy uses the inactive panel directory for a blank destination.\nRefuse stops on existing destinations; Skip leaves each existing top-level file or directory untouched. Directories are not merged.\nOverwrite, append, refresh and description copying are unavailable.\nCheck free disk space estimates source file lengths before writing; filesystem overhead and later space changes can still cause errors.\nVerify disk writes reads back staged bytes before publication. Remove source files forces verification and removes sources only after the whole copied batch succeeds. Skipped sources remain.\nF4 opens destination history; Tree chooses a directory without navigating either panel. Esc cancels.",
        PromptKind::Operation(Kind::Move) => "Move/rename uses the inactive panel directory for a blank destination; Shift+F6 uses the active directory. Relative paths use that base. A single item may use a new name; multiple items need an existing directory.\nRefuse stops before changes if a top-level destination exists. Skip leaves existing files, folders and their sources untouched; folders are never merged.\nNative no-replace rename is preferred. Check free disk space applies when copying is required. Verify disk writes uses staged copying, reads back the bytes, then removes sources through guarded batch cleanup.\nRemove source files is always selected for Move. Overwrite, append, refresh and description copying are unavailable.\nF4 opens the shared destination history; Tree chooses a directory without navigating either panel. Esc cancels; completed moves remain completed.",
    };
    app.push_dialog(Dialog {
        title: " Help ",
        message: message.into(),
    });
}

pub fn message_key(app: &mut crate::app::App, code: crossterm::event::KeyCode) {
    use crossterm::event::KeyCode;
    match code {
        KeyCode::Esc | KeyCode::Enter => {
            app.pop_dialog();
        }
        KeyCode::Up => app.set_message_offset(app.message_offset().saturating_sub(1)),
        KeyCode::Down => app.set_message_offset(app.message_offset().saturating_add(1)),
        KeyCode::PageUp => app.set_message_offset(app.message_offset().saturating_sub(8)),
        KeyCode::PageDown => app.set_message_offset(app.message_offset().saturating_add(8)),
        KeyCode::Home => app.set_message_offset(0),
        KeyCode::End => app.set_message_offset(usize::MAX),
        _ => {}
    }
}

/// File-manager help is also available from the welcome window.
pub fn keyboard_help(app: &mut crate::app::App) {
    app.push_dialog(Dialog {
        title: " Directory Navigator — Help ",
        message: if app.config.shortcuts.is_mac() {
            crate::shortcuts::MANAGER_HELP.into()
        } else {
            crate::commands::help::CLASSIC_MANAGER.into()
        },
    });
    if app.config.shortcuts.is_mac() {
        let language = app.config.language;
        if let Some(help) = app.last_dialog_mut() {
            help.message.push('\n');
            help.message.push_str(
                language.text("Options: Import DOS settings previews DN.CFG preferences"),
            );
        }
    }
}
