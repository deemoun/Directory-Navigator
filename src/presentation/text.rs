//! Styled Unicode text. Width and traversal use whole grapheme clusters.
use super::{layout::Alignment, style::Style};
use std::borrow::Cow;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Span<'a> {
    pub content: Cow<'a, str>,
    pub style: Style,
}
impl<'a> Span<'a> {
    pub fn raw(content: impl Into<Cow<'a, str>>) -> Self {
        Self {
            content: content.into(),
            style: Style::default(),
        }
    }
    pub fn styled(content: impl Into<Cow<'a, str>>, style: Style) -> Self {
        Self {
            content: content.into(),
            style,
        }
    }
    pub fn width(&self) -> usize {
        self.content.width()
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Line<'a> {
    pub spans: Vec<Span<'a>>,
    pub style: Style,
    pub alignment: Option<Alignment>,
}
pub struct StyledGrapheme<'a> {
    pub symbol: &'a str,
    pub style: Style,
}
impl Line<'_> {
    pub fn width(&self) -> usize {
        self.spans.iter().map(Span::width).sum()
    }
    pub fn styled_graphemes(&self, base: Style) -> impl Iterator<Item = StyledGrapheme<'_>> {
        let base = base.patch(self.style);
        self.spans.iter().flat_map(move |span| {
            let style = base.patch(span.style);
            span.content
                .graphemes(true)
                .filter(|g| *g != "\n")
                .map(move |symbol| StyledGrapheme { symbol, style })
        })
    }
}
impl<'a> From<Vec<Span<'a>>> for Line<'a> {
    fn from(spans: Vec<Span<'a>>) -> Self {
        Self {
            spans,
            ..Self::default()
        }
    }
}
impl<'a> From<Span<'a>> for Line<'a> {
    fn from(span: Span<'a>) -> Self {
        Self::from(vec![span])
    }
}
impl<'a> From<&'a str> for Line<'a> {
    fn from(s: &'a str) -> Self {
        Self::from(Span::raw(s))
    }
}
impl From<String> for Line<'_> {
    fn from(s: String) -> Self {
        Self::from(Span::raw(s))
    }
}
impl<'a> From<&'a String> for Line<'a> {
    fn from(s: &'a String) -> Self {
        Self::from(s.as_str())
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Text<'a> {
    pub lines: Vec<Line<'a>>,
    pub style: Style,
}
impl<'a> From<Vec<Line<'a>>> for Text<'a> {
    fn from(lines: Vec<Line<'a>>) -> Self {
        Self {
            lines,
            style: Style::default(),
        }
    }
}
impl<'a> From<Line<'a>> for Text<'a> {
    fn from(line: Line<'a>) -> Self {
        Self::from(vec![line])
    }
}
impl<'a> From<Span<'a>> for Text<'a> {
    fn from(span: Span<'a>) -> Self {
        Self::from(Line::from(span))
    }
}
impl<'a> From<&'a str> for Text<'a> {
    fn from(s: &'a str) -> Self {
        Self::from(s.lines().map(Line::from).collect::<Vec<_>>())
    }
}
impl<'a> From<&'a String> for Text<'a> {
    fn from(s: &'a String) -> Self {
        Self::from(s.as_str())
    }
}
impl From<String> for Text<'_> {
    fn from(s: String) -> Self {
        Self::from(
            s.lines()
                .map(|l| Line::from(l.to_owned()))
                .collect::<Vec<_>>(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presentation::style::{Color, Modifier};
    #[test]
    fn graphemes_keep_span_style_and_unicode_width() {
        let mut line = Line::from(vec![
            Span::styled("e\u{301}", Style::default().fg(Color::Red)),
            Span::raw("界👩‍💻"),
        ]);
        line.style = Style::default().add_modifier(Modifier::BOLD);
        let g: Vec<_> = line
            .styled_graphemes(Style::default().bg(Color::Blue))
            .collect();
        assert_eq!(
            g.iter().map(|g| g.symbol).collect::<Vec<_>>(),
            ["e\u{301}", "界", "👩‍💻"]
        );
        assert_eq!(line.width(), 5);
        assert_eq!(g[0].style.fg, Some(Color::Red));
        assert!(g
            .iter()
            .all(|g| g.style.bg == Some(Color::Blue)
                && g.style.add_modifier.contains(Modifier::BOLD)));
        assert_eq!(Text::from("first\n\nthird\n").lines.len(), 3);
    }
}
