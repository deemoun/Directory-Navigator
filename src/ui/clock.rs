use crate::app::App;
use crate::painting::{Frame, Paragraph};
use crate::presentation::layout::Rect;
pub(super) fn visible(app: &App) -> bool {
    app.config.show_clock
        && app.desktop.game().is_none()
        && app.utilities.size.0 >= 80
        && app.utilities.size.1 >= 25
}
pub(super) fn header(app: &App, area: Rect) -> Rect {
    Rect {
        width: area.width.saturating_sub(if visible(app) {
            app.config.datetime.clock_width()
        } else {
            0
        }),
        ..area
    }
}
pub(super) fn draw(frame: &mut impl Frame, app: &App, area: Rect) {
    if visible(app) {
        frame.render_widget(
            Paragraph::new(app.clock.text.as_str()).style(app.config.palette.dos(0x71)),
            Rect::new(
                area.right() - app.config.datetime.clock_width(),
                area.y,
                app.config.datetime.clock_width(),
                1,
            ),
        );
    }
}
