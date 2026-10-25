//! Scratch native widget drawing. Session remains the sole terminal I/O owner.
use std::{io, time::Duration};
use turbo_vision::{
    core::event::Event,
    terminal::{Backend, Terminal},
};
pub(crate) fn terminal(width: u16, height: u16) -> Terminal {
    Terminal::with_backend(Box::new(Surface(width, height)))
        .expect("in-memory Turbo Vision surface")
}
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
