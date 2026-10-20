use super::{
    buffer::{Buffer, MAX_BYTES},
    editing::word_character,
};
use std::{collections::VecDeque, ops::Range};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchOptions {
    pub case_sensitive: bool,
    pub whole_words: bool,
    pub backward: bool,
    pub selected_scope: bool,
    pub entire_scope: bool,
    pub prompt: bool,
}
impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            case_sensitive: true,
            whole_words: false,
            backward: false,
            selected_scope: false,
            entire_scope: false,
            prompt: true,
        }
    }
}
#[derive(Clone, Default)]
pub struct SearchMemory {
    pub query: String,
    pub replacement: String,
    pub options: SearchOptions,
}
pub struct SearchDialog {
    pub replacing: bool,
    pub query: String,
    pub replacement: String,
    pub cursors: [usize; 2],
    pub focus: usize,
    pub options: SearchOptions,
    pub selection: Option<Range<usize>>,
}
impl SearchDialog {
    pub fn new(replacing: bool, memory: &SearchMemory, selection: Range<usize>) -> Self {
        let mut options = memory.options;
        if selection.is_empty() {
            options.selected_scope = false;
        }
        Self {
            replacing,
            query: memory.query.clone(),
            replacement: memory.replacement.clone(),
            cursors: [memory.query.len(), memory.replacement.len()],
            focus: 0,
            options,
            selection: (!selection.is_empty()).then_some(selection),
        }
    }
    pub fn choices(&self) -> Vec<usize> {
        let mut choices = vec![0];
        if self.replacing {
            choices.push(1);
        }
        choices.extend([2, 3]);
        if self.replacing {
            choices.push(4);
        }
        choices.push(5);
        if self.selection.is_some() {
            choices.push(6);
        }
        choices.extend([7, 8]);
        if self.replacing {
            choices.push(9);
        }
        choices.push(10);
        choices
    }
    pub fn tab(&mut self, reverse: bool) {
        let choices = self.choices();
        let index = choices.iter().position(|f| *f == self.focus).unwrap_or(0);
        self.focus = choices[(index + if reverse { choices.len() - 1 } else { 1 }) % choices.len()];
    }
    pub fn toggle(&mut self, choice: usize) {
        match choice {
            2 => self.options.case_sensitive = !self.options.case_sensitive,
            3 => self.options.whole_words = !self.options.whole_words,
            4 => self.options.prompt = !self.options.prompt,
            5 => self.options.backward = !self.options.backward,
            6 if self.selection.is_some() => {
                self.options.selected_scope = !self.options.selected_scope
            }
            7 => self.options.entire_scope = !self.options.entire_scope,
            _ => {}
        }
    }
}
fn lower(text: &str) -> String {
    text.chars().flat_map(char::to_lowercase).collect()
}

