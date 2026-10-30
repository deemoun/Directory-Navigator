//! Coordinate ownership and compositing for the native floating desktop.
use super::geometry::{self, Capabilities, Definition, Surface, SurfaceKey};
use crate::presentation::placement::Placement;
use crate::{
    app::App,
    desktop::{WindowId, WindowKind},
    presentation::layout::Rect,
    turbo::scene::Scene,
};
pub(crate) fn move_plugin_scene(scene: &mut Scene, surface: Surface) {
    let dx = i32::from(surface.bounds.x) - i32::from(surface.default.x);
    let dy = i32::from(surface.bounds.y) - i32::from(surface.default.y);
    translate_scene(
        scene,
        dx,
        dy,
        geometry::workspace((scene.width, scene.height)),
        true,
    );
    scene.regions.insert(
        0,
        (
            surface.bounds,
            crate::mouse::Target::Surface(surface.id, super::geometry::Chrome::Activate),
        ),
    );
    chrome(scene, surface);
}
pub(crate) fn translate_scene(scene: &mut Scene, dx: i32, dy: i32, clip: Rect, fixed_status: bool) {
    Placement::translated(dx, dy, clip)
        .with_workspace_hits()
        .with_status_row(fixed_status.then_some(scene.height - 1))
        .apply(scene);
}
pub(crate) fn chrome(scene: &mut Scene, s: Surface) {
    use super::geometry::Chrome;
    use crate::mouse::Target;
    let r = s.bounds;
    let y = r.y + s.title_row;
    scene.regions.push((
        Rect::new(r.x, y, r.width, 1),
        Target::Surface(s.id, Chrome::Move),
    ));
    scene.regions.push((
        Rect::new(r.x + 2, y, 3, 1),
        Target::Surface(s.id, Chrome::Close),
    ));
    if s.caps.zoom {
        scene.text(
            r.right()
                - if s.maximized && s.title_row == 0 && matches!(s.id.key, SurfaceKey::Document(_))
                {
                    16
                } else {
                    5
                },
            y,
            3,
            if s.maximized { "[↕]" } else { "[↑]" },
            0x7f,
        );
        scene.regions.push((
            Rect::new(
                r.right()
                    - if s.maximized
                        && s.title_row == 0
                        && matches!(s.id.key, SurfaceKey::Document(_))
                    {
                        16
                    } else {
                        5
                    },
                y,
                3,
                1,
            ),
            Target::Surface(s.id, Chrome::Zoom),
        ));
    }
    if s.caps.resize && !s.maximized {
        scene.text(r.right() - 2, r.bottom() - 1, 2, "◢◤", 0x7f);
        scene.regions.push((
            Rect::new(r.right() - 2, r.bottom() - 1, 2, 1),
            Target::Surface(s.id, Chrome::Resize),
        ));
    }
    if matches!(scene.profile, crate::palette::PaletteProfile::Custom(_)) {
        for ((x, row), glyph) in scene.cells.iter_mut() {
            if ((*row == y && *x >= r.x && *x < r.right())
                || (*row == r.bottom() - 1 && *x >= r.right().saturating_sub(2) && *x < r.right()))
                && glyph.style.is_none()
            {
                glyph.style = scene
                    .profile
                    .themed(crate::palette::theme::Group::Dialogs, glyph.attribute);
            }
        }
    }
}
pub(crate) fn definitions(app: &App, area: Rect) -> Vec<Definition> {
    let mut result = app
        .desktop
        .windows()
        .filter_map(|w| match w.kind {
            WindowKind::Editor => {
                use unicode_width::UnicodeWidthStr;
                let menu = crate::editor::menus::LABELS
                    .iter()
                    .map(|label| {
                        crate::menu::plain(app.config.language.text(label)).width() as u16 + 3
                    })
                    .sum::<u16>();
                let clock = if app.config.show_clock {
                    app.clock.text.width() as u16 + 2
                } else {
                    0
                };
                Some((w, Capabilities::document(72.max(menu + clock + 2), 12)))
            }
            WindowKind::Viewer => Some((w, Capabilities::document(48, 10))),
            _ => None,
        })
        .map(|(w, caps)| Definition {
            key: SurfaceKey::Document(w.id),
            bounds: area,
            caps,
            maximized: true,
            title_row: u16::from(w.kind == WindowKind::Editor),
            parent: None,
        })
        .collect::<Vec<_>>();
    result.extend(crate::plugins::host::window_definitions(app, area));
    for modal in app.visible_modals() {
        use crate::focus::Focus;
        if result.iter().any(|d| d.key == SurfaceKey::Modal(modal.id)) {
            continue;
        }
        let bounds = match modal.focus {
            Focus::Prompt => crate::ui::copy_spec(app, area)
                .or_else(|| crate::ui::prompt_spec(app, area))
                .map(|s| spec_bounds(s, area)),
            Focus::Settings => {
                let child = app.modals.settings_choice().is_some()
                    && app
                        .modals
                        .infos()
                        .filter(|m| m.focus == Focus::Settings)
                        .last()
                        .is_some_and(|m| m.id == modal.id);
                if child {
                    crate::ui::settings_choice_bounds(app, area)
                } else {
                    crate::ui::settings_spec(app, area).map(|s| spec_bounds(s, area))
                }
            }
            Focus::ViewerSearch => {
                crate::ui::viewer_search_spec(app, area).map(|s| spec_bounds(s, area))
            }
            Focus::Message => app
                .modals
                .native_help(app.desktop.active().id)
                .map(|h| h.bounds())
                .or_else(|| {
                    crate::ui::remaining_spec(app, area, Focus::Message)
                        .map(|s| spec_bounds(s, area))
                }),
            Focus::EditorModal => {
                app.desktop
                    .active_editor()
                    .and_then(|e| match e.modals.editor_modal() {
                        Some(crate::editor::Modal::File(p)) => Some(p.bounds()),
                        _ => crate::ui::editor_spec(app, area).map(|s| spec_bounds(s, area)),
                    })
            }
            Focus::Welcome => app.welcome.modals.welcome().map(|w| w.page.bounds()),
            Focus::Menu | Focus::EditorMenu | Focus::QuickSearch => None,
            _ => crate::ui::remaining_spec(app, area, modal.focus).map(|s| spec_bounds(s, area)),
        };
        if let Some(bounds) = bounds {
            let help = modal.focus == Focus::Message
                && app.modals.native_help(app.desktop.active().id).is_some();
            result.push(Definition {
                key: SurfaceKey::Modal(modal.id),
                bounds,
                caps: if help {
                    Capabilities::document(48, 10)
                } else {
                    Capabilities::fixed(bounds.width, bounds.height)
                },
                maximized: false,
                title_row: 0,
                parent: modal.parent.map(SurfaceKey::Modal).or(match modal.owner {
                    crate::modals::ModalOwner::Document(id) => Some(SurfaceKey::Document(id)),
                    _ => None,
                }),
            });
        }
    }
    result
}
pub(crate) fn prepare(app: &mut App, area: Rect) -> Vec<Surface> {
    let definitions = definitions(app, area);
    let native = app.desktop.native();
    let surfaces = native.geometry_sync(
        (area.width, area.height),
        &definitions,
        app.modal_top().map(|m| m.id),
    );
    let size = crate::presentation::layout::Size::new(area.width, area.height);
    let profile = app.config.palette;
    let language = app.config.language;
    let shortcuts = app.config.shortcuts;
    for surface in &surfaces {
        if let SurfaceKey::Modal(id) = surface.id.key {
            let focus = app
                .visible_modals()
                .iter()
                .find(|m| m.id == id)
                .map(|m| m.focus);
            match focus {
                Some(crate::focus::Focus::Message) => {
                    if let Some(help) = app.modals.native_help_mut(app.desktop.active().id) {
                        help.place(surface.bounds);
                        help.prepare(size, profile, language, shortcuts);
                    }
                }
                Some(crate::focus::Focus::Welcome) => {
                    if let Some(w) = app.welcome.modals.welcome_mut() {
                        w.page.place(surface.bounds);
                    }
                    crate::welcome::prepare(app, size);
                }
                Some(crate::focus::Focus::EditorModal) => {
                    if let Some(crate::editor::Modal::File(p)) = app
                        .desktop
                        .active_editor_mut()
                        .and_then(|e| e.modals.editor_modal_mut())
                    {
                        p.place(surface.bounds);
                        p.prepare(size, profile);
                    }
                }
                _ => {}
            }
        }
    }
    for s in &surfaces {
        if let SurfaceKey::Document(id) = s.id.key {
            let w = s
                .bounds
                .width
                .saturating_sub(if s.maximized { 0 } else { 2 });
            let h = s.bounds.height.saturating_sub(u16::from(!s.maximized));
            if let Some(e) = app.desktop.editor_mut(id) {
                e.rows = usize::from(h.saturating_sub(4)).max(1);
                e.columns = usize::from(w.saturating_sub(crate::layout::editor_gutter(e))).max(1);
                e.keep_visible();
            }
            if let Some(v) = app.desktop.viewer_mut(id) {
                v.layout(w);
                if app.desktop.active().id == id {
                    app.page_rows = usize::from(h.saturating_sub(3)).max(1);
                }
            }
        }
    }
    surfaces
}

