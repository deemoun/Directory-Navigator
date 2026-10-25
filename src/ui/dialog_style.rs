//! Shared Turbo Vision dialog chrome; dimensions remain content-specific.
use crate::painting::{Block, BorderType, Borders, Clear, Frame, Paragraph};
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
    let width = width.min(area.width);
    let height = height.min(area.height);
    let rect = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, rect);
    let title = crate::formatting::tail(title, width.saturating_sub(14));
    frame.render_widget(
        Block::default()
            .title(title)
            .title_alignment(Alignment::Center)
            .title_style(profile.dos(0x7f))
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .border_style(profile.dos(0x7f))
            .style(profile.dos(0x70)),
        rect,
    );
    rect
}
pub(super) fn button(
    frame: &mut impl Frame,
    profile: PaletteProfile,
    rect: Rect,
    label: &str,
    focused: bool,
) {
    if profile == PaletteProfile::Terminal {
        frame.render_widget(
            Paragraph::new(label).style(profile.dos(if focused { 0x30 } else { 0x70 })),
            rect,
        );
        return;
    }
    let label = label
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim();
    let width = usize::from(rect.width);
    let label_width = Span::raw(label).width();
    let spare = width.saturating_sub(label_width + 2);
    let line = Line::from(vec![
        Span::styled(if focused { "►" } else { " " }, profile.dos(0x8b)),
        Span::raw(" ".repeat(spare / 2)),
        Span::styled(
            label.to_string(),
            profile.dos(if focused { 0x8e } else { 0x8f }),
        ),
        Span::raw(" ".repeat(spare - spare / 2)),
        Span::styled(if focused { "◄" } else { " " }, profile.dos(0x8b)),
    ]);
    frame.render_widget(Paragraph::new(line).style(profile.dos(0x8f)), rect);
}
pub(super) fn input_style(
    profile: PaletteProfile,
    focused: bool,
) -> crate::presentation::style::Style {
    profile.dos(if profile == PaletteProfile::ClassicDn {
        if focused {
            0x0f
        } else {
            0x07
        }
    } else if focused {
        0x1f
    } else {
        0x17
    })
}

/// Attach the border control to the modal's existing Cancel/Close action.
pub(super) fn close_button(
    frame: &mut impl Frame,
    profile: PaletteProfile,
    rect: Rect,
    target: crate::mouse::Target,
) {
    close_control(frame, profile, rect, target, 0x7a);
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
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("[", profile.dos(0x7f)),
            Span::styled("■", profile.dos(color)),
            Span::styled("]", profile.dos(0x7f)),
        ])),
        close,
    );
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
            .border_style(profile.dos(0x7f))
            .title(title)
            .title_alignment(Alignment::Center)
            .title_style(profile.dos(0x7f))
            .style(profile.dos(0x70)),
        rect,
    );
    close_control(frame, profile, rect, target, 0x7b);
}
