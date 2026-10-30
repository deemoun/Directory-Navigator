use crate::app::App;
use crate::focus::{self, Focus};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

pub fn handle(app: &mut App, mut key: KeyEvent) {
    if key.code == KeyCode::Tab && key.modifiers.contains(KeyModifiers::SHIFT) {
        key.code = KeyCode::BackTab;
    }
    if app.output_visible {
        crate::terminal_output::event(app, &crossterm::event::Event::Key(key));
        return;
    }
    if app.exit_pending && app.quit_review.is_some() {
        if key.code == KeyCode::Esc {
            app.abort_quit();
        }
        return;
    }
    handle_key(app, key);
    crate::layout::update(app);
}
fn handle_key(app: &mut App, key: KeyEvent) {
    if key.kind == KeyEventKind::Release {
        return;
    }
    if app.screensaver.activity(std::time::Instant::now()) {
        app.mouse.reset_click();
        return;
    }
    if crate::windowing::workspace::key(app, key) {
        return;
    }
    app.mouse.reset_click();
    let Some(key) = crate::shortcuts::key(app, key) else {
        return;
    };
    let focus = focus::current(app);
    if (key.code == KeyCode::Char('o') && key.modifiers == KeyModifiers::CONTROL
        || key.code == KeyCode::F(5) && key.modifiers == KeyModifiers::ALT)
        && matches!(focus, Focus::Manager | Focus::Menu | Focus::QuickSearch)
    {
        if key.kind == KeyEventKind::Press {
            crate::terminal_output::request(app);
        }
        return;
    }
    if focus == Focus::WindowList {
        crate::dialog_adapter::key(app, key, focus);
        return;
    }
    if key.code == KeyCode::Char('0') && key.modifiers == KeyModifiers::ALT {
        crate::windowing::open(app);
        return;
    }
    let exit = key.code == KeyCode::F(10)
        || (key.code == KeyCode::Char('x') && key.modifiers.contains(KeyModifiers::ALT));
    if exit
        && focus != Focus::SaverSetup
        && crate::focus::screen(app) != crate::focus::Screen::Editor
    {
        app.request_exit();
        return;
    }
    if focus == Focus::Message {
        if let Some(help) = app.modals.native_help_mut(app.desktop.active().id) {
            let close = help.key(key);
            if close {
                app.pop_dialog();
            }
            return;
        }
        if crate::dialog_controls::key(app, key, focus) {
            return;
        }
        crate::dialog::message_key(app, key.code);
        return;
    }
    if crate::dialog_adapter::key(app, key, focus) {
        return;
    }
    if focus == Focus::Welcome {
        crate::welcome::key(app, key);
        return;
    }
    if focus == Focus::SaverSetup {
        crate::screensaver::key(app, key.code);
        return;
    }
    if matches!(
        focus,
        Focus::Editor | Focus::EditorModal | Focus::EditorMenu
    ) {
        crate::editor::key(app, key);
        return;
    }
    if focus == Focus::QuickSearch && crate::quick_search::key(app, key) {
        return;
    }
    if focus == Focus::Plugin {
        crate::plugins::host::key(app, key);
        return;
    }
    if focus == Focus::Job && crate::dialog_controls::key(app, key, focus) {
        return;
    }
    if let Some(job) = app.job.as_ref().filter(|_| focus == Focus::Job) {
        if key.code == KeyCode::Esc {
            job.cancel();
            app.notice = "Cancelling…".into();
        }
        return;
    }
    if focus == Focus::Colors {
        crate::palette::key_event(app, key);
        return;
    }
    if focus == Focus::Settings {
        crate::settings::key(app, key.code);
        return;
    }
    if focus == Focus::Pattern {
        crate::pattern::key(app, key);
        return;
    }
    if focus == Focus::Columns {
        crate::columns::key(app, key.code);
        return;
    }
    if focus == Focus::FileInfo && crate::dialog_controls::key(app, key, focus) {
        return;
    }
    if let Some(info) = app
        .modals
        .file_info_mut()
        .filter(|_| focus == Focus::FileInfo)
    {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => app.modals.set_file_info(None),
            KeyCode::Up => info.step(-1),
            KeyCode::Down => info.step(1),
            KeyCode::PageUp => info.step(-10),
            KeyCode::PageDown => info.step(10),
            KeyCode::Home => {
                info.offset = 0;
                info.horizontal = 0;
            }
            KeyCode::End => info.step(isize::MAX),
            KeyCode::Left => info.horizontal = info.horizontal.saturating_sub(8),
            KeyCode::Right => info.horizontal = info.horizontal.saturating_add(8),
            _ => {}
        }
        return;
    }
    if matches!(focus, Focus::Manager | Focus::Menu | Focus::QuickSearch)
        && key.code == KeyCode::F(9)
        && key.modifiers.contains(KeyModifiers::ALT)
    {
        crate::plugins::host::open(app, crate::plugins::TETRIS);
        return;
    }
    if focus == Focus::Menu {
        if key.modifiers.contains(KeyModifiers::ALT) {
            if let KeyCode::Char(c) = key.code {
                if crate::menu::open_mnemonic(app, c) {
                    return;
                }
            }
        }
        crate::menu::key(app, key.code);
        return;
    }
    if focus == Focus::CopyHistory {
        crate::copy_dialog::history_key(app, key);
        return;
    }
    if focus == Focus::Prompt && app.modals.prompt().is_some_and(|p| p.copy.is_some()) {
        crate::copy_dialog::key(app, key);
        return;
    }
    if focus == Focus::Prompt {
        crate::turbo::forms::prompt_key(app, key);
        return;
    }

    if matches!(focus, Focus::Viewer | Focus::ViewerSearch) {
        crate::viewer::key(app, key);
        return;
    }
    if focus == Focus::Manager && crate::command_line::key(app, key) {
        return;
    }
    if key.modifiers.contains(KeyModifiers::ALT) {
        if crate::commands::manager_modified(app, key) {
            return;
        }
        match key.code {
            KeyCode::Char(c) if ('1'..='9').contains(&c) => {
                crate::navigation_tools::go(app, c as usize - '1' as usize)
            }
            KeyCode::Char(c) => {
                crate::menu::open_mnemonic(app, c);
            }
            _ => {}
        }
        return;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        if crate::commands::manager_modified(app, key) {
            return;
        }
        if let KeyCode::Char(c @ '1'..='9') = key.code {
            crate::navigation_tools::assign(app, c as usize - '1' as usize);
        }
        return;
    }
    if crate::turbo::panes::key(app, key) {
        return;
    }

    crate::commands::manager_key(app, key);
}

