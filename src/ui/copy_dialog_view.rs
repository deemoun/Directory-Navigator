use crate::dialog_theme as theme;
use crate::turbo::dialog_spec::{centered, scope, Control, ControlKind, DialogSpec};
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
    if let Some(spec) = copy_spec(app, area) {
        spec.paint(frame, app);
    }
}
pub(crate) fn copy_spec(app: &App, area: Rect) -> Option<DialogSpec<'_>> {
    let prompt = app.modals.prompt()?;
    let copy = prompt.copy.as_ref()?;
    let moving = matches!(
        prompt.kind,
        crate::dialog::PromptKind::Operation(crate::operations::Kind::Move)
    );
    let language = app.config.language;
    let profile = app.config.palette;
    let dos = profile.styles_for(crate::palette::theme::Group::Dialogs);
    let rect = centered(area, 59, 16);
    let mut spec = DialogSpec::new(
        rect,
        scope(app, crate::focus::Focus::Prompt)?,
        prompt.focus,
        language.text(if moving { "Rename/move" } else { "Copy" }),
        Target::SubmitPrompt,
        Target::CancelPrompt,
    );
    let label = if prompt.sources.len() == 1 {
        let directory = app
            .panels
            .iter()
            .flat_map(|p| &p.entries)
            .any(|entry| entry.path == prompt.sources[0] && entry.directory);
        language
            .text(match (moving, directory) {
                (true, true) => "Move/rename directory {} to",
                (true, false) => "Move/rename file {} to",
                (false, true) => "Copy directory {} to",
                (false, false) => "Copy file {} to",
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
            .text(if moving {
                "Move/rename {} selected items to"
            } else {
                "Copy {} selected items to"
            })
            .replace("{}", &prompt.sources.len().to_string())
    };
    spec.caption(
        Paragraph::new(formatting::tail(&label, 55)).style(dos(theme::FRAME)),
        Rect::new(rect.x + 2, rect.y + 1, 55, 1),
    );
    let field = Rect::new(rect.x + 2, rect.y + 2, 53, 1);
    let column = Line::from(&prompt.field[..prompt.cursor]).width();
    let scroll = column.saturating_sub(field.width.saturating_sub(1) as usize);
    let mut field_control = Control::new(
        0,
        field,
        ControlKind::ScrolledInput {
            value: prompt.field.clone(),
            scroll,
        },
        Target::PromptField,
    );
    field_control.mnemonic = Some('d');
    spec.control(field_control);
    if prompt.focus == 0 {
        spec.cursor = Some((field.x + (column - scroll) as u16, field.y));
    }
    let history = Rect::new(field.right(), field.y, 2, 1);
    spec.control(Control::new(
        10,
        history,
        ControlKind::History,
        Target::Copy(Action::History),
    ));
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
        let mut control = Control::new(
            if focus == usize::MAX {
                20 + row as usize
            } else {
                focus
            },
            area,
            ControlKind::Radio {
                caption: language.text(label).into(),
                checked,
            },
            Target::Copy(action.unwrap_or(Action::Refuse)),
        );
        control.label_keys(language, label);
        control.enabled = action.is_some();
        spec.control(control);
    }
    spec.caption(
        Paragraph::new(language.text("Existing destinations are never replaced."))
            .style(dos(theme::DISABLED)),
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
            moving || copy.options.remove_sources,
            5,
            !moving,
        ),
    ] {
        let area = Rect::new(rect.x + x, rect.y + y, 25, 1);
        let mut control = Control::new(
            if focus == usize::MAX { 30 } else { focus },
            area,
            ControlKind::Checkbox {
                caption: language.text(label).into(),
                checked,
            },
            Target::Copy(action.unwrap_or(Action::CheckSpace)),
        );
        control.label_keys(language, label);
        control.enabled = enabled;
        spec.control(control);
    }
    let buttons = [
        (0, "O~K~", Target::SubmitPrompt),
        (1, "~C~ancel", Target::CancelPrompt),
        (2, "~T~ree", Target::Copy(Action::Tree)),
        (3, "~H~elp", Target::PromptHelp),
    ];
    let labels = buttons.map(|(_, label, _)| language.text(label));
    let bounds = theme::button_row(Rect::new(rect.x + 2, rect.y + 13, 50, 1), &labels, 10, 0);
    for ((index, source, target), (label, area)) in
        buttons.into_iter().zip(labels.into_iter().zip(bounds))
    {
        let mut control = Control::new(
            6 + index as usize,
            area,
            ControlKind::Button(label.into()),
            target,
        );
        control.label_keys(language, source);
        spec.control(control);
    }
    spec.tab_order = crate::copy_dialog::FOCUS.to_vec();
    Some(spec)
}

pub(super) fn history(frame: &mut impl Frame, app: &App, area: Rect) {
    if let Some(s) = history_spec(app, area) {
        s.paint(frame, app);
    }
}
pub(crate) fn history_spec(app: &App, area: Rect) -> Option<DialogSpec<'_>> {
    let h = app.modals.copy_history()?;
    let p = app.config.palette;
    let rect = crate::turbo::dialog_spec::centered(area, 70, h.rows as u16 + 4);
    let action = if h.paths.is_empty() {
        Target::CloseCopyHistory
    } else {
        Target::CopyHistory(h.cursor)
    };
    let mut s = DialogSpec::new(
        rect,
        scope(app, crate::focus::Focus::CopyHistory)?,
        0,
        app.config.language.text("Destination history"),
        action,
        Target::CloseCopyHistory,
    );
    if h.paths.is_empty() {
        s.caption(
            Paragraph::new(app.config.language.text("No destination history"))
                .style(p.dos(theme::DISABLED)),
            Rect::new(rect.x + 2, rect.y + 2, 66, 1),
        );
    }
    let mut c = Control::new(
        0,
        Rect::new(rect.x + 1, rect.y + 1, 68, h.rows as u16),
        ControlKind::List {
            items: h
                .paths
                .iter()
                .map(|p| display_name(&p.to_string_lossy()))
                .collect(),
            cursor: h.cursor,
            offset: h.offset,
        },
        action,
    );
    c.enter_action = true;
    c.paint = !h.paths.is_empty();
    c.hits = (h.offset..h.paths.len().min(h.offset + h.rows))
        .map(|i| {
            (
                Rect::new(rect.x + 1, rect.y + 1 + (i - h.offset) as u16, 68, 1),
                Target::CopyHistory(i),
            )
        })
        .collect();
    s.control(c);
    s.caption(
        Paragraph::new(app.config.language.text("Enter selects · Esc returns"))
            .style(p.dos(theme::TEXT)),
        Rect::new(rect.x + 2, rect.bottom() - 2, 66, 1),
    );
    Some(s)
}
