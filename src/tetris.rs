//! Classic game adapted from TETRIS.PAS by RIT Research Labs.
//! Retains the 12×19 well, seven figures, rotation sizes, delays and scoring.
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
mod controls;
pub mod dos_scores;
pub mod scores;
pub use controls::{action, key, open, paste, GameAction};

pub const BUTTONS: [(&str, GameAction); 4] = [
    ("~N~ew", GameAction::New),
    ("~S~etup", GameAction::Setup),
    ("~T~op 10", GameAction::Scores),
    ("~P~ause", GameAction::Pause),
];
pub const WIDTH: usize = 12;
pub const HEIGHT: usize = 19;
// Coordinates are (column, row), converted from the original one-based figures.
pub const FIGURES: [[(i32, i32); 4]; 7] = [
    [(2, 0), (2, 1), (2, 2), (1, 2)],
    [(1, 0), (1, 1), (1, 2), (2, 2)],
    [(1, 0), (1, 1), (1, 2), (1, 3)],
    [(1, 1), (2, 1), (1, 2), (2, 2)],
    [(1, 0), (1, 1), (2, 1), (1, 2)],
    [(1, 0), (1, 1), (2, 1), (2, 2)],
    [(2, 0), (2, 1), (1, 1), (1, 2)],
];
const ROTATION_SIZES: [i32; 7] = [4, 4, 4, 4, 3, 4, 4];
const DELAYS_MS: [u64; 10] = [800, 600, 500, 300, 250, 200, 150, 100, 50, 40];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Play,
    GameOver,
    Name,
    Help,
    Scores,
    Setup { level: u8, preview: bool },
}
#[derive(Clone, Copy, Debug)]
pub struct Piece {
    pub kind: usize,
    pub cells: [(i32, i32); 4],
    pub x: i32,
    pub y: i32,
}
impl Piece {
    fn new(kind: usize) -> Self {
        Self {
            kind,
            cells: FIGURES[kind],
            x: 4,
            y: -FIGURES[kind][0].1,
        }
    }
    pub fn positions(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        self.cells.iter().map(|(x, y)| (x + self.x, y + self.y))
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub name: String,
    pub start_level: u8,
    pub score: u64,
    pub lines: Option<u32>,
    pub level: u8,
}

pub struct Game {
    pub board: [[u8; WIDTH]; HEIGHT],
    pub piece: Piece,
    pub next: usize,
    pub score: u64,
    pub lines: u32,
    pub level: u8,
    pub start_level: u8,
    pub preview: bool,
    pub paused: bool,
    pub over: bool,
    pub screen: Screen,
    pub setup_focus: usize,
    pub records: Vec<Record>,
    pub name: String,
    pub name_cursor: usize,
    pub replace_name: bool,
    pub highlighted: Option<usize>,
    pub notice: String,
    pub submitted: bool,
    obscured: bool,
    random: u64,
    last_tick: Instant,
}
impl Default for Game {
    fn default() -> Self {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|time| time.as_nanos() as u64)
            .unwrap_or(1)
            ^ u64::from(std::process::id());
        Self::with_seed(seed, Instant::now())
    }
}
impl Game {
    pub fn with_seed(seed: u64, now: Instant) -> Self {
        let mut game = Self {
            board: [[0; WIDTH]; HEIGHT],
            piece: Piece::new(0),
            next: 0,
            score: 0,
            lines: 0,
            level: 1,
            start_level: 1,
            preview: true,
            paused: false,
            over: false,
            screen: Screen::Play,
            setup_focus: 0,
            records: Vec::new(),
            name: scores::ANONYMOUS.into(),
            name_cursor: scores::ANONYMOUS.len(),
            replace_name: true,
            highlighted: None,
            notice: String::new(),
            submitted: false,
            obscured: false,
            random: if seed == 0 { 1 } else { seed },
            last_tick: now,
        };
        game.restart(now);
        game
    }
    fn random_piece(&mut self) -> usize {
        // Independent pieces, like DN; this generator is only for the game.
        self.random ^= self.random << 13;
        self.random ^= self.random >> 7;
        self.random ^= self.random << 17;
        (self.random % 7) as usize
    }
    pub fn restart(&mut self, now: Instant) {
        self.board = [[0; WIDTH]; HEIGHT];
        self.piece = Piece::new(self.random_piece());
        self.next = self.random_piece();
        self.score = 0;
        self.lines = 0;
        self.start_level = self.start_level.clamp(1, 10);
        self.level = self.start_level;
        self.paused = false;
        self.over = false;
        self.submitted = false;
        self.highlighted = None;
        self.screen = Screen::Play;
        self.last_tick = now;
    }
    fn valid(&self, piece: &Piece) -> bool {
        piece.positions().all(|(x, y)| {
            x >= 0
                && x < WIDTH as i32
                && y < HEIGHT as i32
                && (y < 0 || self.board[y as usize][x as usize] == 0)
        })
    }
    pub fn running(&self) -> bool {
        !self.paused && !self.over && !self.obscured && self.screen == Screen::Play
    }
    pub fn translate(&mut self, dx: i32, dy: i32) -> bool {
        if !self.running() {
            return false;
        }
        let candidate = Piece {
            x: self.piece.x + dx,
            y: self.piece.y + dy,
            ..self.piece
        };
        if !self.valid(&candidate) {
            return false;
        }
        self.piece = candidate;
        true
    }
    pub fn rotate(&mut self) {
        if !self.running() {
            return;
        }
        let mut candidate = self.piece;
        for (x, y) in &mut candidate.cells {
            (*x, *y) = (*y, ROTATION_SIZES[candidate.kind] - 1 - *x);
        }
        // Original DN rejects a blocked rotation without moving the piece.
        if self.valid(&candidate) {
            self.piece = candidate;
        }
    }
    fn finish(&mut self) {
        self.over = true;
        self.screen = Screen::GameOver;
    }
    fn lock(&mut self) {
        if self.piece.positions().any(|(_, y)| y < 0) {
            self.finish();
            return;
        }
        for (x, y) in self.piece.positions() {
            self.board[y as usize][x as usize] = 15 - self.piece.kind as u8;
        }
        let multiplier = if self.preview { 7 } else { 10 };
        let height_score = (HEIGHT as i32 + 1 - self.piece.y).max(0) as u64 * 2;
        self.score = self
            .score
            .saturating_add((height_score + u64::from(self.level) * 6) * multiplier / 10);
        let mut cleared = 0;
        for (row, cells) in self.board.iter().enumerate() {
            if cells.iter().all(|cell| *cell != 0) {
                self.score = self
                    .score
                    .saturating_add((cleared + HEIGHT - row) as u64 * 30);
                cleared += 1;
            }
        }
        let mut remaining = [[0; WIDTH]; HEIGHT];
        let mut destination = HEIGHT;
        for row in self.board.iter().rev().filter(|row| row.contains(&0)) {
            destination -= 1;
            remaining[destination] = *row;
        }
        self.board = remaining;
        self.lines = self.lines.saturating_add(cleared as u32);
        if self.lines > u32::from(self.start_level) * 10 {
            self.level = (1
                + (self.lines - u32::from(self.start_level) * 10) / 20
                + u32::from(self.start_level))
            .min(10) as u8;
        }
        self.piece = Piece::new(self.next);
        self.next = self.random_piece();
        if !self.valid(&self.piece) {
            self.finish();
        }
    }
    pub fn fall(&mut self, now: Instant) {
        if !self.running() {
            return;
        }
        if !self.translate(0, 1) {
            self.lock();
        }
        self.last_tick = now;
    }
    pub fn drop(&mut self, now: Instant) {
        if !self.running() {
            return;
        }
        while self.translate(0, 1) {}
        self.lock();
        self.last_tick = now;
    }
    pub fn delay(&self) -> Duration {
        Duration::from_millis(DELAYS_MS[usize::from(self.level.clamp(1, 10) - 1)])
    }
    pub fn tick(&mut self, now: Instant) -> bool {
        if self.running() && now.saturating_duration_since(self.last_tick) >= self.delay() {
            self.fall(now);
            return true;
        }
        false
    }
    pub fn wait_duration(&self, now: Instant) -> Duration {
        if !self.running() {
            return Duration::from_millis(50);
        }
        self.delay()
            .saturating_sub(now.saturating_duration_since(self.last_tick))
            .min(Duration::from_millis(50))
    }
    pub fn set_obscured(&mut self, obscured: bool, now: Instant) {
        if self.obscured != obscured {
            self.last_tick = now;
            self.obscured = obscured;
        }
    }
    pub fn resume_clock(&mut self, now: Instant) {
        self.last_tick = now;
    }
}
