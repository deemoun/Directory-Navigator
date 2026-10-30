use crate::dialog_theme as theme;
use crate::painting::{Frame, Paragraph};
use crate::presentation::layout::Rect;
use crate::turbo::dialog_spec::{centered, scope, Control, ControlKind, DialogSpec};
use crate::{app::App, mouse::Target, settings::SettingsKind};

pub(super) fn draw_settings(frame: &mut impl Frame, app: &App, area: Rect) {
    if let Some(spec) = settings_spec(app, area) {
        spec.paint(frame, app);
    }
    if app.modals.settings_choice().is_some() {
        if let Some(child) = app
            .modals
            .infos()
            .filter(|m| m.focus == crate::focus::Focus::Settings)
            .last()
        {
            frame.modal_scope(Some(child));
        }
        draw_choices(frame, app, area);
    }
}
pub(crate) fn settings_spec(app: &App, area: Rect) -> Option<DialogSpec<'_>> {
    let profile = app.config.palette;
    let dos = profile.styles_for(crate::palette::theme::Group::Dialogs);
    let settings = app.modals.settings()?;
    if settings.kind == SettingsKind::Keyboard {
        return super::shortcuts_view::settings_spec(app, area);
    }
    if settings.kind == SettingsKind::Regional {
        return regional_spec(app, area);
    }
    if settings.kind == SettingsKind::Plugins {
        return plugins_spec(app, area);
    }
    let form = settings.kind.definition();
    let (width, height) = (form.width, form.height);
    let rect = centered(area, width, height);
    let mut spec = DialogSpec::new(
        rect,
        scope(app, crate::focus::Focus::Settings)?,
        settings.cursor,
        app.config.language.text(settings.title()),
        Target::AcceptSettings,
        Target::CancelSettings,
    );
    let two_columns = form.columns;
    spec.caption(
        Paragraph::new(
            app.config
                .language
                .text(if two_columns { "Behavior" } else { "Options" }),
        )
        .style(dos(theme::HEADING)),
        Rect::new(rect.x + 3, rect.y + 2, 30, 1),
    );
    if two_columns {
        spec.caption(
            Paragraph::new(app.config.language.text("Display")).style(dos(theme::HEADING)),
            Rect::new(rect.x + 39, rect.y + 2, 26, 1),
        );
    }
    for (index, setting) in settings.rows().iter().enumerate() {
        let row = form.bounds(rect, index);
        let caption = app.config.language.text(setting.label());
        let mut control = Control::new(
            index,
            row,
            ControlKind::Checkbox {
                caption: caption.into(),
                checked: settings.value(*setting),
            },
            Target::Setting(index),
        );
        control.mnemonic = crate::menu::mnemonic(caption);
        control.fallback_mnemonic = crate::menu::mnemonic(setting.label());
        spec.control(control);
    }
    if settings.kind == SettingsKind::Highlight {
        for (index, (color, label)) in [
            (0x0f, "Directories / documents"),
            (0x0a, "Executables / scripts"),
            (0x0b, "Source / configuration"),
            (0x0d, "Archives"),
            (0x0c, "Images / audio / video"),
            (0x09, "Symbolic links"),
        ]
        .iter()
        .enumerate()
        {
            spec.caption(
                Paragraph::new(app.config.language.text(label)).style(dos(*color)),
                Rect::new(rect.x + 5, rect.y + 6 + index as u16, width - 10, 1),
            );
        }
    } else if settings.kind == SettingsKind::System {
        spec.caption(Paragraph::new(app.config.language.text("Existing destinations are always refused.\nTrash and permanent deletion require confirmation.")).style(dos(theme::TEXT)), Rect::new(rect.x + 3, rect.y + 7, width - 6, 2));
    }
    spec.caption(
        Paragraph::new(
            app.config
                .language
                .text("Tab / ↑↓ choose · Space toggle · Enter OK · Esc Cancel"),
        )
        .style(dos(theme::TEXT)),
        Rect::new(rect.x + 3, rect.bottom() - theme::HINT_BOTTOM, width - 6, 1),
    );
    let count = spec.controls.len();
    for (index, (label, target)) in [
        (" [ OK ] ", Target::AcceptSettings),
        (" [ Cancel ] ", Target::CancelSettings),
    ]
    .iter()
    .enumerate()
    {
        let button = theme::button(rect, width / 2 - 15, index);
        spec.control(Control::new(
            count + index,
            button,
            ControlKind::Button(app.config.language.text(label).into()),
            *target,
        ));
    }
    Some(spec)
}

