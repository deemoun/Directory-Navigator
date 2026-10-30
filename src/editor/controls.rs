use super::*;
use crate::{app::App, dialog, menu};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorAction {
    WindowMove,
    WindowZoom,
    Open,
    FileDialog,
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
    Indent,
    Unindent,
    Uppercase,
    Lowercase,
    Capitalize,
    MatchBracket,
    SortLines,
    SelectAll,
    Clear,
    Undo,
    Redo,
    Copy,
    Cut,
    Paste,
    Help,
    Windows,
    Close,
    Quit,
    Submit,
    Cancel,
    Discard,
    SearchSubmit(bool),
    PromptField,
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
    if app.job.is_some() || matches!(crate::focus::screen(app), crate::focus::Screen::Plugin(_)) {
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
        let path = entry.path.clone();
        menu::close(app);
        app.modals.set_quick_search(None);
        open_path(app, path);
        return;
    };
    match result {
        Ok(mut editor) => {
            editor.apply_preferences(app.config.editor);
            editor.search = app.editor_search.query.clone();
            editor.replacement = app.editor_search.replacement.clone();
            editor.search_options = app.editor_search.options;
            menu::close(app);
            app.modals.set_quick_search(None);
            app.show_editor(editor);
        }
        Err(error) => app.error(error),
    }
}
pub fn open_path(app: &mut App, path: PathBuf) {
    let _ = open_path_with_focus(app, path, true);
}
pub fn open_path_background(app: &mut App, path: PathBuf) {
    let _ = open_path_with_focus(app, path, false);
}
pub(crate) fn open_path_with_focus(
    app: &mut App,
    path: PathBuf,
    foreground: bool,
) -> Option<crate::desktop::WindowId> {
    if let Some(id) = app
        .desktop
        .find_path(crate::desktop::WindowKind::Editor, &path)
    {
        if foreground {
            crate::windowing::activate(app, id);
        }
        return Some(id);
    }
    match Editor::load(path, app.panels[app.active].path.clone()) {
        Ok(mut editor) => {
            editor.apply_preferences(app.config.editor);
            editor.search = app.editor_search.query.clone();
            editor.replacement = app.editor_search.replacement.clone();
            editor.search_options = app.editor_search.options;
            let result = if foreground {
                app.install_document(|desktop| desktop.open_editor(editor))
            } else {
                app.install_document(|desktop| desktop.open_editor_background(editor))
            };
            match result {
                Ok(id) => return Some(id),
                Err(error) => app.error(error.to_string()),
            }
        }
        Err(error) => app.error(error),
    }
    None
}
fn prepare_save(app: &mut App, path: PathBuf, save_as: bool) -> Result<(), String> {
    let id = app.desktop.target();
    if app.desktop.editor(id).is_none_or(Editor::busy) {
        return Ok(());
    }
    app.desktop
        .reserve(id, crate::requests::absolute_key(&path))?;
    let task = app
        .desktop
        .editor(id)
        .expect("live editor")
        .requests
        .spawn("dn-save-identity", move |_| {
            crate::requests::destination_key(&path)
                .map(|key| SaveDestination { path, key, save_as })
                .map_err(|e| e.to_string())
        })
        .map_err(|e| {
            app.desktop.release_reservations(id);
            e.to_string()
        })?;
    if let Some(editor) = app.desktop.editor_mut(id) {
        editor.save_prepare = Some(task);
        editor.saving = true;
        editor.notice = "Preparing save…".into();
    }
    Ok(())
}
pub fn poll(app: &mut App) -> bool {
    let id = app.desktop.target();
    let resolving = app
        .desktop
        .document_ids(crate::desktop::WindowKind::Editor)
        .iter()
        .any(|other| app.desktop.identity_pending(*other));
    let preparation = app
        .desktop
        .editor(id)
        .filter(|_| !resolving)
        .and_then(|e| e.save_prepare.as_ref())
        .and_then(|task| match task.try_recv() {
            Ok(value) => Some(value),
            Err(mpsc::TryRecvError::Disconnected) => Some(Err("Save preparation stopped".into())),
            Err(mpsc::TryRecvError::Empty) => None,
        });
    if let Some(preparation) = preparation {
        if let Some(e) = app.desktop.editor_mut(id) {
            e.save_prepare = None;
        }
        let result = preparation.and_then(|SaveDestination { path, key, save_as }| {
            app.desktop.reserve(id, key.clone())?;
            let editor = app
                .desktop
                .editor_mut(id)
                .ok_or("Editor closed".to_string())?;
            editor.save_identity = Some(key);
            editor.save(path, save_as)
        });
        if let Err(error) = result {
            app.desktop.release_reservations(id);
            if let Some(e) = app.desktop.editor_mut(id) {
                e.saving = false;
                e.closing = None;
            }
            app.abort_quit();
            app.error(error);
            return true;
        }
        return true;
    }
    let reply = app
        .desktop
        .active_editor()
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
    if let Some(editor) = app.desktop.active_editor_mut() {
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
                editor.modals.set_editor_modal(None);
                editor.notice = "UTF-8 · F2 Save · F7 Search · F10 Menu · Esc Close".into();
                editor.recolor();
                if let Some(line) = editor.reveal_line.take() {
                    editor.go_to_line(line);
                }
            }
            Reply::Saved(Ok((path, outcome))) => {
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
                editor.original = Some(outcome.snapshot);
                if let Some(buffer) = &mut editor.buffer {
                    if outcome.warning.is_none() {
                        buffer.mark_saved();
                        editor.durability_pending = None;
                    } else {
                        editor.durability_pending = Some(buffer.revision());
                    }
                }
                editor.notice = "Saved".into();
                if let Some(warning) = outcome.warning {
                    editor.closing = None;
                    editor.notice = "Written — durability unconfirmed; retry Save".into();
                    error = Some(format!("{} {warning}\n\n{}", app.config.language.text("File was written, but durability could not be confirmed:"),app.config.language.text("Your buffer is retained and recovery checkpointing continues. Retry Save to confirm it without rewriting unchanged text.")));
                }
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
    if saved {
        let key = app
            .desktop
            .editor_mut(id)
            .and_then(|e| e.save_identity.take());
        if let Some(key) = key {
            app.desktop.saved_path(id, key);
        }
    } else if error.is_some() {
        app.desktop.release_reservations(id);
    }
    if let Some(error) = error {
        app.abort_quit();
        if app
            .desktop
            .editor(id)
            .is_some_and(|editor| editor.buffer.is_none())
        {
            if app.desktop.active().id == id {
                app.retire_document(id);
                let survivor = app.desktop.active().id;
                app.with_document(survivor, |app| app.error(error));
            } else {
                // Retain an inactive failed placeholder so its error is shown only
                // when the user returns to that document.
                if let Some(editor) = app.desktop.editor_mut(id) {
                    editor.buffer = Buffer::new(String::new()).ok();
                }
                app.error(error);
            }
        } else {
            app.error(error);
        }
    } else if let Some(pending) = app
        .desktop
        .active_editor_mut()
        .and_then(|editor| editor.closing.take())
    {
        request_action(app, pending);
    }
    if saved {
        app.refresh();
    }
    true
}
pub fn request_close(app: &mut App, intent: CloseIntent) {
    if intent == CloseIntent::Quit && app.quit_review.is_none() {
        app.request_exit();
        return;
    }
    request_action(
        app,
        match intent {
            CloseIntent::Panel => PendingAction::Close,
            CloseIntent::Quit => PendingAction::Quit,
        },
    );
}
fn request_action(app: &mut App, pending: PendingAction) {
    let Some(editor) = app.desktop.active_editor_mut() else {
        return;
    };
    editor.modals.set_editor_menu(None);
    editor.closing = Some(pending);
    if editor.busy() && !editor.saving {
        editor.worker = None;
    }
    if editor.busy() {
        editor.notice = "Finishing file access before switching…".into();
        return;
    }
    if editor.dirty() {
        editor
            .modals
            .set_editor_modal(Some(Modal::ConfirmClose { focus: 2 }));
        return;
    }
    complete_pending(app);
}
fn complete_pending(app: &mut App) {
    let pending = app.desktop.active_editor_mut().and_then(|editor| {
        if matches!(
            editor.modals.editor_modal(),
            Some(Modal::ConfirmClose { .. })
        ) {
            editor.modals.set_editor_modal(None);
        }
        editor.closing.take()
    });
    match pending {
        Some(PendingAction::Close) => {
            let id = app.desktop.target();
            app.retire_document(id);
        }
        Some(PendingAction::Quit) => {
            app.approve_quit(app.desktop.target());
        }
        None => {}
    }
}
fn file_prompt(
    editor: &mut Editor,
    mode: crate::turbo::file_dialog::Mode,
    language: crate::localization::Language,
    hidden: bool,
) {
    editor.modals.set_editor_modal(Some(Modal::File(Box::new(
        crate::turbo::file_dialog::Picker::new(mode, editor.base.clone(), language, hidden),
    ))));
}
fn finish_file(app: &mut App, outcome: Option<crate::turbo::file_dialog::Outcome>) {
    use crate::turbo::file_dialog::{Mode, Outcome};
    match outcome {
        Some(Outcome::Cancelled) => action(app, EditorAction::Cancel),
        Some(Outcome::Selected(Mode::Open, path)) => {
            if let Some(editor) = app.desktop.active_editor_mut() {
                editor.modals.set_editor_modal(None);
            }
            open_path(app, path);
        }
        Some(Outcome::Selected(Mode::SaveAs, path)) => match prepare_save(app, path, true) {
            Ok(()) => {
                if let Some(editor) = app.desktop.active_editor_mut() {
                    editor.modals.set_editor_modal(None);
                }
            }
            Err(error) => {
                if let Some(Modal::File(picker)) = app
                    .desktop
                    .active_editor_mut()
                    .and_then(|e| e.modals.editor_modal_mut())
                {
                    picker.reset();
                }
                app.error(error);
            }
        },
        None => {}
    }
}
pub(crate) fn file_key(app: &mut App, key: crossterm::event::KeyEvent) {
    if key.code == crossterm::event::KeyCode::F(1) {
        action(app, EditorAction::Help);
        return;
    }
    let clipboard = app.editor_clipboard.clone();
    let Some(Modal::File(picker)) = app
        .desktop
        .active_editor_mut()
        .and_then(|e| e.modals.editor_modal_mut())
    else {
        return;
    };
    use crossterm::event::{KeyCode as K, KeyModifiers as M};
    if (key.code == K::Char('v') && key.modifiers.contains(M::CONTROL))
        || (key.code == K::Insert && key.modifiers.contains(M::SHIFT))
    {
        picker.paste(&clipboard);
        return;
    }
    let outcome = picker.key(key);
    finish_file(app, outcome);
}
pub(crate) fn file_mouse(app: &mut App, event: crossterm::event::MouseEvent) {
    let offset = app
        .layout_key
        .map_or((0, 0), |(_, bounds, _)| (bounds.x, bounds.y));
    let outcome = app
        .desktop
        .active_editor_mut()
        .and_then(|e| e.modals.editor_modal_mut())
        .and_then(|modal| match modal {
            Modal::File(picker) => picker.mouse(event, offset),
            _ => None,
        });
    finish_file(app, outcome);
}
fn prompt(editor: &mut Editor, kind: PromptKind) {
    let field = if kind == PromptKind::TabWidth {
        editor.preferences.tab_size.to_string()
    } else {
        String::new()
    };
    let cursor = field.len();
    editor.modals.set_editor_modal(Some(Modal::Prompt {
        kind,
        field,
        cursor,
        focus: 0,
    }));
}
pub fn paste(app: &mut App, text: &str) {
    let Some(editor) = app.desktop.active_editor_mut() else {
        return;
    };
    if editor.busy() {
        return;
    }
    match editor.modals.editor_modal_mut() {
        Some(Modal::File(picker)) => {
            picker.paste(text);
            return;
        }
        Some(Modal::Prompt {
            field,
            cursor,
            focus: 0,
            ..
        }) => {
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
    let Some(editor) = app.desktop.active_editor_mut() else {
        return Ok(());
    };
    let Some(Modal::Prompt { kind, field, .. }) = editor.modals.take_editor_modal() else {
        return Ok(());
    };
    match kind {
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
    }
    Ok(())
}
pub fn action(app: &mut App, command: EditorAction) {
    if matches!(command, EditorAction::WindowMove | EditorAction::WindowZoom) {
        crate::windowing::move_active(app, command == EditorAction::WindowZoom);
        return;
    }
    app.sync_modals();
    use EditorAction as A;
    if command == A::Submit
        && matches!(
            app.desktop
                .active_editor()
                .and_then(|e| e.modals.editor_modal()),
            Some(Modal::File(_))
        )
    {
        let outcome = match app
            .desktop
            .active_editor_mut()
            .unwrap()
            .modals
            .editor_modal_mut()
        {
            Some(Modal::File(picker)) => picker.submit(),
            _ => None,
        };
        finish_file(app, outcome);
        return;
    }
    if command == A::Cancel && app.native_help().is_some() {
        app.pop_dialog();
        return;
    }
    if command == A::Help {
        app.push_dialog(crate::dialog::Dialog {
            title: " Editor Help ",
            message: app
                .config
                .language
                .text(if app.config.shortcuts.is_mac() {
                    crate::shortcuts::EDITOR_HELP
                } else {
                    crate::commands::help::CLASSIC_EDITOR
                })
                .into(),
        });
        return;
    }
    if command == A::Windows {
        crate::windowing::open(app);
        return;
    }
    if command == A::Quit {
        app.request_exit();
        return;
    }
    if command == A::Close {
        request_action(app, PendingAction::Close);
        return;
    }
    if command == A::Save {
        if let Some(path) = app.desktop.active_editor().and_then(|e| e.path.clone()) {
            if let Some(e) = app.desktop.active_editor_mut() {
                e.modals.set_editor_modal(None);
            }
            if let Err(error) = prepare_save(app, path, false) {
                app.error(error);
            }
            return;
        }
    }
    if app.desktop.active_editor().is_none_or(Editor::busy) {
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
        menus::open_root(app, menus::EditorRootId::File);
        return;
    }
    if !matches!(command, A::MenuSurface | A::SearchChoice(_)) {
        if let Some(editor) = app.desktop.active_editor_mut() {
            editor.modals.set_editor_menu(None);
        }
    }
    let previous_preferences = app.config.editor;
    let result = match command {
        A::Submit => submit_prompt(app),
        A::SearchSubmit(all) => search_controls::submit(app, all),
        A::Discard
            if app.desktop.active_editor().is_some_and(|e| {
                matches!(e.modals.editor_modal(), Some(Modal::ConfirmClose { .. }))
            }) =>
        {
            complete_pending(app);
            Ok(())
        }
        A::SaveDesktop => {
            app.save_config();
            let no_errors = app.dialogs().is_empty();
            if let Some(editor) = app.desktop.active_editor_mut() {
                editor.notice = if no_errors {
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
    if previous_preferences != app.config.editor {
        for id in app.desktop.document_ids(crate::desktop::WindowKind::Editor) {
            if let Some(editor) = app.desktop.editor_mut(id) {
                editor.apply_preferences(app.config.editor);
            }
        }
    }
    if let Some(editor) = app.desktop.active_editor_mut() {
        editor.recolor();
        editor.keep_visible();
    }
    if let Err(error) = result {
        app.error(error);
    }
}
fn apply(app: &mut App, command: EditorAction) -> Result<(), String> {
    use EditorAction as A;
    let Some(editor) = app.desktop.active_editor_mut() else {
        return Ok(());
    };
    if matches!(command, A::ReplaceYes | A::ReplaceNo | A::ReplaceAll)
        || (command == A::Cancel && matches!(editor.modals.editor_modal(), Some(Modal::Replace(_))))
    {
        return search_controls::decision(editor, command);
    }
    match command {
        A::PromptField => {
            if let Some(Modal::Prompt { focus, .. }) = editor.modals.editor_modal_mut() {
                *focus = 0;
            }
        }
        A::CancelMenu => editor.modals.set_editor_menu(None),
        A::Cancel => {
            let quit = matches!(
                editor.modals.editor_modal(),
                Some(Modal::ConfirmClose { .. })
            ) || (editor.closing == Some(PendingAction::Quit)
                && matches!(editor.modals.editor_modal(), Some(Modal::File(_))));
            editor.modals.set_editor_modal(None);
            if quit || editor.modals.editor_modal().is_none() {
                editor.closing = None;
            }
            if quit {
                app.abort_quit();
            }
        }
        A::Open => file_prompt(
            editor,
            crate::turbo::file_dialog::Mode::Open,
            app.config.language,
            app.config.show_hidden,
        ),
        A::Save => {
            editor.modals.set_editor_modal(None);
            if let Some(path) = editor.path.clone() {
                editor.save(path, false)?;
            } else {
                file_prompt(
                    editor,
                    crate::turbo::file_dialog::Mode::SaveAs,
                    app.config.language,
                    app.config.show_hidden,
                );
            }
        }
        A::SaveAs => file_prompt(
            editor,
            crate::turbo::file_dialog::Mode::SaveAs,
            app.config.language,
            app.config.show_hidden,
        ),
        A::GoTo => prompt(editor, PromptKind::GoTo),
        A::TabWidth => prompt(editor, PromptKind::TabWidth),
        A::Find | A::Replace => search_controls::open(editor, command == A::Replace),
        A::FindNext | A::FindPrevious => {
            search_controls::repeat(editor, command == A::FindPrevious)?
        }
        A::SearchChoice(choice) => {
            if let Some(Modal::Search(dialog)) = editor.modals.editor_modal_mut() {
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
        A::Help => editor.modals.set_editor_modal(Some(Modal::Help)),
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
                    A::Indent | A::Unindent => buffer
                        .indent_block(command == A::Unindent, editor.preferences.expand_tabs)?,
                    A::Uppercase | A::Lowercase | A::Capitalize => {
                        buffer.change_case(match command {
                            A::Uppercase => Case::Upper,
                            A::Lowercase => Case::Lower,
                            _ => Case::Capitalize,
                        })?
                    }
                    A::SortLines => buffer.sort_lines()?,
                    A::MatchBracket => {
                        editor.highlight.refresh(buffer);
                        let pos = buffer
                            .matching_bracket(&editor.highlight)
                            .ok_or("No matching bracket at the cursor.")?;
                        buffer.move_to(pos, false);
                    }
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

#[cfg(test)]
mod save_outcome_tests {
    use super::*;
    #[test]
    fn published_but_uncertain_save_keeps_dirty_state_aborts_quit_and_confirms_without_replacement(
    ) -> std::io::Result<()> {
        let root = std::env::temp_dir().join(format!("dn-editor-uncertain-{}", std::process::id()));
        std::fs::create_dir(&root)?;
        struct Temp(PathBuf);
        impl Drop for Temp {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _temp = Temp(root.clone());
        let path = root.join("source");
        std::fs::write(&path, "old")?;
        let mut app = App::new(root)?;
        app.config.save_on_exit = false;
        open_path(&mut app, path.clone());
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while app.desktop.active_editor().is_none_or(|e| e.busy())
            || app.desktop.identity_pending(app.desktop.target())
        {
            app.poll();
            assert!(std::time::Instant::now() < until);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        paste(&mut app, "new");
        let e = app.desktop.active_editor_mut().unwrap();
        let text = e.buffer.as_ref().unwrap().text.clone();
        let mut outcome = storage::save(&path, &text, false, e.original.as_ref()).unwrap();
        outcome.warning = Some("injected sync failure".into());
        #[cfg(unix)]
        let ino = {
            use std::os::unix::fs::MetadataExt;
            std::fs::metadata(&path)?.ino()
        };
        e.closing = Some(PendingAction::Quit);
        e.saving = true;
        let dest = path.clone();
        e.worker = Some(
            e.requests
                .spawn("dn-editor-save", move |_| Reply::Saved(Ok((dest, outcome))))?,
        );
        while app.desktop.active_editor().unwrap().busy() {
            app.poll();
            assert!(std::time::Instant::now() < until);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(!app.quit);
        let e = app.desktop.active_editor().unwrap();
        assert!(e.dirty());
        assert!(e.durability_pending.is_some());
        assert!(e.closing.is_none());
        assert_eq!(e.buffer.as_ref().unwrap().text, text);
        app.clear_dialogs();
        action(&mut app, EditorAction::Save);
        while app.desktop.active_editor().unwrap().busy() {
            app.poll();
            assert!(std::time::Instant::now() < until);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(!app.desktop.active_editor().unwrap().dirty());
        assert!(app.dialogs().is_empty());
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(std::fs::metadata(&path)?.ino(), ino);
        }
        Ok(())
    }
}
