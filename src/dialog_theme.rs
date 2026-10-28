//! Semantic DOS attributes and shared dialog geometry. Values deliberately match
//! the existing DN theme; native Turbo Vision controls retain their own palettes.
use crate::presentation::layout::Rect;
pub const TEXT: u8 = 0x70;
pub const HEADING: u8 = 0x71;
pub const DISABLED: u8 = 0x78;
pub const FRAME: u8 = 0x7f;
pub const MODAL_CLOSE: u8 = 0x7a;
pub const DOCUMENT_CLOSE: u8 = 0x7b;
pub const CONTENT_X: u16 = 3;
pub const CONTENT_Y: u16 = 4;
pub const HINT_BOTTOM: u16 = 5;
pub const BUTTON_BOTTOM: u16 = 3;
pub const BUTTON_WIDTH: u16 = 14;
pub const BUTTON_STEP: u16 = 18;
pub fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}
pub fn button(rect: Rect, first_x: u16, index: usize) -> Rect {
    Rect::new(
        rect.x + first_x + index as u16 * BUTTON_STEP,
        rect.bottom() - BUTTON_BOTTOM,
        BUTTON_WIDTH,
        1,
    )
}
