use crate::{
    app::App,
    mouse::Target,
    tetris::{GameAction, Screen, FIGURES, HEIGHT, WIDTH},
};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

fn button(frame: &mut Frame, app: &mut App, rect: Rect, label: &str, action: GameAction) {
    let profile = app.config.palette;
    let dos = profile.styles();
    frame.render_widget(Paragraph::new(label).style(dos(0x30)), rect);
    app.mouse.regions.push((rect, Target::Game(action)));
}

pub(super) fn draw(frame: &mut Frame, app: &mut App, area: Rect) {
    let profile = app.config.palette;
    let dos = profile.styles();
    app.mouse.regions.clear();
    let rect = Rect::new(
        area.x + (area.width - 54) / 2,
        area.y + (area.height - 23) / 2,
        54,
        23,
    );
    frame.render_widget(Clear, rect);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .title(" Tetris — Directory Navigator ")
            .style(dos(0x70)),
        rect,
    );
    let well = Rect::new(
        rect.x + 1,
        rect.y + 1,
        WIDTH as u16 * 2 + 2,
        HEIGHT as u16 + 2,
    );
    frame.render_widget(
        Block::default().borders(Borders::ALL).style(dos(0x07)),
        well,
    );
    let Some(game) = app.tetris.as_ref() else {
        return;
    };
    for (y, row) in game.board.iter().enumerate() {
        for (x, color) in row.iter().enumerate().filter(|(_, color)| **color != 0) {
            frame.render_widget(
                Paragraph::new("██").style(dos(*color)),
                Rect::new(well.x + 1 + x as u16 * 2, well.y + 1 + y as u16, 2, 1),
            );
        }
    }
    if !game.over {
        for (x, y) in game
            .piece
            .positions()
            .filter(|(x, y)| *x >= 0 && *x < WIDTH as i32 && *y >= 0 && *y < HEIGHT as i32)
        {
            frame.render_widget(
                Paragraph::new("██").style(dos(15 - game.piece.kind as u8)),
                Rect::new(well.x + 1 + x as u16 * 2, well.y + 1 + y as u16, 2, 1),
            );
        }
    }
    let info_x = rect.x + 29;
    frame.render_widget(
        Paragraph::new(format!(
            "Score: {}\nLines: {}\nLevel: {}",
            game.score, game.lines, game.level
        ))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Information "),
        )
        .style(dos(0x71)),
        Rect::new(info_x, rect.y + 1, 23, 5),
    );
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(" Next ")
            .style(dos(0x70)),
        Rect::new(info_x, rect.y + 6, 23, 7),
    );
    if game.preview {
        for (x, y) in FIGURES[game.next] {
            frame.render_widget(
                Paragraph::new("██").style(dos(0x70 | (15 - game.next as u8))),
                Rect::new(info_x + 7 + x as u16 * 2, rect.y + 7 + y as u16, 2, 1),
            );
        }
    } else {
        frame.render_widget(
            Paragraph::new("Preview off").style(dos(0x71)),
            Rect::new(info_x + 3, rect.y + 9, 16, 1),
        );
    }
    frame.render_widget(
        Paragraph::new("← → Move   ↑ Rotate\n↓ / Space Drop\nEsc returns to files")
            .style(dos(0x70)),
        Rect::new(info_x, rect.y + 14, 23, 3),
    );
    if game.paused || game.over {
        frame.render_widget(
            Paragraph::new(if game.over {
                " GAME OVER · F2 New "
            } else {
                "      PAUSED       "
            })
            .style(dos(0x3f)),
            Rect::new(well.x + 3, well.y + 9, 20, 1),
        );
    }
    let screen = game.screen;
    button(
        frame,
        app,
        Rect::new(info_x, rect.y + 18, 10, 1),
        " New game ",
        GameAction::New,
    );
    button(
        frame,
        app,
        Rect::new(info_x + 12, rect.y + 18, 11, 1),
        " Pause ",
        GameAction::Pause,
    );
    button(
        frame,
        app,
        Rect::new(info_x, rect.y + 20, 10, 1),
        " Setup ",
        GameAction::Setup,
    );
    button(
        frame,
        app,
        Rect::new(info_x + 12, rect.y + 20, 11, 1),
        " Close ",
        GameAction::Close,
    );
    let bar = Rect::new(area.x, area.bottom() - 1, area.width, 1);
    let cells = Layout::horizontal([Constraint::Fill(1); 8]).split(bar);
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
        button(frame, app, cells[i], label, action);
    }
    if screen != Screen::Play {
        draw_page(frame, app, rect, screen);
    }
}

