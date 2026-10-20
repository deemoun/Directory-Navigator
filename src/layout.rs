//! Explicit viewport preparation. Painting consumes these dimensions and geometry.
use crate::{app::App, panel_layout::PanelGeometry};
use ratatui::layout::{Constraint, Layout, Rect};
pub struct Prepared {
    pub regions: [Rect; 4],
    pub panel_areas: [Rect; 2],
    pub panels: [PanelGeometry; 2],
    pub message: Option<MessageLayout>,
}
pub fn editor_gutter(editor: &crate::editor::Editor) -> u16 {
    editor
        .buffer
        .as_ref()
        .map(|b| b.lines.len().to_string().len() as u16 + 2)
        .unwrap_or(3)
}
pub fn prepare(app: &mut App, area: Rect) -> Prepared {
    if app.utilities.size != (area.width, area.height) {
        app.mouse.scrollbar_drag = None;
    }
    app.utilities.size = (area.width, area.height);
    let r = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(area);
    let regions = [r[0], r[1], r[2], r[3]];
    let p = Layout::horizontal([Constraint::Percentage(50); 2]).split(regions[1]);
    let panel_areas = [p[0], p[1]];
    let panels = std::array::from_fn(|i| {
        PanelGeometry::new(
            panel_areas[i].inner(ratatui::layout::Margin::new(1, 1)),
            app.config.layouts[i],
        )
    });
    if let Some(q) = &mut app.quick_view {
        q.layout(panel_areas[q.source ^ 1]);
    }
    if let Some(game) = &mut app.tetris {
        game.set_obscured(
            area.width < 80 || area.height < 25,
            std::time::Instant::now(),
        );
    }
    if app.screensaver.active {
        app.screensaver.resize(area.width, area.height);
    }
    if area.width >= 80 && area.height >= 25 {
        match crate::focus::screen(app) {
            crate::focus::Screen::Editor => {
                if let Some(e) = &mut app.editor {
                    e.rows = usize::from(area.height - 4).max(1);
                    e.columns = usize::from(area.width.saturating_sub(editor_gutter(e))).max(1);
                    e.keep_visible();
                }
            }
            crate::focus::Screen::Viewer => {
                app.page_rows = regions[1].height as usize;
                if let Some(v) = &mut app.viewer {
                    v.layout(regions[1].width);
                }
            }
            _ => {
                app.page_rows = usize::from(panel_areas[0].height.saturating_sub(6));
                for (p, g) in app.panels.iter_mut().zip(&panels) {
                    p.keep_visible_grid(g.rows(), g.columns.len());
                }
            }
        }
    }
    if area.width >= 80
        && area.height >= 25
        && crate::focus::screen(app) == crate::focus::Screen::Manager
    {
        if let Some(b) = &mut app.archive {
            b.rows = usize::from(area.height.saturating_sub(4).min(30).saturating_sub(9)).max(1);
            b.keep_visible();
        }
        if let Some(m) = &mut app.user_menu.modal {
            let height = if m.review.is_some() {
                area.height.saturating_sub(4).min(26)
            } else {
                (m.items().len() + 7)
                    .clamp(10, 24)
                    .min(area.height.saturating_sub(4) as usize) as u16
            };
            m.rows = usize::from(height.saturating_sub(7)).max(1);
            m.keep_visible();
        }
        if let Some(t) = &mut app.directory_tree {
            t.page_rows = usize::from(
                crate::directory_tree::geometry(area)
                    .height
                    .saturating_sub(9),
            )
            .max(1);
            t.keep_visible();
        }
        if let Some(m) = &mut app.discovery.modal {
            m.rows = usize::from(area.height.saturating_sub(4).min(26).saturating_sub(11)).max(1);
            m.keep_visible();
        }
        if let Some(crate::file_tools::ToolDialog::History {
            paths,
            cursor,
            offset,
            rows,
            ..
        }) = &mut app.file_tools.dialog
        {
            *rows = usize::from(area.height.saturating_sub(4).min(22) - 6);
            *cursor = (*cursor).min(paths.len().saturating_sub(1));
            if *cursor < *offset {
                *offset = *cursor;
            }
            if *cursor >= *offset + *rows {
                *offset = *cursor + 1 - *rows;
            }
        }
        if let Some(info) = &mut app.utilities.information {
            info.rows = usize::from(area.height.saturating_sub(4).min(24).saturating_sub(6));
            info.step(0);
        }
    }
    let message = if area.width >= 80 && area.height >= 25 {
        app.dialogs
            .last()
            .map(|d| message_layout(area, d.title, &d.message))
    } else {
        None
    };
    if let Some(m) = &message {
        app.message_offset = app.message_offset.min(m.maximum);
    }
    Prepared {
        message,
        regions,
        panel_areas,
        panels,
    }
}
/// Prepare after commands, even when no terminal draw is requested.
pub fn update(app: &mut App) {
    let (w, h) = app.utilities.size;
    if w > 0 && h > 0 {
        let _ = prepare(app, Rect::new(0, 0, w, h));
    }
}

pub struct MessageLayout {
    pub rect: Rect,
    pub lines: Vec<String>,
    pub visible: usize,
    pub maximum: usize,
}
pub fn message_layout(area: Rect, title: &str, text: &str) -> MessageLayout {
    use ratatui::text::Span;
    let text = text
        .lines()
        .map(crate::filesystem::display_name)
        .collect::<Vec<_>>()
        .join("\n");
    let width = (text
        .lines()
        .map(|s| Span::raw(s).width())
        .max()
        .unwrap_or(0)
        + 6)
    .max(Span::raw(title).width() + 6)
    .clamp(36, 76)
    .min(usize::from(area.width)) as u16;
    let lines = wrap(&text, width.saturating_sub(4));
    let height = (lines.len() + 5).min(area.height.saturating_sub(2) as usize) as u16;
    let rect = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    let visible = usize::from(height.saturating_sub(4));
    let maximum = lines.len().saturating_sub(visible);
    MessageLayout {
        rect,
        lines,
        visible,
        maximum,
    }
}
pub(crate) fn wrap(text: &str, width: u16) -> Vec<String> {
    use unicode_segmentation::UnicodeSegmentation;
    let mut result = Vec::new();
    for line in text.split('\n') {
        let mut row = String::new();
        let mut cells = 0;
        for grapheme in line.graphemes(true) {
            let size = ratatui::text::Span::raw(grapheme).width();
            if cells + size > usize::from(width) && !row.is_empty() {
                result.push(std::mem::take(&mut row));
                cells = 0;
            }
            row.push_str(grapheme);
            cells += size;
        }
        result.push(row);
    }
    result
}
