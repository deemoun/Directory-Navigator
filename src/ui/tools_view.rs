use crate::painting::{Frame, Paragraph};
use crate::presentation::{layout::Rect, text::Line};
use crate::{app::App, columns::ColumnPreset, filesystem::display_name, mouse::Target};

fn popup(
    frame: &mut impl Frame,
    app: &App,
    area: Rect,
    width: u16,
    height: u16,
    title: &str,
) -> Rect {
    super::dialog_style::window(
        frame,
        app.config.palette,
        area,
        width,
        height,
        app.config.language.text(title),
    )
}

fn button(
    frame: &mut impl Frame,
    app: &App,
    rect: Rect,
    text: &str,
    target: Target,
    focused: bool,
) {
    super::dialog_style::button(
        frame,
        app.config.palette,
        rect,
        app.config.language.text(text),
        focused,
    );
    frame.regions().push((rect, target));
}

pub(super) fn draw_tools(
    frame: &mut impl Frame,
    app: &App,
    area: Rect,
    layer: crate::focus::Focus,
) {
    let profile = app.config.palette;
    let dos = profile.styles();
    if let Some(pattern) = app
        .modals
        .pattern()
        .filter(|_| layer == crate::focus::Focus::Pattern)
    {
        let rect = popup(frame, app, area, 58, 8, pattern.title());
        frame.render_widget(
            Paragraph::new(app.config.language.text("Name pattern")).style(dos(0x7f)),
            Rect::new(rect.x + 2, rect.y + 1, rect.width - 4, 1),
        );
        let input = Rect::new(rect.x + 2, rect.y + 2, rect.width - 4, 1);
        let cursor = Line::from(&pattern.field[..pattern.cursor]).width();
        let scroll = cursor.saturating_sub(input.width.saturating_sub(1) as usize);
        frame.render_widget(
            Paragraph::new(pattern.field.as_str())
                .scroll((0, scroll as u16))
                .style(if pattern.replace_on_type {
                    dos(0x30)
                } else {
                    super::dialog_style::input_style(profile, true)
                }),
            input,
        );
        frame.set_cursor_position((input.x + (cursor - scroll) as u16, input.y));
        frame.render_widget(
            Paragraph::new(app.config.language.text(
                "* any sequence · ? one character\nVisible files and folders only · case-sensitive",
            ))
            .style(dos(0x70)),
            Rect::new(rect.x + 2, rect.y + 3, rect.width - 4, 2),
        );
        frame.regions().clear();
        super::dialog_style::close_button(frame, profile, rect, Target::CancelPattern);
        button(
            frame,
            app,
            Rect::new(rect.x + 15, rect.bottom() - 2, 14, 1),
            " [ OK ] ",
            Target::AcceptPattern,
            true,
        );
        button(
            frame,
            app,
            Rect::new(rect.right() - 28, rect.bottom() - 2, 14, 1),
            " [ Cancel ] ",
            Target::CancelPattern,
            true,
        );
    }
    if let Some(dialog) = app
        .modals
        .columns_dialog()
        .filter(|_| layer == crate::focus::Focus::Columns)
    {
        let rect = popup(frame, app, area, 74, 13, " Setup columns ");
        let cursor = dialog.cursor;
        frame.regions().clear();
        super::dialog_style::close_button(frame, profile, rect, Target::CancelColumns);
        for panel in 0..2 {
            let x = rect.x + 3 + panel as u16 * 35;
            frame.render_widget(
                Paragraph::new(app.config.language.text(if panel == 0 {
                    "Left panel"
                } else {
                    "Right panel"
                }))
                .style(dos(0x71)),
                Rect::new(x, rect.y + 2, 31, 1),
            );
            for (row, preset) in ColumnPreset::ALL.into_iter().enumerate() {
                let checked = app.config.columns[panel] == preset;
                let choice = Rect::new(x, rect.y + 4 + row as u16, 31, 1);
                frame.render_widget(
                    Paragraph::new(format!(
                        "({}) {}",
                        if checked { 'x' } else { ' ' },
                        app.config.language.text(preset.label())
                    ))
                    .style(dos(if cursor == panel && checked {
                        0x30
                    } else {
                        0x70
                    })),
                    choice,
                );
                frame
                    .regions()
                    .push((choice, Target::ColumnChoice(panel, preset)));
            }
        }
        frame.render_widget(
            Paragraph::new(
                app.config
                    .language
                    .text("Tab chooses panel · Arrows change · Esc restores"),
            )
            .style(dos(0x70)),
            Rect::new(rect.x + 3, rect.y + 9, rect.width - 6, 1),
        );
        button(
            frame,
            app,
            Rect::new(rect.x + 22, rect.bottom() - 2, 14, 1),
            " [ OK ] ",
            Target::AcceptColumns,
            cursor == 2,
        );
        button(
            frame,
            app,
            Rect::new(rect.x + 40, rect.bottom() - 2, 14, 1),
            " [ Cancel ] ",
            Target::CancelColumns,
            cursor == 3,
        );
    }
    if let Some(info) = app
        .modals
        .file_info()
        .filter(|_| layer == crate::focus::Focus::FileInfo)
    {
        let rect = popup(
            frame,
            app,
            area,
            area.width.min(76),
            area.height.min(21),
            " File Attributes / Info (read-only) ",
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
        let visible: Vec<Line> = lines
            .iter()
            .skip(info.offset)
            .take(rect.height.saturating_sub(4) as usize)
            .map(|line| Line::from(app.config.language.field(line, app.config.datetime)))
            .collect();
        frame.render_widget(
            Paragraph::new(visible)
                .scroll((0, info.horizontal))
                .style(dos(0x70)),
            Rect::new(rect.x + 2, rect.y + 1, rect.width - 4, rect.height - 4),
        );
        frame.render_widget(
            Paragraph::new(
                app.config
                    .language
                    .text("↑↓ / PgUp/PgDn scroll · ←→ long lines · Home resets"),
            )
            .style(dos(0x70)),
            Rect::new(rect.x + 2, rect.bottom() - 3, rect.width - 4, 1),
        );
        frame.regions().clear();
        super::dialog_style::close_button(frame, profile, rect, Target::CloseFileInfo);
        button(
            frame,
            app,
            Rect::new(rect.x + (rect.width - 14) / 2, rect.bottom() - 2, 14, 1),
            " [ Close ] ",
            Target::CloseFileInfo,
            true,
        );
    }
}
