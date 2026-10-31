//! Styled wrapping planned as source ranges. Glyphs and styles stay untouched;
//! only row membership is computed here, independently of native painting.
use super::Run;
use std::ops::Range;

struct Rows<'a> {
    source: &'a [Run],
    widths: Vec<usize>,
    limit: usize,
    trim: bool,
    finished: Vec<Vec<usize>>,
    current: Vec<usize>,
    columns: usize,
    separator: Range<usize>,
}
impl Rows<'_> {
    fn measure(&self, range: Range<usize>) -> usize {
        self.widths[range.end] - self.widths[range.start]
    }
    fn append(&mut self, range: Range<usize>) {
        self.columns += self.measure(range.clone());
        self.current.extend(range);
    }
    fn take_separator(&mut self) {
        let range = self.separator.clone();
        self.separator = range.end..range.end;
        if !self.trim || !self.current.is_empty() {
            self.append(range);
        }
    }
    fn full_before(&self, position: usize, fragment: Range<usize>) -> bool {
        self.columns >= self.limit
            || (self.source[position].width != 0
                && self.measure(self.separator.clone()) + self.measure(fragment)
                    >= self.limit - self.columns)
    }
    fn next_row(&mut self) {
        // Separators that fit in the unused tail are consumed by the boundary.
        // Excess indentation stays available to the following row.
        let mut tail = self.limit.saturating_sub(self.columns);
        while self.separator.start < self.separator.end {
            let columns = usize::from(self.source[self.separator.start].width);
            if columns > tail {
                break;
            }
            tail -= columns;
            self.separator.start += 1;
        }
        self.finished.push(std::mem::take(&mut self.current));
        self.columns = 0;
    }
    fn word(&mut self, range: Range<usize>) {
        let mut fragment = range.start;
        for position in range.clone() {
            let word_columns = self.measure(fragment..position + 1);
            let gap_columns = self.measure(self.separator.clone());
            let first_fragment_fits = if self.trim {
                word_columns <= self.limit
                    && gap_columns + usize::from(self.source[position].width) <= self.limit
            } else {
                word_columns + gap_columns <= self.limit
            };
            if self.current.is_empty() && !first_fragment_fits {
                self.take_separator();
                self.append(fragment..position);
                fragment = position;
            }
            if self.full_before(position, fragment..position) {
                self.next_row();
            }
        }
        self.take_separator();
        self.append(fragment..range.end);
        self.separator = range.end..range.end;
    }
    fn whitespace(&mut self, range: Range<usize>) {
        self.separator = range.start..range.start;
        for position in range {
            if self.current.is_empty()
                && self.measure(self.separator.clone()) + usize::from(self.source[position].width)
                    > self.limit
            {
                self.take_separator();
            }
            if self.full_before(position, position..position) {
                self.next_row();
                if self.separator.is_empty() {
                    self.separator = position + 1..position + 1;
                    continue;
                }
            }
            if self.separator.is_empty() {
                self.separator.start = position;
            }
            self.separator.end = position + 1;
        }
    }
}

pub(super) fn reflow(source: Vec<Run>, limit: u16, trim: bool) -> Vec<Vec<Run>> {
    let source: Vec<_> = source.into_iter().filter(|g| g.width <= limit).collect();
    let mut widths = Vec::with_capacity(source.len() + 1);
    widths.push(0);
    for glyph in &source {
        widths.push(widths.last().unwrap() + usize::from(glyph.width));
    }
    let mut rows = Rows {
        source: &source,
        widths,
        limit: usize::from(limit),
        trim,
        finished: Vec::new(),
        current: Vec::new(),
        columns: 0,
        separator: 0..0,
    };
    let mut start = 0;
    while start < source.len() {
        let whitespace = source[start].space;
        let end = (start + 1..source.len())
            .find(|&i| source[i].space != whitespace)
            .unwrap_or(source.len());
        if whitespace {
            rows.whitespace(start..end);
        } else {
            rows.word(start..end);
        }
        start = end;
    }
    if rows.current.is_empty() && !rows.separator.is_empty() {
        rows.finished.push(Vec::new());
    }
    rows.take_separator();
    if !rows.current.is_empty() {
        rows.finished.push(rows.current);
    }
    if rows.finished.is_empty() {
        rows.finished.push(Vec::new());
    }
    let planned = rows.finished;
    let mut glyphs: Vec<_> = source.into_iter().map(Some).collect();
    planned
        .into_iter()
        .map(|indices| {
            indices
                .into_iter()
                .map(|i| glyphs[i].take().expect("A glyph belongs to only one row"))
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frozen_row_membership_preserves_glyphs_styles_and_whitespace() {
        let fixture = include_str!("../../tests/fixtures/styled_wrapping.txt");
        for (case, line) in fixture.lines().enumerate() {
            let fields: Vec<_> = line.split(';').collect();
            assert_eq!(fields.len(), 4);
            let limit = fields[0].parse().unwrap();
            let trim = fields[1].parse().unwrap();
            let (tokens, remainder) = fields[2].as_bytes().as_chunks::<2>();
            assert!(remainder.is_empty());
            let source: Vec<_> = tokens
                .iter()
                .enumerate()
                .map(|(index, token)| Run {
                    text: format!("glyph-{index}"),
                    style: crate::palette::PaletteProfile::ClassicDn.dos(index as u8),
                    width: u16::from(token[1] - b'0'),
                    space: token[0] == b's',
                })
                .collect();
            let expected: Vec<Vec<_>> = fields[3]
                .split('/')
                .map(|row| {
                    if row == "-" {
                        Vec::new()
                    } else {
                        row.split(',')
                            .map(|index| source[index.parse::<usize>().unwrap()].clone())
                            .collect()
                    }
                })
                .collect();
            let describe = |rows: Vec<Vec<Run>>| {
                rows.into_iter()
                    .map(|row| {
                        row.into_iter()
                            .map(|g| (g.text, g.style, g.width, g.space))
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>()
            };
            assert_eq!(
                describe(reflow(source, limit, trim)),
                describe(expected),
                "case {case}"
            );
        }
        assert_eq!(fixture.lines().count(), 512);
    }
}
