//! Bounds shared by native page painting, cursor placement and control targets.
use super::{Game, Screen};
use crate::{localization::Language, presentation::layout::Rect};
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
        Screen::Play => (54, 23),
    };
    crate::turbo::dialog_spec::centered(area, w, h)
}
pub(crate) fn button_width(label: &str) -> u16 {
    crate::dialog_theme::minimum_button_width(label)
}
