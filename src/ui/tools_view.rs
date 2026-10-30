use crate::dialog_theme as theme;
use crate::painting::{Frame, Paragraph};
use crate::presentation::{layout::Rect, text::Line};
use crate::turbo::dialog_spec::{scope, Control, ControlKind, DialogSpec};
use crate::{app::App, columns::ColumnPreset, filesystem::display_name, mouse::Target};
pub(super) fn draw_tools(
    frame: &mut impl Frame,
    app: &App,
    area: Rect,
    layer: crate::focus::Focus,
) {
    if let Some(s) = tools_spec(app, area, layer) {
        s.paint(frame, app);
    }
}
pub(crate) fn tools_spec(
    app: &App,
    area: Rect,
    layer: crate::focus::Focus,
) -> Option<DialogSpec<'_>> {
    let profile = app.config.palette;
    let dos = profile.styles_for(crate::palette::theme::Group::Dialogs);
    if let Some(pattern) = app
        .modals
        .pattern()
        .filter(|_| layer == crate::focus::Focus::Pattern)
    {
        let rect = crate::tool_windows::bounds(app, area, 58, 8);
        let mut spec = DialogSpec::new(
            rect,
            scope(app, layer)?,
            pattern.focus,
            app.config.language.text(pattern.title()),
            Target::AcceptPattern,
            Target::CancelPattern,
        );
        spec.caption(
            Paragraph::new(app.config.language.text("Name pattern")).style(dos(theme::FRAME)),
            Rect::new(rect.x + 2, rect.y + 1, rect.width - 4, 1),
        );
        let input = Rect::new(rect.x + 2, rect.y + 2, rect.width - 4, 1);
        let cursor = Line::from(&pattern.field[..pattern.cursor]).width();
        let scroll = cursor.saturating_sub(input.width.saturating_sub(1) as usize);
        spec.control(Control::new(
            0,
            input,
            if pattern.replace_on_type {
                ControlKind::SelectedInput(crate::turbo::widgets::pan(&pattern.field, scroll))
            } else {
                ControlKind::ScrolledInput {
                    value: pattern.field.clone(),
                    scroll,
                }
            },
            Target::PatternField,
        ));
        if pattern.focus == 0 {
            spec.cursor = Some((input.x + (cursor - scroll) as u16, input.y));
        }
        spec.caption(
            Paragraph::new(app.config.language.text(
                "* any sequence · ? one character\nVisible files and folders only · case-sensitive",
            ))
            .style(dos(theme::TEXT)),
            Rect::new(rect.x + 2, rect.y + 3, rect.width - 4, 2),
        );
        for (id, label, x, target) in [
            (1, " [ OK ] ", rect.x + 15, Target::AcceptPattern),
            (2, " [ Cancel ] ", rect.right() - 28, Target::CancelPattern),
        ] {
            spec.control(Control::new(
                id,
                Rect::new(x, rect.bottom() - 2, 14, 1),
                ControlKind::Button(app.config.language.text(label).into()),
                target,
            ));
        }
        return Some(spec);
    }
    if let Some(dialog) = app
        .modals
        .columns_dialog()
        .filter(|_| layer == crate::focus::Focus::Columns)
    {
        let rect = crate::tool_windows::bounds(app, area, 74, 13);
        let mut spec = DialogSpec::new(
            rect,
            scope(app, layer)?,
            dialog.cursor,
            app.config.language.text(" Setup columns "),
            Target::AcceptColumns,
            Target::CancelColumns,
        );

        for panel in 0..2 {
            let x = rect.x + 3 + panel as u16 * 35;
            spec.caption(
                Paragraph::new(app.config.language.text(if panel == 0 {
                    "Left panel"
                } else {
                    "Right panel"
                }))
                .style(dos(theme::HEADING)),
                Rect::new(x, rect.y + 2, 31, 1),
            );
            for (row, preset) in ColumnPreset::ALL.into_iter().enumerate() {
                let checked = app.config.columns[panel] == preset;
                let choice = Rect::new(x, rect.y + 4 + row as u16, 31, 1);
                if checked {
                    spec.control(Control::new(
                        panel,
                        choice,
                        ControlKind::Radio {
                            caption: app.config.language.text(preset.label()).into(),
                            checked,
                        },
                        Target::ColumnChoice(panel, preset),
                    ));
                } else {
                    spec.native(
                        crate::turbo::widgets::radio(
                            app.config.language.text(preset.label()),
                            false,
                            false,
                        ),
                        choice,
                    );
                    spec.hit(choice, Target::ColumnChoice(panel, preset));
                }
            }
        }
        spec.caption(
            Paragraph::new(
                app.config
                    .language
                    .text("Tab chooses panel · Arrows change · Esc restores"),
            )
            .style(dos(theme::TEXT)),
            Rect::new(rect.x + 3, rect.y + 9, rect.width - 6, 1),
        );
        for (id, label, x, target) in [
            (2, " [ OK ] ", rect.x + 22, Target::AcceptColumns),
            (3, " [ Cancel ] ", rect.x + 40, Target::CancelColumns),
        ] {
            spec.control(Control::new(
                id,
                Rect::new(x, rect.bottom() - 2, 14, 1),
                ControlKind::Button(app.config.language.text(label).into()),
                target,
            ));
        }
        return Some(spec);
    }
    if let Some(info) = app
        .file_info()
        .filter(|_| layer == crate::focus::Focus::FileInfo)
    {
        let rect = crate::tool_windows::bounds(app, area, area.width.min(76), area.height.min(21));
        let mut spec = DialogSpec::new(
            rect,
            scope(app, layer)?,
            0,
            app.config
                .language
                .text(" File Attributes / Info (read-only) "),
            Target::CloseFileInfo,
            Target::CloseFileInfo,
        );
        let lines: std::borrow::Cow<'_, [String]> = match &info.contents {
            Some(Ok(contents)) => std::borrow::Cow::Borrowed(&contents.lines),
            Some(Err(error)) => std::borrow::Cow::Owned(vec![
                "Cannot read file information.".into(),
                format!("Full path: {}", display_name(&info.path.to_string_lossy())),
                format!("Error: {}", display_name(error)),
            ]),
            None => std::borrow::Cow::Owned(vec!["Reading metadata…".into()]),
        };
        let visible: Vec<String> = lines
            .iter()
            .skip(info.offset)
            .take(rect.height.saturating_sub(4) as usize)
            .map(|line| app.config.language.field(line, app.config.datetime))
            .collect();
        spec.native(
            crate::turbo::widgets::text_view(&visible, info.horizontal as usize, 0x70),
            Rect::new(rect.x + 2, rect.y + 1, rect.width - 4, rect.height - 4),
        );
        spec.caption(
            Paragraph::new(
                app.config
                    .language
                    .text("↑↓ / PgUp/PgDn scroll · ←→ long lines · Home resets"),
            )
            .style(dos(theme::TEXT)),
            Rect::new(rect.x + 2, rect.bottom() - 3, rect.width - 4, 1),
        );
        spec.control(Control::new(
            0,
            Rect::new(rect.x + (rect.width - 14) / 2, rect.bottom() - 2, 14, 1),
            ControlKind::Button(app.config.language.text(" [ Close ] ").into()),
            Target::CloseFileInfo,
        ));
        return Some(spec);
    }
    None
}
