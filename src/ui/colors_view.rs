use crate::dialog_theme as theme;
use crate::turbo::dialog_spec::{scope, Control, ControlKind, DialogSpec};
pub(super) fn draw(frame: &mut impl Frame, app: &App, area: Rect) {
    if let Some(s) = colors_spec(app, area) {
        s.paint(frame, app);
    }
}
use crate::painting::{Frame, Paragraph};
use crate::presentation::layout::Rect;
use crate::{app::App, mouse::Target, palette::PaletteProfile};
pub(crate) fn colors_spec(app: &App, area: Rect) -> Option<DialogSpec<'_>> {
    let dialog = app.modals.color_dialog()?;
    let cursor = dialog.cursor;
    let profile = app.config.palette;
    let dos = profile.styles();
    let rect = Rect::new(
        area.x + (area.width - 72) / 2,
        area.y + (area.height - 18) / 2,
        72,
        18,
    );
    let mut spec = DialogSpec::new(
        rect,
        scope(app, crate::focus::Focus::Colors)?,
        cursor,
        app.config.language.text(" Colors "),
        Target::AcceptColors,
        Target::CancelColors,
    );
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
        spec.control(Control::new(
            i as usize,
            row,
            ControlKind::Radio {
                caption: label.into(),
                checked: profile == value,
            },
            Target::ColorChoice(value),
        ));
    }
    spec.caption(
        Paragraph::new(app.config.language.text("Preview")).style(dos(theme::TEXT)),
        Rect::new(rect.x + 3, rect.y + 6, 65, 1),
    );
    for (i, attr, label) in [
        (0, 0x8e, " Name      marked"),
        (1, 0x8f, " folder"),
        (2, 0x87, " ordinary file"),
        (3, 0x8b, " source.rs"),
        (4, 0x30, " selected file"),
    ] {
        spec.caption(
            Paragraph::new(app.config.language.text(label)).style(dos(attr)),
            Rect::new(rect.x + 4, rect.y + 7 + i, 38, 1),
        );
    }
    spec.caption(
        Paragraph::new(" 12:34:56 ").style(dos(theme::HEADING)),
        Rect::new(rect.right() - 17, rect.y + 7, 10, 1),
    );
    spec.caption(
        Paragraph::new(profile.function(5, "Copy", true)).style(profile.bar(true)),
        Rect::new(rect.x + 4, rect.y + 12, 62, 1),
    );
    spec.caption(
        Paragraph::new(
            app.config
                .language
                .text("C/T or Space chooses · Enter applies · Esc restores"),
        )
        .style(dos(theme::TEXT)),
        Rect::new(rect.x + 3, rect.y + 14, 66, 1),
    );
    for (index, (label, target, x)) in [
        ("[ Apply ]", Target::AcceptColors, rect.x + 12),
        ("[ Cancel ]", Target::CancelColors, rect.right() - 26),
    ]
    .into_iter()
    .enumerate()
    {
        let row = Rect::new(x, rect.bottom() - 2, 16, 1);
        spec.control(Control::new(
            index + 2,
            row,
            ControlKind::Button(app.config.language.text(label).into()),
            target,
        ));
    }
    Some(spec)
}
