use crate::painting::{Frame, Paragraph};
use crate::{
    app::App, filesystem::display_name, formatting::tail, mouse::Target, user_menu::Action,
};
use ratatui::{
    layout::Rect,
    text::{Line, Span},
};
pub(super) fn draw(frame: &mut impl Frame, app: &mut App, area: Rect) {
    let profile = app.config.palette;
    let dos = profile.styles();
    let Some(m) = &app.user_menu.modal else {
        return;
    };
    let review = m.review.is_some();
    let width = if review {
        76
    } else {
        m.items()
            .iter()
            .map(|i| Span::raw(crate::menu::plain(&i.label)).width() + 10)
            .max()
            .unwrap_or(44)
            .clamp(52, 76) as u16
    };
    let height = if review {
        area.height.saturating_sub(4).min(26)
    } else {
        (m.items().len() + 7)
            .clamp(10, 24)
            .min(area.height.saturating_sub(4) as usize) as u16
    };
    let rect = super::dialog_style::window(
        frame,
        profile,
        area,
        width,
        height,
        app.config.language.text(if review {
            " Run user command "
        } else {
            " User menu "
        }),
    );
    app.mouse.regions.clear();
    super::dialog_style::close_button(
        frame,
        profile,
        rect,
        &mut app.mouse.regions,
        Target::UserMenu(if review {
            Action::Cancel
        } else {
            Action::Close
        }),
    );
    let x = rect.x + 2;
    let w = rect.width - 4;
    let source = m
        .data
        .as_ref()
        .map(|d| {
            let label = if d.local { "Local: " } else { "Global: " };
            format!(
                "{}{}",
                label,
                tail(
                    &display_name(&d.path.to_string_lossy()),
                    w.saturating_sub(label.len() as u16)
                )
            )
        })
        .unwrap_or_default();
    frame.render_widget(
        Paragraph::new(tail(&source, w)).style(dos(0x70)),
        Rect::new(x, rect.y + 1, w, 1),
    );
    for row in 0..m.rows {
        let index = m.offset + row;
        let r = Rect::new(x, rect.y + 3 + row as u16, w, 1);
        if review {
            if let Some(text) = m.review_lines.get(index) {
                frame.render_widget(Paragraph::new(tail(text, w)).style(dos(0x70)), r);
            }
        } else if let Some(item) = m.items().get(index) {
            if item.separator() {
                frame.render_widget(Paragraph::new("─".repeat(w as usize)).style(dos(0x78)), r);
                continue;
            }
            let selected = index == m.cursor;
            let mut highlight = false;
            let mut spans = Vec::new();
            for c in item.label.chars() {
                if c == '~' {
                    highlight = !highlight;
                    continue;
                }
                spans.push(Span::styled(
                    c.to_string(),
                    dos(if highlight {
                        if selected {
                            0x2c
                        } else {
                            0x74
                        }
                    } else if selected {
                        0x20
                    } else {
                        0x70
                    }),
                ));
            }
            if !item.children.is_empty() {
                let used = Line::from(spans.clone()).width();
                spans.push(Span::raw(
                    " ".repeat(usize::from(w).saturating_sub(used + 1)),
                ));
                spans.push(Span::raw("►"));
            }
            frame.render_widget(
                Paragraph::new(Line::from(spans)).style(dos(if selected { 0x20 } else { 0x70 })),
                r,
            );
            app.mouse
                .regions
                .push((r, Target::UserMenu(Action::Choose(index))));
        }
    }
    if !review {
        frame.render_widget(
            Paragraph::new(display_name(&m.status)).style(dos(0x78)),
            Rect::new(x, rect.bottom() - 3, w, 1),
        );
    }
    let buttons: Vec<(&str, Action)> = if review {
        vec![("Run", Action::Run), ("Cancel", Action::Cancel)]
    } else {
        vec![
            (
                if m.data.as_ref().is_some_and(|d| d.local) {
                    "F2 Global"
                } else {
                    "F2 Local"
                },
                Action::Switch,
            ),
            ("F4 Edit", Action::Edit),
            ("Close", Action::Close),
        ]
    };
    let bw = w / buttons.len() as u16;
    for (i, (label, action)) in buttons.into_iter().enumerate() {
        let r = Rect::new(x + i as u16 * bw, rect.bottom() - 2, bw - 1, 1);
        let translated = app.config.language.text(label);
        let caption = if let Some((key, rest)) = translated.split_once(' ') {
            format!(
                "{} {rest}",
                crate::shortcuts::label(app.config.shortcuts, crate::focus::Screen::Manager, key)
            )
        } else {
            translated.to_owned()
        };
        super::dialog_style::button(frame, profile, r, &caption, m.focus == i + 1);
        app.mouse.regions.push((r, Target::UserMenu(action)));
    }
}
