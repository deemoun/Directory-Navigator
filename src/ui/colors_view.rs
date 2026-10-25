use crate::painting::{Frame, Paragraph};
use crate::presentation::layout::Rect;
use crate::{app::App, mouse::Target, palette::PaletteProfile};
pub(super) fn draw(frame: &mut impl Frame, app: &App, area: Rect) {
    let Some(dialog) = app.modals.color_dialog() else {
        return;
    };
    let cursor = dialog.cursor;
    let profile = app.config.palette;
    let dos = profile.styles();
    let rect = Rect::new(
        area.x + (area.width - 72) / 2,
        area.y + (area.height - 18) / 2,
        72,
        18,
    );
    super::dialog_style::window(
        frame,
        profile,
        rect,
        rect.width,
        rect.height,
        app.config.language.text(" Colors "),
    );
    frame.regions().clear();
    super::dialog_style::close_button(frame, profile, rect, Target::CancelColors);
    for (i, label, value) in [
        (
            0,
            "Classic DN — fixed 256 colors",
            PaletteProfile::ClassicDn,
        ),
        (
            1,
            "Terminal palette — legacy ANSI colors",
            PaletteProfile::Terminal,
        ),
    ] {
        let row = Rect::new(rect.x + 3, rect.y + 2 + i as u16 * 2, 66, 1);
        let label = app.config.language.text(label);
        frame.render_widget(
            crate::turbo::widgets::radio(label, profile == value, cursor == i),
            row,
        );
        frame.regions().push((row, Target::ColorChoice(value)));
    }
    frame.render_widget(
        Paragraph::new(app.config.language.text("Preview")).style(dos(0x70)),
        Rect::new(rect.x + 3, rect.y + 6, 65, 1),
    );
    for (i, attr, label) in [
        (0, 0x8e, " Name      marked"),
        (1, 0x8f, " folder"),
        (2, 0x87, " ordinary file"),
        (3, 0x8b, " source.rs"),
        (4, 0x30, " selected file"),
    ] {
        frame.render_widget(
            Paragraph::new(app.config.language.text(label)).style(dos(attr)),
            Rect::new(rect.x + 4, rect.y + 7 + i, 38, 1),
        );
    }
    frame.render_widget(
        Paragraph::new(" 12:34:56 ").style(dos(0x71)),
        Rect::new(rect.right() - 17, rect.y + 7, 10, 1),
    );
    frame.render_widget(
        Paragraph::new(profile.function(5, "Copy", true)).style(profile.bar(true)),
        Rect::new(rect.x + 4, rect.y + 12, 62, 1),
    );
    frame.render_widget(
        Paragraph::new(
            app.config
                .language
                .text("C/T or Space chooses · Enter applies · Esc restores"),
        )
        .style(dos(0x70)),
        Rect::new(rect.x + 3, rect.y + 14, 66, 1),
    );
    for (label, target, selected, x) in [
        ("[ Apply ]", Target::AcceptColors, cursor == 2, rect.x + 12),
        (
            "[ Cancel ]",
            Target::CancelColors,
            cursor == 3,
            rect.right() - 26,
        ),
    ] {
        let row = Rect::new(x, rect.bottom() - 2, 16, 1);
        super::dialog_style::button(
            frame,
            profile,
            row,
            app.config.language.text(label),
            selected,
        );
        frame.regions().push((row, target));
    }
}
