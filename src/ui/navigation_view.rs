use crate::painting::{Frame, Paragraph};
use crate::presentation::{layout::Rect, text::Line};
use crate::{
    app::App,
    filesystem::display_name,
    formatting::tail,
    mouse::Target,
    navigation_tools::{Action, Modal},
};
pub(super) fn draw(frame: &mut impl Frame, app: &App, area: Rect) {
    let profile = app.config.palette;
    let dos = profile.styles();
    let Some(modal) = app.modals.navigation_dialog() else {
        return;
    };
    let height = if matches!(modal, Modal::Bookmarks { .. }) {
        18
    } else {
        11
    };
    let width = area.width.saturating_sub(6).min(94);
    let rect = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    let title = if matches!(modal, Modal::Bookmarks { .. }) {
        " Quick directories "
    } else {
        " Filename filter "
    };
    super::dialog_style::window(
        frame,
        profile,
        rect,
        rect.width,
        rect.height,
        app.config.language.text(title),
    );
    frame.regions().clear();
    super::dialog_style::close_button(frame, profile, rect, Target::Navigation(Action::Close));
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
            frame.render_widget(
                crate::turbo::widgets::list(items, *slot, 0, *focus == 0),
                Rect::new(x, rect.y + 1, w, 9),
            );
            for i in 0..9 {
                frame.regions().push((
                    Rect::new(x, rect.y + 1 + i as u16, w, 1),
                    Target::Navigation(Action::Slot(i)),
                ));
            }
            frame.render_widget(
                Paragraph::new(
                    app.config
                        .language
                        .text("↑↓ / 1–9 choose · Tab moves · Ctrl+1–9 set · Alt+1–9 go"),
                )
                .style(dos(0x70)),
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
            frame.render_widget(Paragraph::new(app.config.language.text("Visible cached filenames · case-sensitive * and ?\nEmpty or * clears · Parent stays visible\nFiltered marks are preserved; operations use visible marks")).style(dos(0x70)),Rect::new(x,rect.y+1,w,3));
            let input = Rect::new(x, rect.y + 5, w, 1);
            let column = Line::from(&field[..*cursor]).width();
            let scroll = column.saturating_sub(w.saturating_sub(1) as usize);
            frame.render_widget(
                crate::turbo::widgets::input_scrolled(&display_name(field), scroll, *focus == 0),
                input,
            );
            frame
                .regions()
                .push((input, Target::Navigation(Action::FilterField)));
            if *focus == 0 {
                frame.set_cursor_position((x + (column - scroll) as u16, input.y));
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
    for (rect, label, action, focus) in buttons {
        super::dialog_style::button(frame, profile, rect, app.config.language.text(label), focus);
        frame.regions().push((rect, Target::Navigation(action)));
    }
}
