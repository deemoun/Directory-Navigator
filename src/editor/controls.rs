use super::*;
use crate::{app::App, dialog, menu};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorAction {
    Open,
    Save,
    SaveAs,
    Find,
    FindNext,
    FindPrevious,
    Replace,
    GoTo,
    Colors,
    AutoIndent,
    ExpandTabs,
    TabWidth,
    SaveDesktop,
    Duplicate,
    SelectAll,
    Clear,
    Undo,
    Redo,
    Copy,
    Cut,
    Paste,
    Help,
    Close,
    Quit,
    Submit,
    Cancel,
    Discard,
    SearchSubmit(bool),
    SearchChoice(usize),
    ReplaceYes,
    ReplaceNo,
    ReplaceAll,
    Menu,
    CancelMenu,
    MenuSurface,
    MenuRoot(usize),
    MenuRow(usize),
    Position(usize, bool),
}
pub fn open(app: &mut App, new: bool) {
    if app.editor.is_some() || app.job.is_some() || app.tetris.is_some() || app.viewer.is_some() {
        return;
    }
    let panel = &app.panels[app.active];
    if panel.loading {
        return;
    }
    let result = if new {
        Editor::new(panel.path.clone())
    } else {
        let Some(entry) = panel.entries.get(panel.cursor) else {
            return;
        };
        if entry.directory || entry.parent || entry.symlink {
            app.error("F4 edits regular text files; directories and links cannot be edited.");
            return;
        }
        Editor::load(entry.path.clone(), panel.path.clone())
    };
    match result {
        Ok(mut editor) => {
            editor.apply_preferences(app.config.editor);
            editor.search = app.editor_search.query.clone();
            editor.replacement = app.editor_search.replacement.clone();
            editor.search_options = app.editor_search.options;
            menu::close(app);
            app.quick_search = None;
            app.editor = Some(editor);
        }
        Err(error) => app.error(error),
    }
}
pub fn open_path(app: &mut App, path: PathBuf) {
    if app.editor.is_some() {
        request_action(app, PendingAction::Open(path));
        return;
    }
    match Editor::load(path, app.panels[app.active].path.clone()) {
        Ok(mut editor) => {
            editor.apply_preferences(app.config.editor);
            editor.search = app.editor_search.query.clone();
            editor.replacement = app.editor_search.replacement.clone();
            editor.search_options = app.editor_search.options;
            app.editor = Some(editor);
        }
        Err(error) => app.error(error),
    }
}
pub fn poll(app: &mut App) -> bool {
    let reply = app
        .editor
        .as_ref()
        .and_then(|editor| editor.worker.as_ref())
        .and_then(|worker| match worker.try_recv() {
            Ok(reply) => Some(reply),
            Err(mpsc::TryRecvError::Disconnected) => Some(Reply::Saved(Err(
                "Editor worker stopped; unsaved text is retained.".into(),
            ))),
            Err(mpsc::TryRecvError::Empty) => None,
        });
    let Some(reply) = reply else {
        return false;
    };
    let mut error = None;
    let mut saved = false;
    if let Some(editor) = &mut app.editor {
        editor.worker = None;
        editor.saving = false;
        match reply {
            Reply::Loaded(path, Ok((mut buffer, snapshot, bom))) => {
                buffer.tab_size = usize::from(editor.preferences.tab_size);
                editor.buffer = Some(buffer);
                editor.original = Some(snapshot);
                editor.bom = bom;
                editor.base = path
                    .parent()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| editor.base.clone());
                app.file_tools
                    .histories
                    .record(crate::file_tools::history::HistoryKind::Edit, path.clone());
                editor.path = Some(path);
                editor.highlight = Cache::new(Language::for_path(editor.path.as_deref()));
                editor.scroll_row = 0;
                editor.scroll_column = 0;
                editor.search_scope = None;
                editor.modal = None;
                editor.notice = "UTF-8 · F2 Save · F7 Search · F10 Menu · Esc Close".into();
                editor.recolor();
            }
            Reply::Saved(Ok((path, snapshot))) => {
                editor.base = path
                    .parent()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| editor.base.clone());
                if editor.path.as_ref() != Some(&path) {
                    app.file_tools
                        .histories
                        .record(crate::file_tools::history::HistoryKind::Edit, path.clone());
                }
                editor.path = Some(path);
                editor.original = Some(snapshot);
                if let Some(buffer) = &mut editor.buffer {
                    buffer.mark_saved();
                }
                editor.notice = "Saved".into();
                editor.recolor();
                saved = true;
            }
            Reply::Loaded(_, Err(message)) | Reply::Saved(Err(message)) => {
                editor.closing = None;
                editor.notice = "File access failed — current text retained".into();
                error = Some(message);
            }
        }
    }
    if let Some(error) = error {
        if app
            .editor
            .as_ref()
            .is_some_and(|editor| editor.buffer.is_none())
        {
            app.editor = None;
        }
        app.error(error);
    } else if let Some(pending) = app.editor.as_mut().and_then(|editor| editor.closing.take()) {
        request_action(app, pending);
    }
    if saved {
        app.refresh();
    }
    true
}
pub fn request_close(app: &mut App, intent: CloseIntent) {
    request_action(
        app,
        match intent {
            CloseIntent::Panel => PendingAction::Close,
            CloseIntent::Quit => PendingAction::Quit,
        },
    );
}
fn request_action(app: &mut App, pending: PendingAction) {
    let Some(editor) = &mut app.editor else {
        return;
    };
    editor.menu = None;
    editor.closing = Some(pending);
    if editor.busy() && !editor.saving {
        editor.worker = None;
    }
    if editor.busy() {
        editor.notice = "Finishing file access before switching…".into();
        return;
    }
    if editor.dirty() {
        editor.modal = Some(Modal::ConfirmClose);
        return;
    }
    complete_pending(app);
}
fn complete_pending(app: &mut App) {
    let pending = app.editor.as_mut().and_then(|editor| {
        editor.modal = None;
        editor.closing.take()
    });
    match pending {
        Some(PendingAction::Open(path)) => {
            if let Some(editor) = &mut app.editor {
                if let Err(error) = editor.load_next(path) {
                    app.error(error);
                }
            }
        }
        Some(PendingAction::Close) => app.editor = None,
        Some(PendingAction::Quit) => {
            app.editor = None;
            app.quit = true;
        }
        None => {}
    }
}
fn prompt(editor: &mut Editor, kind: PromptKind) {
    let field = if kind == PromptKind::TabWidth {
        editor.preferences.tab_size.to_string()
    } else {
        String::new()
    };
    let cursor = field.len();
    editor.modal = Some(Modal::Prompt {
        kind,
        field,
        cursor,
    });
}
pub fn paste(app: &mut App, text: &str) {
    let Some(editor) = &mut app.editor else {
        return;
    };
    if editor.busy() {
        return;
    }
    match &mut editor.modal {
        Some(Modal::Prompt { field, cursor, .. }) => {
            dialog::paste_input(field, cursor, text);
            return;
        }
        Some(Modal::Search(search)) if search.focus < 2 => {
            let field = if search.focus == 0 {
                &mut search.query
            } else {
                &mut search.replacement
            };
            dialog::paste_input(field, &mut search.cursors[search.focus], text);
            return;
        }
        Some(_) => return,
        None => {}
    }
    let result = editor
        .buffer
        .as_mut()
        .map(|buffer| buffer.insert(text))
        .unwrap_or(Ok(()));
    editor.recolor();
    editor.keep_visible();
    if let Err(error) = result {
        app.error(error);
    }
}
fn submit_prompt(app: &mut App) -> Result<(), String> {
    let Some(editor) = &mut app.editor else {
        return Ok(());
    };
    let Some(Modal::Prompt {
        kind,
        field,
        cursor,
    }) = editor.modal.take()
    else {
        return Ok(());
    };
    match kind {
        PromptKind::Open if !field.is_empty() => {
            let path = editor.base.join(field);
            request_action(app, PendingAction::Open(path));
        }
        PromptKind::SaveAs if !field.is_empty() => editor.save(editor.base.join(field), true)?,
        PromptKind::GoTo => {
            let line = field
                .parse::<usize>()
                .ok()
                .filter(|line| *line > 0)
                .ok_or("Enter a positive line number.")?;
            if let Some(buffer) = &mut editor.buffer {
                buffer.move_to(buffer.position_at(line - 1, 0), false);
            }
        }
        PromptKind::TabWidth => {
            let width = field
                .parse::<u8>()
                .ok()
                .filter(|v| (1..=16).contains(v))
                .ok_or("Tab width must be between 1 and 16.")?;
            app.config.editor.tab_size = width;
            editor.apply_preferences(app.config.editor);
        }
        _ => {
            editor.modal = Some(Modal::Prompt {
                kind,
                field,
                cursor,
            });
            return Err("Enter a filename.".into());
        }
    }
    Ok(())
}
pub fn action(app: &mut App, command: EditorAction) {
    use EditorAction as A;
    if command == A::Quit {
        request_action(app, PendingAction::Quit);
        return;
    }
    if command == A::Close {
        request_action(app, PendingAction::Close);
        return;
    }
    if app.editor.as_ref().is_none_or(Editor::busy) {
        return;
    }
    if let A::MenuRoot(root) = command {
        menus::open(app, root);
        return;
    }
    if let A::MenuRow(row) = command {
        menus::activate(app, row);
        return;
    }
    if command == A::Menu {
        menus::open(app, 0);
        return;
    }
    if !matches!(command, A::MenuSurface | A::SearchChoice(_)) {
        if let Some(editor) = &mut app.editor {
            editor.menu = None;
        }
    }
    let result = match command {
        A::Submit => submit_prompt(app),
        A::SearchSubmit(all) => search_controls::submit(app, all),
        A::Discard
            if app
                .editor
                .as_ref()
                .is_some_and(|e| matches!(e.modal, Some(Modal::ConfirmClose))) =>
        {
            complete_pending(app);
            Ok(())
        }
        A::SaveDesktop => {
            app.save_config();
            if let Some(editor) = &mut app.editor {
                editor.notice = if app.dialogs.is_empty() {
                    "Desktop preferences saved"
                } else {
                    "Desktop preferences were not saved"
                }
                .into();
            }
            Ok(())
        }
        _ => apply(app, command),
    };
    if let Some(editor) = &mut app.editor {
        editor.recolor();
        editor.keep_visible();
    }
    if let Err(error) = result {
        app.error(error);
    }
}
fn apply(app: &mut App, command: EditorAction) -> Result<(), String> {
    use EditorAction as A;
    let Some(editor) = &mut app.editor else {
        return Ok(());
    };
    if matches!(command, A::ReplaceYes | A::ReplaceNo | A::ReplaceAll)
        || (command == A::Cancel && matches!(editor.modal, Some(Modal::Replace(_))))
    {
        return search_controls::decision(editor, command);
    }
    match command {
        A::CancelMenu => editor.menu = None,
        A::Cancel => {
            editor.modal = None;
            editor.closing = None;
        }
        A::Open => prompt(editor, PromptKind::Open),
        A::Save => {
            editor.modal = None;
            if let Some(path) = editor.path.clone() {
                editor.save(path, false)?;
            } else {
                prompt(editor, PromptKind::SaveAs);
            }
        }
        A::SaveAs => prompt(editor, PromptKind::SaveAs),
        A::GoTo => prompt(editor, PromptKind::GoTo),
        A::TabWidth => prompt(editor, PromptKind::TabWidth),
        A::Find | A::Replace => search_controls::open(editor, command == A::Replace),
        A::FindNext | A::FindPrevious => {
            search_controls::repeat(editor, command == A::FindPrevious)?
        }
        A::SearchChoice(choice) => {
            if let Some(Modal::Search(dialog)) = &mut editor.modal {
                dialog.focus = choice;
                dialog.toggle(choice);
            }
        }
        A::Colors | A::AutoIndent | A::ExpandTabs => {
            match command {
                A::Colors => app.config.editor.syntax = !app.config.editor.syntax,
                A::AutoIndent => app.config.editor.auto_indent = !app.config.editor.auto_indent,
                _ => app.config.editor.expand_tabs = !app.config.editor.expand_tabs,
            }
            editor.apply_preferences(app.config.editor);
        }
        A::Help => editor.modal = Some(Modal::Help),
        A::Paste => {
            let text = app.editor_clipboard.clone();
            paste(app, &text);
        }
        _ => {
            if let Some(buffer) = &mut editor.buffer {
                match command {
                    A::Undo => buffer.undo(false),
                    A::Redo => buffer.undo(true),
                    A::Duplicate => buffer.duplicate_line()?,
                    A::SelectAll => {
                        buffer.move_to(0, false);
                        buffer.move_to(buffer.text.len(), true);
                    }
                    A::Copy | A::Cut | A::Clear => {
                        let range = buffer.selected();
                        if !range.is_empty() {
                            if command != A::Clear {
                                app.editor_clipboard = buffer.text[range.clone()].into();
                            }
                            if command != A::Copy {
                                buffer.replace(range, "")?;
                            }
                            editor.notice =
                                "Editor clipboard retained across files; terminal paste also works"
                                    .into();
                        }
                    }
                    A::Position(position, select) => buffer.move_to(position, select),
                    _ => {}
                }
            }
        }
    }
    Ok(())
}
