//! Layout follows original-dn/COLORSEL.PAS: groups, items, selectors, sample, presets.
use crate::{
    app::App,
    dialog_theme as theme,
    mouse::Target,
    painting::{Frame, Paragraph},
    palette::{
        theme::{self as colors, Group},
        Action,
    },
    presentation::layout::Rect,
    turbo::dialog_spec::{scope, Control, ControlKind, DialogSpec},
};
pub(super) fn draw(frame: &mut impl Frame, app: &App, area: Rect) {
    if let Some(s) = colors_spec(app, area) {
        s.paint(frame, app);
    }
}
pub(crate) fn colors_spec(app: &App, area: Rect) -> Option<DialogSpec<'_>> {
    let d = app.modals.color_dialog()?;
    let p = app.config.palette;
    let lang = app.config.language;
    let r = theme::centered(area, 78, 22);
    let mut s = DialogSpec::new(
        r,
        scope(app, crate::focus::Focus::Colors)?,
        d.cursor,
        lang.text(" Colors "),
        Target::AcceptColors,
        Target::CancelColors,
    );
    s.tab_order = vec![4, 5, 6, 7, 8, 0, 1, 9, 2, 3];
    let group_rect = Rect::new(r.x + 3, r.y + 3, 22, 13);
    let item_rect = Rect::new(r.x + 27, r.y + 3, 25, 13);
    for (id, label, bounds, items, cursor, offset, is_item) in [
        (
            4,
            "Group",
            group_rect,
            colors::GROUPS
                .iter()
                .map(|(_, name)| label(lang, name))
                .collect::<Vec<_>>(),
            d.group,
            d.group_offset,
            false,
        ),
        (
            5,
            "Item",
            item_rect,
            colors::items(d.group)
                .map(|i| label(lang, i.label))
                .collect::<Vec<_>>(),
            d.item,
            d.item_offset,
            true,
        ),
    ] {
        s.caption(
            heading(lang.text(label), p, d.cursor == id),
            Rect::new(bounds.x, bounds.y - 1, bounds.width, 1),
        );
        s.hit(
            Rect::new(bounds.x, bounds.y - 1, bounds.width, 1),
            Target::Colors(Action::Focus(id)),
        );
        let mut c = Control::new(
            id,
            bounds,
            ControlKind::List {
                items: items.clone(),
                cursor,
                offset,
            },
            Target::Colors(if is_item {
                Action::Item(cursor)
            } else {
                Action::Group(cursor)
            }),
        );
        c.dn_list = true;
        c.hits = (offset..items.len().min(offset + 13))
            .map(|row| {
                (
                    Rect::new(bounds.x, bounds.y + (row - offset) as u16, bounds.width, 1),
                    Target::Colors(if is_item {
                        Action::Item(row)
                    } else {
                        Action::Group(row)
                    }),
                )
            })
            .collect();
        s.control(c);
        let bar = Rect::new(bounds.right(), bounds.y, 1, bounds.height);
        s.native(
            crate::turbo::widgets::scrollbar(cursor, items.len().saturating_sub(1), 13),
            bar,
        );
        s.hit(
            Rect::new(bar.x, bar.y, 1, 1),
            Target::Colors(Action::Scroll {
                items: is_item,
                down: false,
                page: false,
            }),
        );
        s.hit(
            Rect::new(bar.x, bar.bottom() - 1, 1, 1),
            Target::Colors(Action::Scroll {
                items: is_item,
                down: true,
                page: false,
            }),
        );
        for y in 1..bar.height - 1 {
            s.hit(
                Rect::new(bar.x, bar.y + y, 1, 1),
                Target::Colors(Action::Scroll {
                    items: is_item,
                    down: y >= bar.height / 2,
                    page: true,
                }),
            );
        }
    }
    let attr = p.slot(d.selected().slot);
    for (id, title, y, value, foreground) in [
        (6, "Foreground", r.y + 3, attr & 15, true),
        (7, "Background", r.y + 10, attr >> 4, false),
    ] {
        let x = r.x + 56;
        s.caption(
            Paragraph::new(format!(
                "┌─ {}{}┐",
                lang.text(title),
                "─".repeat(14usize.saturating_sub(
                    crate::presentation::text::Span::raw(lang.text(title)).width()
                ))
            ))
            .style(p.for_group(Group::Dialogs, theme::FRAME)),
            Rect::new(x - 1, y - 1, 18, 1),
        );
        s.caption(
            heading(lang.text(title), p, d.cursor == id),
            Rect::new(x + 2, y - 1, 14, 1),
        );
        s.hit(
            Rect::new(x + 2, y - 1, 14, 1),
            Target::Colors(Action::Focus(id)),
        );
        for dy in 0..4 {
            s.caption(
                Paragraph::new("│").style(p.for_group(Group::Dialogs, theme::FRAME)),
                Rect::new(x - 1, y + dy, 1, 1),
            );
            s.caption(
                Paragraph::new("│").style(p.for_group(Group::Dialogs, theme::FRAME)),
                Rect::new(x + 16, y + dy, 1, 1),
            );
        }
        s.caption(
            Paragraph::new("└────────────────┘").style(p.for_group(Group::Dialogs, theme::FRAME)),
            Rect::new(x - 1, y + 4, 18, 1),
        );
        let mut c = Control::new(
            id,
            Rect::new(x, y, 16, 4),
            ControlKind::ColorGrid(value),
            Target::Colors(if foreground {
                Action::Foreground(value)
            } else {
                Action::Background(value)
            }),
        );
        c.hits = (0..16)
            .map(|n| {
                (
                    Rect::new(x + u16::from(n % 4) * 4, y + u16::from(n / 4), 4, 1),
                    Target::Colors(if foreground {
                        Action::Foreground(n)
                    } else {
                        Action::Background(n)
                    }),
                )
            })
            .collect();
        s.control(c);
    }
    for row in 0..3 {
        s.caption(
            Paragraph::new(lang.text("Text Text Text")).style(p.dos(attr)),
            Rect::new(r.x + 55, r.y + 16 + row, 19, 1),
        );
    }
    s.control(Control::new(
        8,
        Rect::new(r.x + 3, r.y + 17, 46, 1),
        ControlKind::Checkbox {
            caption: lang.text("Default colors set").into(),
            checked: d.defaults,
        },
        Target::Colors(Action::Defaults),
    ));
    s.caption(
        Paragraph::new(lang.text("Tab selects · Arrows change · Esc restores"))
            .style(p.for_group(Group::Dialogs, theme::TEXT)),
        Rect::new(r.x + 3, r.y + 18, 50, 1),
    );
    let labels = ["~C~olor", "B/~W~", "~M~ono", "O~K~", "[ Cancel ]"];
    let buttons = theme::button_row(
        Rect::new(r.x + 3, r.bottom() - 3, r.width - 6, 1),
        &labels.map(|x| lang.text(x)),
        12,
        2,
    );
    for ((id, label, target), bounds) in [
        (0, labels[0], Target::Colors(Action::Color)),
        (1, labels[1], Target::Colors(Action::BlackWhite)),
        (9, labels[2], Target::Colors(Action::Mono)),
        (2, labels[3], Target::AcceptColors),
        (3, labels[4], Target::CancelColors),
    ]
    .into_iter()
    .zip(buttons)
    {
        let mut c = Control::new(
            id,
            bounds,
            ControlKind::Button(lang.text(label).into()),
            target,
        );
        c.label_keys(lang, label);
        s.control(c);
    }
    Some(s)
}

fn label(language: crate::localization::Language, source: &str) -> String {
    if language == crate::localization::Language::English {
        source.to_owned()
    } else {
        language.text(&format!("Colors: {source}")).to_owned()
    }
}

fn heading(text: &str, p: crate::palette::PaletteProfile, focused: bool) -> Paragraph<'static> {
    let mut chars = text.chars();
    Paragraph::new(crate::presentation::text::Line::from(vec![
        crate::presentation::text::Span::styled(
            chars.next().unwrap_or(' ').to_string(),
            p.for_group(Group::Dialogs, 0x7e),
        ),
        crate::presentation::text::Span::styled(
            chars.collect::<String>(),
            p.for_group(Group::Dialogs, if focused { 0x7f } else { 0x70 }),
        ),
    ]))
}
