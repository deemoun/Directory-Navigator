use crate::painting::{Block, Frame, Paragraph};
use crate::presentation::{
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
};
use crate::{app::App, filesystem::display_name, formatting::tail, mouse::Target, viewer::Action};
pub(super) fn draw_viewer(frame: &mut impl Frame, app: &App, area: Rect) {
    let profile = app.config.palette;
    let dos = profile.styles();
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
        Constraint::Length(1),
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
            Paragraph::new(text).style(dos(0x71)),
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
        let line = viewer.anchor().0 + 1;
        frame.render_widget(
            Paragraph::new(format!(
                " {} │ {line}/{} lines{} · {}",
                if contents.binary {
                    "Hex"
                } else if viewer.wrap {
                    "Text Wrap"
                } else {
                    "Text"
                },
                contents.lines.len(),
                if contents.truncated {
                    " [bounded preview]"
                } else {
                    ""
                },
                viewer.notice
            ))
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
    let cells = Layout::horizontal([
        Constraint::Length(12),
        Constraint::Length(12),
        Constraint::Length(16),
        Constraint::Min(1),
    ])
    .split(regions[3]);
    for (i, label, action) in [
        (0, "F2 Wrap", Action::Wrap),
        (1, "F7 Find", Action::Find),
        (2, "S-F7 Next", Action::Next),
        (3, "C-F7 Reverse", Action::Reverse),
    ] {
        let enabled = viewer.text_ready();
        let line = if profile == crate::palette::PaletteProfile::ClassicDn
            || app.config.shortcuts.is_mac()
        {
            let (key, description) = label.split_once(' ').unwrap_or((label, ""));
            Line::from(vec![
                Span::styled(
                    format!(
                        "{} ",
                        crate::shortcuts::label(
                            app.config.shortcuts,
                            crate::focus::Screen::Viewer,
                            key
                        )
                    ),
                    dos(if enabled { 0x74 } else { 0x78 }),
                ),
                Span::styled(app.config.language.text(description), profile.bar(enabled)),
            ])
        } else {
            Line::from(Span::styled(
                app.config.language.text(label),
                profile.bar(enabled),
            ))
        };
        frame.render_widget(Paragraph::new(line).style(profile.bar(enabled)), cells[i]);
        if enabled {
            frame.regions().push((cells[i], Target::Viewer(action)));
        }
    }
}
pub(super) fn draw_search(frame: &mut impl Frame, app: &App, area: Rect) {
    let profile = app.config.palette;
    let dos = profile.styles();
    let Some(viewer) = app.desktop.active_viewer() else {
        return;
    };
    if let Some(dialog) = viewer.modals.viewer_search() {
        let width = 58;
        let rect = super::dialog_style::window(
            frame,
            profile,
            area,
            width,
            10,
            app.config.language.text(" Viewer search — loaded text "),
        );
        frame.regions().clear();
        super::dialog_style::close_button(frame, profile, rect, Target::Viewer(Action::Cancel));
        let x = rect.x + 2;
        let w = width - 4;
        let input = Rect::new(x, rect.y + 2, w, 1);
        let column = Line::from(&dialog.memory.query[..dialog.cursor]).width();
        let scroll = column.saturating_sub(w.saturating_sub(1) as usize);
        frame.render_widget(
            Paragraph::new(display_name(&dialog.memory.query))
                .scroll((0, scroll as u16))
                .style(super::dialog_style::input_style(profile, dialog.focus == 0)),
            input,
        );
        frame.regions().push((input, Target::Viewer(Action::Field)));
        if dialog.focus == 0 && app.dialogs().is_empty() {
            frame.set_cursor_position((input.x + (column - scroll) as u16, input.y));
        }
        let case = Rect::new(x, rect.y + 4, w, 1);
        frame.render_widget(
            Paragraph::new(format!(
                "[{}] Case-sensitive",
                if dialog.memory.case_sensitive {
                    'x'
                } else {
                    ' '
                }
            ))
            .style(dos(if dialog.focus == 1 { 0x30 } else { 0x70 })),
            case,
        );
        frame.regions().push((case, Target::Viewer(Action::Case)));
        frame.render_widget(
            Paragraph::new(display_name(&viewer.notice)).style(dos(0x70)),
            Rect::new(x, rect.y + 6, w, 2),
        );
        for (label, action, focus, x) in [
            ("[ Find ]", Action::Submit, 2, x + 8),
            ("[ Cancel ]", Action::Cancel, 3, rect.right() - 24),
        ] {
            let row = Rect::new(x, rect.bottom() - 2, 16, 1);
            super::dialog_style::button(
                frame,
                profile,
                row,
                app.config.language.text(label),
                dialog.focus == focus,
            );
            frame.regions().push((row, Target::Viewer(action)));
        }
    }
}

pub(super) fn draw_text(
    frame: &mut impl Frame,
    viewer: &crate::viewer::Viewer,
    area: Rect,
    profile: crate::palette::PaletteProfile,
    language: crate::localization::Language,
) {
    let dos = profile.styles();
    if let Some(contents) = &viewer.contents {
        let lines = viewer
            .rows
            .iter()
            .skip(viewer.offset)
            .take(area.height as usize)
            .map(|row| {
                let text = &contents.lines[row.line];
                let range = &row.range;
                let highlight = viewer.current.as_ref().filter(|m| {
                    m.line == row.line && m.range.start < range.end && m.range.end > range.start
                });
                if let Some(found) = highlight {
                    let start = found.range.start.max(range.start);
                    let end = found.range.end.min(range.end);
                    Line::from(vec![
                        Span::raw(&text[range.start..start]),
                        Span::styled(&text[start..end], dos(0x30)),
                        Span::raw(&text[end..range.end]),
                    ])
                } else {
                    Line::from(&text[range.clone()])
                }
            })
            .collect::<Vec<_>>();
        frame.render_widget(
            Paragraph::new(lines)
                .scroll((0, if viewer.wrap { 0 } else { viewer.horizontal }))
                .style(dos(0x1f)),
            area,
        );
    } else {
        frame.render_widget(
            Paragraph::new(language.text("Reading file…")).style(dos(0x1f)),
            area,
        );
    }
}
