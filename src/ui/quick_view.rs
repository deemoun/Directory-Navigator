use crate::{app::App, filesystem::display_name, formatting::tail, mouse::Target};
use ratatui::{
    layout::{Margin, Rect},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
pub(super) fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    let profile = app.config.palette;
    let dos = profile.styles();
    let Some(q) = &mut app.quick_view else {
        return;
    };
    frame.render_widget(
        Block::default().borders(Borders::ALL).style(dos(0x87)),
        area,
    );
    let title = format!(
        " {} · {}",
        app.config.language.text("Quick view"),
        tail(
            &q.path()
                .map(|p| display_name(&p.to_string_lossy()))
                .unwrap_or_default(),
            area.width.saturating_sub(28)
        )
    );
    super::dialog_style::document_header(
        frame,
        profile,
        Rect::new(area.x, area.y, area.width, 1),
        &title,
        &mut app.mouse.regions,
        Target::CloseQuickView,
    );
    let inner = area.inner(Margin::new(1, 1));
    let body = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(1),
    );
    app.mouse.regions.push((body, Target::QuickPreview));
    if let Some(viewer) = &q.viewer {
        super::viewer_view::draw_text(frame, viewer, body, profile, app.config.language);
        let status = viewer
            .contents
            .as_ref()
            .map(|c| {
                format!(
                    "{} · {} lines{}",
                    if c.binary { "Hex" } else { "Text" },
                    c.lines.len(),
                    if c.truncated {
                        " [bounded preview]"
                    } else {
                        ""
                    }
                )
            })
            .unwrap_or_else(|| "Reading…".into());
        frame.render_widget(
            Paragraph::new(status).style(dos(0x30)),
            Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1),
        );
    } else {
        frame.render_widget(
            Paragraph::new(
                q.lines
                    .iter()
                    .map(|l| {
                        app.config
                            .language
                            .field(&display_name(l), app.config.datetime)
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
            .wrap(Wrap { trim: false })
            .style(dos(0x87)),
            body,
        );
    }
}
