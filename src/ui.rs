use crate::{app::App, mouse::Target};
use ratatui::{
    layout::{Constraint, Layout},
    widgets::{Block, Paragraph, Wrap},
    Frame,
};
mod archive_view;
mod clock;
mod colors_view;
mod command_history_view;
mod dialog_style;
mod directory_tree_view;
mod discovery_view;
mod editor_dialogs;
mod editor_menu;
mod editor_view;
mod file_tools_view;
mod menu_view;
mod navigation_view;
mod overlays;
mod panel_footer;
mod panel_view;
mod quick_view;
mod screensaver_view;
mod settings_view;
mod shortcuts_view;
mod tetris_view;
mod theme;
mod tools_view;
mod user_menu_view;
mod utilities_view;
mod viewer_view;
mod welcome_view;
use overlays::{draw_info_dialog, draw_operation_overlays};
use panel_view::draw_panel;

pub use theme::entry_color;
use viewer_view::draw_viewer;

pub fn draw(frame: &mut Frame, app: &mut App) {
    draw_content(frame, app);
    shortcuts_view::draw_prefix(frame, app);
}
fn draw_content(frame: &mut Frame, app: &mut App) {
    let profile = app.config.palette;
    let dos = profile.styles();
    let area = frame.area();
    let prepared = crate::layout::prepare(app, area);
    app.mouse.regions.clear();
    if app.screensaver.active {
        screensaver_view::draw_sky(frame, app, area);
        return;
    }
    frame.render_widget(Block::default().style(dos(0x87)), area);
    if area.width < 80 || area.height < 25 {
        frame.render_widget(
            Paragraph::new(
                if app.editor.is_some() {
                    "Directory Navigator\nTerminal too small — resize to 80×25.\nAlt+X requests exit; resize to review unsaved prompts."
                } else { "Directory Navigator\nTerminal too small — resize to 80×25.\nF10 exits." },
            )
            .style(dos(0x3f))
            .wrap(Wrap { trim: false }),
            area,
        );
        return;
    }
    if app.editor.is_some() {
        editor_view::draw(frame, app, area);
        draw_info_dialog(frame, app, prepared.message.as_ref());
        return;
    }
    if app.viewer.is_some() {
        draw_viewer(frame, app, area);
        tools_view::draw_tools(frame, app, area);
        draw_info_dialog(frame, app, prepared.message.as_ref());
        return;
    }
    let regions = &prepared.regions;
    let menu_positions = menu_view::draw_bar(frame, app, regions[0]);
    let panels = &prepared.panel_areas;
    for (index, area) in panels.iter().enumerate() {
        if app
            .quick_view
            .as_ref()
            .is_some_and(|q| q.source ^ 1 == index)
        {
            quick_view::draw(frame, app, *area);
            continue;
        }
        let geometry = &prepared.panels[index];
        draw_panel(
            frame,
            &app.panels[index],
            *area,
            (index, index == app.active),
            &app.config,
            app.quick_search
                .as_ref()
                .filter(|search| search.panel == index),
            geometry,
        );
        app.mouse.regions.push((*area, Target::Panel(index)));
        let panel = &app.panels[index];
        for entry in panel.offset..panel.entries.len().min(panel.offset + geometry.capacity()) {
            if let Some(rect) = geometry.entry_rect(entry, panel.offset) {
                app.mouse.regions.push((rect, Target::Entry(index, entry)));
            }
        }
        if !panel.loading && panel.entries.len() > 1 && geometry.scrollbar.height >= 3 {
            use crate::panel_layout::ScrollAction;
            let bar = geometry.scrollbar;
            app.mouse.regions.extend([
                (
                    ratatui::layout::Rect::new(bar.x, bar.y, 1, 1),
                    Target::Scrollbar(index, ScrollAction::Up),
                ),
                (
                    ratatui::layout::Rect::new(bar.x, bar.bottom() - 1, 1, 1),
                    Target::Scrollbar(index, ScrollAction::Down),
                ),
                (
                    geometry.scroll_track(),
                    Target::Scrollbar(index, ScrollAction::Track),
                ),
            ]);
            if let Some(thumb) = geometry.scroll_thumb(panel.cursor, panel.entries.len()) {
                app.mouse
                    .regions
                    .push((thumb, Target::Scrollbar(index, ScrollAction::Thumb)));
            }
        }
    }
    crate::command_line::draw(frame, app, regions[2]);
    let buttons = Layout::horizontal([Constraint::Percentage(10); 10]).split(regions[3]);
    for (index, label) in [
        "Help", "User", "View", "Edit", "Copy", "Move", "MkDir", "Delete", "Menu", "Quit",
    ]
    .iter()
    .enumerate()
    {
        let enabled = true;
        if enabled {
            app.mouse
                .regions
                .push((buttons[index], Target::Function(index as u8 + 1)));
        }
        let label = if profile == crate::palette::PaletteProfile::ClassicDn && *label == "Delete" {
            "Del"
        } else {
            label
        };
        frame.render_widget(
            Paragraph::new(crate::shortcuts::function(
                app.config.shortcuts,
                profile,
                index as u8 + 1,
                app.config.language.text(label),
                enabled,
            ))
            .style(profile.bar(enabled)),
            buttons[index],
        );
    }
    if app.tetris.is_some() {
        tetris_view::draw(frame, app, area);
        return;
    }
    if app.welcome.dialog.is_some() {
        welcome_view::draw(frame, app, area);
        settings_view::draw_settings(frame, app, area);
        draw_info_dialog(frame, app, prepared.message.as_ref());
        return;
    }
    menu_view::draw_dropdowns(frame, app, &menu_positions, area);
    draw_operation_overlays(frame, app, area);
    settings_view::draw_settings(frame, app, area);
    tools_view::draw_tools(frame, app, area);
    file_tools_view::draw(frame, app, area);
    discovery_view::draw(frame, app, area);
    directory_tree_view::draw(frame, app, area);
    user_menu_view::draw(frame, app, area);
    archive_view::draw(frame, app, area);
    navigation_view::draw(frame, app, area);
    utilities_view::draw(frame, app, area);
    colors_view::draw(frame, app, area);
    screensaver_view::draw_setup(frame, app, area);
    command_history_view::draw(frame, app, area);
    draw_info_dialog(frame, app, prepared.message.as_ref());
}
