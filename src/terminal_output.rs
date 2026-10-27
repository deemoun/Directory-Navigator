//! Reveal the terminal's primary screen; no shell is started and no output is replayed.
use crate::{
    app::App,
    focus::{self, Focus},
};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

pub fn request(app: &mut App) {
    if app.output_visible
        || app.quit
        || app.exit_pending
        || app.screensaver.active
        || !matches!(
            focus::current(app),
            Focus::Manager | Focus::Menu | Focus::QuickSearch
        )
        || app.command_line.pending.is_some()
        || app.user_menu.pending.is_some()
        || app.associations.pending.is_some()
    {
        return;
    }
    crate::menu::close(app);
    app.modals.set_quick_search(None);
    app.shortcuts.clear();
    app.mouse.regions.clear();
    app.mouse.frame = None;
    app.mouse.scrollbar_drag = None;
    app.mouse.reset_click();
    app.output_visible = true;
}
pub fn return_key(key: KeyEvent) -> bool {
    key.kind == KeyEventKind::Press
        && (matches!(key.code, KeyCode::Enter | KeyCode::Esc)
            && key.modifiers == KeyModifiers::NONE
            || key.code == KeyCode::Char('o') && key.modifiers == KeyModifiers::CONTROL)
}
pub fn event(app: &mut App, event: &Event) {
    if let Event::Key(key) = event {
        if return_key(*key) {
            app.output_visible = false;
            app.screensaver.activity(std::time::Instant::now());
            app.shortcuts.clear();
        }
    }
    // Paste, clicks and all other keys are consumed, never queued as commands.
}
