use super::Viewer;
use crate::app::App;
use crate::presentation::text::Span;
use crossterm::event::{KeyCode, KeyEvent};
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
pub struct GoToDialog {
    pub field: String,
    pub cursor: usize,
    pub focus: usize,
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
    GoTo,
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
    pub fn go_to(&mut self, field: &str) -> Result<(), String> {
        let contents = self.contents.as_ref().ok_or("Wait for the file to load.")?;
        let field = field.trim();
        let value = if contents.binary {
            if let Some(hex) = field
                .strip_prefix("0x")
                .or_else(|| field.strip_prefix("0X"))
            {
                usize::from_str_radix(hex, 16)
                    .map_err(|_| "Enter a decimal byte offset or 0x hexadecimal offset.")?
            } else {
                field
                    .parse::<usize>()
                    .map_err(|_| "Enter a decimal byte offset or 0x hexadecimal offset.")?
            }
        } else {
            field
                .parse::<usize>()
                .map_err(|_| "Enter a line number starting at 1.")?
        };
        if contents.binary {
            if value >= contents.loaded_bytes.max(1) {
                return Err("Offset is outside the loaded preview.".into());
            }
            self.offset = value / 16;
            self.horizontal = 0;
            self.current = (contents.loaded_bytes > 0).then_some(Match {
                line: value / 16,
                range: (10 + 3 * (value % 16))..(12 + 3 * (value % 16)),
            });
        } else {
            if value == 0 || value > contents.lines.len() {
                return Err("Line is outside the loaded preview.".into());
            }
            self.current = None;
            self.go_to_line(value);
            self.layout(self.layout_width.max(1) as u16);
        }
        self.notice.clear();
        Ok(())
    }
    pub fn go_to_line(&mut self, line: usize) {
        self.reveal_line = Some(line.saturating_sub(1));
        self.layout_width = 0;
    }

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
        if self.contents.is_some() {
            if let Some(line) = self.reveal_line.take() {
                self.offset = self.row_for(line, 0);
                self.horizontal = 0;
            }
        }
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
        {
            let id = app.desktop.target();
            app.retire_document(id);
        };
        return;
    }
    let Some(viewer) = app.desktop.active_viewer_mut() else {
        return;
    };
    if viewer.modals.viewer_goto().is_some() {
        match action {
            Action::Cancel => viewer.modals.set_viewer_goto(None),
            Action::Field => viewer.modals.viewer_goto_mut().unwrap().focus = 0,
            Action::Submit => {
                let dialog = viewer.modals.take_viewer_goto().unwrap();
                if let Err(error) = viewer.go_to(&dialog.field) {
                    viewer.notice = error;
                    viewer.modals.set_viewer_goto(Some(dialog));
                }
            }
            _ => {}
        }
        return;
    }
    match action {
        Action::GoTo => {
            if viewer.contents.is_some() {
                viewer.modals.set_viewer_goto(Some(GoToDialog {
                    field: String::new(),
                    cursor: 0,
                    focus: 0,
                }));
            }
        }
        Action::Wrap => viewer.toggle_wrap(),
        Action::Find => {
            if viewer.text_ready() {
                viewer.modals.set_viewer_search(Some(SearchDialog {
                    cursor: app.viewer_search.query.len(),
                    memory: app.viewer_search.clone(),
                    focus: 0,
                }));
            } else {
                viewer.notice = "Text search unavailable in Hex view or while loading".into();
            }
        }
        Action::Cancel | Action::Case | Action::Field => {
            if let Some(dialog) = viewer.modals.viewer_search_mut() {
                if super::search_controller::action(dialog, action)
                    == super::search_controller::Effect::Cancel
                {
                    viewer.modals.set_viewer_search(None);
                }
            }
        }
        Action::Submit => {
            if let Some(dialog) = viewer.modals.take_viewer_search() {
                match viewer.find(&dialog.memory, false, false) {
                    Ok(_) => app.viewer_search = dialog.memory,
                    Err(error) => {
                        viewer.notice = error;
                        viewer.modals.set_viewer_search(Some(dialog));
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
        .active_viewer_mut()
        .and_then(|v| v.modals.viewer_goto_mut())
    {
        if dialog.focus == 0 {
            crate::dialog::paste_input(&mut dialog.field, &mut dialog.cursor, text);
        }
        return;
    }
    if let Some(dialog) = app
        .desktop
        .active_viewer_mut()
        .and_then(|v| v.modals.viewer_search_mut())
    {
        super::search_controller::paste(dialog, text);
    }
}
pub fn key(app: &mut App, key: KeyEvent) {
    let proposal = {
        let spec = crate::ui::viewer_search_spec(app, crate::turbo::dialog_spec::input_area(app));
        spec.and_then(|spec| {
            if matches!(
                key.code,
                KeyCode::Tab | KeyCode::BackTab | KeyCode::Enter | KeyCode::Esc
            ) || (key.code == KeyCode::Char(' ') && spec.focus != 0)
            {
                spec.key(key)
            } else {
                None
            }
        })
    };
    if let Some(proposal) = proposal {
        crate::dialog_controls::apply(app, proposal, |app, target| {
            if let crate::mouse::Target::Viewer(command) = target {
                action(app, command);
            }
        });
        return;
    }
    let Some(viewer) = app.desktop.active_viewer_mut() else {
        return;
    };
    if let Some(dialog) = viewer.modals.viewer_goto_mut() {
        if dialog.focus == 0 {
            if key.code == KeyCode::Char('a')
                && key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL)
            {
                dialog.field.clear();
                dialog.cursor = 0;
            } else if !key.modifiers.intersects(
                crossterm::event::KeyModifiers::CONTROL | crossterm::event::KeyModifiers::ALT,
            ) {
                crate::dialog::edit_input(&mut dialog.field, &mut dialog.cursor, key.code);
            }
        }
        return;
    }
    if let Some(dialog) = viewer.modals.viewer_search_mut() {
        super::search_controller::edit(dialog, key);
        return;
    }

    if let Some(command) = crate::commands::viewer_key(key) {
        action(app, command);
        return;
    }
    let command = match key.code {
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
