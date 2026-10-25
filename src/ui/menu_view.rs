use crate::painting::Frame;
use crate::presentation::{layout::Rect, text::Line};
use crate::{
    app::App,
    menu::{self, Action},
    mouse::Target,
};

fn menu_width(
    entries: &[menu::Item],
    language: crate::localization::Language,
    shortcuts: crate::shortcuts::Profile,
) -> u16 {
    let names = entries
        .iter()
        .map(|item| Line::from(menu::plain(language.text(item.label))).width())
        .max()
        .unwrap_or(0);
    let keys = entries
        .iter()
        .map(|item| {
            crate::shortcuts::label(shortcuts, crate::focus::Screen::Manager, item.shortcut).len()
        })
        .max()
        .unwrap_or(0);
    (names + keys + 8).max(18) as u16
}

pub(super) fn draw_dropdowns(frame: &mut impl Frame, app: &App, bar: &[Rect], area: Rect) {
    let language = app.config.language;
    let Some((root, _)) = app.modals.menu() else {
        return;
    };
    let mut parent: Option<Rect> = None;
    for depth in 0..=app.modals.submenus().len() {
        let entries = menu::level_items(app, depth);
        let width = menu_width(&entries, language, app.config.shortcuts).min(area.width);
        let height = (entries.len() as u16 + 2).min(area.height - 1);
        let (x, y) = if let Some(parent) = parent {
            let right = parent.right() - 1;
            let x = if right + width <= area.right() {
                right
            } else {
                parent.x.saturating_sub(width - 1).max(area.x)
            };
            (
                x,
                (parent.y + 1 + menu::selected_row(app, depth - 1) as u16)
                    .min(area.bottom() - height),
            )
        } else {
            (bar[root].x.min(area.right() - width), area.y + 1)
        };
        let rect = Rect::new(x, y, width, height);
        let native = entries
            .iter()
            .map(|item| crate::turbo::widgets::MenuEntry {
                label: if item.action == Action::Separator {
                    String::new()
                } else {
                    language.text(item.label).into()
                },
                shortcut: crate::shortcuts::label(
                    app.config.shortcuts,
                    crate::focus::Screen::Manager,
                    item.shortcut,
                )
                .into_owned(),
                enabled: item.selectable(),
                checked: match item.action {
                    Action::Sort(mode) => app.panels[app.active].sort == mode,
                    Action::Layout(layout) => app.config.layouts[app.active] == layout,
                    Action::Hidden => app.config.show_hidden,
                    Action::QuickView => app.quick_view.is_some(),
                    _ => false,
                },
                submenu: matches!(item.action, Action::Submenu(_)),
            })
            .collect();
        frame.render_widget(
            crate::turbo::widgets::menu(native, menu::selected_row(app, depth)),
            rect,
        );
        frame.regions().push((rect, Target::MenuSurface));
        for (row, item) in entries
            .iter()
            .enumerate()
            .take(height.saturating_sub(2) as usize)
        {
            if item.action != Action::Separator {
                frame.regions().push((
                    Rect::new(x + 1, y + 1 + row as u16, width - 2, 1),
                    Target::MenuItem(depth, row),
                ));
            }
        }
        parent = Some(rect);
    }
}
