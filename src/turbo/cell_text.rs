//! Display-only native text surface. DN owns scrolling and encodes each terminal
//! cell before drawing; UTF-8 byte offsets must never clip those cell tokens.
use turbo_vision::{
    core::{
        draw::DrawBuffer,
        geometry::Rect,
        palette::{palettes, Palette},
    },
    terminal::Terminal,
    views::{view::write_line_to_terminal, View, ViewCore},
};
pub(super) struct CellText {
    core: ViewCore,
    lines: Vec<String>,
}
impl CellText {
    pub(super) fn new(lines: Vec<String>) -> Self {
        Self {
            core: ViewCore::new(Rect::new(0, 0, 1, 1)),
            lines,
        }
    }
}
impl View for CellText {
    fn core(&self) -> &ViewCore {
        &self.core
    }
    fn core_mut(&mut self) -> &mut ViewCore {
        &mut self.core
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn get_palette(&self) -> Option<Palette> {
        Some(Palette::from_slice(palettes::CP_SCROLLER))
    }
    fn handle_event(&mut self, _: &mut turbo_vision::core::event::Event) {}
    fn draw(&mut self, terminal: &mut Terminal) {
        let width = self.bounds().width_clamped() as usize;
        let height = self.bounds().height_clamped() as usize;
        let color = self.map_color(1);
        for row in 0..height {
            let mut buffer = DrawBuffer::new(width);
            buffer.move_char(0, ' ', color, width);
            if let Some(line) = self.lines.get(row) {
                // TextCells has already expanded wide graphemes into one head
                // and padding tokens. Each scalar is exactly one native cell.
                for (column, token) in line.chars().take(width).enumerate() {
                    buffer.move_char(column, token, color, 1);
                }
            }
            write_line_to_terminal(terminal, 0, row as i16, &buffer);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{scene::Scene, widgets};
    use crate::{palette::PaletteProfile, presentation::layout::Rect};
    use std::collections::BTreeMap;
    use unicode_width::UnicodeWidthStr;
    fn render(text: &str, width: u16, horizontal: usize) -> Scene {
        let mut scene = Scene {
            width: 80,
            height: 4,
            profile: PaletteProfile::ClassicDn,
            cells: BTreeMap::new(),
            regions: Vec::new(),
            cursor: None,
        };
        widgets::paint_into(
            &mut scene,
            widgets::text_view(&[text.into()], horizontal, 0x70),
            Rect::new(2, 1, width, 2),
        );
        scene
    }
    fn row(scene: &Scene, width: u16) -> String {
        (2..2 + width)
            .map(|x| scene.cells.get(&(x, 1)).map_or(" ", |c| c.text.as_str()))
            .collect()
    }
    #[test]
    fn full_numeric_suffix_survives_unicode_prefixes_and_exact_cell_width() {
        for prefix in ["ASCII", "Победитель", "界", "e\u{301}", "👩‍💻", "\u{f0001}"]
        {
            let text = format!("{prefix} 18446744073709551615");
            let width = text.width() as u16;
            let scene = render(&text, width, 0);
            assert_eq!(row(&scene, width), text);
            assert_eq!(scene.cells[&(2 + width - 1, 1)].text, "5");
            let style = scene.cells[&(2, 2)].resolved(PaletteProfile::ClassicDn);
            assert_eq!(style.fg, Some(PaletteProfile::ClassicDn.color(0)));
            assert_eq!(style.bg, Some(PaletteProfile::ClassicDn.color(7)));
        }
    }
    #[test]
    fn clipping_and_panning_preserve_graphemes_and_neighbor_cells() {
        let scene = render("A界e\u{301}B", 5, 0);
        assert_eq!(row(&scene, 5), "A界e\u{301}B");
        let scene = render("A界e\u{301}B", 3, 2);
        assert_eq!(row(&scene, 3), "e\u{301}B ");
        for width in 0..=5 {
            let scene = render("界e\u{301}\u{f0001}text", width, 0);
            assert!(row(&scene, width).width() <= usize::from(width));
            assert!(!scene.cells.contains_key(&(2 + width, 1)));
        }
    }
}