fn scan_matches(
    haystack: &str,
    needle: &str,
    backward: bool,
    map: impl Fn(usize, usize) -> Option<Range<usize>>,
    valid: impl Fn(&Range<usize>) -> bool,
) -> Vec<Range<usize>> {
    let mut result = Vec::new();
    let mut cursor = if backward { haystack.len() } else { 0 };
    loop {
        let found = if backward {
            haystack[..cursor].rfind(needle)
        } else {
            haystack[cursor..].find(needle).map(|i| cursor + i)
        };
        let Some(start) = found else {
            break;
        };
        let end = start + needle.len();
        if let Some(range) = map(start, end).filter(|range| valid(range)) {
            result.push(range);
            cursor = if backward { start } else { end };
        } else if backward {
            cursor = haystack[..end]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
        } else {
            cursor = start
                + haystack[start..]
                    .chars()
                    .next()
                    .map(char::len_utf8)
                    .unwrap_or(1);
        }
    }
    result.sort_by_key(|range| range.start);
    result
}
/// Literal, non-overlapping matches always start/end at source grapheme boundaries.
pub fn matches(
    text: &str,
    query: &str,
    scope: Range<usize>,
    options: SearchOptions,
) -> Result<Vec<Range<usize>>, String> {
    if query.is_empty() {
        return Err("Enter text to find.".into());
    }
    if scope.start > scope.end
        || scope.end > text.len()
        || !text.is_char_boundary(scope.start)
        || !text.is_char_boundary(scope.end)
    {
        return Err("Search scope changed; reopen Find/Replace.".into());
    }
    let mut edges = vec![0_u8; text.len() + 1];
    let mut previous_word = false;
    for (i, grapheme) in text.grapheme_indices(true) {
        let word = word_character(grapheme);
        edges[i] = 4 | u8::from(!previous_word) | (u8::from(!word) << 1);
        previous_word = word;
    }
    edges[text.len()] = 6 | u8::from(!previous_word);
    if edges[scope.start] & 4 == 0 || edges[scope.end] & 4 == 0 {
        return Err("Search scope splits a Unicode character.".into());
    }
    let valid = |range: &Range<usize>| {
        edges[range.start] & 4 != 0
            && edges[range.end] & 4 != 0
            && (!options.whole_words || (edges[range.start] & 1 != 0 && edges[range.end] & 2 != 0))
    };
    if options.case_sensitive {
        return Ok(scan_matches(
            &text[scope.clone()],
            query,
            options.backward,
            |start, end| Some(scope.start + start..scope.start + end),
            valid,
        ));
    }
    let mut folded = String::new();
    let mut mapping = vec![usize::MAX];
    for (i, grapheme) in text[scope.clone()].grapheme_indices(true) {
        let start = folded.len();
        folded.push_str(&lower(grapheme));
        mapping.resize(folded.len() + 1, usize::MAX);
        mapping[start] = scope.start + i;
        mapping[folded.len()] = scope.start + i + grapheme.len();
    }
    let needle = lower(query);
    let ranges = scan_matches(
        &folded,
        &needle,
        options.backward,
        |i, end| {
            let (start, end) = (mapping[i], mapping[end]);
            (start != usize::MAX && end != usize::MAX).then_some(start..end)
        },
        valid,
    );
    Ok(ranges)
}
pub fn ordered(mut matches: Vec<Range<usize>>, backward: bool, origin: usize) -> Vec<Range<usize>> {
    if backward {
        matches.reverse();
    }
    let index = matches
        .iter()
        .position(|range| {
            if backward {
                range.end <= origin
            } else {
                range.start >= origin
            }
        })
        .unwrap_or(0);
    matches.rotate_left(index);
    matches
}
pub fn select(buffer: &mut Buffer, range: Range<usize>) {
    buffer.move_to(range.start, false);
    buffer.move_to(range.end, true);
}

pub struct ReplaceSession {
    pub remaining: VecDeque<Range<usize>>,
    pub replacement: String,
    pub changed: usize,
}
impl ReplaceSession {
    pub fn new(matches: Vec<Range<usize>>, replacement: String) -> Self {
        Self {
            remaining: matches.into(),
            replacement,
            changed: 0,
        }
    }
    pub fn current(&self) -> Option<Range<usize>> {
        self.remaining.front().cloned()
    }
    pub fn skip(&mut self) {
        self.remaining.pop_front();
    }
    pub fn yes(&mut self, buffer: &mut Buffer) -> Result<(), String> {
        let Some(range) = self.current() else {
            return Ok(());
        };
        buffer.replace(range.clone(), &self.replacement)?;
        self.remaining.pop_front();
        self.changed += 1;
        let delta = self.replacement.len() as isize - range.len() as isize;
        for pending in &mut self.remaining {
            if pending.start >= range.end {
                pending.start = pending.start.saturating_add_signed(delta);
                pending.end = pending.end.saturating_add_signed(delta);
            }
        }
        Ok(())
    }
    pub fn all(&mut self, buffer: &mut Buffer) -> Result<(), String> {
        if self.remaining.is_empty() {
            return Ok(());
        }
        let mut ranges: Vec<_> = self.remaining.iter().cloned().collect();
        ranges.sort_by_key(|r| r.start);
        let start = ranges[0].start;
        let end = ranges[ranges.len() - 1].end;
        let mut output = String::new();
        let mut previous = start;
        for range in &ranges {
            let part = &buffer.text[previous..range.start];
            if output.len() + part.len() + self.replacement.len() > MAX_BYTES {
                return Err(
                    "Replacement exceeds the editor size limit; no changes applied.".into(),
                );
            }
            output.push_str(part);
            output.push_str(&self.replacement);
            previous = range.end;
        }
        output.push_str(&buffer.text[previous..end]);
        buffer.replace(start..end, &output)?;
        self.changed += ranges.len();
        self.remaining.clear();
        Ok(())
    }
}
