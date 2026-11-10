use crate::dialog_theme as theme;
use crate::turbo::dialog_spec::{scope, Control, ControlKind, DialogSpec};

use crate::painting::{Paragraph, Wrap};
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
    let dos = profile.styles_for(crate::palette::theme::Group::Dialogs);
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
        Action::Close if app.utility_dialog() == Some(Kind::Characters) => 4,
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
    let dos = profile.styles_for(crate::palette::theme::Group::Dialogs);
    let kind = app.utility_dialog()?;
    if kind == Kind::Calculator {
        return None;
    }
    let height = match kind {
        Kind::Characters => 24,
        Kind::Calculator => 22,
        _ => area.height.saturating_sub(4).min(24),
    };
    let width = area.width.saturating_sub(4).min(100);
    let rect = crate::tool_windows::bounds(app, area, width, height);
    let title = match kind {
        Kind::Calculator => " Calculator ",
        Kind::Characters => " ASCII / DOS CP437 Table ",
        Kind::System => " System Information ",
        Kind::Memory => " Memory Information ",
        Kind::Network => " Network Information ",
    };
    let focus = if kind == Kind::Characters {
        app.utility_state().chart.focus
    } else {
        app.utility_state().focus
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
            let page = app.utility_state().chart.page;
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
            let chart = &app.utility_state().chart;
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
            let note = if !app.utility_state().notice.is_empty() {
                app.utility_state().notice.as_str()
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
        Kind::System | Kind::Memory | Kind::Network => {
            if let Some(info) = &app.utility_state().information {
                if info.loading() {
                    text(
                        &mut spec,
                        profile,
                        Rect::new(x, rect.y + if kind == Kind::Network { 4 } else { 2 }, w, 1),
                        app.config.language.text("Loading native information…"),
                        false,
                    );
                }
                if kind == Kind::Network {
                    text(
                        &mut spec,
                        profile,
                        Rect::new(x, rect.y + 1, w, 1),
                        app.config.language.text("Network interface"),
                        false,
                    );
                    let label = info
                        .network
                        .as_ref()
                        .and_then(|n| n.adapters.get(info.selected_interface))
                        .map_or_else(
                            || app.config.language.text("Unavailable").to_owned(),
                            |a| display_name(&a.label),
                        );
                    let mut selector = Control::new(
                        2,
                        Rect::new(x, rect.y + 2, w, 1),
                        ControlKind::Choice(vec![label]),
                        Target::Utility(Action::InterfaceOpen),
                    );
                    selector.enabled = info
                        .network
                        .as_ref()
                        .is_some_and(|n| !n.adapters.is_empty());
                    selector.enter_action = true;
                    spec.control(selector);
                    let (download, upload) = match info.traffic.state() {
                        crate::network::traffic::State::Ready { download, upload } => (
                            crate::network::traffic::format_rate(*download),
                            crate::network::traffic::format_rate(*upload),
                        ),
                        state => {
                            let label = app.config.language.text(
                                if matches!(state, crate::network::traffic::State::Measuring) {
                                    "Measuring…"
                                } else {
                                    "Unavailable"
                                },
                            );
                            (label.to_owned(), label.to_owned())
                        }
                    };
                    let half = w / 2;
                    for (column, label, value) in
                        [(x, "Download", download), (x + half, "Upload", upload)]
                    {
                        text(
                            &mut spec,
                            profile,
                            Rect::new(column, rect.y + 3, half, 1),
                            &format!("{}: {}", app.config.language.text(label), value),
                            false,
                        );
                    }
                }
                for (index, (name, value)) in info
                    .fields
                    .iter()
                    .enumerate()
                    .skip(info.offset)
                    .take(info.rows)
                {
                    let value = crate::utilities::information::display_value(
                        app.config.language,
                        name,
                        value,
                    );
                    spec.caption(
                        Paragraph::new(format!(
                            "{}: {}",
                            app.config.language.text(name),
                            display_name(&value)
                        ))
                        .scroll((0, info.horizontal))
                        .style(dos(theme::TEXT)),
                        Rect::new(
                            x,
                            rect.y
                                + if kind == Kind::Network { 4 } else { 1 }
                                + (index - info.offset) as u16,
                            w,
                            1,
                        ),
                    );
                }
            }
            text(
                &mut spec,
                profile,
                Rect::new(x, rect.bottom() - 4, w, 1),
                app.config.language.text(if kind == Kind::Network {
                    "Live traffic · R/F5 Refresh details · ↑↓ scroll · ←→ pan"
                } else {
                    "Snapshot · R/F5 Refresh · ↑↓ PgUp/PgDn scroll · ←→ pan"
                }),
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
    if let Some(info) = &app.utility_state().information {
        if let (Some(choice), Some(network)) = (info.interface_choice, &info.network) {
            let rows = usize::from(rect.height.saturating_sub(9))
                .clamp(1, 8)
                .min(network.adapters.len());
            let offset = choice.saturating_sub(rows.saturating_sub(1));
            let bounds = Rect::new(x, rect.y + 3, w, rows as u16 + 2);
            for control in &mut spec.controls {
                control.enabled = false;
            }
            spec.native(crate::turbo::widgets::dialog(""), bounds);
            let list_bounds = Rect::new(x + 1, rect.y + 4, w.saturating_sub(2), rows as u16);
            let mut list = Control::new(
                3,
                list_bounds,
                ControlKind::List {
                    items: network
                        .adapters
                        .iter()
                        .map(|a| display_name(&a.label))
                        .collect(),
                    cursor: choice,
                    offset,
                },
                Target::Utility(Action::InterfaceAccept),
            );
            list.dn_list = true;
            list.enter_action = true;
            list.hits = (0..rows)
                .map(|row| {
                    (
                        Rect::new(
                            list_bounds.x,
                            list_bounds.y + row as u16,
                            list_bounds.width,
                            1,
                        ),
                        Target::Utility(Action::InterfacePick(info.interface_epoch, offset + row)),
                    )
                })
                .collect();
            spec.control(list);
        }
    }
    Some(spec)
}
