use crate::{
    app::App,
    filesystem::display_name,
    mouse::Target,
    utilities::{characters::Page, Action, Kind},
};
use ratatui::{
    layout::Rect,
    text::Line,
    widgets::{Paragraph, Wrap},
    Frame,
};
fn text(
    frame: &mut Frame,
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
    frame: &mut Frame,
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
pub(super) fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    let profile = app.config.palette;
    let dos = profile.styles();
    let Some(kind) = app.utilities.dialog else {
        return;
    };
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
        Kind::Calculator => {
            let calc = &app.utilities.calculator;
            let focus = calc.focus;
            text(
                frame,
                profile,
                Rect::new(x, rect.y + 1, w, 1),
                app.config
                    .language
                    .text("Expression · + - * / % ^ · sqrt() abs() · 0x / 0b / 0o"),
                false,
            );
            let input = Rect::new(x, rect.y + 2, w, 1);
            let column = Line::from(&calc.expression[..calc.cursor]).width();
            let scroll = column.saturating_sub(usize::from(w.saturating_sub(1)));
            frame.render_widget(
                Paragraph::new(display_name(&calc.expression))
                    .scroll((0, scroll as u16))
                    .style(super::dialog_style::input_style(profile, focus == 0)),
                input,
            );
            app.mouse
                .regions
                .push((input, Target::Utility(Action::Field)));
            if focus == 0 {
                frame.set_cursor_position((input.x + (column - scroll) as u16, input.y));
            }
            text(
                frame,
                profile,
                Rect::new(x, rect.y + 3, w, 1),
                app.config
                    .language
                    .text("Up/Down history · Enter evaluates · Tab moves · Esc closes"),
                false,
            );
            let result = calc.result.map(|v| v.representations());
            let status = calc.error.clone().unwrap_or_else(|| {
                if calc.result.is_some() {
                    "Result · Select a representation to copy".into()
                } else {
                    "Enter an expression, then Evaluate".into()
                }
            });
            text(
                frame,
                profile,
                Rect::new(x, rect.y + 4, w, 2),
                &status,
                false,
            );
            for (index, label) in ["DEC", "HEX", "BIN", "OCT"].iter().enumerate() {
                let value = result
                    .as_ref()
                    .map(|values| {
                        values[index]
                            .as_ref()
                            .map(String::as_str)
                            .unwrap_or_else(|e| e.as_str())
                    })
                    .unwrap_or("—");
                let row = Rect::new(x, rect.y + 6 + index as u16 * 2, w, 2);
                text(
                    frame,
                    profile,
                    row,
                    &format!(
                        "{} {label}: {value}",
                        if calc.base == index { '●' } else { ' ' }
                    ),
                    focus == index + 1,
                );
                app.mouse
                    .regions
                    .push((row, Target::Utility(Action::Base(index))));
            }
            text(
                frame,
                profile,
                Rect::new(x, rect.y + 15, w, 2),
                &app.utilities.notice,
                false,
            );
            text(
                frame,
                profile,
                Rect::new(x, rect.y + 18, w, 1),
                app.config
                    .language
                    .text("Copy uses DN's internal clipboard · Ctrl+C copies"),
                false,
            );
            button(
                frame,
                app,
                Rect::new(x + 2, rect.bottom() - 2, 16, 1),
                " [ Evaluate ] ",
                Action::Evaluate,
                focus == 5,
            );
            button(
                frame,
                app,
                Rect::new(x + 25, rect.bottom() - 2, 14, 1),
                " [ Copy ] ",
                Action::Copy,
                focus == 6,
            );
            button(
                frame,
                app,
                Rect::new(rect.right() - 20, rect.bottom() - 2, 14, 1),
                " [ Close ] ",
                Action::Close,
                focus == 7,
            );
        }
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
