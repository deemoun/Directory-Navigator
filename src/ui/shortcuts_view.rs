use crate::{
    app::App,
    focus::{self, Screen},
    mouse::Target,
    shortcuts,
};
use ratatui::{layout::Rect, widgets::Paragraph, Frame};

pub(super) fn draw_prefix(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    if area.width < 80 || area.height < 25 || !shortcuts::armed(app) {
        return;
    }
    let text = app
        .config
        .language
        .text(if focus::current(app) == crate::focus::Focus::UserMenu {
            shortcuts::USER_MENU_HELP
        } else {
            match focus::screen(app) {
                Screen::Editor => shortcuts::EDITOR_HELP,
                Screen::Viewer => shortcuts::VIEWER_HELP,
                _ => shortcuts::MANAGER_HELP,
            }
        });
    let height = (text.lines().count() as u16 + 5).min(area.height);
    let rect = super::dialog_style::window(
        frame,
        app.config.palette,
        area,
        74,
        height,
        app.config.language.text(" Keyboard commands "),
    );
    app.mouse.regions.clear();
    super::dialog_style::close_button(
        frame,
        app.config.palette,
        rect,
        &mut app.mouse.regions,
        Target::CloseDialog,
    );
    frame.render_widget(
        Paragraph::new(text).style(app.config.palette.dos(0x70)),
        Rect::new(rect.x + 2, rect.y + 2, rect.width - 4, rect.height - 4),
    );
}

pub(super) fn draw_settings(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(settings) = &app.settings else {
        return;
    };
    let profile = app.config.palette;
    let language = app.config.language;
    let rect = super::dialog_style::window(
        frame,
        profile,
        area,
        74,
        20,
        language.text(" Keyboard shortcuts "),
    );
    app.mouse.regions.clear();
    super::dialog_style::close_button(
        frame,
        profile,
        rect,
        &mut app.mouse.regions,
        Target::CancelSettings,
    );
    let choice = Rect::new(rect.x + 3, rect.y + 2, rect.width - 6, 1);
    frame.render_widget(
        Paragraph::new(format!(
            "{}: ◄ {} ►",
            language.text("Profile"),
            language.text(settings.draft.shortcuts.name())
        ))
        .style(profile.dos(if settings.cursor == 0 { 0x30 } else { 0x70 })),
        choice,
    );
    app.mouse.regions.push((choice, Target::ShortcutProfile));
    let text = if settings.draft.shortcuts.is_mac() {
        "^O means Ctrl+O, release, then the next key.\n1 Help · 2 User · 3 View · 4 Edit · 5 Copy\n6 Move · 7 MkDir · 8 Trash · 9 Menu · 0 Quit\ns Quick search · f Find · i Info · z Sort\nb Bookmarks · l Filter · h Commands · k Keyboard\nEditor: a Save As · w Close · r Replace · x Quit\nViewer: 2 Wrap · n Next · p Reverse · w Close\nThe prefix displays a guide for the current screen.\nClassic DN shortcuts remain available."
    } else {
        "Classic DN uses F1–F10 and Alt/Ctrl combinations.\nTab switches panels; Insert or Space marks entries.\nF3 View · F4 Edit · F5 Copy · F6 Move\nF7 MkDir · F8 Trash/delete · F9 Menu · F10 Quit\nEditor: F2 Save · F7 Find · F10 Menu\nMac-friendly adds Ctrl+O sequences without Fn/Option.\nDefault: Classic DN. Automatic: Mac on macOS, Classic elsewhere."
    };
    frame.render_widget(
        Paragraph::new(language.text(text)).style(profile.dos(0x70)),
        Rect::new(rect.x + 3, rect.y + 4, rect.width - 6, 10),
    );
    frame.render_widget(
        Paragraph::new(language.text("←/→ or Space changes · Tab/↑↓ chooses"))
            .style(profile.dos(0x70)),
        Rect::new(rect.x + 3, rect.bottom() - 5, rect.width - 6, 1),
    );
    for (i, (label, target)) in [
        ("[ OK ]", Target::AcceptSettings),
        ("[ Cancel ]", Target::CancelSettings),
    ]
    .into_iter()
    .enumerate()
    {
        let button = Rect::new(rect.x + 20 + i as u16 * 18, rect.bottom() - 3, 14, 1);
        super::dialog_style::button(
            frame,
            profile,
            button,
            language.text(label),
            settings.cursor == 1 + i,
        );
        app.mouse.regions.push((button, target));
    }
}
