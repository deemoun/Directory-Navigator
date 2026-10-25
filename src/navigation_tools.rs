use crate::{app::App, dialog, menu};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Slot(usize),
    Go,
    Set,
    Clear,
    Close,
    ApplyFilter,
    FilterField,
}
pub enum Modal {
    Bookmarks {
        slot: usize,
        focus: usize,
    },
    Filter {
        field: String,
        cursor: usize,
        focus: usize,
    },
}
pub fn open_bookmarks(app: &mut App) {
    menu::close(app);
    app.quick_search = None;
    app.navigation_dialog = Some(Modal::Bookmarks { slot: 0, focus: 0 });
}
pub fn open_filter(app: &mut App) {
    if app.panels[app.active].loading {
        return;
    }
    menu::close(app);
    app.quick_search = None;
    let field = app.panels[app.active].filter.clone();
    let cursor = field.len();
    app.navigation_dialog = Some(Modal::Filter {
        field,
        cursor,
        focus: 0,
    });
}
pub fn assign(app: &mut App, slot: usize) {
    if slot < 9 {
        app.config.bookmarks[slot] = Some(app.panels[app.active].path.clone());
        app.notice = format!("Bookmark {} set", slot + 1);
    }
}
pub fn go(app: &mut App, slot: usize) {
    if app.panels[app.active].loading {
        app.error("Wait for directory navigation to finish.");
        return;
    }
    if let Some(path) = app.config.bookmarks.get(slot).and_then(Clone::clone) {
        app.navigation_dialog = None;
        app.panels[app.active].go_to(path, app.config.show_hidden);
    } else {
        app.notice = format!("Bookmark {} is empty", slot + 1);
    }
}
pub fn action(app: &mut App, action: Action) {
    let slot = match &app.navigation_dialog {
        Some(Modal::Bookmarks { slot, .. }) => *slot,
        _ => 0,
    };
    match action {
        Action::Slot(n) => {
            if n < 9 {
                if let Some(Modal::Bookmarks { slot, focus }) = &mut app.navigation_dialog {
                    *slot = n;
                    *focus = 0;
                }
            }
        }
        Action::Go => go(app, slot),
        Action::Set => assign(app, slot),
        Action::Clear => app.config.bookmarks[slot] = None,
        Action::Close => app.navigation_dialog = None,
        Action::ApplyFilter => {
            if let Some(Modal::Filter { field, .. }) = app.navigation_dialog.take() {
                app.panels[app.active].set_filter(field);
            }
        }
        Action::FilterField => {
            if let Some(Modal::Filter { focus, .. }) = &mut app.navigation_dialog {
                *focus = 0;
            }
        }
    }
}
pub fn paste(app: &mut App, text: &str) {
    if let Some(Modal::Filter {
        field,
        cursor,
        focus: 0,
    }) = &mut app.navigation_dialog
    {
        dialog::paste_input(field, cursor, text);
    }
}
pub fn key(app: &mut App, key: KeyEvent) {
    if key.code == KeyCode::Esc {
        app.navigation_dialog = None;
        return;
    }
    let mut command = None;
    match &mut app.navigation_dialog {
        Some(Modal::Bookmarks { slot, focus }) => match key.code {
            KeyCode::Tab => *focus = crate::control_focus::cycle(*focus, 5, false),
            KeyCode::BackTab => *focus = crate::control_focus::cycle(*focus, 5, true),
            KeyCode::Up => *slot = slot.saturating_sub(1),
            KeyCode::Down => *slot = (*slot + 1).min(8),
            KeyCode::Home => *slot = 0,
            KeyCode::End => *slot = 8,
            KeyCode::Char(c) if ('1'..='9').contains(&c) => *slot = c as usize - '1' as usize,
            KeyCode::Enter => {
                command = Some(match *focus {
                    2 => Action::Set,
                    3 => Action::Clear,
                    4 => Action::Close,
                    _ => Action::Go,
                })
            }
            _ => {}
        },
        Some(Modal::Filter {
            field,
            cursor,
            focus,
        }) => match key.code {
            KeyCode::Tab => *focus = crate::control_focus::cycle(*focus, 3, false),
            KeyCode::BackTab => *focus = crate::control_focus::cycle(*focus, 3, true),
            KeyCode::Enter => {
                command = Some(if *focus == 2 {
                    Action::Close
                } else {
                    Action::ApplyFilter
                })
            }
            code if *focus == 0
                && !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                dialog::edit_input(field, cursor, code)
            }
            _ => {}
        },
        _ => {}
    }
    if let Some(command) = command {
        action(app, command);
    }
}
pub fn mouse(app: &mut App, event: MouseEvent, target: Option<crate::mouse::Target>) {
    if event.kind == MouseEventKind::Down(MouseButton::Left) {
        if let Some(crate::mouse::Target::Navigation(command)) = target {
            action(app, command);
        }
    }
}
