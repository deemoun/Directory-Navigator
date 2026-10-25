use crate::painting::{Frame, Paragraph};
use crate::{
    app::App,
    directory_tree::{geometry, Action},
    filesystem::display_name,
    formatting::tail,
    mouse::Target,
};
use ratatui::{
    layout::Rect,
    text::{Line, Span},
};
pub(super) fn draw(frame: &mut impl Frame, app: &mut App, area: Rect) {
    let profile = app.config.palette;
    let dos = profile.styles();
    let Some(tree) = &app.directory_tree else {
        return;
    };
    let g = geometry(area);
    let rect = super::dialog_style::window(
        frame,
        profile,
        area,
        g.width,
        g.height,
        app.config.language.text(" Directory tree "),
    );
    app.mouse.regions.clear();
    super::dialog_style::close_button(
        frame,
        profile,
        rect,
        &mut app.mouse.regions,
        Target::Tree(Action::Close),
    );
    let x = rect.x + 2;
    let w = rect.width - 5;
    let top = rect.y + 3;
    let root = tree.root.as_ref().unwrap_or(&tree.original);
    frame.render_widget(
        Paragraph::new(tail(&display_name(&root.to_string_lossy()), w)).style(dos(0x70)),
        Rect::new(x, rect.y + 1, w, 1),
    );
    for (row, index) in (tree.offset..tree.rows.len())
        .take(tree.page_rows)
        .enumerate()
    {
        let entry = &tree.rows[index];
        let selected = index == tree.cursor;
        let style = dos(if selected { 0x30 } else { 0x70 });
        let name = entry
            .path
            .file_name()
            .unwrap_or(entry.path.as_os_str())
            .to_string_lossy();
        let line = Line::from(vec![
            Span::raw(entry.guide.clone()),
            Span::styled(
                tree.marker(entry),
                if selected {
                    style
                } else {
                    dos(if entry.link { 0x79 } else { 0x71 })
                },
            ),
            Span::raw(" "),
            Span::raw(display_name(&name)),
        ]);
        let marker = ratatui::text::Span::raw(&entry.guide).width() as u16;
        let r = Rect::new(x, top + row as u16, w, 1);
        frame.render_widget(Paragraph::new(line).style(style), r);
        app.mouse
            .regions
            .push((r, Target::Tree(Action::Row(index))));
        if marker < w {
            app.mouse.regions.push((
                Rect::new(x + marker, r.y, 1, 1),
                Target::Tree(Action::Toggle(index)),
            ));
        }
    }
    let rows = tree.page_rows as u16;
    if rows >= 3 {
        let track = rows - 2;
        let thumb = if tree.rows.len() > 1 {
            tree.cursor * (usize::from(track) - 1) / (tree.rows.len() - 1)
        } else {
            0
        };
        for row in 0..rows {
            let (glyph, command) = if row == 0 {
                ("▲", Action::Up)
            } else if row + 1 == rows {
                ("▼", Action::Down)
            } else if usize::from(row - 1) == thumb {
                ("■", Action::Thumb)
            } else if usize::from(row - 1) < thumb {
                ("░", Action::PageUp)
            } else {
                ("░", Action::PageDown)
            };
            let r = Rect::new(rect.right() - 2, top + row, 1, 1);
            frame.render_widget(Paragraph::new(glyph).style(dos(0x70)), r);
            app.mouse.regions.push((r, Target::Tree(command)));
        }
    }
    let path = tree
        .selected()
        .map(|r| r.path.as_path())
        .unwrap_or(tree.original.as_path());
    for (y, text, color) in [
        (
            rect.bottom() - 6,
            tail(&display_name(&path.to_string_lossy()), w),
            0x71,
        ),
        (
            rect.bottom() - 5,
            tail(&display_name(&tree.status()), w),
            0x70,
        ),
        (
            rect.bottom() - 4,
            "←/→ fold · Space toggle · Ctrl+R refresh · Ctrl+H hidden".into(),
            0x78,
        ),
    ] {
        frame.render_widget(
            Paragraph::new(text).style(dos(color)),
            Rect::new(x, y, w, 1),
        );
    }
    let bw = (rect.width - 4) / 4;
    for (index, label, command) in [
        (1, "Open".to_string(), Action::Open),
        (2, "Refresh".to_string(), Action::Refresh),
        (
            3,
            format!("Hidden: {}", if tree.hidden { "on" } else { "off" }),
            Action::Hidden,
        ),
        (4, "Close".to_string(), Action::Close),
    ] {
        let r = Rect::new(x + (index as u16 - 1) * bw, rect.bottom() - 2, bw - 1, 1);
        super::dialog_style::button(
            frame,
            profile,
            r,
            app.config.language.text(&label),
            tree.focus == index,
        );
        app.mouse.regions.push((r, Target::Tree(command)));
    }
}
