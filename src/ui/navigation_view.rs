use crate::dialog_theme as theme;
use crate::turbo::dialog_spec::{scope, Control, ControlKind, DialogSpec};

use crate::painting::Paragraph;
use crate::presentation::{layout::Rect, text::Line};
use crate::{
    app::App,
    filesystem::display_name,
    formatting::tail,
    mouse::Target,
    navigation_tools::{Action, Modal},
};
pub(crate) fn navigation_spec(app: &App, area: Rect) -> Option<DialogSpec<'_>> {
    let profile = app.config.palette;
    let dos = profile.styles_for(crate::palette::theme::Group::Dialogs);
    let modal = app.navigation_dialog()?;
    let height = if matches!(modal, Modal::Bookmarks { .. }) {
        18
    } else {
        11
    };
    let width = area.width.saturating_sub(6).min(94);
    let rect = crate::tool_windows::bounds(app, area, width, height);
    let title = if matches!(modal, Modal::Bookmarks { .. }) {
        " Quick directories "
    } else {
        " Filename filter "
    };
    let focus = match modal {
        Modal::Bookmarks { focus, .. } | Modal::Filter { focus, .. } => *focus,
    };
    let default = if matches!(modal, Modal::Bookmarks { .. }) {
        Action::Go
    } else {
        Action::ApplyFilter
    };
    let mut spec = DialogSpec::new(
        rect,
        scope(app, crate::focus::Focus::Navigation)?,
        focus,
        app.config.language.text(title),
        Target::Navigation(default),
        Target::Navigation(Action::Close),
    );
    let x = rect.x + 2;
    let w = rect.width - 4;
    let mut buttons = Vec::new();
    match modal {
        Modal::Bookmarks { slot, focus } => {
            let items = (0..9)
                .map(|i| {
                    let value = app.config.bookmarks[i]
                        .as_ref()
                        .map(|p| display_name(&p.to_string_lossy()))
                        .unwrap_or_else(|| app.config.language.text("<empty>").into());
                    format!("{}  {}", i + 1, tail(&value, w - 3))
                })
                .collect();
            let mut control = Control::new(
                0,
                Rect::new(x, rect.y + 1, w, 9),
                ControlKind::List {
                    items,
                    cursor: *slot,
                    offset: 0,
                },
                Target::Navigation(Action::Go),
            );
            control.hits.clear();
            spec.control(control);
            for i in 0..9 {
                spec.hit(
                    Rect::new(x, rect.y + 1 + i as u16, w, 1),
                    Target::Navigation(Action::Slot(i)),
                );
            }
            spec.caption(
                Paragraph::new(
                    app.config
                        .language
                        .text("↑↓ / 1–9 choose · Tab moves · Ctrl+1–9 set · Alt+1–9 go"),
                )
                .style(dos(theme::TEXT)),
                Rect::new(x, rect.y + 11, w, 2),
            );
            for (i, label, action) in [
                (0, "[ Go To ]", Action::Go),
                (1, "[ Set Current ]", Action::Set),
                (2, "[ Clear ]", Action::Clear),
                (3, "[ Close ]", Action::Close),
            ] {
                buttons.push((
                    Rect::new(x + i as u16 * (w / 4), rect.bottom() - 3, w / 4, 1),
                    label,
                    action,
                    *focus == i + 1,
                ));
            }
        }
        Modal::Filter {
            field,
            cursor,
            focus,
        } => {
            spec.caption(Paragraph::new(app.config.language.text("Visible cached filenames · case-sensitive * and ?\nEmpty or * clears · Parent stays visible\nFiltered marks are preserved; operations use visible marks")).style(dos(theme::TEXT)),Rect::new(x,rect.y+1,w,3));
            let input = Rect::new(x, rect.y + 5, w, 1);
            let column = Line::from(&field[..*cursor]).width();
            let scroll = column.saturating_sub(w.saturating_sub(1) as usize);
            spec.control(Control::new(
                0,
                input,
                ControlKind::ScrolledInput {
                    value: display_name(field),
                    scroll,
                },
                Target::Navigation(Action::FilterField),
            ));
            if *focus == 0 {
                spec.cursor = Some((x + (column - scroll) as u16, input.y));
            }
            buttons.push((
                Rect::new(x + 8, rect.bottom() - 2, 16, 1),
                "[ Apply ]",
                Action::ApplyFilter,
                *focus == 1,
            ));
            buttons.push((
                Rect::new(rect.right() - 24, rect.bottom() - 2, 16, 1),
                "[ Cancel ]",
                Action::Close,
                *focus == 2,
            ));
        }
    }
    for (i, (rect, label, action, _)) in buttons.into_iter().enumerate() {
        spec.control(Control::new(
            i + 1,
            rect,
            ControlKind::Button(app.config.language.text(label).into()),
            Target::Navigation(action),
        ));
    }
    Some(spec)
}
