use crate::painting::{Block, Borders, Clear, Frame, Paragraph};
use crate::presentation::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
};
use crate::{
    app::App,
    menu::{self, Action},
    mouse::Target,
};

pub(super) fn label_spans(label: &str, normal: Style, hot: Style) -> Vec<Span<'_>> {
    label
        .split('~')
        .enumerate()
        .map(|(index, part)| Span::styled(part, if index % 2 == 1 { hot } else { normal }))
        .collect()
}

pub(super) fn draw_bar(frame: &mut impl Frame, app: &mut App, area: Rect) -> Vec<Rect> {
    let language = app.config.language;
    let profile = app.config.palette;
    let dos = profile.styles();
    frame.render_widget(Block::default().style(dos(0x70)), area);
    let header = super::clock::header(app, area);
    let mut x = area.x;
    let mut positions = Vec::new();
    for (index, label) in menu::LABELS.iter().enumerate() {
        let label = language.text(label);
        let width = Line::from(menu::plain(label)).width() as u16 + 3;
        let rect = Rect::new(x, area.y, width, 1).intersection(header);
        let selected = app.menu.is_some_and(|(root, _)| root == index);
        let normal = dos(if selected { 0x30 } else { 0x70 });
        let hot = dos(if selected { 0x3e } else { 0x74 });
        let mut spans = vec![Span::styled(" ", normal)];
        spans.extend(label_spans(label, normal, hot));
        spans.push(Span::styled("  ", normal));
        frame.render_widget(Paragraph::new(Line::from(spans)), rect);
        positions.push(rect);
        app.mouse.regions.push((rect, Target::Menu(index)));
        x += width;
    }
    super::clock::draw(frame, app, area);
    positions
}

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

pub(super) fn draw_dropdowns(frame: &mut impl Frame, app: &mut App, bar: &[Rect], area: Rect) {
    let language = app.config.language;
    let profile = app.config.palette;
    let dos = profile.styles();
    let Some((root, _)) = app.menu else {
        return;
    };
    let mut parent: Option<Rect> = None;
    for depth in 0..=app.submenus.len() {
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
        let shadow = Rect::new(x + 1, y + 1, width, height).intersection(area);
        frame.render_widget(Block::default().style(dos(0x08)), shadow);
        frame.render_widget(Clear, rect);
        frame.render_widget(
            Block::default().borders(Borders::ALL).style(dos(0x70)),
            rect,
        );
        app.mouse.regions.push((rect, Target::MenuSurface));
        for (row, item) in entries
            .iter()
            .enumerate()
            .take(height.saturating_sub(2) as usize)
        {
            let row_rect = Rect::new(x + 1, y + 1 + row as u16, width - 2, 1);
            if item.action == Action::Separator {
                frame.render_widget(
                    Paragraph::new(format!(
                        "├{}┤",
                        "─".repeat(width.saturating_sub(2) as usize)
                    ))
                    .style(dos(0x70)),
                    Rect::new(x, row_rect.y, width, 1),
                );
                continue;
            }
            let selected = row == menu::selected_row(app, depth);
            let normal = dos(if !item.selectable() {
                0x78
            } else if selected {
                0x30
            } else {
                0x70
            });
            let hot = if !item.selectable() {
                normal
            } else {
                dos(if selected { 0x3e } else { 0x74 })
            };
            let checked = match item.action {
                Action::Sort(mode) => app.panels[app.active].sort == mode,
                Action::Layout(layout) => app.config.layouts[app.active] == layout,
                Action::Hidden => app.config.show_hidden,
                Action::QuickView => app.quick_view.is_some(),
                _ => false,
            };
            let shortcut = crate::shortcuts::label(
                app.config.shortcuts,
                crate::focus::Screen::Manager,
                item.shortcut,
            );
            let label_width = Line::from(menu::plain(language.text(item.label))).width();
            let padding = (width as usize - 2).saturating_sub(2 + label_width + shortcut.len() + 2);
            let mut spans = vec![Span::styled(if checked { "• " } else { "  " }, normal)];
            spans.extend(label_spans(language.text(item.label), normal, hot));
            spans.push(Span::styled(" ".repeat(padding), normal));
            spans.push(Span::styled(shortcut.into_owned(), normal));
            spans.push(Span::styled(
                if matches!(item.action, Action::Submenu(_)) {
                    " ▶"
                } else {
                    "  "
                },
                normal,
            ));
            frame.render_widget(Paragraph::new(Line::from(spans)), row_rect);
            app.mouse
                .regions
                .push((row_rect, Target::MenuItem(depth, row)));
        }
        parent = Some(rect);
    }
}
