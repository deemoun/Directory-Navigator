use crate::{
    app::App,
    archive::{reader::Kind, Action},
    filesystem::display_name,
    formatting::tail,
    mouse::Target,
};
use ratatui::{
    layout::Rect,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
pub(super) fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(b) = &app.archive else {
        return;
    };
    let profile = app.config.palette;
    let dos = profile.styles();
    let rect = super::dialog_style::window(
        frame,
        profile,
        area,
        area.width.saturating_sub(4).min(110),
        area.height.saturating_sub(4).min(30),
        " Archive browser — read-only ",
    );
    app.mouse.regions.clear();
    super::dialog_style::close_button(
        frame,
        profile,
        rect,
        &mut app.mouse.regions,
        Target::Archive(Action::Close),
    );
    let x = rect.x + 2;
    let w = rect.width - 5;
    frame.render_widget(
        Paragraph::new(tail(&display_name(&b.path.to_string_lossy()), w)).style(dos(0x70)),
        Rect::new(x, rect.y + 1, w, 1),
    );
    let directory = b
        .catalog
        .as_ref()
        .and_then(|c| c.nodes.get(b.directory))
        .map(|n| format!("/{}", display_name(&String::from_utf8_lossy(&n.path))))
        .unwrap_or_else(|| "/".into());
    frame.render_widget(
        Paragraph::new(tail(&directory, w)).style(dos(0x71)),
        Rect::new(x, rect.y + 2, w, 1),
    );
    frame.render_widget(
        Paragraph::new(format!(
            "{:<width$}{:>12}  Type",
            "Name",
            "Bytes",
            width = usize::from(w.saturating_sub(24))
        ))
        .style(dos(0x7e)),
        Rect::new(x, rect.y + 3, w, 1),
    );
    for row in 0..b.rows {
        let index = b.offset + row;
        if index > b.children().len() {
            break;
        }
        let node = index
            .checked_sub(1)
            .and_then(|i| b.children().get(i))
            .and_then(|id| b.catalog.as_ref().and_then(|c| c.nodes.get(*id)));
        let selected = index == b.cursor;
        let color = if selected {
            0x30
        } else if node.is_some_and(|n| n.kind == Kind::Link) {
            0x79
        } else {
            0x70
        };
        let name = node.map(|n| n.name()).unwrap_or_else(|| "..".into());
        let kind = node
            .map(|n| {
                if n.locked {
                    "Locked"
                } else {
                    match n.kind {
                        Kind::Directory => "<DIR>",
                        Kind::File => "File",
                        Kind::Link => "Link",
                        Kind::Special => "Special",
                    }
                }
            })
            .unwrap_or("<UP>");
        let size = node
            .filter(|n| n.kind != Kind::Directory)
            .map(|n| {
                if n.size.to_string().len() > 12 {
                    crate::formatting::size(n.size)
                } else {
                    n.size.to_string()
                }
            })
            .unwrap_or_default();
        let nw = w.saturating_sub(24);
        let name = tail(&name, nw);
        let name_width = Span::raw(&name).width();
        let line = Line::from(vec![
            Span::raw(name),
            Span::raw(" ".repeat(usize::from(nw).saturating_sub(name_width))),
            Span::raw(format!("{size:>12}  {kind}")),
        ]);
        let r = Rect::new(x, rect.y + 4 + row as u16, w, 1);
        frame.render_widget(Paragraph::new(line).style(dos(color)), r);
        app.mouse
            .regions
            .push((r, Target::Archive(Action::Row(index))));
    }
    let rows = b.rows as u16;
    for row in 0..rows {
        let thumb = if b.children().is_empty() {
            0
        } else {
            b.cursor * usize::from(rows.saturating_sub(3)) / b.children().len()
        };
        let (glyph, a) = if row == 0 {
            ("▲", Action::Up)
        } else if row + 1 == rows {
            ("▼", Action::Down)
        } else if usize::from(row - 1) == thumb {
            ("■", Action::Row(b.cursor))
        } else if usize::from(row - 1) < thumb {
            ("░", Action::PageUp)
        } else {
            ("░", Action::PageDown)
        };
        let r = Rect::new(rect.right() - 2, rect.y + 4 + row, 1, 1);
        frame.render_widget(Paragraph::new(glyph).style(dos(0x70)), r);
        if glyph != "■" {
            app.mouse.regions.push((r, Target::Archive(a)));
        }
    }
    let details = b
        .selected()
        .and_then(|i| b.catalog.as_ref().map(|c| &c.nodes[i]))
        .map(|n| {
            format!(
                "{} B · {}",
                n.size,
                n.modified.as_deref().unwrap_or("Modified: unavailable")
            )
        })
        .unwrap_or_else(|| "Enter opens · Backspace parent · Esc closes".into());
    frame.render_widget(
        Paragraph::new(details).style(dos(0x71)),
        Rect::new(x, rect.bottom() - 5, w, 1),
    );
    frame.render_widget(
        Paragraph::new(display_name(&b.status)).style(dos(0x70)),
        Rect::new(x, rect.bottom() - 4, w, 1),
    );
    frame.render_widget(
        Paragraph::new("Read-only · No extraction · Links are not followed").style(dos(0x78)),
        Rect::new(x, rect.bottom() - 3, w, 1),
    );
    let bw = (rect.width - 4) / 6;
    for (i, label, a) in [
        (1, "Open", Action::Open),
        (2, "F3 View", Action::View),
        (3, "F9 Info", Action::Info),
        (4, "Refresh", Action::Refresh),
        (5, "Stop", Action::Stop),
        (6, "Close", Action::Close),
    ] {
        let r = Rect::new(x + (i as u16 - 1) * bw, rect.bottom() - 2, bw - 1, 1);
        let enabled = match a {
            Action::Open | Action::View | Action::Info => !b.loading() && b.catalog.is_some(),
            Action::Stop => b.loading(),
            _ => true,
        };
        if enabled {
            super::dialog_style::button(frame, profile, r, label, b.focus == i);
            app.mouse.regions.push((r, Target::Archive(a)));
        } else {
            frame.render_widget(Paragraph::new(label).style(dos(0x78)), r);
        }
    }
}
