use crate::painting::{Block, Frame, Paragraph, Wrap};
use crate::presentation::layout::{Constraint, Layout, Rect};
use crate::{app::App, mouse::Target};
mod archive_view;
mod clock;
mod colors_view;
mod command_history_view;
mod config_import_view;
mod copy_dialog_view;
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
pub(crate) use archive_view::archive_spec;
pub(crate) use command_history_view::command_history_spec;
pub(crate) use config_import_view::import_spec;
pub(crate) use copy_dialog_view::copy_spec;
pub(crate) use directory_tree_view::tree_spec;
pub(crate) use discovery_view::discovery_spec;
pub(crate) use editor_dialogs::editor_spec;
pub(crate) use file_tools_view::file_tools_spec;
pub(crate) use navigation_view::navigation_spec;
pub(crate) use overlays::prompt_spec;
use overlays::{draw_info_dialog, draw_operation_overlays};
pub(crate) use settings_view::{settings_choice_bounds, settings_spec};
pub(crate) use user_menu_view::user_menu_spec;
pub(crate) use utilities_view::utility_spec;
pub(crate) use viewer_view::viewer_search_spec;
pub(crate) use windows_view::windows_spec;
pub(crate) fn paint_spec_close(
    frame: &mut impl Frame,
    app: &App,
    rect: crate::presentation::layout::Rect,
    target: Target,
) {
    dialog_style::close_button(frame, app.config.palette, rect, target);
}

pub use theme::entry_color;
use viewer_view::draw_viewer;

