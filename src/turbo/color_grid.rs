//! DN's 4×4 selector as a native TV control; controller and painting share it.
use turbo_vision::{
    core::{event::*, geometry::Rect, palette::Attr},
    terminal::Terminal,
    views::{View, ViewCore},
};
pub(super) struct ColorGrid {
    core: ViewCore,
    pub selected: u8,
}
impl ColorGrid {
    pub fn new(selected: u8) -> Self {
        Self {
            core: ViewCore::new(Rect::new(0, 0, 16, 4)),
            selected: selected.min(15),
        }
    }
}
impl View for ColorGrid {
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
    fn can_focus(&self) -> bool {
        true
    }
    fn get_palette(&self) -> Option<turbo_vision::core::palette::Palette> {
        None
    }
    fn draw(&mut self, t: &mut Terminal) {
        for color in 0..16u8 {
            for x in 0..4i16 {
                let foreground =
                    if color == 0 || color == 1 || color == 4 || color == 5 || color == 8 {
                        15
                    } else {
                        0
                    };
                t.write_cell(
                    i16::from(color % 4) * 4 + x,
                    i16::from(color / 4),
                    turbo_vision::core::draw::Cell::new(
                        if color == self.selected && x == 2 {
                            '•'
                        } else {
                            ' '
                        },
                        Attr::from_u8((color << 4) | foreground),
                    ),
                );
            }
        }
    }
    fn handle_event(&mut self, event: &mut Event) {
        if event.what != EventType::Keyboard {
            return;
        }
        let col = self.selected % 4;
        let row = self.selected / 4;
        self.selected = match event.key_code {
            KB_LEFT => row * 4 + col.saturating_sub(1),
            KB_RIGHT => row * 4 + (col + 1).min(3),
            KB_UP => row.saturating_sub(1) * 4 + col,
            KB_DOWN => (row + 1).min(3) * 4 + col,
            _ => return,
        };
        event.clear();
    }
}
