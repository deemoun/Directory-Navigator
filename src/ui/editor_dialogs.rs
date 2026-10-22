use crate::{
    app::App,
    editor::{EditorAction as A, Modal, PromptKind},
    filesystem::display_name,
    formatting,
    mouse::Target,
};
use ratatui::{layout::Rect, text::Span, widgets::Paragraph, Frame};
fn button(frame: &mut Frame, app: &mut App, rect: Rect, label: &str, command: A, focused: bool) {
    super::dialog_style::button(
        frame,
        app.config.palette,
        rect,
        app.config.language.text(label),
        focused,
    );
    app.mouse.regions.push((rect, Target::Editor(command)));
}
fn input(
    frame: &mut Frame,
    profile: crate::palette::PaletteProfile,
    rect: Rect,
    field: &str,
    cursor: usize,
    focused: bool,
) {
    let prefix = formatting::tail(&field[..cursor], rect.width.saturating_sub(1));
    let col = Span::raw(&prefix).width() as u16;
    frame.render_widget(
        Paragraph::new(format!("{prefix}{}", &field[cursor..]))
            .style(super::dialog_style::input_style(profile, focused)),
        rect,
    );
    if focused {
        frame.set_cursor_position((rect.x + col, rect.y));
    }
}
pub(super) fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    let profile = app.config.palette;
    let dos = profile.styles();
    let Some(editor) = &app.editor else {
        return;
    };
    let Some(modal) = &editor.modal else {
        return;
    };
    let title = match modal {
        Modal::ConfirmClose => " Unsaved changes ",
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
            PromptKind::Open => " Open file ",
            PromptKind::SaveAs => " Save As — new destination ",
            PromptKind::GoTo => " Go to line ",
            PromptKind::TabWidth => " Tab width ",
        },
    };
    let (width, height) = match modal {
        Modal::Prompt { .. } => (60, 7),
        Modal::ConfirmClose => (66, 10),
        Modal::Replace(_) => (72, 15),
        _ => (72, 20),
    };
    let rect = super::dialog_style::window(
        frame,
        profile,
        area,
        width,
        height,
        app.config.language.text(title),
    );
    app.mouse.regions.clear();
    super::dialog_style::close_button(
        frame,
        profile,
        rect,
        &mut app.mouse.regions,
        Target::Editor(A::Cancel),
    );
    match modal {
        Modal::Search(dialog) => {
            let replacing = dialog.replacing;
            let focus = dialog.focus;
            let selection = dialog.selection.is_some();
            let opts = dialog.options;
            for (row, label, field, cursor, choice) in [
                (2, "Text to find:", &dialog.query, dialog.cursors[0], 0),
                (4, "New text:", &dialog.replacement, dialog.cursors[1], 1),
            ] {
                if choice == 1 && !replacing {
                    continue;
                }
                frame.render_widget(
                    Paragraph::new(app.config.language.text(label)).style(dos(0x70)),
                    Rect::new(rect.x + 3, rect.y + row, 14, 1),
                );
                let field_rect = Rect::new(rect.x + 18, rect.y + row, 50, 1);
                input(
                    frame,
                    profile,
                    field_rect,
                    field,
                    cursor,
                    focus == choice && app.dialogs.is_empty(),
                );
                app.mouse
                    .regions
                    .push((field_rect, Target::Editor(A::SearchChoice(choice))));
            }
            let checks = [
                (
                    2,
                    7,
                    3,
                    format!(
                        "[{}] {}",
                        if opts.case_sensitive { 'x' } else { ' ' },
                        app.config.language.text("Case sensitive")
                    ),
                    true,
                ),
                (
                    3,
                    8,
                    3,
                    format!(
                        "[{}] {}",
                        if opts.whole_words { 'x' } else { ' ' },
                        app.config.language.text("Whole words only")
                    ),
                    true,
                ),
                (
                    4,
                    9,
                    3,
                    format!(
                        "[{}] {}",
                        if opts.prompt { 'x' } else { ' ' },
                        app.config.language.text("Prompt on replace")
                    ),
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
                frame.render_widget(
                    Paragraph::new(label).style(dos(if !enabled {
                        0x78
                    } else if focus == choice {
                        0x30
                    } else {
                        0x70
                    })),
                    row,
                );
                if enabled {
                    app.mouse
                        .regions
                        .push((row, Target::Editor(A::SearchChoice(choice))));
                }
            }
            frame.render_widget(Paragraph::new(app.config.language.text("Tab chooses field/option · Space toggles · Enter OK\nAlt+C/W: options · Alt+B/D: direction · Esc Cancel")).style(dos(0x70)), Rect::new(rect.x + 3, rect.y + 14, 66, 2));
            button(
                frame,
                app,
                Rect::new(rect.x + 6, rect.bottom() - 3, 15, 1),
                " [ OK ] ",
                A::SearchSubmit(false),
                focus == 8,
            );
            if replacing {
                button(
                    frame,
                    app,
                    Rect::new(rect.x + 26, rect.bottom() - 3, 18, 1),
                    " [ Change all ] ",
                    A::SearchSubmit(true),
                    focus == 9,
                );
            }
            button(
                frame,
                app,
                Rect::new(rect.x + 50, rect.bottom() - 3, 17, 1),
                " [ Cancel ] ",
                A::Cancel,
                focus == 10,
            );
        }
        Modal::Replace(session) => {
            let current = session
                .current()
                .and_then(|range| editor.buffer.as_ref().map(|b| display_name(&b.text[range])))
                .unwrap_or_default();
            let text = format!("Replace highlighted occurrence?\n\nCurrent: {}\nNew: {}\n\n{} replaced · {} remaining\n\nY Yes · N No · A All · Esc Cancel\nCancel keeps replacements already accepted.",
                formatting::tail(&current, 55), formatting::tail(&session.replacement, 55), session.changed, session.remaining.len());
            frame.render_widget(
                Paragraph::new(text).style(dos(0x70)),
                Rect::new(rect.x + 3, rect.y + 2, 66, 9),
            );
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
                    frame,
                    app,
                    Rect::new(rect.x + 4 + i as u16 * 17, rect.bottom() - 3, 15, 1),
                    label,
                    command,
                    false,
                );
            }
        }
        Modal::Help => {
            frame.render_widget(Paragraph::new(app.config.language.text(if app.config.shortcuts.is_mac() { crate::shortcuts::EDITOR_HELP } else { "F2 / Ctrl+S Save · Shift+F2 Save As · F3 Open\nF6 Duplicate line · F7 / Ctrl+F Find\nShift+F7 Search again · Alt+F7 Reverse search\nCtrl+F7 Replace · Alt+G / Ctrl+G Go to line\nCtrl+←/→ words · Ctrl+Shift+←/→ select words\nShift+arrows select · Ctrl+A select all\nAlt+Backspace / Ctrl+Z Undo · Ctrl+Y Redo\nCtrl+Ins/C Copy · Shift+Del / Ctrl+X Cut\nShift+Ins / Ctrl+V Paste · terminal paste supported\nAlt+B Block: selection/clipboard · Options: indent/colors/tabs\nF10 Menu · Ctrl+F4 / Esc Close · Alt+X Quit DN\nOpen/Close/Quit protect unsaved changes.\nUTF-8, 2 MiB / 100k lines. Line endings preserved.\nSave As never overwrites an existing destination." }))
                .style(dos(0x70)), Rect::new(rect.x + 2, rect.y + 2, 68, 15));
            button(
                frame,
                app,
                Rect::new(rect.x + 28, rect.bottom() - 2, 16, 1),
                " Enter / Esc OK ",
                A::Cancel,
                false,
            );
        }
        Modal::ConfirmClose => {
            frame.render_widget(Paragraph::new(app.config.language.text("This document has unsaved changes.\n\nSave before closing or switching files?\n\nS: Save    D: Discard    C / Esc / Enter: Cancel"))
                .style(dos(0x70)), Rect::new(rect.x + 3, rect.y + 2, rect.width-6, 5));
            for (i, (label, command)) in [
                (" [ Save ] ", A::Save),
                (" [ Discard ] ", A::Discard),
                (" [ Cancel ] ", A::Cancel),
            ]
            .into_iter()
            .enumerate()
            {
                button(
                    frame,
                    app,
                    Rect::new(rect.x + 4 + i as u16 * 20, rect.bottom() - 3, 14, 1),
                    label,
                    command,
                    false,
                );
            }
        }
        Modal::Prompt {
            kind,
            field,
            cursor,
        } => {
            let label = match kind {
                PromptKind::SaveAs => "New file name (existing files are refused)",
                PromptKind::Open => "File name (absolute or relative path)",
                PromptKind::GoTo => "Line number (starting at 1)",
                PromptKind::TabWidth => "Tab columns (1–16); Ctrl+A clears",
            };
            frame.render_widget(
                Paragraph::new(app.config.language.text(label)).style(dos(0x70)),
                Rect::new(rect.x + 2, rect.y + 1, rect.width - 4, 1),
            );
            input(
                frame,
                profile,
                Rect::new(rect.x + 2, rect.y + 2, rect.width - 4, 1),
                field,
                *cursor,
                app.dialogs.is_empty(),
            );
            button(
                frame,
                app,
                Rect::new(rect.right() - 29, rect.bottom() - 3, 12, 1),
                " [ OK ] ",
                A::Submit,
                false,
            );
            button(
                frame,
                app,
                Rect::new(rect.right() - 15, rect.bottom() - 3, 12, 1),
                " [ Cancel ] ",
                A::Cancel,
                false,
            );
        }
    }
}
