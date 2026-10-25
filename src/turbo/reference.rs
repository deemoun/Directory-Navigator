//! Reference transport adapter, never used by the production event loop.
use super::*;
use ratatui::backend::{Backend, WindowSize};
impl<W: Write + Send + 'static> Backend for TurboBackend<W> {
    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a ratatui::buffer::Cell)>,
    {
        let size = self.current_size()?;
        self.resize(size)?;
        let len = usize::from(size.width) * usize::from(size.height);
        if self.canvas.len() != len {
            self.canvas = vec![Cell::default(); len];
            self.pixels =
                vec![TvCell::new(' ', turbo_vision::core::palette::Attr::from_u8(0x07)); len];
        }
        // Reference frames retain their own cells; native frames are independent
        // of this buffer and paint through the document's native scene.
        for (x, y, cell) in content {
            if x >= size.width || y >= size.height {
                continue;
            }
            self.canvas[usize::from(y) * usize::from(size.width) + usize::from(x)] =
                Cell::from_reference(cell);
            let width = crate::presentation::text::Span::raw(cell.symbol())
                .width()
                .max(1);
            for offset in 1..width.min(usize::from(size.width - x)) {
                let mut padding = Cell::from_reference(cell);
                padding.set_symbol("\0");
                self.canvas[usize::from(y) * usize::from(size.width) + usize::from(x) + offset] =
                    padding;
            }
        }
        self.paint()
    }
    fn hide_cursor(&mut self) -> io::Result<()> {
        self.cursor_visible = false;
        Ok(())
    }
    fn show_cursor(&mut self) -> io::Result<()> {
        self.cursor_visible = true;
        Ok(())
    }
    fn get_cursor_position(&mut self) -> io::Result<Position> {
        Ok(self.cursor)
    }
    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
        self.cursor = position.into();
        self.show_cursor()
    }
    fn clear(&mut self) -> io::Result<()> {
        TurboBackend::clear(self)
    }
    fn size(&self) -> io::Result<Size> {
        self.current_size()
    }
    fn window_size(&mut self) -> io::Result<WindowSize> {
        Ok(WindowSize {
            columns_rows: self.current_size()?,
            pixels: Size::ZERO,
        })
    }
    fn flush(&mut self) -> io::Result<()> {
        TurboBackend::flush(self)
    }
}

impl Cell {
    fn from_reference(source: &ratatui::buffer::Cell) -> Self {
        let mut cell = Cell::default();
        cell.set_symbol(source.symbol());
        cell.set_style(source.style());
        cell
    }
}
