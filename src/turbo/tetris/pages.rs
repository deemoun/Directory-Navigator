//! Game pages drawn with native cells; actions still use the guarded game controller.
use super::Scene;
use crate::presentation::layout::Rect;
use crate::tetris::{Game, Screen};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

impl Scene {
    fn paragraph(&mut self, r: Rect, text: &str, attr: u8) {
        self.fill(r.x, r.y, r.width, r.height, attr);
        for (i, line) in text.lines().take(r.height as usize).enumerate() {
            self.text(r.x, r.y + i as u16, r.width, line, attr);
        }
    }
    fn wrapped(&mut self, r: Rect, text: &str, attr: u8) {
        self.fill(r.x, r.y, r.width, r.height, attr);
        let mut row = 0;
        for line in text.split('\n') {
            let mut col = 0;
            let mut spaces = String::new();
            for word in line.split_inclusive(char::is_whitespace) {
                let content = word.trim_end_matches(char::is_whitespace);
                let trailing = &word[content.len()..];
                if content.is_empty() {
                    spaces.push_str(trailing);
                    continue;
                }
                if col > 0
                    && usize::from(col) + spaces.width() + content.width() > usize::from(r.width)
                {
                    let mut remaining = (r.width - col).max(1);
                    spaces = spaces
                        .graphemes(true)
                        .filter(|g| {
                            let w = g.width() as u16;
                            if w <= remaining {
                                remaining -= w;
                                false
                            } else {
                                true
                            }
                        })
                        .collect();
                    row += 1;
                    col = 0;
                }
                for glyph in spaces.graphemes(true).chain(content.graphemes(true)) {
                    let w = glyph.width() as u16;
                    if col + w > r.width {
                        row += 1;
                        col = 0;
                    }
                    if row >= r.height {
                        return;
                    }
                    self.text(r.x + col, r.y + row, w, glyph, attr);
                    col += w;
                }
                spaces = trailing.into();
            }
            row += 1;
            if row >= r.height {
                return;
            }
        }
    }

    fn window(&mut self, r: Rect, title: &str) {
        crate::turbo::widgets::paint_into(self, crate::turbo::widgets::dialog(title), r);
    }
}