pub(crate) fn float_document(
    scene: &mut Scene,
    s: Surface,
    kind: WindowKind,
    workspace: Rect,
    app: &App,
) {
    use crate::painting::{Frame, NativeFrame};
    let mut frame = NativeFrame::new(workspace, app.config.palette);
    let title = match kind {
        WindowKind::Editor => app
            .desktop
            .editor(match s.id.key {
                SurfaceKey::Document(id) => id,
                _ => unreachable!(),
            })
            .map(|e| {
                e.path
                    .as_ref()
                    .map(|p| crate::filesystem::display_name(&p.to_string_lossy()))
                    .unwrap_or_else(|| app.config.language.text("[New file]").to_owned())
            })
            .unwrap_or_default(),
        WindowKind::Viewer => app
            .desktop
            .viewer(match s.id.key {
                SurfaceKey::Document(id) => id,
                _ => unreachable!(),
            })
            .map(|v| crate::filesystem::display_name(&v.path.to_string_lossy()))
            .unwrap_or_default(),
        _ => String::new(),
    };
    frame.render_widget(
        crate::turbo::widgets::dialog(&crate::formatting::tail(
            &title,
            s.bounds.width.saturating_sub(16),
        )),
        s.bounds,
    );
    let mut border = frame.finish();
    let desktop = geometry::workspace((workspace.width, workspace.height));
    border
        .cells
        .retain(|(x, y), _| desktop.contains((*x, *y).into()));
    Placement::document(s.bounds, kind == WindowKind::Editor).apply(scene);
    border.cells.extend(std::mem::take(&mut scene.cells));
    scene.cells = border.cells;
    scene.width = workspace.width;
    scene.height = workspace.height;
}
/// Cut covered pieces out of old hit rectangles, rather than forwarding a
/// click through a foreground window's blank interior.
pub(crate) fn occlude(frame: &mut impl crate::painting::Frame, cover: Rect) {
    let mut out = Vec::new();
    for (r, t) in std::mem::take(frame.regions()) {
        let i = r.intersection(cover);
        if i.is_empty() {
            out.push((r, t));
            continue;
        }
        for piece in [
            Rect::new(r.x, r.y, r.width, i.y - r.y),
            Rect::new(r.x, i.bottom(), r.width, r.bottom() - i.bottom()),
            Rect::new(r.x, i.y, i.x - r.x, i.height),
            Rect::new(i.right(), i.y, r.right() - i.right(), i.height),
        ] {
            if !piece.is_empty() {
                out.push((piece, t));
            }
        }
    }
    *frame.regions() = out;
}

