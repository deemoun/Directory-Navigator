use crate::painting::Paragraph;
use crate::presentation::{layout::Rect, text::Line};
use crate::turbo::dialog_spec::{scope, Control, ControlKind, DialogSpec};
use crate::{
    app::App,
    file_tools::{ToolAction, ToolDialog},
    filesystem::display_name,
    formatting::tail,
    mouse::Target,
};

pub(crate) fn file_tools_spec(app: &App, area: Rect) -> Option<DialogSpec<'_>> {
    let profile = app.config.palette;
    let modal = app.file_tools.modals.file_tools()?;
    let (title, height) = match modal {
        ToolDialog::History { kind, .. } => (kind.title(), area.height.saturating_sub(4).min(22)),
        ToolDialog::Compare { .. } => (" Compare directories ", 15),
        ToolDialog::Export { .. } => (" Make list file ", 13),
        ToolDialog::Busy => (" Make list file ", 7),
    };
    let width = area.width.saturating_sub(6).min(94);
    let rect = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    let focus = match modal {
        ToolDialog::History { focus, .. }
        | ToolDialog::Compare { focus }
        | ToolDialog::Export { focus, .. } => *focus,
        _ => 0,
    };
    let mut spec = DialogSpec::new(
        rect,
        scope(app, crate::focus::Focus::FileTools)?,
        focus,
        app.config.language.text(title),
        Target::Tool(ToolAction::Accept),
        Target::Tool(ToolAction::Cancel),
    );
    spec.close_visible = !matches!(modal, ToolDialog::Busy);
    let mut controls = Vec::new();
    let x = rect.x + 2;
    let w = rect.width - 4;
    match modal {
        ToolDialog::History {
            paths,
            cursor,
            offset,
            rows,
            ..
        } => {
            if paths.is_empty() {
                text(
                    &mut spec,
                    profile,
                    Rect::new(x, rect.y + 2, w, 1),
                    app.config.language.text("No history in this session."),
                    false,
                );
            }
            let mut list = Control::new(
                0,
                Rect::new(x, rect.y + 1, w, *rows as u16),
                ControlKind::List {
                    items: paths
                        .iter()
                        .map(|p| tail(&display_name(&p.to_string_lossy()), w))
                        .collect(),
                    cursor: *cursor,
                    offset: *offset,
                },
                Target::Tool(ToolAction::Accept),
            );
            list.hits.clear();
            list.paint = !paths.is_empty();
            spec.control(list);
            for index in *offset..paths.len().min(*offset + *rows) {
                controls.push((
                    Rect::new(x, rect.y + 1 + (index - *offset) as u16, w, 1),
                    ToolAction::HistoryRow(index),
                ));
            }
            text(
                &mut spec,
                profile,
                Rect::new(x, rect.bottom() - 4, w, 1),
                &format!(
                    "{} / {} · Enter opens · Double-click opens",
                    if paths.is_empty() { 0 } else { *cursor + 1 },
                    paths.len()
                ),
                false,
            );
            if let Some(path) = paths.get(*cursor) {
                text(
                    &mut spec,
                    profile,
                    Rect::new(x, rect.bottom() - 3, w, 1),
                    &tail(&display_name(&path.to_string_lossy()), w),
                    false,
                );
            }
            controls.push((
                Rect::new(x + 8, rect.bottom() - 2, 14, 1),
                ToolAction::Accept,
            ));
            controls.push((
                Rect::new(rect.right() - 24, rect.bottom() - 2, 14, 1),
                ToolAction::Cancel,
            ));
        }
        ToolDialog::Compare { .. } => {
            text(
                &mut spec,
                profile,
                Rect::new(x, rect.y + 1, w, 1),
                app.config
                    .language
                    .text("Visible files only · No recursive or content reads"),
                false,
            );
            let options = app.file_tools.comparison;
            for (index, label, value) in [
                (0, "Compare size", options.size),
                (1, "Compare time", options.time),
                (2, "Select differences (replace marks)", !options.unselect),
                (
                    3,
                    "Unselect equivalent (retain differences)",
                    options.unselect,
                ),
            ] {
                let row = Rect::new(x + 2, rect.y + 3 + index as u16 * 2, w - 4, 1);
                let label = app.config.language.text(label);
                let kind = if index < 2 {
                    ControlKind::Checkbox {
                        caption: label.into(),
                        checked: value,
                    }
                } else {
                    ControlKind::Radio {
                        caption: label.into(),
                        checked: value,
                    }
                };
                spec.control(Control::new(
                    index,
                    row,
                    kind,
                    Target::Tool(ToolAction::Choice(index)),
                ));
            }
            controls.push((
                Rect::new(x + 8, rect.bottom() - 2, 14, 1),
                ToolAction::Accept,
            ));
            controls.push((
                Rect::new(rect.right() - 24, rect.bottom() - 2, 14, 1),
                ToolAction::Cancel,
            ));
            text(
                &mut spec,
                profile,
                Rect::new(x, rect.bottom() - 3, w, 1),
                app.config
                    .language
                    .text("Tab moves · Space toggles · Enter compares · Esc cancels"),
                false,
            );
        }
        ToolDialog::Export {
            field,
            cursor,
            focus,
            paths,
            base,
        } => {
            text(
                &mut spec,
                profile,
                Rect::new(x, rect.y + 1, w, 1),
                &format!(
                    "{} entries · Existing destinations are refused",
                    paths.len()
                ),
                false,
            );
            text(
                &mut spec,
                profile,
                Rect::new(x, rect.y + 2, w, 1),
                &tail(&display_name(&base.to_string_lossy()), w),
                false,
            );
            text(
                &mut spec,
                profile,
                Rect::new(x, rect.y + 4, w, 1),
                app.config.language.text("Destination:"),
                false,
            );
            let input = Rect::new(x, rect.y + 5, w, 1);
            let column = Line::from(&field[..*cursor]).width();
            let scroll = column.saturating_sub(usize::from(w.saturating_sub(1)));
            spec.control(Control::new(
                0,
                input,
                ControlKind::ScrolledInput {
                    value: display_name(field),
                    scroll,
                },
                Target::Tool(ToolAction::Field),
            ));
            if *focus == 0 {
                spec.cursor = Some((input.x + (column - scroll) as u16, input.y));
            }
            for (index, label, chosen) in [
                (0, "Names", !app.file_tools.full_paths),
                (1, "Full paths", app.file_tools.full_paths),
            ] {
                let row = Rect::new(x + index as u16 * 25, rect.y + 7, 24, 1);
                let label = app.config.language.text(label);
                spec.control(Control::new(
                    index + 1,
                    row,
                    ControlKind::Radio {
                        caption: label.into(),
                        checked: chosen,
                    },
                    Target::Tool(ToolAction::Choice(index)),
                ));
            }
            text(
                &mut spec,
                profile,
                Rect::new(x, rect.y + 9, w, 1),
                app.config
                    .language
                    .text("UTF-8 · One entry per line · Tab moves · Esc cancels"),
                false,
            );
            controls.push((
                Rect::new(x + 8, rect.bottom() - 2, 14, 1),
                ToolAction::Accept,
            ));
            controls.push((
                Rect::new(rect.right() - 24, rect.bottom() - 2, 14, 1),
                ToolAction::Cancel,
            ));
        }
        ToolDialog::Busy => text(
            &mut spec,
            profile,
            Rect::new(x, rect.y + 2, w, 2),
            "Writing file list…\nPlease wait for exclusive publication.",
            false,
        ),
    }
    for (row, action) in controls {
        let ids = match modal {
            ToolDialog::History { .. } => (1, 2),
            ToolDialog::Compare { .. } => (4, 5),
            ToolDialog::Export { .. } => (3, 4),
            _ => continue,
        };
        if matches!(action, ToolAction::Accept | ToolAction::Cancel) {
            let accept = action == ToolAction::Accept;
            let mut control = Control::new(
                if accept { ids.0 } else { ids.1 },
                row,
                ControlKind::Button(
                    app.config
                        .language
                        .text(if accept { " [ OK ] " } else { " [ Cancel ] " })
                        .into(),
                ),
                Target::Tool(action),
            );
            control.enabled =
                !(accept && matches!(modal,ToolDialog::History {paths,..} if paths.is_empty()));
            spec.control(control);
        } else {
            spec.hit(row, Target::Tool(action));
        }
    }
    Some(spec)
}

fn text(
    spec: &mut DialogSpec<'_>,
    profile: crate::palette::PaletteProfile,
    rect: Rect,
    value: &str,
    focused: bool,
) {
    let dos = profile.styles();
    spec.caption(
        Paragraph::new(value.to_owned()).style(dos(if focused { 0x30 } else { 0x70 })),
        rect,
    );
}
