//! Character grid backed by Turbo Vision's reusable Table control.
//! The showcase's AsciiTable is a display-only example, not a library widget.
//! DN supplies its byte mappings and skin; Table draws and navigates the cells.
use super::scene::Scene;
use crate::{
    mouse::Target,
    palette::PaletteProfile,
    presentation::layout::{Rect, Size},
    utilities::{characters::Chart, Action},
};
use crossterm::event::KeyCode;
use std::{collections::BTreeMap, io, time::Duration};
use turbo_vision::{
    core::{event::*, geometry::Point, geometry::Rect as TvRect},
    terminal::{Backend, Terminal},
    views::{table::Column, table::Table, View},
};

const COLUMNS: usize = 16;
const CELL_WIDTH: u16 = 4;

fn table(chart: &Chart) -> Table {
    let rows = usize::from(chart.limit()) / COLUMNS + 1;
    let mut table = Table::new(TvRect::new(0, 0, 64, rows as i16), 0);
    table.set_show_header(false);
    // Table reserves one cell between columns; together these retain DN's
    // four-cell hit areas and the existing 16-by-16 arrangement.
    table.set_columns(
        (0..COLUMNS)
            .map(|_| Column::new("", CELL_WIDTH - 1))
            .collect(),
    );
    table.set_rows(
        (0..rows)
            .map(|row| {
                (0..COLUMNS)
                    .map(|col| {
                        let c = chart.mapped((row * COLUMNS + col) as u8);
                        format!(" {} ", if c.is_control() { '·' } else { c })
                    })
                    .collect()
            })
            .collect(),
    );
    table.set_selected_row(usize::from(chart.selected()) / COLUMNS);
    table.set_selected_col(usize::from(chart.selected()) % COLUMNS);
    table.set_focus(true);
    table
}

fn selection(table: &Table) -> u8 {
    (table.selected_row().unwrap_or(0) * COLUMNS + table.selected_col()) as u8
}

pub(crate) fn key(chart: &mut Chart, code: KeyCode) {
    let mut table = table(chart);
    let code = match code {
        KeyCode::Left => KB_LEFT,
        KeyCode::Right => KB_RIGHT,
        KeyCode::Up => KB_UP,
        KeyCode::Down => KB_DOWN,
        KeyCode::PageUp => KB_PGUP,
        KeyCode::PageDown => KB_PGDN,
        KeyCode::Home => {
            table.set_selected_col(0);
            KB_HOME
        }
        KeyCode::End => {
            table.set_selected_col(COLUMNS - 1);
            KB_END
        }
        _ => return,
    };
    table.handle_event(&mut Event::keyboard(code));
    chart.select(selection(&table));
}

pub(crate) fn select(chart: &mut Chart, byte: u8) {
    let mut table = table(chart);
    let byte = usize::from(byte.min(chart.limit()));
    table.handle_event(&mut Event::mouse(
        EventType::MouseDown,
        Point::new(
            (byte % COLUMNS * usize::from(CELL_WIDTH)) as i16,
            (byte / COLUMNS) as i16,
        ),
        MB_LEFT_BUTTON,
        false,
    ));
    chart.select(selection(&table));
}

/// Paint the library control into native cells without owning terminal I/O.
/// The resulting scene joins the same immutable frame as its chrome and hits.
pub(crate) fn draw(chart: &Chart, origin: Rect, screen: Size, profile: PaletteProfile) -> Scene {
    let mut table = table(chart);
    let rows = table.row_count() as u16;
    let mut terminal = Terminal::with_backend(Box::new(Surface(64, rows)))
        .expect("in-memory Turbo Vision surface");
    terminal.draw_view(&mut table);
    let mut scene = Scene {
        width: screen.width,
        height: screen.height,
        profile,
        cells: BTreeMap::new(),
        regions: Vec::new(),
        cursor: None,
    };
    let selected_row = table.selected_row().unwrap_or(0);
    let selected_col = table.selected_col();
    for (row, cells) in terminal.buffer().iter().enumerate() {
        for (col, cell) in cells.iter().enumerate() {
            // Keep DN's single-cell highlight rather than Table's full-row bar.
            // Glyphs and column layout still come from the native Table painter.
            let attr = if row == selected_row && col / usize::from(CELL_WIDTH) == selected_col {
                0x30
            } else {
                0x1f
            };
            scene.put(
                origin.x + col as u16,
                origin.y + row as u16,
                &cell.ch.to_string(),
                attr,
            );
        }
    }
    for row in 0..rows {
        for (col, (x, _)) in table.column_offsets().into_iter().enumerate() {
            scene.regions.push((
                Rect::new(origin.x + x as u16, origin.y + row, CELL_WIDTH, 1),
                Target::Utility(Action::Cell((usize::from(row) * COLUMNS + col) as u8)),
            ));
        }
    }
    scene
}

