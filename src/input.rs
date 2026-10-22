use crate::focus::{self, Focus};
use crate::{app::App, dialog::Dialog, operations::Kind, panel::SortMode};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

pub fn handle(app: &mut App, key: KeyEvent) {
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
    app.mouse.reset_click();
    let Some(key) = crate::shortcuts::key(app, key) else {
        return;
    };
    let focus = focus::current(app);
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
        crate::dialog::message_key(app, key.code);
        return;
    }
    if focus == Focus::CommandHistory {
        crate::command_line::history::key(app, key);
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
    if focus == Focus::Game {
        crate::tetris::key(app, key);
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
        crate::palette::key(app, key.code);
        return;
    }
    if focus == Focus::Navigation {
        crate::navigation_tools::key(app, key);
        return;
    }
    if focus == Focus::Utility {
        crate::utilities::key(app, key);
        return;
    }
    if focus == Focus::FileTools {
        crate::file_tools::key(app, key);
        return;
    }
    if focus == Focus::Archive {
        crate::archive::key(app, key);
        return;
    }
    if focus == Focus::UserMenu {
        crate::user_menu::key(app, key);
        return;
    }
    if focus == Focus::Tree {
        crate::directory_tree::key(app, key);
        return;
    }
    if focus == Focus::Discovery {
        crate::discovery::key(app, key);
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
    if let Some(info) = app.file_info.as_mut().filter(|_| focus == Focus::FileInfo) {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => app.file_info = None,
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
        crate::tetris::open(app);
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
    if let Some(prompt) = app.prompt.as_mut().filter(|_| focus == Focus::Prompt) {
        match key.code {
            KeyCode::Esc => app.prompt = None,
            KeyCode::F(1) => crate::dialog::prompt_help(app),
            KeyCode::Char('d' | 'D') if key.modifiers.contains(KeyModifiers::ALT) => {
                prompt.focus = 0
            }
            KeyCode::Char('k' | 'K') if key.modifiers.contains(KeyModifiers::ALT) => {
                app.submit_prompt()
            }
            KeyCode::Char('c' | 'C') if key.modifiers.contains(KeyModifiers::ALT) => {
                app.prompt = None
            }
            KeyCode::Char('h' | 'H') if key.modifiers.contains(KeyModifiers::ALT) => {
                crate::dialog::prompt_help(app)
            }
            KeyCode::Tab | KeyCode::BackTab => {
                let first = usize::from(matches!(
                    prompt.kind,
                    crate::dialog::PromptKind::Operation(Kind::Delete | Kind::Trash)
                ));
                let count = 4 - first;
                let backward =
                    key.code == KeyCode::BackTab || key.modifiers.contains(KeyModifiers::SHIFT);
                prompt.focus = first
                    + (prompt.focus.saturating_sub(first) + if backward { count - 1 } else { 1 })
                        % count;
            }
            KeyCode::Enter => match prompt.focus {
                2 => app.prompt = None,
                3 => crate::dialog::prompt_help(app),
                _ => app.submit_prompt(),
            },
            _ if prompt.focus == 0
                && !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                prompt.edit(key.code)
            }
            _ => {}
        }
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
        match key.code {
            KeyCode::Char(c) if ('1'..='9').contains(&c) => {
                crate::navigation_tools::go(app, c as usize - '1' as usize)
            }
            KeyCode::Delete => crate::navigation_tools::open_filter(app),
            KeyCode::Char('y') => crate::utilities::open(app, crate::utilities::Kind::Memory),
            KeyCode::Backspace => crate::file_tools::open_history(
                app,
                crate::file_tools::history::HistoryKind::Directory,
            ),
            KeyCode::PageUp => {
                crate::file_tools::open_history(app, crate::file_tools::history::HistoryKind::Edit)
            }
            KeyCode::PageDown => {
                crate::file_tools::open_history(app, crate::file_tools::history::HistoryKind::View)
            }
            KeyCode::F(8) => crate::command_line::history::open(app),
            KeyCode::Char('l') => crate::file_tools::open_export(app),
            KeyCode::F(9) => crate::tetris::open(app),
            KeyCode::F(7) => crate::quick_search::open(app),
            KeyCode::Char('x') => app.request_exit(),
            KeyCode::Char('e') => app.open_file_info(),
            KeyCode::Char('k') => app.open_columns(),
            KeyCode::Char('t') => app.prompt_goto(),
            KeyCode::Char('r') => app.panels[app.active].refresh(app.config.show_hidden),
            KeyCode::Char('s') => app.open_settings(crate::settings::SettingsKind::FileManager),
            KeyCode::Char('b') => {
                crate::menu::open(app, 3);
                if let Some(index) = crate::menu::items(3).iter().position(|item| {
                    item.action == crate::menu::Action::Submenu(crate::menu::MenuId::Sort)
                }) {
                    crate::menu::activate(app, 0, index);
                }
            }
            KeyCode::Char(c) => {
                crate::menu::open_mnemonic(app, c);
            }
            _ => {}
        }
        return;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Enter => app.open_external_current(),
            KeyCode::Char(c) if ('1'..='9').contains(&c) => {
                crate::navigation_tools::assign(app, c as usize - '1' as usize)
            }
            KeyCode::Char('k') => crate::utilities::open(app, crate::utilities::Kind::Calculator),
            KeyCode::Char('b') => crate::utilities::open(app, crate::utilities::Kind::Characters),
            KeyCode::Char('c') => crate::file_tools::open_compare(app),
            KeyCode::Char('t') => crate::directory_tree::open(app),
            KeyCode::Char('q') => crate::quick_view::toggle(app),
            KeyCode::F(7) => crate::discovery::open_find(app),
            KeyCode::F(3) => app.panels[app.active].set_sort(SortMode::Name),
            KeyCode::F(4) => app.panels[app.active].set_sort(SortMode::Extension),
            KeyCode::F(5) => app.panels[app.active].set_sort(SortMode::Date),
            KeyCode::F(6) => app.panels[app.active].set_sort(SortMode::Size),
            KeyCode::Char('h') => app.toggle_hidden(),
            KeyCode::Char('l') => app.prompt_goto(),
            KeyCode::Char('a') => app.panels[app.active].select_visible(),
            KeyCode::Char('u') => app.panels[app.active].selected.clear(),
            KeyCode::Char('r') => app.panels[app.active].refresh(app.config.show_hidden),
            _ => {}
        }
        return;
    }
    let brief = app.config.layouts[app.active] == crate::panel_layout::PanelLayout::Brief;
    let panel = &mut app.panels[app.active];
    let capacity = panel.viewport_rows * if brief { 3 } else { 1 };
    match key.code {
        KeyCode::Tab | KeyCode::BackTab => {app.quick_view=None;app.active ^= 1;},
        KeyCode::Char('+') => app.open_pattern(true),
        KeyCode::Char('-') => app.open_pattern(false),
        KeyCode::Up => panel.step(-1),
        KeyCode::Down => panel.step(1),
        KeyCode::Left if brief => panel.move_column(false),
        KeyCode::Right if brief => panel.move_column(true),
        KeyCode::PageUp => panel.step(-(capacity as isize)),
        KeyCode::PageDown => panel.step(capacity as isize),
        KeyCode::Home => panel.cursor = 0,
        KeyCode::End => panel.cursor = panel.entries.len().saturating_sub(1),
        KeyCode::Enter => app.open_current(),
        KeyCode::Backspace if app.config.backspace_parent => panel.parent(app.config.show_hidden),
        KeyCode::Insert => { panel.toggle_selection(); panel.step(1); }
        KeyCode::Char(' ') if app.config.space_select => { panel.toggle_selection(); panel.step(1); }
        KeyCode::Char('*') => crate::menu::execute(app, crate::menu::Action::InvertSelection),
        KeyCode::F(2) if key.modifiers.contains(KeyModifiers::SHIFT) => {
            let next = match panel.sort { SortMode::Name => SortMode::Extension, SortMode::Extension => SortMode::Size, SortMode::Size => SortMode::Date, SortMode::Date => SortMode::Name };
            panel.set_sort(next);
        }
        KeyCode::F(2) => crate::user_menu::open(app,crate::user_menu::Source::Auto),
        KeyCode::F(4) => crate::editor::open(app, key.modifiers.contains(KeyModifiers::SHIFT)),
        KeyCode::F(5) => app.prompt_operation(Kind::Copy, false),
        KeyCode::F(3) => app.open_viewer(),
        KeyCode::F(6) => app.prompt_operation(Kind::Move, key.modifiers.contains(KeyModifiers::SHIFT)),
        KeyCode::F(7) => app.prompt_operation(Kind::Mkdir, false),
        KeyCode::F(8) => app.prompt_operation(Kind::Delete, false),
        KeyCode::Delete if app.config.delete_key => app.prompt_operation(Kind::Delete, false),
        KeyCode::F(9) => crate::menu::open(app, 1),
        KeyCode::F(1) => app.dialogs.push(Dialog { title: " Directory Navigator — Help ",
            message: if app.config.shortcuts.is_mac() { crate::shortcuts::MANAGER_HELP.into() } else { "Tab panels · Arrows/PgUp/PgDn/Home/End navigate\nAlt+F/U/P/M/O menus · Alt+X exits\nEnter app/dir/archive · Ctrl+Enter OS app · F3 view · F4 edit · BS parent\nIns / Space marks · Ctrl+A all · Ctrl+U clear\n+ / - group · Alt+E info · Alt+K columns\nAlt+F7 quick search · Ctrl+F7 recursive Find\nCtrl+Q quick view · Ctrl+T tree · Alt+F9 Tetris\nCtrl+1–9 bookmarks set · Alt+1–9 go · Alt+Del filter\nCtrl+K calculator · Ctrl+B ASCII · Alt+Y memory\nCtrl+C compare · Alt+L list file\nAlt+BS directories · Alt+PgUp edits / PgDn views\nF2 user menu · Shift+F2 sort · Ctrl+F3 name / F4 ext / F5 date / F6 size\nF5 copy · F6 move · Shift+F6 rename here\nF7 mkdir · F8 Trash/delete · F9 menu · F10 exit\nCtrl+H hidden · Ctrl+L go to · Ctrl+R refresh\nClick row · Ctrl+click mark · Double-click open\nWheel scrolls · Esc cancels dialogs/operations\nType commands · Enter runs · Esc clears · Ctrl+E/X history · Alt+F8 list\nFile menu: Copy names / paths uses internal clipboard\nOptions: File associations configures Enter actions\nColors: white dirs · green exec · cyan source\nMagenta archives · blue links · yellow marks\nTrash by default; File Manager Setup changes mode.\nExisting destinations are refused; no overwrites.".into() } }),
        _ => {}
    }
}

/// Paste uses the same topmost recipient as keyboard and mouse input.
pub fn paste(app: &mut App, text: &str) {
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
        Focus::Game => crate::tetris::paste(app, text),
        Focus::Manager => crate::command_line::paste(app, text),
        Focus::Navigation => crate::navigation_tools::paste(app, text),
        Focus::Utility => app.utilities.paste(text),
        Focus::FileTools => app.file_tools.paste(text),
        Focus::CommandHistory => crate::command_line::history::paste(app, text),
        Focus::Discovery => crate::discovery::paste(app, text),
        Focus::QuickSearch => {
            if let Some(s) = &mut app.quick_search {
                s.paste(&mut app.panels[s.panel], text);
            }
        }
        Focus::Pattern => {
            if let Some(p) = &mut app.pattern {
                p.paste(text);
            }
        }
        Focus::Prompt => {
            if let Some(p) = &mut app.prompt {
                p.paste(text);
            }
        }
        _ => {}
    }
}
