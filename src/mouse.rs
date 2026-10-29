use crate::focus::{self, Focus};

use crate::presentation::layout::Rect;
use crate::{
    app::App,
    menu::{self, Action},
};
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Surface(
        crate::windowing::geometry::SurfaceId,
        crate::windowing::geometry::Chrome,
    ),
    Window(crate::windowing::Action),
    ApplyImport,
    CancelImport,
    Welcome(crate::welcome::Action),
    ShortcutProfile,
    OpenSettingsChoice(usize),
    SettingsChoice(usize),
    CancelSettingsChoice,
    CommandHistory(crate::command_line::history::Action),
    Archive(crate::archive::Action),
    UserMenu(crate::user_menu::Action),
    Tree(crate::directory_tree::Action),
    Discovery(crate::discovery::Action),
    CloseQuickView,
    QuickPreview,
    CommandLine,
    Screensaver(usize),
    ColorChoice(crate::palette::PaletteProfile),
    AcceptColors,
    CancelColors,
    Navigation(crate::navigation_tools::Action),
    Viewer(crate::viewer::Action),
    Utility(crate::utilities::Action),
    Tool(crate::file_tools::ToolAction),
    Editor(crate::editor::EditorAction),
    Menu(usize),
    RevealMenu,
    MenuItem(usize, usize),
    MenuSurface,
    Function(u8),
    Panel(usize),
    Scrollbar(usize, crate::panel_layout::ScrollAction),
    Entry(usize, usize),
    CloseDialog,
    SubmitPrompt,
    Copy(crate::copy_dialog::Action),
    CopyHistory(usize),
    CloseCopyHistory,
    PromptField,
    PromptHelp,
    CancelPrompt,
    CancelJob,
    CloseViewer,
    Setting(usize),
    RegionalChoice(usize),
    AcceptSettings,
    CancelSettings,
    AcceptPattern,
    PatternField,
    CancelPattern,
    CloseFileInfo,
    ColumnChoice(usize, crate::columns::ColumnPreset),
    AcceptColumns,
    CancelColumns,
    Plugin(crate::plugins::PluginId, crate::plugins::ActionId),
}

#[derive(Default)]
pub struct MouseState {
    pub(crate) surfaces: Vec<crate::windowing::geometry::Surface>,
    pub frame: Option<crate::layout::FrameContext>,
    pub regions: Vec<(Rect, Target)>,
    last_click: Option<(usize, PathBuf, Instant)>,
    pub scrollbar_drag: Option<(usize, Rect)>,
    pub(crate) dialogs:
        Vec<std::rc::Rc<std::cell::RefCell<crate::turbo::composition::DialogGroup>>>,
}

impl MouseState {
    pub fn reset_click(&mut self) {
        self.last_click = None;
        self.scrollbar_drag = None;
    }
}

