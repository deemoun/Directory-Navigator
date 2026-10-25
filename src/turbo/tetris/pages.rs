//! Game pages drawn with native cells; actions still use the guarded game controller.
use super::Scene;
use crate::{
    app::App,
    mouse::Target,
    palette::PaletteProfile,
    tetris::{GameAction, Screen},
};
use ratatui::layout::Rect;
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
        self.fill(r.x, r.y, r.width, r.height, 0x70);
        self.border(r.x, r.y, r.width, r.height, true, 0x7f);
        self.title(
            r.x,
            r.y,
            r.width,
            &crate::formatting::tail(title, r.width.saturating_sub(14)),
        );
    }
    fn close(&mut self, r: Rect, app: &mut App) {
        self.text(r.x + 2, r.y, 1, "[", 0x7f);
        self.text(r.x + 3, r.y, 1, "■", 0x7a);
        self.text(r.x + 4, r.y, 1, "]", 0x7f);
        app.mouse.regions.push((
            Rect::new(r.x + 2, r.y, 3, 1),
            Target::Game(GameAction::Back),
        ));
    }
    fn plain_button(&mut self, app: &mut App, r: Rect, label: &str, action: GameAction) {
        self.paragraph(r, app.config.language.text(label), 0x30);
        app.mouse.regions.push((r, Target::Game(action)));
    }
    fn classic(&mut self, app: &mut App, r: Rect, label: &str, action: GameAction, focused: bool) {
        let label = app.config.language.text(label);
        self.fill(r.x + 1, r.y + 1, r.width, 1, 0x00);
        if self.profile == PaletteProfile::Terminal {
            self.paragraph(r, label, if focused { 0x30 } else { 0x70 });
        } else {
            self.fill(r.x, r.y, r.width, 1, 0x8f);
            let label = label
                .trim()
                .trim_start_matches('[')
                .trim_end_matches(']')
                .trim();
            let spare = usize::from(r.width).saturating_sub(label.width() + 2);
            self.text(r.x, r.y, 1, if focused { "►" } else { " " }, 0x8b);
            let start = 1 + spare / 2;
            self.text(
                r.x + start as u16,
                r.y,
                r.width.saturating_sub(start as u16),
                label,
                if focused { 0x8e } else { 0x8f },
            );
            let end = start + label.width() + spare - spare / 2;
            if end < usize::from(r.width) {
                self.text(
                    r.x + end as u16,
                    r.y,
                    1,
                    if focused { "◄" } else { " " },
                    0x8b,
                );
            }
            if !focused {
                // The legacy mnemonic overlay resets the whole button's style.
                for col in r.x..r.right() {
                    if let Some(glyph) = self.cells.get_mut(&(col, r.y)) {
                        glyph.attribute = 0x8f;
                    }
                }
                let pad = usize::from(r.width).saturating_sub(label.width()) / 2;
                let mut chars = label.chars();
                let first = chars.next().map(|c| c.to_string()).unwrap_or_default();
                self.text(r.x + pad as u16, r.y, r.width - pad as u16, &first, 0x8e);
                let rest: String = chars.collect();
                let start = pad + first.width();
                self.text(
                    r.x + start as u16,
                    r.y,
                    r.width.saturating_sub(start as u16),
                    &rest,
                    0x8f,
                );
            }
        }
        app.mouse.regions.push((r, Target::Game(action)));
    }
}

