use crate::dialog_theme as theme;
use crate::painting::Paragraph;
use crate::presentation::{layout::Rect, text::Span};
use crate::turbo::dialog_spec::{centered, scope, Control, ControlKind, DialogSpec};
use crate::{
    app::App, filesystem::display_name, formatting::tail, mouse::Target, user_menu::Action,
};

pub(crate) fn user_menu_spec(app: &App, area: Rect) -> Option<DialogSpec<'_>> {
    let profile = app.config.palette;
    let dos = profile.styles_for(crate::palette::theme::Group::Dialogs);
    let m = app.user_menu.modals.user_menu()?;
    if m.parameters.is_some() {
        return parameters_spec(app, area);
    }
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
    let rect = centered(area, width, height);
    let mut spec = DialogSpec::new(
        rect,
        scope(app, crate::focus::Focus::UserMenu)?,
        m.focus,
        app.config.language.text(if review {
            " Run user command "
        } else {
            " User menu "
        }),
        Target::UserMenu(if review {
            Action::Run
        } else {
            Action::Choose(m.cursor)
        }),
        Target::UserMenu(if review { Action::Cancel } else { Action::Back }),
    );
    if !review {
        spec.close_action = Target::UserMenu(Action::Close);
    }
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
    spec.caption(
        Paragraph::new(tail(&source, w)).style(dos(theme::TEXT)),
        Rect::new(x, rect.y + 1, w, 1),
    );
    if review {
        let mut memo = Control::new(
            0,
            Rect::new(x, rect.y + 3, w, m.rows as u16),
            ControlKind::Memo(
                m.review_lines
                    .iter()
                    .skip(m.offset)
                    .take(m.rows)
                    .map(|s| tail(s, w))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            Target::UserMenu(Action::Run),
        );
        memo.hits.clear();
        memo.enter_action = true;
        spec.control(memo);
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
        let mut control = Control::new(
            0,
            Rect::new(x - 1, rect.y + 2, w + 2, m.rows as u16 + 2),
            ControlKind::Menu {
                items,
                cursor: m.cursor.saturating_sub(m.offset),
            },
            Target::UserMenu(Action::Choose(m.cursor)),
        );
        control.hits.clear();
        control.enter_action = true;
        spec.control(control);
        for (index, item) in m.items().iter().enumerate().skip(m.offset).take(m.rows) {
            if !item.separator() {
                spec.hit(
                    Rect::new(x, rect.y + 3 + (index - m.offset) as u16, w, 1),
                    Target::UserMenu(Action::Choose(index)),
                );
            }
        }
    }
    if !review {
        spec.caption(
            Paragraph::new(display_name(app.config.language.text(&m.status)))
                .style(dos(theme::DISABLED)),
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
        spec.control(Control::new(
            i + 1,
            r,
            ControlKind::Button(caption),
            Target::UserMenu(action),
        ));
    }
    Some(spec)
}

fn parameters_spec(app: &App, area: Rect) -> Option<DialogSpec<'_>> {
    let m = app.user_menu.modals.user_menu()?;
    let p = m.parameters.as_ref()?;
    let lang = app.config.language;
    let dos = app
        .config
        .palette
        .styles_for(crate::palette::theme::Group::Dialogs);
    let r = centered(area, 76, p.values.len() as u16 + 8);
    let mut spec = DialogSpec::new(
        r,
        scope(app, crate::focus::Focus::UserMenu)?,
        m.focus,
        lang.text(" User command parameters "),
        Target::UserMenu(Action::ParametersAccept),
        Target::UserMenu(Action::Cancel),
    );
    spec.caption(
        Paragraph::new(lang.text("Values are single arguments; review follows before execution."))
            .style(dos(theme::TEXT)),
        Rect::new(r.x + 2, r.y + 1, r.width - 4, 1),
    );
    for (i, (d, value)) in p.definitions.iter().zip(&p.values).enumerate() {
        let y = r.y + 3 + i as u16;
        spec.caption(
            Paragraph::new(tail(&display_name(&d.label), 24)).style(dos(theme::TEXT)),
            Rect::new(r.x + 2, y, 24, 1),
        );
        let input = Rect::new(r.x + 27, y, r.width - 30, 1);
        let col = crate::presentation::text::Line::from(&value[..p.cursors[i]]).width();
        let scroll = col.saturating_sub(input.width.saturating_sub(1) as usize);
        spec.control(Control::new(
            i,
            input,
            ControlKind::ScrolledInput {
                value: display_name(value),
                scroll,
            },
            Target::UserMenu(Action::Parameter(i)),
        ));
        if m.focus == i {
            spec.cursor = Some((input.x + (col - scroll) as u16, y));
        }
    }
    for (i, label, action, x) in [
        (0, "Review", Action::ParametersAccept, 18),
        (1, "Cancel", Action::Cancel, 42),
    ] {
        spec.control(Control::new(
            p.values.len() + i,
            Rect::new(r.x + x, r.bottom() - 3, 18, 1),
            ControlKind::Button(lang.text(label).into()),
            Target::UserMenu(action),
        ));
    }
    Some(spec)
}
