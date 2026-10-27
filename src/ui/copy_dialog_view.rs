use crate::{
    app::App,
    copy_dialog::Action,
    filesystem::display_name,
    formatting,
    mouse::Target,
    operations::ExistingDestination,
    painting::{Frame, Paragraph},
    presentation::{layout::Rect, text::Line},
};
pub(super) fn draw(frame: &mut impl Frame, app: &App, area: Rect) {
    let Some(prompt) = app.modals.prompt() else {
        return;
    };
    let Some(copy) = &prompt.copy else {
        return;
    };
    let language = app.config.language;
    let profile = app.config.palette;
    let dos = profile.styles();
    let rect = super::dialog_style::window(frame, profile, area, 59, 16, language.text("Copy"));
    frame.regions().clear();
    super::dialog_style::close_button(frame, profile, rect, Target::CancelPrompt);
    let label = if prompt.sources.len() == 1 {
        let directory = app
            .panels
            .iter()
            .flat_map(|p| &p.entries)
            .any(|entry| entry.path == prompt.sources[0] && entry.directory);
        language
            .text(if directory {
                "Copy directory {} to"
            } else {
                "Copy file {} to"
            })
            .replace(
                "{}",
                &display_name(
                    &prompt.sources[0]
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy(),
                ),
            )
    } else {
        language
            .text("Copy {} selected items to")
            .replace("{}", &prompt.sources.len().to_string())
    };
    frame.render_widget(
        Paragraph::new(formatting::tail(&label, 55)).style(dos(0x7f)),
        Rect::new(rect.x + 2, rect.y + 1, 55, 1),
    );
    let field = Rect::new(rect.x + 2, rect.y + 2, 53, 1);
    let column = Line::from(&prompt.field[..prompt.cursor]).width();
    let scroll = column.saturating_sub(field.width.saturating_sub(1) as usize);
    frame.render_widget(
        crate::turbo::widgets::input_scrolled(&prompt.field, scroll, prompt.focus == 0),
        field,
    );
    frame.regions().push((field, Target::PromptField));
    if prompt.focus == 0 {
        frame.set_cursor_position((field.x + (column - scroll) as u16, field.y));
    }
    let history = Rect::new(field.right(), field.y, 2, 1);
    frame.render_widget(crate::turbo::widgets::history_button(), history);
    frame
        .regions()
        .push((history, Target::Copy(Action::History)));
    for (row, label, action, checked, focus) in [
        (0, "~O~verwrite all existing files", None, false, usize::MAX),
        (1, "A~p~pend to all existing files", None, false, usize::MAX),
        (
            2,
            "~R~efuse existing destinations",
            Some(Action::Refuse),
            copy.options.existing == ExistingDestination::Refuse,
            1,
        ),
        (
            3,
            "~S~kip all existing files",
            Some(Action::Skip),
            copy.options.existing == ExistingDestination::Skip,
            2,
        ),
        (4, "Refresh o~l~d files", None, false, usize::MAX),
    ] {
        let area = Rect::new(rect.x + 3, rect.y + 4 + row, 53, 1);
        frame.render_widget(
            crate::turbo::widgets::radio_enabled(
                language.text(label),
                checked,
                prompt.focus == focus,
                action.is_some(),
            ),
            area,
        );
        if let Some(action) = action {
            frame.regions().push((area, Target::Copy(action)));
        }
    }
    frame.render_widget(
        Paragraph::new(language.text("Existing destinations are never replaced.")).style(dos(0x78)),
        Rect::new(rect.x + 3, rect.y + 9, 53, 1),
    );
    for (x, y, label, action, checked, focus, enabled) in [
        (
            3,
            10,
            "Check ~f~ree disk space",
            Some(Action::CheckSpace),
            copy.options.check_free_space,
            3,
            true,
        ),
        (
            3,
            11,
            "~V~erify disk writes",
            Some(Action::Verify),
            copy.options.verify_writes || copy.options.remove_sources,
            4,
            !copy.options.remove_sources,
        ),
        (
            31,
            10,
            "Copy ~d~escriptions",
            None,
            false,
            usize::MAX,
            false,
        ),
        (
            31,
            11,
            "Remove so~u~rce files",
            Some(Action::RemoveSources),
            copy.options.remove_sources,
            5,
            true,
        ),
    ] {
        let area = Rect::new(rect.x + x, rect.y + y, 25, 1);
        frame.render_widget(
            crate::turbo::widgets::checkbox(
                language.text(label),
                checked,
                prompt.focus == focus,
                enabled,
            ),
            area,
        );
        if let Some(action) = action.filter(|_| enabled) {
            frame.regions().push((area, Target::Copy(action)));
        }
    }
    for (index, label, target, focused) in [
        (0, "O~K~", Target::SubmitPrompt, prompt.focus <= 6),
        (1, "~C~ancel", Target::CancelPrompt, prompt.focus == 7),
        (2, "~T~ree", Target::Copy(Action::Tree), prompt.focus == 8),
        (3, "~H~elp", Target::PromptHelp, prompt.focus == 9),
    ] {
        let area = Rect::new(rect.x + 12 + index * 10, rect.y + 13, 10, 1);
        super::dialog_style::button(frame, profile, area, language.text(label), focused);
        frame.regions().push((area, target));
    }
}
pub(super) fn history(frame: &mut impl Frame, app: &App, area: Rect) {
    let Some(history) = app.modals.copy_history() else {
        return;
    };
    let p = app.config.palette;
    let rect = super::dialog_style::window(
        frame,
        p,
        area,
        70,
        history.rows as u16 + 4,
        app.config.language.text("Destination history"),
    );
    frame.regions().clear();
    super::dialog_style::close_button(frame, p, rect, Target::CloseCopyHistory);
    if history.paths.is_empty() {
        frame.render_widget(
            Paragraph::new(app.config.language.text("No destination history")).style(p.dos(0x78)),
            Rect::new(rect.x + 2, rect.y + 2, 66, 1),
        );
    } else {
        let rows = history
            .paths
            .iter()
            .map(|path| display_name(&path.to_string_lossy()))
            .collect();
        frame.render_widget(
            crate::turbo::widgets::list(rows, history.cursor, history.offset, true),
            Rect::new(rect.x + 1, rect.y + 1, 68, history.rows as u16),
        );
        for i in history.offset..history.paths.len().min(history.offset + history.rows) {
            frame.regions().push((
                Rect::new(rect.x + 1, rect.y + 1 + (i - history.offset) as u16, 68, 1),
                Target::CopyHistory(i),
            ));
        }
    }
    frame.render_widget(
        Paragraph::new(app.config.language.text("Enter selects · Esc returns")).style(p.dos(0x70)),
        Rect::new(rect.x + 2, rect.bottom() - 2, 66, 1),
    );
}
