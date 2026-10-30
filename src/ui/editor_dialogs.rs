use crate::dialog_theme as theme;
use crate::painting::{Frame, Paragraph};
use crate::presentation::{layout::Rect, text::Span};
use crate::turbo::dialog_spec::{centered, scope, Control, ControlKind, DialogSpec};
use crate::{
    app::App,
    editor::{EditorAction as A, Modal, PromptKind},
    filesystem::display_name,
    formatting,
    mouse::Target,
};
fn button(spec: &mut DialogSpec<'_>, app: &App, rect: Rect, label: &str, command: A, id: usize) {
    spec.control(Control::new(
        id,
        rect,
        ControlKind::Button(app.config.language.text(label).into()),
        Target::Editor(command),
    ));
}
fn input(
    spec: &mut DialogSpec<'_>,
    rect: Rect,
    field: &str,
    cursor: usize,
    id: usize,
    action: A,
    visible: bool,
) {
    let prefix = formatting::tail(&field[..cursor], rect.width.saturating_sub(1));
    let col = Span::raw(&prefix).width() as u16;
    let mut control = Control::new(
        id,
        rect,
        ControlKind::Input(format!("{prefix}{}", &field[cursor..])),
        Target::Editor(action),
    );
    control.focus_visible = visible;
    spec.control(control);
    if spec.focus == id && visible {
        spec.cursor = Some((rect.x + col, rect.y));
    }
}
pub(super) fn draw(frame: &mut impl Frame, app: &App, area: Rect) {
    let Some(editor) = app.desktop.active_editor() else {
        return;
    };
    let Some(modal) = editor.modals.editor_modal() else {
        return;
    };
    if let Modal::File(picker) = modal {
        frame.regions().clear();
        if let Some(scene) = picker.scene() {
            let mut scene = scene.clone();
            if app
                .modal_top()
                .is_none_or(|m| m.focus != crate::focus::Focus::EditorModal)
            {
                scene.cursor = None;
            }
            frame.compose(scene);
            frame.native_shadow(picker.bounds());
        }
        return;
    }
    if let Some(spec) = editor_spec(app, area) {
        spec.paint(frame, app);
    }
}
pub(crate) fn editor_spec(app: &App, area: Rect) -> Option<DialogSpec<'_>> {
    let dos = app
        .config
        .palette
        .styles_for(crate::palette::theme::Group::Dialogs);
    let editor = app.desktop.active_editor()?;
    let modal = editor.modals.editor_modal()?;
    if matches!(modal, Modal::File(_)) {
        return None;
    }
    let title = match modal {
        Modal::File(_) => unreachable!(),
        Modal::ConfirmClose { .. } => " Unsaved changes ",
        Modal::Help => " Editor Help ",
        Modal::Search(dialog) => {
            if dialog.replacing {
                " Replace "
            } else {
                " Find "
            }
        }
        Modal::Replace(_) => " Confirm replacement ",
        Modal::Prompt { kind, .. } => match kind {
            PromptKind::GoTo => " Go to line ",
            PromptKind::TabWidth => " Tab width ",
        },
    };
    let (width, height) = match modal {
        Modal::Prompt { .. } => (60, 7),
        Modal::ConfirmClose { .. } => (66, 10),
        Modal::Replace(_) => (72, 15),
        _ => (72, 20),
    };
    let (focus, default) = match modal {
        Modal::Search(d) => (d.focus, A::SearchSubmit(false)),
        Modal::Replace(s) => (s.focus, A::Cancel),
        Modal::ConfirmClose { focus } => (*focus, A::Cancel),
        Modal::Prompt { focus, .. } => (*focus, A::Submit),
        _ => (0, A::Cancel),
    };
    let rect = centered(area, width, height);
    let mut spec = DialogSpec::new(
        rect,
        scope(app, crate::focus::Focus::EditorModal)?,
        focus,
        app.config.language.text(title),
        Target::Editor(default),
        Target::Editor(A::Cancel),
    );
    match modal {
        Modal::File(_) => unreachable!(),
        Modal::Search(dialog) => {
            let replacing = dialog.replacing;
            let selection = dialog.selection.is_some();
            let opts = dialog.options;
            for (row, label, field, cursor, choice) in [
                (2, "Text to find:", &dialog.query, dialog.cursors[0], 0),
                (4, "New text:", &dialog.replacement, dialog.cursors[1], 1),
            ] {
                if choice == 1 && !replacing {
                    continue;
                }
                spec.caption(
                    Paragraph::new(app.config.language.text(label)).style(dos(theme::TEXT)),
                    Rect::new(rect.x + 3, rect.y + row, 14, 1),
                );
                let field_rect = Rect::new(rect.x + 18, rect.y + row, 50, 1);
                input(
                    &mut spec,
                    field_rect,
                    field,
                    cursor,
                    choice,
                    A::SearchChoice(choice),
                    app.dialogs().is_empty(),
                );
            }
            let checks = [
                (
                    2,
                    7,
                    3,
                    app.config.language.text("Case sensitive").to_owned(),
                    true,
                ),
                (
                    3,
                    8,
                    3,
                    app.config.language.text("Whole words only").to_owned(),
                    true,
                ),
                (
                    4,
                    9,
                    3,
                    app.config.language.text("Prompt on replace").to_owned(),
                    replacing,
                ),
                (
                    5,
                    7,
                    38,
                    format!(
                        "{}: {}",
                        app.config.language.text("Direction"),
                        app.config.language.text(if opts.backward {
                            "Backward"
                        } else {
                            "Forward"
                        })
                    ),
                    true,
                ),
                (
                    6,
                    10,
                    38,
                    format!(
                        "{}: {}",
                        app.config.language.text("Scope"),
                        app.config.language.text(if opts.selected_scope {
                            "Selected text"
                        } else {
                            "Global"
                        })
                    ),
                    selection,
                ),
                (
                    7,
                    12,
                    38,
                    format!(
                        "{}: {}",
                        app.config.language.text("Origin"),
                        app.config.language.text(if opts.entire_scope {
                            "Entire scope"
                        } else {
                            "From cursor"
                        })
                    ),
                    true,
                ),
            ];
            for (choice, y, x, label, enabled) in checks {
                if choice == 4 && !replacing {
                    continue;
                }
                let row = Rect::new(rect.x + x, rect.y + y, 29, 1);
                let checked = match choice {
                    2 => opts.case_sensitive,
                    3 => opts.whole_words,
                    4 => opts.prompt,
                    5 => opts.backward,
                    6 => opts.selected_scope,
                    7 => opts.entire_scope,
                    _ => false,
                };
                let mut control = Control::new(
                    choice,
                    row,
                    ControlKind::Checkbox {
                        caption: label,
                        checked,
                    },
                    Target::Editor(A::SearchChoice(choice)),
                );
                control.enabled = enabled;
                spec.control(control);
            }
            spec.caption(Paragraph::new(app.config.language.text("Tab chooses field/option · Space toggles · Enter OK\nAlt+C/W: options · Alt+B/D: direction · Esc Cancel")).style(dos(theme::TEXT)), Rect::new(rect.x + 3, rect.y + 14, 66, 2));
            button(
                &mut spec,
                app,
                Rect::new(rect.x + 6, rect.bottom() - 3, 15, 1),
                " [ OK ] ",
                A::SearchSubmit(false),
                8,
            );
            if replacing {
                button(
                    &mut spec,
                    app,
                    Rect::new(rect.x + 26, rect.bottom() - 3, 18, 1),
                    " [ Change all ] ",
                    A::SearchSubmit(true),
                    9,
                );
            }
            button(
                &mut spec,
                app,
                Rect::new(rect.x + 50, rect.bottom() - 3, 17, 1),
                " [ Cancel ] ",
                A::Cancel,
                10,
            );
        }
        Modal::Replace(session) => {
            let current = session
                .current()
                .and_then(|range| editor.buffer.as_ref().map(|b| display_name(&b.text[range])))
                .unwrap_or_default();
            let text = format!("Replace highlighted occurrence?\n\nCurrent: {}\nNew: {}\n\n{} replaced · {} remaining\n\nY Yes · N No · A All · Esc Cancel\nCancel keeps replacements already accepted.",
                formatting::tail(&current, 55), formatting::tail(&session.replacement, 55), session.changed, session.remaining.len());
            spec.memo(&text, Rect::new(rect.x + 3, rect.y + 2, 66, 9));
            for (i, (label, command)) in [
                (" [ Yes ] ", A::ReplaceYes),
                (" [ No ] ", A::ReplaceNo),
                (" [ All ] ", A::ReplaceAll),
                (" [ Cancel ] ", A::Cancel),
            ]
            .into_iter()
            .enumerate()
            {
                button(
                    &mut spec,
                    app,
                    Rect::new(rect.x + 4 + i as u16 * 17, rect.bottom() - 3, 15, 1),
                    label,
                    command,
                    i,
                );
            }
        }
        Modal::Help => {
            spec.memo(
                app.config.language.text(if app.config.shortcuts.is_mac() {
                    crate::shortcuts::EDITOR_HELP
                } else {
                    crate::commands::help::CLASSIC_EDITOR
                }),
                Rect::new(rect.x + 2, rect.y + 2, 68, 15),
            );
            button(
                &mut spec,
                app,
                Rect::new(rect.x + 28, rect.bottom() - 2, 16, 1),
                " Enter / Esc OK ",
                A::Cancel,
                0,
            );
        }
        Modal::ConfirmClose { .. } => {
            spec.memo(app.config.language.text("This document has unsaved changes.\nSave before closing or switching files?\n\nS: Save · D: Discard · Esc: Cancel\nTab / Shift+Tab chooses · Enter activates"), Rect::new(rect.x + 3, rect.y + 2, rect.width-6, 5));
            for (i, (label, command)) in [
                (" [ Save ] ", A::Save),
                (" [ Discard ] ", A::Discard),
                (" [ Cancel ] ", A::Cancel),
            ]
            .into_iter()
            .enumerate()
            {
                button(
                    &mut spec,
                    app,
                    Rect::new(rect.x + 4 + i as u16 * 20, rect.bottom() - 3, 14, 1),
                    label,
                    command,
                    i,
                );
            }
        }
        Modal::Prompt {
            kind,
            field,
            cursor,
            ..
        } => {
            let label = match kind {
                PromptKind::GoTo => "Line number (starting at 1)",
                PromptKind::TabWidth => "Tab columns (1–16); Ctrl+A clears",
            };
            spec.caption(
                Paragraph::new(app.config.language.text(label)).style(dos(theme::TEXT)),
                Rect::new(rect.x + 2, rect.y + 1, rect.width - 4, 1),
            );
            input(
                &mut spec,
                Rect::new(rect.x + 2, rect.y + 2, rect.width - 4, 1),
                field,
                *cursor,
                0,
                A::PromptField,
                app.dialogs().is_empty(),
            );
            button(
                &mut spec,
                app,
                Rect::new(rect.right() - 29, rect.bottom() - 3, 12, 1),
                " [ OK ] ",
                A::Submit,
                1,
            );
            button(
                &mut spec,
                app,
                Rect::new(rect.right() - 15, rect.bottom() - 3, 12, 1),
                " [ Cancel ] ",
                A::Cancel,
                2,
            );
        }
    }
    Some(spec)
}
