use crate::painting::{Block, Borders, Frame, Paragraph};
use crate::presentation::layout::{Margin, Rect};
use crate::{app::App, filesystem::display_name, formatting::tail, mouse::Target};
pub(super) fn draw(frame: &mut impl Frame, app: &App, area: Rect) {
    let profile = app.config.palette;
    let dos = profile.styles_for(crate::palette::theme::Group::QuickView);
    let Some(q) = &app.quick_view else {
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
        Target::CloseQuickView,
    );
    let inner = area.inner(Margin::new(1, 1));
    let body = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(1),
    );
    frame.regions().push((body, Target::QuickPreview));
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
        let content = q
            .lines
            .iter()
            .map(|l| {
                app.config
                    .language
                    .field(&display_name(l), app.config.datetime)
            })
            .collect::<Vec<_>>()
            .join("\n");
        let lines = crate::painting::wrap_plain(&content, body.width);
        frame.render_widget(
            crate::turbo::widgets::text_view(&lines, 0, 0x87)
                .themed(crate::palette::theme::Group::QuickView),
            body,
        );
    }
}
