//! Game pages drawn with native cells; actions still use the guarded game controller.
use super::Scene;
use crate::presentation::layout::Rect;
use crate::{app::App, tetris::Screen};
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

pub(super) fn draw(scene: &mut Scene, app: &App, game_y: u16) {
    let game = app.desktop.game().expect("selected game");
    let screen = game.screen;
    if screen == Screen::Play {
        return;
    }
    scene.regions.clear();
    let lang = app.config.language;
    match screen {
        Screen::Setup { .. } => {
            let r = Rect::new((scene.width - 72) / 2, (scene.height - 18) / 2, 72, 18);
            scene.window(r, lang.text("Setup game"));
            let right = r.x + 42;
            scene.paragraph(Rect::new(r.x + 4, r.y + 2, 34, 1), lang.text("Level"), 0x7f);
            scene.paragraph(
                Rect::new(right, r.y + 2, 26, 1),
                lang.text("Game style"),
                0x70,
            );
            scene.paragraph(Rect::new(right, r.y + 6, 26, 1), lang.text("Options"), 0x70);
            scene.paragraph(
                Rect::new(r.x + 4, r.y + 16, 64, 1),
                lang.text("OK starts a new game · Esc cancels"),
                0x78,
            );
        }
        Screen::Scores => scores(scene, app),
        _ => {
            let r = Rect::new((scene.width - 50) / 2, game_y + 2, 50, 17);
            let title = match screen {
                Screen::Help => "Tetris Help",
                Screen::Name => "New high score",
                _ => "Game over",
            };
            scene.window(r, lang.text(title));
            let game = app.desktop.game().unwrap();
            if matches!(screen, Screen::Name | Screen::GameOver) {
                scene.paragraph(
                    Rect::new(r.x + 3, r.y + 2, 44, 3),
                    &format!(
                        "{}: {}\n{}: {}    {}: {} → {}",
                        lang.text("Final score"),
                        game.score,
                        lang.text("Lines"),
                        game.lines,
                        lang.text("Level"),
                        game.start_level,
                        game.level
                    ),
                    0x71,
                );
                if screen == Screen::Name {
                    scene.paragraph(
                        Rect::new(r.x + 3, r.y + 6, 44, 1),
                        lang.text("Your name (30 characters)"),
                        0x70,
                    );
                    scene.paragraph(
                        Rect::new(r.x + 3, r.y + 9, 44, 1),
                        lang.text("Tab chooses · Enter activates · Esc cancels"),
                        0x70,
                    );
                    scene.wrapped(
                        Rect::new(r.x + 3, r.y + 10, 44, 3),
                        &crate::filesystem::display_name(lang.text(&game.notice)),
                        0x74,
                    );
                } else {
                    scene.paragraph(
                        Rect::new(r.x + 3, r.y + 7, 44, 2),
                        lang.text("Enter continues · Esc cancels"),
                        0x70,
                    );
                }
            } else {
                scene.paragraph(Rect::new(r.x+2,r.y+1,46,13),lang.text("←/Home left · →/PgUp right · ↑ rotate\n↓/Space drop · F2 new game · F3 pause\nF4 high scores · F5 setup\n+ increase level · * toggle next preview\n\nAlt+N new · Alt+P pause · Alt+T scores\nAlt+S setup · Esc returns to files\n\nHigh scores save in tetris.conf."),0x70);
            }
        }
    }
}
fn scores(scene: &mut Scene, app: &App) {
    let lang = app.config.language;
    let r = Rect::new((scene.width - 72) / 2, (scene.height - 20) / 2, 72, 20);
    scene.window(r, lang.text("Tetris Top Ten"));
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
    let game = app.desktop.game().unwrap();
    let items = game
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
        crate::turbo::widgets::list_selection(
            items,
            game.highlighted,
            0,
            game.highlighted.is_some(),
        ),
        Rect::new(x, body.y + 1, width, 10),
    );
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
        scene.paragraph(
            Rect::new(x, r.y + 16, width, 1),
            lang.text(&game.notice),
            0x70,
        );
    } else {
        scene.wrapped(
            Rect::new(x, r.y + 15, width, 2),
            &crate::filesystem::display_name(lang.text(&game.notice)),
            0x74,
        );
    }
}
