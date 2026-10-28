use super::{Action, CalculatorPlugin};
use crate::plugins::Context;
use crate::turbo::dialog_spec::{centered, Control, ControlKind, DialogSpec};
// Calculator controls share the guarded utility definition.
use crate::painting::{Paragraph, Wrap};
use crate::presentation::{
    layout::{Alignment, Rect},
    text::Line,
};
use crate::{filesystem::display_name, formatting, utilities::calculator::Value};

pub(super) fn calculator_spec<'a>(plugin: &'a CalculatorPlugin, ctx: &Context) -> DialogSpec<'a> {
    let area = ctx.area;
    let profile = ctx.palette;
    let dos = profile.styles();
    let language = ctx.language;
    let rect = centered(area, 72, 18);
    let calc = &plugin.state;
    let focus = calc.focus;
    let mut spec = ctx.dialog(
        rect,
        focus,
        language.text("Calculator"),
        Action::Evaluate.id(),
        Action::Close.id(),
    );
    let expression = Rect::new(rect.x + 2, rect.y + 2, rect.width - 4, 3);
    spec.native(
        crate::turbo::widgets::group_box(language.text("Expression")),
        expression,
    );
    let field = Rect::new(expression.x + 2, expression.y + 1, expression.width - 8, 1);
    let column = Line::from(&calc.expression[..calc.cursor]).width();
    let scroll = column.saturating_sub(usize::from(field.width - 1));
    spec.control(Control::new(
        0,
        field,
        ControlKind::ScrolledInput {
            value: display_name(&calc.expression),
            scroll,
        },
        ctx.target(Action::Field.id()),
    ));
    let history_button = Rect::new(expression.right() - 4, field.y, 2, 1);
    spec.native(crate::turbo::widgets::history_button(), history_button);
    spec.hit(history_button, ctx.target(Action::History.id()));
    if focus == 0 && !calc.history_open {
        spec.cursor = Some((field.x + (column - scroll) as u16, field.y));
    }
    spec.caption(
        Paragraph::new(language.text("Copy as")).style(dos(0x70)),
        Rect::new(rect.x + 4, rect.y + 6, 11, 1),
    );
    let results = Rect::new(rect.x + 16, rect.y + 6, rect.width - 18, 7);
    spec.native(crate::turbo::widgets::group_box(""), results);
    let value = calc.result.or_else(|| {
        (calc.expression.is_empty() && calc.error.is_none()).then_some(Value::Integer(0))
    });
    let bases = value.map(Value::representations);
    for (index, label) in ["DEC", "HEX", "BIN", "OCT", "EXP"].iter().enumerate() {
        let y = results.y + 1 + index as u16;
        let radio = Rect::new(rect.x + 4, y, 11, 1);
        spec.control(Control::new(
            index + 1,
            radio,
            ControlKind::Radio {
                caption: (*label).into(),
                checked: calc.base == index,
            },
            ctx.target(Action::Base(index).id()),
        ));
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
        spec.caption(
            Paragraph::new(formatting::tail(&text, row.width))
                .alignment(Alignment::Right)
                .style(dos(0x78)),
            row,
        );
        spec.hit(row, ctx.target(Action::Base(index).id()));
    }
    let notice = calc
        .error
        .as_deref()
        .filter(|_| !calc.history_open)
        .unwrap_or(&plugin.notice);
    spec.caption(
        Paragraph::new(language.text(notice))
            .style(dos(if calc.error.is_some() { 0x74 } else { 0x70 }))
            .wrap(Wrap { trim: false }),
        Rect::new(rect.x + 3, rect.y + 13, rect.width - 6, 2),
    );
    for (i, (label, command, mnemonic)) in [
        ("Evaluate", Action::Evaluate, 'e'),
        ("Copy", Action::Copy, 'c'),
        ("Close", Action::Close, 'l'),
        ("Help", Action::Help, 'h'),
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
        spec.control(Control::new(
            i + 6,
            button,
            ControlKind::Button(label),
            ctx.target(command.id()),
        ));
    }
    if let Some(child) = history_spec(plugin, ctx) {
        spec.overlay(child);
    }
    spec
}
pub(super) fn history_spec<'a>(
    plugin: &'a CalculatorPlugin,
    ctx: &Context,
) -> Option<DialogSpec<'a>> {
    let area = ctx.area;
    let calc = &plugin.state;
    if !calc.history_open {
        return None;
    }
    let rect = centered(area, 72, 18);
    let rows = calc.history.len().clamp(1, 6);
    let r = Rect::new(rect.x + 2, rect.y + 5, rect.width - 4, rows as u16 + 2);
    let action = ctx.target(Action::Recall(calc.history_selection).id());
    let mut s = ctx.dialog(
        r,
        0,
        ctx.language.text("Expression history"),
        Action::Recall(calc.history_selection).id(),
        Action::History.id(),
    );
    if calc.history.is_empty() {
        s.caption(
            Paragraph::new(ctx.language.text("No expression history")).style(ctx.palette.dos(0x78)),
            Rect::new(r.x + 2, r.y + 1, r.width - 4, 1),
        );
    }
    let offset = calc.history_selection.saturating_sub(rows - 1);
    let mut c = Control::new(
        0,
        Rect::new(r.x + 1, r.y + 1, r.width - 2, rows as u16),
        ControlKind::List {
            items: calc.history.iter().map(|s| display_name(s)).collect(),
            cursor: calc.history_selection,
            offset,
        },
        action,
    );
    c.enter_action = true;
    c.paint = !calc.history.is_empty();
    c.hits = (offset..calc.history.len().min(offset + rows))
        .map(|i| {
            (
                Rect::new(r.x + 1, r.y + 1 + (i - offset) as u16, r.width - 2, 1),
                ctx.target(Action::Recall(i).id()),
            )
        })
        .collect();
    s.control(c);
    Some(s)
}
