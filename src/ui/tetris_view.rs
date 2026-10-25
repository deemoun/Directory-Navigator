use crate::painting::{Block, Borders, Clear, Frame, Paragraph};
use crate::{
    app::App,
    mouse::Target,
    tetris::{GameAction, Screen, FIGURES, HEIGHT, WIDTH},
};
use ratatui::layout::{Constraint, Layout, Rect};

fn button(frame: &mut impl Frame, app: &mut App, rect: Rect, label: &str, action: GameAction) {
    let profile = app.config.palette;
    let dos = profile.styles();
    frame.render_widget(Clear, rect);
    frame.render_widget(
        Paragraph::new(app.config.language.text(label)).style(dos(0x30)),
        rect,
    );
    app.mouse.regions.push((rect, Target::Game(action)));
}

pub(super) fn draw(frame: &mut impl Frame, app: &mut App, area: Rect) {
    let profile = app.config.palette;
    let dos = profile.styles();
    app.mouse.regions.clear();
    let rect = Rect::new(
        area.x + (area.width - 54) / 2,
        area.y + (area.height - 23) / 2,
        54,
        23,
    );
    super::dialog_style::window(
        frame,
        profile,
        rect,
        rect.width,
        rect.height,
        app.config.language.text("Navigator's game"),
    );
    super::dialog_style::close_button(
        frame,
        profile,
        rect,
        &mut app.mouse.regions,
        Target::Game(GameAction::Close),
    );
    let well = Rect::new(
        rect.x + 1,
        rect.y + 1,
        WIDTH as u16 * 2 + 2,
        HEIGHT as u16 + 2,
    );
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(dos(0x7f))
            .style(dos(0x0f)),
        well,
    );
    let Some(game) = app.desktop.game() else {
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
    let info_x = rect.x + 28;
    let sidebar = 24;
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(dos(0x70))
            .title(app.config.language.text("Info"))
            .title_alignment(ratatui::layout::Alignment::Center)
            .title_style(dos(0x7f)),
        Rect::new(info_x, rect.y + 1, sidebar, 5),
    );
    for (i, (label, value)) in [
        ("Score", game.score.to_string()),
        ("Lines", game.lines.to_string()),
        ("Level", game.level.to_string()),
    ]
    .into_iter()
    .enumerate()
    {
        frame.render_widget(
            Paragraph::new(ratatui::text::Line::from(vec![
                ratatui::text::Span::styled(
                    format!("{}: ", app.config.language.text(label)),
                    dos(0x70),
                ),
                ratatui::text::Span::styled(value, dos(0x7f)),
            ])),
            Rect::new(info_x + 1, rect.y + 2 + i as u16, sidebar - 2, 1),
        );
    }
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(dos(0x70))
            .title(app.config.language.text("Next"))
            .title_alignment(ratatui::layout::Alignment::Center)
            .title_style(dos(0x7f)),
        Rect::new(info_x, rect.y + 5, sidebar, 7),
    );
    if game.preview {
        for (x, y) in FIGURES[game.next] {
            frame.render_widget(
                Paragraph::new("██").style(dos(0x70 | (15 - game.next as u8))),
                Rect::new(info_x + 7 + x as u16 * 2, rect.y + 6 + y as u16, 2, 1),
            );
        }
    } else {
        frame.render_widget(
            Paragraph::new(app.config.language.text("Preview off")).style(dos(0x78)),
            Rect::new(info_x + 2, rect.y + 8, sidebar - 4, 1),
        );
    }
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(dos(0x7e))
            .title(app.config.language.text("Best"))
            .title_alignment(ratatui::layout::Alignment::Center)
            .title_style(dos(0x7f)),
        Rect::new(info_x, rect.y + 13, sidebar, 4),
    );
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
        let prefix = format!("{}: ", app.config.language.text(label));
        let available =
            (sidebar - 2).saturating_sub(ratatui::text::Span::raw(&prefix).width() as u16);
        frame.render_widget(
            Paragraph::new(ratatui::text::Line::from(vec![
                ratatui::text::Span::styled(prefix, dos(0x7f)),
                ratatui::text::Span::styled(crate::formatting::tail(&value, available), dos(0x7e)),
            ])),
            Rect::new(info_x + 1, rect.y + 14 + i as u16, sidebar - 2, 1),
        );
    }
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
    for (label, action, x, y) in [
        ("New", GameAction::New, info_x, rect.y + 18),
        ("Setup", GameAction::Setup, info_x + 13, rect.y + 18),
        ("Top 10", GameAction::Scores, info_x, rect.y + 20),
        ("Pause", GameAction::Pause, info_x + 13, rect.y + 20),
    ] {
        classic_button(frame, app, Rect::new(x, y, 10, 1), label, action, false);
    }
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

