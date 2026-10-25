//! Native presentation primitives. All surfaces paint Turbo Vision scenes.
use crate::presentation::{
    layout::{Alignment, Rect},
    style::Style,
    text::{Line, Text},
};
use crate::{app::App, layout::Prepared};
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum BorderType {
    #[default]
    Plain,
    Double,
}
#[derive(Clone, Copy, Default)]
pub struct Borders(u8);
impl Borders {
    pub const LEFT: Self = Self(1);
    pub const RIGHT: Self = Self(2);
    pub const TOP: Self = Self(4);
    pub const BOTTOM: Self = Self(8);
    pub const ALL: Self = Self(15);
    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}
#[derive(Clone, Copy)]
pub struct Wrap {
    pub trim: bool,
}

pub trait Frame {
    fn area(&self) -> Rect;
    fn palette(&mut self, profile: crate::palette::PaletteProfile);
    fn render_widget(&mut self, widget: impl Widget, area: Rect);
    fn set_cursor_position(&mut self, position: impl Into<crate::presentation::layout::Position>);
    fn manager(&mut self, app: &mut App, prepared: &Prepared) -> Vec<Rect>;
    fn game(&mut self, app: &mut App, area: Rect);
}
pub trait Widget {
    fn native(self, frame: &mut NativeFrame, area: Rect);
}

#[derive(Default)]
pub struct Block<'a> {
    style: Style,
    border_style: Style,
    title_style: Style,
    borders: Borders,
    border_type: BorderType,
    title: Option<Line<'a>>,
    alignment: Alignment,
}
impl<'a> Block<'a> {
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }
    pub fn border_style(mut self, style: Style) -> Self {
        self.border_style = style;
        self
    }
    pub fn title_style(mut self, style: Style) -> Self {
        self.title_style = style;
        self
    }
    pub fn borders(mut self, borders: Borders) -> Self {
        self.borders = borders;
        self
    }
    pub fn border_type(mut self, kind: BorderType) -> Self {
        self.border_type = kind;
        self
    }
    pub fn title(mut self, title: impl Into<Line<'a>>) -> Self {
        self.title = Some(title.into());
        self
    }
    pub fn title_alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }
    pub fn inner(&self, mut r: Rect) -> Rect {
        if self.borders.contains(Borders::LEFT) && r.width > 0 {
            r.x += 1;
            r.width -= 1;
        }
        if self.borders.contains(Borders::RIGHT) {
            r.width = r.width.saturating_sub(1);
        }
        if (self.borders.contains(Borders::TOP) || self.title.is_some()) && r.height > 0 {
            r.y += 1;
            r.height -= 1;
        }
        if self.borders.contains(Borders::BOTTOM) {
            r.height = r.height.saturating_sub(1);
        }
        r
    }
}
impl Widget for Block<'_> {
    fn native(self, f: &mut NativeFrame, area: Rect) {
        let r = area.intersection(f.area());
        if r.is_empty() {
            return;
        }
        f.style(r, self.style);
        let double = self.border_type == BorderType::Double;
        let (h, v, tl, tr, bl, br) = if double {
            ("═", "║", "╔", "╗", "╚", "╝")
        } else {
            ("─", "│", "┌", "┐", "└", "┘")
        };
        let top = self.borders.contains(Borders::TOP);
        let bottom = self.borders.contains(Borders::BOTTOM);
        let left = self.borders.contains(Borders::LEFT);
        let right = self.borders.contains(Borders::RIGHT);
        for x in r.x..r.right() {
            if top {
                f.put(x, r.y, h, self.border_style);
            }
            if bottom {
                f.put(x, r.bottom() - 1, h, self.border_style);
            }
        }
        for y in r.y..r.bottom() {
            if left {
                f.put(r.x, y, v, self.border_style);
            }
            if right {
                f.put(r.right() - 1, y, v, self.border_style);
            }
        }
        for (show, x, y, text) in [
            (top && left, r.x, r.y, tl),
            (top && right, r.right() - 1, r.y, tr),
            (bottom && left, r.x, r.bottom() - 1, bl),
            (bottom && right, r.right() - 1, r.bottom() - 1, br),
        ] {
            if show {
                f.put(x, y, text, self.border_style);
            }
        }
        if let Some(title) = self.title {
            let x = r.x + u16::from(left);
            let width = r.width.saturating_sub(u16::from(left) + u16::from(right));
            let title_width = title.width().min(usize::from(width)) as u16;
            let offset = match title.alignment.unwrap_or(self.alignment) {
                Alignment::Left => 0,
                Alignment::Center => (width - title_width) / 2,
                Alignment::Right => width - title_width,
            };
            let mut title = title;
            title.style = self.title_style.patch(title.style);
            title.alignment = None;
            Paragraph::new(title).native(f, Rect::new(x + offset, r.y, width - offset, 1));
        }
    }
}

