use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use dn::testing::{backend::TestBackend, Terminal};
use dn::{
    app::App,
    input,
    menu::{self, Action},
    mouse::{self, Target},
    tetris::{self, Game, GameAction, Piece, Record, Screen, FIGURES, HEIGHT, WIDTH},
    ui,
};
use std::{
    fs, io,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

fn piece(game: &mut Game, kind: usize, x: i32, y: i32) {
    game.piece = Piece {
        kind,
        cells: FIGURES[kind],
        x,
        y,
    };
}

#[test]
fn original_figures_rotations_walls_and_blocked_moves() {
    let now = Instant::now();
    let mut game = Game::with_seed(7, now);
    for kind in 0..7 {
        piece(&mut game, kind, 4, 5);
        let initial = game.piece.cells;
        for _ in 0..4 {
            game.rotate();
            let occupied: std::collections::BTreeSet<_> = game.piece.positions().collect();
            assert_eq!(occupied.len(), 4);
        }
        assert_eq!(game.piece.cells, initial);
    }
    piece(&mut game, 2, -1, 5);
    let initial = game.piece.cells;
    game.rotate();
    assert_eq!(game.piece.cells, initial);
    assert!(!game.translate(-1, 0));
    while game.translate(1, 0) {}
    assert!(game
        .piece
        .positions()
        .all(|(x, _)| (0..WIDTH as i32).contains(&x)));
    let (x, y) = game.piece.positions().next().unwrap();
    game.board[y as usize + 1][x as usize] = 7;
    assert!(!game.translate(0, 1));
}

#[test]
fn one_to_four_lines_compact_and_use_original_scoring() {
    let now = Instant::now();
    for lines in 1..=4 {
        let mut game = Game::with_seed(1, now);
        game.board[0][0] = 9;
        for row in HEIGHT - lines..HEIGHT {
            game.board[row] = [7; WIDTH];
            game.board[row][5] = 0;
        }
        piece(&mut game, 2, 4, HEIGHT as i32 - 4);
        game.drop(now);
        assert_eq!(game.lines, lines as u32);
        assert_eq!(game.score, 11 + (lines * lines * 30) as u64);
        assert_eq!(game.board[lines][0], 9);
        assert!(game.board[..lines].iter().all(|row| *row == [0; WIDTH]));
        assert!(game.board.iter().all(|row| row.contains(&0)));
    }
}

#[test]
fn game_over_records_restart_and_ceiling_collision() {
    let now = Instant::now();
    let mut game = Game::with_seed(1, now);
    game.board[0][6] = 7;
    game.next = 0;
    piece(&mut game, 2, 4, HEIGHT as i32 - 4);
    game.drop(now);
    assert!(game.over);
    assert_eq!(game.screen, Screen::GameOver);
    assert!(game.records.is_empty());
    let board = game.board;
    game.drop(now);
    game.rotate();
    assert!(!game.translate(1, 0));
    assert_eq!(game.board, board);
    assert_eq!(game.screen, Screen::GameOver);
    assert!(game.records.is_empty());
    for score in (1..=10).rev() {
        game.records.push(Record {
            name: "Anonymous".into(),
            start_level: 1,
            score,
            lines: Some(0),
            level: 1,
        });
    }
    game.restart(now);
    assert!(!game.over && !game.paused);
    assert_eq!(game.board, [[0; WIDTH]; HEIGHT]);
    assert_eq!(game.score, 0);
    piece(&mut game, 2, 4, -2);
    game.board[2][5] = 7;
    game.fall(now);
    assert!(game.over);
    assert_eq!(game.records.len(), 10);
}

#[test]
fn gravity_pause_subwindows_and_resize_suspend_without_catchup() {
    let now = Instant::now();
    let mut game = Game::with_seed(1, now);
    let initial = game.piece.y;
    assert!(!game.tick(now + Duration::from_millis(799)));
    assert!(game.tick(now + Duration::from_millis(800)));
    assert_eq!(game.piece.y, initial + 1);
    game.paused = true;
    assert!(!game.tick(now + Duration::from_secs(100)));
    assert!(!game.translate(1, 0));
    game.paused = false;
    game.resume_clock(now + Duration::from_secs(100));
    assert!(!game.tick(now + Duration::from_secs(100)));
    game.screen = Screen::Help;
    assert!(!game.tick(now + Duration::from_secs(200)));
    game.screen = Screen::Play;
    game.set_obscured(true, now);
    assert!(!game.tick(now + Duration::from_secs(300)));
    game.set_obscured(false, now + Duration::from_secs(300));
    assert!(!game.tick(now + Duration::from_secs(300)));
    assert!(game.tick(now + Duration::from_millis(300_800)));
    game.level = 10;
    assert_eq!(game.delay(), Duration::from_millis(40));
    assert!(game.wait_duration(now + Duration::from_millis(300_810)) <= Duration::from_millis(30));
}

#[test]
fn level_progression_preview_bonus_and_seeded_pieces() {
    let now = Instant::now();
    let mut game = Game::with_seed(0, now);
    let other = Game::with_seed(0, now);
    assert_eq!(game.piece.kind, other.piece.kind);
    assert_eq!(game.next, other.next);
    game.preview = false;
    game.lines = 10;
    game.board[HEIGHT - 1] = [7; WIDTH];
    game.board[HEIGHT - 1][5] = 0;
    piece(&mut game, 2, 4, HEIGHT as i32 - 4);
    game.drop(now);
    assert_eq!(game.level, 2);
    assert_eq!(game.score, 46);
    game.lines = u32::MAX - 1;
    game.score = u64::MAX - 1;
    game.board = [[0; WIDTH]; HEIGHT];
    for row in HEIGHT - 4..HEIGHT {
        game.board[row] = [7; WIDTH];
        game.board[row][5] = 0;
    }
    piece(&mut game, 2, 4, HEIGHT as i32 - 4);
    game.drop(now);
    assert_eq!(game.lines, u32::MAX);
    assert_eq!(game.score, u64::MAX);
    assert_eq!(game.level, 10);
}

struct Temp(PathBuf);
impl Temp {
    fn new() -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "dn-tetris-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path)?;
        fs::write(path.join("keep.txt"), b"unchanged")?;
        Ok(Self(path))
    }
    fn app(&self) -> io::Result<App> {
        let mut app = App::new(self.0.clone())?;
        app.config_store = dn::config::Store::new(Some(self.0.join("config.conf")));
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.panels.iter().any(|panel| panel.loading) {
            app.poll();
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
        Ok(app)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn key(app: &mut App, code: KeyCode) {
    input::handle(app, KeyEvent::new(code, KeyModifiers::NONE));
}
fn render(app: &mut App, width: u16, height: u16) -> io::Result<Terminal<TestBackend>> {
    let mut terminal = Terminal::new(TestBackend::new(width, height))?;
    terminal.draw(|frame| ui::draw(frame, app))?;
    assert!(app
        .mouse
        .regions
        .iter()
        .all(|(rect, _)| rect.right() <= width && rect.bottom() <= height));
    Ok(terminal)
}
fn click(app: &mut App, action: GameAction) {
    let rect = app
        .mouse
        .regions
        .iter()
        .find(|(_, target)| *target == Target::Game(action))
        .unwrap()
        .0;
    mouse::handle(
        app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: rect.x,
            row: rect.y,
            modifiers: KeyModifiers::NONE,
        },
    );
}

#[test]
fn original_shortcut_menu_and_game_keys_preserve_files_and_panel_state() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    app.panels[0].cursor = 1;
    app.panels[0].toggle_selection();
    let sort = app.panels[0].sort;
    input::handle(&mut app, KeyEvent::new(KeyCode::F(9), KeyModifiers::ALT));
    assert!(app.desktop.game().is_some());
    key(&mut app, KeyCode::F(3));
    assert!(app.desktop.game().unwrap().paused);
    key(&mut app, KeyCode::F(3));
    assert!(!app.desktop.game().unwrap().paused);
    key(&mut app, KeyCode::F(2));
    assert_eq!(app.panels[0].sort, sort);
    key(&mut app, KeyCode::F(5));
    assert!(matches!(
        app.desktop.game().unwrap().screen,
        Screen::Setup { .. }
    ));
    key(&mut app, KeyCode::Esc);
    for code in [
        KeyCode::F(6),
        KeyCode::F(7),
        KeyCode::F(8),
        KeyCode::Delete,
        KeyCode::Insert,
    ] {
        key(&mut app, code);
    }
    assert!(
        app.modals.prompt().is_none() && app.desktop.active_viewer().is_none() && app.job.is_none()
    );
    assert_eq!(app.panels[0].selected.len(), 1);
    assert_eq!(app.panels[0].cursor, 1);
    key(&mut app, KeyCode::Esc);
    assert!(app.desktop.game().is_none());
    assert_eq!(fs::read(temp.0.join("keep.txt"))?, b"unchanged");
    menu::open(&mut app, 0);
    let row = menu::items(0)
        .iter()
        .position(|item| item.action == Action::Game)
        .unwrap();
    menu::activate(&mut app, 0, row);
    assert!(app.desktop.game().is_some() & app.modals.menu().is_none());
    key(&mut app, KeyCode::Esc);
    menu::open(&mut app, 3);
    input::handle(&mut app, KeyEvent::new(KeyCode::F(9), KeyModifiers::ALT));
    assert!(app.desktop.game().is_some() & app.modals.menu().is_none());
    key(&mut app, KeyCode::Esc);
    Ok(())
}

#[test]
fn game_dialog_mouse_setup_cancel_apply_scores_and_resize() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    tetris::open(&mut app);
    for (width, height) in [(80, 25), (120, 40), (161, 30), (40, 10), (80, 25)] {
        let terminal = render(&mut app, width, height)?;
        if width == 80 {
            let bar: String = (0..80)
                .map(|x| terminal.backend().buffer()[(x, height - 1)].symbol())
                .collect();
            let expected = [
                "F1 Help",
                "F2 New",
                "F3 Pause",
                "F4 Scores",
                "F5 Setup",
                "+ Level",
                "* Preview",
                "Esc Close",
            ]
            .map(|s| format!("{s:<10}"))
            .concat();
            assert_eq!(bar, expected);
        }
        assert_eq!(
            app.desktop.game().unwrap().running(),
            width >= 80 && height >= 25
        );
        if width >= 80 {
            assert!(app
                .mouse
                .regions
                .iter()
                .all(|(_, target)| matches!(target, Target::Game(_))));
        } else {
            assert!(app.mouse.regions.is_empty());
        }
    }
    click(&mut app, GameAction::Setup);
    render(&mut app, 80, 25)?;
    click(&mut app, GameAction::SetLevel(2));
    click(&mut app, GameAction::Preview);
    click(&mut app, GameAction::Back);
    assert_eq!(app.desktop.game().unwrap().start_level, 1);
    assert!(app.desktop.game().unwrap().preview);
    render(&mut app, 80, 25)?;
    click(&mut app, GameAction::Setup);
    key(&mut app, KeyCode::Right);
    key(&mut app, KeyCode::Char(' '));
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.desktop.game().unwrap().start_level, 2);
    assert!(!app.desktop.game().unwrap().preview);
    key(&mut app, KeyCode::Char('+'));
    assert_eq!(app.desktop.game().unwrap().level, 3);
    key(&mut app, KeyCode::Char('*'));
    assert!(app.desktop.game().unwrap().preview);
    key(&mut app, KeyCode::F(1));
    render(&mut app, 80, 25)?;
    assert!(app
        .mouse
        .regions
        .iter()
        .all(|(_, target)| *target == Target::Game(GameAction::Back)));
    click(&mut app, GameAction::Back);
    key(&mut app, KeyCode::F(4));
    render(&mut app, 80, 25)?;
    assert_eq!(app.desktop.game().unwrap().screen, Screen::Scores);
    key(&mut app, KeyCode::Enter);
    app.game_scores.add(Record {
        name: "Anonymous".into(),
        start_level: 1,
        score: 123,
        lines: Some(4),
        level: 2,
    })?;
    render(&mut app, 80, 25)?;
    click(&mut app, GameAction::Close);
    tetris::open(&mut app);
    assert_eq!(app.desktop.game().unwrap().records[0].score, 123);
    key(&mut app, KeyCode::F(10));
    assert!(app.quit);
    Ok(())
}