fn draw_page(frame: &mut impl Frame, app: &mut App, game_rect: Rect, screen: Screen) {
    if screen == Screen::Scores {
        draw_scores(frame, app);
        return;
    }
    if let Screen::Setup { level, preview } = screen {
        draw_setup(frame, app, level, preview);
        return;
    }
    let language = app.config.language;
    let profile = app.config.palette;
    let dos = profile.styles();
    app.mouse.regions.clear();
    let width = 50;
    let height = 17;
    let area = frame.area();
    let rect = Rect::new(
        area.x + (area.width - width) / 2,
        game_rect.y + 2,
        width,
        height,
    );
    let title = match screen {
        Screen::Help => "Tetris Help",
        Screen::Scores => "Tetris Top Ten",
        Screen::Name => "New high score",
        Screen::GameOver => "Game over",
        _ => "Tetris Setup",
    };
    super::dialog_style::window(frame, profile, rect, width, height, language.text(title));
    let Some(game) = app.desktop.game() else {
        return;
    };
    if matches!(screen, Screen::GameOver | Screen::Name) {
        let text = format!(
            "{}: {}\n{}: {}    {}: {} → {}",
            language.text("Final score"),
            game.score,
            language.text("Lines"),
            game.lines,
            language.text("Level"),
            game.start_level,
            game.level
        );
        frame.render_widget(
            Paragraph::new(text).style(dos(0x71)),
            Rect::new(rect.x + 3, rect.y + 2, 44, 3),
        );
        if screen == Screen::Name {
            frame.render_widget(
                Paragraph::new(language.text("Your name (30 characters)")).style(dos(0x70)),
                Rect::new(rect.x + 3, rect.y + 6, 44, 1),
            );
            let field = Rect::new(rect.x + 3, rect.y + 7, 44, 1);
            let col = ratatui::text::Line::from(&game.name[..game.name_cursor]).width();
            let scroll = col.saturating_sub(43);
            frame.render_widget(
                Paragraph::new(game.name.as_str())
                    .scroll((0, scroll as u16))
                    .style(if game.replace_name {
                        dos(0x30)
                    } else {
                        super::dialog_style::input_style(profile, true)
                    }),
                field,
            );
            frame.set_cursor_position((field.x + (col - scroll) as u16, field.y));
            frame.render_widget(
                Paragraph::new(language.text("Enter saves · Esc uses Anonymous")).style(dos(0x70)),
                Rect::new(rect.x + 3, rect.y + 9, 44, 1),
            );
            frame.render_widget(
                Paragraph::new(crate::filesystem::display_name(language.text(&game.notice)))
                    .style(dos(0x74))
                    .wrap(crate::painting::Wrap { trim: false }),
                Rect::new(rect.x + 3, rect.y + 10, 44, 3),
            );
            button(
                frame,
                app,
                Rect::new(rect.x + 5, rect.bottom() - 2, 16, 1),
                "OK",
                GameAction::SubmitName,
            );
            button(
                frame,
                app,
                Rect::new(rect.x + 27, rect.bottom() - 2, 18, 1),
                "Skip name",
                GameAction::SkipName,
            );
        } else {
            frame.render_widget(
                Paragraph::new(language.text("Enter / Esc continues to high scores"))
                    .style(dos(0x70)),
                Rect::new(rect.x + 3, rect.y + 7, 44, 2),
            );
            button(
                frame,
                app,
                Rect::new(rect.x + 16, rect.bottom() - 2, 18, 1),
                "Continue",
                GameAction::Continue,
            );
        }
        return;
    }
    let text = match screen {
        Screen::Help => language.text("←/Home left · →/PgUp right · ↑ rotate\n↓/Space drop · F2 new game · F3 pause\nF4 high scores · F5 setup\n+ increase level · * toggle next preview\n\nAlt+N new · Alt+P pause · Alt+T scores\nAlt+S setup · Esc returns to files\n\nHigh scores save in tetris.conf.").to_owned(),
        _=>String::new(),
    };
    frame.render_widget(
        Paragraph::new(text).style(dos(0x70)),
        Rect::new(rect.x + 2, rect.y + 1, 46, 13),
    );
    button(
        frame,
        app,
        Rect::new(rect.x + 16, rect.bottom() - 2, 18, 1),
        " Enter / Esc: Back ",
        GameAction::Back,
    );
}