pub fn handle(app: &mut App, event: MouseEvent) {
    if app.output_visible {
        return;
    }
    if app.screensaver.active {
        if matches!(event.kind, MouseEventKind::Up(_)) {
            return;
        }
        app.screensaver.activity(Instant::now());
        app.mouse.reset_click();
        crate::layout::update(app);
        return;
    }
    if app.exit_pending && app.quit_review.is_some() {
        return;
    }
    if crate::windowing::workspace::capture_mouse(app, event) {
        crate::layout::update(app);
        return;
    }
    if !crate::eligibility::Gate::new(app).frame(app.mouse.frame) {
        app.mouse.reset_click();
        return;
    }
    if crate::windowing::workspace::mouse(app, event) {
        crate::layout::update(app);
        return;
    }
    let mut event = event;
    if event.kind == MouseEventKind::Down(MouseButton::Left) {
        let offset = app.layout_key.map_or((0, 0), |(_, r, _)| (r.x, r.y));
        let local = crate::presentation::placement::Placement::translated(
            i32::from(offset.0),
            i32::from(offset.1),
            Rect::default(),
        )
        .local_mouse(event);
        let top = app.modal_top().map(|m| m.id);
        let proposal = app
            .mouse
            .dialogs
            .iter()
            .rev()
            .find(|d| Some(d.borrow().scope.id) == top)
            .and_then(|d| d.borrow_mut().button(local.column, local.row));
        if let Some((rect, _target)) = proposal {
            event.column = rect.x + offset.0;
            event.row = rect.y + offset.1;
        }
    }
    handle_mouse(app, event);
    crate::layout::update(app);
}
fn handle_mouse(app: &mut App, event: MouseEvent) {
    if crate::shortcuts::armed(app)
        && matches!(
            event.kind,
            MouseEventKind::Down(_) | MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
        )
    {
        app.shortcuts.clear();
        return;
    }
    if event.kind == MouseEventKind::Up(MouseButton::Left) {
        app.mouse.scrollbar_drag = None;
        // Native file-dialog buttons activate on release, unlike DN's
        // immediate command targets. Keep the release inside its live scope.
        if !(focus::current(app) == Focus::EditorModal
            && matches!(
                app.desktop
                    .active_editor()
                    .and_then(|e| e.modals.editor_modal()),
                Some(crate::editor::Modal::File(_))
            ))
        {
            return;
        }
    }
    if matches!(event.kind, MouseEventKind::Down(_)) {
        app.mouse.scrollbar_drag = None;
    }
    if app.screensaver.activity(Instant::now()) {
        app.mouse.reset_click();
        return;
    }
    if focus::current(app) == Focus::QuickSearch {
        app.modals.set_quick_search(None);
    }
    let focus = focus::current(app);
    let target = app
        .desktop
        .native()
        .mouse_target(&app.mouse.regions, event.column, event.row);

    if crate::dialog_adapter::mouse(app, event, target, focus) {
        return;
    }
    if focus == Focus::Welcome {
        if event.kind == MouseEventKind::Down(MouseButton::Left) {
            if let Some(Target::Welcome(action)) = target {
                crate::welcome::action(app, action);
            }
        } else if matches!(
            event.kind,
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
        ) {
            let offset = app.layout_key.map_or((0, 0), |(_, r, _)| (r.x, r.y));
            if let Some(welcome) = app.welcome.modals.welcome_mut() {
                welcome.page.mouse(event, offset);
            }
        }
        return;
    }
    if focus == Focus::Manager
        && matches!(target, Some(Target::CloseQuickView | Target::QuickPreview))
    {
        if target == Some(Target::CloseQuickView)
            && event.kind == MouseEventKind::Down(MouseButton::Left)
        {
            app.quick_view = None;
        } else if let Some(q) = &mut app.quick_view {
            match event.kind {
                MouseEventKind::ScrollUp => q.step(-3),
                MouseEventKind::ScrollDown => q.step(3),
                _ => {}
            }
        }
        return;
    }
    if focus == Focus::Message {
        let offset = app.layout_key.map_or((0, 0), |(_, r, _)| (r.x, r.y));
        if let Some(help) = app.modals.native_help_mut(app.desktop.active().id) {
            let close = help.mouse(event, offset);
            if close {
                app.pop_dialog();
            }
            return;
        }
        match event.kind {
            MouseEventKind::Down(MouseButton::Left) if target == Some(Target::CloseDialog) => {
                crate::dialog::message_key(app, crossterm::event::KeyCode::Enter)
            }
            MouseEventKind::ScrollUp => {
                app.set_message_offset(app.message_offset().saturating_sub(3))
            }
            MouseEventKind::ScrollDown => {
                app.set_message_offset(app.message_offset().saturating_add(3))
            }
            _ => {}
        }
        return;
    }
    if focus == Focus::SaverSetup {
        if event.kind == MouseEventKind::Down(MouseButton::Left) {
            if let Some(Target::Screensaver(i)) = target {
                crate::screensaver::command(app, i);
            }
        }
        return;
    }
    if focus == Focus::EditorModal
        && matches!(
            app.desktop
                .active_editor()
                .and_then(|e| e.modals.editor_modal()),
            Some(crate::editor::Modal::File(_))
        )
    {
        crate::editor::file_mouse(app, event);
        return;
    }
    if matches!(
        focus,
        Focus::Editor | Focus::EditorModal | Focus::EditorMenu
    ) {
        if event.kind == MouseEventKind::Down(MouseButton::Left) {
            if let Some(Target::Editor(mut command)) = target {
                if let crate::editor::EditorAction::Position(position, _) = command {
                    command = crate::editor::EditorAction::Position(
                        position,
                        event
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::SHIFT),
                    );
                }
                crate::editor::action(app, command);
            }
        } else if let Some(editor) = app.desktop.active_editor_mut() {
            if focus == Focus::Editor {
                if let Some(buffer) = &mut editor.buffer {
                    match event.kind {
                        MouseEventKind::ScrollUp => buffer.vertical(-3, false),
                        MouseEventKind::ScrollDown => buffer.vertical(3, false),
                        _ => {}
                    }
                    editor.keep_visible();
                }
            }
        }
        return;
    }
    if focus == Focus::Plugin {
        crate::plugins::host::mouse(app, event, target);
        return;
    }
    if focus == Focus::Colors {
        if event.kind == MouseEventKind::Down(MouseButton::Left) {
            if let Some(
                t @ (Target::ColorChoice(_) | Target::AcceptColors | Target::CancelColors),
            ) = target
            {
                crate::dialog_controls::tool_action(app, t);
            }
        }
        return;
    }
    if focus == Focus::Pattern {
        if event.kind == MouseEventKind::Down(MouseButton::Left) {
            match target {
                Some(Target::PatternField) => {
                    if let Some(p) = app.modals.pattern_mut() {
                        p.focus = 0;
                    }
                }
                Some(Target::AcceptPattern) => crate::pattern::accept(app),
                Some(Target::CancelPattern) => app.modals.set_pattern(None),
                _ => {}
            }
        }
        return;
    }
    if focus == Focus::Columns {
        if event.kind == MouseEventKind::Down(MouseButton::Left) {
            if let Some(
                t @ (Target::ColumnChoice(_, _) | Target::AcceptColumns | Target::CancelColumns),
            ) = target
            {
                crate::dialog_controls::tool_action(app, t);
            }
        }
        return;
    }
    if let Some(info) = app
        .modals
        .file_info_mut()
        .filter(|_| focus == Focus::FileInfo)
    {
        match event.kind {
            MouseEventKind::ScrollUp => info.step(-3),
            MouseEventKind::ScrollDown => info.step(3),
            MouseEventKind::Down(MouseButton::Left) if target == Some(Target::CloseFileInfo) => {
                app.modals.set_file_info(None)
            }
            _ => {}
        }
        return;
    }
    if focus == Focus::Settings {
        if app.modals.settings_choice().is_some() {
            if event.kind == MouseEventKind::Down(MouseButton::Left) {
                if let Some(Target::SettingsChoice(row)) = target {
                    if let Some(choice) = app.modals.settings_choice_mut() {
                        choice.cursor = row.min(choice.items.len().saturating_sub(1));
                    }
                    crate::settings::choice_key(app, crossterm::event::KeyCode::Enter);
                } else {
                    app.modals.set_settings_choice(None);
                }
            }
            return;
        }
        if event.kind == MouseEventKind::Down(MouseButton::Left) {
            if let Some(target) = target {
                crate::settings::action(app, target);
            }
        }
        return;
    }
    if focus == Focus::ViewerSearch {
        if event.kind == MouseEventKind::Down(MouseButton::Left) {
            if let Some(Target::Viewer(action)) = target {
                crate::viewer::action(app, action);
            }
        }
        return;
    }
    if focus == Focus::Viewer {
        match event.kind {
            MouseEventKind::ScrollUp => {
                if let Some(viewer) = app.desktop.active_viewer_mut() {
                    viewer.step(-3);
                }
            }
            MouseEventKind::ScrollDown => {
                if let Some(viewer) = app.desktop.active_viewer_mut() {
                    viewer.step(3);
                }
            }
            MouseEventKind::Down(MouseButton::Left)
                if matches!(target, Some(Target::Viewer(_))) =>
            {
                if let Some(Target::Viewer(action)) = target {
                    crate::viewer::action(app, action);
                }
            }
            MouseEventKind::Down(MouseButton::Left) if target == Some(Target::CloseViewer) => {
                let id = app.desktop.target();
                app.retire_document(id);
            }
            _ => {}
        }
        return;
    }
    if focus == Focus::Job {
        if event.kind == MouseEventKind::Down(MouseButton::Left)
            && target == Some(Target::CancelJob)
        {
            if let Some(job) = &app.job {
                job.cancel();
            }
            app.notice = "Cancelling…".into();
        }
        return;
    }
    if focus == Focus::CopyHistory {
        crate::copy_dialog::history_mouse(app, event, target);
        return;
    }
    if focus == Focus::Prompt && app.modals.prompt().is_some_and(|p| p.copy.is_some()) {
        if event.kind == MouseEventKind::Down(MouseButton::Left) {
            if let Some(target) = target {
                crate::copy_dialog::command(app, target);
            }
        }
        return;
    }
    if focus == Focus::Prompt {
        if event.kind == MouseEventKind::Down(MouseButton::Left) {
            crate::dialog::prompt_action(app, None, target);
        }
        return;
    }
    if event.kind == MouseEventKind::Drag(MouseButton::Left) {
        if app.modals.menu().is_none() {
            if let Some((index, track)) = app.mouse.scrollbar_drag {
                let panel = &mut app.panels[index];
                if !panel.loading && !panel.entries.is_empty() {
                    panel.cursor =
                        crate::panel_layout::scroll_position(track, event.row, panel.entries.len());
                    app.active = index;
                }
            }
        }
        return;
    }
    if matches!(
        event.kind,
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
    ) {
        if app.modals.menu().is_none() && app.dialogs().is_empty() {
            if let Some(
                Target::Panel(index) | Target::Entry(index, _) | Target::Scrollbar(index, _),
            ) = target
            {
                app.active = index;
                app.mouse.last_click = None;
                app.panels[index].step(if event.kind == MouseEventKind::ScrollUp {
                    -3
                } else {
                    3
                });
            }
        }
        return;
    }
    if event.kind != MouseEventKind::Down(MouseButton::Left) {
        return;
    }
    if !app.dialogs().is_empty() {
        if target == Some(Target::CloseDialog) {
            app.pop_dialog();
        }
        app.mouse.last_click = None;
        return;
    }
    if app.modals.menu().is_some()
        && !matches!(
            target,
            Some(Target::Menu(_) | Target::MenuItem(_, _) | Target::MenuSurface)
        )
    {
        menu::close(app);
        app.mouse.last_click = None;
        return;
    }
    let previous = app.mouse.last_click.take();
    match target {
        Some(Target::RevealMenu) => menu::open(app, 0),
        Some(Target::Menu(index)) => {
            if app.modals.menu().is_some_and(|(open, _)| open == index) {
                menu::close(app);
            } else {
                menu::open(app, index);
            }
        }
        Some(Target::MenuItem(depth, row)) => menu::activate(app, depth, row),
        Some(Target::Function(number)) => menu::execute(app, Action::Key(number)),
        Some(Target::Panel(index)) => app.active = index,
        Some(Target::Scrollbar(index, action)) => {
            use crate::panel_layout::ScrollAction;
            app.active = index;
            app.mouse.scrollbar_drag = None;
            let track = app
                .mouse
                .regions
                .iter()
                .find(|(_, t)| *t == Target::Scrollbar(index, ScrollAction::Track))
                .map(|(r, _)| *r);
            let panel = &mut app.panels[index];
            if panel.loading || panel.entries.is_empty() {
                return;
            }
            match action {
                ScrollAction::Up | ScrollAction::Down => {
                    panel.cursor = crate::turbo::widgets::scroll_step(
                        panel.cursor,
                        panel.entries.len() - 1,
                        1,
                        action == ScrollAction::Down,
                        false,
                    );
                }
                ScrollAction::Thumb => {
                    if let Some(track) = track {
                        app.mouse.scrollbar_drag = Some((index, track));
                    }
                }
                ScrollAction::Track => {
                    let thumb = app
                        .mouse
                        .regions
                        .iter()
                        .find(|(_, t)| *t == Target::Scrollbar(index, ScrollAction::Thumb))
                        .map(|(r, _)| r.y);
                    if let Some(thumb) = thumb {
                        let page = panel
                            .viewport_rows
                            .saturating_mul(panel.viewport_columns)
                            .min(isize::MAX as usize) as isize;
                        panel.cursor = crate::turbo::widgets::scroll_step(
                            panel.cursor,
                            panel.entries.len() - 1,
                            page as usize,
                            event.row >= thumb,
                            true,
                        );
                    }
                }
            }
        }
        Some(Target::CommandLine) => app.command_line.click(event.column),
        Some(Target::Entry(index, row)) => {
            app.active = index;
            let panel = &mut app.panels[index];
            if panel.loading {
                return;
            }
            if let Some(entry) = panel.entries.get(row) {
                let path = entry.path.clone();
                panel.cursor = row;
                if event
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL)
                {
                    panel.toggle_selection();
                    return;
                }
                let now = Instant::now();
                if previous.is_some_and(|(old_panel, old_path, when)| {
                    old_panel == index
                        && old_path == path
                        && now.duration_since(when) <= Duration::from_millis(400)
                }) {
                    app.open_current();
                } else {
                    app.mouse.last_click = Some((index, path, now));
                }
            }
        }
        _ => {}
    }
}
