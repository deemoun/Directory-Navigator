//! Bounds shared by native page painting, cursor placement and control targets.
use super::{Game, Screen};
use crate::{localization::Language, presentation::layout::Rect};
/// Integer cell scaling preserves the board's proportions and terminal glyphs.
pub(crate) struct Play {
    pub bounds: Rect,
    pub scale: u16,
}
impl Play {
    pub fn new(area: Rect) -> Self {
        let scale = (area.width.saturating_sub(32) / 24)
            .min(area.height.saturating_sub(6) / 19)
            .clamp(1, 4);
        Self {
            bounds: crate::turbo::dialog_spec::centered(area, 30 + 24 * scale, 4 + 19 * scale),
            scale,
        }
    }
    pub fn sidebar(&self) -> u16 {
        self.bounds.x + 4 + 24 * self.scale
    }
    pub fn buttons_y(&self) -> u16 {
        self.bounds.bottom() - 6
    }
}
pub(crate) fn page(area: Rect, game: &Game, language: Language, pending: bool) -> Rect {
    let (w, h) = match game.screen {
        Screen::Setup { .. } => (
            if language == Language::Russian {
                68
            } else {
                53
            },
            15,
        ),
        Screen::Scores => (
            72,
            if pending || (!game.notice.is_empty() && game.notice != "Scores saved") {
                21
            } else {
                19
            },
        ),
        Screen::Name => (60, 9),
        Screen::GameOver => (44, 7),
        Screen::Help => (66, 17),
        Screen::Play => return Play::new(area).bounds,
    };
    crate::turbo::dialog_spec::centered(area, w, h)
}
pub(crate) fn button_width(label: &str) -> u16 {
    crate::dialog_theme::minimum_button_width(label)
}