fn draw_page(frame: &mut Frame, app: &mut App, game_rect: Rect, screen: Screen) {
    let profile = app.config.palette;
    let dos = profile.styles();
    // A subwindow consumes all underlying board and function-bar mouse targets.
    app.mouse.regions.clear();
    let rect = Rect::new(game_rect.x + 2, game_rect.y + 3, 50, 17);
    let title = match screen {
        Screen::Help => " Tetris Help ",
        Screen::Scores => " Session Top Ten ",
        _ => " Tetris Setup ",
    };
    frame.render_widget(Clear, rect);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .style(dos(0x70)),
        rect,
    );
    let text = match screen {
        Screen::Help => "Based on TETRIS.PAS by RIT Research Labs.\n\n←/Home left · →/PgUp right · ↑ rotate\n↓/Space drop · F2 new game · F3 pause\nF4 session scores · F5 setup\n+ increase level · * toggle next preview\n\nAlt+N new · Alt+P pause · Alt+T scores\nAlt+S setup · Esc returns to files\n\nSmall terminals automatically suspend play.\nScores are kept for this app session only.".into(),
        Screen::Scores => {
            let mut text = "Rank       Score       Lines    Level\n".to_owned();
            if let Some(game) = &app.tetris {
                if game.records.is_empty() { text.push_str("\nNo finished games yet."); }
                for (i, record) in game.records.iter().enumerate() {
                    text.push_str(&format!("\n{:>4} {:>11} {:>11} {:>8}", i + 1, record.score, record.lines, record.level));
                }
            }
            text
        }
        Screen::Setup { level, preview } => format!("Classic Tetris · 12 × 19\n\nStarting level: {level}\nNext-piece preview: {}\n\n←/→ or +/− change level (1–10)\nSpace or * toggles preview\n\nEnter starts a new game\nEsc cancels without changing the game", if preview { "on" } else { "off" }),
        Screen::Play => String::new(),
    };
    frame.render_widget(
        Paragraph::new(text).style(dos(0x70)),
        Rect::new(rect.x + 2, rect.y + 1, 46, 13),
    );
    if matches!(screen, Screen::Setup { .. }) {
        button(
            frame,
            app,
            Rect::new(rect.x + 23, rect.y + 3, 5, 1),
            " [-] ",
            GameAction::LevelDown,
        );
        button(
            frame,
            app,
            Rect::new(rect.x + 30, rect.y + 3, 5, 1),
            " [+] ",
            GameAction::LevelUp,
        );
        button(
            frame,
            app,
            Rect::new(rect.x + 34, rect.y + 4, 12, 1),
            " Toggle ",
            GameAction::Preview,
        );
        button(
            frame,
            app,
            Rect::new(rect.x + 5, rect.bottom() - 2, 18, 1),
            " Start new game ",
            GameAction::Apply,
        );
        button(
            frame,
            app,
            Rect::new(rect.x + 28, rect.bottom() - 2, 16, 1),
            " Cancel ",
            GameAction::Back,
        );
    } else {
        button(
            frame,
            app,
            Rect::new(rect.x + 16, rect.bottom() - 2, 18, 1),
            " Enter / Esc: Back ",
            GameAction::Back,
        );
    }
}
