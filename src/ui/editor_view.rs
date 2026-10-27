use crate::painting::{Block, Frame, Paragraph};
use crate::presentation::{
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
};
use crate::{
    app::App, editor::EditorAction as A, filesystem::display_name, formatting, mouse::Target,
};
use unicode_segmentation::UnicodeSegmentation;

pub(super) fn draw(frame: &mut impl Frame, app: &App, area: Rect) {
    let prefix_armed = crate::shortcuts::armed(app);
    let profile = app.config.palette;
    let dos = profile.styles();
    super::editor_menu::draw_bar(frame, app, Rect::new(area.x, area.y, area.width, 1));
    let Some(editor) = app.desktop.active_editor() else {
        return;
    };
    let path = editor
        .path
        .as_ref()
        .map(|path| display_name(&path.to_string_lossy()))
        .unwrap_or_else(|| app.config.language.text("[New file]").into());
    let title = format!(
        " {} {} {} │ {}",
        crate::shortcuts::label(app.config.shortcuts, crate::focus::Screen::Editor, "F4"),
        app.config.language.text("Editor"),
        if editor.dirty() { "*" } else { " " },
        formatting::tail(
            &path,
            area.width.saturating_sub(
                27 + crate::shortcuts::label(
                    app.config.shortcuts,
                    crate::focus::Screen::Editor,
                    "F4"
                )
                .len() as u16
            )
        )
    );
    super::dialog_style::document_header(
        frame,
        profile,
        Rect::new(area.x, area.y + 1, area.width, 1),
        &title,
        Target::Editor(A::Close),
    );
    let content = Rect::new(area.x, area.y + 2, area.width, area.height - 4);
    frame.render_widget(Block::default().style(dos(0x1f)), content);
    let gutter = crate::layout::editor_gutter(editor);
    if let Some(buffer) = &editor.buffer {
        for row in editor.scroll_row..buffer.lines.len().min(editor.scroll_row + editor.rows) {
            let y = content.y + (row - editor.scroll_row) as u16;
            frame.render_widget(
                Paragraph::new(format!(
                    "{:>width$} ",
                    row + 1,
                    width = usize::from(gutter - 1)
                ))
                .style(dos(0x1b)),
                Rect::new(content.x, y, gutter, 1),
            );
            let mut column = 0;
            let mut spans = Vec::new();
            let selected = buffer.selected();
            for (byte, g) in buffer.line_text(row).grapheme_indices(true) {
                let width = buffer.cell_width(g, column);
                let start = column;
                column += width;
                if column <= editor.scroll_column {
                    continue;
                }
                let left = start.saturating_sub(editor.scroll_column);
                if left >= editor.columns {
                    break;
                }
                let visible_width = (column - editor.scroll_column).min(editor.columns) - left;
                let clipped = start < editor.scroll_column || left + width > editor.columns;
                let text = if g == "\t" || clipped {
                    " ".repeat(visible_width)
                } else {
                    g.to_owned()
                };
                let position = buffer.lines[row].start + byte;
                let color = if selected.contains(&position) {
                    0x30
                } else if editor.colors {
                    editor.highlight.color(row, byte)
                } else {
                    0x1f
                };
                spans.push(Span::styled(text, dos(color)));
                if visible_width > 0 {
                    frame.regions().push((
                        Rect::new(content.x + gutter + left as u16, y, visible_width as u16, 1),
                        Target::Editor(A::Position(position, false)),
                    ));
                }
            }
            frame.render_widget(
                Paragraph::new(Line::from(spans)).style(dos(0x1f)),
                Rect::new(content.x + gutter, y, editor.columns as u16, 1),
            );
            let end_col = buffer
                .text_width(buffer.line_text(row))
                .saturating_sub(editor.scroll_column)
                .min(editor.columns);
            if end_col < editor.columns {
                frame.regions().push((
                    Rect::new(
                        content.x + gutter + end_col as u16,
                        y,
                        (editor.columns - end_col) as u16,
                        1,
                    ),
                    Target::Editor(A::Position(buffer.lines[row].end, false)),
                ));
            }
        }
        if !editor.busy()
            & editor.modals.editor_modal().is_none()
            & editor.modals.editor_menu().is_none()
            && app.dialogs().is_empty() & app.modals.window_list().is_none()
            && !prefix_armed
        {
            frame.set_cursor_position((
                content.x + gutter + (buffer.column() - editor.scroll_column) as u16,
                content.y + (buffer.row() - editor.scroll_row) as u16,
            ));
        }
    } else {
        frame.render_widget(
            Paragraph::new(app.config.language.text("Reading file…")).style(dos(0x1f)),
            content,
        );
    }
    let position = editor
        .buffer
        .as_ref()
        .map(|buffer| {
            format!(
                "{} {} {} {}",
                app.config.language.text("Ln"),
                buffer.row() + 1,
                app.config.language.text("Col"),
                buffer.column() + 1
            )
        })
        .unwrap_or_default();
    let status = format!(
        " {position} │ {} │ {}",
        editor.highlight.language.label(),
        app.config.language.text(&editor.notice)
    );
    frame.render_widget(
        Paragraph::new(status).style(dos(0x70)),
        Rect::new(area.x, area.bottom() - 2, area.width, 1),
    );
    let cells = Layout::horizontal([Constraint::Fill(1); 10]).split(Rect::new(
        area.x,
        area.bottom() - 1,
        area.width,
        1,
    ));
    for (index, (name, command)) in [
        ("Help", Some(A::Help)),
        ("Save", Some(A::Save)),
        ("Open", Some(A::Open)),
        ("", None),
        ("", None),
        ("Dupe", Some(A::Duplicate)),
        ("Find", Some(A::Find)),
        ("", None),
        ("", None),
        ("Menu", Some(A::Menu)),
    ]
    .into_iter()
    .enumerate()
    {
        let key = if app.config.shortcuts.is_mac() {
            format!("^O{} ", (index + 1) % 10)
        } else if profile == crate::palette::PaletteProfile::ClassicDn {
            format!("F{} ", index + 1)
        } else {
            (index + 1).to_string()
        };
        frame.render_widget(
            crate::turbo::widgets::function_status(
                &key,
                app.config.language.text(name),
                command.is_some(),
            ),
            cells[index],
        );
        if let Some(command) = command {
            frame
                .regions()
                .push((cells[index], Target::Editor(command)));
        }
    }
}
