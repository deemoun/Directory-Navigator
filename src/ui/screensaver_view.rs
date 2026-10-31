use crate::dialog_theme as theme;
use crate::painting::{Frame, Paragraph};
use crate::presentation::layout::Rect;
use crate::turbo::dialog_spec::{scope, Control, ControlKind, DialogSpec};
use crate::{app::App, mouse::Target, screensaver::*};

pub(super) fn draw_setup(frame: &mut impl Frame, app: &App, area: Rect) {
    if let Some(s) = saver_spec(app, area) {
        s.paint(frame, app);
    }
}
pub(crate) fn saver_spec(app: &App, area: Rect) -> Option<DialogSpec<'_>> {
    let s = app.screensaver.modals.screensaver()?;
    let dos = app
        .config
        .palette
        .styles_for(crate::palette::theme::Group::Dialogs);
    let rect = crate::dialog_theme::centered(area, 76, 23);
    let mut spec = DialogSpec::new(
        rect,
        scope(app, crate::focus::Focus::SaverSetup)?,
        s.cursor,
        app.config.language.text(" Screen savers "),
        Target::Screensaver(ACCEPT),
        Target::Screensaver(CANCEL),
    );
    for (id, x, label, items, row, action) in [
        (
            SELECTED,
            3,
            "Selected savers",
            s.selected
                .iter()
                .map(|kind| app.config.language.text(kind.label()).to_owned())
                .collect(),
            s.selected_row,
            SELECTED_ROW,
        ),
        (
            AVAILABLE,
            51,
            "Available savers",
            Kind::ALL
                .iter()
                .map(|kind| app.config.language.text(kind.label()).to_owned())
                .collect::<Vec<_>>(),
            s.available_row,
            AVAILABLE_ROW,
        ),
    ] {
        spec.caption(
            Paragraph::new(app.config.language.text(label)).style(dos(theme::TEXT)),
            Rect::new(rect.x + x, rect.y + 2, 22, 1),
        );
        let mut control = Control::new(
            id,
            Rect::new(rect.x + x, rect.y + 3, 22, 4),
            ControlKind::List {
                items,
                cursor: row,
                offset: 0,
            },
            Target::Screensaver(id),
        );
        control.hits.clear();
        spec.control(control);
        let count = if id == SELECTED {
            s.selected.len()
        } else {
            Kind::ALL.len()
        };
        for i in 0..4 {
            spec.hit(
                Rect::new(rect.x + x, rect.y + 3 + i as u16, 22, 1),
                Target::Screensaver(if i < count { action + i } else { id }),
            );
        }
    }
    for (id, y, label, enabled) in [
        (
            ADD,
            3,
            "[ Add ]",
            !s.selected.contains(&Kind::ALL[s.available_row]),
        ),
        (REMOVE, 6, "[ Remove ]", !s.selected.is_empty()),
    ] {
        let mut control = Control::new(
            id,
            Rect::new(rect.x + 28, rect.y + y, 20, 1),
            ControlKind::Button(app.config.language.text(label).into()),
            Target::Screensaver(id),
        );
        control.enabled = enabled;
        spec.control(control);
    }
    spec.caption(
        Paragraph::new(app.config.language.text("Time before screen rest")).style(dos(theme::TEXT)),
        Rect::new(rect.x + 3, rect.y + 9, 68, 1),
    );
    for (i, minutes) in DELAYS.iter().enumerate() {
        let label = if *minutes == 0 {
            app.config.language.text("Never").to_string()
        } else {
            format!(
                "{minutes} {}",
                app.config
                    .language
                    .text(if *minutes == 1 { "minute" } else { "minutes" })
            )
        };
        spec.control(Control::new(
            i,
            Rect::new(
                rect.x + 4 + (i / 4) as u16 * 32,
                rect.y + 10 + (i % 4) as u16,
                30,
                1,
            ),
            ControlKind::Radio {
                caption: label,
                checked: s.delay == *minutes,
            },
            Target::Screensaver(i),
        ));
    }
    spec.caption(
        Paragraph::new(
            app.config
                .language
                .text("Randomly chosen from Selected savers"),
        )
        .style(dos(theme::TEXT)),
        Rect::new(rect.x + 3, rect.y + 15, 70, 1),
    );
    spec.caption(
        Paragraph::new(
            app.config
                .language
                .text("Any key or mouse input returns to DN"),
        )
        .style(dos(theme::TEXT)),
        Rect::new(rect.x + 3, rect.y + 17, 70, 1),
    );
    for (id, x, width, label) in [
        (PREVIEW, 3, 17, "[ Preview ]"),
        (ACCEPT, 23, 12, "[ OK ]"),
        (CANCEL, 39, 16, "[ Cancel ]"),
        (HELP, 58, 15, "[ Help ]"),
    ] {
        spec.control(Control::new(
            id,
            Rect::new(rect.x + x, rect.bottom() - 3, width, 1),
            ControlKind::Button(app.config.language.text(label).into()),
            Target::Screensaver(id),
        ));
    }
    spec.tab_order = [
        vec![SELECTED, AVAILABLE, ADD, REMOVE],
        (0..DELAYS.len()).collect(),
        vec![PREVIEW, ACCEPT, HELP, CANCEL],
    ]
    .concat();
    Some(spec)
}