/// Compatibility entry point: prepare viewport state, then perform read-only paint.
pub fn draw(frame: &mut crate::painting::NativeFrame, app: &mut App) {
    let snapshot = crate::layout::prepare(app, frame.area());
    let result = render(app, &snapshot);
    publish(app, &result);
    frame.replace(result.scene);
}
/// A published surface shares the frame revision used to validate input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfaceFrame {
    pub surface: crate::windowing::geometry::Surface,
    pub clip: Rect,
    pub revision: u64,
    pub cursor: Option<(u16, u16)>,
    pub hits: Vec<(Rect, Target)>,
}
pub struct RenderedFrame {
    pub(crate) scene: crate::turbo::scene::Scene,
    pub context: crate::layout::FrameContext,
    pub(crate) surfaces: Vec<crate::windowing::geometry::Surface>,
    surface_frames: Vec<SurfaceFrame>,
    dialogs: Vec<std::rc::Rc<std::cell::RefCell<crate::turbo::composition::DialogGroup>>>,
}
impl RenderedFrame {
    pub fn surface_frames(&self) -> &[SurfaceFrame] {
        &self.surface_frames
    }

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
    frame.placements(&snapshot.surfaces);
    draw_workspace(&mut frame, app, snapshot);
    for surface in &snapshot.surfaces {
        if let crate::windowing::geometry::SurfaceKey::Modal(id) = surface.id.key {
            let native = app
                .visible_modals()
                .iter()
                .find(|m| m.id == id)
                .is_some_and(|m| match m.focus {
                    crate::focus::Focus::Message => {
                        app.modals.native_help(app.desktop.active().id).is_some()
                    }
                    crate::focus::Focus::Welcome => true,
                    crate::focus::Focus::EditorModal => matches!(
                        app.desktop
                            .active_editor()
                            .and_then(|e| e.modals.editor_modal()),
                        Some(crate::editor::Modal::File(_))
                    ),
                    _ => false,
                });
            if native {
                let mut overlay =
                    crate::painting::NativeFrame::new(frame.area(), app.config.palette).finish();
                crate::windowing::workspace::chrome(&mut overlay, *surface);
                overlay.cursor = frame.cursor();
                frame.compose(overlay);
            }
        }
    }
    if snapshot.manipulation.is_some() {
        frame.render_widget(
            Paragraph::new(
                app.config
                    .language
                    .text("Arrows move · Shift+arrows size · Enter accepts · Esc cancels"),
            )
            .style(app.config.palette.dos(0x70)),
            crate::presentation::layout::Rect::new(
                0,
                frame.area().height.saturating_sub(1),
                frame.area().width,
                1,
            ),
        );
        frame.clear_cursor();
    }
    shortcuts_view::draw_prefix(&mut frame, app);
    let (mut scene, dialogs) = frame.finish_dialogs();
    if let Some(surface) = snapshot
        .surfaces
        .iter()
        .find(|s| Some(s.id) == snapshot.manipulation)
    {
        crate::windowing::workspace::highlight_manipulation(&mut scene, *surface);
    }
    scene.normalize_wide();
    snapshot.transform(&mut scene);
    scene.normalize_wide();
    let active_surface = snapshot.surfaces.iter().find(|s| {
        s.id.key == crate::windowing::geometry::SurfaceKey::Document(snapshot.context.owner)
    });
    let surfaces = snapshot
        .surfaces
        .iter()
        .filter(|s| match s.id.key {
            crate::windowing::geometry::SurfaceKey::Document(id) => {
                (!active_surface.is_some_and(|a| a.maximized) || id == snapshot.context.owner)
                    && !(snapshot.context.owner == crate::desktop::WindowId::MANAGER && s.maximized)
            }
            _ => true,
        })
        .copied()
        .collect::<Vec<_>>();
    let surface_frames = surfaces
        .iter()
        .map(|surface| {
            let clip = snapshot.to_screen(surface.bounds);
            let owner = match surface.id.key {
                crate::windowing::geometry::SurfaceKey::Document(id)
                | crate::windowing::geometry::SurfaceKey::Page(id, _) => id,
                crate::windowing::geometry::SurfaceKey::Modal(_)
                | crate::windowing::geometry::SurfaceKey::ModalPage(..) => snapshot.context.owner,
            };
            let foreground = crate::windowing::workspace::foreground(app);
            SurfaceFrame {
                surface: *surface,
                clip,
                revision: snapshot.context.revision,
                cursor: scene.cursor.filter(|p| {
                    owner == snapshot.context.owner
                        && clip.contains((*p).into())
                        && foreground.is_none_or(|f| f.id == surface.id)
                }),
                hits: scene
                    .regions
                    .iter()
                    .filter(|(r, target)| match target {
                        Target::Surface(id, _) => *id == surface.id,
                        _ => {
                            clip.contains((r.x, r.y).into())
                                && owner == snapshot.context.owner
                                && foreground.is_none_or(|f| f.id == surface.id)
                        }
                    })
                    .copied()
                    .collect(),
            }
        })
        .collect();
    RenderedFrame {
        scene,
        context: snapshot.context,
        surface_frames,
        dialogs,
        surfaces,
    }
}
pub(crate) fn publish(app: &mut App, frame: &RenderedFrame) {
    app.mouse.regions = frame.scene.regions.clone();
    app.mouse.frame = Some(frame.context);
    app.mouse.dialogs = frame.dialogs.clone();
    app.mouse.surfaces = frame.surfaces.clone();
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

fn draw_content(
    frame: &mut impl Frame,
    app: &App,
    prepared: &crate::layout::LayoutSnapshot,
    overlays: bool,
) {
    let profile = app.config.palette;
    let dos = profile.styles();
    let area = frame.area();
    let screen = app.desktop.target_info().kind;
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
        if overlays {
            draw_layers(frame, app, prepared, &[]);
        }
        return;
    }
    if screen == crate::desktop::WindowKind::Viewer {
        draw_viewer(frame, app, area);
        if overlays {
            draw_layers(frame, app, prepared, &[]);
        }
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
    if crate::menu::bar_visible(app) {
        if app.config.hide_menu {
            let (bar, _) = crate::turbo::manager::menu_bar(app, area.width);
            frame.compose(bar);
        }
        for (index, rect) in menu_positions.iter().enumerate() {
            frame.regions().push((*rect, Target::Menu(index)));
        }
    } else if screen == crate::desktop::WindowKind::Manager && app.config.hide_menu {
        frame.regions().push((
            crate::presentation::layout::Rect::new(0, 0, area.width, 1),
            Target::RevealMenu,
        ));
    }
    if let crate::desktop::WindowKind::Plugin(id) = screen {
        frame.regions().clear();
        if let Some(b) = app.desktop.plugins.binding(id) {
            crate::plugins::host::draw(frame, app, area, b);
        }
        if overlays {
            draw_layers(frame, app, prepared, &menu_positions);
        }
        return;
    }
    if overlays {
        draw_layers(frame, app, prepared, &menu_positions);
    }
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
        if modal.focus == Focus::EditorMenu
            && prepared.surfaces.iter().any(|s| {
                s.id.key
                    == crate::windowing::geometry::SurfaceKey::Document(app.desktop.active().id)
                    && !s.maximized
            })
        {
            continue;
        }
        if crate::dialog_adapter::paint(frame, app, area, modal.focus) {
            frame.modal_scope(None);
            continue;
        }
        match modal.focus {
            Focus::Message => draw_info_dialog(frame, app, prepared.message.as_ref()),
            Focus::Welcome => welcome_view::draw(frame, app, area),
            Focus::EditorModal => editor_dialogs::draw(frame, app, area),
            Focus::EditorMenu => editor_menu::draw_dropdown(frame, app, area),
            Focus::ViewerSearch => viewer_view::draw_search(frame, app, area),
            Focus::Menu => menu_view::draw_dropdowns(frame, app, menu_positions, area),
            Focus::CopyHistory => copy_dialog_view::history(frame, app, area),
            Focus::Prompt | Focus::Job => draw_operation_overlays(frame, app, area, modal.focus),
            Focus::Settings => settings_view::draw_settings(frame, app, area),
            Focus::Pattern | Focus::Columns | Focus::FileInfo => {
                tools_view::draw_tools(frame, app, area, modal.focus)
            }
            Focus::Plugin => {
                if let Some(id) = app.modals.plugin_dialog() {
                    if let Some(b) = app.desktop.plugins.binding(id) {
                        crate::plugins::host::draw(frame, app, area, b);
                    }
                }
            }
            Focus::Colors => colors_view::draw(frame, app, area),
            Focus::SaverSetup => screensaver_view::draw_setup(frame, app, area),
            _ => {}
        }
        frame.modal_scope(None);
    }
}