pub(crate) fn foreground(app: &App) -> Option<Surface> {
    app.desktop.native().top_surface()
}
pub(crate) fn key(app: &mut App, key: crossterm::event::KeyEvent) -> bool {
    use crossterm::event::{KeyCode as K, KeyModifiers as M};
    let native = app.desktop.native();
    if native.geometry_key(key) {
        return true;
    }
    if (key.code == K::F(5) && key.modifiers == M::CONTROL)
        || (key.code == K::Char('z') && key.modifiers == M::ALT)
    {
        if let Some(surface) = foreground(app) {
            if key.code == K::F(5) {
                native.begin_move(surface.id, None);
            } else {
                native.zoom_surface(surface.id);
            }
            return true;
        }
    }
    if key.code == K::Char('0')
        && key.modifiers == M::ALT
        && foreground(app).is_some_and(|s| matches!(s.id.key, SurfaceKey::Page(..)))
    {
        return true;
    }
    false
}
pub(crate) fn capture_mouse(app: &mut App, event: crossterm::event::MouseEvent) -> bool {
    let offset = app.layout_key.map_or((0, 0), |(_, b, _)| (b.x, b.y));
    let local = Placement::translated(i32::from(offset.0), i32::from(offset.1), Rect::default())
        .local_mouse(event);
    app.desktop.native().geometry_mouse(local)
}
pub(crate) fn mouse(app: &mut App, event: crossterm::event::MouseEvent) -> bool {
    use super::geometry::{Chrome, SurfaceKey};
    use crossterm::event::{MouseButton, MouseEventKind};
    if event.kind != MouseEventKind::Down(MouseButton::Left) {
        return false;
    }
    let native = app.desktop.native();
    let target = native.mouse_target(&app.mouse.regions, event.column, event.row);
    let offset = app.layout_key.map_or((0, 0), |(_, b, _)| (b.x, b.y));
    let local = Placement::translated(i32::from(offset.0), i32::from(offset.1), Rect::default())
        .local_mouse(event);
    if let Some(crate::mouse::Target::Surface(id, command)) = target {
        let Some(surface) = crate::eligibility::Gate::new(app).surface(id) else {
            return true;
        };
        if let SurfaceKey::Document(owner) | SurfaceKey::Page(owner, _) = id.key {
            if app.desktop.active().id != owner {
                if app.modal_top().is_some() {
                    return true;
                }
                app.desktop.activate(owner);
                if command == Chrome::Activate {
                    return true;
                }
            }
        }
        match command {
            Chrome::Move => {
                if surface.caps.zoom
                    && native.title_double(id, (local.column, local.row), Instant::now())
                {
                    native.zoom_surface(id);
                } else {
                    native.begin_move(id, Some(local));
                }
            }
            Chrome::Resize => {
                if surface.caps.resize {
                    native.begin_move(id, Some(local));
                }
            }
            Chrome::Zoom => {
                native.zoom_surface(id);
            }
            Chrome::Close => close(app, surface),
            Chrome::Activate => {}
        }
        return true;
    }
    if foreground(app).is_some_and(|s| {
        matches!(s.id.key, SurfaceKey::Page(..))
            && !s.bounds.contains((local.column, local.row).into())
            && local.row + 1 != app.utilities.size.1
    }) {
        return true;
    }
    if app.modal_top().is_none()
        && !foreground(app).is_some_and(|s| matches!(s.id.key, SurfaceKey::Page(..)))
        && app.desktop.active().id != WindowId::MANAGER
    {
        let in_window = app.mouse.surfaces.iter().any(|s| {
            matches!(s.id.key, SurfaceKey::Document(_))
                && s.bounds.contains((local.column, local.row).into())
        });
        if !in_window {
            app.desktop.activate(WindowId::MANAGER);
            return true;
        }
    }
    false
}
fn close(app: &mut App, surface: Surface) {
    match surface.id.key {
        SurfaceKey::Document(id) => {
            let kind = app.desktop.windows().find(|w| w.id == id).map(|w| w.kind);
            match kind {
                Some(WindowKind::Editor) => {
                    crate::editor::action(app, crate::editor::EditorAction::Close)
                }
                Some(WindowKind::Viewer) => {
                    crate::viewer::action(app, crate::viewer::Action::Close)
                }
                Some(WindowKind::Plugin(_)) => {
                    app.retire_document(id);
                }
                _ => {}
            }
        }
        SurfaceKey::Modal(_) | SurfaceKey::ModalPage(..) | SurfaceKey::Page(..) => {
            crate::input::handle(
                app,
                crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Esc,
                    crossterm::event::KeyModifiers::NONE,
                ),
            )
        }
    }
}
use std::time::Instant;

fn spec_bounds(spec: crate::turbo::dialog_spec::DialogSpec<'_>, area: Rect) -> Rect {
    spec.native_bounds(area)
}