/// Scratch storage for native widget drawing. Session keeps exclusive ownership
/// of raw mode, input, the visible terminal and its restoration.
struct Surface(u16, u16);
impl Backend for Surface {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn init(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn cleanup(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn size(&self) -> io::Result<(u16, u16)> {
        Ok((self.0, self.1))
    }
    fn poll_event(&mut self, _: Duration) -> io::Result<Option<Event>> {
        Ok(None)
    }
    fn write_raw(&mut self, _: &[u8]) -> io::Result<()> {
        Ok(())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn show_cursor(&mut self, _: u16, _: u16) -> io::Result<()> {
        Ok(())
    }
    fn hide_cursor(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utilities::characters::Page;

    #[test]
    fn native_table_draws_every_byte_and_hit_region_with_the_dn_skin() {
        for page in [Page::Ascii, Page::Cp437] {
            let mut chart = Chart {
                page,
                ..Default::default()
            };
            chart.select(65);
            for profile in [PaletteProfile::ClassicDn, PaletteProfile::Terminal] {
                let scene = draw(&chart, Rect::new(7, 3, 64, 16), Size::new(80, 25), profile);
                assert_eq!(scene.regions.len(), usize::from(chart.limit()) + 1);
                for byte in 0..=chart.limit() {
                    let x = 7 + u16::from(byte % 16) * 4;
                    let y = 3 + u16::from(byte / 16);
                    let mapped = chart.mapped(byte);
                    let expected = if mapped.is_control() { '·' } else { mapped };
                    assert_eq!(
                        scene.cells[&(x + 1, y)].text,
                        expected.to_string(),
                        "{page:?}, byte {byte}"
                    );
                    assert_eq!(
                        scene.regions[usize::from(byte)],
                        (Rect::new(x, y, 4, 1), Target::Utility(Action::Cell(byte)))
                    );
                    for offset in 0..4 {
                        assert_eq!(
                            scene.cells[&(x + offset, y)].attribute,
                            if byte == 65 { 0x30 } else { 0x1f }
                        );
                    }
                }
                assert_eq!(chart.selected(), 65, "painting is read-only");
            }
        }
    }

    #[test]
    fn native_selection_keys_and_mouse_use_the_same_cell_coordinates() {
        for page in [Page::Ascii, Page::Cp437] {
            let mut chart = Chart {
                page,
                ..Default::default()
            };
            for byte in 0..=chart.limit() {
                select(&mut chart, byte);
                assert_eq!(chart.selected(), byte);
            }
            select(&mut chart, 65);
            key(&mut chart, KeyCode::Right);
            assert_eq!(chart.selected(), 66);
            key(&mut chart, KeyCode::Down);
            assert_eq!(chart.selected(), 82);
            key(&mut chart, KeyCode::Left);
            assert_eq!(chart.selected(), 81);
            key(&mut chart, KeyCode::Up);
            assert_eq!(chart.selected(), 65);
            key(&mut chart, KeyCode::Home);
            assert_eq!(chart.selected(), 0);
            key(&mut chart, KeyCode::Left);
            assert_eq!(chart.selected(), 0);
            key(&mut chart, KeyCode::End);
            assert_eq!(chart.selected(), chart.limit());
            key(&mut chart, KeyCode::Right);
            assert_eq!(chart.selected(), chart.limit());
            key(&mut chart, KeyCode::PageUp);
            assert_eq!(chart.selected(), 15);
            key(&mut chart, KeyCode::PageDown);
            assert_eq!(chart.selected(), chart.limit());
        }
    }
}