pub(super) fn draw(
    scene: &mut Scene,
    game: &Game,
    lang: crate::localization::Language,
    pending: bool,
) {
    let screen = game.screen;
    if screen == Screen::Play {
        return;
    }
    scene.regions.clear();
    let r = crate::tetris::layout::page(
        Rect::new(0, 0, scene.width, scene.height),
        game,
        lang,
        pending,
    );
    let title = match screen {
        Screen::Setup { .. } => "Setup game",
        Screen::Name => "You have entered Top Ten!",
        Screen::GameOver => "Game over",
        Screen::Help if game.help_mode() == crate::tetris::GameMode::Pentix => "Pentix Help",
        Screen::Help => "Tetris Help",
        _ => "",
    };
    if screen == Screen::Scores {
        scores(scene, game, lang, pending);
        return;
    }
    scene.window(r, lang.text(title));
    match screen {
        Screen::Setup { .. } => {
            for (x, y, w, label) in [
                (r.x + 3, r.y + 2, 26, "Level"),
                (r.x + 30, r.y + 2, r.width - 33, "Game style"),
                (r.x + 30, r.y + 6, r.width - 33, "Options"),
            ] {
                scene.paragraph(Rect::new(x, y, w, 1), lang.text(label), 0x70);
            }
        }
        Screen::GameOver => scene.paragraph(
            Rect::new(r.x + 3, r.y + 2, r.width - 6, 1),
            &format!("{}: {}", lang.text("Final score"), game.score),
            0x71,
        ),
        Screen::Name => {
            scene.paragraph(
                Rect::new(r.x + 2, r.y + 2, 18, 1),
                lang.text("Enter your name:"),
                0x70,
            );
            scene.paragraph(
                Rect::new(r.x + 2, r.y + 3, r.width - 4, 1),
                &crate::filesystem::display_name(lang.text(&game.notice)),
                0x74,
            );
        }
        Screen::Help => {
            let rules = if game.help_mode() == crate::tetris::GameMode::Pentix {
                "Pentix: 27 figures, from one to five cells."
            } else {
                "Classic Tetris: seven four-cell figures."
            };
            scene.wrapped(
                Rect::new(r.x + 2, r.y + 2, r.width - 4, 2),
                lang.text(rules),
                0x71,
            );
            scene.wrapped(Rect::new(r.x+2,r.y+4,r.width-4,8),lang.text("←/Home left · →/PgUp right · ↑ rotate\n↓/Space drop; next tick locks the piece\nF2 new · F3 pause · F4 Top 10 · F5 setup\n+ level · * preview · Esc returns\nAlt+N new · Alt+P pause · Alt+T scores · Alt+S setup\n\nSeparate Top Ten for each game. Saved in tetris.conf."),0x70);
        }
        _ => {}
    }
}
fn scores(scene: &mut Scene, game: &Game, lang: crate::localization::Language, pending: bool) {
    let r = crate::tetris::layout::page(
        Rect::new(0, 0, scene.width, scene.height),
        game,
        lang,
        pending,
    );
    scene.window(
        r,
        lang.text(if game.mode == crate::tetris::GameMode::Pentix {
            "Pentix Top Ten"
        } else {
            "Tetris Top Ten"
        }),
    );
    let body = Rect::new(r.x + 2, r.y + 3, 68, 12);
    crate::turbo::widgets::paint_into(scene, crate::turbo::widgets::group_box(""), body);
    let x = body.x + 1;
    let width = body.width - 2;
    let nw = width - 33;
    for (label, x, w, right) in [
        ("Name", x, nw, false),
        ("Start", x + nw + 1, 5, true),
        ("End", x + nw + 7, 5, true),
        ("Score", x + nw + 13, 20, true),
    ] {
        let label = lang.text(label);
        scene.fill(x, r.y + 2, w, 1, 0x7e);
        let offset = if right {
            usize::from(w).saturating_sub(label.width()) as u16
        } else {
            0
        };
        scene.text(x + offset, r.y + 2, w - offset, label, 0x7e);
    }
    let items: Vec<String> = game
        .records
        .iter()
        .take(10)
        .map(|record| {
            let name = crate::formatting::tail(&crate::filesystem::display_name(&record.name), nw);
            let dots = usize::from(nw).saturating_sub(name.width());
            format!(
                "{name}{} {:.>5} {:.>5} {:.>20}",
                ".".repeat(dots),
                record.start_level,
                record.level,
                record.score
            )
        })
        .collect();
    crate::turbo::widgets::paint_into(
        scene,
        crate::turbo::widgets::text_view(&items, 0, 0x70),
        Rect::new(x, body.y + 1, width, 10),
    );
    if let Some((index, record)) = game
        .highlighted
        .and_then(|index| game.records.get(index).map(|r| (index, r)))
    {
        let name = crate::formatting::tail(&crate::filesystem::display_name(&record.name), nw);
        scene.text(x, body.y + 1 + index as u16, nw, &name, 0x7f);
    }
    if game.records.is_empty() {
        scene.paragraph(
            Rect::new(x + 1, body.y + 1, width - 2, 1),
            lang.text("No finished games yet."),
            0x78,
        );
    }
    if game.notice.is_empty() || game.notice == "Scores saved" {
        if let Some(record) = game.records.get(game.highlighted.unwrap_or(0)) {
            scene.paragraph(
                Rect::new(x, r.y + 15, width, 1),
                &format!(
                    "{}: {}",
                    lang.text("Lines"),
                    record.lines.map_or_else(|| "—".into(), |n| n.to_string())
                ),
                0x78,
            );
        }
    } else {
        scene.wrapped(
            Rect::new(x, r.y + 15, width, 2),
            &crate::filesystem::display_name(lang.text(&game.notice)),
            0x74,
        );
    }
}
