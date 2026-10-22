use crate::focus::{self, Focus};
use crate::{
    app::App,
    menu::{self, Action},
};
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    ShortcutProfile,
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
    MenuItem(usize, usize),
    MenuSurface,
    Function(u8),
    Panel(usize),
    Scrollbar(usize, crate::panel_layout::ScrollAction),
    Entry(usize, usize),
    CloseDialog,
    SubmitPrompt,
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
    CancelPattern,
    CloseFileInfo,
    ColumnChoice(usize, crate::columns::ColumnPreset),
    AcceptColumns,
    CancelColumns,
    Game(crate::tetris::GameAction),
}

#[derive(Default)]
pub struct MouseState {
    pub regions: Vec<(Rect, Target)>,
    last_click: Option<(usize, PathBuf, Instant)>,
    pub scrollbar_drag: Option<(usize, Rect)>,
}

impl MouseState {
    pub fn reset_click(&mut self) {
        self.last_click = None;
        self.scrollbar_drag = None;
    }
}

pub fn handle(app: &mut App, event: MouseEvent) {
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
        return;
    }
    if matches!(event.kind, MouseEventKind::Down(_)) {
        app.mouse.scrollbar_drag = None;
    }
    if app.screensaver.activity(Instant::now()) {
        app.mouse.reset_click();
        return;
    }
    if focus::current(app) == Focus::QuickSearch {
        app.quick_search = None;
    }
    let focus = focus::current(app);
    let target = app
        .mouse
        .regions
        .iter()
        .rev()
        .find(|(rect, _)| rect.contains(Position::new(event.column, event.row)))
        .map(|(_, target)| *target);
    if focus == Focus::CommandHistory {
        crate::command_line::history::mouse(app, event, target);
        return;
    }
    if focus == Focus::Archive {
        crate::archive::mouse(app, event, target);
        return;
    }
    if focus == Focus::UserMenu {
        crate::user_menu::mouse(app, event, target);
        return;
    }
    if focus == Focus::Tree {
        crate::directory_tree::mouse(app, event, target);
        return;
    }
    if focus == Focus::Discovery {
        crate::discovery::mouse(app, event, target);
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
        match event.kind {
            MouseEventKind::Down(MouseButton::Left) if target == Some(Target::CloseDialog) => {
                crate::dialog::message_key(app, crossterm::event::KeyCode::Enter)
            }
            MouseEventKind::ScrollUp => app.message_offset = app.message_offset.saturating_sub(3),
            MouseEventKind::ScrollDown => app.message_offset = app.message_offset.saturating_add(3),
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
        } else if let Some(editor) = &mut app.editor {
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
    if focus == Focus::Game {
        if event.kind == MouseEventKind::Down(MouseButton::Left) {
            if let Some(Target::Game(action)) = target {
                crate::tetris::action(app, action);
            }
        }
        return;
    }
    if focus == Focus::Colors {
        if event.kind == MouseEventKind::Down(MouseButton::Left) {
            match target {
                Some(Target::ColorChoice(profile)) => crate::palette::choose(app, profile),
                Some(Target::AcceptColors) => app.color_dialog = None,
                Some(Target::CancelColors) => crate::palette::cancel(app),
                _ => {}
            }
        }
        return;
    }
    if focus == Focus::Navigation {
        crate::navigation_tools::mouse(app, event, target);
        return;
    }
    if focus == Focus::Utility {
        crate::utilities::mouse(app, event, target);
        return;
    }
    if focus == Focus::FileTools {
        crate::file_tools::mouse(app, event, target);
        return;
    }
    if focus == Focus::Pattern {
        if event.kind == MouseEventKind::Down(MouseButton::Left) {
            match target {
                Some(Target::AcceptPattern) => crate::pattern::accept(app),
                Some(Target::CancelPattern) => app.pattern = None,
                _ => {}
            }
        }
        return;
    }
    if focus == Focus::Columns {
        if event.kind == MouseEventKind::Down(MouseButton::Left) {
            match target {
                Some(Target::ColumnChoice(panel, preset)) => {
                    crate::columns::choose(app, panel, preset)
                }
                Some(Target::AcceptColumns) => crate::columns::accept(app),
                Some(Target::CancelColumns) => crate::columns::cancel(app),
                _ => {}
            }
        }
        return;
    }
    if let Some(info) = app.file_info.as_mut().filter(|_| focus == Focus::FileInfo) {
        match event.kind {
            MouseEventKind::ScrollUp => info.step(-3),
            MouseEventKind::ScrollDown => info.step(3),
            MouseEventKind::Down(MouseButton::Left) if target == Some(Target::CloseFileInfo) => {
                app.file_info = None
            }
            _ => {}
        }
        return;
    }
    if focus == Focus::Settings {
        if event.kind == MouseEventKind::Down(MouseButton::Left) {
            match target {
                Some(Target::ShortcutProfile) => {
                    if let Some(settings) = &mut app.settings {
                        settings.keyboard_choice(false);
                    }
                }
                Some(Target::RegionalChoice(index)) => {
                    if let Some(settings) = &mut app.settings {
                        settings.regional_choice(index, false);
                    }
                }
                Some(Target::Setting(index)) => {
                    if let Some(settings) = &mut app.settings {
                        settings.toggle(index);
                    }
                }
                Some(Target::AcceptSettings) => crate::settings::accept(app),
                Some(Target::CancelSettings) => app.settings = None,
                _ => {}
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
                if let Some(viewer) = &mut app.viewer {
                    viewer.step(-3);
                }
            }
            MouseEventKind::ScrollDown => {
                if let Some(viewer) = &mut app.viewer {
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
                app.viewer = None
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
    if focus == Focus::Prompt {
        if event.kind == MouseEventKind::Down(MouseButton::Left) {
            match target {
                Some(Target::SubmitPrompt) => app.submit_prompt(),
                Some(Target::PromptHelp) => crate::dialog::prompt_help(app),
                Some(Target::PromptField) => {
                    if let Some(prompt) = &mut app.prompt {
                        prompt.focus = 0;
                    }
                }
                Some(Target::CancelPrompt) => app.prompt = None,
                _ => {}
            }
        }
        return;
    }
    if event.kind == MouseEventKind::Drag(MouseButton::Left) {
        if app.menu.is_none() {
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
        if app.menu.is_none() && app.dialogs.is_empty() {
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
    if !app.dialogs.is_empty() {
        if target == Some(Target::CloseDialog) {
            app.dialogs.pop();
        }
        app.mouse.last_click = None;
        return;
    }
    if app.menu.is_some()
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
        Some(Target::Menu(index)) => {
            if app.menu.is_some_and(|(open, _)| open == index) {
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
                ScrollAction::Up => panel.step(-1),
                ScrollAction::Down => panel.step(1),
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
                        panel.step(if event.row < thumb { -page } else { page });
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
