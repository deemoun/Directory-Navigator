//! One definition for game page widgets, traversal and semantic mouse actions.
use crate::{
    presentation::layout::{Constraint, Layout, Rect},
    tetris::{GameAction as A, Screen},
    turbo::{
        dialog_spec::{Control, ControlKind, DialogSpec},
        widgets,
    },
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
fn button(
    s: &mut DialogSpec<'_>,
    ctx: &crate::plugins::Context,
    id: usize,
    r: Rect,
    label: &str,
    a: A,
    two_rows: bool,
) {
    let bounds = Rect::new(r.x, r.y, r.width, if two_rows { 2 } else { r.height });
    let mut c = Control::new(
        id,
        bounds,
        ControlKind::Button(ctx.language.text(label).into()),
        crate::tetris::target(a),
    );
    c.hits = vec![(r, crate::tetris::target(a))];
    s.control(c);
}
pub(crate) fn spec(
    g: &crate::tetris::Game,
    pending: bool,
    ctx: &crate::plugins::Context,
) -> DialogSpec<'static> {
    let area = ctx.area;
    let lang = ctx.language;
    let x = (area.width - 54) / 2;
    let y = (area.height - 23) / 2;
    let (focus, default, cancel) = match g.screen {
        Screen::Play => (
            g.play_focus,
            A::BoardFocus,
            if g.play_focus == 0 {
                A::Close
            } else {
                A::BoardFocus
            },
        ),
        Screen::Setup { .. } => (g.setup_focus, A::Apply, A::Back),
        Screen::Name => (g.page_focus, A::SubmitName, A::Back),
        Screen::GameOver => (0, A::Continue, A::Back),
        _ => (g.page_focus, A::Back, A::Back),
    };
    let mut s = ctx.dialog(area, focus, "", default.id(), cancel.id());
    let page = crate::tetris::layout::page(area, g, lang, pending);
    if g.screen != Screen::Play {
        // A game page must not expose the underlying manager's file-operation bar.
        let bar = Rect::new(0, area.height - 1, area.width, 1);
        s.native(widgets::function_status("", "", true), bar);
        if g.screen != Screen::Help {
            let help = Rect::new(0, bar.y, 16, 1);
            s.native(
                widgets::function_status("F1 ", lang.text("Help"), true),
                help,
            );
        }
        let back = Rect::new(area.width - 16, bar.y, 16, 1);
        let caption = if matches!(g.screen, Screen::Setup { .. } | Screen::Name) {
            "Cancel"
        } else {
            "Back"
        };
        s.native(
            widgets::function_status("Esc ", lang.text(caption), true),
            back,
        );
    }
    match g.screen {
        Screen::Play => {
            s.hit(Rect::new(x + 2, y, 3, 1), crate::tetris::target(A::Close));
            let mut board = Control::new(
                0,
                Rect::new(x + 2, y + 2, 20, 20),
                ControlKind::Surface,
                crate::tetris::target(A::BoardFocus),
            );
            board.paint = false;
            s.control(board);
            for (row, pair) in crate::tetris::BUTTONS.chunks(2).enumerate() {
                let mut left = x + 28;
                for (column, (label, action)) in pair.iter().enumerate() {
                    let width = crate::tetris::layout::button_width(lang.text(label)).max(11);
                    button(
                        &mut s,
                        ctx,
                        row * 2 + column + 1,
                        Rect::new(left, y + 17 + row as u16 * 2, width, 1),
                        label,
                        *action,
                        true,
                    );
                    left += width;
                }
            }
            let slots = Layout::horizontal([Constraint::Fill(1); 8]).split(Rect::new(
                0,
                area.height - 1,
                area.width,
                1,
            ));
            for (i, (label, a)) in [
                ("F1 Help", A::Help),
                ("F2 New", A::New),
                ("F3 Pause", A::Pause),
                ("F4 Scores", A::Scores),
                ("F5 Setup", A::Setup),
                ("+ Level", A::LevelUp),
                ("* Preview", A::Preview),
                ("Esc Close", A::Close),
            ]
            .into_iter()
            .enumerate()
            {
                let (key, text) = label.split_once(' ').unwrap();
                s.native(
                    widgets::function_status(&format!("{key} "), lang.text(text), true),
                    slots[i],
                );
                s.hit(slots[i], crate::tetris::target(a));
            }
        }
        Screen::Setup {
            level,
            mode,
            preview,
        } => {
            let r = page;
            let right = r.x + 30;
            let rw = r.width - 33;
            s.hit(
                Rect::new(r.x + 2, r.y, 3, 1),
                crate::tetris::target(A::Back),
            );
            for (i, label) in [
                "Baby",
                "Little fella",
                "Big child",
                "It's easy",
                "Never mind",
                "I'm powerful !",
                "Insanity coming",
                "So what ?..",
                "Madness",
                "Sanitarium",
            ]
            .into_iter()
            .enumerate()
            {
                let row = Rect::new(r.x + 3, r.y + 3 + i as u16, 26, 1);
                let number = if i == 9 {
                    "1~0~".into()
                } else {
                    format!("~{}~", i + 1)
                };
                let caption = format!("{number} - {}", lang.text(label));
                if level == i as u8 + 1 {
                    let mut c = Control::new(
                        0,
                        row,
                        ControlKind::Radio {
                            caption,
                            checked: true,
                        },
                        crate::tetris::target(A::SetLevel(i as u8 + 1)),
                    );
                    c.hits.clear();
                    s.control(c);
                } else {
                    s.native(widgets::radio(&caption, false, false), row);
                }
                s.hit(row, crate::tetris::target(A::SetLevel(i as u8 + 1)));
            }
            for (value, label, row) in [
                (crate::tetris::GameMode::Tetris, "Classic ~t~etris", 3),
                (crate::tetris::GameMode::Pentix, "Penti~x~", 4),
            ] {
                let bounds = Rect::new(right, r.y + row, rw, 1);
                let caption = lang.text(label).to_owned();
                if mode == value {
                    let mut c = Control::new(
                        1,
                        bounds,
                        ControlKind::Radio {
                            caption,
                            checked: true,
                        },
                        crate::tetris::target(A::SetMode(value)),
                    );
                    c.hits.clear();
                    s.control(c);
                } else {
                    s.native(widgets::radio(&caption, false, false), bounds);
                }
                s.hit(bounds, crate::tetris::target(A::SetMode(value)));
            }
            s.control(Control::new(
                2,
                Rect::new(right, r.y + 7, rw, 1),
                ControlKind::Checkbox {
                    caption: lang.text("~P~iece preview").into(),
                    checked: preview,
                },
                crate::tetris::target(A::Preview),
            ));
            let width = crate::tetris::layout::button_width(lang.text("Cancel")).max(10);
            button(
                &mut s,
                ctx,
                3,
                Rect::new(r.right() - width - 2, r.y + 10, width, 1),
                "OK",
                A::Apply,
                true,
            );
            button(
                &mut s,
                ctx,
                4,
                Rect::new(r.right() - width - 2, r.y + 12, width, 1),
                "Cancel",
                A::Back,
                true,
            );
        }
        Screen::Scores => {
            let r = page;
            s.hit(
                Rect::new(r.x + 2, r.y, 3, 1),
                crate::tetris::target(A::Back),
            );
            let retry = pending;
            if retry {
                button(
                    &mut s,
                    ctx,
                    1,
                    Rect::new(r.x + 8, r.bottom() - 3, 20, 1),
                    "Retry save",
                    A::RetrySave,
                    true,
                );
            }
            button(
                &mut s,
                ctx,
                0,
                Rect::new(r.x + if retry { 43 } else { 29 }, r.bottom() - 3, 14, 1),
                "OK",
                A::Back,
                true,
            );
        }
        screen => {
            let r = page;
            s.hit(
                Rect::new(r.x + 2, r.y, 3, 1),
                crate::tetris::target(A::Back),
            );
            if screen == Screen::Name {
                let bounds = Rect::new(r.x + 21, r.y + 2, r.width - 25, 1);
                let col = g.name[..g.name_cursor].width();
                let desired = col.saturating_sub(usize::from(bounds.width) - 1);
                let mut scroll = 0;
                let mut start = 0;
                for (byte, glyph) in g.name.grapheme_indices(true) {
                    if scroll >= desired {
                        break;
                    }
                    scroll += glyph.width();
                    start = byte + glyph.len();
                }
                let visible = g.name[start..].to_owned();
                let kind = if g.replace_name {
                    ControlKind::SelectedInput(visible)
                } else {
                    ControlKind::Input(visible)
                };
                s.control(Control::new(
                    0,
                    bounds,
                    kind,
                    crate::tetris::target(A::NameField),
                ));
                if g.page_focus == 0 {
                    s.cursor = Some((bounds.x + (col - scroll) as u16, bounds.y));
                }
                let buttons = [
                    (1, "OK", A::SubmitName),
                    (2, "Cancel", A::Back),
                    (3, "Help", A::Help),
                ];
                let widths = buttons.map(|(_, label, _)| {
                    crate::tetris::layout::button_width(lang.text(label)).max(10)
                });
                let mut left = r.right() - 3 - widths.iter().sum::<u16>();
                for ((id, label, action), width) in buttons.into_iter().zip(widths) {
                    button(
                        &mut s,
                        ctx,
                        id,
                        Rect::new(left, r.y + 4, width, 1),
                        label,
                        action,
                        true,
                    );
                    left += width;
                }
                let width = crate::tetris::layout::button_width(lang.text("Skip name")).max(16);
                button(
                    &mut s,
                    ctx,
                    4,
                    Rect::new(r.right() - width - 3, r.y + 6, width, 1),
                    "Skip name",
                    A::SkipName,
                    true,
                );
            } else {
                let label = if screen == Screen::GameOver {
                    "OK"
                } else {
                    "Back"
                };
                let width = crate::tetris::layout::button_width(lang.text(label)).max(12);
                button(
                    &mut s,
                    ctx,
                    0,
                    Rect::new(r.x + (r.width - width) / 2, r.bottom() - 3, width, 1),
                    label,
                    if screen == Screen::GameOver {
                        A::Continue
                    } else {
                        A::Back
                    },
                    true,
                );
            }
        }
    }
    if g.screen != Screen::Play {
        if g.screen != Screen::Help {
            s.hit(
                Rect::new(0, area.height - 1, 16, 1),
                crate::tetris::target(A::Help),
            );
        }
        s.hit(
            Rect::new(area.width - 16, area.height - 1, 16, 1),
            crate::tetris::target(A::Back),
        );
    }
    s
}
