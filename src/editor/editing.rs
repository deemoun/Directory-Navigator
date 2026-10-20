use super::buffer::Buffer;
use unicode_segmentation::UnicodeSegmentation;

pub fn word_character(grapheme: &str) -> bool {
    grapheme.chars().any(|c| c.is_alphanumeric() || c == '_')
}
impl Buffer {
    pub fn word_move(&mut self, right: bool, select: bool) {
        let mut row = self.row();
        let mut position = self.cursor;
        loop {
            let line = self.lines[row];
            if right {
                let initial = position < line.end;
                let mut graphemes = self.text[position..line.end]
                    .grapheme_indices(true)
                    .peekable();
                while graphemes.peek().is_some_and(|(_, g)| word_character(g)) {
                    graphemes.next();
                }
                while graphemes.peek().is_some_and(|(_, g)| !word_character(g)) {
                    graphemes.next();
                }
                if let Some((offset, _)) = graphemes.next() {
                    position += offset;
                    break;
                }
                if initial || row + 1 >= self.lines.len() {
                    position = line.end;
                    break;
                }
                row += 1;
                position = self.lines[row].start;
                if self
                    .line_text(row)
                    .graphemes(true)
                    .next()
                    .is_some_and(word_character)
                {
                    break;
                }
            } else {
                let mut graphemes = self.text[line.start..position]
                    .grapheme_indices(true)
                    .rev()
                    .peekable();
                while graphemes.peek().is_some_and(|(_, g)| !word_character(g)) {
                    graphemes.next();
                }
                let mut start = None;
                while graphemes.peek().is_some_and(|(_, g)| word_character(g)) {
                    start = graphemes.next().map(|(i, _)| line.start + i);
                }
                if let Some(start) = start {
                    position = start;
                    break;
                }
                if row == 0 {
                    position = 0;
                    break;
                }
                row -= 1;
                position = self.lines[row].end;
            }
        }
        self.move_to(position, select);
    }
    pub fn enter(&mut self, auto_indent: bool) -> Result<(), String> {
        let start = self.selected().start;
        let row = self
            .lines
            .partition_point(|line| line.start <= start)
            .saturating_sub(1);
        let indent: String = if auto_indent {
            self.text[self.lines[row].start..start]
                .chars()
                .take_while(|c| matches!(c, ' ' | '\t'))
                .collect()
        } else {
            String::new()
        };
        self.insert(&format!("\n{indent}"))
    }
    pub fn duplicate_line(&mut self) -> Result<(), String> {
        let line = self.lines[self.row()];
        let cursor = self.cursor;
        let text = self.line_text(self.row()).to_owned();
        let copy = if line.next > line.end {
            format!("{text}{}", &self.text[line.end..line.next])
        } else {
            format!("{}{text}", self.newline)
        };
        self.replace(line.next..line.next, &copy)?;
        self.move_to(cursor, false);
        self.record_cursor();
        Ok(())
    }
}
