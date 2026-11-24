use crate::panel::filter::{FilenameFilter, TypeRow, TypesFilter};
use crate::{app::App, dialog, menu};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use std::{path::PathBuf, time::Instant};
#[derive(Clone, Debug)]
pub struct FilterDraft {
    pub panel: usize,
    pub path: PathBuf,
    pub location: (u64, usize),
    pub revision: u64,
    pub types_mode: bool,
    pub types: TypesFilter,
    pub rows: Vec<TypeRow>,
    pub limited: bool,
    pub row: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Slot(usize),
    Go,
    Set,
    Clear,
    Close,
    ApplyFilter,
    FilterField,
    FilterMode,
    FilterPolicy,
    FilterType(usize),
    ClearFilter,
    #[cfg(windows)]
    Drive(u8),
}
pub enum Modal {
    #[cfg(windows)]
    Drives {
        panel: usize,
        letters: Vec<u8>,
        focus: usize,
    },
    Bookmarks {
        slot: usize,
        focus: usize,
        pending: Option<(std::path::PathBuf, u64)>,
        last_click: Option<((usize, Option<PathBuf>), Instant)>,
    },
    Filter {
        draft: FilterDraft,
        field: String,
        cursor: usize,
        focus: usize,
    },
}
impl Modal {
    pub(crate) fn focus_mut(&mut self) -> &mut usize {
        match self {
            Self::Bookmarks { focus, .. } | Self::Filter { focus, .. } => focus,
            #[cfg(windows)]
            Self::Drives { focus, .. } => focus,
        }
    }
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
        last_click: None,
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
    let panel = &app.panels[app.active];
    let (rows, limited) = panel.file_types();
    let draft = FilterDraft {
        panel: app.active,
        path: panel.path.clone(),
        location: panel.location_key(),
        revision: panel.revision,
        types_mode: panel.types_filter.is_some(),
        types: panel.types_filter.clone().unwrap_or_default(),
        rows,
        limited,
        row: 0,
    };
    app.modals.set_navigation_dialog(Some(Modal::Filter {
        draft,
        field,
        cursor,
        focus: 1,
    }));
}
pub fn assign(app: &mut App, slot: usize) {
    reset_click(app);
    if !crate::archive::require_native(app) {
        return;
    }
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
    reset_click(app);
    let slot = match app.navigation_dialog() {
        Some(Modal::Bookmarks { slot, .. }) => *slot,
        _ => 0,
    };
    match action {
        #[cfg(windows)]
        Action::Drive(letter) => {
            if let Some(Modal::Drives { panel, letters, .. }) = app.navigation_dialog() {
                if !letters.contains(&letter) {
                    return;
                }
                let panel = *panel;
                app.set_navigation_dialog(None);
                crate::windows_drives::navigate(app, panel, letter);
            }
        }
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
            let Some(Modal::Filter { draft, field, .. }) = app.navigation_dialog() else {
                return;
            };
            let panel = &app.panels[draft.panel];
            if panel.loading
                || panel.path != draft.path
                || panel.location_key() != draft.location
                || panel.revision != draft.revision
            {
                app.error("Directory changed; reopen the filename filter.");
                return;
            }
            let index = draft.panel;
            let filter = if draft.types_mode {
                FilenameFilter::Types(draft.types.clone())
            } else {
                FilenameFilter::Wildcard(field.clone())
            };
            if let Err(error) = app.panels[index].set_filename_filter(filter) {
                app.error(error.to_string());
            } else {
                app.set_navigation_dialog(None);
            }
        }
        Action::FilterField => {
            if let Some(Modal::Filter { focus, .. }) = app.navigation_dialog_mut() {
                *focus = 1;
            }
        }
        Action::FilterMode => {
            if let Some(Modal::Filter { draft, .. }) = app.navigation_dialog_mut() {
                draft.types_mode = !draft.types_mode;
            }
        }
        Action::FilterPolicy => {
            if let Some(Modal::Filter { draft, .. }) = app.navigation_dialog_mut() {
                draft.types.hide = !draft.types.hide;
            }
        }
        Action::FilterType(row) => {
            if let Some(Modal::Filter { draft, focus, .. }) = app.navigation_dialog_mut() {
                if let Some(value) = draft.rows.get(row) {
                    draft.row = row;
                    *focus = 1;
                    if !draft.types.selected.remove(&value.extension) {
                        draft.types.selected.insert(value.extension.clone());
                    }
                }
            }
        }
        Action::ClearFilter => {
            if let Some(Modal::Filter {
                draft,
                field,
                cursor,
                focus,
            }) = app.navigation_dialog_mut()
            {
                draft.types = TypesFilter::default();
                draft.types_mode = false;
                field.clear();
                *cursor = 0;
                *focus = 1;
            }
        }
    }
}
pub fn paste(app: &mut App, text: &str) {
    if let Some(Modal::Filter {
        field,
        cursor,
        focus: 1,
        draft,
    }) = app.navigation_dialog_mut()
    {
        if !draft.types_mode {
            dialog::paste_input(field, cursor, text);
        }
    }
}
pub fn key(app: &mut App, key: KeyEvent) {
    reset_click(app);
    if key.code == KeyCode::F(1) && matches!(app.navigation_dialog(), Some(Modal::Filter { .. })) {
        app.push_dialog(crate::dialog::Dialog {
            title: " Filename filter ",
            message: HELP.into(),
        });
        return;
    }
    if key.code == KeyCode::Char(' ') {
        if let Some(Modal::Filter {
            draft, focus: 1, ..
        }) = app.navigation_dialog()
        {
            if draft.types_mode {
                let row = draft.row;
                action(app, Action::FilterType(row));
                return;
            }
        }
    }
    if crate::dialog_controls::key(app, key, crate::focus::Focus::Navigation) {
        return;
    }
    #[cfg(windows)]
    if matches!(app.navigation_dialog(), Some(Modal::Drives { .. })) {
        crate::windows_drives::key(app, key);
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
            draft,
            field,
            cursor,
            focus,
        }) => match key.code {
            code if *focus == 1 && draft.types_mode => {
                if let Some(row) =
                    crate::turbo::widgets::list_key(draft.row, draft.rows.len(), 6, code)
                {
                    draft.row = row;
                }
            }
            code if *focus == 1
                && !draft.types_mode
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
    if matches!(
        event.kind,
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
    ) {
        if let Some(Modal::Filter { draft, .. }) = app.navigation_dialog_mut() {
            if draft.types_mode {
                draft.row = if event.kind == MouseEventKind::ScrollUp {
                    draft.row.saturating_sub(1)
                } else {
                    (draft.row + 1).min(draft.rows.len().saturating_sub(1))
                };
                return;
            }
        }
    }
    if event.kind == MouseEventKind::Down(MouseButton::Left) {
        if let Some(crate::mouse::Target::Navigation(Action::Slot(slot))) = target {
            if let Some(path) = app.config.bookmarks.get(slot) {
                let identity = (slot, path.clone());
                let now = Instant::now();
                let double = match app.navigation_dialog() {
                    Some(Modal::Bookmarks { last_click, .. }) => {
                        crate::list_state::ClickTracker::STRICT.matches(
                            last_click.as_ref(),
                            &identity,
                            now,
                        )
                    }
                    _ => false,
                };
                action(app, Action::Slot(slot));
                if double {
                    action(app, Action::Go);
                } else if let Some(Modal::Bookmarks { last_click, .. }) =
                    app.navigation_dialog_mut()
                {
                    *last_click = Some((identity, now));
                }
                return;
            }
        }
        reset_click(app);
        if let Some(crate::mouse::Target::Navigation(command)) = target {
            action(app, command);
        }
    } else if matches!(
        event.kind,
        MouseEventKind::Down(_)
            | MouseEventKind::Drag(_)
            | MouseEventKind::ScrollUp
            | MouseEventKind::ScrollDown
    ) {
        reset_click(app);
    }
}

fn reset_click(app: &mut App) {
    if let Some(Modal::Bookmarks { last_click, .. }) = app.navigation_dialog_mut() {
        *last_click = None;
    }
}

pub const HELP: &str = "Wildcard keeps case-sensitive * and ? matching; semicolons remain literal. File types lists extensions from the loaded panel, including entries hidden by its filename filter. The hidden-file setting still applies.
Space/click toggles types; arrows, Page keys and wheel move. Show selected includes those types; Hide selected excludes them. Directories stay visible in File types mode. Extensions match exactly: .jpg and .JPG differ. No extension includes dotfiles without a suffix; a trailing dot is a separate empty extension.
Apply changes only the originating pane. Cancel/Esc keeps its filter. Clear resets the draft; Apply with no selected types clears filtering. Marks are retained; operations use visible marks. Up to 256 types and 4 KiB of serialized selection are supported; use Wildcard for types not shown. No directory scan is started.";