fn plugins_spec(app: &App, area: Rect) -> Option<DialogSpec<'_>> {
    let settings = app.modals.settings()?;
    let language = app.config.language;
    let palette = app.config.palette;
    let count = settings.plugins.len();
    let form = settings.kind.definition();
    let rect = centered(
        area,
        form.width,
        ((count + usize::from(form.height)).min(usize::from(area.height.saturating_sub(2)))) as u16,
    );
    let rows = usize::from(rect.height.saturating_sub(9)).max(1);
    let offset = settings
        .cursor
        .min(count.saturating_sub(1))
        .saturating_sub(rows - 1);
    let mut spec = DialogSpec::new(
        rect,
        scope(app, crate::focus::Focus::Settings)?,
        settings.cursor,
        language.text(settings.title()),
        Target::AcceptSettings,
        Target::CancelSettings,
    );
    spec.caption(
        Paragraph::new(language.text("Enabled plugins"))
            .style(palette.for_group(crate::palette::theme::Group::Dialogs, theme::HEADING)),
        Rect::new(rect.x + 3, rect.y + 2, rect.width - 6, 1),
    );
    for (index, (id, title)) in settings.plugins.iter().enumerate() {
        let visible = (offset..offset + rows).contains(&index);
        let row = Rect::new(
            rect.x + 3,
            rect.y + 4 + if visible { (index - offset) as u16 } else { 0 },
            rect.width - 7,
            1,
        );
        let mut c = Control::new(
            index,
            row,
            ControlKind::Checkbox {
                caption: format!("{} ({})", language.text(title), id.0),
                checked: settings.draft.plugin_enabled(*id),
            },
            Target::Setting(index),
        );
        if !visible {
            c.paint = false;
            c.hits.clear();
        }
        spec.control(c);
    }
    if count > rows {
        spec.native(
            crate::turbo::widgets::scrollbar(settings.cursor.min(count - 1), count - 1, rows),
            Rect::new(rect.right() - 3, rect.y + 4, 1, rows as u16),
        );
    }
    spec.caption(
        Paragraph::new(language.text("Disabling closes open plugin windows."))
            .style(palette.for_group(crate::palette::theme::Group::Dialogs, theme::DISABLED)),
        Rect::new(
            rect.x + 3,
            rect.bottom() - theme::HINT_BOTTOM,
            rect.width - 6,
            1,
        ),
    );
    for (index, label, target) in [
        (0, " [ OK ] ", Target::AcceptSettings),
        (1, " [ Cancel ] ", Target::CancelSettings),
    ] {
        spec.control(Control::new(
            count + index,
            theme::button(rect, 17, index),
            ControlKind::Button(language.text(label).into()),
            target,
        ));
    }
    Some(spec)
}

