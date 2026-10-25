//! Lossless boundary between DN's grapheme cells/indexed colors and TV's char/RGB cells.
//! Tokens exist only inside the TV buffer; the transport decodes them before writing.
use crate::presentation::{
    layout::Size,
    style::{Color, Modifier},
};
use std::{collections::HashMap, io};
use turbo_vision::core::{
    draw::Cell as TvCell,
    palette::{Attr, Style, TvColor},
};
#[derive(Clone)]
pub(super) struct Cell {
    symbol: String,
    fg: Color,
    bg: Color,
    modifier: Modifier,
}
impl Default for Cell {
    fn default() -> Self {
        Self {
            symbol: " ".into(),
            fg: Color::Reset,
            bg: Color::Reset,
            modifier: Modifier::empty(),
        }
    }
}
impl Cell {
    pub fn symbol(&self) -> &str {
        &self.symbol
    }
    pub fn set_symbol(&mut self, symbol: &str) {
        self.symbol = symbol.into();
    }
    pub fn set_style(&mut self, style: crate::presentation::style::Style) {
        if let Some(fg) = style.fg {
            self.fg = fg;
        }
        if let Some(bg) = style.bg {
            self.bg = bg;
        }
        self.modifier.insert(style.add_modifier);
        self.modifier.remove(style.sub_modifier);
    }
}
#[derive(Default)]
pub(super) struct Codec {
    glyphs: HashMap<char, (String, usize)>,
    symbols: HashMap<String, char>,
    positions: Vec<Option<char>>,
    free: Vec<char>,
    next: u32,
    colors: Vec<Color>,
    size: Size,
}
const COLOR_BASE: u32 = 0xfe0000;
impl Codec {
    pub fn resize(&mut self, size: Size) {
        self.glyphs.clear();
        self.symbols.clear();
        self.free.clear();
        self.next = 0;
        self.positions = vec![None; usize::from(size.width) * usize::from(size.height)];
        self.size = size;
    }
    fn release(&mut self, x: u16, y: u16) {
        let pos = usize::from(y) * usize::from(self.size.width) + usize::from(x);
        if let Some(token) = self.positions[pos].take() {
            if let Some((_, refs)) = self.glyphs.get_mut(&token) {
                *refs -= 1;
            }
        }
    }
    fn token(&mut self, symbol: &str) -> io::Result<char> {
        if let Some(token) = self.symbols.get(symbol).copied() {
            self.glyphs.get_mut(&token).expect("live glyph").1 += 1;
            return Ok(token);
        }
        let token = if let Some(token) = self.free.pop() {
            token
        } else {
            let n = self.next;
            let value = if n < 65534 {
                0xf0000 + n
            } else {
                0x100000 + n - 65534
            };
            if n >= 131068 {
                return Err(io::Error::other(
                    "Too many simultaneous graphemes for Turbo Vision",
                ));
            }
            self.next += 1;
            char::from_u32(value).expect("private-use scalar")
        };
        self.glyphs.insert(token, (symbol.into(), 1));
        self.symbols.insert(symbol.into(), token);
        Ok(token)
    }
    fn color(&mut self, color: Color) -> io::Result<TvColor> {
        let index = if let Some(index) = self.colors.iter().position(|c| *c == color) {
            index
        } else {
            if self.colors.len() >= 4096 {
                return Err(io::Error::other("Too many simultaneous UI colors"));
            }
            self.colors.push(color);
            self.colors.len() - 1
        };
        let token = COLOR_BASE + index as u32;
        Ok(TvColor::Rgb {
            r: (token >> 16) as u8,
            g: (token >> 8) as u8,
            b: token as u8,
        })
    }
    pub fn cell(&mut self, x: u16, y: u16, cell: &Cell) -> io::Result<(TvCell, usize)> {
        self.release(x, y);
        let symbol = cell.symbol();
        let mut chars = symbol.chars();
        let first = chars.next().unwrap_or(' ');
        let complex = chars.next().is_some()
            || ('\u{f0000}'..='\u{ffffd}').contains(&first)
            || ('\u{100000}'..='\u{10fffd}').contains(&first);
        let ch = if complex {
            let token = self.token(symbol)?;
            self.positions[usize::from(y) * usize::from(self.size.width) + usize::from(x)] =
                Some(token);
            token
        } else {
            first
        };
        let mut attr = Attr::new(self.color(cell.fg)?, self.color(cell.bg)?);
        for (source, destination) in [
            (Modifier::BOLD, Style::BOLD),
            (Modifier::DIM, Style::DIM),
            (Modifier::ITALIC, Style::ITALIC),
            (Modifier::UNDERLINED, Style::UNDERLINE),
            (Modifier::REVERSED, Style::REVERSE),
            (Modifier::CROSSED_OUT, Style::STRIKETHROUGH),
        ] {
            if cell.modifier.contains(source) {
                attr.style.insert(destination);
            }
        }
        Ok((
            TvCell::new(ch, attr),
            unicode_width::UnicodeWidthStr::width(symbol).max(1),
        ))
    }
    // Keep old glyph identities until the terminal has committed its diff. A
    // repeated grapheme then retains its token across consecutive native frames.
    pub fn begin_frame(&mut self) {
        for token in self.positions.iter_mut().filter_map(Option::take) {
            self.glyphs.get_mut(&token).expect("live glyph").1 -= 1;
        }
    }
    pub fn committed(&mut self) {
        let dead: Vec<_> = self
            .glyphs
            .iter()
            .filter(|(_, (_, refs))| *refs == 0)
            .map(|(key, _)| *key)
            .collect();
        for token in dead {
            if let Some((symbol, _)) = self.glyphs.remove(&token) {
                self.symbols.remove(&symbol);
                self.free.push(token);
            }
        }
    }
    pub fn decode(&self, bytes: &[u8]) -> io::Result<Vec<u8>> {
        let text = std::str::from_utf8(bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let mut output = String::with_capacity(text.len());
        let mut rest = text;
        while !rest.is_empty() {
            if let Some(sgr) = rest.strip_prefix("\x1b[") {
                let end = sgr.find(|c: char| c.is_ascii_alphabetic());
                if let Some(end) = end.filter(|end| sgr.as_bytes()[*end] == b'm') {
                    let parameters: Vec<_> = sgr[..end].split(';').collect();
                    let mut decoded = Vec::new();
                    let mut i = 0;
                    while i < parameters.len() {
                        if matches!(parameters[i], "38" | "48")
                            && parameters.get(i + 1) == Some(&"2")
                            && i + 4 < parameters.len()
                        {
                            let rgb = parameters[i + 2..i + 5]
                                .iter()
                                .map(|s| s.parse::<u8>())
                                .collect::<Result<Vec<_>, _>>();
                            if let Ok(rgb) = rgb {
                                let token = (u32::from(rgb[0]) << 16)
                                    | (u32::from(rgb[1]) << 8)
                                    | u32::from(rgb[2]);
                                if let Some(color) = token
                                    .checked_sub(COLOR_BASE)
                                    .and_then(|index| self.colors.get(index as usize))
                                {
                                    decoded.push(color_parameters(*color, parameters[i] == "38"));
                                    i += 5;
                                    continue;
                                }
                            }
                        }
                        decoded.push(parameters[i].to_string());
                        i += 1;
                    }
                    output.push_str("\x1b[");
                    output.push_str(&decoded.join(";"));
                    output.push('m');
                    rest = &sgr[end + 1..];
                    continue;
                }
            }
            let ch = rest.chars().next().expect("nonempty text");
            if let Some((glyph, _)) = self.glyphs.get(&ch) {
                output.push_str(glyph);
            } else {
                output.push(ch);
            }
            rest = &rest[ch.len_utf8()..];
        }
        Ok(output.into_bytes())
    }
}
fn color_parameters(color: Color, foreground: bool) -> String {
    let kind = if foreground { 38 } else { 48 };
    let index = match color {
        Color::Reset => return if foreground { "39" } else { "49" }.into(),
        Color::Indexed(index) => return format!("{kind};5;{index}"),
        Color::Rgb(r, g, b) => return format!("{kind};2;{r};{g};{b}"),
        Color::Black => 0,
        Color::Red => 1,
        Color::Green => 2,
        Color::Yellow => 3,
        Color::Blue => 4,
        Color::Magenta => 5,
        Color::Cyan => 6,
        Color::Gray => 7,
        Color::DarkGray => 8,
        Color::LightRed => 9,
        Color::LightGreen => 10,
        Color::LightYellow => 11,
        Color::LightBlue => 12,
        Color::LightMagenta => 13,
        Color::LightCyan => 14,
        Color::White => 15,
    };
    format!("{kind};5;{index}")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_retirement_reuses_live_graphemes_and_reclaims_only_after_commit() -> io::Result<()> {
        let mut codec = Codec::default();
        codec.resize(Size::new(8, 2));
        let mut source = Cell::default();
        source.set_symbol("e\u{301}");
        let token = codec.cell(0, 0, &source)?.0.ch;
        assert_eq!(token, codec.cell(1, 0, &source)?.0.ch);
        codec.begin_frame();
        assert_eq!(
            codec.decode(token.to_string().as_bytes())?,
            "e\u{301}".as_bytes()
        );
        assert_eq!(token, codec.cell(0, 0, &source)?.0.ch);
        codec.committed();
        assert_eq!(codec.glyphs[&token].1, 1);
        for n in 0..1024 {
            codec.begin_frame();
            source.set_symbol(&format!("界\u{301}{n}"));
            codec.cell(1, 0, &source)?;
            codec.committed();
            assert_eq!(codec.glyphs.len(), 1);
        }
        codec.begin_frame();
        codec.committed();
        assert!(codec.glyphs.is_empty());
        Ok(())
    }
    #[test]
    fn retired_graphemes_are_reclaimed_after_commit_and_live_cells_keep_shared_tokens(
    ) -> io::Result<()> {
        let mut codec = Codec::default();
        codec.resize(Size::new(16, 2));
        for n in 0..4096 {
            let mut cell = Cell::default();
            cell.set_symbol(&format!("e\u{301}{n}"));
            codec.cell(0, 0, &cell)?;
            codec.committed();
            assert_eq!(codec.glyphs.len(), 1);
            assert_eq!(codec.symbols.len(), 1);
            assert!(codec.next <= 2);
        }
        let mut cell = Cell::default();
        cell.set_symbol("e\u{301}");
        let a = codec.cell(0, 0, &cell)?.0.ch;
        let b = codec.cell(2, 0, &cell)?.0.ch;
        codec.committed();
        assert_eq!(a, b);
        assert_eq!(codec.glyphs[&a].1, 2);
        codec.cell(0, 0, &Cell::default())?;
        codec.committed();
        assert_eq!(codec.glyphs[&a].1, 1);
        codec.cell(2, 0, &Cell::default())?;
        codec.committed();
        assert!(codec.glyphs.is_empty());
        Ok(())
    }
}
