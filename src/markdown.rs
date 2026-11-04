//! Bounded CommonMark presentation shared by the viewer and editor highlighter.
//! No HTML execution, resource loading or link activation.
use crate::task::Cancellation;
use pulldown_cmark::{Alignment, CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use std::{io, ops::Range, path::Path};
use unicode_width::UnicodeWidthStr;
pub const MAX_OUTPUT: usize = 8 * 1024 * 1024;
pub const MAX_ROWS: usize = 100_000;
const MAX_SPANS: usize = 200_000;
pub const LIMIT_NOTICE: &str = "Markdown rendering limit reached; showing source";
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Role {
    #[default]
    Text,
    Heading,
    Strong,
    Emphasis,
    Strike,
    Code,
    Quote,
    Link,
    Rule,
}
impl Role {
    pub fn color(self) -> u8 {
        match self {
            Self::Text => 0x1f,
            Self::Heading => 0x1e,
            Self::Strong => 0x1b,
            Self::Emphasis => 0x1d,
            Self::Strike => 0x18,
            Self::Code => 0x1a,
            Self::Quote => 0x17,
            Self::Link => 0x1b,
            Self::Rule => 0x19,
        }
    }
}
#[derive(Clone, Debug)]
pub struct Run {
    pub range: Range<usize>,
    pub role: Role,
    pub source: Range<usize>,
}
#[derive(Clone, Debug, Default)]
pub struct Row {
    pub text: String,
    pub runs: Vec<Run>,
    pub source_line: usize,
    pub fixed: bool,
}
#[derive(Clone, Debug)]
pub struct SourceStyle {
    pub range: Range<usize>,
    pub role: Role,
    pub code_language: Option<String>,
}
#[derive(Clone, Debug)]
pub struct Document {
    pub rows: Vec<Row>,
    pub styles: Vec<SourceStyle>,
    pub source_lines: usize,
    pub source_starts: Vec<usize>,
}
pub fn is_path(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("md") || s.eq_ignore_ascii_case("markdown"))
}
fn limit() -> io::Error {
    io::Error::other(LIMIT_NOTICE)
}
fn clean(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() && c != '\t' { '·' } else { c })
        .collect()
}
struct Builder {
    rows: Vec<Row>,
    row: Row,
    styles: Vec<SourceStyle>,
    starts: Vec<usize>,
    bytes: usize,
    spans: usize,
    quotes: usize,
    lists: Vec<Option<u64>>,
    item_start: bool,
    code: Option<String>,
    table: Option<Table>,
}
struct Table {
    rows: Vec<Vec<Row>>,
    cells: Vec<Row>,
    align: Vec<Alignment>,
}
impl Builder {
    fn line(&self, offset: usize) -> usize {
        self.starts
            .partition_point(|s| *s <= offset)
            .saturating_sub(1)
    }
    fn style(
        &mut self,
        range: Range<usize>,
        role: Role,
        code_language: Option<String>,
    ) -> io::Result<()> {
        if self.styles.len() >= MAX_SPANS {
            return Err(limit());
        }
        self.styles.push(SourceStyle {
            range,
            role,
            code_language,
        });
        Ok(())
    }
    fn append(&mut self, s: &str, role: Role, source: Range<usize>) -> io::Result<()> {
        let s = clean(s);
        let before = self.row.text.len();
        self.spans += 1;
        if self.spans > MAX_SPANS {
            return Err(limit());
        }
        if self.row.text.is_empty() {
            self.row.source_line = self.line(source.start);
        }
        let start = self.row.text.len();
        // Linear expansion: computing the whole line width for every tab would
        // make a long code line quadratic.
        let mut column = self.row.text.width();
        for part in s.split_inclusive('\t') {
            if let Some(text) = part.strip_suffix('\t') {
                self.row.text.push_str(text);
                column += text.width();
                let spaces = 4 - column % 4;
                self.row.text.push_str(&" ".repeat(spaces));
                column += spaces;
            } else {
                self.row.text.push_str(part);
                column += part.width();
            }
        }
        self.bytes = self
            .bytes
            .checked_add(self.row.text.len() - before)
            .filter(|n| *n <= MAX_OUTPUT)
            .ok_or_else(limit)?;
        if self.row.text.len() > MAX_OUTPUT || self.row.runs.len() >= MAX_SPANS {
            return Err(limit());
        }
        self.row.runs.push(Run {
            range: start..self.row.text.len(),
            role,
            source,
        });
        Ok(())
    }
    fn prefix(&mut self, source: Range<usize>) -> io::Result<()> {
        if self.row.text.is_empty() {
            if self.quotes > 0 {
                self.append(&"│ ".repeat(self.quotes), Role::Quote, source.clone())?;
            }
            let indent = self.lists.len().saturating_sub(1) * 2;
            if indent > 0 {
                self.append(&" ".repeat(indent), Role::Text, source)?;
            }
        }
        Ok(())
    }
    fn flush(&mut self, blank: bool) -> io::Result<()> {
        if self.table.is_some() {
            return Ok(());
        }
        if !self.row.text.is_empty() || blank {
            if self.rows.len() >= MAX_ROWS {
                return Err(limit());
            }
            let mut row = std::mem::take(&mut self.row);
            row.fixed = self.code.is_some();
            self.rows.push(row);
        }
        Ok(())
    }
    fn space(&mut self) -> io::Result<()> {
        self.flush(false)?;
        if self.rows.last().is_some_and(|r| !r.text.is_empty()) {
            self.flush(true)?;
        }
        Ok(())
    }
    fn table_end(&mut self) -> io::Result<()> {
        let table = self.table.take().unwrap();
        self.bytes = self.bytes.saturating_sub(
            table
                .rows
                .iter()
                .flatten()
                .map(|c| c.text.len())
                .sum::<usize>(),
        );
        let cols = table.align.len();
        let mut widths = vec![3; cols];
        for row in &table.rows {
            for (i, c) in row.iter().take(cols).enumerate() {
                widths[i] = widths[i].max(c.text.width());
            }
        }
        let width = widths.iter().try_fold(1usize, |sum, w| {
            sum.checked_add(w + 3)
                .filter(|n| *n <= MAX_OUTPUT)
                .ok_or_else(limit)
        })?;
        if self.rows.len() + table.rows.len() * 2 > MAX_ROWS
            || width
                .checked_mul(table.rows.len())
                .is_none_or(|n| n > MAX_OUTPUT.saturating_sub(self.bytes))
        {
            return Err(limit());
        }
        for (index, cells) in table.rows.into_iter().enumerate() {
            let source = cells.first().map_or(0, |c| c.source_line);
            let mut row = Row {
                source_line: source,
                fixed: true,
                ..Default::default()
            };
            row.text.push('│');
            for (i, w) in widths.iter().enumerate() {
                let empty = Row::default();
                let cell = cells.get(i).unwrap_or(&empty);
                let gap = w.saturating_sub(cell.text.width());
                let left = match table.align[i] {
                    Alignment::Right => gap,
                    Alignment::Center => gap / 2,
                    _ => 0,
                };
                row.text.push_str(&" ".repeat(left + 1));
                let start = row.text.len();
                row.text.push_str(&cell.text);
                for run in &cell.runs {
                    let mut run = run.clone();
                    run.range = run.range.start + start..run.range.end + start;
                    row.runs.push(run);
                }
                row.text.push_str(&" ".repeat(gap - left + 1));
                row.text.push('│');
            }
            self.bytes = self
                .bytes
                .checked_add(row.text.len())
                .filter(|n| *n <= MAX_OUTPUT)
                .ok_or_else(limit)?;
            self.rows.push(row);
            if index == 0 {
                let separator = Row {
                    text: format!(
                        "├{}┤",
                        widths
                            .iter()
                            .map(|w| "─".repeat(w + 2))
                            .collect::<Vec<_>>()
                            .join("┼")
                    ),
                    source_line: source,
                    fixed: true,
                    ..Default::default()
                };
                self.bytes = self
                    .bytes
                    .checked_add(separator.text.len())
                    .filter(|n| *n <= MAX_OUTPUT)
                    .ok_or_else(limit)?;
                self.rows.push(separator);
            }
        }
        self.space()
    }
}
pub fn parse(source: &str, cancel: &Cancellation) -> io::Result<Document> {
    if source.len() > crate::viewer::MAX_BYTES {
        return Err(limit());
    }
    let mut starts = vec![0];
    for (i, b) in source.bytes().enumerate() {
        if b == b'\n' {
            starts.push(i + 1);
        }
    }
    if starts.len() > MAX_ROWS {
        return Err(limit());
    }
    let mut b = Builder {
        rows: Vec::new(),
        row: Row::default(),
        styles: Vec::new(),
        starts,
        bytes: 0,
        spans: 0,
        quotes: 0,
        lists: Vec::new(),
        item_start: false,
        code: None,
        table: None,
    };
    let mut roles = Vec::new();
    let mut links: Vec<(String, usize, bool)> = Vec::new();
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH;
    let bom = if source.starts_with('\u{feff}') { 3 } else { 0 };
    for (event, mut range) in Parser::new_ext(&source[bom..], options).into_offset_iter() {
        cancel.check()?;
        range.start += bom;
        range.end += bom;
        let role = roles.last().copied().unwrap_or_default();
        match event {
            Event::Start(tag) => {
                if roles.len() >= 64 {
                    return Err(limit());
                }
                let next = match &tag {
                    Tag::TableHead => Role::Strong,
                    Tag::Heading { .. } => Role::Heading,
                    Tag::Strong => Role::Strong,
                    Tag::Emphasis => Role::Emphasis,
                    Tag::Strikethrough => Role::Strike,
                    Tag::CodeBlock(_) => Role::Code,
                    Tag::BlockQuote(_) => Role::Quote,
                    Tag::Link { .. } | Tag::Image { .. } => Role::Link,
                    _ => role,
                };
                if next != Role::Text {
                    b.style(range.clone(), next, None)?;
                }
                match &tag {
                    Tag::Paragraph => {
                        if !b.item_start {
                            b.flush(false)?;
                        }
                        b.item_start = false;
                    }
                    Tag::Heading { .. } | Tag::CodeBlock(_) | Tag::Table(_) => b.space()?,
                    _ => {}
                }
                match tag {
                    Tag::BlockQuote(_) => {
                        b.flush(false)?;
                        b.quotes += 1;
                    }
                    Tag::List(start) => {
                        b.flush(false)?;
                        b.lists.push(start);
                    }
                    Tag::Item => {
                        b.flush(false)?;
                        b.prefix(range.clone())?;
                        let marker = if let Some(Some(n)) = b.lists.last_mut() {
                            let s = format!("{n}. ");
                            *n = n.saturating_add(1);
                            s
                        } else {
                            "• ".into()
                        };
                        b.append(&marker, Role::Text, range.clone())?;
                        b.item_start = true;
                    }
                    Tag::CodeBlock(kind) => {
                        b.code = Some(match kind {
                            CodeBlockKind::Fenced(name) => {
                                name.split_whitespace().next().unwrap_or("").to_string()
                            }
                            CodeBlockKind::Indented => String::new(),
                        })
                    }
                    Tag::Table(align) => {
                        b.table = Some(Table {
                            rows: Vec::new(),
                            cells: Vec::new(),
                            align,
                        })
                    }
                    Tag::TableHead => {}
                    Tag::Link { dest_url, .. } => {
                        links.push((dest_url.into_string(), b.row.text.len(), false))
                    }
                    Tag::Image { dest_url, .. } => {
                        b.prefix(range.clone())?;
                        b.append("[Image: ", Role::Link, range.clone())?;
                        links.push((dest_url.into_string(), b.row.text.len(), true));
                    }
                    _ => {}
                }
                roles.push(next);
            }
            Event::End(tag) => {
                match tag {
                    TagEnd::Paragraph => {
                        if b.lists.is_empty() {
                            b.space()?;
                        } else {
                            b.flush(false)?;
                        }
                    }
                    TagEnd::Heading(_) => b.space()?,
                    TagEnd::Item => b.flush(false)?,
                    TagEnd::List(_) => {
                        b.flush(false)?;
                        b.lists.pop();
                        if b.lists.is_empty() {
                            b.space()?;
                        }
                    }
                    TagEnd::BlockQuote(_) => {
                        b.flush(false)?;
                        b.quotes = b.quotes.saturating_sub(1);
                        b.space()?;
                    }
                    TagEnd::CodeBlock => {
                        b.flush(false)?;
                        b.code = None;
                        b.space()?;
                    }
                    TagEnd::Link | TagEnd::Image => {
                        if let Some((url, start, image)) = links.pop() {
                            let same = b.row.text.get(start..) == Some(url.as_str());
                            if image {
                                b.append("]", Role::Link, range.clone())?;
                            }
                            if !same {
                                b.append(&format!(" ({url})"), Role::Link, range.clone())?;
                            }
                        }
                    }
                    TagEnd::TableCell => {
                        let row = std::mem::take(&mut b.row);
                        b.table.as_mut().unwrap().cells.push(row);
                    }
                    TagEnd::TableHead | TagEnd::TableRow => {
                        let t = b.table.as_mut().unwrap();
                        t.rows.push(std::mem::take(&mut t.cells));
                    }
                    TagEnd::Table => b.table_end()?,
                    _ => {}
                }
                roles.pop();
            }
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                if b.code.is_some() {
                    b.style(range.clone(), Role::Code, b.code.clone())?;
                }
                for (i, line) in text.split('\n').enumerate() {
                    if i > 0 {
                        if b.code.is_some() {
                            b.flush(true)?;
                        } else if b.table.is_some() {
                            b.append(" ", role, range.clone())?;
                        } else {
                            b.flush(false)?;
                        }
                    }
                    if b.code.is_some() {
                        b.row.source_line = b.line(range.start) + i;
                    }
                    if !line.is_empty() {
                        b.prefix(range.clone())?;
                        b.append(line.strip_suffix('\r').unwrap_or(line), role, range.clone())?;
                        if b.code.is_some() {
                            b.row.source_line = b.line(range.start) + i;
                        }
                    }
                }
            }
            Event::Code(text) => {
                b.style(range.clone(), Role::Code, None)?;
                b.prefix(range.clone())?;
                b.append(&text, Role::Code, range)?;
            }
            Event::SoftBreak => {
                b.append(" ", role, range)?;
            }
            Event::HardBreak => {
                b.flush(false)?;
            }
            Event::Rule => {
                b.space()?;
                b.append("────────────────────────", Role::Rule, range)?;
                b.space()?;
            }
            Event::TaskListMarker(checked) => {
                b.append(if checked { "[x] " } else { "[ ] " }, Role::Strong, range)?
            }
            _ => {}
        }
    }
    cancel.check()?;
    b.flush(false)?;
    while b.rows.last().is_some_and(|r| r.text.is_empty()) {
        b.rows.pop();
    }
    if b.rows.is_empty() {
        b.rows.push(Row::default());
    }
    Ok(Document {
        rows: b.rows,
        styles: b.styles,
        source_lines: source.lines().count().max(1),
        source_starts: b.starts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn text(doc: &Document) -> String {
        doc.rows
            .iter()
            .map(|r| r.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
    #[test]
    fn commonmark_blocks_inline_styles_links_and_literal_html() {
        let source="# Heading\n\n**bold** *soft* ~~gone~~ `code` [site](https://example.org)\n\n![caption](image.png)\n\n> quote\n\n- [x] done\n- [ ] todo\n\n---\n\n<div>literal HTML</div>\n";
        let doc = parse(source, &Cancellation::default()).unwrap();
        let out = text(&doc);
        for value in [
            "Heading",
            "bold soft gone code site (https://example.org)",
            "[Image: caption] (image.png)",
            "│ quote",
            "[x] done",
            "[ ] todo",
            "<div>literal HTML</div>",
        ] {
            assert!(out.contains(value), "{value}: {out}");
        }
        assert!(!out.contains("**"));
        for role in [
            Role::Heading,
            Role::Strong,
            Role::Emphasis,
            Role::Strike,
            Role::Code,
            Role::Link,
        ] {
            assert!(doc.styles.iter().any(|s| s.role == role));
        }
    }
    #[test]
    fn tables_unicode_alignment_code_tabs_crlf_and_source_lines() {
        let source="| Name | Value |\r\n|:---|---:|\r\n| 界 | 42 |\r\n\r\n```rust\r\nfn main() {\r\n\t42\r\n}\r\n```\r\n";
        let doc = parse(source, &Cancellation::default()).unwrap();
        let out = text(&doc);
        assert!(out.contains("│ Name │ Value │"), "{out}");
        assert!(out.contains("    42"), "{out}");
        assert!(!out.contains('\r'));
        assert!(!out.contains('·'));
        let row = doc.rows.iter().find(|r| r.text == "    42").unwrap();
        assert!(row.fixed);
        assert_eq!(row.source_line, 6);
        assert!(doc
            .styles
            .iter()
            .any(|s| s.code_language.as_deref() == Some("rust")));
        assert_eq!(doc.source_lines, 9);
    }
    #[test]
    fn cancellation_nesting_and_input_limits_fall_back_without_panics() {
        let cancel = Cancellation::default();
        cancel.cancel();
        assert!(parse("hello", &cancel).is_err());
        assert!(parse(
            &format!("{}text", "> ".repeat(65)),
            &Cancellation::default()
        )
        .is_err());
        assert!(parse(
            &"x".repeat(crate::viewer::MAX_BYTES + 1),
            &Cancellation::default()
        )
        .is_err());
        for source in [
            "",
            "```",
            "**unfinished",
            "<!S\r:",
            "[a](unterminated",
            "\x1b[31mtext",
        ] {
            let doc = parse(source, &Cancellation::default()).unwrap();
            assert!(!text(&doc).contains('\x1b'));
        }
    }
}

#[cfg(test)]
mod bounds_tests {
    use super::*;
    #[test]
    fn wide_tables_are_rejected_before_padding_can_expand_quadratically() {
        let source = format!(
            "| {} | b |\n|---|---|\n{}",
            "x".repeat(100_000),
            "| a | b |\n".repeat(100)
        );
        assert_eq!(
            parse(&source, &Cancellation::default())
                .unwrap_err()
                .to_string(),
            LIMIT_NOTICE
        );
    }
    #[test]
    fn bom_and_nested_lists_keep_source_offsets_and_unicode() {
        let source = "\u{feff}# Title\n\n1. first\n   - 界\n\nnext";
        let doc = parse(source, &Cancellation::default()).unwrap();
        assert!(doc.rows.iter().any(|r| r.text == "Title"));
        assert!(doc
            .styles
            .iter()
            .any(|s| s.role == Role::Heading && s.range.start == 3));
        assert!(doc.rows.iter().any(|r| r.text.contains("1. first")));
        assert!(doc.rows.iter().any(|r| r.text.contains("  • 界")));
    }
}

#[cfg(test)]
mod source_map_tests {
    #[test]
    fn blank_code_lines_retain_their_source_line() {
        let doc = super::parse("```\na\n\nb\n```", &crate::task::Cancellation::default()).unwrap();
        assert!(doc
            .rows
            .iter()
            .any(|r| r.fixed && r.text.is_empty() && r.source_line == 2));
    }
}
