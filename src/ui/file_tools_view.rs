use crate::painting::{Frame, Paragraph};
use crate::presentation::{layout::Rect, text::Line};
use crate::{
    app::App,
    file_tools::{ToolAction, ToolDialog},
    filesystem::display_name,
    formatting::tail,
    mouse::Target,
};

pub(super) fn draw(frame: &mut impl Frame, app: &App, area: Rect) {
    let profile = app.config.palette;
    let Some(modal) = app.file_tools.modals.file_tools() else {
        return;
    };
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
    super::dialog_style::window(
        frame,
        profile,
        rect,
        rect.width,
        rect.height,
        app.config.language.text(title),
    );
    frame.regions().clear();
    if !matches!(modal, ToolDialog::Busy) {
        super::dialog_style::close_button(frame, profile, rect, Target::Tool(ToolAction::Cancel));
    }
    let mut controls = Vec::new();
    let x = rect.x + 2;
    let w = rect.width - 4;
    match modal {
        ToolDialog::History {
            paths,
            cursor,
            offset,
            rows,
            focus,
            ..
        } => {
            if paths.is_empty() {
                text(
                    frame,
                    profile,
                    Rect::new(x, rect.y + 2, w, 1),
                    app.config.language.text("No history in this session."),
                    false,
                );
            }
            if !paths.is_empty() {
                frame.render_widget(
                    crate::turbo::widgets::list(
                        paths
                            .iter()
                            .map(|p| tail(&display_name(&p.to_string_lossy()), w))
                            .collect(),
                        *cursor,
                        *offset,
                        *focus == 0,
                    ),
                    Rect::new(x, rect.y + 1, w, *rows as u16),
                );
            }
            for index in *offset..paths.len().min(*offset + *rows) {
                controls.push((
                    Rect::new(x, rect.y + 1 + (index - *offset) as u16, w, 1),
                    ToolAction::HistoryRow(index),
                ));
            }
            text(
                frame,
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
                    frame,
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
        ToolDialog::Compare { focus } => {
            text(
                frame,
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
                frame.render_widget(
                    if index < 2 {
                        crate::turbo::widgets::checkbox(label, value, *focus == index, true)
                    } else {
                        crate::turbo::widgets::radio(label, value, *focus == index)
                    },
                    row,
                );
                controls.push((row, ToolAction::Choice(index)));
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
                frame,
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
                frame,
                profile,
                Rect::new(x, rect.y + 1, w, 1),
                &format!(
                    "{} entries · Existing destinations are refused",
                    paths.len()
                ),
                false,
            );
            text(
                frame,
                profile,
                Rect::new(x, rect.y + 2, w, 1),
                &tail(&display_name(&base.to_string_lossy()), w),
                false,
            );
            text(
                frame,
                profile,
                Rect::new(x, rect.y + 4, w, 1),
                app.config.language.text("Destination:"),
                false,
            );
            let input = Rect::new(x, rect.y + 5, w, 1);
            let column = Line::from(&field[..*cursor]).width();
            let scroll = column.saturating_sub(usize::from(w.saturating_sub(1)));
            frame.render_widget(
                crate::turbo::widgets::input_scrolled(&display_name(field), scroll, *focus == 0),
                input,
            );
            controls.push((input, ToolAction::Field));
            if *focus == 0 {
                frame.set_cursor_position((input.x + (column - scroll) as u16, input.y));
            }
            for (index, label, chosen) in [
                (0, "Names", !app.file_tools.full_paths),
                (1, "Full paths", app.file_tools.full_paths),
            ] {
                let row = Rect::new(x + index as u16 * 25, rect.y + 7, 24, 1);
                let label = app.config.language.text(label);
                text(
                    frame,
                    profile,
                    row,
                    &format!("({}) {label}", if chosen { '●' } else { ' ' }),
                    *focus == index + 1,
                );
                controls.push((row, ToolAction::Choice(index)));
            }
            text(
                frame,
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
            frame,
            profile,
            Rect::new(x, rect.y + 2, w, 2),
            "Writing file list…\nPlease wait for exclusive publication.",
            false,
        ),
    }
    for (row, action) in controls {
        let focus = match modal {
            ToolDialog::History { focus, .. } => Some((*focus, 1, 2)),
            ToolDialog::Compare { focus } => Some((*focus, 4, 5)),
            ToolDialog::Export { focus, .. } => Some((*focus, 3, 4)),
            _ => None,
        };
        if action == ToolAction::Accept
            && matches!(modal, ToolDialog::History { paths, .. } if paths.is_empty())
        {
            frame.render_widget(
                crate::turbo::widgets::disabled_button(app.config.language.text(" [ OK ] ")),
                row,
            );
            continue;
        }
        match action {
            ToolAction::Accept => super::dialog_style::button_with_default(
                frame,
                profile,
                row,
                app.config.language.text(" [ OK ] "),
                focus.is_some_and(|(f, a, _)| f == a),
                focus.map_or_else(
                    || matches!(modal, ToolDialog::History { paths, .. } if !paths.is_empty()),
                    |(f, a, _)| f < a,
                ),
            ),
            ToolAction::Cancel => super::dialog_style::button(
                frame,
                profile,
                row,
                app.config.language.text(" [ Cancel ] "),
                focus.is_some_and(|(f, _, c)| f == c),
            ),
            _ => {}
        }
        frame.regions().push((row, Target::Tool(action)));
    }
}
fn text(
    frame: &mut impl Frame,
    profile: crate::palette::PaletteProfile,
    rect: Rect,
    value: &str,
    focused: bool,
) {
    let dos = profile.styles();
    frame.render_widget(
        Paragraph::new(value).style(dos(if focused { 0x30 } else { 0x70 })),
        rect,
    );
}
