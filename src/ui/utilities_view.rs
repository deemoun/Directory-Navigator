use crate::turbo::dialog_spec::{scope, Control, ControlKind, DialogSpec};
pub(super) fn draw(frame: &mut impl Frame, app: &App, area: Rect) {
    if let Some(s) = utility_spec(app, area) {
        s.paint(frame, app);
    }
}
use crate::painting::{Frame, Paragraph, Wrap};
use crate::presentation::layout::Rect;
use crate::{
    app::App,
    filesystem::display_name,
    mouse::Target,
    utilities::{characters::Page, Action, Kind},
};
fn text(
    spec: &mut DialogSpec<'_>,
    profile: crate::palette::PaletteProfile,
    rect: Rect,
    value: &str,
    focused: bool,
) {
    let dos = profile.styles();
    spec.caption(
        Paragraph::new(value.to_owned())
            .style(dos(if focused { 0x30 } else { 0x70 }))
            .wrap(Wrap { trim: false }),
        rect,
    );
}
fn button(spec: &mut DialogSpec<'_>, app: &App, rect: Rect, label: &str, action: Action) {
    let id = match action {
        Action::Page(Page::Ascii) => 1,
        Action::Page(Page::Cp437) => 2,
        Action::Copy => 3,
        Action::Close if app.utilities.modals.utility_dialog() == Some(Kind::Characters) => 4,
        Action::Close => 1,
        _ => 0,
    };
    spec.control(Control::new(
        id,
        rect,
        ControlKind::Button(app.config.language.text(label).into()),
        Target::Utility(action),
    ));
}
pub(crate) fn utility_spec(app: &App, area: Rect) -> Option<DialogSpec<'_>> {
    let profile = app.config.palette;
    let dos = profile.styles();
    let kind = app.utilities.modals.utility_dialog()?;
    if kind == Kind::Calculator {
        return super::calculator_view::calculator_spec(app, area);
    }
    let height = match kind {
        Kind::Characters => 24,
        Kind::Calculator => 22,
        _ => area.height.saturating_sub(4).min(24),
    };
    let width = area.width.saturating_sub(4).min(100);
    let rect = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    let title = match kind {
        Kind::Calculator => " Calculator ",
        Kind::Characters => " ASCII / DOS CP437 Table ",
        Kind::System => " System Information ",
        Kind::Memory => " Memory Information ",
    };
    let focus = if kind == Kind::Characters {
        app.utilities.chart.focus
    } else {
        app.utilities.focus
    };
    let default = if kind == Kind::Characters {
        Action::Copy
    } else {
        Action::Refresh
    };
    let mut spec = DialogSpec::new(
        rect,
        scope(app, crate::focus::Focus::Utility)?,
        focus,
        app.config.language.text(title),
        Target::Utility(default),
        Target::Utility(Action::Close),
    );
    if kind == Kind::Characters {
        spec.tab_order = vec![0, 1, 2, 3, 4];
    }
    let x = rect.x + 2;
    let w = rect.width - 4;
    match kind {
        Kind::Calculator => {}
        Kind::Characters => {
            let page = app.utilities.chart.page;
            button(
                &mut spec,
                app,
                Rect::new(x, rect.y + 1, 18, 1),
                " [ ASCII ] ",
                Action::Page(Page::Ascii),
            );
            button(
                &mut spec,
                app,
                Rect::new(x + 20, rect.y + 1, 22, 1),
                " [ DOS CP437 ] ",
                Action::Page(Page::Cp437),
            );
            for col in 0..16u16 {
                text(
                    &mut spec,
                    profile,
                    Rect::new(x + 4 + col * 4, rect.y + 2, 4, 1),
                    &format!(" {col:X} "),
                    false,
                );
            }
            let chart = &app.utilities.chart;
            for row in 0..if page == Page::Ascii { 8 } else { 16 } {
                text(
                    &mut spec,
                    profile,
                    Rect::new(x, rect.y + 3 + row, 4, 1),
                    &format!("{row:X}_"),
                    false,
                );
            }
            let grid = Rect::new(
                x + 4,
                rect.y + 3,
                64,
                if page == Page::Ascii { 8 } else { 16 },
            );
            let mut control = Control::new(
                0,
                grid,
                ControlKind::CharacterGrid {
                    page,
                    cells: chart.cells,
                },
                Target::Utility(Action::Copy),
            );
            control.paint = false;
            control.hits.clear();
            spec.control(control);
            spec.scene(crate::turbo::characters::draw(
                chart,
                grid,
                crate::presentation::layout::Size::new(area.width, area.height),
                profile,
            ));
            let detail = chart.details();
            let copyable = chart.copy_text().is_some();
            text(
                &mut spec,
                profile,
                Rect::new(x, rect.y + 19, w, 1),
                &detail,
                false,
            );
            let note = if !app.utilities.notice.is_empty() {
                app.utilities.notice.as_str()
            } else if page == Page::Cp437 {
                "DOS font glyphs at 01–1F / 7F; not control-code decoding"
            } else {
                "ASCII controls are shown as dots and cannot be copied"
            };
            text(
                &mut spec,
                profile,
                Rect::new(x, rect.y + 20, w, 1),
                note,
                false,
            );
            if copyable {
                button(
                    &mut spec,
                    app,
                    Rect::new(x + 10, rect.bottom() - 2, 16, 1),
                    " [ Copy ] ",
                    Action::Copy,
                );
            } else {
                let mut disabled = Control::new(
                    3,
                    Rect::new(x + 10, rect.bottom() - 2, 16, 1),
                    ControlKind::Button("Copy".into()),
                    Target::Utility(Action::Copy),
                );
                disabled.enabled = false;
                disabled.paint = false;
                spec.control(disabled);
                text(
                    &mut spec,
                    profile,
                    Rect::new(x + 10, rect.bottom() - 2, 16, 1),
                    app.config.language.text(" Copy disabled "),
                    false,
                );
            }
            button(
                &mut spec,
                app,
                Rect::new(rect.right() - 24, rect.bottom() - 2, 16, 1),
                " [ Close ] ",
                Action::Close,
            );
        }
        Kind::System | Kind::Memory => {
            if let Some(info) = &app.utilities.information {
                if info.loading() {
                    text(
                        &mut spec,
                        profile,
                        Rect::new(x, rect.y + 2, w, 1),
                        app.config.language.text("Loading native information…"),
                        false,
                    );
                }
                for (index, (name, value)) in info
                    .fields
                    .iter()
                    .enumerate()
                    .skip(info.offset)
                    .take(info.rows)
                {
                    spec.caption(
                        Paragraph::new(format!(
                            "{}: {}",
                            app.config.language.text(name),
                            display_name(value)
                        ))
                        .scroll((0, info.horizontal))
                        .style(dos(0x70)),
                        Rect::new(x, rect.y + 1 + (index - info.offset) as u16, w, 1),
                    );
                }
            }
            text(
                &mut spec,
                profile,
                Rect::new(x, rect.bottom() - 4, w, 1),
                app.config
                    .language
                    .text("Snapshot · R/F5 Refresh · ↑↓ PgUp/PgDn scroll · ←→ pan"),
                false,
            );
            button(
                &mut spec,
                app,
                Rect::new(x + 8, rect.bottom() - 2, 16, 1),
                " [ Refresh ] ",
                Action::Refresh,
            );
            button(
                &mut spec,
                app,
                Rect::new(rect.right() - 24, rect.bottom() - 2, 16, 1),
                " [ Close ] ",
                Action::Close,
            );
        }
    }
    Some(spec)
}