pub(super) fn draw(scene: &mut Scene, app: &mut App, game_y: u16) {
    let game = app.desktop.game().expect("selected game");
    let screen = game.screen;
    if screen == Screen::Play {
        return;
    }
    app.mouse.regions.clear();
    let lang = app.config.language;
    match screen {
        Screen::Setup { level, preview } => {
            let r = Rect::new((scene.width - 72) / 2, (scene.height - 18) / 2, 72, 18);
            scene.window(r, lang.text("Setup game"));
            scene.close(r, app);
            let focus = app.desktop.game().unwrap().setup_focus;
            let right = r.x + 42;
            scene.paragraph(Rect::new(r.x + 4, r.y + 2, 34, 1), lang.text("Level"), 0x7f);
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
            .iter()
            .enumerate()
            {
                let selected = level == i as u8 + 1;
                let row = Rect::new(r.x + 4, r.y + 3 + i as u16, 35, 1);
                scene.text(
                    row.x,
                    row.y,
                    4,
                    if selected { "(•) " } else { "( ) " },
                    if selected && focus == 0 { 0x7f } else { 0x78 },
                );
                scene.text(row.x + 4, row.y, 2, &format!("{:>2}", i + 1), 0x7e);
                scene.text(
                    row.x + 6,
                    row.y,
                    29,
                    &format!(" — {}", lang.text(label)),
                    if selected { 0x7f } else { 0x78 },
                );
                app.mouse
                    .regions
                    .push((row, Target::Game(GameAction::SetLevel(i as u8 + 1))));
            }
            scene.paragraph(
                Rect::new(right, r.y + 2, 26, 1),
                lang.text("Game style"),
                0x70,
            );
            scene.paragraph(
                Rect::new(right, r.y + 3, 26, 1),
                &format!("(•) {}", lang.text("Classic Tetris")),
                0x78,
            );
            scene.paragraph(Rect::new(right, r.y + 6, 26, 1), lang.text("Options"), 0x70);
            let check = Rect::new(right, r.y + 7, 26, 1);
            scene.paragraph(
                check,
                &format!(
                    "[{}] {}",
                    if preview { "✓" } else { " " },
                    lang.text("Piece preview")
                ),
                if focus == 1 { 0x7f } else { 0x78 },
            );
            app.mouse
                .regions
                .push((check, Target::Game(GameAction::Preview)));
            scene.paragraph(
                Rect::new(r.x + 4, r.y + 16, 64, 1),
                lang.text("OK starts a new game · Esc cancels"),
                0x78,
            );
            scene.classic(
                app,
                Rect::new(right + 7, r.y + 11, 16, 1),
                "OK",
                GameAction::Apply,
                focus != 3,
            );
            scene.classic(
                app,
                Rect::new(right + 7, r.y + 14, 16, 1),
                "Cancel",
                GameAction::Back,
                focus == 3,
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
                    let col = game.name[..game.name_cursor].width();
                    let scroll = col.saturating_sub(43);
                    let mut skipped = 0;
                    let mut visible = String::new();
                    for glyph in game.name.graphemes(true) {
                        if skipped + glyph.width() <= scroll {
                            skipped += glyph.width();
                            continue;
                        }
                        skipped = scroll;
                        visible.push_str(glyph);
                    }
                    scene.paragraph(
                        Rect::new(r.x + 3, r.y + 7, 44, 1),
                        &visible,
                        if game.replace_name {
                            0x30
                        } else if scene.profile == PaletteProfile::ClassicDn {
                            0x0f
                        } else {
                            0x1f
                        },
                    );
                    scene.cursor = Some((r.x + 3 + (col - scroll) as u16, r.y + 7));
                    scene.paragraph(
                        Rect::new(r.x + 3, r.y + 9, 44, 1),
                        lang.text("Enter saves · Esc uses Anonymous"),
                        0x70,
                    );
                    scene.wrapped(
                        Rect::new(r.x + 3, r.y + 10, 44, 3),
                        &crate::filesystem::display_name(lang.text(&game.notice)),
                        0x74,
                    );
                    scene.plain_button(
                        app,
                        Rect::new(r.x + 5, r.bottom() - 2, 16, 1),
                        "OK",
                        GameAction::SubmitName,
                    );
                    scene.plain_button(
                        app,
                        Rect::new(r.x + 27, r.bottom() - 2, 18, 1),
                        "Skip name",
                        GameAction::SkipName,
                    );
                } else {
                    scene.paragraph(
                        Rect::new(r.x + 3, r.y + 7, 44, 2),
                        lang.text("Enter / Esc continues to high scores"),
                        0x70,
                    );
                    scene.plain_button(
                        app,
                        Rect::new(r.x + 16, r.bottom() - 2, 18, 1),
                        "Continue",
                        GameAction::Continue,
                    );
                }
            } else {
                scene.paragraph(Rect::new(r.x+2,r.y+1,46,13),lang.text("←/Home left · →/PgUp right · ↑ rotate\n↓/Space drop · F2 new game · F3 pause\nF4 high scores · F5 setup\n+ increase level · * toggle next preview\n\nAlt+N new · Alt+P pause · Alt+T scores\nAlt+S setup · Esc returns to files\n\nHigh scores save in tetris.conf."),0x70);
                scene.plain_button(
                    app,
                    Rect::new(r.x + 16, r.bottom() - 2, 18, 1),
                    " Enter / Esc: Back ",
                    GameAction::Back,
                );
            }
        }
    }
}
fn scores(scene: &mut Scene, app: &mut App) {
    let lang = app.config.language;
    let r = Rect::new((scene.width - 72) / 2, (scene.height - 20) / 2, 72, 20);
    scene.window(r, lang.text("Tetris Top Ten"));
    scene.close(r, app);
    let body = Rect::new(r.x + 2, r.y + 3, 68, 12);
    scene.border(body.x, body.y, body.width, body.height, false, 0x7f);
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
    for (i, record) in game.records.iter().take(10).enumerate() {
        let name = crate::formatting::tail(&crate::filesystem::display_name(&record.name), nw);
        let dots = usize::from(nw).saturating_sub(name.width());
        let text = format!(
            "{name}{} {:.>5} {:.>5} {:.>20}",
            ".".repeat(dots),
            record.start_level,
            record.level,
            record.score
        );
        scene.paragraph(
            Rect::new(x, body.y + 1 + i as u16, width, 1),
            &text,
            if game.highlighted == Some(i) {
                0x30
            } else {
                0x70
            },
        );
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
    let retry = app.game_scores.has_pending();
    if retry {
        scene.classic(
            app,
            Rect::new(r.x + 8, r.y + 17, 20, 1),
            "Retry save",
            GameAction::RetrySave,
            false,
        );
    }
    scene.classic(
        app,
        Rect::new(r.x + if retry { 43 } else { 29 }, r.y + 17, 14, 1),
        "OK",
        GameAction::Back,
        true,
    );
}
