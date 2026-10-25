use crate::painting::{Frame, Paragraph};
use crate::{app::App, focus::Screen, mouse::Target, shortcuts, welcome};
use ratatui::{layout::Rect, text::Line};

pub(super) fn draw(frame: &mut impl Frame, app: &mut App, area: Rect) {
    let Some(welcome) = &app.welcome.dialog else {
        return;
    };
    let profile = app.config.palette;
    let language = app.config.language;
    let rect = super::dialog_style::window(
        frame,
        profile,
        area,
        72,
        20,
        language.text(" Welcome to Directory Navigator "),
    );
    app.mouse.regions.clear();
    super::dialog_style::close_button(
        frame,
        profile,
        rect,
        &mut app.mouse.regions,
        Target::Welcome(welcome::Action::Start),
    );
    let x = rect.x + 3;
    let width = rect.width - 6;
    let key = |classic| shortcuts::label(app.config.shortcuts, Screen::Manager, classic);
    let lines = [
        (
            2,
            format!("{} {}", crate::APPLICATION_NAME, crate::DISPLAY_VERSION),
            0x7f,
        ),
        (
            3,
            language
                .text("Native Rust terminal file manager")
                .to_owned(),
            0x70,
        ),
        (
            5,
            language
                .text("Arrows navigate; Tab switches panels.")
                .to_owned(),
            0x70,
        ),
        (
            6,
            language
                .text("Enter opens folders, archives or associated applications.")
                .to_owned(),
            0x70,
        ),
        (
            7,
            language
                .text(if app.config.space_select {
                    "Space / Insert marks entries."
                } else {
                    "Insert marks entries; Space marking is disabled."
                })
                .to_owned(),
            0x70,
        ),
        (
            8,
            format!(
                "{} {} · {} {}",
                key("F3"),
                language.text("Viewer"),
                key("F4"),
                language.text("Editor")
            ),
            0x70,
        ),
        (
            10,
            format!(
                "{} {} · {} {} · {} {}",
                key("F5"),
                language.text("Copy"),
                key("F6"),
                language.text("Move"),
                key("F7"),
                language.text("Make directory")
            ),
            0x70,
        ),
        (
            11,
            format!(
                "{} {} · {}",
                key("F8"),
                language.text(if app.config.use_trash {
                    "Trash"
                } else {
                    "Delete"
                }),
                language.text(if app.config.use_trash {
                    "Deletion mode: Trash"
                } else {
                    "Deletion mode: permanent deletion"
                })
            ),
            0x70,
        ),
        (
            12,
            language
                .text("Destinations and deletion require confirmation.")
                .to_owned(),
            0x70,
        ),
        (
            14,
            language
                .text(if app.config.shortcuts.is_mac() {
                    "Mac: press Ctrl+O, release, then another key."
                } else {
                    "Classic keys: F1–F10 and Alt/Ctrl combinations."
                })
                .to_owned(),
            0x70,
        ),
        (
            15,
            language
                .text(if app.config.shortcuts.is_mac() {
                    "Ctrl+O displays a guide; Esc cancels the prefix."
                } else {
                    "F1 opens help; keyboard settings offer a Mac-friendly profile."
                })
                .to_owned(),
            0x70,
        ),
    ];
    for (row, text, attribute) in lines {
        frame.render_widget(
            Paragraph::new(Line::from(text)).style(profile.dos(attribute)),
            Rect::new(x, rect.y + row, width, 1),
        );
    }
    for (i, (label, action)) in welcome::BUTTONS.into_iter().enumerate() {
        let button = Rect::new(x + i as u16 * 22, rect.bottom() - 3, 21, 1);
        super::dialog_style::button(
            frame,
            profile,
            button,
            &crate::menu::plain(language.text(label)),
            welcome.focus == i,
        );
        app.mouse.regions.push((button, Target::Welcome(action)));
    }
}
