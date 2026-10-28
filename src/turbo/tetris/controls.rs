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
        ctx.target(a.id()),
    );
    c.hits = vec![(r, ctx.target(a.id()))];
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
            for (i, (label, a)) in crate::tetris::BUTTONS.into_iter().enumerate() {
                button(
                    &mut s,
                    ctx,
                    i + 1,
                    Rect::new(
                        x + 28 + (i as u16 % 2) * 11,
                        y + 18 + (i as u16 / 2) * 2,
                        11,
                        1,
                    ),
                    label,
                    a,
                    true,
                );
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
                s.hit(slots[i], ctx.target(a.id()));
            }
        }
        Screen::Setup { level, preview } => {
            let r = Rect::new((area.width - 72) / 2, (area.height - 18) / 2, 72, 18);
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
                let row = Rect::new(r.x + 4, r.y + 3 + i as u16, 35, 1);
                let caption = format!("{:>2} — {}", i + 1, lang.text(label));
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
            s.native(
                widgets::radio(lang.text("Classic Tetris"), true, false),
                Rect::new(r.x + 42, r.y + 3, 26, 1),
            );
            s.control(Control::new(
                1,
                Rect::new(r.x + 42, r.y + 7, 26, 1),
                ControlKind::Checkbox {
                    caption: lang.text("Piece preview").into(),
                    checked: preview,
                },
                crate::tetris::target(A::Preview),
            ));
            button(
                &mut s,
                ctx,
                2,
                Rect::new(r.x + 49, r.y + 11, 16, 1),
                "OK",
                A::Apply,
                true,
            );
            button(
                &mut s,
                ctx,
                3,
                Rect::new(r.x + 49, r.y + 14, 16, 1),
                "Cancel",
                A::Back,
                true,
            );
        }
        Screen::Scores => {
            let r = Rect::new((area.width - 72) / 2, (area.height - 20) / 2, 72, 20);
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
                    Rect::new(r.x + 8, r.y + 17, 20, 1),
                    "Retry save",
                    A::RetrySave,
                    true,
                );
            }
            button(
                &mut s,
                ctx,
                0,
                Rect::new(r.x + if retry { 43 } else { 29 }, r.y + 17, 14, 1),
                "OK",
                A::Back,
                true,
            );
        }
        screen => {
            let r = Rect::new((area.width - 50) / 2, y + 2, 50, 17);
            s.hit(
                Rect::new(r.x + 2, r.y, 3, 1),
                crate::tetris::target(A::Back),
            );
            if screen == Screen::Name {
                let col = g.name[..g.name_cursor].width();
                let scroll = col.saturating_sub(43);
                let mut skipped = 0;
                let mut visible = String::new();
                for glyph in g.name.graphemes(true) {
                    if skipped + glyph.width() <= scroll {
                        skipped += glyph.width();
                        continue;
                    }
                    skipped = scroll;
                    visible.push_str(glyph);
                }
                let kind = if g.replace_name {
                    ControlKind::SelectedInput(visible)
                } else {
                    ControlKind::Input(visible)
                };
                s.control(Control::new(
                    0,
                    Rect::new(r.x + 3, r.y + 7, 44, 1),
                    kind,
                    crate::tetris::target(A::NameField),
                ));
                if g.page_focus == 0 {
                    s.cursor = Some((r.x + 3 + (col - scroll) as u16, r.y + 7));
                }
                button(
                    &mut s,
                    ctx,
                    1,
                    Rect::new(r.x + 5, r.bottom() - 2, 16, 1),
                    "OK",
                    A::SubmitName,
                    false,
                );
                button(
                    &mut s,
                    ctx,
                    2,
                    Rect::new(r.x + 27, r.bottom() - 2, 18, 1),
                    "Skip name",
                    A::SkipName,
                    false,
                );
            } else {
                button(
                    &mut s,
                    ctx,
                    0,
                    Rect::new(r.x + 16, r.bottom() - 2, 18, 1),
                    if screen == Screen::GameOver {
                        "Continue"
                    } else {
                        " Enter / Esc: Back "
                    },
                    if screen == Screen::GameOver {
                        A::Continue
                    } else {
                        A::Back
                    },
                    false,
                );
            }
        }
    }
    s
}