fn ended(app: &mut App, score: u64) {
    let g = app.desktop.game_mut().unwrap();
    g.score = score;
    g.lines = 4;
    g.start_level = 2;
    g.level = 3;
    g.over = true;
    g.screen = Screen::GameOver;
    g.submitted = false;
}
#[test]
fn final_score_name_entry_saved_immediately_and_reloaded() -> io::Result<()> {
    let t = Temp::new()?;
    let mut app = t.app()?;
    app.config.save_on_exit = false;
    tetris::open(&mut app);
    ended(&mut app, 1234);
    let terminal = render(&mut app, 80, 25)?;
    let screen: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect();
    assert!(screen.contains("Final score: 1234"));
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.desktop.game().unwrap().screen, Screen::Name);
    key(&mut app, KeyCode::F(2));
    assert_eq!(app.desktop.game().unwrap().screen, Screen::Name);
    input::paste(&mut app, "Дмитрий, 世界 👩‍💻");
    render(&mut app, 80, 25)?;
    click(&mut app, GameAction::SubmitName);
    let g = app.desktop.game().unwrap();
    assert_eq!(g.screen, Screen::Scores);
    assert_eq!(g.highlighted, Some(0));
    assert_eq!(g.records[0].name, "Дмитрий, 世界 👩‍💻");
    assert_eq!(g.records[0].start_level, 2);
    let path = t.0.join("tetris.conf");
    assert!(fs::read_to_string(&path)?.contains("Дмитрий, 世界 👩‍💻"));
    assert!(!t.0.join("config.conf").exists());
    tetris::action(&mut app, GameAction::SubmitName);
    assert_eq!(app.game_scores.board.records.len(), 1);
    let mut other = t.app()?;
    tetris::open(&mut other);
    assert_eq!(
        other.desktop.game().unwrap().records,
        app.game_scores.board.records
    );
    key(&mut other, KeyCode::F(4));
    for (w, h) in [(80, 25), (120, 40), (40, 10), (80, 25)] {
        render(&mut other, w, h)?;
    }
    Ok(())
}
#[test]
fn score_qualification_ties_anonymous_and_grapheme_name_limits() -> io::Result<()> {
    let t = Temp::new()?;
    let mut app = t.app()?;
    tetris::open(&mut app);
    ended(&mut app, 0);
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.desktop.game().unwrap().screen, Screen::Scores);
    assert!(!t.0.join("tetris.conf").exists());
    ended(&mut app, 100);
    key(&mut app, KeyCode::Enter);
    input::paste(&mut app, "é👩‍💻");
    key(&mut app, KeyCode::Backspace);
    assert_eq!(app.desktop.game().unwrap().name, "é");
    input::paste(&mut app, "\n");
    assert_eq!(app.desktop.game().unwrap().name, "é");
    input::paste(&mut app, &"x".repeat(31));
    assert_eq!(app.desktop.game().unwrap().name, "é");
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.game_scores.board.records[0].name, "Anonymous");
    for score in 1..=10 {
        app.desktop.game_mut().unwrap().records.push(Record {
            name: "other".into(),
            start_level: 1,
            score: score * 100,
            lines: Some(0),
            level: 1,
        });
    }
    app.desktop
        .game_mut()
        .unwrap()
        .records
        .sort_by_key(|r| std::cmp::Reverse(r.score));
    app.desktop.game_mut().unwrap().records.truncate(10);
    let threshold = app.desktop.game().unwrap().records.last().unwrap().score;
    ended(&mut app, threshold);
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.desktop.game().unwrap().screen, Screen::Scores);
    ended(&mut app, threshold + 1);
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.desktop.game().unwrap().screen, Screen::Name);
    Ok(())
}
#[test]
fn score_files_merge_instances_and_refuse_corruption_symlinks_and_readonly() -> io::Result<()> {
    use dn::tetris::scores::{Board, Store};
    let t = Temp::new()?;
    let path = t.0.join("tetris.conf");
    let mut a = Store::default();
    let mut b = Store::default();
    a.load(Some(path.clone()))?;
    b.load(Some(path.clone()))?;
    for (store, score) in [(&mut a, 20), (&mut b, 30)] {
        store.add(Record {
            name: format!("name,{score}"),
            score,
            lines: Some(1),
            start_level: 1,
            level: 2,
        })?;
        store.save()?;
    }
    let contents = fs::read(&path)?;
    assert_eq!(Board::parse(&contents)?.records.len(), 2);
    b.save()?;
    assert_eq!(Board::parse(&fs::read(&path)?)?.records.len(), 2);
    for invalid in [
        b"tetris_version=3\n".as_slice(),
        b"tetris_version=1\nscore=1,0,11,1,bad\n",
        b"tetris_version=1\nscore=18446744073709551616,0,1,1,bad\n",
        b"tetris_version=1\nlast_name=bad\tname\n",
    ] {
        assert!(Board::parse(invalid).is_err());
    }
    fs::write(&path, "broken file")?;
    assert!(b.save().is_err());
    b.add(Record {
        name: "pending".into(),
        score: 100,
        lines: Some(0),
        start_level: 1,
        level: 1,
    })?;
    assert!(b.save().is_err());
    assert!(b.has_pending());
    assert_eq!(fs::read_to_string(&path)?, "broken file");
    fs::write(&path, &contents)?;
    b.save()?;
    assert!(!b.has_pending());
    assert_eq!(Board::parse(&fs::read(&path)?)?.records.len(), 3);
    let before = fs::read(&path)?;
    b.add(Record {
        name: "readonly pending".into(),
        score: 200,
        lines: Some(0),
        start_level: 1,
        level: 1,
    })?;
    let mut permissions = fs::metadata(&path)?.permissions();
    let original = permissions.clone();
    permissions.set_readonly(true);
    fs::set_permissions(&path, permissions)?;
    assert!(b.save().is_err());
    assert_eq!(fs::read(&path)?, before);
    fs::set_permissions(&path, original)?;
    fs::write(path.with_file_name("tetris.conf.lock"), "busy")?;
    assert!(b.save().is_err());
    assert_eq!(fs::read(&path)?, before);
    #[cfg(unix)]
    {
        let link = t.0.join("link.conf");
        std::os::unix::fs::symlink(&path, &link)?;
        let mut linked = Store::default();
        assert!(linked.load(Some(link)).is_err());
        assert_eq!(fs::read(&path)?, before);
    }
    Ok(())
}
#[test]
fn failed_score_save_is_visible_retryable_and_survives_game_close() -> io::Result<()> {
    let t = Temp::new()?;
    let mut app = t.app()?;
    let path = t.0.join("tetris.conf");
    fs::write(&path, "invalid")?;
    tetris::open(&mut app);
    ended(&mut app, 999);
    key(&mut app, KeyCode::Enter);
    input::paste(&mut app, "Winner");
    key(&mut app, KeyCode::Enter);
    assert!(app.game_scores.has_pending());
    assert!(app.desktop.game().unwrap().notice.contains("not saved"));
    assert_eq!(fs::read_to_string(&path)?, "invalid");
    tetris::action(&mut app, GameAction::Close);
    tetris::open(&mut app);
    assert_eq!(app.desktop.game().unwrap().records[0].name, "Winner");
    fs::write(&path, "tetris_version=1\nlast_name=Anonymous\n")?;
    tetris::action(&mut app, GameAction::Scores);
    render(&mut app, 80, 25)?;
    click(&mut app, GameAction::RetrySave);
    assert!(!app.game_scores.has_pending());
    assert_eq!(
        dn::tetris::scores::Board::parse(&fs::read(&path)?)?.records[0].name,
        "Winner"
    );
    Ok(())
}

