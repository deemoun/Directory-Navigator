use crate::painting::{Block, Frame, Paragraph, Wrap};
use crate::presentation::layout::{Constraint, Layout};
use crate::{app::App, mouse::Target};
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
mod quick_view;
mod screensaver_view;
mod settings_view;
mod shortcuts_view;
mod theme;
mod tools_view;
mod user_menu_view;
mod utilities_view;
mod viewer_view;
mod welcome_view;
mod windows_view;
use overlays::{draw_info_dialog, draw_operation_overlays};

pub use theme::entry_color;
use viewer_view::draw_viewer;

/// Compatibility entry point: prepare viewport state, then perform read-only paint.
pub fn draw(frame: &mut crate::painting::NativeFrame, app: &mut App) {
    let snapshot = crate::layout::prepare(app, frame.area());
    let result = render(app, &snapshot);
    publish(app, &result);
    frame.replace(result.scene);
}
pub struct RenderedFrame {
    pub(crate) scene: crate::turbo::scene::Scene,
    pub context: crate::layout::FrameContext,
    dialogs: Vec<std::rc::Rc<std::cell::RefCell<crate::turbo::composition::DialogGroup>>>,
}
impl RenderedFrame {
    pub fn hit_regions(&self) -> &[(crate::presentation::layout::Rect, crate::mouse::Target)] {
        &self.scene.regions
    }
    pub fn cursor(&self) -> Option<(u16, u16)> {
        self.scene.cursor
    }
}
pub fn render(app: &App, snapshot: &crate::layout::LayoutSnapshot) -> RenderedFrame {
    let mut frame = crate::painting::NativeFrame::new(
        crate::presentation::layout::Rect::new(0, 0, snapshot.bounds.width, snapshot.bounds.height),
        app.config.palette,
    );
    draw_content(&mut frame, app, snapshot);
    shortcuts_view::draw_prefix(&mut frame, app);
    let (mut scene, dialogs) = frame.finish_dialogs();
    scene.normalize_wide();
    snapshot.transform(&mut scene);
    scene.normalize_wide();
    RenderedFrame {
        scene,
        context: snapshot.context,
        dialogs,
    }
}
pub(crate) fn publish(app: &mut App, frame: &RenderedFrame) {
    app.mouse.regions = frame.scene.regions.clone();
    app.mouse.frame = Some(frame.context);
    app.mouse.dialogs = frame.dialogs.clone();
}
pub(crate) fn native_scene(
    app: &mut App,
    area: crate::presentation::layout::Rect,
) -> crate::turbo::scene::Scene {
    let snapshot = crate::layout::prepare(app, area);
    let result = render(app, &snapshot);
    publish(app, &result);
    result.scene
}

