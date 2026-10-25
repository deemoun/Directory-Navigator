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
    app: &mut App,
    message: Option<&crate::layout::MessageLayout>,
) {
    let profile = app.config.palette;
    if let Some(dialog) = app.dialogs.last() {
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
        let body = Paragraph::new(
            lines
                .iter()
                .skip(app.message_offset)
                .take(visible)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .style(profile.dos(0x70));
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
        app.mouse.regions.clear();
        super::dialog_style::close_button(
            frame,
            profile,
            rect,
            &mut app.mouse.regions,
            Target::CloseDialog,
        );
        app.mouse.regions.push((close, Target::CloseDialog));
    }
}

pub(super) fn draw_operation_overlays(frame: &mut impl Frame, app: &mut App, area: Rect) {
    let profile = app.config.palette;
    let dos = profile.styles();
    if let Some(prompt) = &app.prompt {
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
        app.mouse.regions.clear();
        super::dialog_style::close_button(
            frame,
            profile,
            rect,
            &mut app.mouse.regions,
            Target::CancelPrompt,
        );
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
                Paragraph::new(format!("{prefix}{}", &prompt.field[prompt.cursor..]))
                    .style(dialog_style::input_style(profile, prompt.focus == 0)),
                input,
            );
            app.mouse.regions.push((input, Target::PromptField));
            if prompt.focus == 0 && app.dialogs.is_empty() {
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
            app.mouse.regions.push((button, target));
        }
    }
    if let Some(job) = &app.job {
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
        let cancel = Rect::new(rect.x + (rect.width - 12) / 2, rect.bottom() - 2, 12, 1);
        dialog_style::button(
            frame,
            profile,
            cancel,
            app.config.language.text("[ Cancel ]"),
            true,
        );
        app.mouse.regions.clear();
        super::dialog_style::close_button(
            frame,
            profile,
            rect,
            &mut app.mouse.regions,
            Target::CancelJob,
        );
        app.mouse.regions.push((cancel, Target::CancelJob));
    }
}
