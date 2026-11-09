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
    pub(super) output_colors: crate::terminal::colors::OutputColors,
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
        // Transport resets attributes after every write; track only this batch.
        let mut output_fg = Color::Reset;
        let mut output_bg = Color::Reset;
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
                                    let color = *color;
                                    if parameters[i] == "38" {
                                        output_fg = color;
                                        decoded
                                            .push(self.output_colors.foreground(color, output_bg));
                                    } else {
                                        output_bg = color;
                                        decoded.push(self.output_colors.parameters(color, false));
                                        // Reevaluate foreground when console background
                                        // quantization changes contrast. Extended output
                                        // deliberately keeps its exact existing bytes.
                                        if self.output_colors.is_console() {
                                            decoded.push(
                                                self.output_colors.foreground(output_fg, color),
                                            );
                                        }
                                    }
                                    i += 5;
                                    continue;
                                }
                            }
                        }
                        match parameters[i] {
                            "0" | "" => {
                                output_fg = Color::Reset;
                                output_bg = Color::Reset;
                            }
                            "39" => output_fg = Color::Reset,
                            "49" => {
                                output_bg = Color::Reset;
                                if self.output_colors.is_console() {
                                    decoded.push("49".into());
                                    decoded.push(
                                        self.output_colors.foreground(output_fg, Color::Reset),
                                    );
                                    i += 1;
                                    continue;
                                }
                            }
                            _ => {}
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn console_output_converts_colors_without_changing_cells_or_unicode() -> io::Result<()> {
        let mut codec = Codec::default();
        codec.resize(Size::new(8, 2));
        let mut cell = Cell::default();
        cell.set_symbol("界\u{301}");
        cell.set_style(crate::palette::PaletteProfile::ClassicDn.dos(0x70));
        let glyph = codec.cell(0, 0, &cell)?.0.ch;
        let fg = codec.color(Color::Indexed(16))?;
        let bg = codec.color(Color::Indexed(248))?;
        let parameter = |color: TvColor| match color {
            TvColor::Rgb { r, g, b } => format!("{r};{g};{b}"),
            _ => panic!("expected color token"),
        };
        let encoded = format!(
            "\x1b[38;2;{};48;2;{};1m{glyph}\x1b[39;49;0m",
            parameter(fg),
            parameter(bg)
        );
        let extended = codec.decode(encoded.as_bytes())?;
        assert_eq!(
            extended,
            "\x1b[38;5;16;48;5;248;1m界\u{301}\x1b[39;49;0m".as_bytes()
        );
        codec.output_colors = crate::terminal::colors::OutputColors::linux_console();
        for _ in 0..4 {
            assert_eq!(
                codec.decode(encoded.as_bytes())?,
                "\x1b[30;47;30;1m界\u{301}\x1b[39;49;39;0m".as_bytes()
            );
        }
        codec.output_colors = Default::default();
        assert_eq!(codec.decode(encoded.as_bytes())?, extended);
        codec.output_colors = crate::terminal::colors::OutputColors::linux_console();
        let fg = parameter(codec.color(Color::Indexed(248))?);
        let bg = parameter(codec.color(Color::Indexed(240))?);
        for encoded in [
            format!("\x1b[38;2;{fg};48;2;{bg}m{glyph}\x1b[49m{glyph}"),
            format!("\x1b[48;2;{bg};38;2;{fg}m{glyph}\x1b[49m{glyph}"),
        ] {
            let output = String::from_utf8(codec.decode(encoded.as_bytes())?).unwrap();
            assert!(output.contains(";97m界\u{301}"), "{output:?}");
            assert!(output.ends_with("\x1b[49;37m界\u{301}"));
        }
        Ok(())
    }
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