#[test]
fn reference_game_scores_and_setup_layouts_preserve_colors_values_and_mouse_bounds(
) -> io::Result<()> {
    use dn::{localization::Language, palette::PaletteProfile};
    let temp = Temp::new()?;
    for language in [Language::English, Language::Russian] {
        for profile in [PaletteProfile::ClassicDn, PaletteProfile::Terminal] {
            let mut app = temp.app()?;
            app.config.language = language;
            app.config.palette = profile;
            tetris::open(&mut app);
            let game = app.desktop.game_mut().unwrap();
            game.records = vec![
                Record {
                    name: "Winner 界".into(),
                    start_level: 5,
                    level: 10,
                    score: u64::MAX,
                    lines: None,
                },
                Record {
                    name: "Second".into(),
                    start_level: 1,
                    level: 2,
                    score: 384,
                    lines: Some(2),
                },
            ];
            game.highlighted = Some(1);
            for (w, h) in [(80, 25), (132, 40), (80, 25)] {
                let terminal = render(&mut app, w, h)?;
                let text: String = terminal
                    .backend()
                    .buffer()
                    .content
                    .iter()
                    .map(|c| c.symbol())
                    .collect();
                for label in ["Navigator's game", "Info", "Next", "Best", "Top 10"] {
                    assert!(text.contains(language.text(label)), "{label}");
                }
                let close = app
                    .mouse
                    .regions
                    .iter()
                    .find(|(_, t)| *t == Target::Game(GameAction::Close))
                    .unwrap()
                    .0;
                let origin = (close.x - 2, close.y);
                let b = terminal.backend().buffer();
                assert_eq!(b[(origin.0 + 1, origin.1 + 1)].fg, profile.color(15));
                assert_eq!(b[(origin.0 + 28, origin.1 + 13)].fg, profile.color(14));
                for action in [
                    GameAction::New,
                    GameAction::Setup,
                    GameAction::Scores,
                    GameAction::Pause,
                    GameAction::Close,
                ] {
                    assert!(app
                        .mouse
                        .regions
                        .iter()
                        .any(|(_, t)| *t == Target::Game(action)));
                }
                tetris::action(&mut app, GameAction::Scores);
                let terminal = render(&mut app, w, h)?;
                let text: String = terminal
                    .backend()
                    .buffer()
                    .content
                    .iter()
                    .map(|c| c.symbol())
                    .collect();
                for label in ["Tetris Top Ten", "Name", "Start", "End", "Score"] {
                    assert!(text.contains(language.text(label)), "{label}");
                }
                assert!(text.contains("18446744073709551615"));
                assert!(text.contains("Winner 界"));
                assert!(text.contains("...."));
                assert!(!app
                    .mouse
                    .regions
                    .iter()
                    .any(|(_, t)| *t == Target::Game(GameAction::RetrySave)));
                let ok = app
                    .mouse
                    .regions
                    .iter()
                    .find(|(_, t)| *t == Target::Game(GameAction::Back))
                    .unwrap()
                    .0;
                // The border close control cancels only this page and retains the game.
                mouse::handle(
                    &mut app,
                    MouseEvent {
                        kind: MouseEventKind::Down(MouseButton::Left),
                        column: ok.x,
                        row: ok.y,
                        modifiers: KeyModifiers::NONE,
                    },
                );
                assert_eq!(app.desktop.game().unwrap().screen, Screen::Play);
                tetris::action(&mut app, GameAction::Setup);
                let terminal = render(&mut app, w, h)?;
                let text: String = terminal
                    .backend()
                    .buffer()
                    .content
                    .iter()
                    .map(|c| c.symbol())
                    .collect();
                for label in [
                    "Setup game",
                    "Game style",
                    "Classic Tetris",
                    "Piece preview",
                    "Sanitarium",
                ] {
                    assert!(text.contains(language.text(label)), "{label}");
                }
                assert!(!text.contains("Pentix"));
                for level in 1..=10 {
                    assert!(app
                        .mouse
                        .regions
                        .iter()
                        .any(|(_, t)| *t == Target::Game(GameAction::SetLevel(level))));
                }
                let apply = app
                    .mouse
                    .regions
                    .iter()
                    .find(|(_, t)| *t == Target::Game(GameAction::Apply))
                    .unwrap()
                    .0;
                assert_eq!(
                    terminal.backend().buffer()[(apply.x + 1, apply.y + 1)].bg,
                    profile.color(8)
                );
                key(&mut app, KeyCode::Esc);
                assert_eq!(app.desktop.game().unwrap().records[0].score, u64::MAX);
            }
            tetris::action(&mut app, GameAction::Setup);
            render(&mut app, 40, 10)?;
            assert!(app.mouse.regions.is_empty());
            assert!(matches!(
                app.desktop.game().unwrap().screen,
                Screen::Setup { .. }
            ));
            render(&mut app, 80, 25)?;
            key(&mut app, KeyCode::Esc);
        }
    }
    Ok(())
}

