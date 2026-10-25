//! Native cell composition shared by documents and file panes.
use crate::palette::PaletteProfile;
use std::{collections::BTreeMap, io};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Clone)]
pub(crate) struct Glyph {
    pub(crate) text: String,
    pub(crate) attribute: u8,
}
#[derive(Clone)]
pub(crate) struct Scene {
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) profile: PaletteProfile,
    pub(crate) cells: BTreeMap<(u16, u16), Glyph>,
    pub(crate) cursor: Option<(u16, u16)>,
    pub(crate) underlay: bool,
}
// A control marker carries inherited style through unported overlays without
// copying native text into the old widget canvas. It is resolved before output.
pub(super) const UNDERLAY: &str = "\u{1}";
impl Scene {
    pub(crate) fn seed_overlay_styles(&self, buffer: &mut ratatui::buffer::Buffer) {
        for (&(x, y), glyph) in &self.cells {
            if let Some(cell) = buffer.cell_mut((x, y)) {
                cell.set_symbol(UNDERLAY)
                    .set_style(self.profile.dos(glyph.attribute));
            }
        }
    }
    pub(super) fn overlay_cell(
        &self,
        x: u16,
        y: u16,
        source: &ratatui::buffer::Cell,
    ) -> Option<ratatui::buffer::Cell> {
        if source.symbol() != UNDERLAY {
            return Some(source.clone());
        }
        let glyph = self.cells.get(&(x, y))?;
        let style = self.profile.dos(glyph.attribute);
        if Some(source.fg) == style.fg && Some(source.bg) == style.bg && source.modifier.is_empty()
        {
            return None;
        }
        let mut cell = source.clone();
        cell.set_symbol(if glyph.text.is_empty() {
            "\0"
        } else {
            &glyph.text
        });
        Some(cell)
    }

    pub(crate) fn put(&mut self, x: u16, y: u16, text: &str, attribute: u8) {
        if x < self.width && y < self.height {
            self.cells.insert(
                (x, y),
                Glyph {
                    text: text.into(),
                    attribute,
                },
            );
        }
    }
    pub(crate) fn fill(&mut self, x: u16, y: u16, width: u16, height: u16, attribute: u8) {
        for row in y..(y + height).min(self.height) {
            for col in x..(x + width).min(self.width) {
                self.put(col, row, " ", attribute);
            }
        }
    }
    pub(crate) fn text(&mut self, x: u16, y: u16, width: u16, text: &str, attribute: u8) {
        let mut col = x;
        let end = x.saturating_add(width).min(self.width);
        for grapheme in text.graphemes(true) {
            let w = grapheme.width() as u16;
            if w == 0 {
                continue;
            }
            if col + w > end {
                break;
            }
            self.put(col, y, grapheme, attribute);
            for offset in 1..w {
                self.put(col + offset, y, "", attribute);
            }
            col += w;
        }
    }
    pub(crate) fn border(
        &mut self,
        x: u16,
        y: u16,
        width: u16,
        height: u16,
        double: bool,
        attr: u8,
    ) {
        let chars = if double {
            ["╔", "═", "╗", "║", "╚", "╝"]
        } else {
            ["┌", "─", "┐", "│", "└", "┘"]
        };
        for col in x + 1..x + width - 1 {
            self.put(col, y, chars[1], attr);
            self.put(col, y + height - 1, chars[1], attr);
        }
        for row in y + 1..y + height - 1 {
            self.put(x, row, chars[3], attr);
            self.put(x + width - 1, row, chars[3], attr);
        }
        for (col, row, ch) in [
            (x, y, chars[0]),
            (x + width - 1, y, chars[2]),
            (x, y + height - 1, chars[4]),
            (x + width - 1, y + height - 1, chars[5]),
        ] {
            self.put(col, row, ch, attr);
        }
    }
    pub(crate) fn title(&mut self, x: u16, y: u16, width: u16, label: &str) {
        let w = label.width().min(usize::from(width - 2)) as u16;
        self.text(x + (width - w) / 2, y, w, label, 0x7f);
    }
    pub(super) fn encode(
        &self,
        codec: &mut super::codec::Codec,
        width: u16,
        height: u16,
    ) -> io::Result<Vec<(u16, u16, turbo_vision::core::draw::Cell)>> {
        let mut output = Vec::with_capacity(self.cells.len());
        for (&(x, y), glyph) in &self.cells {
            // A terminal resize can land between frame preparation and painting.
            if x >= width || y >= height || glyph.text.width() > usize::from(width - x) {
                continue;
            }
            // Only the transport boundary uses legacy cell/color types.
            let mut source = ratatui::buffer::Cell::default();
            source.set_symbol(if glyph.text.is_empty() {
                " "
            } else {
                &glyph.text
            });
            source.set_style(self.profile.dos(glyph.attribute));
            let (mut cell, _) = codec.native_cell(x, y, &source)?;
            if glyph.text.is_empty() {
                cell.ch = '\0';
            }
            output.push((x, y, cell));
        }
        Ok(output)
    }
}