/// Paste uses the same topmost recipient as keyboard and mouse input.
pub fn paste(app: &mut App, text: &str) {
    if app.desktop.native().manipulating() {
        return;
    }
    if app.output_visible {
        return;
    }
    paste_text(app, text);
    crate::layout::update(app);
}
fn paste_text(app: &mut App, text: &str) {
    app.shortcuts.clear();
    if app.screensaver.activity(std::time::Instant::now()) {
        app.mouse.reset_click();
        return;
    }
    match focus::current(app) {
        Focus::Editor | Focus::EditorModal => crate::editor::paste(app, text),
        Focus::Viewer | Focus::ViewerSearch => crate::viewer::paste(app, text),
        Focus::Plugin => crate::plugins::host::paste(app, text),
        Focus::Manager => crate::command_line::paste(app, text),
        Focus::Navigation => crate::navigation_tools::paste(app, text),
        Focus::Utility => {}
        Focus::FileTools => app.file_tools.paste(text),
        Focus::CommandHistory => crate::command_line::history::paste(app, text),
        Focus::Discovery => crate::discovery::paste(app, text),
        Focus::QuickSearch => {
            if let Some(s) = app.modals.quick_search_mut() {
                s.paste(&mut app.panels[s.panel], text);
            }
        }
        Focus::Pattern => {
            if let Some(p) = app.modals.pattern_mut() {
                p.paste(text);
            }
        }
        Focus::Prompt => {
            if let Some(p) = app.modals.prompt_mut() {
                p.paste(text);
            }
        }
        _ => {}
    }
}
