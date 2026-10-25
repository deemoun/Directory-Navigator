//! Explicit Ratatui comparison/fallback adapter. Production TV frames never enter here.
use crate::painting::*;
use crate::presentation::layout::Rect;
pub trait ReferenceWidget {
    fn reference(self, frame: &mut ratatui::Frame<'_>, area: Rect);
}
impl Frame for ratatui::Frame<'_> {
    fn area(&self) -> Rect {
        self.area()
    }
    fn render_widget(&mut self, widget: impl Widget, area: Rect) {
        widget.reference(self, area);
    }
    fn set_cursor_position(&mut self, position: impl Into<ratatui::layout::Position>) {
        self.set_cursor_position(position);
    }
}

impl ReferenceWidget for Block<'_> {
    fn reference(self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        let mut block = ratatui::widgets::Block::default()
            .style(self.style)
            .border_style(self.border_style)
            .title_style(self.title_style)
            .borders(reference_borders(self.borders))
            .border_type(match self.border_type {
                BorderType::Plain => ratatui::widgets::BorderType::Plain,
                BorderType::Double => ratatui::widgets::BorderType::Double,
            })
            .title_alignment(self.alignment);
        if let Some(title) = self.title {
            block = block.title(title);
        }
        frame.render_widget(block, area);
    }
}
impl ReferenceWidget for Paragraph<'_> {
    fn reference(self, f: &mut ratatui::Frame<'_>, r: Rect) {
        let mut p = ratatui::widgets::Paragraph::new(self.text)
            .style(self.style)
            .alignment(self.alignment)
            .scroll(self.scroll);
        if let Some(wrap) = self.wrap {
            p = p.wrap(ratatui::widgets::Wrap { trim: wrap.trim });
        }
        f.render_widget(p, r);
    }
}
impl ReferenceWidget for Clear {
    fn reference(self, f: &mut ratatui::Frame<'_>, r: Rect) {
        f.render_widget(ratatui::widgets::Clear, r);
    }
}
impl ReferenceWidget for Table<'_> {
    fn reference(self, f: &mut ratatui::Frame<'_>, r: Rect) {
        let rows = self.rows.into_iter().map(|row| {
            ratatui::widgets::Row::new(
                row.cells
                    .into_iter()
                    .map(|c| ratatui::widgets::Cell::new(c.text).style(c.style)),
            )
            .style(row.style)
        });
        f.render_widget(
            ratatui::widgets::Table::new(rows, self.widths)
                .style(self.style)
                .column_spacing(self.spacing),
            r,
        );
    }
}

#[cfg(feature = "turbo-vision")]
pub fn draw_native(frame: &mut ratatui::Frame<'_>, app: &mut crate::app::App) {
    let scene = crate::ui::native_scene(app, frame.area());
    if let Some(cursor) = scene.cursor {
        frame.set_cursor_position(cursor);
    }
    app.desktop.native().set_scene(Some(scene));
}

fn reference_borders(borders: Borders) -> ratatui::widgets::Borders {
    let mut result = ratatui::widgets::Borders::empty();
    for (native, reference) in [
        (Borders::LEFT, ratatui::widgets::Borders::LEFT),
        (Borders::RIGHT, ratatui::widgets::Borders::RIGHT),
        (Borders::TOP, ratatui::widgets::Borders::TOP),
        (Borders::BOTTOM, ratatui::widgets::Borders::BOTTOM),
    ] {
        if borders.contains(native) {
            result.insert(reference);
        }
    }
    result
}
