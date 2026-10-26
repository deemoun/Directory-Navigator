use super::dialog_style;
use crate::painting::{Frame, Paragraph};
use crate::presentation::{
    layout::Rect,
    text::{Line, Span},
};
use crate::{
    app::App, dialog::PromptKind, filesystem::display_name, formatting, mouse::Target,
    operations::Kind,
};

pub(super) fn draw_info_dialog(
    frame: &mut impl Frame,
    app: &App,
    message: Option<&crate::layout::MessageLayout>,
) {
    if let Some(help) = app.modals.native_help(app.desktop.active().id) {
        frame.regions().clear();
        if let Some(scene) = help.scene() {
            frame.compose(scene.clone());
            frame.native_shadow(help.bounds());
            help_status(frame, app, false);
        }
        return;
    }
    let profile = app.config.palette;
    if let Some(dialog) = app.last_dialog() {
        let Some(message) = message else {
            return;
        };
        let rect = dialog_style::window(
            frame,
            profile,
            message.rect,
            message.rect.width,
            message.rect.height,
            app.config.language.text(dialog.title),
        );
        let lines = &message.lines;
        let visible = message.visible;
        let maximum = message.maximum;
        let body = crate::turbo::widgets::memo(
            &lines
                .iter()
                .skip(app.message_offset())
                .take(visible)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n"),
        );
        frame.render_widget(
            body,
            Rect::new(
                rect.x + 2,
                rect.y + 1,
                rect.width - 4,
                rect.height.saturating_sub(4),
            ),
        );
        if maximum > 0 {
            frame.render_widget(
                Paragraph::new(app.config.language.text("↑↓ / PgUp/PgDn scroll"))
                    .style(profile.dos(0x70)),
                Rect::new(rect.x + 2, rect.bottom() - 3, rect.width - 4, 1),
            );
        }
        let close = Rect::new(rect.x + (rect.width - 12) / 2, rect.bottom() - 2, 12, 1);
        dialog_style::button(
            frame,
            profile,
            close,
            app.config.language.text("[ Close ]"),
            true,
        );
        frame.regions().clear();
        super::dialog_style::close_button(frame, profile, rect, Target::CloseDialog);
        frame.regions().push((close, Target::CloseDialog));
    }
}