fn classic_button(
    frame: &mut impl Frame,
    app: &mut App,
    rect: Rect,
    label: &str,
    action: GameAction,
    focused: bool,
) {
    let profile = app.config.palette;
    frame.render_widget(
        Paragraph::new(" ".repeat(rect.width as usize)).style(profile.dos(0x00)),
        Rect::new(rect.x + 1, rect.y + 1, rect.width, 1),
    );
    super::dialog_style::button(
        frame,
        profile,
        rect,
        app.config.language.text(label),
        focused,
    );
    if !focused && profile == crate::palette::PaletteProfile::ClassicDn {
        use ratatui::text::{Line, Span};
        let label = app.config.language.text(label);
        let mut chars = label.chars();
        let first = chars.next().map(|c| c.to_string()).unwrap_or_default();
        let rest: String = chars.collect();
        let padding = usize::from(rect.width).saturating_sub(Span::raw(label).width());
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::raw(" ".repeat(padding / 2)),
                Span::styled(first, profile.dos(0x8e)),
                Span::styled(rest, profile.dos(0x8f)),
            ]))
            .style(profile.dos(0x8f)),
            rect,
        );
    }
    app.mouse.regions.push((rect, Target::Game(action)));
}
fn page(frame: &mut impl Frame, app: &mut App, width: u16, height: u16, title: &str) -> Rect {
    app.mouse.regions.clear();
    let rect = super::dialog_style::window(
        frame,
        app.config.palette,
        frame.area(),
        width,
        height,
        app.config.language.text(title),
    );
    super::dialog_style::close_button(
        frame,
        app.config.palette,
        rect,
        &mut app.mouse.regions,
        Target::Game(GameAction::Back),
    );
    rect
}
fn draw_scores(frame: &mut impl Frame, app: &mut App) {
    use crate::painting::Wrap;
    use ratatui::{layout::Alignment, text::Span};
    let rect = page(frame, app, 72, 20, "Tetris Top Ten");
    let language = app.config.language;
    let dos = app.config.palette.styles();
    let body = Rect::new(rect.x + 2, rect.y + 3, rect.width - 4, 12);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(dos(0x7f)),
        body,
    );
    let x = body.x + 1;
    let width = body.width - 2;
    let name_width = width - 33;
    let start_x = x + name_width + 1;
    let end_x = start_x + 6;
    let score_x = end_x + 6;
    for (label, x, w, alignment) in [
        ("Name", x, name_width, Alignment::Left),
        ("Start", start_x, 5, Alignment::Right),
        ("End", end_x, 5, Alignment::Right),
        ("Score", score_x, 20, Alignment::Right),
    ] {
        frame.render_widget(
            Paragraph::new(language.text(label))
                .alignment(alignment)
                .style(dos(0x7e)),
            Rect::new(x, rect.y + 2, w, 1),
        );
    }
    let Some(game) = app.desktop.game() else {
        return;
    };
    for (i, record) in game.records.iter().take(10).enumerate() {
        let name =
            crate::formatting::tail(&crate::filesystem::display_name(&record.name), name_width);
        let dots = usize::from(name_width).saturating_sub(Span::raw(&name).width());
        let row = format!(
            "{name}{} {:.>5} {:.>5} {:.>20}",
            ".".repeat(dots),
            record.start_level,
            record.level,
            record.score
        );
        frame.render_widget(
            Paragraph::new(row).style(dos(if game.highlighted == Some(i) {
                0x30
            } else {
                0x70
            })),
            Rect::new(x, body.y + 1 + i as u16, width, 1),
        );
    }
    if game.records.is_empty() {
        frame.render_widget(
            Paragraph::new(language.text("No finished games yet.")).style(dos(0x78)),
            Rect::new(x + 1, body.y + 1, width - 2, 1),
        );
    }
    if game.notice.is_empty() || game.notice == "Scores saved" {
        if let Some(record) = game.records.get(game.highlighted.unwrap_or(0)) {
            frame.render_widget(
                Paragraph::new(format!(
                    "{}: {}",
                    language.text("Lines"),
                    record.lines.map_or_else(|| "—".into(), |n| n.to_string())
                ))
                .style(dos(0x78)),
                Rect::new(x, rect.y + 15, width, 1),
            );
        }
        frame.render_widget(
            Paragraph::new(language.text(&game.notice)).style(dos(0x70)),
            Rect::new(x, rect.y + 16, width, 1),
        );
    } else {
        frame.render_widget(
            Paragraph::new(crate::filesystem::display_name(language.text(&game.notice)))
                .wrap(Wrap { trim: false })
                .style(dos(0x74)),
            Rect::new(x, rect.y + 15, width, 2),
        );
    }
    let retry = app.game_scores.has_pending();
    if retry {
        classic_button(
            frame,
            app,
            Rect::new(rect.x + 8, rect.y + 17, 20, 1),
            "Retry save",
            GameAction::RetrySave,
            false,
        );
    }
    classic_button(
        frame,
        app,
        Rect::new(rect.x + if retry { 43 } else { 29 }, rect.y + 17, 14, 1),
        "OK",
        GameAction::Back,
        true,
    );
}
const LEVEL_NAMES: [&str; 10] = [
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
];
fn draw_setup(frame: &mut impl Frame, app: &mut App, level: u8, preview: bool) {
    use ratatui::text::{Line, Span};
    let rect = page(frame, app, 72, 18, "Setup game");
    let language = app.config.language;
    let dos = app.config.palette.styles();
    let focus = app.desktop.game().map_or(0, |g| g.setup_focus);
    let right = rect.x + 42;
    frame.render_widget(
        Paragraph::new(language.text("Level")).style(dos(0x7f)),
        Rect::new(rect.x + 4, rect.y + 2, 34, 1),
    );
    for (i, label) in LEVEL_NAMES.iter().enumerate() {
        let selected = level == i as u8 + 1;
        let row = Rect::new(rect.x + 4, rect.y + 3 + i as u16, 35, 1);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    if selected { "(•) " } else { "( ) " },
                    dos(if selected && focus == 0 { 0x7f } else { 0x78 }),
                ),
                Span::styled(format!("{:>2}", i + 1), dos(0x7e)),
                Span::styled(
                    format!(" — {}", language.text(label)),
                    dos(if selected { 0x7f } else { 0x78 }),
                ),
            ])),
            row,
        );
        app.mouse
            .regions
            .push((row, Target::Game(GameAction::SetLevel(i as u8 + 1))));
    }
    frame.render_widget(
        Paragraph::new(language.text("Game style")).style(dos(0x70)),
        Rect::new(right, rect.y + 2, 26, 1),
    );
    frame.render_widget(
        Paragraph::new(format!("(•) {}", language.text("Classic Tetris"))).style(dos(0x78)),
        Rect::new(right, rect.y + 3, 26, 1),
    );
    frame.render_widget(
        Paragraph::new(language.text("Options")).style(dos(0x70)),
        Rect::new(right, rect.y + 6, 26, 1),
    );
    let check = Rect::new(right, rect.y + 7, 26, 1);
    frame.render_widget(
        Paragraph::new(format!(
            "[{}] {}",
            if preview { "✓" } else { " " },
            language.text("Piece preview")
        ))
        .style(dos(if focus == 1 { 0x7f } else { 0x78 })),
        check,
    );
    app.mouse
        .regions
        .push((check, Target::Game(GameAction::Preview)));
    frame.render_widget(
        Paragraph::new(language.text("OK starts a new game · Esc cancels")).style(dos(0x78)),
        Rect::new(rect.x + 4, rect.y + 16, rect.width - 8, 1),
    );
    classic_button(
        frame,
        app,
        Rect::new(right + 7, rect.y + 11, 16, 1),
        "OK",
        GameAction::Apply,
        focus != 3,
    );
    classic_button(
        frame,
        app,
        Rect::new(right + 7, rect.y + 14, 16, 1),
        "Cancel",
        GameAction::Back,
        focus == 3,
    );
}
