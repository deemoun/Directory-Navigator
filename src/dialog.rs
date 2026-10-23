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
}

impl Prompt {
    pub fn paste(&mut self, text: &str) {
        if self.focus == 0 {
            paste_input(&mut self.field, &mut self.cursor, text);
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
        edit_input(&mut self.field, &mut self.cursor, code);
    }
}

pub fn paste_input(field: &mut String, cursor: &mut usize, text: &str) {
    let clean: String = text
        .chars()
        .filter(|c| !c.is_control())
        .take(4096)
        .collect();
    if field.len() + clean.len() <= 16_384 {
        field.insert_str(*cursor, &clean);
        *cursor += clean.len();
    }
}

pub fn edit_input(field: &mut String, cursor: &mut usize, code: crossterm::event::KeyCode) {
    use crossterm::event::KeyCode;
    match code {
        KeyCode::Char(c) if !c.is_control() && field.len() < 16_384 => {
            field.insert(*cursor, c);
            *cursor += c.len_utf8();
        }
        KeyCode::Left => {
            *cursor = field[..*cursor]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0)
        }
        KeyCode::Right => {
            *cursor += field[*cursor..]
                .chars()
                .next()
                .map(char::len_utf8)
                .unwrap_or(0)
        }
        KeyCode::Home => *cursor = 0,
        KeyCode::End => *cursor = field.len(),
        KeyCode::Backspace if *cursor > 0 => {
            let previous = field[..*cursor]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
            field.drain(previous..*cursor);
            *cursor = previous;
        }
        KeyCode::Delete if *cursor < field.len() => {
            field.remove(*cursor);
        }
        _ => {}
    }
}

pub fn prompt_help(app: &mut crate::app::App) {
    let Some(prompt) = &app.prompt else {
        return;
    };
    let message = match prompt.kind {
        PromptKind::Operation(Kind::Mkdir) => "Enter a directory name or path.\nRelative paths use the active panel directory.\nExisting files and directories are never overwritten.",
        PromptKind::ImportDos => "Choose an original DN.CFG file. A read-only preview shows supported preferences before Apply. DOS paths, commands and desktop windows are not imported.",
        PromptKind::GoTo => "Enter an absolute or relative directory path.\nRelative paths use the active panel directory.",
        PromptKind::Operation(Kind::Delete) => "Permanently deletes the listed files and directories.\nSymbolic links are removed without following targets.\nCancel leaves everything unchanged.",
        PromptKind::Operation(Kind::Trash) => "Moves the listed items to the system Trash.\nRestore items using your desktop file manager.\nLinks themselves are moved; targets are not followed.\nIf Trash is unavailable, the operation stops without\nfalling back to permanent deletion.",
        _ => "Enter a destination path. A blank path uses the base directory.\nExisting destinations are refused; files are never overwritten.\nSymbolic links are copied as links.",
    };
    app.dialogs.push(Dialog {
        title: " Help ",
        message: message.into(),
    });
}

pub fn message_key(app: &mut crate::app::App, code: crossterm::event::KeyCode) {
    use crossterm::event::KeyCode;
    match code {
        KeyCode::Esc | KeyCode::Enter => {
            app.dialogs.pop();
            app.message_offset = 0;
        }
        KeyCode::Up => app.message_offset = app.message_offset.saturating_sub(1),
        KeyCode::Down => app.message_offset = app.message_offset.saturating_add(1),
        KeyCode::PageUp => app.message_offset = app.message_offset.saturating_sub(8),
        KeyCode::PageDown => app.message_offset = app.message_offset.saturating_add(8),
        KeyCode::Home => app.message_offset = 0,
        KeyCode::End => app.message_offset = usize::MAX,
        _ => {}
    }
}

/// File-manager help is also available from the welcome window.
pub fn keyboard_help(app: &mut crate::app::App) {
    app.dialogs.push(Dialog { title: " Directory Navigator — Help ",
            message: if app.config.shortcuts.is_mac() { crate::shortcuts::MANAGER_HELP.into() } else { "Tab panels · Arrows/PgUp/PgDn/Home/End navigate\nAlt+F/U/P/M/O menus · Alt+X exits\nEnter app/dir/archive · Ctrl+Enter OS app · F3 view · F4 edit · BS parent\nIns / Space marks · Ctrl+A all · Ctrl+U clear\n+ / - group · Alt+E info · Alt+K columns\nAlt+F7 quick search · Ctrl+F7 recursive Find\nCtrl+Q quick view · Ctrl+T tree · Alt+F9 Tetris\nCtrl+1–9 bookmarks set · Alt+1–9 go · Alt+Del filter\nCtrl+K calculator · Ctrl+B ASCII · Alt+Y memory\nCtrl+C compare · Alt+L list file\nAlt+BS directories · Alt+PgUp edits / PgDn views\nF2 user menu · Shift+F2 sort · Ctrl+F3 name / F4 ext / F5 date / F6 size\nF5 copy · F6 move · Shift+F6 rename here\nF7 mkdir · F8 Trash/delete · F9 menu · F10 exit\nCtrl+H hidden · Ctrl+L go to · Ctrl+R refresh\nClick row · Ctrl+click mark · Double-click open\nWheel scrolls · Esc cancels dialogs/operations\nType commands · Enter runs · Esc clears · Ctrl+E/X history · Alt+F8 list\nFile menu: Copy names / paths uses internal clipboard\nOptions: File associations configures Enter actions\nOptions: Import DOS settings previews DN.CFG preferences\nColors: white dirs · green exec · cyan source\nMagenta archives · blue links · yellow marks\nTrash by default; File Manager Setup changes mode.\nExisting destinations are refused; no overwrites.".into() } });
    if app.config.shortcuts.is_mac() {
        if let Some(help) = app.dialogs.last_mut() {
            help.message.push('\n');
            help.message.push_str(
                app.config
                    .language
                    .text("Options: Import DOS settings previews DN.CFG preferences"),
            );
        }
    }
}
