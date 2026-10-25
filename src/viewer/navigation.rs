use super::Viewer;
use crate::presentation::text::Span;
use crate::{app::App, dialog};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchMemory {
    pub query: String,
    pub case_sensitive: bool,
}
impl Default for SearchMemory {
    fn default() -> Self {
        Self {
            query: String::new(),
            case_sensitive: true,
        }
    }
}
pub struct SearchDialog {
    pub memory: SearchMemory,
    pub cursor: usize,
    pub focus: usize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    pub line: usize,
    pub range: Range<usize>,
}
#[derive(Debug)]
pub struct VisualRow {
    pub line: usize,
    pub range: Range<usize>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Wrap,
    Find,
    Next,
    Reverse,
    Close,
    Submit,
    Cancel,
    Case,
    Field,
}
impl Viewer {
    pub fn text_ready(&self) -> bool {
        self.contents.as_ref().is_some_and(|c| !c.binary)
    }
    pub fn layout(&mut self, width: u16) {
        let width = usize::from(width.max(1));
        if self.layout_width == width && self.mapped_wrap == self.wrap && !self.rows.is_empty() {
            return;
        }
        let anchor = self.anchor();
        let was_wrapped = self.mapped_wrap;
        self.rows.clear();
        self.layout_width = width;
        if let Some(contents) = &self.contents {
            for (line, text) in contents.lines.iter().enumerate() {
                if !self.wrap || contents.binary {
                    self.rows.push(VisualRow {
                        line,
                        range: 0..text.len(),
                    });
                    continue;
                }
                let mut start = 0;
                let mut cells = 0;
                for (byte, grapheme) in text.grapheme_indices(true) {
                    let next = Span::raw(grapheme).width();
                    if cells + next > width && byte > start {
                        self.rows.push(VisualRow {
                            line,
                            range: start..byte,
                        });
                        start = byte;
                        cells = 0;
                    }
                    cells += next;
                }
                self.rows.push(VisualRow {
                    line,
                    range: start..text.len(),
                });
            }
        }
        self.offset = self.row_for(anchor.0, anchor.1);
        self.mapped_wrap = self.wrap;
        if was_wrapped && !self.wrap {
            if let Some(line) = self.contents.as_ref().and_then(|c| c.lines.get(anchor.0)) {
                self.horizontal = Span::raw(&line[..anchor.1.min(line.len())])
                    .width()
                    .min(u16::MAX as usize) as u16;
            }
        }
    }
    pub fn anchor(&self) -> (usize, usize) {
        let Some(row) = self.rows.get(self.offset) else {
            return (self.offset, 0);
        };
        if self.mapped_wrap {
            return (row.line, row.range.start);
        }
        let Some(text) = self.contents.as_ref().and_then(|c| c.lines.get(row.line)) else {
            return (row.line, 0);
        };
        let mut cells = 0;
        for (byte, grapheme) in text.grapheme_indices(true) {
            let width = Span::raw(grapheme).width();
            if cells + width > usize::from(self.horizontal) {
                return (row.line, byte);
            }
            cells += width;
        }
        (row.line, text.len())
    }
    fn row_for(&self, line: usize, byte: usize) -> usize {
        self.rows
            .iter()
            .position(|r| {
                r.line == line
                    && (r.range.contains(&byte)
                        || r.range.is_empty()
                        || r.range.end == byte
                            && self
                                .contents
                                .as_ref()
                                .and_then(|c| c.lines.get(line))
                                .is_some_and(|s| s.len() == byte))
            })
            .unwrap_or_else(|| self.rows.len().saturating_sub(1))
    }
    pub fn toggle_wrap(&mut self) {
        if !self.text_ready() {
            return;
        }
        self.wrap ^= true;
        let width = self.layout_width.max(1);
        self.layout_width = 0;
        self.layout(width as u16);
    }
    pub fn find(
        &mut self,
        memory: &SearchMemory,
        backward: bool,
        repeat: bool,
    ) -> Result<bool, String> {
        if !self.text_ready() {
            return Err("Text search is unavailable in Hex view or while loading.".into());
        }
        if memory.query.is_empty() {
            return Err("Enter text to find.".into());
        }
        if self.rows.is_empty() {
            self.layout(self.layout_width.max(80) as u16);
        }
        let mut origin = self.anchor();
        if repeat && self.last_search == *memory {
            if let Some(found) = &self.current {
                origin = (
                    found.line,
                    if backward {
                        found.range.start
                    } else {
                        found.range.end
                    },
                );
            }
        }
        let Some(contents) = &self.contents else {
            return Ok(false);
        };
        let count = contents.lines.len();
        if count == 0 {
            return Ok(false);
        }
        let origin_line = origin.0.min(count - 1);
        let options = crate::editor::search::SearchOptions {
            case_sensitive: memory.case_sensitive,
            backward,
            ..Default::default()
        };
        let mut found = None;
        for turn in 0..=count {
            let line = if backward {
                (origin_line + count - turn % count) % count
            } else {
                (origin_line + turn) % count
            };
            let text = &contents.lines[line];
            let ranges =
                crate::editor::search::matches(text, &memory.query, 0..text.len(), options)?;
            let eligible = |range: &Range<usize>| {
                if turn == 0 {
                    if backward {
                        range.end <= origin.1
                    } else {
                        range.start >= origin.1
                    }
                } else if turn == count {
                    if backward {
                        range.end > origin.1
                    } else {
                        range.start < origin.1
                    }
                } else {
                    true
                }
            };
            let range = if backward {
                ranges.into_iter().rev().find(eligible)
            } else {
                ranges.into_iter().find(eligible)
            };
            if let Some(range) = range {
                found = Some(Match { line, range });
                break;
            }
        }
        self.last_search = memory.clone();
        if let Some(found) = found {
            self.offset = self.row_for(found.line, found.range.start);
            if !self.wrap {
                let text = &contents.lines[found.line];
                let start = Span::raw(&text[..found.range.start]).width();
                let length = Span::raw(&text[found.range.clone()]).width();
                if start < usize::from(self.horizontal)
                    || start + length > usize::from(self.horizontal) + self.layout_width
                {
                    self.horizontal = start.min(u16::MAX as usize) as u16;
                }
            }
            self.current = Some(found);
            self.notice.clear();
            Ok(true)
        } else {
            self.current = None;
            self.notice = if contents.truncated {
                "No match in loaded preview"
            } else {
                "No match"
            }
            .into();
            Ok(false)
        }
    }
}
pub fn action(app: &mut App, action: Action) {
    if action == Action::Close {
        app.desktop.close_viewer();
        return;
    }
    let Some(viewer) = app.desktop.viewer_mut() else {
        return;
    };
    match action {
        Action::Wrap => viewer.toggle_wrap(),
        Action::Find => {
            if viewer.text_ready() {
                viewer.search_dialog = Some(SearchDialog {
                    cursor: app.viewer_search.query.len(),
                    memory: app.viewer_search.clone(),
                    focus: 0,
                });
            } else {
                viewer.notice = "Text search unavailable in Hex view or while loading".into();
            }
        }
        Action::Cancel => viewer.search_dialog = None,
        Action::Case => {
            if let Some(dialog) = &mut viewer.search_dialog {
                dialog.memory.case_sensitive ^= true;
                dialog.focus = 1;
            }
        }
        Action::Field => {
            if let Some(dialog) = &mut viewer.search_dialog {
                dialog.focus = 0;
            }
        }
        Action::Submit => {
            if let Some(dialog) = viewer.search_dialog.take() {
                match viewer.find(&dialog.memory, false, false) {
                    Ok(_) => app.viewer_search = dialog.memory,
                    Err(error) => {
                        viewer.notice = error;
                        viewer.search_dialog = Some(dialog);
                    }
                }
            }
        }
        Action::Next | Action::Reverse => {
            if let Err(error) = viewer.find(&app.viewer_search, action == Action::Reverse, true) {
                viewer.notice = error;
            }
        }
        Action::Close => {}
    }
}
pub fn paste(app: &mut App, text: &str) {
    if let Some(dialog) = app
        .desktop
        .viewer_mut()
        .and_then(|v| v.search_dialog.as_mut())
    {
        if dialog.focus == 0 {
            dialog::paste_input(&mut dialog.memory.query, &mut dialog.cursor, text);
        }
    }
}
pub fn key(app: &mut App, key: KeyEvent) {
    let Some(viewer) = app.desktop.viewer_mut() else {
        return;
    };
    if let Some(dialog) = &mut viewer.search_dialog {
        let command = match key.code {
            KeyCode::Esc => Some(Action::Cancel),
            KeyCode::Tab => {
                dialog.focus = crate::control_focus::cycle(dialog.focus, 4, false);
                None
            }
            KeyCode::BackTab => {
                dialog.focus = crate::control_focus::cycle(dialog.focus, 4, true);
                None
            }
            KeyCode::Enter => Some(if dialog.focus == 3 {
                Action::Cancel
            } else {
                Action::Submit
            }),
            KeyCode::Char(' ') if dialog.focus == 1 => Some(Action::Case),
            code if dialog.focus == 0
                && !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                dialog::edit_input(&mut dialog.memory.query, &mut dialog.cursor, code);
                None
            }
            _ => None,
        };
        if let Some(command) = command {
            action(app, command);
        }
        return;
    }
    let command = match key.code {
        KeyCode::Esc | KeyCode::F(3) => Some(Action::Close),
        KeyCode::F(2) => Some(Action::Wrap),
        KeyCode::F(7) => Some(if key.modifiers.contains(KeyModifiers::CONTROL) {
            Action::Reverse
        } else if key.modifiers.contains(KeyModifiers::SHIFT) {
            Action::Next
        } else {
            Action::Find
        }),
        KeyCode::Up => {
            viewer.step(-1);
            None
        }
        KeyCode::Down => {
            viewer.step(1);
            None
        }
        KeyCode::PageUp => {
            viewer.step(-(app.page_rows as isize));
            None
        }
        KeyCode::PageDown | KeyCode::Char(' ') => {
            viewer.step(app.page_rows as isize);
            None
        }
        KeyCode::Home => {
            viewer.offset = 0;
            None
        }
        KeyCode::End => {
            viewer.step(isize::MAX);
            None
        }
        KeyCode::Left if !viewer.wrap => {
            viewer.horizontal = viewer.horizontal.saturating_sub(4);
            None
        }
        KeyCode::Right if !viewer.wrap => {
            viewer.horizontal = viewer.horizontal.saturating_add(4).min(16384);
            None
        }
        _ => None,
    };
    if let Some(command) = command {
        action(app, command);
    }
}
