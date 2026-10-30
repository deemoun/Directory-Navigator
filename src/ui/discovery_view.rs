use crate::dialog_theme as theme;
use crate::turbo::dialog_spec::{centered, scope, Control, ControlKind, DialogSpec};

use crate::painting::Paragraph;
use crate::presentation::layout::Rect;
use crate::{
    app::App,
    discovery::{Action, Kind},
    filesystem::display_name,
    formatting::tail,
    mouse::Target,
};
pub(crate) fn discovery_spec(app: &App, area: Rect) -> Option<DialogSpec<'_>> {
    let profile = app.config.palette;
    let dos = profile.styles_for(crate::palette::theme::Group::Dialogs);
    let m = app.discovery.modals.discovery()?;
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
    let rect = centered(area, width, height);
    let default = if form {
        Action::Start
    } else if m.kind == Kind::Sizes {
        Action::Recalculate
    } else {
        Action::Jump
    };
    let mut spec = DialogSpec::new(
        rect,
        scope(app, crate::focus::Focus::Discovery)?,
        m.focus,
        app.config.language.text(title),
        Target::Discovery(default),
        Target::Discovery(Action::Close),
    );
    let x = rect.x + 2;
    let w = rect.width - 4;
    spec.caption(
        Paragraph::new(tail(&display_name(&m.root.to_string_lossy()), w)).style(dos(theme::TEXT)),
        Rect::new(x, rect.y + 1, w, 1),
    );
    if form {
        spec.caption(
            Paragraph::new(app.config.language.text("Filename pattern")).style(dos(theme::TEXT)),
            Rect::new(x, rect.y + 3, w, 1),
        );
        let input = Rect::new(x, rect.y + 4, w, 1);
        let column =
            crate::presentation::text::Line::from(&m.prefs.pattern[..m.input_cursor]).width();
        let scroll = column.saturating_sub(usize::from(w.saturating_sub(1)));
        spec.control(Control::new(
            0,
            input,
            ControlKind::ScrolledInput {
                value: display_name(&m.prefs.pattern),
                scroll,
            },
            Target::Discovery(Action::Field),
        ));
        if m.focus == 0 && app.dialogs().is_empty() {
            spec.cursor = Some((input.x + (column - scroll) as u16, input.y));
        }
        for (i, label, on) in [
            (1, "Include subdirectories", m.prefs.recursive),
            (2, "Show hidden", m.prefs.hidden),
        ] {
            let row = Rect::new(x, rect.y + 5 + i as u16, w, 1);
            let label = app.config.language.text(label);
            spec.control(Control::new(
                i,
                row,
                ControlKind::Checkbox {
                    caption: label.into(),
                    checked: on,
                },
                Target::Discovery(Action::Toggle(i)),
            ));
        }
        for (i, label, command) in [(3, "Find", Action::Start), (4, "Cancel", Action::Close)] {
            let button = Rect::new(x + 12 + (i as u16 - 3) * 22, rect.bottom() - 3, 18, 1);
            spec.control(Control::new(
                i,
                button,
                ControlKind::Button(app.config.language.text(label).into()),
                Target::Discovery(command),
            ));
        }
    } else {
        let items = (0..m.count())
            .map(|index| {
                if m.kind == Kind::Sizes {
                    let s = &m.sizes[index];
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
                } else {
                    tail(&display_name(&m.hits[index].path.to_string_lossy()), w)
                }
            })
            .collect();
        let mut list = Control::new(
            0,
            Rect::new(x, rect.y + 3, w, m.rows as u16),
            ControlKind::List {
                items,
                cursor: m.cursor,
                offset: m.offset,
            },
            Target::Discovery(default),
        );
        list.hits.clear();
        list.space_default = true;
        spec.control(list);
        for index in m.offset..m.count().min(m.offset + m.rows) {
            spec.hit(
                Rect::new(x, rect.y + 3 + (index - m.offset) as u16, w, 1),
                Target::Discovery(Action::Row(index)),
            );
        }
        let summary = format!(
            "{} · {} results · {} visited · {} skipped",
            m.status,
            m.count(),
            m.stats.visited,
            m.stats.skipped
        );
        spec.caption(
            Paragraph::new(summary).style(dos(theme::TEXT)),
            Rect::new(x, rect.bottom() - 7, w, 1),
        );
        spec.caption(
            Paragraph::new(tail(&display_name(&m.stats.path.to_string_lossy()), w))
                .style(dos(theme::TEXT)),
            Rect::new(x, rect.bottom() - 6, w, 1),
        );
        if let Some(error) = &m.stats.first_error {
            spec.caption(
                Paragraph::new(format!("First error: {}", display_name(error)))
                    .wrap(crate::painting::Wrap { trim: false })
                    .style(dos(0x74)),
                Rect::new(x, rect.bottom() - 5, w, 2),
            );
        }
        if m.job.is_some() {
            spec.native(
                crate::turbo::widgets::busy(),
                Rect::new(x, rect.bottom() - 3, w, 1),
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
            let mut control = Control::new(
                index + 1,
                r,
                ControlKind::Button(app.config.language.text(label).into()),
                Target::Discovery(*command),
            );
            control.enabled = enabled;
            spec.control(control);
        }
    }
    Some(spec)
}
