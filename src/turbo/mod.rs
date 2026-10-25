//! Turbo Vision output host. Runtime scenes paint directly through native views;
//! reference frames retain a separate cell transport for fallback/parity checks.
mod codec;
pub mod desktop;
pub mod forms;
pub(crate) mod manager;
mod mouse;
pub(crate) mod panes;
pub(crate) mod scene;
pub(crate) mod tetris;
use crate::presentation::layout::{Position, Size};
use codec::Cell;
use codec::Codec;
mod reference;
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
    canvas: Vec<Cell>,
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
            pixels: Vec::new(),
            canvas: Vec::new(),
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
            self.canvas.clear();
            self.pixels.clear();
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
impl<W: Write + Send + 'static> TurboBackend<W> {
    /// The production frame never constructs a Ratatui Terminal, Frame or Buffer.
    pub fn draw_app(&mut self, app: &mut crate::app::App) -> io::Result<()> {
        let size = self.current_size()?;
        self.resize(size)?;
        let scene = crate::ui::native_scene(
            app,
            crate::presentation::layout::Rect::new(0, 0, size.width, size.height),
        );
        self.cursor_visible = scene.cursor.is_some();
        if let Some((x, y)) = scene.cursor {
            self.cursor = Position::new(x, y);
        }
        self.desktop.set_scene(Some(scene));
        self.paint()?;
        self.flush()
    }
    fn paint(&mut self) -> io::Result<()> {
        let size = *lock(&self.dimensions)?;
        let scene = self.desktop.scene();
        {
            let mut codec = lock(&self.codec)?;
            for y in 0..if self.canvas.is_empty() {
                0
            } else {
                size.height
            } {
                for x in 0..size.width {
                    let i = usize::from(y) * usize::from(size.width) + usize::from(x);
                    let source = &self.canvas[i];
                    self.pixels[i] = codec.cell(x, y, source)?.0;
                }
            }
        }
        let native = {
            let mut codec = lock(&self.codec)?;
            codec.clear_native();
            scene
                .as_ref()
                .map(|scene| scene.encode(&mut codec, size.width, size.height))
                .transpose()?
        };
        self.desktop.draw(
            &mut self.terminal,
            TvRect::new(0, 0, size.width as i16, size.height as i16),
            &self.pixels,
            native.as_deref().unwrap_or_default(),
        );
        Ok(())
    }
    pub fn clear(&mut self) -> io::Result<()> {
        self.terminal.clear();
        self.terminal.force_full_redraw();
        self.canvas.fill(Cell::default());
        self.pixels.fill(TvCell::new(
            ' ',
            turbo_vision::core::palette::Attr::from_u8(0x07),
        ));
        lock(&self.codec)?.resize(*lock(&self.dimensions)?);
        lock(&self.writer)?.write_all(b"\x1b[2J")
    }
    pub fn flush(&mut self) -> io::Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn production_frames_resize_and_resume_without_reference_buffers() -> io::Result<()> {
        let mut app = crate::app::App::new(std::env::current_dir()?)?;
        let mut host = TurboBackend::with_size(io::sink(), Size::new(80, 25))?;
        host.bind_desktop(app.desktop.native());
        for size in [
            Size::new(80, 25),
            Size::new(132, 40),
            Size::new(50, 12),
            Size::new(80, 25),
        ] {
            host.resize(size)?;
            host.draw_app(&mut app)?;
            assert!(host.canvas.is_empty());
            assert!(host.pixels.is_empty());
            assert!(host.desktop.scene().is_some());
            host.clear()?;
            host.draw_app(&mut app)?;
            assert!(host.canvas.is_empty());
        }
        Ok(())
    }
}
