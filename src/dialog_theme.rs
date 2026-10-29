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

/// The native title has one leading owner cell and one shadow column. Leave
/// another cell on each side for the DN focus markers, measured in screen cells.
pub fn button_caption(label: &str) -> &str {
    label
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim()
}
pub fn minimum_button_width(label: &str) -> u16 {
    use unicode_width::UnicodeWidthStr;
    button_caption(label)
        .replace('~', "")
        .width()
        .saturating_add(4)
        .min(u16::MAX as usize) as u16
}
/// Right-anchored compact row. Preserve preferred sizes/gaps when possible;
/// translated captions may borrow padding, never a neighbouring button's cells.
pub fn button_row(row: Rect, labels: &[&str], preferred: u16, gap: u16) -> Vec<Rect> {
    let minimum: Vec<_> = labels
        .iter()
        .map(|label| minimum_button_width(label))
        .collect();
    let mut widths: Vec<_> = minimum.iter().map(|w| preferred.max(*w)).collect();
    let gaps = labels.len().saturating_sub(1) as u16;
    let mut gap = gap;
    let total = |widths: &[u16], gap: u16| widths.iter().copied().sum::<u16>() + gaps * gap;
    if total(&widths, gap) > row.width {
        gap = 0;
    }
    while total(&widths, gap) > row.width {
        let Some(i) = widths.iter().zip(&minimum).position(|(w, min)| w > min) else {
            break;
        };
        widths[i] -= 1;
    }
    let mut x = row.right().saturating_sub(total(&widths, gap)).max(row.x);
    widths
        .into_iter()
        .map(|width| {
            let r = Rect::new(x, row.y, width, row.height);
            x += width + gap;
            r
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compact_rows_reserve_markers_for_mnemonics_combining_and_wide_captions() {
        assert_eq!(minimum_button_width("[ Cancel ]"), 10);
        assert_eq!(minimum_button_width("От~м~ена"), 10);
        assert_eq!(minimum_button_width("~界~e\u{301}"), 7);
        let row = Rect::new(2, 3, 35, 1);
        let labels = ["OK", "От~м~ена", "~С~правка"];
        let bounds = button_row(row, &labels, 9, 1);
        assert_eq!(bounds.last().unwrap().right(), row.right());
        for (r, label) in bounds.iter().zip(labels) {
            assert!(r.width >= minimum_button_width(label));
        }
        assert!(bounds.windows(2).all(|r| r[0].right() <= r[1].x));
    }
}
