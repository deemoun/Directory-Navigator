use super::{
    search::{self, ReplaceSession, SearchDialog, SearchMemory},
    *,
};
use crate::{app::App, dialog};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub(super) fn open(editor: &mut Editor, replacing: bool) {
    let selection = editor.buffer.as_ref().map(Buffer::selected).unwrap_or(0..0);
    editor.modal = Some(Modal::Search(SearchDialog::new(
        replacing,
        &SearchMemory {
            query: editor.search.clone(),
            replacement: editor.replacement.clone(),
            options: editor.search_options,
        },
        selection,
    )));
}
fn scope(editor: &Editor) -> Result<std::ops::Range<usize>, String> {
    let Some(buffer) = &editor.buffer else {
        return Err("No document loaded.".into());
    };
    if editor.search_options.selected_scope {
        editor
            .search_scope
            .clone()
            .filter(|range| {
                range.end <= buffer.text.len() && editor.search_revision == buffer.revision()
            })
            .ok_or("Selection changed; reopen Find/Replace.".into())
    } else {
        Ok(0..buffer.text.len())
    }
}
pub(super) fn repeat(editor: &mut Editor, reverse: bool) -> Result<(), String> {
    if editor.search.is_empty() {
        open(editor, false);
        return Ok(());
    }
    let scope = scope(editor)?;
    if let Some(buffer) = &mut editor.buffer {
        let backwards = editor.search_options.backward ^ reverse;
        let origin = if backwards {
            buffer.selected().start
        } else {
            buffer.selected().end
        };
        let mut options = editor.search_options;
        options.backward = backwards;
        let matches = search::matches(&buffer.text, &editor.search, scope, options)?;
        if let Some(range) = search::ordered(matches, backwards, origin)
            .into_iter()
            .next()
        {
            search::select(buffer, range);
            editor.notice = "Match".into();
        } else {
            editor.notice = "No match".into();
        }
    }
    Ok(())
}
pub(super) fn submit(app: &mut App, all: bool) -> Result<(), String> {
    let Some(editor) = app.desktop.editor_mut() else {
        return Ok(());
    };
    let Some(Modal::Search(dialog)) = editor.modal.take() else {
        return Ok(());
    };
    if dialog.query.is_empty() {
        editor.modal = Some(Modal::Search(dialog));
        return Err("Enter text to find.".into());
    }
    editor.search = dialog.query;
    editor.replacement = dialog.replacement;
    editor.search_options = dialog.options;
    editor.search_scope = dialog.selection;
    app.editor_search = SearchMemory {
        query: editor.search.clone(),
        replacement: editor.replacement.clone(),
        options: editor.search_options,
    };
    if let Some(buffer) = &editor.buffer {
        editor.search_revision = buffer.revision();
    }
    let scope = scope(editor)?;
    if let Some(buffer) = &mut editor.buffer {
        let options = editor.search_options;
        let origin = if options.entire_scope {
            if options.backward {
                scope.end
            } else {
                scope.start
            }
        } else {
            buffer.cursor
        };
        let ranges = search::ordered(
            search::matches(&buffer.text, &editor.search, scope, options)?,
            options.backward,
            origin,
        );
        if ranges.is_empty() {
            editor.notice = "No match".into();
            return Ok(());
        }
        if !dialog.replacing {
            search::select(buffer, ranges[0].clone());
            editor.notice = "Match".into();
        } else {
            let replacement = editor
                .replacement
                .replace("\r\n", "\n")
                .replace('\r', "\n")
                .replace('\n', buffer.newline);
            let mut session = ReplaceSession::new(ranges, replacement);
            if all || !options.prompt {
                session.all(buffer)?;
                editor.notice = format!("Replaced {} occurrence(s)", session.changed);
            } else {
                if let Some(range) = session.current() {
                    search::select(buffer, range);
                }
                editor.modal = Some(Modal::Replace(session));
            }
        }
    }
    editor.recolor();
    editor.keep_visible();
    Ok(())
}
pub(super) fn decision(editor: &mut Editor, command: EditorAction) -> Result<(), String> {
    let Some(Modal::Replace(mut session)) = editor.modal.take() else {
        return Ok(());
    };
    if let Some(buffer) = &mut editor.buffer {
        let result = match command {
            EditorAction::ReplaceYes => session.yes(buffer),
            EditorAction::ReplaceAll => session.all(buffer),
            EditorAction::ReplaceNo => {
                session.skip();
                Ok(())
            }
            _ => Ok(()),
        };
        if let Err(error) = result {
            editor.modal = Some(Modal::Replace(session));
            return Err(error);
        }
        editor.notice = format!("Replaced {} occurrence(s)", session.changed);
        if command != EditorAction::Cancel {
            if let Some(range) = session.current() {
                search::select(buffer, range);
                editor.modal = Some(Modal::Replace(session));
            }
        }
    }
    editor.recolor();
    editor.keep_visible();
    Ok(())
}
pub(super) fn key(app: &mut App, key: KeyEvent) {
    let Some(editor) = app.desktop.editor_mut() else {
        return;
    };
    let command = match &mut editor.modal {
        Some(Modal::Search(dialog)) => match key.code {
            KeyCode::Char(c) if key.modifiers.contains(KeyModifiers::ALT) => {
                match c.to_ascii_lowercase() {
                    't' => dialog.focus = 0,
                    'n' if dialog.replacing => dialog.focus = 1,
                    'c' => dialog.options.case_sensitive = !dialog.options.case_sensitive,
                    'w' => dialog.options.whole_words = !dialog.options.whole_words,
                    'p' if dialog.replacing => dialog.options.prompt = !dialog.options.prompt,
                    'b' => dialog.options.backward = true,
                    'd' => dialog.options.backward = false,
                    's' if dialog.selection.is_some() => dialog.options.selected_scope = true,
                    'g' => dialog.options.selected_scope = false,
                    'e' => dialog.options.entire_scope = true,
                    'f' => dialog.options.entire_scope = false,
                    _ => {}
                };
                None
            }
            KeyCode::Char('a')
                if key.modifiers.contains(KeyModifiers::CONTROL) && dialog.focus < 2 =>
            {
                if dialog.focus == 0 {
                    dialog.query.clear();
                } else {
                    dialog.replacement.clear();
                }
                dialog.cursors[dialog.focus] = 0;
                None
            }
            KeyCode::Esc => Some(EditorAction::Cancel),
            KeyCode::Tab => {
                dialog.tab(false);
                None
            }
            KeyCode::BackTab => {
                dialog.tab(true);
                None
            }
            KeyCode::Enter => Some(match dialog.focus {
                9 => EditorAction::SearchSubmit(true),
                10 => EditorAction::Cancel,
                _ => EditorAction::SearchSubmit(false),
            }),
            KeyCode::Char(' ') if dialog.focus >= 2 => match dialog.focus {
                8 => Some(EditorAction::SearchSubmit(false)),
                9 => Some(EditorAction::SearchSubmit(true)),
                10 => Some(EditorAction::Cancel),
                choice => {
                    dialog.toggle(choice);
                    None
                }
            },
            _ if dialog.focus < 2
                && !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                let field = if dialog.focus == 0 {
                    &mut dialog.query
                } else {
                    &mut dialog.replacement
                };
                dialog::edit_input(field, &mut dialog.cursors[dialog.focus], key.code);
                None
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down => {
                dialog.toggle(dialog.focus);
                None
            }
            _ => None,
        },
        Some(Modal::Replace(_)) => match key.code {
            KeyCode::Char('y' | 'Y') => Some(EditorAction::ReplaceYes),
            KeyCode::Char('n' | 'N') => Some(EditorAction::ReplaceNo),
            KeyCode::Char('a' | 'A') => Some(EditorAction::ReplaceAll),
            KeyCode::Esc | KeyCode::Char('c' | 'C') => Some(EditorAction::Cancel),
            _ => None,
        },
        _ => None,
    };
    if let Some(command) = command {
        super::action(app, command);
    }
}