#[test]
fn setup_radio_choices_cancel_focus_and_apply_preserve_game_and_saved_records() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    tetris::open(&mut app);
    let game = app.desktop.game_mut().unwrap();
    game.board[HEIGHT - 1][0] = 14;
    game.score = 480;
    game.lines = 2;
    game.paused = true;
    let board = game.board;
    tetris::action(&mut app, GameAction::Setup);
    render(&mut app, 80, 25)?;
    click(&mut app, GameAction::SetLevel(10));
    click(&mut app, GameAction::Preview);
    assert_eq!(
        app.desktop.game().unwrap().screen,
        Screen::Setup {
            level: 10,
            preview: false
        }
    );
    assert!(!app
        .desktop
        .game_mut()
        .unwrap()
        .tick(Instant::now() + Duration::from_secs(600)));
    for _ in 0..3 {
        key(&mut app, KeyCode::Tab);
    }
    assert_eq!(app.desktop.game().unwrap().setup_focus, 3);
    key(&mut app, KeyCode::Enter);
    let game = app.desktop.game().unwrap();
    assert_eq!(game.screen, Screen::Play);
    assert_eq!(game.board, board);
    assert_eq!(
        (
            game.score,
            game.lines,
            game.level,
            game.start_level,
            game.preview,
            game.paused
        ),
        (480, 2, 1, 1, true, true)
    );
    tetris::action(&mut app, GameAction::Setup);
    key(&mut app, KeyCode::Char('0'));
    assert!(matches!(
        app.desktop.game().unwrap().screen,
        Screen::Setup { level: 10, .. }
    ));
    key(&mut app, KeyCode::Home);
    key(&mut app, KeyCode::Down);
    assert!(matches!(
        app.desktop.game().unwrap().screen,
        Screen::Setup { level: 2, .. }
    ));
    key(&mut app, KeyCode::Up);
    key(&mut app, KeyCode::End);
    key(&mut app, KeyCode::Char('5'));
    key(&mut app, KeyCode::Tab);
    key(&mut app, KeyCode::Char(' '));
    key(&mut app, KeyCode::Tab);
    assert_eq!(app.desktop.game().unwrap().setup_focus, 2);
    key(&mut app, KeyCode::Enter);
    let game = app.desktop.game().unwrap();
    assert_eq!(game.screen, Screen::Play);
    assert_eq!(
        (
            game.level,
            game.start_level,
            game.preview,
            game.paused,
            game.score,
            game.lines
        ),
        (5, 5, false, false, 0, 0)
    );
    assert_eq!(game.board, [[0; WIDTH]; HEIGHT]);
    assert!(!temp.0.join("tetris.conf").exists());
    assert_eq!(fs::read(temp.0.join("keep.txt"))?, b"unchanged");
    Ok(())
}
