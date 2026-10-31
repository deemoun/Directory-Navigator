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
        pending: Option<(std::path::PathBuf, u64)>,
    },
    Filter {
        field: String,
        cursor: usize,
        focus: usize,
    },
}
pub fn open_bookmarks(app: &mut App) {
    if !crate::tool_windows::open(app, crate::tool_windows::Kind::Bookmarks, None) {
        return;
    }
    menu::close(app);
    app.modals.set_quick_search(None);
    app.set_navigation_dialog(Some(Modal::Bookmarks {
        slot: 0,
        focus: 0,
        pending: None,
    }));
}
pub fn open_filter(app: &mut App) {
    if app.panels[app.active].loading {
        return;
    }
    menu::close(app);
    app.modals.set_quick_search(None);
    let field = app.panels[app.active].filter.clone();
    let cursor = field.len();
    app.modals.set_navigation_dialog(Some(Modal::Filter {
        field,
        cursor,
        focus: 0,
    }));
}
pub fn assign(app: &mut App, slot: usize) {
    if slot < 9 {
        let panel = app
            .desktop
            .tool(app.desktop.target())
            .map_or(app.active, |t| t.panel);
        app.config.bookmarks[slot] = Some(app.panels[panel].path.clone());
        app.notice = format!("Bookmark {} set", slot + 1);
    }
}
pub fn go(app: &mut App, slot: usize) {
    let panel = app
        .desktop
        .tool(app.desktop.target())
        .map_or(app.active, |t| t.panel);
    if app.panels[panel].loading {
        app.error("Wait for directory navigation to finish.");
        return;
    }
    if let Some(path) = app.config.bookmarks.get(slot).and_then(Clone::clone) {
        let revision = app.panels[panel].revision;
        app.panels[panel].go_to(path.clone(), app.config.show_hidden);
        if app.panels[panel].loading {
            if let Some(Modal::Bookmarks { pending, .. }) = app.navigation_dialog_mut() {
                *pending = Some((path, revision));
            }
        }
        app.active = panel;
        crate::windowing::activate(app, crate::desktop::WindowId::MANAGER);
    } else {
        app.notice = format!("Bookmark {} is empty", slot + 1);
    }
}

/// The tool owns its pending navigation; failures leave its bookmarks available.
pub(crate) fn poll(app: &mut App) -> bool {
    let Some(tool) = app.desktop.tool(app.desktop.target()) else {
        return false;
    };
    let panel = &app.panels[tool.panel];
    let Some(Modal::Bookmarks {
        pending: Some((path, revision)),
        ..
    }) = app.navigation_dialog()
    else {
        return false;
    };
    if panel.loading {
        return false;
    }
    if panel.path == *path && panel.revision != *revision {
        app.set_navigation_dialog(None);
    } else if let Some(Modal::Bookmarks { pending, .. }) = app.navigation_dialog_mut() {
        *pending = None;
    }
    true
}
pub fn action(app: &mut App, action: Action) {
    let slot = match app.navigation_dialog() {
        Some(Modal::Bookmarks { slot, .. }) => *slot,
        _ => 0,
    };
    match action {
        Action::Slot(n) => {
            if n < 9 {
                if let Some(Modal::Bookmarks { slot, focus, .. }) = app.navigation_dialog_mut() {
                    *slot = n;
                    *focus = 0;
                }
            }
        }
        Action::Go => go(app, slot),
        Action::Set => assign(app, slot),
        Action::Clear => app.config.bookmarks[slot] = None,
        Action::Close => app.set_navigation_dialog(None),
        Action::ApplyFilter => {
            if let Some(Modal::Filter { field, .. }) = app.take_navigation_dialog() {
                app.panels[app.active].set_filter(field);
            }
        }
        Action::FilterField => {
            if let Some(Modal::Filter { focus, .. }) = app.navigation_dialog_mut() {
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
    }) = app.navigation_dialog_mut()
    {
        dialog::paste_input(field, cursor, text);
    }
}
pub fn key(app: &mut App, key: KeyEvent) {
    if crate::dialog_controls::key(app, key, crate::focus::Focus::Navigation) {
        return;
    }
    match app.navigation_dialog_mut() {
        Some(Modal::Bookmarks { slot, .. }) => {
            if let Some(row) = crate::turbo::widgets::list_key(*slot, 9, 9, key.code) {
                *slot = row;
                return;
            }
            match key.code {
                KeyCode::Char(c) if ('1'..='9').contains(&c) => *slot = c as usize - '1' as usize,
                _ => {}
            }
        }
        Some(Modal::Filter {
            field,
            cursor,
            focus,
        }) => match key.code {
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
}
pub fn mouse(app: &mut App, event: MouseEvent, target: Option<crate::mouse::Target>) {
    if event.kind == MouseEventKind::Down(MouseButton::Left) {
        if let Some(crate::mouse::Target::Navigation(command)) = target {
            action(app, command);
        }
    }
}