pub(super) fn draw_operation_overlays(
    frame: &mut impl Frame,
    app: &App,
    area: Rect,
    layer: crate::focus::Focus,
) {
    let profile = app.config.palette;
    let dos = profile.styles();
    if let Some(prompt) = app
        .modals
        .prompt()
        .filter(|_| layer == crate::focus::Focus::Prompt)
    {
        let deleting = matches!(
            prompt.kind,
            PromptKind::Operation(Kind::Delete | Kind::Trash)
        );
        let simple = matches!(
            prompt.kind,
            PromptKind::GoTo | PromptKind::ImportDos | PromptKind::Operation(Kind::Mkdir)
        );
        // original-dn/RESOURCE/ENGLISH/DN.DNR dlgMkDir is 49×7.
        let width = if matches!(prompt.kind, PromptKind::Operation(Kind::Mkdir)) {
            49
        } else {
            59
        };
        let rect = dialog_style::window(
            frame,
            profile,
            area,
            width,
            if simple { 7 } else { 11 },
            app.config.language.text(prompt.title()),
        );
        frame.regions().clear();
        super::dialog_style::close_button(frame, profile, rect, Target::CancelPrompt);
        if deleting {
            let mut lines = if matches!(prompt.kind, PromptKind::Operation(Kind::Trash)) {
                vec![
                    app.config
                        .language
                        .text("Move {} item(s) to Trash?")
                        .replace("{}", &prompt.sources.len().to_string()),
                    app.config
                        .language
                        .text("Restore using your desktop file manager.")
                        .to_string(),
                ]
            } else {
                vec![
                    app.config
                        .language
                        .text("Permanently delete {} item(s)?")
                        .replace("{}", &prompt.sources.len().to_string()),
                    app.config
                        .language
                        .text("This cannot be undone.")
                        .to_string(),
                ]
            };
            for path in prompt.sources.iter().take(3) {
                lines.push(formatting::tail(
                    &display_name(&path.file_name().unwrap_or_default().to_string_lossy()),
                    rect.width - 4,
                ));
            }
            frame.render_widget(
                Paragraph::new(lines.join("\n")).style(dos(0x70)),
                Rect::new(rect.x + 2, rect.y + 2, rect.width - 4, 5),
            );
        } else {
            let label = app
                .config
                .language
                .text(if matches!(prompt.kind, PromptKind::ImportDos) {
                    "DOS configuration file"
                } else if simple {
                    "Directory name"
                } else {
                    "Destination"
                });
            let first = label.chars().next().map_or(0, char::len_utf8);
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(&label[..first], dos(0x7e)),
                    Span::styled(&label[first..], dos(0x7f)),
                ])),
                Rect::new(rect.x + 2, rect.y + 1, rect.width - 4, 1),
            );
            let input = Rect::new(rect.x + 2, rect.y + 2, rect.width - 4, 1);
            // Keep the cursor visible without splitting a UTF-8 path.
            let prefix = formatting::tail(
                &prompt.field[..prompt.cursor],
                input.width.saturating_sub(1),
            );
            let cursor = Span::raw(&prefix).width() as u16;
            frame.render_widget(
                crate::turbo::widgets::input(
                    &format!("{prefix}{}", &prompt.field[prompt.cursor..]),
                    prompt.focus == 0,
                ),
                input,
            );
            frame.regions().push((input, Target::PromptField));
            if prompt.focus == 0 && app.dialogs().is_empty() {
                frame.set_cursor_position((input.x + cursor, input.y));
            }
            if !simple {
                frame.render_widget(
                    Paragraph::new(
                        app.config
                            .language
                            .text("Existing destinations are refused."),
                    )
                    .style(dos(0x70)),
                    Rect::new(rect.x + 2, rect.y + 3, rect.width - 4, 1),
                );
                let mut lines = vec![format!("{} item(s):", prompt.sources.len())];
                for path in prompt.sources.iter().take(3) {
                    lines.push(formatting::tail(
                        &display_name(&path.file_name().unwrap_or_default().to_string_lossy()),
                        rect.width - 4,
                    ));
                }
                frame.render_widget(
                    Paragraph::new(lines.join("\n")).style(dos(0x70)),
                    Rect::new(rect.x + 2, rect.y + 4, rect.width - 4, 4),
                );
            }
        }
        for (i, label, target) in [
            (
                1,
                if deleting { "[ Delete ]" } else { "[ OK ]" },
                Target::SubmitPrompt,
            ),
            (2, "[ Cancel ]", Target::CancelPrompt),
            (3, "[ Help ]", Target::PromptHelp),
        ] {
            let button = Rect::new(rect.right() - 31 + (i - 1) * 10, rect.bottom() - 3, 9, 1);
            dialog_style::button(
                frame,
                profile,
                button,
                app.config.language.text(label),
                prompt.focus == i as usize || (i == 1 && prompt.focus == 0),
            );
            frame.regions().push((button, target));
        }
    }
    if let Some(job) = app
        .job
        .as_ref()
        .filter(|_| layer == crate::focus::Focus::Job)
    {
        let rect = dialog_style::window(
            frame,
            profile,
            area,
            59,
            9,
            app.config.language.text(" File operation "),
        );
        let text=format!("{:?} in progress…\n{} items · {} bytes\n{}\nEsc cancels; completed items are retained.",job.kind,job.progress.files,job.progress.bytes,formatting::tail(&display_name(&job.progress.path.to_string_lossy()),rect.width-4));
        frame.render_widget(
            Paragraph::new(text).style(dos(0x70)),
            Rect::new(rect.x + 2, rect.y + 1, rect.width - 4, 4),
        );
        frame.render_widget(
            crate::turbo::widgets::busy(),
            Rect::new(rect.x + 2, rect.bottom() - 4, rect.width - 4, 1),
        );
        let cancel = Rect::new(rect.x + (rect.width - 12) / 2, rect.bottom() - 2, 12, 1);
        dialog_style::button(
            frame,
            profile,
            cancel,
            app.config.language.text("[ Cancel ]"),
            true,
        );
        frame.regions().clear();
        super::dialog_style::close_button(frame, profile, rect, Target::CancelJob);
        frame.regions().push((cancel, Target::CancelJob));
    }
}

/// DN's help context replaces the manager's file-operation hints.
pub(super) fn help_status(frame: &mut impl Frame, app: &App, welcome: bool) {
    if welcome && crate::focus::current(app) != crate::focus::Focus::Welcome {
        return;
    }
    let label = app.config.language.text(if welcome {
        "Tab Next · Shift+Tab Previous · Enter Select · F1 Help · Esc Start"
    } else {
        "F1 Contents · Tab Links · Enter Follow · Alt+F1 Back · Esc Close"
    });
    let label = label
        .split_whitespace()
        .map(|word| {
            if ["Tab", "Shift+Tab", "Enter", "F1", "Esc", "Alt+F1"].contains(&word) {
                format!("~{word}~")
            } else {
                word.into()
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    let area = frame.area();
    frame.render_widget(
        crate::turbo::widgets::status(&label),
        Rect::new(0, area.height.saturating_sub(1), area.width, 1),
    );
}