fn regional_spec(app: &App, area: Rect) -> Option<DialogSpec<'_>> {
    let settings = app.modals.settings()?;
    let profile = app.config.palette;
    let language = app.config.language;
    let form = settings.kind.definition();
    let rect = centered(area, form.width, form.height);
    let mut spec = DialogSpec::new(
        rect,
        scope(app, crate::focus::Focus::Settings)?,
        settings.cursor,
        language.text(settings.title()),
        Target::AcceptSettings,
        Target::CancelSettings,
    );
    let format = settings.draft.datetime;
    for (i, choice) in settings.choice_fields().iter().enumerate() {
        let label = choice.definition().caption;
        let row = Rect::new(rect.x + 3, rect.y + 2 + i as u16, rect.width - 6, 1);
        spec.caption(
            Paragraph::new(language.text(label))
                .style(profile.for_group(crate::palette::theme::Group::Dialogs, theme::TEXT)),
            Rect::new(row.x, row.y, 24, 1),
        );
        let field = Rect::new(row.x + 24, row.y, row.width - 24, 1);
        let mut control = Control::new(
            i,
            field,
            ControlKind::Choice(crate::settings::choice_items(settings, i, language)),
            Target::RegionalChoice(i),
        );
        control.hits = vec![
            (row, control.action),
            (
                Rect::new(field.right() - 2, field.y, 2, 1),
                Target::OpenSettingsChoice(i),
            ),
        ];
        spec.control(control);
    }
    // One sample instant per dialog; painting must not read ambient time.
    let now = &settings.preview_time;
    spec.caption(
        Paragraph::new(format!(
            "{}: {}",
            language.text("Preview"),
            format.stamp(now, false, true)
        ))
        .style(profile.for_group(crate::palette::theme::Group::Dialogs, theme::HEADING)),
        Rect::new(rect.x + 3, rect.y + 9, rect.width - 6, 1),
    );
    spec.caption(
        Paragraph::new(language.text("←/→ or Space changes · Tab/↑↓ chooses"))
            .style(profile.for_group(crate::palette::theme::Group::Dialogs, theme::TEXT)),
        Rect::new(rect.x + 3, rect.y + 11, rect.width - 6, 1),
    );
    spec.caption(
        Paragraph::new(language.text("Enter applies · Esc cancels"))
            .style(profile.for_group(crate::palette::theme::Group::Dialogs, theme::TEXT)),
        Rect::new(rect.x + 3, rect.y + 12, rect.width - 6, 1),
    );
    for (i, (label, target)) in [
        ("[ OK ]", Target::AcceptSettings),
        ("[ Cancel ]", Target::CancelSettings),
    ]
    .into_iter()
    .enumerate()
    {
        let button = Rect::new(rect.x + 17 + i as u16 * 18, rect.bottom() - 3, 14, 1);
        spec.control(Control::new(
            5 + i,
            button,
            ControlKind::Button(language.text(label).into()),
            target,
        ));
    }
    Some(spec)
}

fn draw_choices(frame: &mut impl Frame, app: &App, area: Rect) {
    let Some(choice) = app.modals.settings_choice() else {
        return;
    };
    let bounds = settings_choice_bounds(app, area).unwrap();
    let rect = super::dialog_style::window(
        frame,
        app.config.palette,
        area,
        bounds.width,
        bounds.height,
        app.config
            .language
            .text(if choice.kind == SettingsKind::Keyboard {
                "Profile"
            } else {
                "Options"
            }),
    );
    frame.regions().clear();
    frame.regions().push((area, Target::CancelSettingsChoice));
    let list = Rect::new(
        rect.x + 2,
        rect.y + 2,
        rect.width - 4,
        choice.items.len() as u16,
    );
    frame.render_widget(
        crate::turbo::widgets::list(choice.items.clone(), choice.cursor, 0, true),
        list,
    );
    for i in 0..choice.items.len() {
        frame.regions().push((
            Rect::new(list.x, list.y + i as u16, list.width, 1),
            Target::SettingsChoice(i),
        ));
    }
}

pub(crate) fn settings_choice_bounds(app: &App, area: Rect) -> Option<Rect> {
    let choice = app.modals.settings_choice()?;
    let width = choice
        .items
        .iter()
        .map(|s| crate::presentation::text::Span::raw(s).width())
        .max()
        .unwrap_or(20)
        .clamp(24, 60) as u16
        + 4;
    Some(centered(area, width, choice.items.len() as u16 + 4))
}
