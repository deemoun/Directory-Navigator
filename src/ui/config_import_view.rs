use crate::dialog_theme as theme;
use crate::painting::Paragraph;
use crate::presentation::layout::Rect;
use crate::turbo::dialog_spec::{centered, scope, Control, ControlKind, DialogSpec};
use crate::{app::App, mouse::Target};

pub(crate) fn import_spec(app: &App, area: Rect) -> Option<DialogSpec<'_>> {
    let preview = app.modals.config_import()?;
    let profile = app.config.palette;
    let lang = app.config.language;
    let rect = centered(area, 76, 21);
    let mut spec = DialogSpec::new(
        rect,
        scope(app, crate::focus::Focus::ConfigImport)?,
        preview.focus,
        lang.text(" Import DOS settings "),
        Target::ApplyImport,
        Target::CancelImport,
    );
    let rows = usize::from(rect.height.saturating_sub(5));
    let lines = preview.lines(lang);
    let offset = preview.offset.min(lines.len().saturating_sub(rows));
    let body = lines
        .iter()
        .skip(offset)
        .take(rows)
        .map(|line| crate::filesystem::display_name(line))
        .collect::<Vec<_>>()
        .join("\n");
    spec.native(
        crate::turbo::widgets::memo(
            &body
                .lines()
                .map(|s| crate::turbo::widgets::pan(s, preview.horizontal as usize))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        Rect::new(rect.x + 2, rect.y + 1, rect.width - 4, rect.height - 5),
    );
    spec.caption(
        Paragraph::new(lang.text("↑↓ / PgUp/PgDn scroll")).style(profile.dos(theme::TEXT)),
        Rect::new(rect.x + 2, rect.bottom() - 4, rect.width - 4, 1),
    );
    let enabled = preview.can_apply();
    for (index, label, target) in [
        (0, "[ Apply ]", Target::ApplyImport),
        (1, "[ Cancel ]", Target::CancelImport),
    ] {
        let button = Rect::new(
            rect.x + rect.width / 2 - 15 + index * 16,
            rect.bottom() - 2,
            14,
            1,
        );
        let mut c = Control::new(
            usize::from(index),
            button,
            ControlKind::Button(lang.text(label).into()),
            target,
        );
        c.enabled = index != 0 || enabled;
        spec.control(c);
    }
    Some(spec)
}
