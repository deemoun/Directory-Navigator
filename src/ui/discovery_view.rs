use crate::painting::{Frame, Paragraph};
use crate::{
    app::App,
    discovery::{Action, Kind},
    filesystem::display_name,
    formatting::tail,
    mouse::Target,
};
use ratatui::layout::Rect;
pub(super) fn draw(frame: &mut impl Frame, app: &mut App, area: Rect) {
    let profile = app.config.palette;
    let dos = profile.styles();
    let Some(m) = &mut app.discovery.modal else {
        return;
    };
    let form = m.kind == Kind::FindForm;
    let width = if form {
        66
    } else {
        area.width.saturating_sub(4).min(110)
    };
    let height = if form {
        12
    } else {
        area.height.saturating_sub(4).min(26)
    };
    let title = match m.kind {
        Kind::FindForm => " Find files ",
        Kind::FindResults => " Find results ",
        Kind::Sizes => " Directory sizes — snapshot ",
    };
    let rect = super::dialog_style::window(
        frame,
        profile,
        area,
        width,
        height,
        app.config.language.text(title),
    );
    app.mouse.regions.clear();
    super::dialog_style::close_button(
        frame,
        profile,
        rect,
        &mut app.mouse.regions,
        Target::Discovery(Action::Close),
    );
    let x = rect.x + 2;
    let w = rect.width - 4;
    frame.render_widget(
        Paragraph::new(tail(&display_name(&m.root.to_string_lossy()), w)).style(dos(0x70)),
        Rect::new(x, rect.y + 1, w, 1),
    );
    if form {
        frame.render_widget(
            Paragraph::new(app.config.language.text("Filename pattern")).style(dos(0x70)),
            Rect::new(x, rect.y + 3, w, 1),
        );
        let input = Rect::new(x, rect.y + 4, w, 1);
        let column = ratatui::text::Line::from(&m.prefs.pattern[..m.input_cursor]).width();
        let scroll = column.saturating_sub(usize::from(w.saturating_sub(1)));
        frame.render_widget(
            Paragraph::new(display_name(&m.prefs.pattern))
                .scroll((0, scroll as u16))
                .style(super::dialog_style::input_style(profile, m.focus == 0)),
            input,
        );
        app.mouse
            .regions
            .push((input, Target::Discovery(Action::Field)));
        if m.focus == 0 && app.dialogs.is_empty() {
            frame.set_cursor_position((input.x + (column - scroll) as u16, input.y));
        }
        for (i, label, on) in [
            (1, "Include subdirectories", m.prefs.recursive),
            (2, "Show hidden", m.prefs.hidden),
        ] {
            let row = Rect::new(x, rect.y + 5 + i as u16, w, 1);
            let label = app.config.language.text(label);
            frame.render_widget(
                Paragraph::new(format!("[{}] {label}", if on { 'x' } else { ' ' }))
                    .style(dos(if m.focus == i { 0x30 } else { 0x70 })),
                row,
            );
            app.mouse
                .regions
                .push((row, Target::Discovery(Action::Toggle(i))));
        }
        for (i, label, command) in [(3, "Find", Action::Start), (4, "Cancel", Action::Close)] {
            let button = Rect::new(x + 12 + (i as u16 - 3) * 22, rect.bottom() - 3, 18, 1);
            super::dialog_style::button(
                frame,
                profile,
                button,
                app.config.language.text(label),
                m.focus == i,
            );
            app.mouse.regions.push((button, Target::Discovery(command)));
        }
    } else {
        for row in 0..m.rows {
            let index = m.offset + row;
            let text = if m.kind == Kind::Sizes {
                m.sizes.get(index).map(|s| {
                    format!(
                        "{} {} B · {}f {}d {}l · {} skipped{}",
                        tail(&display_name(&s.path.to_string_lossy()), w / 3),
                        s.bytes,
                        s.files,
                        s.directories,
                        s.links,
                        s.skipped,
                        if s.partial { " [partial]" } else { "" }
                    )
                })
            } else {
                m.hits
                    .get(index)
                    .map(|h| tail(&display_name(&h.path.to_string_lossy()), w))
            };
            if let Some(text) = text {
                let r = Rect::new(x, rect.y + 3 + row as u16, w, 1);
                frame.render_widget(
                    Paragraph::new(text).style(dos(if index == m.cursor { 0x30 } else { 0x70 })),
                    r,
                );
                app.mouse
                    .regions
                    .push((r, Target::Discovery(Action::Row(index))));
            }
        }
        let summary = format!(
            "{} · {} results · {} visited · {} skipped",
            m.status,
            m.count(),
            m.stats.visited,
            m.stats.skipped
        );
        frame.render_widget(
            Paragraph::new(summary).style(dos(0x70)),
            Rect::new(x, rect.bottom() - 7, w, 1),
        );
        frame.render_widget(
            Paragraph::new(tail(&display_name(&m.stats.path.to_string_lossy()), w))
                .style(dos(0x70)),
            Rect::new(x, rect.bottom() - 6, w, 1),
        );
        if let Some(error) = &m.stats.first_error {
            frame.render_widget(
                Paragraph::new(format!("First error: {}", display_name(error)))
                    .wrap(crate::painting::Wrap { trim: false })
                    .style(dos(0x74)),
                Rect::new(x, rect.bottom() - 5, w, 2),
            );
        }
        let buttons: &[(&str, Action)] = if m.kind == Kind::Sizes {
            &[
                ("Recalculate", Action::Recalculate),
                ("Stop", Action::Stop),
                ("Close", Action::Close),
            ]
        } else {
            &[
                ("Go to", Action::Jump),
                ("F3 View", Action::View),
                ("F4 Edit", Action::Edit),
                ("Stop", Action::Stop),
                ("Close", Action::Close),
            ]
        };
        let bw = w / buttons.len() as u16;
        for (index, (label, command)) in buttons.iter().enumerate() {
            let r = Rect::new(
                x + index as u16 * bw,
                rect.bottom() - 2,
                bw.saturating_sub(1),
                1,
            );
            let enabled = match command {
                Action::Stop => m.job.is_some(),
                Action::Recalculate => m.job.is_none(),
                Action::Jump => !m.hits.is_empty(),
                Action::View | Action::Edit => m.hits.get(m.cursor).is_some_and(|h| h.regular),
                _ => true,
            };
            if enabled {
                super::dialog_style::button(
                    frame,
                    profile,
                    r,
                    app.config.language.text(label),
                    m.focus == index,
                );
                app.mouse.regions.push((r, Target::Discovery(*command)));
            } else {
                frame.render_widget(Paragraph::new(*label).style(dos(0x78)), r);
            }
        }
    }
}
