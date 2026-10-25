use crate::painting::{Frame, Paragraph};
use crate::{app::App, mouse::Target};
use ratatui::layout::Rect;
pub(super) fn draw(frame: &mut impl Frame, app: &mut App, area: Rect) {
    let Some(preview) = &mut app.config_import else {
        return;
    };
    let profile = app.config.palette;
    let lang = app.config.language;
    let rect = super::dialog_style::window(
        frame,
        profile,
        area,
        76,
        21,
        lang.text(" Import DOS settings "),
    );
    let rows = usize::from(rect.height.saturating_sub(5));
    let lines = preview.lines(lang);
    preview.offset = preview.offset.min(lines.len().saturating_sub(rows));
    let body = lines
        .iter()
        .skip(preview.offset)
        .take(rows)
        .map(|line| crate::filesystem::display_name(line))
        .collect::<Vec<_>>()
        .join("\n");
    frame.render_widget(
        Paragraph::new(body)
            .scroll((0, preview.horizontal))
            .style(profile.dos(0x70)),
        Rect::new(rect.x + 2, rect.y + 1, rect.width - 4, rect.height - 5),
    );
    frame.render_widget(
        Paragraph::new(lang.text("↑↓ / PgUp/PgDn scroll")).style(profile.dos(0x70)),
        Rect::new(rect.x + 2, rect.bottom() - 4, rect.width - 4, 1),
    );
    let enabled = preview.can_apply();
    for (index, label, target) in [
        (0, "[ Apply ]", Target::ApplyImport),
        (1, "[ Cancel ]", Target::CancelImport),
    ] {
        let button = Rect::new(
            rect.x + rect.width / 2 - 15 + index * 16,
            rect.bottom() - 2,
            14,
            1,
        );
        if index == 0 && !enabled {
            frame.render_widget(
                Paragraph::new(lang.text(label)).style(profile.dos(0x78)),
                button,
            );
        } else {
            super::dialog_style::button(
                frame,
                profile,
                button,
                lang.text(label),
                preview.focus == usize::from(index),
            );
        }
        if index == 0 && !enabled {
            continue;
        }
        app.mouse.regions.push((button, target));
    }
    // Remove every background target while retaining only this modal's buttons.
    app.mouse
        .regions
        .retain(|(_, target)| matches!(target, Target::ApplyImport | Target::CancelImport));
    super::dialog_style::close_button(
        frame,
        profile,
        rect,
        &mut app.mouse.regions,
        Target::CancelImport,
    );
}
