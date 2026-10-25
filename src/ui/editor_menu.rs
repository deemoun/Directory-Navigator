use crate::painting::{Block, Frame};
use crate::presentation::{layout::Rect, text::Line};
use crate::{
    app::App,
    editor::{menus, EditorAction as A},
    menu::plain,
    mouse::Target,
};
pub(super) fn draw_bar(frame: &mut impl Frame, app: &App, area: Rect) {
    let language = app.config.language;
    let profile = app.config.palette;
    let dos = profile.styles();
    frame.render_widget(Block::default().style(dos(0x70)), area);
    let header = super::clock::header(app, area);
    let current = app
        .desktop
        .active_editor()
        .and_then(|e| e.modals.editor_menu());
    let mut x = area.x;
    for (root, label) in menus::LABELS.iter().enumerate() {
        let label = language.text(label);
        let rect = Rect::new(
            x,
            area.y,
            Line::from(plain(language.text(label))).width() as u16 + 3,
            1,
        )
        .intersection(header);
        frame.render_widget(
            crate::turbo::widgets::menu_title(
                label,
                current.is_some_and(|(r, _)| r == root),
                menus::enabled(root),
            ),
            rect,
        );
        if menus::enabled(root) {
            frame
                .regions()
                .push((rect, Target::Editor(A::MenuRoot(root))));
        }
        x += rect.width;
    }
    super::clock::draw(frame, app, area);
}
pub(super) fn draw_dropdown(frame: &mut impl Frame, app: &App, area: Rect) {
    let language = app.config.language;
    let Some(editor) = app.desktop.active_editor() else {
        return;
    };
    let Some((root, selected)) = editor.modals.editor_menu() else {
        return;
    };
    let entries = menus::items(root);
    let width = entries
        .iter()
        .map(|item| {
            Line::from(plain(language.text(item.label))).width()
                + crate::shortcuts::label(
                    app.config.shortcuts,
                    crate::focus::Screen::Editor,
                    item.shortcut,
                )
                .len()
                + 8
        })
        .max()
        .unwrap_or(20)
        .min(area.width as usize) as u16;
    let x = menus::LABELS[..root]
        .iter()
        .map(|label| Line::from(plain(language.text(label))).width() as u16 + 3)
        .sum::<u16>()
        .min(area.width - width);
    let rect = Rect::new(area.x + x, area.y + 1, width, entries.len() as u16 + 2);
    let preferences = editor.preferences;
    let states = entries
        .iter()
        .map(|item| match item.action {
            Some(A::AutoIndent) => preferences.auto_indent,
            Some(A::Colors) => preferences.syntax,
            Some(A::ExpandTabs) => preferences.expand_tabs,
            _ => false,
        })
        .collect::<Vec<_>>();
    // Consume clicks outside the menu; the underlying document never receives them.
    frame.regions().clear();
    frame.regions().push((area, Target::Editor(A::CancelMenu)));
    draw_bar(frame, app, Rect::new(area.x, area.y, area.width, 1));
    let native = entries
        .iter()
        .enumerate()
        .map(|(row, item)| crate::turbo::widgets::MenuEntry {
            label: language.text(item.label).into(),
            shortcut: crate::shortcuts::label(
                app.config.shortcuts,
                crate::focus::Screen::Editor,
                item.shortcut,
            )
            .into_owned(),
            enabled: item.action.is_some(),
            checked: states[row],
            submenu: false,
        })
        .collect();
    frame.render_widget(crate::turbo::widgets::menu(native, selected), rect);
    frame.regions().push((rect, Target::Editor(A::MenuSurface)));
    for (row, item) in entries.iter().enumerate() {
        if item.action.is_some() {
            frame.regions().push((
                Rect::new(rect.x + 1, rect.y + 1 + row as u16, width - 2, 1),
                Target::Editor(A::MenuRow(row)),
            ));
        }
    }
}
