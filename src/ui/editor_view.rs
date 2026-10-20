use crate::{
    app::App, editor::EditorAction as A, filesystem::display_name, formatting, mouse::Target,
};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::{Block, Paragraph},
    Frame,
};
use unicode_segmentation::UnicodeSegmentation;

pub(super) fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    let profile = app.config.palette;
    let dos = profile.styles();
    super::editor_menu::draw_bar(frame, app, Rect::new(area.x, area.y, area.width, 1));
    let Some(editor) = &mut app.editor else {
        return;
    };
    let path = editor
        .path
        .as_ref()
        .map(|path| display_name(&path.to_string_lossy()))
        .unwrap_or_else(|| "[New file]".into());
    let title = format!(
        " F4 Editor {} │ {}",
        if editor.dirty() { "*" } else { " " },
        formatting::tail(&path, area.width.saturating_sub(29))
    );
    super::dialog_style::document_header(
        frame,
        profile,
        Rect::new(area.x, area.y + 1, area.width, 1),
        &title,
        &mut app.mouse.regions,
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
                    app.mouse.regions.push((
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
                app.mouse.regions.push((
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
            && editor.modal.is_none()
            && editor.menu.is_none()
            && app.dialogs.is_empty()
        {
            frame.set_cursor_position((
                content.x + gutter + (buffer.column() - editor.scroll_column) as u16,
                content.y + (buffer.row() - editor.scroll_row) as u16,
            ));
        }
    } else {
        frame.render_widget(Paragraph::new("Reading file…").style(dos(0x1f)), content);
    }
    let position = editor
        .buffer
        .as_ref()
        .map(|buffer| format!("Ln {} Col {}", buffer.row() + 1, buffer.column() + 1))
        .unwrap_or_default();
    let status = format!(
        " {position} │ {} │ {}",
        editor.highlight.language.label(),
        editor.notice
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
        frame.render_widget(
            Paragraph::new(profile.function(index as u8 + 1, name, command.is_some()))
                .style(profile.bar(command.is_some())),
            cells[index],
        );
        if let Some(command) = command {
            app.mouse
                .regions
                .push((cells[index], Target::Editor(command)));
        }
    }
    if editor.modal.is_some() {
        super::editor_dialogs::draw(frame, app, area);
    } else if editor.menu.is_some() {
        super::editor_menu::draw_dropdown(frame, app, area);
    }
}
