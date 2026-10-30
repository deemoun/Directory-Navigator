use crate::dialog_theme as theme;
use crate::painting::{Block, Frame, Paragraph};
use crate::presentation::layout::Rect;
use crate::turbo::dialog_spec::{scope, Control, ControlKind, DialogSpec};
use crate::{
    app::App,
    mouse::Target,
    screensaver::{ACCEPT, CANCEL, DELAYS, PREVIEW},
};

pub(super) fn draw_sky(frame: &mut impl Frame, app: &App, area: Rect) {
    let dos = app
        .config
        .palette
        .styles_for(crate::palette::theme::Group::Dialogs);
    frame.render_widget(Block::default().style(dos(0x07)), area);
    for (x, y, glyph, attr) in app.screensaver.cells() {
        frame.render_widget(
            Paragraph::new(glyph.to_string()).style(dos(attr)),
            Rect::new(area.x + x, area.y + y, 1, 1),
        );
    }
}
pub(super) fn draw_setup(frame: &mut impl Frame, app: &App, area: Rect) {
    if let Some(s) = saver_spec(app, area) {
        s.paint(frame, app);
    }
}
pub(crate) fn saver_spec(app: &App, area: Rect) -> Option<DialogSpec<'_>> {
    let setup = app.screensaver.modals.screensaver()?;
    let dos = app
        .config
        .palette
        .styles_for(crate::palette::theme::Group::Dialogs);
    let rect = Rect::new(
        area.x + (area.width - 58) / 2,
        area.y + (area.height - 20) / 2,
        58,
        20,
    );
    let mut spec = DialogSpec::new(
        rect,
        scope(app, crate::focus::Focus::SaverSetup)?,
        setup.cursor,
        app.config.language.text(" Screen savers "),
        Target::Screensaver(ACCEPT),
        Target::Screensaver(CANCEL),
    );
    spec.caption(
        Paragraph::new(app.config.language.text("Selected saver: Star Sky"))
            .style(dos(theme::TEXT)),
        Rect::new(rect.x + 3, rect.y + 2, 52, 1),
    );
    spec.caption(
        Paragraph::new(app.config.language.text("Time before screen rest")).style(dos(theme::TEXT)),
        Rect::new(rect.x + 3, rect.y + 4, 52, 1),
    );
    for (i, minutes) in DELAYS.iter().enumerate() {
        let label = if *minutes == 0 {
            app.config.language.text("Never").to_string()
        } else {
            format!(
                "{minutes} {}",
                app.config
                    .language
                    .text(if *minutes == 1 { "minute" } else { "minutes" })
            )
        };
        let row = Rect::new(rect.x + 4, rect.y + 6 + i as u16, 48, 1);
        spec.control(Control::new(
            i,
            row,
            ControlKind::Radio {
                caption: label,
                checked: setup.delay == *minutes,
            },
            Target::Screensaver(i),
        ));
    }
    spec.caption(
        Paragraph::new(
            app.config
                .language
                .text("Any key or mouse input returns to DN"),
        )
        .style(dos(theme::TEXT)),
        Rect::new(rect.x + 3, rect.y + 14, 52, 1),
    );
    for (i, x, label) in [
        (PREVIEW, 4, "[ Preview ]"),
        (ACCEPT, 23, "[ OK ]"),
        (CANCEL, 39, "[ Cancel ]"),
    ] {
        let row = Rect::new(rect.x + x, rect.y + 17, 12, 1);
        spec.control(Control::new(
            i,
            row,
            ControlKind::Button(app.config.language.text(label).into()),
            Target::Screensaver(i),
        ));
    }
    Some(spec)
}