pub struct Paragraph<'a> {
    text: Text<'a>,
    style: Style,
    alignment: Alignment,
    scroll: (u16, u16),
    wrap: Option<Wrap>,
}
impl<'a> Paragraph<'a> {
    pub fn new(text: impl Into<Text<'a>>) -> Self {
        Self {
            text: text.into(),
            style: Style::default(),
            alignment: Alignment::Left,
            scroll: (0, 0),
            wrap: None,
        }
    }
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }
    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }
    pub fn scroll(mut self, scroll: (u16, u16)) -> Self {
        self.scroll = scroll;
        self
    }
    pub fn wrap(mut self, wrap: Wrap) -> Self {
        self.wrap = Some(wrap);
        self
    }
}
impl Widget for Paragraph<'_> {
    fn native(self, f: &mut NativeFrame, area: Rect) {
        use unicode_width::UnicodeWidthStr;
        let r = area.intersection(f.area());
        if r.is_empty() {
            return;
        }
        f.style(r, self.style);
        let mut rows = Vec::new();
        for line in &self.text.lines {
            let glyphs: Vec<_> = line
                .styled_graphemes(self.text.style)
                .map(|g| Run {
                    width: g.symbol.width() as u16,
                    space: g.symbol == "\u{200b}"
                        || (g.symbol != "\u{a0}" && g.symbol.chars().all(char::is_whitespace)),
                    text: g.symbol.to_owned(),
                    style: g.style,
                })
                .collect();
            let alignment = line.alignment.unwrap_or(self.alignment);
            if let Some(wrap) = self.wrap {
                for line in reflow(glyphs, r.width, wrap.trim) {
                    rows.push((line, alignment));
                }
            } else {
                let (mut skipped, mut width) = (self.scroll.1, 0);
                let mut visible = Vec::new();
                for g in glyphs {
                    if g.width > r.width {
                        continue;
                    }
                    if width + g.width > r.width {
                        break;
                    }
                    if skipped >= g.width {
                        skipped -= g.width;
                        continue;
                    }
                    skipped = 0;
                    width += g.width;
                    visible.push(g);
                }
                rows.push((visible, alignment));
            }
        }
        for (row, (line, alignment)) in rows
            .into_iter()
            .skip(usize::from(self.scroll.0))
            .take(usize::from(r.height))
            .enumerate()
        {
            let width: u16 = line.iter().map(|g| g.width).sum();
            let offset = match alignment {
                Alignment::Left => 0,
                Alignment::Center => (r.width / 2).saturating_sub(width / 2),
                Alignment::Right => r.width.saturating_sub(width),
            };
            let mut x = r.x + offset;
            for g in line {
                if g.width > 0 && x < r.right() {
                    f.put(x, r.y + row as u16, &g.text, g.style);
                    x += g.width;
                }
            }
        }
    }
}
pub struct Clear;
impl Widget for Clear {
    fn native(self, f: &mut NativeFrame, area: Rect) {
        let r = area.intersection(f.area());
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                f.put(x, y, " ", Style::reset());
            }
        }
    }
}
#[derive(Clone)]
struct Run {
    text: String,
    style: Style,
    width: u16,
    space: bool,
}
fn reflow(glyphs: Vec<Run>, limit: u16, trim: bool) -> Vec<Vec<Run>> {
    use std::collections::VecDeque;
    // Preserve the reference renderer's word-boundary behavior, adapted from
    // Ratatui 0.29's MIT-licensed reflow implementation (see third-party notices).
    let limit = usize::from(limit);
    let mut output = Vec::new();
    let mut line = Vec::new();
    let mut word = Vec::new();
    let mut spaces: VecDeque<Run> = VecDeque::new();
    let (mut line_width, mut word_width, mut space_width) = (0, 0, 0);
    let mut previous_word = false;
    for glyph in glyphs {
        let width = usize::from(glyph.width);
        if width > limit {
            continue;
        }
        let ended_word = previous_word && glyph.space;
        let pending_overflow = line.is_empty()
            && if trim {
                word_width + width > limit || space_width + width > limit
            } else {
                word_width + space_width + width > limit
            };
        if ended_word || pending_overflow {
            if !line.is_empty() || !trim {
                line.extend(spaces.drain(..));
                line_width += space_width;
            }
            line.append(&mut word);
            line_width += word_width;
            spaces.clear();
            word_width = 0;
            space_width = 0;
        }
        if line_width >= limit || (width > 0 && line_width + space_width + word_width >= limit) {
            output.push(std::mem::take(&mut line));
            let mut remaining = limit.saturating_sub(line_width);
            line_width = 0;
            while let Some(space) = spaces.front() {
                let width = usize::from(space.width);
                if width > remaining {
                    break;
                }
                space_width -= width;
                remaining -= width;
                spaces.pop_front();
            }
            if glyph.space && spaces.is_empty() {
                continue;
            }
        }
        previous_word = !glyph.space;
        if glyph.space {
            space_width += width;
            spaces.push_back(glyph);
        } else {
            word_width += width;
            word.push(glyph);
        }
    }
    if line.is_empty() && word.is_empty() && !spaces.is_empty() {
        output.push(Vec::new());
    }
    if !line.is_empty() || !trim {
        line.extend(spaces);
    }
    line.append(&mut word);
    if !line.is_empty() {
        output.push(line);
    }
    if output.is_empty() {
        output.push(Vec::new());
    }
    output
}

