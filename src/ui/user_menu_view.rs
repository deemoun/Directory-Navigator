use crate::painting::{Frame, Paragraph};
use crate::presentation::{layout::Rect, text::Span};
use crate::{
    app::App, filesystem::display_name, formatting::tail, mouse::Target, user_menu::Action,
};
pub(super) fn draw(frame: &mut impl Frame, app: &App, area: Rect) {
    let profile = app.config.palette;
    let dos = profile.styles();
    let Some(m) = app.user_menu.modals.user_menu() else {
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
    frame.regions().clear();
    super::dialog_style::close_button(
        frame,
        profile,
        rect,
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
    if review {
        frame.render_widget(
            crate::turbo::widgets::memo(
                &m.review_lines
                    .iter()
                    .skip(m.offset)
                    .take(m.rows)
                    .map(|s| tail(s, w))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            Rect::new(x, rect.y + 3, w, m.rows as u16),
        );
    } else {
        let items = m
            .items()
            .iter()
            .skip(m.offset)
            .take(m.rows)
            .map(|item| crate::turbo::widgets::MenuEntry {
                label: if item.separator() {
                    String::new()
                } else {
                    item.label.clone()
                },
                shortcut: String::new(),
                enabled: !item.separator(),
                checked: false,
                submenu: !item.children.is_empty(),
            })
            .collect();
        frame.render_widget(
            crate::turbo::widgets::menu(items, m.cursor.saturating_sub(m.offset)),
            Rect::new(x - 1, rect.y + 2, w + 2, m.rows as u16 + 2),
        );
        for (index, item) in m.items().iter().enumerate().skip(m.offset).take(m.rows) {
            if !item.separator() {
                frame.regions().push((
                    Rect::new(x, rect.y + 3 + (index - m.offset) as u16, w, 1),
                    Target::UserMenu(Action::Choose(index)),
                ));
            }
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
        super::dialog_style::button_with_default(
            frame,
            profile,
            r,
            &caption,
            m.focus == i + 1,
            review && i == 0 && m.focus == 0,
        );
        frame.regions().push((r, Target::UserMenu(action)));
    }
}
