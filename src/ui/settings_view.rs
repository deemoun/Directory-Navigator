use super::menu_view::label_spans;
use crate::{app::App, mouse::Target, settings::SettingsKind};
use ratatui::{
    layout::Rect,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

pub(super) fn draw_settings(frame: &mut Frame, app: &mut App, area: Rect) {
    let profile = app.config.palette;
    let dos = profile.styles();
    let Some(settings) = &app.settings else {
        return;
    };
    let width = if settings.kind == SettingsKind::FileManager {
        74
    } else {
        60
    };
    let height = match settings.kind {
        SettingsKind::System => 14,
        SettingsKind::Interface => 12,
        SettingsKind::FileManager => 14,
        SettingsKind::Highlight => 18,
    };
    let rect = super::dialog_style::window(frame, profile, area, width, height, settings.title());
    app.mouse.regions.clear();
    super::dialog_style::close_button(
        frame,
        profile,
        rect,
        &mut app.mouse.regions,
        Target::CancelSettings,
    );
    let two_columns = settings.kind == SettingsKind::FileManager;
    frame.render_widget(
        Paragraph::new(if two_columns { "Behavior" } else { "Options" }).style(dos(0x71)),
        Rect::new(rect.x + 3, rect.y + 2, 30, 1),
    );
    if two_columns {
        frame.render_widget(
            Paragraph::new("Display").style(dos(0x71)),
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
        let normal = dos(if selected { 0x30 } else { 0x70 });
        let hot = dos(if selected { 0x3e } else { 0x74 });
        let mut spans = vec![Span::styled(
            if settings.value(*setting) {
                "[x] "
            } else {
                "[ ] "
            },
            normal,
        )];
        spans.extend(label_spans(setting.label(), normal, hot));
        frame.render_widget(Paragraph::new(Line::from(spans)), row);
        app.mouse.regions.push((row, Target::Setting(index)));
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
                Paragraph::new(*label).style(dos(*color)),
                Rect::new(rect.x + 5, rect.y + 6 + index as u16, width - 10, 1),
            );
        }
    } else if settings.kind == SettingsKind::System {
        frame.render_widget(Paragraph::new("Existing destinations are always refused.\nTrash and permanent deletion require confirmation.").style(dos(0x70)), Rect::new(rect.x + 3, rect.y + 7, width - 6, 2));
    }
    frame.render_widget(
        Paragraph::new("Tab / ↑↓ choose · Space toggle · Enter OK · Esc Cancel").style(dos(0x70)),
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
            label,
            settings.cursor == count + index,
        );
        app.mouse.regions.push((button, *target));
    }
}
