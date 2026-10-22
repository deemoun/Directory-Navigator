use crate::{
    app::App,
    command_line::history::{self, Action},
    filesystem::display_name,
    mouse::Target,
};
use ratatui::{layout::Rect, text::Line, widgets::Paragraph, Frame};
pub(super) fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(d) = &app.command_line.dialog else {
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
    app.mouse.regions.clear();
    super::dialog_style::close_button(
        frame,
        profile,
        rect,
        &mut app.mouse.regions,
        Target::CommandHistory(Action::Cancel),
    );
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
        Paragraph::new(d.query.as_str())
            .scroll((0, scroll as u16))
            .style(super::dialog_style::input_style(profile, d.focus == 0)),
        field,
    );
    if d.focus == 0 {
        frame.set_cursor_position((field.x + (column - scroll) as u16, field.y));
    }
    app.mouse
        .regions
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
    for (i, index) in d.matches.iter().enumerate().skip(d.offset).take(d.rows) {
        if let Some(command) = app.command_line.history().get(*index) {
            let row = Rect::new(x, rect.y + 4 + (i - d.offset) as u16, width, 1);
            frame.render_widget(
                Paragraph::new(display_name(command)).style(dos(if i == d.cursor {
                    0x30
                } else {
                    0x70
                })),
                row,
            );
            app.mouse
                .regions
                .push((row, Target::CommandHistory(Action::Row(i))));
        }
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
                Paragraph::new(language.text(label)).style(dos(0x78)),
                button,
            );
        } else {
            super::dialog_style::button(
                frame,
                profile,
                button,
                language.text(label),
                d.focus == focus,
            );
            app.mouse.regions.push((button, Target::CommandHistory(a)));
        }
    }
}
