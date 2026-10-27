use crate::painting::{Frame, Paragraph};
use crate::presentation::{layout::Rect, text::Line};
use crate::{
    app::App,
    command_line::history::{self, Action},
    filesystem::display_name,
    mouse::Target,
};
pub(super) fn draw(frame: &mut impl Frame, app: &App, area: Rect) {
    let Some(d) = app.command_line.modals.command_history() else {
        return;
    };
    let language = app.config.language;
    let profile = app.config.palette;
    let dos = profile.styles();
    let (rect, _) = history::geometry(area);
    super::dialog_style::window(
        frame,
        profile,
        rect,
        rect.width,
        rect.height,
        language.text("Commands History"),
    );
    frame.regions().clear();
    super::dialog_style::close_button(frame, profile, rect, Target::CommandHistory(Action::Cancel));
    let x = rect.x + 2;
    let width = rect.width - 4;
    frame.render_widget(
        Paragraph::new(language.text("Filter commands")).style(dos(0x70)),
        Rect::new(x, rect.y + 1, width, 1),
    );
    let field = Rect::new(x, rect.y + 2, width, 1);
    let column = Line::from(&d.query[..d.input_cursor]).width();
    let scroll = column.saturating_sub(usize::from(width.saturating_sub(1)));
    frame.render_widget(
        crate::turbo::widgets::input_scrolled(d.query.as_str(), scroll, d.focus == 0),
        field,
    );
    if d.focus == 0 {
        frame.set_cursor_position((field.x + (column - scroll) as u16, field.y));
    }
    frame
        .regions()
        .push((field, Target::CommandHistory(Action::Field)));
    if d.matches.is_empty() {
        frame.render_widget(
            Paragraph::new(language.text(if app.command_line.history().is_empty() {
                "No commands in this session."
            } else {
                "No matching commands."
            }))
            .style(dos(0x70)),
            Rect::new(x, rect.y + 4, width, 1),
        );
    }
    let items = d
        .matches
        .iter()
        .filter_map(|i| app.command_line.history().get(*i))
        .map(|s| display_name(s))
        .collect();
    if !d.matches.is_empty() {
        frame.render_widget(
            crate::turbo::widgets::list(items, d.cursor, d.offset, d.focus == 1),
            Rect::new(x, rect.y + 4, width, d.rows as u16),
        );
    }
    for i in d.offset..d.matches.len().min(d.offset + d.rows) {
        frame.regions().push((
            Rect::new(x, rect.y + 4 + (i - d.offset) as u16, width, 1),
            Target::CommandHistory(Action::Row(i)),
        ));
    }
    frame.render_widget(
        Paragraph::new(language.text("Enter recalls without running · Tab moves · Esc cancels"))
            .style(dos(0x70)),
        Rect::new(x, rect.bottom() - 3, width, 1),
    );
    for (label, a, focus, button_x) in [
        ("[ Use ]", Action::Use, 2, rect.x + 10),
        ("[ Cancel ]", Action::Cancel, 3, rect.right() - 26),
    ] {
        let button = Rect::new(button_x, rect.bottom() - 2, 16, 1);
        if a == Action::Use && d.matches.is_empty() {
            frame.render_widget(
                crate::turbo::widgets::disabled_button(language.text(label)),
                button,
            );
        } else {
            super::dialog_style::button_with_default(
                frame,
                profile,
                button,
                language.text(label),
                d.focus == focus,
                focus == 2 && d.focus < 2,
            );
            frame.regions().push((button, Target::CommandHistory(a)));
        }
    }
}
