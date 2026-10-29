//! Native Tetris drawing. Game rules and input actions stay in `tetris`.
//! The document paints these native cells
//! through TV. The codec boundary preserves graphemes and terminal-theme colors.
use super::scene::Scene;
use crate::tetris::{Figure, DEFINITIONS, HEIGHT, WIDTH};
use std::collections::BTreeMap;
use unicode_width::UnicodeWidthStr;
mod controls;
mod pages;
pub(crate) use controls::spec;

pub(crate) fn prepare(
    game: &crate::tetris::Game,
    pending: bool,
    ctx: &crate::plugins::Context,
) -> Scene {
    let width = ctx.area.width;
    let height = ctx.area.height;
    assert!(width >= 80 && height >= 25);
    let language = ctx.language;
    let mut scene = Scene {
        width,
        height,
        profile: ctx.palette,
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
    let wx = x + 1;
    let wy = y + 1;
    scene.fill(wx, wy, WIDTH as u16 * 2 + 2, HEIGHT as u16 + 2, 0x0f);
    scene.border(wx, wy, WIDTH as u16 * 2 + 2, HEIGHT as u16 + 2, false, 0x7f);
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
                Figure::color(game.piece.kind),
            );
        }
    }
    let sx = x + 28;
    for (top, h, title, attr) in [
        (y + 1, 5, "Info", 0x70),
        (y + 5, 7, "Next", 0x70),
        (y + 12, 4, "Best", 0x7e),
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
    // DN's next-piece well is black, independently of the dialog palette.
    scene.fill(sx + 1, y + 6, 22, 5, 0x0f);
    if game.preview && !game.paused && !game.over {
        let figure = &DEFINITIONS[game.next];
        let cells = &figure.cells[..figure.count];
        let min_x = cells.iter().map(|c| c.0).min().unwrap();
        let max_x = cells.iter().map(|c| c.0).max().unwrap();
        let min_y = cells.iter().map(|c| c.1).min().unwrap();
        let max_y = cells.iter().map(|c| c.1).max().unwrap();
        let left = sx + 1 + (22 - (max_x - min_x + 1) as u16 * 2) / 2;
        let top = y + 6 + (5 - (max_y - min_y + 1) as u16) / 2;
        for (col, row) in cells {
            scene.text(
                left + (col - min_x) as u16 * 2,
                top + (row - min_y) as u16,
                2,
                "██",
                Figure::color(game.next),
            );
        }
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
        scene.text(sx + 1, y + 13 + i as u16, 22, &prefix, 0x7f);
        scene.text(
            sx + 1 + n,
            y + 13 + i as u16,
            22 - n,
            &crate::formatting::tail(&value, 22 - n),
            0x7e,
        );
    }
    if !game.notice.is_empty() && game.notice != "Scores saved" {
        scene.text(
            sx,
            y + 16,
            24,
            language.text(if pending {
                "Not saved — F4 retry"
            } else {
                "Cannot load scores"
            }),
            0x74,
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
    let _ = pending;
    scene
}

pub(crate) fn page(
    game: &crate::tetris::Game,
    pending: bool,
    ctx: &crate::plugins::Context,
) -> Scene {
    let mut scene = Scene {
        width: ctx.area.width,
        height: ctx.area.height,
        profile: ctx.palette,
        cells: Default::default(),
        regions: Vec::new(),
        cursor: None,
    };
    pages::draw(&mut scene, game, ctx.language, pending);
    scene
}
