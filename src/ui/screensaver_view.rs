use crate::painting::{Block, Frame, Paragraph};
use crate::{
    app::App,
    mouse::Target,
    screensaver::{ACCEPT, CANCEL, DELAYS, PREVIEW},
};
use ratatui::layout::Rect;

pub(super) fn draw_sky(frame: &mut impl Frame, app: &mut App, area: Rect) {
    let dos = app.config.palette.styles();
    frame.render_widget(Block::default().style(dos(0x07)), area);
    for (x, y, glyph, attr) in app.screensaver.cells() {
        frame.render_widget(
            Paragraph::new(glyph.to_string()).style(dos(attr)),
            Rect::new(area.x + x, area.y + y, 1, 1),
        );
    }
}
pub(super) fn draw_setup(frame: &mut impl Frame, app: &mut App, area: Rect) {
    let Some(setup) = &app.screensaver.setup else {
        return;
    };
    let dos = app.config.palette.styles();
    let rect = Rect::new(
        area.x + (area.width - 58) / 2,
        area.y + (area.height - 20) / 2,
        58,
        20,
    );
    super::dialog_style::window(
        frame,
        app.config.palette,
        rect,
        rect.width,
        rect.height,
        app.config.language.text(" Screen savers "),
    );
    frame.render_widget(
        Paragraph::new(app.config.language.text("Selected saver: Star Sky")).style(dos(0x70)),
        Rect::new(rect.x + 3, rect.y + 2, 52, 1),
    );
    frame.render_widget(
        Paragraph::new(app.config.language.text("Time before screen rest")).style(dos(0x70)),
        Rect::new(rect.x + 3, rect.y + 4, 52, 1),
    );
    app.mouse.regions.clear();
    super::dialog_style::close_button(
        frame,
        app.config.palette,
        rect,
        &mut app.mouse.regions,
        Target::Screensaver(CANCEL),
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
        frame.render_widget(
            Paragraph::new(format!(
                "({}) {label}",
                if setup.delay == *minutes { '●' } else { ' ' }
            ))
            .style(dos(if setup.cursor == i { 0x30 } else { 0x70 })),
            row,
        );
        app.mouse.regions.push((row, Target::Screensaver(i)));
    }
    frame.render_widget(
        Paragraph::new(
            app.config
                .language
                .text("Any key or mouse input returns to DN"),
        )
        .style(dos(0x70)),
        Rect::new(rect.x + 3, rect.y + 14, 52, 1),
    );
    for (i, x, label) in [
        (PREVIEW, 4, "[ Preview ]"),
        (ACCEPT, 23, "[ OK ]"),
        (CANCEL, 39, "[ Cancel ]"),
    ] {
        let row = Rect::new(rect.x + x, rect.y + 17, 12, 1);
        super::dialog_style::button(
            frame,
            app.config.palette,
            row,
            app.config.language.text(label),
            setup.cursor == i,
        );
        app.mouse.regions.push((row, Target::Screensaver(i)));
    }
}
