use crate::dialog_theme as theme;
use crate::turbo::dialog_spec::{centered, scope, Control, ControlKind, DialogSpec};

use crate::painting::Paragraph;
use crate::presentation::{layout::Rect, text::Span};
use crate::{
    app::App,
    archive::{reader::Kind, Action},
    filesystem::display_name,
    formatting::tail,
    mouse::Target,
};
pub(crate) fn archive_spec(app: &App, area: Rect) -> Option<DialogSpec<'_>> {
    let b = app.modals.archive()?;
    let profile = app.config.palette;
    let dos = profile.styles_for(crate::palette::theme::Group::Dialogs);
    let rect = centered(
        area,
        area.width.saturating_sub(4).min(110),
        area.height.saturating_sub(4).min(30),
    );
    let mut spec = DialogSpec::new(
        rect,
        scope(app, crate::focus::Focus::Archive)?,
        b.focus,
        app.config.language.text(" Archive browser — read-only "),
        Target::Archive(Action::Open),
        Target::Archive(Action::Close),
    );
    let x = rect.x + 2;
    let w = rect.width - 5;
    spec.caption(
        Paragraph::new(tail(&display_name(&b.path.to_string_lossy()), w)).style(dos(theme::TEXT)),
        Rect::new(x, rect.y + 1, w, 1),
    );
    let directory = b
        .catalog
        .as_ref()
        .and_then(|c| c.nodes.get(b.directory))
        .map(|n| format!("/{}", display_name(&String::from_utf8_lossy(&n.path))))
        .unwrap_or_else(|| "/".into());
    spec.caption(
        Paragraph::new(tail(&directory, w)).style(dos(theme::HEADING)),
        Rect::new(x, rect.y + 2, w, 1),
    );
    spec.caption(
        Paragraph::new(format!(
            "{:<width$}{:>12}  Type",
            "Name",
            "Bytes",
            width = usize::from(w.saturating_sub(24))
        ))
        .style(dos(0x7e)),
        Rect::new(x, rect.y + 3, w, 1),
    );
    let mut items = Vec::new();
    for index in 0..=b.children().len() {
        let node = index
            .checked_sub(1)
            .and_then(|i| b.children().get(i))
            .and_then(|id| b.catalog.as_ref().and_then(|c| c.nodes.get(*id)));
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
        items.push(format!(
            "{}{}{size:>12}  {kind}",
            name,
            " ".repeat(usize::from(nw).saturating_sub(name_width))
        ));
        if index < b.offset || index >= b.offset + b.rows {
            continue;
        }
        let r = Rect::new(x, rect.y + 4 + (index - b.offset) as u16, w, 1);
        spec.hit(r, Target::Archive(Action::Row(index)));
    }
    let list = Rect::new(x, rect.y + 4, w, b.rows as u16);
    let mut control = Control::new(
        0,
        list,
        ControlKind::List {
            items,
            cursor: b.cursor,
            offset: b.offset,
        },
        Target::Archive(Action::Open),
    );
    control.hits.clear();
    spec.control(control);
    spec.native(
        crate::turbo::widgets::scrollbar(b.cursor, b.children().len(), b.rows),
        Rect::new(rect.right() - 2, rect.y + 4, 1, b.rows as u16),
    );
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
        let _ = glyph;
        if glyph != "■" {
            spec.hit(r, Target::Archive(a));
        }
    }
    let details = b
        .selected()
        .and_then(|i| b.catalog.as_ref().map(|c| &c.nodes[i]))
        .map(|n| {
            format!(
                "{} B · {}",
                n.size,
                app.config
                    .datetime
                    .archive(n.modified.as_deref().unwrap_or("Modified: unavailable"))
            )
        })
        .unwrap_or_else(|| "Enter opens · Backspace parent · Esc closes".into());
    spec.caption(
        Paragraph::new(details).style(dos(theme::HEADING)),
        Rect::new(x, rect.bottom() - 5, w, 1),
    );
    spec.caption(
        Paragraph::new(display_name(&b.status)).style(dos(theme::TEXT)),
        Rect::new(x, rect.bottom() - 4, w, 1),
    );
    spec.caption(
        Paragraph::new(
            app.config
                .language
                .text("Read-only · No extraction · Links are not followed"),
        )
        .style(dos(theme::DISABLED)),
        Rect::new(x, rect.bottom() - 3, w, 1),
    );
    let bw = (rect.width - 4) / 6;
    let buttons = [
        (1, "Open", Action::Open),
        (2, "F3 View", Action::View),
        (3, "F9 Info", Action::Info),
        (4, "Refresh", Action::Refresh),
        (5, "Stop", Action::Stop),
        (6, "Close", Action::Close),
    ];
    let labels = buttons.map(|(_, label, _)| app.config.language.text(label));
    let bounds = theme::button_row(
        Rect::new(x, rect.bottom() - 2, bw * 6 - 1, 1),
        &labels,
        bw - 1,
        1,
    );
    for ((i, _, a), (label, r)) in buttons.into_iter().zip(labels.into_iter().zip(bounds)) {
        let enabled = match a {
            Action::Open | Action::View | Action::Info => !b.loading() && b.catalog.is_some(),
            Action::Stop => b.loading(),
            _ => true,
        };
        let mut control = Control::new(i, r, ControlKind::Button(label.into()), Target::Archive(a));
        control.enabled = enabled;
        spec.control(control);
    }
    Some(spec)
}
