use crate::dialog_theme as theme;
use crate::painting::{Frame, Paragraph};
use crate::presentation::{
    layout::Rect,
    text::{Line, Span},
};
use crate::{
    app::App, dialog::PromptKind, filesystem::display_name, formatting, mouse::Target,
    operations::Kind,
};

use crate::turbo::dialog_spec::{centered, scope, Control, ControlKind, DialogSpec};
pub(crate) fn prompt_spec(app: &App, area: Rect) -> Option<DialogSpec<'_>> {
    let profile = app.config.palette;
    let dos = profile.styles_for(crate::palette::theme::Group::Dialogs);
    let prompt = app.modals.prompt().filter(|p| p.copy.is_none())?;
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
    let rect = centered(area, width, if simple { 7 } else { 11 });
    let mut spec = DialogSpec::new(
        rect,
        scope(app, crate::focus::Focus::Prompt)?,
        prompt.focus,
        app.config.language.text(prompt.title()),
        Target::SubmitPrompt,
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
        spec.caption(
            Paragraph::new(lines.join("\n")).style(dos(theme::TEXT)),
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
        spec.caption(
            Paragraph::new(Line::from(vec![
                Span::styled(&label[..first], dos(0x7e)),
                Span::styled(&label[first..], dos(theme::FRAME)),
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
        let mut field = Control::new(
            0,
            input,
            ControlKind::Input(format!("{prefix}{}", &prompt.field[prompt.cursor..])),
            Target::PromptField,
        );
        field.mnemonic = Some('d');
        spec.control(field);
        if prompt.focus == 0 && app.dialogs().is_empty() {
            spec.cursor = Some((input.x + cursor, input.y));
        }
        if !simple {
            spec.caption(
                Paragraph::new(
                    app.config
                        .language
                        .text("Existing destinations are refused."),
                )
                .style(dos(theme::TEXT)),
                Rect::new(rect.x + 2, rect.y + 3, rect.width - 4, 1),
            );
            let mut lines = vec![format!("{} item(s):", prompt.sources.len())];
            for path in prompt.sources.iter().take(3) {
                lines.push(formatting::tail(
                    &display_name(&path.file_name().unwrap_or_default().to_string_lossy()),
                    rect.width - 4,
                ));
            }
            spec.caption(
                Paragraph::new(lines.join("\n")).style(dos(theme::TEXT)),
                Rect::new(rect.x + 2, rect.y + 4, rect.width - 4, 4),
            );
        }
    }
    let buttons = [
        (
            1,
            if deleting { "[ Delete ]" } else { "[ OK ]" },
            Target::SubmitPrompt,
        ),
        (2, "[ Cancel ]", Target::CancelPrompt),
        (3, "[ Help ]", Target::PromptHelp),
    ];
    let labels = buttons.map(|(_, label, _)| app.config.language.text(label));
    let bounds = theme::button_row(
        Rect::new(rect.x + 2, rect.bottom() - 3, rect.width - 4, 1),
        &labels,
        9,
        1,
    );
    for ((i, _, target), (label, button)) in buttons.into_iter().zip(labels.into_iter().zip(bounds))
    {
        let mut control = Control::new(
            i as usize,
            button,
            ControlKind::Button(label.into()),
            target,
        );
        control.mnemonic = Some(match i {
            1 => 'k',
            2 => 'c',
            _ => 'h',
        });
        spec.control(control);
    }
    Some(spec)
}

pub(super) fn draw_info_dialog(
    frame: &mut impl Frame,
    app: &App,
    message: Option<&crate::layout::MessageLayout>,
) {
    if let Some(help) = app.native_help() {
        frame.regions().clear();
        if let Some(scene) = help.scene() {
            frame.compose(scene.clone());
            frame.native_shadow(help.bounds());
            help_status(frame, app, false);
        }
        return;
    }
    if let Some(s) = message.and_then(|m| message_spec(app, m)) {
        s.paint(frame, app);
    }
}
pub(crate) fn message_spec<'a>(
    app: &'a App,
    message: &crate::layout::MessageLayout,
) -> Option<DialogSpec<'a>> {
    let dialog = app.last_dialog()?;
    let profile = app.config.palette;
    let rect = message.rect;
    let mut s = DialogSpec::new(
        rect,
        scope(app, crate::focus::Focus::Message)?,
        0,
        app.config.language.text(dialog.title),
        Target::CloseDialog,
        Target::CloseDialog,
    );
    if dialog.is_about() {
        for (row, line) in message
            .lines
            .iter()
            .skip(app.message_offset())
            .take(message.visible)
            .enumerate()
        {
            let bounds = Rect::new(rect.x + 2, rect.y + 1 + row as u16, rect.width - 4, 1);
            let link = line == crate::dialog::REPOSITORY_URL;
            let style = if link {
                profile
                    .dos(theme::HEADING)
                    .add_modifier(crate::presentation::style::Modifier::UNDERLINED)
            } else {
                profile.for_group(crate::palette::theme::Group::Dialogs, theme::TEXT)
            };
            s.caption(
                Paragraph::new(line.clone())
                    .alignment(crate::presentation::layout::Alignment::Center)
                    .style(style),
                bounds,
            );
            if link {
                let width = Span::raw(line).width() as u16;
                s.hit(
                    Rect::new(bounds.x + (bounds.width - width) / 2, bounds.y, width, 1),
                    Target::AboutRepository,
                );
            }
        }
    } else {
        s.memo(
            &message
                .lines
                .iter()
                .skip(app.message_offset())
                .take(message.visible)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n"),
            Rect::new(
                rect.x + 2,
                rect.y + 1,
                rect.width - 4,
                rect.height.saturating_sub(4),
            ),
        );
    }
    if message.maximum > 0 {
        s.caption(
            Paragraph::new(app.config.language.text("↑↓ / PgUp/PgDn scroll"))
                .style(profile.for_group(crate::palette::theme::Group::Dialogs, theme::TEXT)),
            Rect::new(rect.x + 2, rect.bottom() - 3, rect.width - 4, 1),
        );
    }
    s.control(Control::new(
        0,
        Rect::new(
            rect.x + (rect.width - 12) / 2,
            rect.bottom() - if dialog.is_about() { 3 } else { 2 },
            12,
            1,
        ),
        ControlKind::Button(
            app.config
                .language
                .text(if dialog.is_about() {
                    "O~K~"
                } else {
                    "[ Close ]"
                })
                .into(),
        ),
        Target::CloseDialog,
    ));
    Some(s)
}

pub(super) fn draw_operation_overlays(
    frame: &mut impl Frame,
    app: &App,
    area: Rect,
    layer: crate::focus::Focus,
) {
    if let Some(prompt) = app
        .modals
        .prompt()
        .filter(|_| layer == crate::focus::Focus::Prompt)
    {
        if prompt.copy.is_some() {
            let area = frame.area();
            super::copy_dialog_view::draw(frame, app, area);
            return;
        }
        if let Some(spec) = prompt_spec(app, area) {
            spec.paint(frame, app);
        }
    }

    if layer == crate::focus::Focus::Job {
        if let Some(s) = job_spec(app, area) {
            s.paint(frame, app);
        }
    }
}
pub(crate) fn job_spec(app: &App, area: Rect) -> Option<DialogSpec<'_>> {
    let job = app.job.as_ref()?;
    let rect = centered(area, 59, 9);
    let mut s = DialogSpec::new(
        rect,
        scope(app, crate::focus::Focus::Job)?,
        0,
        app.config.language.text(" File operation "),
        Target::CancelJob,
        Target::CancelJob,
    );
    let text = format!(
        "{:?} in progress…\n{} items · {} bytes\n{}\nEsc cancels; completed items are retained.",
        job.kind,
        job.progress.files,
        job.progress.bytes,
        formatting::tail(
            &display_name(&job.progress.path.to_string_lossy()),
            rect.width - 4
        )
    );
    s.caption(
        Paragraph::new(text).style(
            app.config
                .palette
                .for_group(crate::palette::theme::Group::Dialogs, theme::TEXT),
        ),
        Rect::new(rect.x + 2, rect.y + 1, rect.width - 4, 4),
    );
    s.native(
        crate::turbo::widgets::busy(),
        Rect::new(rect.x + 2, rect.bottom() - 4, rect.width - 4, 1),
    );
    s.control(Control::new(
        0,
        Rect::new(rect.x + (rect.width - 12) / 2, rect.bottom() - 2, 12, 1),
        ControlKind::Button(app.config.language.text("[ Cancel ]").into()),
        Target::CancelJob,
    ));
    Some(s)
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
