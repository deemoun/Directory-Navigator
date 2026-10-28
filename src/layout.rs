//! Explicit viewport preparation. Painting consumes these dimensions and geometry.
use crate::presentation::layout::{Constraint, Layout, Rect};
use crate::{app::App, panel_layout::PanelGeometry};
pub type Prepared = LayoutSnapshot;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameContext {
    pub owner: crate::desktop::WindowId,
    pub revision: u64,
    pub modal: Option<crate::modals::ModalId>,
}
/// Changes that alter the meaning or placement of published hit regions.
#[derive(PartialEq, Eq)]
pub(crate) struct ViewportState {
    panels: [(u64, usize, usize, bool, &'static str); 2],
    editor: Option<(u64, usize, usize, usize)>,
    viewer: Option<(usize, u16, bool, usize, bool)>,
    picker: Option<u64>,
    help: Option<u64>,
    welcome: Option<u64>,
    windows: Option<(Vec<crate::desktop::WindowId>, usize)>,
    characters: Option<(crate::utilities::characters::Page, bool)>,
    menu: Option<(usize, usize)>,
    hide_menu: bool,
    copy_history: Option<(usize, usize)>,
    submenus: Vec<(crate::menu::MenuId, usize)>,
    modal_offsets: Vec<usize>,
    controls: Vec<(bool, Rect, crate::mouse::Target)>,
}
fn viewport_state(app: &App) -> ViewportState {
    let area = crate::turbo::dialog_spec::input_area(app);
    let focus = crate::focus::current(app);
    let spec = if focus == crate::focus::Focus::Prompt {
        crate::ui::copy_spec(app, area)
    } else if focus == crate::focus::Focus::Settings
        && app
            .modals
            .settings()
            .is_some_and(|s| s.kind == crate::settings::SettingsKind::Plugins)
    {
        crate::ui::settings_spec(app, area)
    } else {
        crate::ui::remaining_spec(app, area, focus)
    };
    ViewportState {
        controls: spec.map_or_else(Vec::new, |s| s.interaction_state()),
        panels: std::array::from_fn(|i| {
            let p = &app.panels[i];
            (
                p.revision,
                p.offset,
                p.entries.len(),
                p.loading,
                app.config.layouts[i].key(),
            )
        }),
        editor: app.desktop.active_editor().map(|e| {
            (
                e.buffer.as_ref().map_or(0, |b| b.revision()),
                e.scroll_row,
                e.scroll_column,
                e.buffer.as_ref().map_or(0, |b| b.lines.len()),
            )
        }),
        picker: app
            .desktop
            .active_editor()
            .and_then(|e| match e.modals.editor_modal() {
                Some(crate::editor::Modal::File(picker)) => Some(picker.revision),
                _ => None,
            }),
        help: app
            .modals
            .native_help(app.desktop.active().id)
            .map(|h| h.revision),
        welcome: app.welcome.modals.welcome().map(|w| w.page.revision),
        viewer: app.desktop.active_viewer().map(|v| {
            (
                v.offset,
                v.horizontal,
                v.wrap,
                v.rows.len(),
                v.contents.is_some(),
            )
        }),
        windows: app.modals.window_list().map(|l| (l.ids.clone(), l.offset)),
        characters: (app.utilities.modals.utility_dialog()
            == Some(crate::utilities::Kind::Characters))
        .then(|| {
            (
                app.utilities.chart.page,
                app.utilities.chart.copy_text().is_some(),
            )
        }),
        menu: app.modals.menu(),
        hide_menu: app.config.hide_menu,
        copy_history: app.modals.copy_history().map(|h| (h.cursor, h.offset)),
        submenus: app.modals.submenus().to_vec(),
        modal_offsets: vec![
            app.modals.archive().map_or(0, |m| m.offset),
            app.modals.directory_tree().map_or(0, |m| m.offset),
            app.user_menu.modals.user_menu().map_or(0, |m| m.offset),
            app.discovery.modals.discovery().map_or(0, |m| m.offset),
            app.command_line
                .modals
                .command_history()
                .map_or(0, |m| m.offset),
        ],
    }
}
pub struct LayoutSnapshot {
    pub context: FrameContext,
    pub bounds: Rect,
    pub clip: Rect,
    pub screen: crate::presentation::layout::Size,
    pub regions: [Rect; 4],
    pub panel_areas: [Rect; 2],
    pub panels: [PanelGeometry; 2],
    pub message: Option<MessageLayout>,
}
pub fn editor_gutter(editor: &crate::editor::Editor) -> u16 {
    editor
        .buffer
        .as_ref()
        .map(|b| b.lines.len().to_string().len() as u16 + 2)
        .unwrap_or(3)
}
pub fn prepare(app: &mut App, area: Rect) -> LayoutSnapshot {
    prepare_window(
        app,
        area,
        Rect::new(0, 0, area.right(), area.bottom()),
        std::time::Instant::now(),
    )
}
pub fn prepare_window(
    app: &mut App,
    bounds: Rect,
    screen: Rect,
    now: std::time::Instant,
) -> LayoutSnapshot {
    app.sync_modals();
    if app.utilities.size != (bounds.width, bounds.height) {
        app.mouse.scrollbar_drag = None;
    }
    app.utilities.size = (bounds.width, bounds.height);
    crate::plugins::host::update_activity(app, now);
    app.sync_modals();
    let clip = bounds.intersection(screen);
    let owner = app.desktop.active().id;
    let key = (owner, bounds, screen);
    if app.layout_key != Some(key) {
        app.layout_revision = app
            .layout_revision
            .checked_add(1)
            .expect("layout revision exhausted");
        app.layout_key = Some(key);
        app.mouse.scrollbar_drag = None;
    }
    app.sync_modals();
    let mut context = FrameContext {
        owner,
        revision: app.layout_revision,
        modal: app.modal_top().map(|m| m.id),
    };
    let area = Rect::new(0, 0, bounds.width, bounds.height);
    if let Some(list) = app.modals.window_list_mut() {
        list.rows = (list.ids.len() + 6)
            .max(12)
            .min(usize::from(area.height.saturating_sub(2)))
            .saturating_sub(6)
            .max(1);
        list.keep_visible();
    }
    app.desktop
        .native()
        .sync_panes(app.active, (bounds.width, bounds.height));
    let r = Layout::vertical([
        Constraint::Length(u16::from(!app.config.hide_menu)),
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(area);
    let regions = [r[0], r[1], r[2], r[3]];
    let p = Layout::horizontal([Constraint::Percentage(50); 2]).split(regions[1]);
    let panel_areas = [p[0], p[1]];
    let panels = std::array::from_fn(|i| {
        PanelGeometry::new(
            panel_areas[i].inner(crate::presentation::layout::Margin::new(1, 1)),
            app.config.layouts[i],
        )
    });
    if let Some(q) = &mut app.quick_view {
        q.layout(panel_areas[q.source ^ 1]);
    }
    if app.screensaver.active {
        app.screensaver.resize(area.width, area.height);
    }
    if area.width >= 80 && area.height >= 25 {
        match crate::focus::screen(app) {
            crate::focus::Screen::Editor => {
                if let Some(e) = app.desktop.active_editor_mut() {
                    e.rows = usize::from(area.height - 4).max(1);
                    e.columns = usize::from(area.width.saturating_sub(editor_gutter(e))).max(1);
                    e.keep_visible();
                }
            }
            crate::focus::Screen::Viewer => {
                app.page_rows = regions[1].height as usize;
                if let Some(v) = app.desktop.active_viewer_mut() {
                    v.layout(regions[1].width);
                }
            }
            _ => {
                app.page_rows = panels[app.active].rows();
                for (p, g) in app.panels.iter_mut().zip(&panels) {
                    p.keep_visible_grid(g.rows(), g.columns.len());
                }
            }
        }
    }
    if area.width >= 80
        && area.height >= 25
        && crate::focus::screen(app) == crate::focus::Screen::Manager
    {
        if let Some(preview) = app.modals.config_import_mut() {
            let rows = usize::from(area.height.min(21).saturating_sub(5));
            preview.offset = preview.offset.min(
                preview
                    .lines(app.config.language)
                    .len()
                    .saturating_sub(rows),
            );
        }
        if let Some(b) = app.modals.archive_mut() {
            b.rows = usize::from(area.height.saturating_sub(4).min(30).saturating_sub(9)).max(1);
            b.keep_visible();
        }
        if let Some(m) = app.user_menu.modals.user_menu_mut() {
            let height = if m.review.is_some() {
                area.height.saturating_sub(4).min(26)
            } else {
                (m.items().len() + 7)
                    .clamp(10, 24)
                    .min(area.height.saturating_sub(4) as usize) as u16
            };
            m.rows = usize::from(height.saturating_sub(7)).max(1);
            m.keep_visible();
        }
        if let Some(t) = app.modals.directory_tree_mut() {
            t.page_rows = usize::from(
                crate::directory_tree::geometry(area)
                    .height
                    .saturating_sub(9),
            )
            .max(1);
            t.keep_visible();
        }
        if let Some(m) = app.discovery.modals.discovery_mut() {
            m.rows = usize::from(area.height.saturating_sub(4).min(26).saturating_sub(11)).max(1);
            m.keep_visible();
        }
        if let Some(crate::file_tools::ToolDialog::History {
            paths,
            cursor,
            offset,
            rows,
            ..
        }) = app.file_tools.modals.file_tools_mut()
        {
            *rows = usize::from(area.height.saturating_sub(4).min(22) - 6);
            *cursor = (*cursor).min(paths.len().saturating_sub(1));
            if *cursor < *offset {
                *offset = *cursor;
            }
            if *cursor >= *offset + *rows {
                *offset = *cursor + 1 - *rows;
            }
        }
        if let Some(d) = app.command_line.modals.command_history_mut() {
            d.rows = crate::command_line::history::geometry(area).1;
            d.keep_visible();
        }
        if let Some(info) = &mut app.utilities.information {
            info.rows = usize::from(area.height.saturating_sub(4).min(24).saturating_sub(6));
            info.step(0);
        }
    }
    if let Some(history) = app.modals.copy_history_mut() {
        history.rows = history.paths.len().clamp(1, 8);
        history.keep_visible();
    }
    let profile = app.config.palette;
    if let Some(crate::editor::Modal::File(picker)) = app
        .desktop
        .active_editor_mut()
        .and_then(|e| e.modals.editor_modal_mut())
    {
        picker.prepare(
            crate::presentation::layout::Size::new(area.width, area.height),
            profile,
        );
    }
    if app.welcome.modals.welcome().is_some() {
        crate::welcome::prepare(
            app,
            crate::presentation::layout::Size::new(area.width, area.height),
        );
    }
    let language = app.config.language;
    let shortcuts = app.config.shortcuts;
    if let Some(help) = app.modals.native_help_mut(owner) {
        help.prepare(
            crate::presentation::layout::Size::new(area.width, area.height),
            profile,
            language,
            shortcuts,
        );
    }
    let message = if area.width >= 80 && area.height >= 25 {
        app.last_dialog().map(|d| {
            message_layout(
                area,
                app.config.language.text(d.title),
                &app.config.language.message(&d.message),
            )
        })
    } else {
        None
    };
    if let Some(m) = &message {
        if app.modals.native_help(owner).is_none() {
            app.set_message_offset(app.message_offset().min(m.maximum));
        }
    }
    crate::command_line::prepare(app, regions[2]);
    app.command_line.translate_input(bounds.x);
    let state = viewport_state(app);
    if app.layout_state.as_ref() != Some(&state) {
        app.layout_revision = app
            .layout_revision
            .checked_add(1)
            .expect("layout revision exhausted");
        app.layout_state = Some(state);
        context.revision = app.layout_revision;
    }
    LayoutSnapshot {
        context,
        bounds,
        clip,
        screen: crate::presentation::layout::Size::new(screen.right(), screen.bottom()),
        message,
        regions,
        panel_areas,
        panels,
    }
}
/// Prepare after commands, even when no terminal draw is requested.
pub fn update(app: &mut App) {
    if let Some((_, bounds, screen)) = app.layout_key {
        let _ = prepare_window(app, bounds, screen, std::time::Instant::now());
    }
}

pub struct MessageLayout {
    pub rect: Rect,
    pub lines: Vec<String>,
    pub visible: usize,
    pub maximum: usize,
}
pub fn message_layout(area: Rect, title: &str, text: &str) -> MessageLayout {
    use crate::presentation::text::Span;
    let text = text
        .lines()
        .map(crate::filesystem::display_name)
        .collect::<Vec<_>>()
        .join("\n");
    let width = (text
        .lines()
        .map(|s| Span::raw(s).width())
        .max()
        .unwrap_or(0)
        + 6)
    .max(Span::raw(title).width() + 6)
    .clamp(36, 76)
    .min(usize::from(area.width)) as u16;
    let lines = wrap(&text, width.saturating_sub(4));
    let height = (lines.len() + 5).min(area.height.saturating_sub(2) as usize) as u16;
    let rect = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    let visible = usize::from(height.saturating_sub(4));
    let maximum = lines.len().saturating_sub(visible);
    MessageLayout {
        rect,
        lines,
        visible,
        maximum,
    }
}
pub(crate) fn wrap(text: &str, width: u16) -> Vec<String> {
    use unicode_segmentation::UnicodeSegmentation;
    let mut result = Vec::new();
    for line in text.split('\n') {
        let mut row = String::new();
        let mut cells = 0;
        for grapheme in line.graphemes(true) {
            let size = crate::presentation::text::Span::raw(grapheme).width();
            if cells + size > usize::from(width) && !row.is_empty() {
                result.push(std::mem::take(&mut row));
                cells = 0;
            }
            row.push_str(grapheme);
            cells += size;
        }
        result.push(row);
    }
    result
}

impl LayoutSnapshot {
    pub fn to_screen(&self, rect: Rect) -> Rect {
        Rect::new(
            self.bounds.x.saturating_add(rect.x),
            self.bounds.y.saturating_add(rect.y),
            rect.width,
            rect.height,
        )
        .intersection(self.clip)
    }
    pub(crate) fn transform(&self, scene: &mut crate::turbo::scene::Scene) {
        scene.cells = std::mem::take(&mut scene.cells)
            .into_iter()
            .filter_map(|((x, y), mut glyph)| {
                let x = x.saturating_add(self.bounds.x);
                let y = y.saturating_add(self.bounds.y);
                if !self.clip.contains((x, y).into()) {
                    return None;
                }
                use unicode_width::UnicodeWidthStr;
                if glyph.text.width() > usize::from(self.clip.right() - x)
                    || (glyph.text.is_empty() && x == self.clip.x && self.clip.x > self.bounds.x)
                {
                    glyph.text = " ".into();
                }
                Some(((x, y), glyph))
            })
            .collect();
        scene.regions = std::mem::take(&mut scene.regions)
            .into_iter()
            .filter_map(|(r, t)| {
                let r = self.to_screen(r);
                (!r.is_empty()).then_some((r, t))
            })
            .collect();
        scene.cursor = scene.cursor.and_then(|(x, y)| {
            let p = (
                x.saturating_add(self.bounds.x),
                y.saturating_add(self.bounds.y),
            );
            self.clip.contains(p.into()).then_some(p)
        });
        scene.width = self.screen.width;
        scene.height = self.screen.height;
    }
}
