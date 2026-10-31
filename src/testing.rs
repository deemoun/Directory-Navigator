//! Native headless frame capture for integration tests. This is a snapshot sink,
//! not an alternate renderer: all painting uses the production NativeFrame.
pub use crate::presentation::{layout, style, text};
use crate::{
    painting::NativeFrame,
    presentation::{
        layout::{Position, Rect, Size},
        style::{Color, Modifier, Style},
    },
};
use std::{io, ops::Index};

/// A per-test clock for the production button-feedback path. It does not advance
/// filesystem workers or game time, and never changes a process-global clock.
pub struct InputClock(std::time::Instant);
impl Default for InputClock {
    fn default() -> Self {
        Self(std::time::Instant::now())
    }
}
impl InputClock {
    pub fn key(&self, app: &mut crate::app::App, key: crossterm::event::KeyEvent) {
        crate::input::handle_ui_at(app, key, self.0);
    }
    pub fn mouse(&self, app: &mut crate::app::App, event: crossterm::event::MouseEvent) {
        crate::mouse::handle_ui_at(app, event, self.0);
    }
    pub fn advance(&mut self, app: &mut crate::app::App, elapsed: std::time::Duration) -> bool {
        self.0 += elapsed;
        crate::button_feedback::poll(app, self.0)
    }
    pub fn busy(&self, app: &crate::app::App) -> bool {
        crate::button_feedback::busy(app)
    }
}

/// Metadata only: no buffer contents, paths, field values or pasted text.
pub fn diagnostic_state(app: &crate::app::App) -> String {
    use std::fmt::Write;
    let native = app.desktop.native();
    let mut state = format!(
        "active={:?} focus={:?} layout_revision={} viewport={:?}\nmodals={:?}\nbutton={}\ncapture={:?} z_order={:?}\n",
        app.desktop.active(), crate::focus::current(app), app.layout_revision,
        app.utilities.size, app.modal_infos(), crate::button_feedback::diagnostic(app),
        native.manipulated_surface(), native.z_order(),
    );
    for window in app.desktop.windows() {
        if let Some(s) = native.surface(crate::windowing::geometry::SurfaceKey::Document(window.id))
        {
            let _ = writeln!(
                state,
                "surface={:?} bounds={:?} maximized={}",
                s.id, s.bounds, s.maximized
            );
        }
    }
    if let Some(s) = native.top_surface() {
        let _ = writeln!(state, "top={:?} bounds={:?}", s.id, s.bounds);
    }
    state
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    symbol: String,
    pub fg: Color,
    pub bg: Color,
    pub modifier: Modifier,
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
    pub fn set_symbol(&mut self, s: &str) -> &mut Self {
        self.symbol = s.into();
        self
    }
    pub fn set_fg(&mut self, c: Color) -> &mut Self {
        self.fg = c;
        self
    }
    pub fn set_bg(&mut self, c: Color) -> &mut Self {
        self.bg = c;
        self
    }
    pub fn set_style(&mut self, s: Style) -> &mut Self {
        if let Some(c) = s.fg {
            self.fg = c
        }
        if let Some(c) = s.bg {
            self.bg = c
        }
        self.modifier.remove(s.sub_modifier);
        self.modifier.insert(s.add_modifier);
        self
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Buffer {
    pub area: Rect,
    pub content: Vec<Cell>,
}
impl Buffer {
    pub fn rendered(result: crate::ui::RenderedFrame) -> Self {
        let mut frame = NativeFrame::new(
            Rect::new(0, 0, result.scene.width, result.scene.height),
            result.scene.profile,
        );
        frame.replace(result.scene);
        Self::capture(frame).0
    }

    pub fn content(&self) -> &[Cell] {
        &self.content
    }
    pub fn empty(area: Rect) -> Self {
        Self {
            area,
            content: vec![Cell::default(); area.area() as usize],
        }
    }
    pub(crate) fn capture(frame: NativeFrame) -> (Self, Option<Position>) {
        let scene = frame.finish();
        let mut buffer = Self::empty(Rect::new(0, 0, scene.width, scene.height));
        // Continuations are hidden placeholders, not independent styled cells.
        for (&(x, y), g) in &scene.cells {
            if x < scene.width && y < scene.height && !g.text.is_empty() {
                let cell =
                    &mut buffer.content[usize::from(y) * usize::from(scene.width) + usize::from(x)];
                cell.set_symbol(if g.text.is_empty() { " " } else { &g.text })
                    .set_style(g.resolved(scene.profile));
            }
        }
        (buffer, scene.cursor.map(Into::into))
    }
}
impl Index<(u16, u16)> for Buffer {
    type Output = Cell;
    fn index(&self, (x, y): (u16, u16)) -> &Cell {
        &self.content[usize::from(y - self.area.y) * usize::from(self.area.width)
            + usize::from(x - self.area.x)]
    }
}
pub trait Target {
    fn size(&self) -> io::Result<Size>;
    fn capture(&mut self, frame: NativeFrame) -> io::Result<()>;
    fn cursor(&self) -> Position;
}
pub mod backend {
    use super::*;
    pub struct TestBackend {
        buffer: Buffer,
        cursor: Position,
    }
    impl TestBackend {
        pub fn new(w: u16, h: u16) -> Self {
            Self {
                buffer: Buffer::empty(Rect::new(0, 0, w, h)),
                cursor: Position::ORIGIN,
            }
        }
        pub fn buffer(&self) -> &Buffer {
            &self.buffer
        }
        pub fn resize(&mut self, w: u16, h: u16) {
            self.buffer = Buffer::empty(Rect::new(0, 0, w, h));
        }
    }
    impl Target for TestBackend {
        fn size(&self) -> io::Result<Size> {
            Ok(Size::new(self.buffer.area.width, self.buffer.area.height))
        }
        fn capture(&mut self, frame: NativeFrame) -> io::Result<()> {
            let (buffer, cursor) = Buffer::capture(frame);
            self.buffer = buffer;
            if let Some(c) = cursor {
                self.cursor = c
            }
            Ok(())
        }
        fn cursor(&self) -> Position {
            self.cursor
        }
    }
}
pub struct Terminal<B: Target> {
    backend: B,
}
impl<B: Target> Terminal<B> {
    pub fn new(backend: B) -> io::Result<Self> {
        Ok(Self { backend })
    }
    pub fn backend(&self) -> &B {
        &self.backend
    }
    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }
    pub fn draw(&mut self, draw: impl FnOnce(&mut NativeFrame)) -> io::Result<()> {
        let size = self.backend.size()?;
        let mut frame = NativeFrame::new(
            Rect::new(0, 0, size.width, size.height),
            crate::palette::PaletteProfile::ClassicDn,
        );
        draw(&mut frame);
        self.backend.capture(frame)
    }
    pub fn get_cursor_position(&self) -> io::Result<Position> {
        Ok(self.backend.cursor())
    }
}
