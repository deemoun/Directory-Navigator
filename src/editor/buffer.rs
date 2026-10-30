use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

pub const MAX_LINES: usize = 100_000;
pub const MAX_BYTES: usize = 2 * 1024 * 1024;
const HISTORY_BYTES: usize = 16 * 1024 * 1024;
#[derive(Clone, Copy, Debug)]
pub struct Line {
    pub start: usize,
    pub end: usize,
    pub next: usize,
}
struct Edit {
    start: usize,
    removed: String,
    inserted: String,
    before: (usize, Option<usize>, u64),
    after: (usize, Option<usize>, u64),
}
pub struct Buffer {
    pub text: String,
    pub lines: Vec<Line>,
    pub cursor: usize,
    pub anchor: Option<usize>,
    pub tab_size: usize,
    pub newline: &'static str,
    pub changed_line: usize,
    pub changed_through: usize,
    revision: u64,
    saved: u64,
    sequence: u64,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
    history_bytes: usize,
}
impl Buffer {
    pub fn new(text: String) -> Result<Self, String> {
        if text.len() > MAX_BYTES {
            return Err("Editor limit is 2 MiB. Use F3 for a bounded preview.".into());
        }
        if count_lines(text.bytes()) > MAX_LINES {
            return Err("Editor limit is 100,000 lines. Use F3.".into());
        }
        if text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\t' | '\r' | '\n'))
        {
            return Err("Binary/control-character content cannot be edited. Use F3.".into());
        }
        let newline = if text.contains("\r\n") {
            "\r\n"
        } else if text.contains('\n') {
            "\n"
        } else if text.contains('\r') {
            "\r"
        } else {
            "\n"
        };
        let mut result = Self {
            text,
            lines: Vec::new(),
            cursor: 0,
            anchor: None,
            newline,
            tab_size: 8,
            changed_line: 0,
            changed_through: 0,
            revision: 0,
            saved: 0,
            sequence: 0,
            undo: Vec::new(),
            redo: Vec::new(),
            history_bytes: 0,
        };
        result.index_lines();
        Ok(result)
    }
    fn index_lines(&mut self) {
        self.lines.clear();
        let bytes = self.text.as_bytes();
        let (mut start, mut i) = (0, 0);
        while i < bytes.len() {
            if matches!(bytes[i], b'\r' | b'\n') {
                let end = i;
                i += if bytes[i] == b'\r' && bytes.get(i + 1) == Some(&b'\n') {
                    2
                } else {
                    1
                };
                self.lines.push(Line {
                    start,
                    end,
                    next: i,
                });
                start = i;
            } else {
                i += 1;
            }
        }
        self.lines.push(Line {
            start,
            end: bytes.len(),
            next: bytes.len(),
        });
    }
    pub fn row(&self) -> usize {
        self.lines
            .partition_point(|line| line.start <= self.cursor)
            .saturating_sub(1)
    }
    pub fn selected(&self) -> Range<usize> {
        let anchor = self.anchor.unwrap_or(self.cursor);
        anchor.min(self.cursor)..anchor.max(self.cursor)
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn dirty(&self) -> bool {
        self.revision != self.saved
    }
    pub(crate) fn mark_recovered(&mut self) {
        self.sequence += 1;
        self.revision = self.sequence;
    }
    pub fn mark_saved(&mut self) {
        self.saved = self.revision;
    }
    pub fn line_text(&self, row: usize) -> &str {
        self.lines
            .get(row)
            .map(|line| &self.text[line.start..line.end])
            .unwrap_or("")
    }
    pub fn move_to(&mut self, position: usize, select: bool) {
        let position = position.min(self.text.len());
        if !self.text.is_char_boundary(position) {
            return;
        }
        if select {
            self.anchor.get_or_insert(self.cursor);
        } else {
            self.anchor = None;
        }
        self.cursor = self.snap_forward(position);
    }
    fn snap_forward(&self, position: usize) -> usize {
        let row = self
            .lines
            .partition_point(|line| line.start <= position)
            .saturating_sub(1);
        let line = self.lines[row];
        self.text[line.start..line.next]
            .grapheme_indices(true)
            .map(|(i, _)| line.start + i)
            .find(|i| *i >= position)
            .unwrap_or(line.next)
    }
    fn snap_backward(&self, position: usize) -> usize {
        let row = self
            .lines
            .partition_point(|line| line.start <= position)
            .saturating_sub(1);
        let line = self.lines[row];
        self.text[line.start..line.next]
            .grapheme_indices(true)
            .map(|(i, _)| line.start + i)
            .take_while(|i| *i <= position)
            .last()
            .unwrap_or(line.start)
    }
    /// Reparse the edited lines plus one neighbor on each side (CR/LF joins).
    /// Untouched line text is never rescanned; only its byte offsets are shifted.
    fn splice_text(&mut self, range: Range<usize>, text: &str) -> Result<(), String> {
        let first = self
            .lines
            .partition_point(|l| l.start <= range.start)
            .saturating_sub(1)
            .saturating_sub(1);
        let last = (self
            .lines
            .partition_point(|l| l.start <= range.end)
            .saturating_sub(1)
            + 1)
        .min(self.lines.len() - 1);
        let begin = self.lines[first].start;
        let end = self.lines[last].next;
        let mut segment = String::new();
        segment.push_str(&self.text[begin..range.start]);
        segment.push_str(text);
        segment.push_str(&self.text[range.end..end]);
        let mut lines = Vec::new();
        let (mut start, mut i) = (0, 0);
        let bytes = segment.as_bytes();
        while i < bytes.len() {
            if matches!(bytes[i], b'\r' | b'\n') {
                let line_end = i;
                i += if bytes[i] == b'\r' && bytes.get(i + 1) == Some(&b'\n') {
                    2
                } else {
                    1
                };
                lines.push(Line {
                    start: begin + start,
                    end: begin + line_end,
                    next: begin + i,
                });
                start = i;
            } else {
                i += 1;
            }
        }
        if end == self.text.len() || start < bytes.len() {
            lines.push(Line {
                start: begin + start,
                end: begin + bytes.len(),
                next: begin + bytes.len(),
            });
        }
        let through = if end == self.text.len() {
            self.lines.len()
        } else {
            last + 1
        };
        if self.lines.len() - (through - first) + lines.len() > MAX_LINES {
            return Err("Editor limit is 100,000 lines; change was not applied.".into());
        }
        let following = first + lines.len();
        let removed = range.len();
        self.text.replace_range(range, text);
        self.lines.splice(first..through, lines);
        for line in &mut self.lines[following..] {
            line.start = line.start - removed + text.len();
            line.end = line.end - removed + text.len();
            line.next = line.next - removed + text.len();
        }
        Ok(())
    }
    pub fn horizontal(&mut self, right: bool, select: bool) {
        let range = self.selected();
        if !select && !range.is_empty() {
            self.move_to(if right { range.end } else { range.start }, false);
            return;
        }
        let position = if right {
            self.cursor
                + self.text[self.cursor..]
                    .graphemes(true)
                    .next()
                    .map(str::len)
                    .unwrap_or(0)
        } else {
            self.text[..self.cursor]
                .grapheme_indices(true)
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0)
        };
        self.move_to(position, select);
    }
    pub fn display_width(text: &str) -> usize {
        text.graphemes(true)
            .fold(0, |col, g| col + Self::grapheme_width(g, col))
    }
    pub fn grapheme_width(g: &str, column: usize) -> usize {
        if g == "\t" {
            8 - column % 8
        } else {
            crate::presentation::text::Span::raw(g).width()
        }
    }
    pub fn cell_width(&self, g: &str, column: usize) -> usize {
        if g == "\t" {
            self.tab_size - column % self.tab_size
        } else {
            crate::presentation::text::Span::raw(g).width()
        }
    }
    pub fn text_width(&self, text: &str) -> usize {
        text.graphemes(true)
            .fold(0, |col, g| col + self.cell_width(g, col))
    }
    pub(super) fn record_cursor(&mut self) {
        if let Some(edit) = self
            .undo
            .last_mut()
            .filter(|edit| edit.after.2 == self.revision)
        {
            edit.after = (self.cursor, self.anchor, self.revision);
        }
    }
    pub fn insertion_column(&self) -> usize {
        let start = self.selected().start;
        let row = self
            .lines
            .partition_point(|line| line.start <= start)
            .saturating_sub(1);
        self.text_width(&self.text[self.lines[row].start..start])
    }
    pub fn column(&self) -> usize {
        self.text_width(&self.text[self.lines[self.row()].start..self.cursor])
    }
    pub fn position_at(&self, row: usize, column: usize) -> usize {
        let row = row.min(self.lines.len() - 1);
        let line = self.lines[row];
        let mut width = 0;
        for (index, g) in self.line_text(row).grapheme_indices(true) {
            let next = width + self.cell_width(g, width);
            if next > column {
                return line.start + index;
            }
            width = next;
        }
        line.end
    }
    pub fn vertical(&mut self, amount: isize, select: bool) {
        let row = self
            .row()
            .saturating_add_signed(amount)
            .min(self.lines.len() - 1);
        self.move_to(self.position_at(row, self.column()), select);
    }
    pub fn replace(&mut self, range: Range<usize>, text: &str) -> Result<(), String> {
        if range.start > range.end
            || range.end > self.text.len()
            || !self.text.is_char_boundary(range.start)
            || !self.text.is_char_boundary(range.end)
        {
            return Err("Invalid text boundary".into());
        }
        if self.text.len() - range.len() + text.len() > MAX_BYTES {
            return Err("Editor limit is 2 MiB; change was not applied.".into());
        }
        if self.text[range.clone()] == *text {
            self.move_to(range.start + text.len(), false);
            return Ok(());
        }
        let before = (self.cursor, self.anchor, self.revision);
        self.changed_line = self
            .lines
            .partition_point(|line| line.start <= range.start)
            .saturating_sub(1);
        let old_end_row = self
            .lines
            .partition_point(|line| line.start <= range.end)
            .saturating_sub(1);
        let removed = self.text[range.clone()].to_owned();
        self.splice_text(range.clone(), text)?;
        self.cursor = self.snap_forward(range.start + text.len());
        self.anchor = None;
        self.sequence = self.sequence.saturating_add(1);
        self.revision = self.sequence;
        self.history_bytes += removed.len() + text.len();
        self.undo.push(Edit {
            start: range.start,
            removed,
            inserted: text.into(),
            before,
            after: (self.cursor, self.anchor, self.revision),
        });
        self.history_bytes -= self
            .redo
            .iter()
            .map(|edit| edit.removed.len() + edit.inserted.len())
            .sum::<usize>();
        self.redo.clear();
        while self.undo.len() > 1000 || self.history_bytes > HISTORY_BYTES {
            if self.undo.is_empty() {
                break;
            }
            let old = self.undo.remove(0);
            self.history_bytes -= old.removed.len() + old.inserted.len();
        }
        self.changed_through = old_end_row.max(self.row());
        Ok(())
    }
    pub fn insert(&mut self, text: &str) -> Result<(), String> {
        let clean: String = text
            .chars()
            .filter(|c| !c.is_control() || matches!(c, '\t' | '\n' | '\r'))
            .collect();
        // Typed/pasted line breaks use the document convention. Existing bytes stay intact.
        let normalized = clean
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .replace('\n', self.newline);
        self.replace(self.selected(), &normalized)
    }
    pub fn delete(&mut self, backspace: bool) -> Result<(), String> {
        let mut range = self.selected();
        if range.is_empty() {
            let cursor = self.cursor;
            self.horizontal(!backspace, false);
            range = cursor.min(self.cursor)..cursor.max(self.cursor);
            self.cursor = cursor;
        }
        self.replace(range, "")
    }
    pub fn undo(&mut self, redo: bool) {
        let action = if redo {
            self.redo.pop()
        } else {
            self.undo.pop()
        };
        if let Some(edit) = action {
            let (from, to, state) = if redo {
                (&edit.removed, &edit.inserted, edit.after)
            } else {
                (&edit.inserted, &edit.removed, edit.before)
            };
            self.changed_line = self
                .lines
                .partition_point(|line| line.start <= edit.start)
                .saturating_sub(1);
            let old_end = self
                .lines
                .partition_point(|line| line.start <= edit.start + from.len())
                .saturating_sub(1);
            if self
                .splice_text(edit.start..edit.start + from.len(), to)
                .is_err()
            {
                if redo {
                    self.redo.push(edit);
                } else {
                    self.undo.push(edit);
                }
                return;
            }
            (self.cursor, self.anchor, self.revision) = state;
            self.changed_through = old_end.max(
                self.lines
                    .partition_point(|line| line.start <= edit.start + to.len())
                    .saturating_sub(1),
            );
            if redo {
                self.undo.push(edit);
            } else {
                self.redo.push(edit);
            }
        }
    }
    pub fn find(&mut self, query: &str) -> bool {
        if query.is_empty() {
            return false;
        }
        let start = self.selected().end;
        let found = self.text[start..]
            .find(query)
            .map(|i| start + i)
            .or_else(|| self.text[..start].find(query));
        if let Some(index) = found {
            self.anchor = Some(self.snap_backward(index));
            self.cursor = self.snap_forward(index + query.len());
            true
        } else {
            false
        }
    }
}

fn count_lines(bytes: impl Iterator<Item = u8>) -> usize {
    let mut bytes = bytes.peekable();
    let mut lines = 1;
    while let Some(byte) = bytes.next() {
        if byte == b'\r' {
            if bytes.peek() == Some(&b'\n') {
                bytes.next();
            }
            lines += 1;
        } else if byte == b'\n' {
            lines += 1;
        }
        if lines > MAX_LINES {
            break;
        }
    }
    lines
}
