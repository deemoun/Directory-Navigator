use crate::painting::{Frame, Paragraph, Wrap};
use crate::presentation::layout::Rect;
use crate::{
    app::App,
    filesystem::display_name,
    mouse::Target,
    utilities::{characters::Page, Action, Kind},
};
fn text(
    frame: &mut impl Frame,
    profile: crate::palette::PaletteProfile,
    rect: Rect,
    value: &str,
    focused: bool,
) {
    let dos = profile.styles();
    frame.render_widget(
        Paragraph::new(value)
            .style(dos(if focused { 0x30 } else { 0x70 }))
            .wrap(Wrap { trim: false }),
        rect,
    );
}
fn button(
    frame: &mut impl Frame,
    app: &mut App,
    rect: Rect,
    label: &str,
    action: Action,
    focused: bool,
) {
    let profile = app.config.palette;
    super::dialog_style::button(
        frame,
        profile,
        rect,
        app.config.language.text(label),
        focused,
    );
    app.mouse.regions.push((rect, Target::Utility(action)));
}
pub(super) fn draw(frame: &mut impl Frame, app: &mut App, area: Rect) {
    let profile = app.config.palette;
    let dos = profile.styles();
    let Some(kind) = app.utilities.dialog else {
        return;
    };
    if kind == Kind::Calculator {
        super::calculator_view::draw(frame, app, area);
        return;
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
    super::dialog_style::window(
        frame,
        profile,
        rect,
        rect.width,
        rect.height,
        app.config.language.text(title),
    );
    app.mouse.regions.clear();
    super::dialog_style::close_button(
        frame,
        profile,
        rect,
        &mut app.mouse.regions,
        Target::Utility(Action::Close),
    );
    let x = rect.x + 2;
    let w = rect.width - 4;
    match kind {
        Kind::Calculator => {}
        Kind::Characters => {
            let page = app.utilities.chart.page;
            let focus = app.utilities.chart.focus;
            button(
                frame,
                app,
                Rect::new(x, rect.y + 1, 18, 1),
                " [ ASCII ] ",
                Action::Page(Page::Ascii),
                page == Page::Ascii || focus == 1,
            );
            button(
                frame,
                app,
                Rect::new(x + 20, rect.y + 1, 22, 1),
                " [ DOS CP437 ] ",
                Action::Page(Page::Cp437),
                page == Page::Cp437 || focus == 2,
            );
            for col in 0..16u16 {
                text(
                    frame,
                    profile,
                    Rect::new(x + 4 + col * 4, rect.y + 2, 4, 1),
                    &format!(" {col:X} "),
                    false,
                );
            }
            let chart = &app.utilities.chart;
            let selected = chart.selected();
            for row in 0..if page == Page::Ascii { 8 } else { 16 } {
                text(
                    frame,
                    profile,
                    Rect::new(x, rect.y + 3 + row, 4, 1),
                    &format!("{row:X}_"),
                    false,
                );
                for col in 0..16u16 {
                    let byte = (row * 16 + col) as u8;
                    let c = chart.mapped(byte);
                    let cell = Rect::new(x + 4 + col * 4, rect.y + 3 + row, 4, 1);
                    frame.render_widget(
                        Paragraph::new(format!(" {} ", if c.is_control() { '·' } else { c }))
                            .style(dos(if selected == byte { 0x30 } else { 0x1f })),
                        cell,
                    );
                    app.mouse
                        .regions
                        .push((cell, Target::Utility(Action::Cell(byte))));
                }
            }
            let detail = chart.details();
            let copyable = chart.copy_text().is_some();
            text(
                frame,
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
            text(frame, profile, Rect::new(x, rect.y + 20, w, 1), note, false);
            if copyable {
                button(
                    frame,
                    app,
                    Rect::new(x + 10, rect.bottom() - 2, 16, 1),
                    " [ Copy ] ",
                    Action::Copy,
                    focus == 3,
                );
            } else {
                text(
                    frame,
                    profile,
                    Rect::new(x + 10, rect.bottom() - 2, 16, 1),
                    app.config.language.text(" Copy disabled "),
                    false,
                );
            }
            button(
                frame,
                app,
                Rect::new(rect.right() - 24, rect.bottom() - 2, 16, 1),
                " [ Close ] ",
                Action::Close,
                focus == 4,
            );
        }
        Kind::System | Kind::Memory => {
            if let Some(info) = &app.utilities.information {
                if info.loading() {
                    text(
                        frame,
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
                    frame.render_widget(
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
                frame,
                profile,
                Rect::new(x, rect.bottom() - 4, w, 1),
                app.config
                    .language
                    .text("Snapshot · R/F5 Refresh · ↑↓ PgUp/PgDn scroll · ←→ pan"),
                false,
            );
            let focus = app.utilities.focus;
            button(
                frame,
                app,
                Rect::new(x + 8, rect.bottom() - 2, 16, 1),
                " [ Refresh ] ",
                Action::Refresh,
                focus == 0,
            );
            button(
                frame,
                app,
                Rect::new(rect.right() - 24, rect.bottom() - 2, 16, 1),
                " [ Close ] ",
                Action::Close,
                focus == 1,
            );
        }
    }
}
