//! Shared Turbo Vision dialog chrome; dimensions remain content-specific.
use crate::dialog_theme as theme;
use crate::painting::{Block, BorderType, Borders, Frame, Paragraph};
use crate::palette::PaletteProfile;
use crate::presentation::{
    layout::{Alignment, Rect},
    text::{Line, Span},
};

pub(super) fn window(
    frame: &mut impl Frame,
    profile: PaletteProfile,
    area: Rect,
    width: u16,
    height: u16,
    title: &str,
) -> Rect {
    let _ = profile;
    let rect = theme::centered(area, width, height);
    frame.render_widget(crate::turbo::widgets::dialog(title), rect);
    rect
}
/// Attach the border control to the modal's existing Cancel/Close action.
pub(super) fn close_button(
    frame: &mut impl Frame,
    profile: PaletteProfile,
    rect: Rect,
    target: crate::mouse::Target,
) {
    close_control(frame, profile, rect, target, theme::MODAL_CLOSE);
}

fn close_control(
    frame: &mut impl Frame,
    profile: PaletteProfile,
    rect: Rect,
    target: crate::mouse::Target,
    color: u8,
) {
    if rect.width < 7 || rect.height == 0 {
        return;
    }
    let close = Rect::new(rect.x + 2, rect.y, 3, 1);
    if color != theme::MODAL_CLOSE {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    "[",
                    profile.for_group(crate::palette::theme::Group::Dialogs, theme::FRAME),
                ),
                Span::styled(
                    "■",
                    profile.for_group(crate::palette::theme::Group::Dialogs, color),
                ),
                Span::styled(
                    "]",
                    profile.for_group(crate::palette::theme::Group::Dialogs, theme::FRAME),
                ),
            ])),
            close,
        );
    }
    frame.regions().push((close, target));
}

/// Document windows use DN's cyan close marker; modal dialogs use green.
pub(super) fn document_header(
    frame: &mut impl Frame,
    profile: PaletteProfile,
    rect: Rect,
    title: &str,
    target: crate::mouse::Target,
) {
    let title = crate::formatting::tail(title, rect.width.saturating_sub(14));
    frame.render_widget(
        Block::default()
            .borders(Borders::TOP)
            .border_type(BorderType::Double)
            .border_style(profile.for_group(crate::palette::theme::Group::Dialogs, theme::FRAME))
            .title(title)
            .title_alignment(Alignment::Center)
            .title_style(profile.for_group(crate::palette::theme::Group::Dialogs, theme::FRAME))
            .style(profile.for_group(crate::palette::theme::Group::Dialogs, theme::TEXT)),
        rect,
    );
    close_control(frame, profile, rect, target, theme::DOCUMENT_CLOSE);
}
