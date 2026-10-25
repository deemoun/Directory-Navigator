//! Transitional Turbo Vision output host. DN models and the tested canvas remain
//! intact while native window/control adapters are migrated separately.
mod codec;
pub mod desktop;
pub mod forms;
use codec::Codec;
use ratatui::{
    backend::{Backend, WindowSize},
    buffer::Cell,
    layout::{Position, Size},
};
use std::{
    io::{self, Write},
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};
use turbo_vision::{
    core::{draw::Cell as TvCell, event::Event, geometry::Rect as TvRect},
    terminal::{Backend as TvBackend, Terminal as TvTerminal},
};
fn lock<T>(value: &Mutex<T>) -> io::Result<MutexGuard<'_, T>> {
    value
        .lock()
        .map_err(|_| io::Error::other("Turbo Vision transport lock poisoned"))
}
struct Transport<W> {
    writer: Arc<Mutex<W>>,
    codec: Arc<Mutex<Codec>>,
    dimensions: Arc<Mutex<Size>>,
    input: Arc<Mutex<Option<crossterm::event::Event>>>,
}
impl<W: Write + Send + 'static> TvBackend for Transport<W> {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    // Session is the sole owner of raw mode, mouse capture, paste and restoration.
    fn init(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn cleanup(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn size(&self) -> io::Result<(u16, u16)> {
        let s = *lock(&self.dimensions)?;
        Ok((s.width, s.height))
    }
    fn poll_event(&mut self, timeout: Duration) -> io::Result<Option<Event>> {
        if crossterm::event::poll(timeout)? {
            *lock(&self.input)? = Some(crossterm::event::read()?);
            // Keep the original event intact: shortcut modifiers, paste payloads,
            // key releases and resize dimensions are DN's established protocol.
            Ok(Some(Event::command(0xff00)))
        } else {
            Ok(None)
        }
    }
    fn write_raw(&mut self, bytes: &[u8]) -> io::Result<()> {
        let decoded = lock(&self.codec)?.decode(bytes)?;
        let mut writer = lock(&self.writer)?;
        writer.write_all(&decoded)?;
        writer.write_all(b"\x1b[39m\x1b[49m\x1b[0m")
    }
    fn flush(&mut self) -> io::Result<()> {
        lock(&self.writer)?.flush()
    }
    fn show_cursor(&mut self, x: u16, y: u16) -> io::Result<()> {
        write!(lock(&self.writer)?, "\x1b[{};{}H\x1b[?25h", y + 1, x + 1)
    }
    fn hide_cursor(&mut self) -> io::Result<()> {
        lock(&self.writer)?.write_all(b"\x1b[?25l")
    }
}
pub struct TurboBackend<W: Write + Send + 'static> {
    terminal: TvTerminal,
    desktop: desktop::NativeDesktop,
    pixels: Vec<TvCell>,
    codec: Arc<Mutex<Codec>>,
    dimensions: Arc<Mutex<Size>>,
    writer: Arc<Mutex<W>>,
    input: Arc<Mutex<Option<crossterm::event::Event>>>,
    cursor: Position,
    cursor_visible: bool,
    fixed: bool,
}
impl<W: Write + Send + 'static> TurboBackend<W> {
    pub fn new(writer: W) -> io::Result<Self> {
        let (w, h) = crossterm::terminal::size()?;
        Self::create(writer, Size::new(w, h), false)
    }
    /// An offscreen transport for deterministic renderer and Unicode tests.
    pub fn with_size(writer: W, size: Size) -> io::Result<Self> {
        Self::create(writer, size, true)
    }
    fn create(writer: W, size: Size, fixed: bool) -> io::Result<Self> {
        validate(size)?;
        let writer = Arc::new(Mutex::new(writer));
        let codec = Arc::new(Mutex::new(Codec::default()));
        let dimensions = Arc::new(Mutex::new(size));
        lock(&codec)?.resize(size);
        let input = Arc::new(Mutex::new(None));
        let terminal = TvTerminal::with_backend(Box::new(Transport {
            writer: writer.clone(),
            codec: codec.clone(),
            dimensions: dimensions.clone(),
            input: input.clone(),
        }))
        .map_err(|e| io::Error::other(e.to_string()))?;
        Ok(Self {
            terminal,
            desktop: desktop::NativeDesktop::new(crate::desktop::WindowInfo {
                id: crate::desktop::WindowId::MANAGER,
                kind: crate::desktop::WindowKind::Manager,
            }),
            pixels: vec![
                TvCell::new(' ', turbo_vision::core::palette::Attr::from_u8(0x07));
                usize::from(size.width) * usize::from(size.height)
            ],
            codec,
            dimensions,
            writer,
            input,
            cursor: Position::ORIGIN,
            cursor_visible: false,
            fixed,
        })
    }
    pub fn bind_desktop(&mut self, desktop: desktop::NativeDesktop) {
        self.desktop = desktop;
    }
    /// One event reader, through TV's host backend, with lossless DN payloads.
    pub fn poll_event(&mut self, timeout: Duration) -> io::Result<Option<crossterm::event::Event>> {
        if self.fixed {
            return Ok(None);
        }
        let _ = self.terminal.poll_event(timeout)?;
        Ok(lock(&self.input)?.take())
    }
    pub fn resize(&mut self, size: Size) -> io::Result<()> {
        validate(size)?;
        if *lock(&self.dimensions)? != size {
            *lock(&self.dimensions)? = size;
            lock(&self.codec)?.resize(size);
            self.terminal.resize(size.width, size.height);
            self.pixels = vec![
                TvCell::new(' ', turbo_vision::core::palette::Attr::from_u8(0x07));
                usize::from(size.width) * usize::from(size.height)
            ];
        }
        Ok(())
    }
    fn current_size(&self) -> io::Result<Size> {
        if self.fixed {
            Ok(*lock(&self.dimensions)?)
        } else {
            let (w, h) = crossterm::terminal::size()?;
            let size = Size::new(w, h);
            validate(size)?;
            Ok(size)
        }
    }
}
fn validate(size: Size) -> io::Result<()> {
    if size.width > i16::MAX as u16 || size.height > i16::MAX as u16 {
        Err(io::Error::other(
            "Terminal exceeds Turbo Vision coordinate range",
        ))
    } else {
        Ok(())
    }
}
impl<W: Write + Send + 'static> Backend for TurboBackend<W> {
    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        let size = self.current_size()?;
        self.resize(size)?;
        {
            let mut codec = lock(&self.codec)?;
            for (x, y, cell) in content {
                if x >= size.width || y >= size.height {
                    continue;
                }
                let (cell, width) = codec.cell(x, y, cell)?;
                self.pixels[usize::from(y) * usize::from(size.width) + usize::from(x)] = cell;
                for offset in 1..width.min(usize::from(size.width - x)) {
                    let px = x + offset as u16;
                    codec.continuation(px, y);
                    self.pixels[usize::from(y) * usize::from(size.width) + usize::from(px)] =
                        TvCell::new('\0', cell.attr);
                }
            }
        }
        self.desktop.draw(
            &mut self.terminal,
            TvRect::new(0, 0, size.width as i16, size.height as i16),
            &self.pixels,
        );
        Ok(())
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
        self.terminal.clear();
        self.terminal.force_full_redraw();
        self.pixels.fill(TvCell::new(
            ' ',
            turbo_vision::core::palette::Attr::from_u8(0x07),
        ));
        lock(&self.codec)?.resize(*lock(&self.dimensions)?);
        lock(&self.writer)?.write_all(b"\x1b[2J")
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
        if self.cursor_visible {
            self.terminal
                .show_cursor(self.cursor.x as i16, self.cursor.y as i16)?;
        } else {
            self.terminal.hide_cursor()?;
        }
        self.terminal.flush()?;
        lock(&self.codec)?.committed();
        Ok(())
    }
}
