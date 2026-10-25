use crate::{app::App, mouse::Target};
use ratatui::{
    layout::{Constraint, Layout},
    widgets::{Block, Paragraph, Wrap},
    Frame,
};
mod archive_view;
mod calculator_view;
mod clock;
mod colors_view;
mod command_history_view;
mod config_import_view;
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
mod windows_view;
use overlays::{draw_info_dialog, draw_operation_overlays};
use panel_view::draw_panel;

pub use theme::entry_color;
use viewer_view::draw_viewer;

pub fn draw(frame: &mut Frame, app: &mut App) {
    #[cfg(feature = "turbo-vision")]
    app.desktop.native().set_scene(None);
    draw_content(frame, app, false);
    shortcuts_view::draw_prefix(frame, app);
}
/// Native manager and game content is painted by its TV document.
/// `draw` remains the legacy renderer for comparison and headless contracts.
#[cfg(feature = "turbo-vision")]
pub fn draw_native(frame: &mut Frame, app: &mut App) {
    app.desktop.native().set_scene(None);
    draw_content(frame, app, true);
    shortcuts_view::draw_prefix(frame, app);
}
fn draw_content(frame: &mut Frame, app: &mut App, native_drawing: bool) {
    let profile = app.config.palette;
    let dos = profile.styles();
    let area = frame.area();
    let screen = app.desktop.active().kind;
    let prepared = crate::layout::prepare(app, area);
    app.mouse.regions.clear();
    if app.screensaver.active {
        screensaver_view::draw_sky(frame, app, area);
        return;
    }
    let native_shell = native_drawing
        && matches!(
            screen,
            crate::desktop::WindowKind::Manager | crate::desktop::WindowKind::Game
        )
        && area.width >= 80
        && area.height >= 25;
    if !native_shell {
        frame.render_widget(Block::default().style(dos(0x87)), area);
    }
    if area.width < 80 || area.height < 25 {
        frame.render_widget(
            Paragraph::new(
                if screen == crate::desktop::WindowKind::Editor {
                    "Directory Navigator\nTerminal too small — resize to 80×25.\nAlt+X requests exit; resize to review unsaved prompts."
                } else { "Directory Navigator\nTerminal too small — resize to 80×25.\nF10 exits." },
            )
            .style(dos(0x3f))
            .wrap(Wrap { trim: false }),
            area,
        );
        return;
    }
    if screen == crate::desktop::WindowKind::Editor {
        editor_view::draw(frame, app, area);
        windows_view::draw(frame, app, area);
        draw_info_dialog(frame, app, prepared.message.as_ref());
        return;
    }
    if screen == crate::desktop::WindowKind::Viewer {
        draw_viewer(frame, app, area);
        tools_view::draw_tools(frame, app, area);
        windows_view::draw(frame, app, area);
        draw_info_dialog(frame, app, prepared.message.as_ref());
        return;
    }
    let regions = &prepared.regions;
    #[cfg(feature = "turbo-vision")]
    let menu_positions = if native_shell {
        let (scene, positions) =
            crate::turbo::manager::prepare(app, &prepared, area.width, area.height);
        if let Some(cursor) = scene.cursor {
            frame.set_cursor_position(cursor);
        }
        scene.seed_overlay_styles(frame.buffer_mut());
        app.desktop.native().set_scene(Some(scene));
        positions
    } else {
        menu_view::draw_bar(frame, app, regions[0])
    };
    #[cfg(not(feature = "turbo-vision"))]
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
        if !native_shell {
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
        }
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
    if !native_shell {
        crate::command_line::draw(frame, app, regions[2]);
    } else {
        app.mouse.regions.push((regions[2], Target::CommandLine));
    }
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
        if !native_shell {
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
    }
    if screen == crate::desktop::WindowKind::Game {
        #[cfg(feature = "turbo-vision")]
        if native_drawing {
            let scene = crate::turbo::tetris::prepare(app, area.width, area.height);
            if let Some(cursor) = scene.cursor {
                frame.set_cursor_position(cursor);
            }
            // Only replace cells covered by the game; compatibility quick-view
            // content outside it must keep its overlay and hit-target behavior.
            scene.seed_overlay_styles(frame.buffer_mut());
            app.desktop.native().compose_scene(scene);
            return;
        }
        tetris_view::draw(frame, app, area);
        return;
    }
    if app.welcome.dialog.is_some() {
        welcome_view::draw(frame, app, area);
        settings_view::draw_settings(frame, app, area);
        windows_view::draw(frame, app, area);
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
    config_import_view::draw(frame, app, area);
    windows_view::draw(frame, app, area);
    draw_info_dialog(frame, app, prepared.message.as_ref());
}
