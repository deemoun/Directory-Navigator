use crate::painting::{Block, Frame, Paragraph};
use crate::presentation::{
    layout::{Constraint, Layout, Rect},
    text::Line,
};
use crate::turbo::dialog_spec::{centered, scope, Control, ControlKind, DialogSpec};
use crate::{app::App, filesystem::display_name, formatting::tail, mouse::Target, viewer::Action};
fn bar_caption<'a>(
    app: &'a App,
    viewer: &crate::viewer::Viewer,
    entry: &crate::menu::definition::BarItem<Action>,
) -> &'a str {
    app.config.language.text(
        if entry.action == Some(Action::ToggleMarkdown) && viewer.markdown.rendered {
            "Source"
        } else {
            entry.label
        },
    )
}
fn bar_width(
    app: &App,
    viewer: &crate::viewer::Viewer,
    entry: &crate::menu::definition::BarItem<Action>,
) -> u16 {
    (Line::from(
        crate::shortcuts::label(
            app.config.shortcuts,
            crate::focus::Screen::Viewer,
            entry.key,
        )
        .as_ref(),
    )
    .width()
        + Line::from(bar_caption(app, viewer, entry)).width()
        + 2)
    .min(u16::MAX as usize) as u16
}
pub(crate) fn viewer_footer_rows(app: &App, viewer: &crate::viewer::Viewer, width: u16) -> u16 {
    let mut rows = 1;
    let mut x: u16 = 0;
    for entry in crate::menu::definition::viewer_bar(viewer) {
        let length = bar_width(app, viewer, &entry).min(width);
        if x > 0 && x.saturating_add(length) > width {
            rows += 1;
            x = 0;
        }
        x = x.saturating_add(length);
    }
    rows
}
pub(super) fn draw_viewer(frame: &mut impl Frame, app: &App, area: Rect) {
    let profile = app.config.palette;
    let dos = profile.styles_for(crate::palette::theme::Group::Viewer);
    let header = super::clock::header(app, Rect::new(area.x, area.y, area.width, 1));
    let clock_text = if super::clock::visible(app) {
        Some(app.clock.text.clone())
    } else {
        None
    };
    let Some(viewer) = app.desktop.active_viewer() else {
        return;
    };
    let regions = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(viewer_footer_rows(app, viewer, area.width)),
    ])
    .split(area);
    let title = format!(
        " {} {} │ {}",
        crate::shortcuts::label(app.config.shortcuts, crate::focus::Screen::Viewer, "F3"),
        app.config.language.text("Viewer"),
        tail(
            &display_name(&viewer.path.to_string_lossy()),
            header.width.saturating_sub(
                25 + crate::shortcuts::label(
                    app.config.shortcuts,
                    crate::focus::Screen::Viewer,
                    "F3"
                )
                .len() as u16
            )
        )
    );
    super::dialog_style::document_header(frame, profile, header, &title, Target::CloseViewer);
    if let Some(text) = clock_text {
        frame.render_widget(
            Paragraph::new(text)
                .style(profile.for_group(crate::palette::theme::Group::Timer, 0x71)),
            Rect::new(
                area.right() - app.config.datetime.clock_width(),
                area.y,
                app.config.datetime.clock_width(),
                1,
            ),
        );
    }
    if let Some(contents) = &viewer.contents {
        draw_text(frame, viewer, regions[1], profile, app.config.language);
        let line = viewer.source_line() + 1;
        let pdf_status = contents.pdf.as_ref().map(|pdf| {
            if pdf.locked {
                return format!(
                    " PDF · {}",
                    app.config.language.text("This PDF requires a password.")
                );
            }
            format!(
                " PDF · {} {}/{} · {}",
                app.config.language.text("Page"),
                pdf.page(viewer.source_line()),
                pdf.page_lines.len(),
                app.config.language.text(&viewer.notice)
            )
        });
        frame.render_widget(
            Paragraph::new(pdf_status.unwrap_or_else(|| {
                format!(
                    " {} │ {line}/{} lines{} · {} · {}",
                    if viewer.markdown.rendered {
                        "Markdown"
                    } else if contents.binary {
                        "Hex"
                    } else if viewer.wrap {
                        "Text Wrap"
                    } else {
                        "Text"
                    },
                    viewer.source_line_count(),
                    if contents.truncated {
                        " [bounded preview]"
                    } else {
                        ""
                    },
                    contents.format.map(|f| f.to_string()).unwrap_or_default(),
                    app.config.language.text(&viewer.notice)
                )
            }))
            .style(dos(0x30)),
            regions[2],
        );
    } else {
        frame.render_widget(
            Paragraph::new(app.config.language.text("Reading file…")).style(dos(0x1f)),
            regions[1],
        );
    }
    frame.render_widget(Block::default().style(profile.bar(true)), regions[3]);
    let entries = crate::menu::definition::viewer_bar(viewer);
    let mut cells = Vec::new();
    let mut x = regions[3].x;
    let mut y = regions[3].y;
    for entry in &entries {
        let width = bar_width(app, viewer, entry).min(regions[3].width);
        if x > regions[3].x && x.saturating_add(width) > regions[3].right() {
            x = regions[3].x;
            y += 1;
        }
        cells.push(Rect::new(x, y, width, 1));
        x += width;
    }
    for (i, entry) in entries.iter().enumerate() {
        let enabled = entry
            .action
            .is_some_and(|action| crate::commands::viewer_available(viewer, action));
        let key = format!(
            "{} ",
            crate::shortcuts::label(
                app.config.shortcuts,
                crate::focus::Screen::Viewer,
                entry.key
            )
        );
        frame.render_widget(
            crate::turbo::widgets::function_status(&key, bar_caption(app, viewer, entry), enabled),
            cells[i],
        );
        if enabled {
            if let Some(action) = entry.action {
                frame.regions().push((cells[i], Target::Viewer(action)));
            }
        }
    }
}
pub(super) fn draw_search(frame: &mut impl Frame, app: &App, area: Rect) {
    if let Some(spec) = viewer_search_spec(app, area) {
        spec.paint(frame, app);
    }
}
pub(crate) fn viewer_search_spec(app: &App, area: Rect) -> Option<DialogSpec<'_>> {
    let dos = app
        .config
        .palette
        .styles_for(crate::palette::theme::Group::Viewer);
    let viewer = app.desktop.active_viewer()?;
    if viewer.modals.viewer_pdf_password().is_some() {
        return crate::viewer::pdf::password::spec(app, area);
    }
    if let Some(dialog) = viewer.modals.viewer_encoding() {
        return crate::text_encoding::spec(app, area, dialog, false);
    }
    if let Some(dialog) = viewer.modals.viewer_goto() {
        let rect = centered(area, 58, 10);
        let mut spec = DialogSpec::new(
            rect,
            scope(app, crate::focus::Focus::ViewerSearch)?,
            dialog.focus,
            app.config.language.text(" Viewer Go to "),
            Target::Viewer(Action::Submit),
            Target::Viewer(Action::Cancel),
        );
        let input = Rect::new(rect.x + 2, rect.y + 3, rect.width - 4, 1);
        spec.caption(
            Paragraph::new(app.config.language.text(
                if viewer.contents.as_ref().is_some_and(|c| c.pdf.is_some()) {
                    "Page number (from 1)"
                } else if viewer.contents.as_ref().is_some_and(|c| c.binary) {
                    "Byte offset (decimal or 0x hexadecimal, from 0)"
                } else {
                    "Line number (from 1)"
                },
            ))
            .style(dos(0x70)),
            Rect::new(input.x, rect.y + 1, input.width, 2),
        );
        let column = Line::from(&dialog.field[..dialog.cursor]).width();
        let scroll = column.saturating_sub(input.width.saturating_sub(1) as usize);
        spec.control(Control::new(
            0,
            input,
            ControlKind::ScrolledInput {
                value: display_name(&dialog.field),
                scroll,
            },
            Target::Viewer(Action::Field),
        ));
        if dialog.focus == 0 && app.dialogs().is_empty() {
            spec.cursor = Some((input.x + (column - scroll) as u16, input.y));
        }
        spec.caption(
            Paragraph::new(app.config.language.text(&viewer.notice)).style(dos(0x70)),
            Rect::new(input.x, rect.y + 5, input.width, 2),
        );
        for (id, label, action, x) in [
            (1, "[ Go ]", Action::Submit, rect.x + 8),
            (2, "[ Cancel ]", Action::Cancel, rect.right() - 24),
        ] {
            spec.control(Control::new(
                id,
                Rect::new(x, rect.bottom() - 2, 16, 1),
                ControlKind::Button(app.config.language.text(label).into()),
                Target::Viewer(action),
            ));
        }
        return Some(spec);
    }
    let dialog = viewer.modals.viewer_search()?;
    let width = 58;
    let rect = centered(area, width, 10);
    let mut spec = DialogSpec::new(
        rect,
        scope(app, crate::focus::Focus::ViewerSearch)?,
        dialog.focus,
        app.config.language.text(" Viewer search — loaded text "),
        Target::Viewer(Action::Submit),
        Target::Viewer(Action::Cancel),
    );
    let x = rect.x + 2;
    let w = width - 4;
    let input = Rect::new(x, rect.y + 2, w, 1);
    let column = Line::from(&dialog.memory.query[..dialog.cursor]).width();
    let scroll = column.saturating_sub(w.saturating_sub(1) as usize);
    spec.control(Control::new(
        0,
        input,
        ControlKind::ScrolledInput {
            value: display_name(&dialog.memory.query),
            scroll,
        },
        Target::Viewer(Action::Field),
    ));
    if dialog.focus == 0 && app.dialogs().is_empty() {
        spec.cursor = Some((input.x + (column - scroll) as u16, input.y));
    }
    let case = Rect::new(x, rect.y + 4, w, 1);
    spec.control(Control::new(
        1,
        case,
        ControlKind::Checkbox {
            caption: app.config.language.text("Case-sensitive").into(),
            checked: dialog.memory.case_sensitive,
        },
        Target::Viewer(Action::Case),
    ));
    spec.caption(
        Paragraph::new(display_name(&viewer.notice)).style(dos(0x70)),
        Rect::new(x, rect.y + 6, w, 2),
    );
    for (label, action, focus, x) in [
        ("[ Find ]", Action::Submit, 2, x + 8),
        ("[ Cancel ]", Action::Cancel, 3, rect.right() - 24),
    ] {
        let row = Rect::new(x, rect.bottom() - 2, 16, 1);
        spec.control(Control::new(
            focus,
            row,
            ControlKind::Button(app.config.language.text(label).into()),
            Target::Viewer(action),
        ));
    }
    Some(spec)
}

