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
    let modal = app.file_tools()?;
    if let ToolDialog::Report(report) = modal {
        return Some(report_spec(app, area, report));
    }
    let (title, height) = match modal {
        ToolDialog::History { kind, .. } => (kind.title(), area.height.saturating_sub(4).min(22)),
        ToolDialog::Compare { .. } => (" Compare directories ", 15),
        ToolDialog::Export { .. } => (" Make list file ", 13),
        ToolDialog::Busy => (" Make list file ", 7),
        ToolDialog::Report(_) => unreachable!(),
    };
    let width = area.width.saturating_sub(6).min(94);
    let rect = crate::tool_windows::bounds(app, area, width, height);
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
            spec.control(Control::new(
                6,
                Rect::new(rect.x + rect.width / 2 - 9, rect.bottom() - 2, 18, 1),
                ControlKind::Button(app.config.language.text("Report…").into()),
                Target::Tool(ToolAction::Report),
            ));
            text(
                &mut spec,
                profile,
                Rect::new(x, rect.y + 1, w, 1),
                app.config
                    .language
                    .text("Quick marks: visible metadata · Report: recursive byte comparison"),
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
        ToolDialog::Report(_) => unreachable!(),
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
    let dos = profile.styles_for(crate::palette::theme::Group::Dialogs);
    spec.caption(
        Paragraph::new(value.to_owned()).style(dos(if focused { 0x30 } else { 0x70 })),
        rect,
    );
}

fn report_spec<'a>(
    app: &'a App,
    area: Rect,
    report: &crate::file_tools::report::Report,
) -> DialogSpec<'a> {
    let rect = crate::tool_windows::bounds(
        app,
        area,
        area.width.saturating_sub(6).min(100),
        area.height.saturating_sub(4).min(28),
    );
    let lang = app.config.language;
    let x = rect.x + 2;
    let w = rect.width.saturating_sub(4);
    let mut spec = DialogSpec::new(
        rect,
        scope(app, crate::focus::Focus::FileTools).expect("comparison document"),
        report.focus,
        lang.text("Directory comparison report"),
        Target::Tool(ToolAction::Accept),
        Target::Tool(ToolAction::Cancel),
    );
    let roots = format!(
        "{}: {}  |  {}: {}",
        lang.text("Left"),
        display_name(&report.roots[0].to_string_lossy()),
        lang.text("Right"),
        display_name(&report.roots[1].to_string_lossy())
    );
    text(
        &mut spec,
        app.config.palette,
        Rect::new(x, rect.y + 1, w, 1),
        &roots,
        false,
    );
    text(
        &mut spec,
        app.config.palette,
        Rect::new(x, rect.y + 2, w, 1),
        lang.text("Recursive byte comparison · Includes hidden entries · Read-only"),
        false,
    );
    let rows = usize::from(rect.height.saturating_sub(11)).max(1);
    let items = report
        .result
        .entries
        .iter()
        .map(|row| {
            format!(
                "{:<16} {}",
                lang.text(row.status.label()),
                display_name(&row.path.to_string_lossy())
            )
        })
        .collect();
    let list_bounds = Rect::new(x, rect.y + 3, w, rows as u16);
    let mut list = Control::new(
        0,
        list_bounds,
        ControlKind::List {
            items,
            cursor: report.cursor,
            offset: report.offset,
        },
        Target::Tool(ToolAction::Accept),
    );
    list.hits = (report.offset..report.result.entries.len().min(report.offset + rows))
        .map(|index| {
            (
                Rect::new(x, list_bounds.y + (index - report.offset) as u16, w, 1),
                Target::Tool(ToolAction::ReportRow(index)),
            )
        })
        .collect();
    list.enter_action = true;
    list.dn_list = true;
    spec.control(list);
    let status = if report.busy() {
        format!("{}: {}", lang.text("Comparing entries"), report.scanned())
    } else {
        lang.text(&report.result.notice).into()
    };
    text(
        &mut spec,
        app.config.palette,
        Rect::new(x, rect.bottom() - 7, w, 1),
        &status,
        false,
    );
    use crate::file_tools::report::Status;
    for (index, statuses) in [
        &[Status::Left, Status::Right][..],
        &[Status::Different, Status::Identical, Status::Unknown][..],
    ]
    .into_iter()
    .enumerate()
    {
        let totals = statuses
            .iter()
            .map(|status| {
                format!(
                    "{}: {}",
                    lang.text(status.label()),
                    report
                        .result
                        .entries
                        .iter()
                        .filter(|r| r.status == *status)
                        .count()
                )
            })
            .collect::<Vec<_>>()
            .join(" · ");
        text(
            &mut spec,
            app.config.palette,
            Rect::new(x, rect.bottom() - 6 + index as u16, w, 1),
            &totals,
            false,
        );
    }
    if let Some(row) = report.result.entries.get(report.cursor) {
        let detail = if row.cause.is_empty() {
            display_name(&row.path.to_string_lossy())
        } else {
            format!(
                "{}: {}",
                display_name(&row.path.to_string_lossy()),
                display_name(lang.text(&row.cause))
            )
        };
        text(
            &mut spec,
            app.config.palette,
            Rect::new(x, rect.bottom() - 4, w, 1),
            &detail,
            false,
        );
    }
    let selected = report.result.entries.get(report.cursor);
    for (id, label, action, enabled) in [
        (
            1,
            "Open left",
            ToolAction::ReportOpen(0),
            selected.is_some_and(|r| r.present[0]) && !app.panels[0].loading,
        ),
        (
            2,
            "Open right",
            ToolAction::ReportOpen(1),
            selected.is_some_and(|r| r.present[1]) && !app.panels[1].loading,
        ),
        (3, "Stop", ToolAction::ReportStop, report.busy()),
        (4, "Close", ToolAction::Cancel, true),
    ] {
        let width = w / 4;
        let mut c = Control::new(
            id,
            Rect::new(
                x + (id as u16 - 1) * width,
                rect.bottom() - 2,
                width.saturating_sub(2),
                1,
            ),
            ControlKind::Button(lang.text(label).into()),
            Target::Tool(action),
        );
        c.enabled = enabled;
        spec.control(c);
    }
    spec
}