pub struct NativeFrame {
    scene: crate::turbo::scene::Scene,
}
impl NativeFrame {
    pub fn new(area: Rect, profile: crate::palette::PaletteProfile) -> Self {
        Self {
            scene: crate::turbo::scene::Scene {
                width: area.width,
                height: area.height,
                profile,
                cells: Default::default(),
                cursor: None,
            },
        }
    }
    pub(crate) fn finish(self) -> crate::turbo::scene::Scene {
        self.scene
    }
    fn style(&mut self, area: Rect, style: Style) {
        for y in area.y..area.bottom().min(self.scene.height) {
            for x in area.x..area.right().min(self.scene.width) {
                let glyph =
                    self.scene
                        .cells
                        .entry((x, y))
                        .or_insert_with(|| crate::turbo::scene::Glyph {
                            text: " ".into(),
                            attribute: 0x07,
                            style: Some(Style::reset()),
                        });
                glyph.style = Some(glyph.resolved(self.scene.profile).patch(style));
            }
        }
    }
    fn put(&mut self, x: u16, y: u16, text: &str, style: Style) {
        use unicode_width::UnicodeWidthStr;
        if x >= self.scene.width || y >= self.scene.height {
            return;
        }
        let previous_width = self.scene.cells.get(&(x, y)).map_or(1, |g| g.text.width());
        let incoming_width = text.width();
        for offset in incoming_width.max(1)..previous_width {
            if let Some(glyph) = self.scene.cells.get_mut(&(x + offset as u16, y)) {
                if glyph.text.is_empty() {
                    glyph.text = " ".into();
                }
            }
        }
        let base = self
            .scene
            .cells
            .get(&(x, y))
            .map(|g| g.resolved(self.scene.profile))
            .unwrap_or_else(Style::reset);
        let style = base.patch(style);
        self.scene.cells.insert(
            (x, y),
            crate::turbo::scene::Glyph {
                text: text.into(),
                attribute: 0x07,
                style: Some(style),
            },
        );
        for offset in 1..(text.width() as u16).min(self.scene.width - x) {
            self.scene.cells.insert(
                (x + offset, y),
                crate::turbo::scene::Glyph {
                    text: String::new(),
                    attribute: 0x07,
                    style: Some(style),
                },
            );
        }
    }
}
impl Frame for NativeFrame {
    fn palette(&mut self, profile: crate::palette::PaletteProfile) {
        self.scene.profile = profile;
    }
    fn area(&self) -> Rect {
        Rect::new(0, 0, self.scene.width, self.scene.height)
    }
    fn render_widget(&mut self, widget: impl Widget, area: Rect) {
        widget.native(self, area);
    }
    fn set_cursor_position(&mut self, position: impl Into<crate::presentation::layout::Position>) {
        let p = position.into();
        self.scene.cursor = Some((p.x, p.y));
    }
    fn manager(&mut self, app: &mut App, prepared: &Prepared) -> Vec<Rect> {
        let (scene, positions) =
            crate::turbo::manager::prepare(app, prepared, self.scene.width, self.scene.height);
        self.scene = scene;

        positions
    }
    fn game(&mut self, app: &mut App, area: Rect) {
        let overlay = crate::turbo::tetris::prepare(app, area.width, area.height);
        self.scene.cells.extend(overlay.cells);
        self.scene.cursor = overlay.cursor;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::PaletteProfile;
    #[test]
    fn native_wrapping_preserves_frozen_spaces_words_and_unicode_contract() -> std::io::Result<()> {
        let samples = [
            "one two three four five".into(),
            "  one    two      three ".into(),
            "x".repeat(150),
            "界 e\u{301} 👩‍💻 ".repeat(12),
            "first\n\nthird\n".into(),
            "a\u{a0}b c\u{200b}d".into(),
            "/path/with-segments/and-a-long-name/next: cannot open file".into(),
            "one ".repeat(30),
        ];
        let mut expected = include_str!("../tests/fixtures/wrapping.txt").lines();
        for width in [1, 2, 5, 17, 44] {
            for sample in &samples {
                let area = Rect::new(0, 0, width, 20);
                let style = PaletteProfile::ClassicDn.dos(0x70);
                let mut native = NativeFrame::new(area, PaletteProfile::ClassicDn);
                native.render_widget(
                    Paragraph::new(sample.as_str())
                        .style(style)
                        .wrap(Wrap { trim: false }),
                    area,
                );
                let (buffer, _) = crate::testing::Buffer::capture(native);
                let actual = format!(
                    "{:?}",
                    buffer
                        .content
                        .iter()
                        .map(|c| c.symbol())
                        .collect::<Vec<_>>()
                );
                let expected = expected.next().unwrap();
                assert_eq!(actual, expected, "sample={sample:?},width={width}");
            }
        }
        Ok(())
    }
}
