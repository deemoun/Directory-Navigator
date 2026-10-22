use super::menu_view::label_spans;
use crate::{
    app::App,
    editor::{menus, EditorAction as A},
    menu::plain,
    mouse::Target,
};
use ratatui::{
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};
pub(super) fn draw_bar(frame: &mut Frame, app: &mut App, area: Rect) {
    let language = app.config.language;
    let profile = app.config.palette;
    let dos = profile.styles();
    frame.render_widget(Block::default().style(dos(0x70)), area);
    let header = super::clock::header(app, area);
    let current = app.editor.as_ref().and_then(|e| e.menu);
    let mut x = area.x;
    for (root, label) in menus::LABELS.iter().enumerate() {
        let label = language.text(label);
        let rect = Rect::new(
            x,
            area.y,
            Line::from(plain(language.text(label))).width() as u16 + 3,
            1,
        )
        .intersection(header);
        let normal = dos(if !menus::enabled(root) {
            0x78
        } else if current.is_some_and(|(r, _)| r == root) {
            0x30
        } else {
            0x70
        });
        let hot = if menus::enabled(root) {
            dos(if current.is_some_and(|(r, _)| r == root) {
                0x3e
            } else {
                0x74
            })
        } else {
            normal
        };
        let mut spans = vec![Span::styled(" ", normal)];
        spans.extend(label_spans(label, normal, hot));
        spans.push(Span::styled("  ", normal));
        frame.render_widget(Paragraph::new(Line::from(spans)).style(normal), rect);
        if menus::enabled(root) {
            app.mouse
                .regions
                .push((rect, Target::Editor(A::MenuRoot(root))));
        }
        x += rect.width;
    }
    super::clock::draw(frame, app, area);
}
pub(super) fn draw_dropdown(frame: &mut Frame, app: &mut App, area: Rect) {
    let language = app.config.language;
    let profile = app.config.palette;
    let dos = profile.styles();
    let Some(editor) = &app.editor else {
        return;
    };
    let Some((root, selected)) = editor.menu else {
        return;
    };
    let entries = menus::items(root);
    let width = entries
        .iter()
        .map(|item| Line::from(plain(language.text(item.label))).width() + item.shortcut.len() + 8)
        .max()
        .unwrap_or(20)
        .min(area.width as usize) as u16;
    let x = menus::LABELS[..root]
        .iter()
        .map(|label| Line::from(plain(language.text(label))).width() as u16 + 3)
        .sum::<u16>()
        .min(area.width - width);
    let rect = Rect::new(area.x + x, area.y + 1, width, entries.len() as u16 + 2);
    let preferences = editor.preferences;
    let states = entries
        .iter()
        .map(|item| match item.action {
            Some(A::AutoIndent) => preferences.auto_indent,
            Some(A::Colors) => preferences.syntax,
            Some(A::ExpandTabs) => preferences.expand_tabs,
            _ => false,
        })
        .collect::<Vec<_>>();
    // Consume clicks outside the menu; the underlying document never receives them.
    app.mouse.regions.clear();
    app.mouse
        .regions
        .push((area, Target::Editor(A::CancelMenu)));
    draw_bar(frame, app, Rect::new(area.x, area.y, area.width, 1));
    frame.render_widget(Clear, rect);
    frame.render_widget(
        Block::default().borders(Borders::ALL).style(dos(0x70)),
        rect,
    );
    app.mouse
        .regions
        .push((rect, Target::Editor(A::MenuSurface)));
    for (row, item) in entries.iter().enumerate() {
        let row_rect = Rect::new(rect.x + 1, rect.y + 1 + row as u16, width - 2, 1);
        if item.label.is_empty() {
            frame.render_widget(
                Paragraph::new(
                    app.config
                        .language
                        .text("─")
                        .repeat(row_rect.width as usize),
                )
                .style(dos(0x70)),
                row_rect,
            );
            continue;
        }
        let normal = dos(if item.action.is_none() {
            0x78
        } else if row == selected {
            0x30
        } else {
            0x70
        });
        let hot = if item.action.is_some() {
            dos(if row == selected { 0x3e } else { 0x74 })
        } else {
            normal
        };
        let mut spans = vec![Span::styled(if states[row] { "• " } else { "  " }, normal)];
        spans.extend(label_spans(language.text(item.label), normal, hot));
        let padding = usize::from(width - 2).saturating_sub(
            2 + Line::from(plain(language.text(item.label))).width() + item.shortcut.len() + 1,
        );
        spans.push(Span::styled(" ".repeat(padding), normal));
        spans.push(Span::styled(item.shortcut, normal));
        frame.render_widget(Paragraph::new(Line::from(spans)).style(normal), row_rect);
        if item.action.is_some() {
            app.mouse
                .regions
                .push((row_rect, Target::Editor(A::MenuRow(row))));
        }
    }
}
