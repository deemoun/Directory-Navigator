use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use dn::{
    app::App,
    input,
    menu::{self, Action},
    mouse::{self, Target},
    tetris::{self, Game, GameAction, Piece, Record, Screen, FIGURES, HEIGHT, WIDTH},
    ui,
};
use ratatui::{backend::TestBackend, Terminal};
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
    assert_eq!(game.records.len(), 1);
    let board = game.board;
    game.drop(now);
    game.rotate();
    assert!(!game.translate(1, 0));
    assert_eq!(game.board, board);
    assert_eq!(game.records.len(), 1);
    for score in 0..20 {
        game.records.push(Record {
            score,
            lines: 0,
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
    assert!(game
        .records
        .windows(2)
        .all(|records| records[0].score >= records[1].score));
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
    assert!(app.tetris.is_some());
    key(&mut app, KeyCode::F(3));
    assert!(app.tetris.as_ref().unwrap().paused);
    key(&mut app, KeyCode::F(3));
    assert!(!app.tetris.as_ref().unwrap().paused);
    key(&mut app, KeyCode::F(2));
    assert_eq!(app.panels[0].sort, sort);
    key(&mut app, KeyCode::F(5));
    assert!(matches!(
        app.tetris.as_ref().unwrap().screen,
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
    assert!(app.prompt.is_none() && app.viewer.is_none() && app.job.is_none());
    assert_eq!(app.panels[0].selected.len(), 1);
    assert_eq!(app.panels[0].cursor, 1);
    key(&mut app, KeyCode::Esc);
    assert!(app.tetris.is_none());
    assert_eq!(fs::read(temp.0.join("keep.txt"))?, b"unchanged");
    menu::open(&mut app, 0);
    let row = menu::items(0)
        .iter()
        .position(|item| item.action == Action::Game)
        .unwrap();
    menu::activate(&mut app, 0, row);
    assert!(app.tetris.is_some() && app.menu.is_none());
    key(&mut app, KeyCode::Esc);
    menu::open(&mut app, 3);
    input::handle(&mut app, KeyEvent::new(KeyCode::F(9), KeyModifiers::ALT));
    assert!(app.tetris.is_some() && app.menu.is_none());
    key(&mut app, KeyCode::Esc);
    Ok(())
}

#[test]
fn game_dialog_mouse_setup_cancel_apply_scores_and_resize() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    tetris::open(&mut app);
    for (width, height) in [(80, 25), (120, 40), (161, 30), (40, 10), (80, 25)] {
        render(&mut app, width, height)?;
        assert_eq!(
            app.tetris.as_ref().unwrap().running(),
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
    click(&mut app, GameAction::LevelUp);
    click(&mut app, GameAction::Preview);
    click(&mut app, GameAction::Back);
    assert_eq!(app.tetris.as_ref().unwrap().start_level, 1);
    assert!(app.tetris.as_ref().unwrap().preview);
    render(&mut app, 80, 25)?;
    click(&mut app, GameAction::Setup);
    key(&mut app, KeyCode::Right);
    key(&mut app, KeyCode::Char(' '));
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.tetris.as_ref().unwrap().start_level, 2);
    assert!(!app.tetris.as_ref().unwrap().preview);
    key(&mut app, KeyCode::Char('+'));
    assert_eq!(app.tetris.as_ref().unwrap().level, 3);
    key(&mut app, KeyCode::Char('*'));
    assert!(app.tetris.as_ref().unwrap().preview);
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
    assert_eq!(app.tetris.as_ref().unwrap().screen, Screen::Scores);
    key(&mut app, KeyCode::Enter);
    app.tetris.as_mut().unwrap().records.push(Record {
        score: 123,
        lines: 4,
        level: 2,
    });
    render(&mut app, 80, 25)?;
    click(&mut app, GameAction::Close);
    tetris::open(&mut app);
    assert_eq!(app.tetris.as_ref().unwrap().records[0].score, 123);
    key(&mut app, KeyCode::F(10));
    assert!(app.quit);
    Ok(())
}
