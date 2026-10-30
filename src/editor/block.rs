//! DN block commands adapted to UTF-8 buffers and a single undo transaction.
use super::{buffer::Buffer, highlight::Cache};
use unicode_segmentation::UnicodeSegmentation;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Case {
    Upper,
    Lower,
    Capitalize,
}
impl Buffer {
    fn block_rows(&self) -> (usize, usize) {
        let range = self.selected();
        let row = |pos| {
            self.lines
                .partition_point(|l| l.start <= pos)
                .saturating_sub(1)
        };
        (
            row(range.start),
            row(if range.is_empty() {
                range.end
            } else {
                range.end - 1
            }),
        )
    }
    fn select_result(&mut self, start: usize, end: usize, reverse: bool) {
        self.move_to(if reverse { end } else { start }, false);
        self.move_to(if reverse { start } else { end }, true);
        self.record_cursor();
    }
    pub fn indent_block(&mut self, unindent: bool, expand_tabs: bool) -> Result<(), String> {
        let (first, last) = self.block_rows();
        let start = self.lines[first].start;
        let end = self.lines[last].next;
        let prefix = if expand_tabs {
            " ".repeat(self.tab_size)
        } else {
            "\t".into()
        };
        let mut text = String::new();
        let mut edits = Vec::new();
        for line in &self.lines[first..=last] {
            let value = &self.text[line.start..line.next];
            let removed = if unindent {
                if value.starts_with('\t') {
                    1
                } else {
                    value
                        .bytes()
                        .take(self.tab_size)
                        .take_while(|b| *b == b' ')
                        .count()
                }
            } else {
                0
            };
            let inserted = if unindent { "" } else { &prefix };
            text.push_str(inserted);
            text.push_str(&value[removed..]);
            edits.push((line.start, removed, inserted.len()));
        }
        if self.text[start..end] == text {
            return Ok(());
        }
        let map = |pos: usize| {
            let mut delta = 0isize;
            for &(at, old, new) in &edits {
                if pos < at {
                    break;
                }
                if pos <= at + old {
                    return (at as isize + delta) as usize + new;
                }
                delta += new as isize - old as isize;
            }
            (pos as isize + delta) as usize
        };
        let cursor = map(self.cursor);
        let anchor = self.anchor.map(map);
        self.replace(start..end, &text)?;
        self.move_to(anchor.unwrap_or(cursor), false);
        if anchor.is_some() {
            self.move_to(cursor, true);
        }
        self.record_cursor();
        Ok(())
    }
    pub fn change_case(&mut self, mode: Case) -> Result<(), String> {
        let range = self.selected();
        if range.is_empty() {
            return Err("Select text first.".into());
        }
        let original = &self.text[range.clone()];
        let text = match mode {
            Case::Upper => original.to_uppercase(),
            Case::Lower => original.to_lowercase(),
            Case::Capitalize => original
                .split_word_bounds()
                .map(|word| {
                    let lower = word.to_lowercase();
                    if let Some((at, c)) = lower.char_indices().find(|(_, c)| c.is_alphabetic()) {
                        format!(
                            "{}{}{}",
                            &lower[..at],
                            c.to_uppercase(),
                            &lower[at + c.len_utf8()..]
                        )
                    } else {
                        lower
                    }
                })
                .collect::<String>(),
        };
        if original == text {
            return Ok(());
        }
        let reverse = self.cursor == range.start;
        self.replace(range.clone(), &text)?;
        self.select_result(range.start, range.start + text.len(), reverse);
        Ok(())
    }
    pub fn sort_lines(&mut self) -> Result<(), String> {
        if self.selected().is_empty() {
            return Err("Select at least two lines first.".into());
        }
        let (first, last) = self.block_rows();
        if first == last {
            return Err("Select at least two lines first.".into());
        }
        let range = self.lines[first].start..self.lines[last].next;
        let mut values = (first..=last)
            .map(|row| self.line_text(row))
            .collect::<Vec<_>>();
        values.sort();
        let mut text = String::new();
        for (value, line) in values.iter().zip(&self.lines[first..=last]) {
            text.push_str(value);
            let ending = &self.text[line.end..line.next];
            // An empty sorted line must not fuse separate CR and LF separators.
            if text.ends_with('\r') && ending.starts_with('\n') {
                return Err("Sorting would merge mixed CR/LF line endings.".into());
            }
            text.push_str(ending);
        }
        if self.text[range.clone()] == text {
            return Ok(());
        }
        let reverse = self.cursor < self.anchor.unwrap_or(self.cursor);
        self.replace(range.clone(), &text)?;
        self.select_result(range.start, range.start + text.len(), reverse);
        Ok(())
    }
    pub fn matching_bracket(&self, cache: &Cache) -> Option<usize> {
        let bytes = self.text.as_bytes();
        let code = |pos| {
            let row = self
                .lines
                .partition_point(|l| l.start <= pos)
                .saturating_sub(1);
            cache.is_code(row, pos - self.lines[row].start)
        };
        let bracket = |b: u8| b"()[]{}".contains(&b);
        let at = if bytes.get(self.cursor).is_some_and(|b| bracket(*b)) && code(self.cursor) {
            self.cursor
        } else if self.cursor > 0 && bracket(bytes[self.cursor - 1]) && code(self.cursor - 1) {
            self.cursor - 1
        } else {
            return None;
        };
        let forward = b"([{".contains(&bytes[at]);
        let pair = |b| match b {
            b'(' => b')',
            b'[' => b']',
            b'{' => b'}',
            b')' => b'(',
            b']' => b'[',
            _ => b'{',
        };
        let mut stack = vec![pair(bytes[at])];
        let scan: Box<dyn Iterator<Item = usize>> = if forward {
            Box::new(at + 1..bytes.len())
        } else {
            Box::new((0..at).rev())
        };
        for pos in scan {
            let b = bytes[pos];
            if !bracket(b) || !code(pos) {
                continue;
            }
            if b"([{".contains(&b) == forward {
                stack.push(pair(b));
            } else if stack.pop() != Some(b) {
                return None;
            } else if stack.is_empty() {
                return Some(pos);
            }
        }
        None
    }
}
