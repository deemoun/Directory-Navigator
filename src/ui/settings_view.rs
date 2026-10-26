use crate::painting::{Frame, Paragraph};
use crate::presentation::layout::Rect;
use crate::{app::App, mouse::Target, settings::SettingsKind};

pub(super) fn draw_settings(frame: &mut impl Frame, app: &App, area: Rect) {
    let profile = app.config.palette;
    let dos = profile.styles();
    let Some(settings) = app.modals.settings() else {
        return;
    };
    if settings.kind == SettingsKind::Keyboard {
        super::shortcuts_view::draw_settings(frame, app, area);
        draw_choices(frame, app, area);
        return;
    }
    if settings.kind == SettingsKind::Regional {
        draw_regional(frame, app, area);
        draw_choices(frame, app, area);
        return;
    }
    let width = if settings.kind == SettingsKind::FileManager {
        74
    } else {
        60
    };
    let height = match settings.kind {
        SettingsKind::Keyboard => 20,
        SettingsKind::Regional => 17,
        SettingsKind::System => 14,
        SettingsKind::Interface => 12,
        SettingsKind::FileManager => 14,
        SettingsKind::Highlight => 18,
    };
    let rect = super::dialog_style::window(
        frame,
        profile,
        area,
        width,
        height,
        app.config.language.text(settings.title()),
    );
    frame.regions().clear();
    super::dialog_style::close_button(frame, profile, rect, Target::CancelSettings);
    let two_columns = settings.kind == SettingsKind::FileManager;
    frame.render_widget(
        Paragraph::new(
            app.config
                .language
                .text(if two_columns { "Behavior" } else { "Options" }),
        )
        .style(dos(0x71)),
        Rect::new(rect.x + 3, rect.y + 2, 30, 1),
    );
    if two_columns {
        frame.render_widget(
            Paragraph::new(app.config.language.text("Display")).style(dos(0x71)),
            Rect::new(rect.x + 39, rect.y + 2, 26, 1),
        );
    }
    for (index, setting) in settings.rows().iter().enumerate() {
        let trash = matches!(setting, crate::settings::Setting::Trash);
        let right = two_columns && index >= 3 && !trash;
        let row = Rect::new(
            rect.x + if right { 39 } else { 3 },
            rect.y
                + 4
                + if right {
                    index as u16 - 3
                } else {
                    if trash {
                        3
                    } else {
                        index as u16
                    }
                },
            if two_columns { 32 } else { width - 6 },
            1,
        );
        let selected = settings.cursor == index;
        frame.render_widget(
            crate::turbo::widgets::checkbox(
                app.config.language.text(setting.label()),
                settings.value(*setting),
                selected,
                true,
            ),
            row,
        );
        frame.regions().push((row, Target::Setting(index)));
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
            frame.render_widget(
                Paragraph::new(app.config.language.text(label)).style(dos(*color)),
                Rect::new(rect.x + 5, rect.y + 6 + index as u16, width - 10, 1),
            );
        }
    } else if settings.kind == SettingsKind::System {
        frame.render_widget(Paragraph::new(app.config.language.text("Existing destinations are always refused.\nTrash and permanent deletion require confirmation.")).style(dos(0x70)), Rect::new(rect.x + 3, rect.y + 7, width - 6, 2));
    }
    frame.render_widget(
        Paragraph::new(
            app.config
                .language
                .text("Tab / ↑↓ choose · Space toggle · Enter OK · Esc Cancel"),
        )
        .style(dos(0x70)),
        Rect::new(rect.x + 3, rect.bottom() - 5, width - 6, 1),
    );
    let count = settings.rows().len();
    for (index, (label, target)) in [
        (" [ OK ] ", Target::AcceptSettings),
        (" [ Cancel ] ", Target::CancelSettings),
    ]
    .iter()
    .enumerate()
    {
        let button = Rect::new(
            rect.x + width / 2 - 15 + index as u16 * 18,
            rect.bottom() - 3,
            14,
            1,
        );
        super::dialog_style::button(
            frame,
            profile,
            button,
            app.config.language.text(label),
            settings.cursor == count + index,
        );
        frame.regions().push((button, *target));
    }
}

fn draw_regional(frame: &mut impl Frame, app: &App, area: Rect) {
    let Some(settings) = app.modals.settings() else {
        return;
    };
    let profile = app.config.palette;
    let language = app.config.language;
    let rect = super::dialog_style::window(
        frame,
        profile,
        area,
        68,
        17,
        language.text(settings.title()),
    );
    frame.regions().clear();
    super::dialog_style::close_button(frame, profile, rect, Target::CancelSettings);
    let format = settings.draft.datetime;
    let labels = [
        "Language",
        "Date order",
        "Date separator",
        "Time format",
        "Time separator",
    ];
    for (i, label) in labels.into_iter().enumerate() {
        let row = Rect::new(rect.x + 3, rect.y + 2 + i as u16, rect.width - 6, 1);
        frame.render_widget(
            Paragraph::new(language.text(label)).style(profile.dos(0x70)),
            Rect::new(row.x, row.y, 24, 1),
        );
        let field = Rect::new(row.x + 24, row.y, row.width - 24, 1);
        frame.render_widget(
            crate::turbo::widgets::combo(
                &crate::settings::choice_items(settings, i, language),
                0,
                settings.cursor == i,
            ),
            field,
        );
        frame.regions().push((row, Target::RegionalChoice(i)));
        frame.regions().push((
            Rect::new(field.right() - 2, field.y, 2, 1),
            Target::OpenSettingsChoice(i),
        ));
    }
    // One sample instant per dialog; painting must not read ambient time.
    let now = &settings.preview_time;
    frame.render_widget(
        Paragraph::new(format!(
            "{}: {}",
            language.text("Preview"),
            format.stamp(now, false, true)
        ))
        .style(profile.dos(0x71)),
        Rect::new(rect.x + 3, rect.y + 9, rect.width - 6, 1),
    );
    frame.render_widget(
        Paragraph::new(language.text("←/→ or Space changes · Tab/↑↓ chooses"))
            .style(profile.dos(0x70)),
        Rect::new(rect.x + 3, rect.y + 11, rect.width - 6, 1),
    );
    frame.render_widget(
        Paragraph::new(language.text("Enter applies · Esc cancels")).style(profile.dos(0x70)),
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
        super::dialog_style::button(
            frame,
            profile,
            button,
            app.config.language.text(language.text(label)),
            settings.cursor == 5 + i,
        );
        frame.regions().push((button, target));
    }
}

fn draw_choices(frame: &mut impl Frame, app: &App, area: Rect) {
    let Some(choice) = app.modals.settings_choice() else {
        return;
    };
    let width = choice
        .items
        .iter()
        .map(|s| crate::presentation::text::Span::raw(s).width())
        .max()
        .unwrap_or(20)
        .clamp(24, 60) as u16
        + 4;
    let rect = super::dialog_style::window(
        frame,
        app.config.palette,
        area,
        width,
        choice.items.len() as u16 + 4,
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
