use crate::dialog_theme as theme;
use crate::turbo::dialog_spec::{scope, Control, ControlKind, DialogSpec};

use crate::painting::Paragraph;
use crate::presentation::{layout::Rect, text::Line};
use crate::{
    app::App,
    command_line::history::{self, Action},
    filesystem::display_name,
    mouse::Target,
};
pub(crate) fn command_history_spec(app: &App, area: Rect) -> Option<DialogSpec<'_>> {
    let d = app.command_history()?;
    let language = app.config.language;
    let profile = app.config.palette;
    let dos = profile.styles_for(crate::palette::theme::Group::Dialogs);
    let (default, _) = history::geometry(area);
    let rect = crate::tool_windows::bounds(app, area, default.width, default.height);
    let mut spec = DialogSpec::new(
        rect,
        scope(app, crate::focus::Focus::CommandHistory)?,
        d.focus,
        language.text("Commands History"),
        Target::CommandHistory(Action::Use),
        Target::CommandHistory(Action::Cancel),
    );
    let x = rect.x + 2;
    let width = rect.width - 4;
    spec.caption(
        Paragraph::new(language.text("Filter commands")).style(dos(theme::TEXT)),
        Rect::new(x, rect.y + 1, width, 1),
    );
    let field = Rect::new(x, rect.y + 2, width, 1);
    let column = Line::from(&d.query[..d.input_cursor]).width();
    let scroll = column.saturating_sub(usize::from(width.saturating_sub(1)));
    spec.control(Control::new(
        0,
        field,
        ControlKind::ScrolledInput {
            value: d.query.clone(),
            scroll,
        },
        Target::CommandHistory(Action::Field),
    ));
    if d.focus == 0 {
        spec.cursor = Some((field.x + (column - scroll) as u16, field.y));
    }
    if d.matches.is_empty() {
        spec.caption(
            Paragraph::new(language.text(if d.source.is_empty() {
                "No commands in this session."
            } else {
                "No matching commands."
            }))
            .style(dos(theme::TEXT)),
            Rect::new(x, rect.y + 4, width, 1),
        );
    }
    let items = d
        .matches
        .iter()
        .filter_map(|i| d.source.get(*i))
        .map(|s| display_name(s))
        .collect();
    let mut list = Control::new(
        1,
        Rect::new(x, rect.y + 4, width, d.rows as u16),
        ControlKind::List {
            items,
            cursor: d.cursor,
            offset: d.offset,
        },
        Target::CommandHistory(Action::Use),
    );
    list.hits.clear();
    list.paint = !d.matches.is_empty();
    spec.control(list);
    for i in d.offset..d.matches.len().min(d.offset + d.rows) {
        spec.hit(
            Rect::new(x, rect.y + 4 + (i - d.offset) as u16, width, 1),
            Target::CommandHistory(Action::Row(i)),
        );
    }
    spec.caption(
        Paragraph::new(language.text("Enter recalls without running · Tab moves · Esc cancels"))
            .style(dos(theme::TEXT)),
        Rect::new(x, rect.bottom() - 3, width, 1),
    );
    for (label, a, focus, button_x) in [
        ("[ Use ]", Action::Use, 2, rect.x + 10),
        ("[ Cancel ]", Action::Cancel, 3, rect.right() - 26),
    ] {
        let button = Rect::new(button_x, rect.bottom() - 2, 16, 1);
        let mut control = Control::new(
            focus,
            button,
            ControlKind::Button(language.text(label).into()),
            Target::CommandHistory(a),
        );
        control.enabled = a != Action::Use || !d.matches.is_empty();
        spec.control(control);
    }
    Some(spec)
}