pub(super) fn draw_text(
    frame: &mut impl Frame,
    viewer: &crate::viewer::Viewer,
    area: Rect,
    profile: crate::palette::PaletteProfile,
    language: crate::localization::Language,
) {
    let dos = profile.styles_for(crate::palette::theme::Group::Viewer);
    if let Some(contents) = &viewer.contents {
        use unicode_width::UnicodeWidthStr;
        let visual = viewer
            .rows
            .iter()
            .skip(viewer.offset)
            .take(area.height as usize)
            .collect::<Vec<_>>();
        let lines = visual
            .iter()
            .map(|row| contents.lines[row.line][row.range.clone()].to_owned())
            .collect::<Vec<_>>();
        let horizontal = if viewer.wrap {
            0
        } else {
            viewer.horizontal as usize
        };
        if viewer.markdown.rendered {
            use crate::presentation::text::Span;
            use unicode_segmentation::UnicodeSegmentation;
            let doc = viewer.markdown.document.as_ref().unwrap();
            for (y, row) in visual.iter().enumerate() {
                let logical = &doc.rows[row.line];
                let pan = if !viewer.wrap || logical.fixed {
                    viewer.horizontal as usize
                } else {
                    0
                };
                let mut cells = 0usize;
                let mut spans = Vec::new();
                for (byte, g) in logical.text[row.range.clone()].grapheme_indices(true) {
                    let width = g.width();
                    if cells >= pan + area.width as usize {
                        break;
                    }
                    if cells + width > pan {
                        let at = byte + row.range.start;
                        let role = logical
                            .runs
                            .get(logical.runs.partition_point(|r| r.range.end <= at))
                            .filter(|r| r.range.contains(&at))
                            .map_or(crate::markdown::Role::Text, |r| r.role);
                        let text = if cells < pan {
                            " ".repeat(cells + width - pan)
                        } else {
                            g.to_string()
                        };
                        spans.push(Span::styled(text, dos(role.color())));
                    }
                    cells += width;
                }
                frame.render_widget(
                    Paragraph::new(Line::from(spans)).style(dos(0x1f)),
                    Rect::new(area.x, area.y + y as u16, area.width, 1),
                );
            }
        } else {
            frame.render_widget(
                crate::turbo::widgets::text_view(&lines, horizontal, 0x1f)
                    .themed(crate::palette::theme::Group::Viewer),
                area,
            );
        }
        for (y, row) in visual.iter().enumerate() {
            let horizontal = if !viewer.wrap || viewer.fixed_row(row.line) {
                viewer.horizontal as usize
            } else {
                0
            };
            if let Some(found) = viewer.current.as_ref().filter(|m| {
                m.line == row.line && m.range.start < row.range.end && m.range.end > row.range.start
            }) {
                let text = &contents.lines[row.line];
                let start = found.range.start.max(row.range.start);
                let end = found.range.end.min(row.range.end);
                let column = text[row.range.start..start].width();
                let width = text[start..end].width();
                if column + width <= horizontal || column >= horizontal + area.width as usize {
                    continue;
                }
                let clipped = horizontal.saturating_sub(column);
                let value = crate::turbo::widgets::pan(&text[start..end], clipped);
                let x = column.saturating_sub(horizontal) as u16;
                let width = (width - clipped).min((area.width - x) as usize) as u16;
                frame.render_widget(
                    Paragraph::new(value).style(dos(0x30)),
                    Rect::new(area.x + x, area.y + y as u16, width, 1),
                );
            }
        }
    } else {
        frame.render_widget(
            Paragraph::new(language.text("Reading file…")).style(dos(0x1f)),
            area,
        );
    }
}
