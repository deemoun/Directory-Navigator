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
    #[cfg(windows)]
    if let Modal::Drives { letters, focus, .. } = modal {
        return crate::windows_drives::spec(app, area, letters, *focus);
    }
    if let Modal::Filter {
        draft,
        field,
        cursor,
        focus,
    } = modal
    {
        return filter_spec(app, area, draft, field, *cursor, *focus);
    }
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
        #[cfg(windows)]
        Modal::Drives { .. } => unreachable!("drive picker has its own specification"),
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
        #[cfg(windows)]
        Modal::Drives { .. } => unreachable!("drive picker has its own specification"),
        Modal::Bookmarks { slot, focus, .. } => {
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
                // Reserve the native button shadow row, as other tool forms do.
                // Omitting it feeds a smaller height back into desktop geometry.
                buttons.push((
                    Rect::new(x + i as u16 * (w / 4), rect.bottom() - 2, w / 4, 1),
                    label,
                    action,
                    *focus == i + 1,
                ));
            }
        }
        Modal::Filter { .. } => unreachable!("filter has its own specification"),
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

fn filter_spec<'a>(
    app: &'a App,
    area: Rect,
    draft: &crate::navigation_tools::FilterDraft,
    field: &str,
    cursor: usize,
    focus: usize,
) -> Option<DialogSpec<'a>> {
    let styles = app
        .config
        .palette
        .styles_for(crate::palette::theme::Group::Dialogs);
    let lang = app.config.language;
    let rect = crate::tool_windows::bounds(app, area, area.width.saturating_sub(6).min(78), 19);
    let mut spec = DialogSpec::new(
        rect,
        scope(app, crate::focus::Focus::Navigation)?,
        focus,
        lang.text(" Filename filter "),
        Target::Navigation(Action::ApplyFilter),
        Target::Navigation(Action::Close),
    );
    let x = rect.x + 2;
    let w = rect.width.saturating_sub(4);
    let mut button = |id, y, label: String, action| {
        spec.control(Control::new(
            id,
            Rect::new(x, y, w, 1),
            ControlKind::Button(label),
            Target::Navigation(action),
        ));
    };
    button(
        0,
        rect.y + 1,
        format!(
            "{}: {}",
            lang.text("Filter mode"),
            lang.text(if draft.types_mode {
                "File types"
            } else {
                "Wildcard"
            })
        ),
        Action::FilterMode,
    );
    let content = Rect::new(x, rect.y + 3, w, 6.min(rect.height.saturating_sub(11)));
    if draft.types_mode {
        let offset = draft
            .row
            .saturating_sub(content.height.saturating_sub(1) as usize);
        let items = draft
            .rows
            .iter()
            .map(|row| {
                let name = row.extension.as_ref().map_or_else(
                    || lang.text("No extension").to_owned(),
                    |ext| format!(".{}", display_name(&ext.to_string_lossy())),
                );
                format!(
                    "[{}] {} ({})",
                    if draft.types.selected.contains(&row.extension) {
                        'x'
                    } else {
                        ' '
                    },
                    name,
                    row.count
                )
            })
            .collect();
        let mut list = Control::new(
            1,
            content,
            ControlKind::List {
                items,
                cursor: draft.row,
                offset,
            },
            Target::Navigation(Action::FilterField),
        );
        list.hits.clear();
        spec.control(list);
        for i in 0..content.height as usize {
            if offset + i < draft.rows.len() {
                spec.hit(
                    Rect::new(x, content.y + i as u16, w, 1),
                    Target::Navigation(Action::FilterType(offset + i)),
                );
            }
        }
        spec.control(Control::new(
            2,
            Rect::new(x, rect.y + 10, w, 1),
            ControlKind::Button(
                lang.text(if draft.types.hide {
                    "Hide selected"
                } else {
                    "Show selected"
                })
                .into(),
            ),
            Target::Navigation(Action::FilterPolicy),
        ));
        spec.caption(
            Paragraph::new(lang.text(if draft.limited {
                "Only 256 types shown; use Wildcard for other types."
            } else if draft.rows.iter().all(|row| row.count == 0) {
                "No file types in the loaded listing."
            } else {
                "Space/click selects types; exact case (.jpg differs from .JPG)."
            }))
            .style(styles(theme::TEXT)),
            Rect::new(x, rect.y + 12, w, 2),
        );
    } else {
        let column = Line::from(&field[..cursor]).width();
        let scroll = column.saturating_sub(w.saturating_sub(1) as usize);
        spec.control(Control::new(
            1,
            Rect::new(x, content.y, w, 1),
            ControlKind::ScrolledInput {
                value: display_name(field),
                scroll,
            },
            Target::Navigation(Action::FilterField),
        ));
        if focus == 1 {
            spec.cursor = Some((x + (column - scroll) as u16, content.y));
        }
        spec.caption(
            Paragraph::new(lang.text(
                "Case-sensitive * and ?; empty or * clears.
Parent stays visible. File types keeps directories visible.",
            ))
            .style(styles(theme::TEXT)),
            Rect::new(x, content.y + 2, w, 3),
        );
    }
    spec.caption(
        Paragraph::new(
            lang.text("Loaded entries only; hidden setting applies. Marks are preserved."),
        )
        .style(styles(theme::TEXT)),
        Rect::new(x, rect.y + 14, w, 2),
    );
    for (i, label, action) in [
        (3, "[ Apply ]", Action::ApplyFilter),
        (4, "[ Clear ]", Action::ClearFilter),
        (5, "[ Cancel ]", Action::Close),
    ] {
        let width = w / 3;
        spec.control(Control::new(
            i,
            Rect::new(
                x + (i as u16 - 3) * width,
                rect.bottom().saturating_sub(2),
                width.saturating_sub(1),
                1,
            ),
            ControlKind::Button(lang.text(label).into()),
            Target::Navigation(action),
        ));
    }
    Some(spec)
}