pub(crate) fn remaining_spec(
    app: &App,
    area: crate::presentation::layout::Rect,
    focus: crate::focus::Focus,
) -> Option<crate::turbo::dialog_spec::DialogSpec<'_>> {
    use crate::focus::Focus;
    if let Some(adapter) = crate::dialog_adapter::get(focus) {
        return (adapter.spec)(app, area);
    }
    match focus {
        Focus::Plugin => crate::plugins::host::controls(app, area),
        Focus::Message if app.modals.native_help(app.desktop.active().id).is_none() => {
            let d = app.last_dialog()?;
            let m = crate::layout::dialog_layout(area, d, app.config.language);
            overlays::message_spec(app, &m)
        }
        Focus::Job => overlays::job_spec(app, area),
        Focus::CopyHistory => copy_dialog_view::history_spec(app, area),
        Focus::Pattern | Focus::Columns | Focus::FileInfo => {
            tools_view::tools_spec(app, area, focus)
        }
        Focus::Colors => colors_view::colors_spec(app, area),
        Focus::SaverSetup => screensaver_view::saver_spec(app, area),
        _ => None,
    }
}

fn draw_workspace(
    frame: &mut crate::painting::NativeFrame,
    app: &App,
    prepared: &crate::layout::LayoutSnapshot,
) {
    use crate::{
        desktop::{WindowId, WindowKind},
        windowing::geometry::SurfaceKey,
    };
    let active = app.desktop.active();
    let active_surface = prepared
        .surfaces
        .iter()
        .find(|s| s.id.key == SurfaceKey::Document(active.id));
    if app.screensaver.active
        || frame.area().width < 80
        || frame.area().height < 25
        || active_surface.is_some_and(|s| s.maximized)
    {
        draw_content(frame, app, prepared, true);
        if app.modal_top().is_none()
            && !app.screensaver.active
            && frame.area().width >= 80
            && frame.area().height >= 25
        {
            if let Some(surface) = active_surface {
                let mut overlay =
                    crate::painting::NativeFrame::new(frame.area(), app.config.palette).finish();
                crate::windowing::workspace::chrome(&mut overlay, *surface);
                overlay.cursor = frame.cursor();
                frame.compose(overlay);
            }
        }
        return;
    }
    app.desktop.render_as(WindowId::MANAGER, || {
        draw_content(frame, app, prepared, false)
    });
    let manager_cursor = frame.cursor();
    for id in app.desktop.native().z_order() {
        if id == WindowId::MANAGER {
            continue;
        }
        let Some(surface) = prepared
            .surfaces
            .iter()
            .find(|s| s.id.key == SurfaceKey::Document(id))
            .copied()
        else {
            continue;
        };
        if surface.maximized && active.id == WindowId::MANAGER {
            continue;
        }
        let Some(info) = app.desktop.windows().find(|w| w.id == id) else {
            continue;
        };
        if let WindowKind::Plugin(plugin) = info.kind {
            if let Some(binding) = app.desktop.plugins.binding(plugin) {
                let mut layer = crate::painting::NativeFrame::new(frame.area(), app.config.palette);
                crate::plugins::host::draw(&mut layer, app, frame.area(), binding);
                let mut scene = layer.finish();
                if id != active.id {
                    scene.regions.clear();
                    scene.cursor = None;
                    scene.regions.push((
                        surface.bounds,
                        Target::Surface(surface.id, crate::windowing::geometry::Chrome::Activate),
                    ));
                    crate::windowing::workspace::chrome(&mut scene, surface);
                }
                crate::windowing::workspace::occlude(frame, surface.bounds);
                frame.compose(scene);
            }
            continue;
        }
        let floating = !surface.maximized;
        let size = if floating {
            (
                surface.bounds.width.saturating_sub(2),
                surface.bounds.height.saturating_sub(1),
            )
        } else {
            (surface.bounds.width, surface.bounds.height)
        };
        let mut local = crate::painting::NativeFrame::new(
            crate::presentation::layout::Rect::new(0, 0, size.0, size.1),
            app.config.palette,
        );
        app.desktop.render_as(id, || match info.kind {
            WindowKind::Editor => {
                editor_view::draw(
                    &mut local,
                    app,
                    crate::presentation::layout::Rect::new(0, 0, size.0, size.1),
                );
            }
            WindowKind::Viewer => draw_viewer(
                &mut local,
                app,
                crate::presentation::layout::Rect::new(0, 0, size.0, size.1),
            ),
            _ => {}
        });
        let mut scene = local.finish();
        if floating {
            crate::windowing::workspace::float_document(
                &mut scene,
                surface,
                info.kind,
                frame.area(),
                app,
            );
        }
        if floating
            && id == active.id
            && info.kind == WindowKind::Editor
            && app
                .desktop
                .active_editor()
                .is_some_and(|e| e.modals.editor_menu().is_some())
        {
            let mut menu = crate::painting::NativeFrame::new(
                Rect::new(0, 0, size.0, size.1),
                app.config.palette,
            );
            editor_menu::draw_dropdown(&mut menu, app, Rect::new(0, 0, size.0, size.1));
            let mut menu = menu.finish();
            crate::windowing::workspace::translate_scene(
                &mut menu,
                i32::from(surface.bounds.x + 1),
                i32::from(surface.bounds.y + 1),
                Rect::new(
                    surface.bounds.x + 1,
                    surface.bounds.y + 1,
                    surface.bounds.width - 2,
                    surface.bounds.height - 2,
                ),
                false,
            );
            scene.cells.extend(menu.cells);
            scene.regions = menu.regions;
            scene.cursor = None;
        }
        if id != active.id {
            scene.cursor = None;
            scene.regions.clear();
        }
        scene.regions.insert(
            0,
            (
                surface.bounds,
                Target::Surface(surface.id, crate::windowing::geometry::Chrome::Activate),
            ),
        );
        crate::windowing::workspace::chrome(&mut scene, surface);
        crate::windowing::workspace::occlude(frame, surface.bounds);
        frame.compose(scene);
    }
    if active.id == WindowId::MANAGER {
        if let Some(cursor) = manager_cursor.filter(|p| {
            !prepared.surfaces.iter().any(|s| {
                !s.maximized
                    && matches!(s.id.key, SurfaceKey::Document(_))
                    && s.bounds.contains((*p).into())
            })
        }) {
            frame.set_cursor_position(cursor);
        }
    }
    let (_, menu_positions) = crate::turbo::manager::menu_bar(app, frame.area().width);
    draw_layers(frame, app, prepared, &menu_positions);
}
