//! Session-only command prompt; commands share the interactive user-menu runner.
use crate::presentation::layout::Rect;
use crate::{
    app::App,
    user_menu::runner::{CommandSpec, Plan},
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::{ffi::OsString, io};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
pub mod history;
const MAX_BYTES: usize = 16 * 1024;
#[derive(Default)]
pub struct CommandLine {
    pub modals: crate::modals::ModalStack,
    pub text: String,
    pub cursor: usize,
    pub pending: Option<Plan>,
    history: Vec<String>,
    recalled: Option<usize>,
    draft: String,
    view_start: usize,
    input_x: u16,
}
impl CommandLine {
    pub(crate) fn translate_input(&mut self, x: u16) {
        self.input_x = self.input_x.saturating_add(x);
    }
    pub fn history(&self) -> &[String] {
        &self.history
    }
    pub fn insert(&mut self, text: &str) -> io::Result<()> {
        if text.chars().any(char::is_control) {
            return Err(io::Error::other(
                "Commands must be a single line without control characters",
            ));
        }
        if self.text.len().saturating_add(text.len()) > MAX_BYTES {
            return Err(io::Error::other("Command exceeds 16 KiB"));
        }
        self.text.insert_str(self.cursor, text);
        let end = self.cursor + text.len();
        self.cursor = self
            .text
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .find(|i| *i >= end)
            .unwrap_or(self.text.len());
        self.recalled = None;
        Ok(())
    }
    fn snap_cursor(&mut self) {
        self.cursor = self
            .text
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .find(|i| *i >= self.cursor)
            .unwrap_or(self.text.len());
    }
    fn previous(&self) -> usize {
        self.text[..self.cursor]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(i, _)| i)
    }
    fn next(&self) -> usize {
        self.text[self.cursor..]
            .graphemes(true)
            .next()
            .map_or(self.cursor, |g| self.cursor + g.len())
    }
    fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.view_start = 0;
        self.recalled = None;
    }
    fn remember(&mut self) {
        self.history.retain(|s| s != &self.text);
        self.history.insert(0, self.text.clone());
        self.history.truncate(100);
    }
    fn recall(&mut self, previous: bool) {
        if self.history.is_empty() {
            return;
        }
        if self.recalled.is_none() {
            self.draft = self.text.clone();
        }
        self.recalled = if previous {
            Some(
                self.recalled
                    .map_or(0, |i| (i + 1).min(self.history.len() - 1)),
            )
        } else {
            self.recalled.and_then(|i| i.checked_sub(1))
        };
        self.text = self
            .recalled
            .map_or_else(|| self.draft.clone(), |i| self.history[i].clone());
        self.cursor = self.text.len();
        self.view_start = 0;
    }
    pub fn click(&mut self, column: u16) {
        let desired = column.saturating_sub(self.input_x) as usize;
        let mut width = 0;
        self.cursor = self.view_start;
        for g in self.text[self.view_start..].graphemes(true) {
            let cells = g.width();
            if width + cells > desired {
                break;
            }
            width += cells;
            self.cursor += g.len();
        }
    }
}
pub fn paste(app: &mut App, text: &str) {
    if let Err(e) = app.command_line.insert(text) {
        app.error(e.to_string());
    }
}
pub fn shell_plan(command: &str, cwd: std::path::PathBuf) -> Plan {
    #[cfg(windows)]
    let (program, args) = (
        std::env::var_os("COMSPEC").unwrap_or_else(|| "cmd.exe".into()),
        vec![
            OsString::from("/D"),
            "/S".into(),
            "/C".into(),
            command.into(),
        ],
    );
    #[cfg(not(windows))]
    let (program, args) = (
        std::env::var_os("SHELL")
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "/bin/sh".into()),
        vec![OsString::from("-c"), command.into()],
    );
    Plan {
        cwd,
        commands: vec![CommandSpec { program, args }],
        env: Vec::new(),
    }
}
fn submit(app: &mut App) -> io::Result<()> {
    let text = app.command_line.text.trim();
    if text.is_empty() {
        app.command_line.clear();
        return Ok(());
    }
    if text.split_whitespace().next() == Some("cd") {
        let words = crate::user_menu::format::words(text)?;
        if words.len() > 2 {
            return Err(io::Error::other(
                "Use cd with one directory; quote paths containing spaces",
            ));
        }
        let home = || {
            std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                .filter(|s| !s.is_empty())
                .map(std::path::PathBuf::from)
                .ok_or_else(|| io::Error::other("Home directory is unavailable"))
        };
        let path = match words.get(1).map(String::as_str) {
            None | Some("~") => home()?,
            Some(s) if s.starts_with("~/") => home()?.join(&s[2..]),
            Some(s) => app.panels[app.active].path.join(s),
        };
        app.panels[app.active].go_to(path, app.config.show_hidden);
    } else {
        app.command_line.pending = Some(shell_plan(text, app.panels[app.active].path.clone()));
    }
    app.command_line.remember();
    app.command_line.clear();
    Ok(())
}
pub fn key(app: &mut App, key: KeyEvent) -> bool {
    if key.modifiers.contains(KeyModifiers::ALT) {
        return false;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('e') => app.command_line.recall(true),
            KeyCode::Char('x') => app.command_line.recall(false),
            _ => return false,
        }
        return true;
    }
    let line = &mut app.command_line;
    match key.code {
        KeyCode::Char(c)
            if !c.is_control()
                && (!line.text.is_empty() || !matches!(c, ' ' | '+' | '-' | '*')) =>
        {
            paste(app, &c.to_string());
        }
        KeyCode::Insert if key.modifiers.contains(KeyModifiers::SHIFT) => {
            let text = app.editor_clipboard.clone();
            paste(app, &text);
        }
        KeyCode::Esc => line.clear(),
        KeyCode::Enter if !line.text.is_empty() => {
            if let Err(e) = submit(app) {
                app.error(e.to_string());
            }
        }
        KeyCode::Left if !line.text.is_empty() => line.cursor = line.previous(),
        KeyCode::Right if !line.text.is_empty() => line.cursor = line.next(),
        KeyCode::Home if !line.text.is_empty() => line.cursor = 0,
        KeyCode::End if !line.text.is_empty() => line.cursor = line.text.len(),
        KeyCode::Backspace if !line.text.is_empty() => {
            let start = line.previous();
            line.text.replace_range(start..line.cursor, "");
            line.cursor = start;
            line.snap_cursor();
        }
        KeyCode::Delete if !line.text.is_empty() => {
            let end = line.next();
            line.text.replace_range(line.cursor..end, "");
            line.snap_cursor();
        }
        _ => return false,
    }
    true
}
pub(crate) fn presentation(app: &App, area: Rect) -> (String, Option<(u16, u16)>) {
    let (text, cursor, _, _) = presentation_data(app, area);
    (text, cursor)
}
pub(crate) fn prepare(app: &mut App, area: Rect) {
    let (_, _, x, start) = presentation_data(app, area);
    app.command_line.input_x = x;
    app.command_line.view_start = start;
}
fn presentation_data(app: &App, area: Rect) -> (String, Option<(u16, u16)>, u16, usize) {
    let prefix_armed = crate::shortcuts::armed(app);
    let focused = crate::focus::current(app) == crate::focus::Focus::Manager;
    let line = &app.command_line;
    let path = format!(
        "{}>",
        crate::filesystem::display_name(&app.panels[app.active].path.to_string_lossy())
    );
    let reserve = line.text.width().saturating_add(1).clamp(1, 20) as u16;
    let prefix = crate::formatting::tail(&path, area.width.saturating_sub(reserve + 1).max(1));
    let prefix_width = prefix.width() as u16;
    let input_x = area.x + prefix_width + 1;
    let width = area.width.saturating_sub(prefix_width + 1) as usize;
    // Recompute from the insertion point so deleting/editing combining clusters
    // can never leave a stale byte offset or split a displayed grapheme.
    let mut start = line.cursor;
    let mut used = 0;
    for (i, g) in line.text[..line.cursor].grapheme_indices(true).rev() {
        let cells = g.width();
        if used + cells >= width {
            break;
        }
        used += cells;
        start = i;
    }
    let mut visible = String::new();
    let mut cells = 0;
    for g in line.text[start..].graphemes(true) {
        let w = g.width();
        if cells + w > width {
            break;
        }
        visible.push_str(g);
        cells += w;
    }
    (
        format!("{prefix} {visible}"),
        if focused && !prefix_armed {
            Some((input_x + used as u16, area.y))
        } else {
            None
        },
        input_x,
        start,
    )
}