fn draw_content(frame: &mut impl Frame, app: &App, prepared: &crate::layout::LayoutSnapshot) {
    let profile = app.config.palette;
    let dos = profile.styles();
    let area = frame.area();
    let screen = app.desktop.active().kind;
    frame.regions().clear();
    if app.screensaver.active {
        screensaver_view::draw_sky(frame, app, area);
        return;
    }
    frame.render_widget(Block::default().style(dos(0x87)), area);
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
        draw_layers(frame, app, prepared, &[]);
        return;
    }
    if screen == crate::desktop::WindowKind::Viewer {
        draw_viewer(frame, app, area);
        draw_layers(frame, app, prepared, &[]);
        return;
    }
    let regions = &prepared.regions;
    let (scene, menu_positions) =
        crate::turbo::manager::prepare(app, prepared, area.width, area.height);
    frame.compose(scene);
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
        frame.regions().push((*area, Target::Panel(index)));
        let panel = &app.panels[index];
        for entry in panel.offset..panel.entries.len().min(panel.offset + geometry.capacity()) {
            if let Some(rect) = geometry.entry_rect(entry, panel.offset) {
                frame.regions().push((rect, Target::Entry(index, entry)));
            }
        }
        if !panel.loading && panel.entries.len() > 1 && geometry.scrollbar.height >= 3 {
            use crate::panel_layout::ScrollAction;
            let bar = geometry.scrollbar;
            frame.regions().extend([
                (
                    crate::presentation::layout::Rect::new(bar.x, bar.y, 1, 1),
                    Target::Scrollbar(index, ScrollAction::Up),
                ),
                (
                    crate::presentation::layout::Rect::new(bar.x, bar.bottom() - 1, 1, 1),
                    Target::Scrollbar(index, ScrollAction::Down),
                ),
                (
                    geometry.scroll_track(),
                    Target::Scrollbar(index, ScrollAction::Track),
                ),
            ]);
            if let Some(thumb) = geometry.scroll_thumb(panel.cursor, panel.entries.len()) {
                frame
                    .regions()
                    .push((thumb, Target::Scrollbar(index, ScrollAction::Thumb)));
            }
        }
    }
    frame.regions().push((regions[2], Target::CommandLine));
    let buttons = Layout::horizontal([Constraint::Percentage(10); 10]).split(regions[3]);
    for (index, rect) in buttons.iter().enumerate() {
        frame
            .regions()
            .push((*rect, Target::Function(index as u8 + 1)));
    }
    if screen == crate::desktop::WindowKind::Game {
        frame.regions().clear();
        frame.compose(crate::turbo::tetris::prepare(app, area.width, area.height));
        draw_layers(frame, app, prepared, &menu_positions);
        return;
    }
    draw_layers(frame, app, prepared, &menu_positions);
}
fn draw_layers(
    frame: &mut impl Frame,
    app: &App,
    prepared: &crate::layout::LayoutSnapshot,
    menu_positions: &[crate::presentation::layout::Rect],
) {
    use crate::focus::Focus;
    let area = frame.area();
    let layers = app.visible_modals();
    for (index, modal) in layers.iter().enumerate() {
        if layers[index + 1..]
            .iter()
            .any(|next| next.focus == modal.focus)
        {
            continue;
        }
        frame.modal_scope(Some(*modal));
        match modal.focus {
            Focus::Message => draw_info_dialog(frame, app, prepared.message.as_ref()),
            Focus::WindowList => windows_view::draw(frame, app, area),
            Focus::Welcome => welcome_view::draw(frame, app, area),
            Focus::EditorModal => editor_dialogs::draw(frame, app, area),
            Focus::EditorMenu => editor_menu::draw_dropdown(frame, app, area),
            Focus::ViewerSearch => viewer_view::draw_search(frame, app, area),
            Focus::Menu => menu_view::draw_dropdowns(frame, app, menu_positions, area),
            Focus::Prompt | Focus::Job => draw_operation_overlays(frame, app, area, modal.focus),
            Focus::Settings => settings_view::draw_settings(frame, app, area),
            Focus::Pattern | Focus::Columns | Focus::FileInfo => {
                tools_view::draw_tools(frame, app, area, modal.focus)
            }
            Focus::FileTools => file_tools_view::draw(frame, app, area),
            Focus::Discovery => discovery_view::draw(frame, app, area),
            Focus::Tree => directory_tree_view::draw(frame, app, area),
            Focus::UserMenu => user_menu_view::draw(frame, app, area),
            Focus::Archive => archive_view::draw(frame, app, area),
            Focus::Navigation => navigation_view::draw(frame, app, area),
            Focus::Utility => utilities_view::draw(frame, app, area),
            Focus::Colors => colors_view::draw(frame, app, area),
            Focus::SaverSetup => screensaver_view::draw_setup(frame, app, area),
            Focus::CommandHistory => command_history_view::draw(frame, app, area),
            Focus::ConfigImport => config_import_view::draw(frame, app, area),
            _ => {}
        }
        frame.modal_scope(None);
    }
}
