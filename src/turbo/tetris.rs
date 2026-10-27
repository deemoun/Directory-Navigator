//! Native Tetris drawing. Game rules and input actions stay in `tetris`.
//! The document paints these native cells
//! through TV. The codec boundary preserves graphemes and terminal-theme colors.
use super::scene::Scene;
use crate::{
    app::App,
    mouse::Target,
    tetris::{GameAction, FIGURES, HEIGHT, WIDTH},
};
use std::collections::BTreeMap;
use unicode_width::UnicodeWidthStr;
mod pages;

impl Scene {
    pub(crate) fn button(&mut self, x: u16, y: u16, label: &str) {
        super::widgets::paint_into(
            self,
            super::widgets::button(label, false),
            crate::presentation::layout::Rect::new(x, y, 11, 2),
        );
    }
}

pub(crate) fn prepare(app: &App, width: u16, height: u16) -> Scene {
    assert!(width >= 80 && height >= 25);
    let language = app.config.language;
    let mut scene = Scene {
        width,
        height,
        profile: app.config.palette,
        cells: BTreeMap::new(),
        cursor: None,
        regions: Vec::new(),
    };
    let x = (width - 54) / 2;
    let y = (height - 23) / 2;
    scene.fill(x, y, 54, 23, 0x70);
    scene.border(x, y, 54, 23, true, 0x7f);
    scene.title(
        x,
        y,
        54,
        &crate::formatting::tail(language.text("Navigator's game"), 40),
    );
    scene.text(x + 2, y, 1, "[", 0x7f);
    scene.text(x + 3, y, 1, "■", 0x7a);
    scene.text(x + 4, y, 1, "]", 0x7f);
    let mut regions = vec![(
        crate::presentation::layout::Rect::new(x + 2, y, 3, 1),
        Target::Game(GameAction::Close),
    )];
    let wx = x + 1;
    let wy = y + 1;
    scene.fill(wx, wy, WIDTH as u16 * 2 + 2, HEIGHT as u16 + 2, 0x0f);
    scene.border(wx, wy, WIDTH as u16 * 2 + 2, HEIGHT as u16 + 2, false, 0x7f);
    let game = app.desktop.game().expect("selected game");
    for (row, cells) in game.board.iter().enumerate() {
        for (col, &color) in cells.iter().enumerate() {
            if color != 0 {
                scene.text(wx + 1 + col as u16 * 2, wy + 1 + row as u16, 2, "██", color);
            }
        }
    }
    if !game.over {
        for (col, row) in game.piece.positions().filter(|(col, row)| {
            (0..WIDTH as i32).contains(col) && (0..HEIGHT as i32).contains(row)
        }) {
            scene.text(
                wx + 1 + col as u16 * 2,
                wy + 1 + row as u16,
                2,
                "██",
                15 - game.piece.kind as u8,
            );
        }
    }
    let sx = x + 28;
    for (top, h, title, attr) in [
        (y + 1, 5, "Info", 0x70),
        (y + 5, 7, "Next", 0x70),
        (y + 13, 4, "Best", 0x7e),
    ] {
        scene.border(sx, top, 24, h, false, attr);
        scene.title(sx, top, 24, language.text(title));
    }
    for (i, (label, value)) in [
        ("Score", game.score.to_string()),
        ("Lines", game.lines.to_string()),
        ("Level", game.level.to_string()),
    ]
    .into_iter()
    .enumerate()
    {
        let prefix = format!("{}: ", language.text(label));
        scene.text(sx + 1, y + 2 + i as u16, 22, &prefix, 0x70);
        let n = prefix.width().min(22) as u16;
        scene.text(sx + 1 + n, y + 2 + i as u16, 22 - n, &value, 0x7f);
    }
    if game.preview {
        for (col, row) in FIGURES[game.next] {
            scene.text(
                sx + 7 + col as u16 * 2,
                y + 6 + row as u16,
                2,
                "██",
                0x70 | (15 - game.next as u8),
            );
        }
    } else {
        scene.fill(sx + 2, y + 8, 20, 1, 0x78);
        scene.text(sx + 2, y + 8, 20, language.text("Preview off"), 0x78);
    }
    let best = game.records.first();
    for (i, (label, value)) in [
        (
            "Name",
            best.map_or_else(|| "—".into(), |r| crate::filesystem::display_name(&r.name)),
        ),
        (
            "Score",
            best.map_or_else(|| "0".into(), |r| r.score.to_string()),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let prefix = format!("{}: ", language.text(label));
        let n = prefix.width().min(22) as u16;
        scene.text(sx + 1, y + 14 + i as u16, 22, &prefix, 0x7f);
        scene.text(
            sx + 1 + n,
            y + 14 + i as u16,
            22 - n,
            &crate::formatting::tail(&value, 22 - n),
            0x7e,
        );
    }
    if game.paused || game.over {
        scene.fill(wx + 3, wy + 9, 20, 1, 0x3f);
        scene.text(
            wx + 3,
            wy + 9,
            20,
            if game.over {
                " GAME OVER · F2 New "
            } else {
                "      PAUSED       "
            },
            0x3f,
        );
    }
    for (index, (label, action)) in crate::tetris::BUTTONS.into_iter().enumerate() {
        let bx = sx + (index as u16 % 2) * 11;
        let by = y + 18 + (index as u16 / 2) * 2;
        scene.button(bx, by, language.text(label));
        regions.push((
            crate::presentation::layout::Rect::new(bx, by, 11, 1),
            Target::Game(action),
        ));
    }
    // Match the established eight equal footer slots, including remainder cells.
    // Layout allocation is kept at the mouse/legacy-shell boundary.
    let slots = crate::presentation::layout::Layout::horizontal(
        [crate::presentation::layout::Constraint::Fill(1); 8],
    )
    .split(crate::presentation::layout::Rect::new(
        0,
        height - 1,
        width,
        1,
    ));
    for (i, (label, action)) in [
        ("F1 Help", GameAction::Help),
        ("F2 New", GameAction::New),
        ("F3 Pause", GameAction::Pause),
        ("F4 Scores", GameAction::Scores),
        ("F5 Setup", GameAction::Setup),
        ("+ Level", GameAction::LevelUp),
        ("* Preview", GameAction::Preview),
        ("Esc Close", GameAction::Close),
    ]
    .into_iter()
    .enumerate()
    {
        let slot = slots[i];
        scene.fill(slot.x, slot.y, slot.width, 1, 0x30);
        scene.text(slot.x, slot.y, slot.width, language.text(label), 0x30);
        regions.push((slot, Target::Game(action)));
    }
    scene.regions = regions;
    pages::draw(&mut scene, app, y);
    scene
}
