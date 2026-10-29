use crate::dialog_theme as theme;
use crate::painting::{Frame, Paragraph};
use crate::presentation::layout::Rect;
use crate::turbo::dialog_spec::{centered, scope, Control, ControlKind, DialogSpec};
use crate::{
    app::App,
    focus::{self, Screen},
    mouse::Target,
    shortcuts,
};

pub(super) fn draw_prefix(frame: &mut impl Frame, app: &App) {
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
    frame.regions().clear();
    super::dialog_style::close_button(frame, app.config.palette, rect, Target::CloseDialog);
    frame.render_widget(
        Paragraph::new(text).style(app.config.palette.dos(theme::TEXT)),
        Rect::new(rect.x + 2, rect.y + 2, rect.width - 4, rect.height - 4),
    );
}

pub(super) fn settings_spec(app: &App, area: Rect) -> Option<DialogSpec<'_>> {
    let settings = app.modals.settings()?;
    let profile = app.config.palette;
    let language = app.config.language;
    let form = settings.kind.definition();
    let rect = centered(area, form.width, form.height);
    let mut spec = DialogSpec::new(
        rect,
        scope(app, crate::focus::Focus::Settings)?,
        settings.cursor,
        language.text(" Keyboard shortcuts "),
        Target::AcceptSettings,
        Target::CancelSettings,
    );
    let choice = Rect::new(rect.x + 3, rect.y + 2, rect.width - 6, 1);
    spec.caption(
        Paragraph::new(language.text(settings.choice_fields()[0].definition().caption))
            .style(profile.dos(theme::TEXT)),
        Rect::new(choice.x, choice.y, 12, 1),
    );
    let field = Rect::new(choice.x + 12, choice.y, choice.width - 12, 1);
    let mut control = Control::new(
        0,
        field,
        ControlKind::Choice(crate::settings::choice_items(settings, 0, language)),
        Target::ShortcutProfile,
    );
    control.hits = vec![
        (choice, control.action),
        (
            Rect::new(field.right() - 2, field.y, 2, 1),
            Target::OpenSettingsChoice(0),
        ),
    ];
    spec.control(control);
    let text = if settings.draft.shortcuts.is_mac() {
        "^O means Ctrl+O, release, then the next key.\n1 Help · 2 User · 3 View · 4 Edit · 5 Copy\n6 Move · 7 MkDir · 8 Trash · 9 Menu · 0 Quit\ns Quick search · f Find · i Info · z Sort\nb Bookmarks · l Filter · h Commands · k Keyboard\nEditor: a Save As · w Close · r Replace · x Quit\nViewer: 2 Wrap · n Next · p Reverse · w Close\nThe prefix displays a guide for the current screen.\nClassic DN shortcuts remain available."
    } else {
        "Classic DN uses F1–F10 and Alt/Ctrl combinations.\nTab switches panels; Insert or Space marks entries.\nF3 View · F4 Edit · F5 Copy · F6 Move\nF7 MkDir · F8 Trash/delete · F9 Menu · F10 Quit\nEditor: F2 Save · F7 Find · F10 Menu\nMac-friendly adds Ctrl+O sequences without Fn/Option.\nDefault: Classic DN. Automatic: Mac on macOS, Classic elsewhere."
    };
    spec.caption(
        Paragraph::new(language.text(text)).style(profile.dos(theme::TEXT)),
        Rect::new(rect.x + 3, rect.y + 4, rect.width - 6, 10),
    );
    spec.caption(
        Paragraph::new(language.text("←/→ or Space changes · Tab/↑↓ chooses"))
            .style(profile.dos(theme::TEXT)),
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
        spec.control(Control::new(
            1 + i,
            button,
            ControlKind::Button(language.text(label).into()),
            target,
        ));
    }
    Some(spec)
}
