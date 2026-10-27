//! Calculator-only DN dialog; other utility layouts keep their existing chrome.
use crate::painting::{Frame, Paragraph, Wrap};
use crate::presentation::{
    layout::{Alignment, Rect},
    text::Line,
};
use crate::{
    app::App,
    filesystem::display_name,
    formatting,
    mouse::Target,
    utilities::{calculator::Value, Action},
};

pub(super) fn draw(frame: &mut impl Frame, app: &App, area: Rect) {
    let profile = app.config.palette;
    let dos = profile.styles();
    let language = app.config.language;
    let rect =
        super::dialog_style::window(frame, profile, area, 72, 18, language.text("Calculator"));
    frame.regions().clear();
    super::dialog_style::close_button(frame, profile, rect, Target::Utility(Action::Close));
    let calc = &app.utilities.calculator;
    let focus = calc.focus;
    let expression = Rect::new(rect.x + 2, rect.y + 2, rect.width - 4, 3);
    frame.render_widget(
        crate::turbo::widgets::group_box(language.text("Expression")),
        expression,
    );
    let field = Rect::new(expression.x + 2, expression.y + 1, expression.width - 8, 1);
    let column = Line::from(&calc.expression[..calc.cursor]).width();
    let scroll = column.saturating_sub(usize::from(field.width - 1));
    frame.render_widget(
        crate::turbo::widgets::input_scrolled(&display_name(&calc.expression), scroll, focus == 0),
        field,
    );
    frame
        .regions()
        .push((field, Target::Utility(Action::Field)));
    let history_button = Rect::new(expression.right() - 4, field.y, 2, 1);
    frame.render_widget(crate::turbo::widgets::history_button(), history_button);
    frame
        .regions()
        .push((history_button, Target::Utility(Action::CalculatorHistory)));
    if focus == 0 && !calc.history_open && app.dialogs().is_empty() {
        frame.set_cursor_position((field.x + (column - scroll) as u16, field.y));
    }
    frame.render_widget(
        Paragraph::new(language.text("Copy as")).style(dos(0x70)),
        Rect::new(rect.x + 4, rect.y + 6, 11, 1),
    );
    let results = Rect::new(rect.x + 16, rect.y + 6, rect.width - 18, 7);
    frame.render_widget(crate::turbo::widgets::group_box(""), results);
    let value = calc.result.or_else(|| {
        (calc.expression.is_empty() && calc.error.is_none()).then_some(Value::Integer(0))
    });
    let bases = value.map(Value::representations);
    for (index, label) in ["DEC", "HEX", "BIN", "OCT", "EXP"].iter().enumerate() {
        let y = results.y + 1 + index as u16;
        let radio = Rect::new(rect.x + 4, y, 11, 1);
        frame.render_widget(
            crate::turbo::widgets::radio(label, calc.base == index, focus == index + 1),
            radio,
        );
        frame
            .regions()
            .push((radio, Target::Utility(Action::Base(index))));
        let text = if index == 4 {
            value.map(Value::scientific).unwrap_or_else(|| "—".into())
        } else {
            bases
                .as_ref()
                .and_then(|b| b[index].as_ref().ok())
                .cloned()
                .unwrap_or_else(|| "—".into())
        };
        let row = Rect::new(results.x + 2, y, results.width - 4, 1);
        frame.render_widget(
            Paragraph::new(formatting::tail(&text, row.width))
                .alignment(Alignment::Right)
                .style(dos(0x78)),
            row,
        );
        frame
            .regions()
            .push((row, Target::Utility(Action::Base(index))));
    }
    let notice = calc
        .error
        .as_deref()
        .filter(|_| !calc.history_open)
        .unwrap_or(&app.utilities.notice);
    frame.render_widget(
        Paragraph::new(language.text(notice))
            .style(dos(if calc.error.is_some() { 0x74 } else { 0x70 }))
            .wrap(Wrap { trim: false }),
        Rect::new(rect.x + 3, rect.y + 13, rect.width - 6, 2),
    );
    for (i, (label, command, selected, mnemonic)) in [
        ("Evaluate", Action::Evaluate, focus == 6, 'e'),
        ("Copy", Action::Copy, focus == 7, 'c'),
        ("Close", Action::Close, focus == 8, 'l'),
        ("Help", Action::CalculatorHelp, focus == 9, 'h'),
    ]
    .into_iter()
    .enumerate()
    {
        let button = Rect::new(rect.x + 2 + i as u16 * 17, rect.y + 15, 14, 1);
        let label = language.text(label);
        let mut marked = false;
        let label = label
            .chars()
            .map(|c| {
                if !marked && c.to_ascii_lowercase() == mnemonic {
                    marked = true;
                    format!("~{c}~")
                } else {
                    c.to_string()
                }
            })
            .collect::<String>();
        frame.render_widget(
            crate::turbo::widgets::button_with_default(
                &label,
                selected,
                command == Action::Evaluate && focus < 6,
            ),
            Rect::new(button.x, button.y, button.width, 2),
        );
        frame.regions().push((button, Target::Utility(command)));
    }
    if calc.history_open {
        let rows = calc.history.len().clamp(1, 6);
        let history = Rect::new(
            expression.x,
            expression.bottom(),
            expression.width,
            rows as u16 + 2,
        );
        frame.render_widget(
            crate::turbo::widgets::dialog(language.text("Expression history")),
            history,
        );
        let offset = calc.history_selection.saturating_sub(rows - 1);
        if calc.history.is_empty() {
            frame.render_widget(
                Paragraph::new(language.text("No expression history")).style(dos(0x78)),
                Rect::new(history.x + 2, history.y + 1, history.width - 4, 1),
            );
        }
        if !calc.history.is_empty() {
            frame.render_widget(
                crate::turbo::widgets::list(
                    calc.history.iter().map(|s| display_name(s)).collect(),
                    calc.history_selection,
                    offset,
                    true,
                ),
                Rect::new(history.x + 1, history.y + 1, history.width - 2, rows as u16),
            );
        }
        for index in offset..calc.history.len().min(offset + rows) {
            frame.regions().push((
                Rect::new(
                    history.x + 1,
                    history.y + 1 + (index - offset) as u16,
                    history.width - 2,
                    1,
                ),
                Target::Utility(Action::CalculatorRecall(index)),
            ));
        }
    }
}
