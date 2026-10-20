//! Star Sky follows IDLERS.PAS: radial, accelerating stars and DOS glyph stages.
use crate::app::App;
use crossterm::event::{Event, KeyCode, KeyEventKind, MouseEventKind};
use std::time::{Duration, Instant};

pub const DEFAULT_DELAY: u8 = 10;
pub const DELAYS: [u8; 7] = [0, 1, 2, 5, DEFAULT_DELAY, 30, 60];
pub const PREVIEW: usize = DELAYS.len();
pub const ACCEPT: usize = PREVIEW + 1;
pub const CANCEL: usize = PREVIEW + 2;
const CONTROL_COUNT: usize = CANCEL + 1;
pub const GLYPHS: [char; 6] = [' ', '·', '∙', '•', '♦', '☼'];
#[derive(Default)]
pub struct Setup {
    pub delay: u8,
    pub cursor: usize,
}
pub struct State {
    pub active: bool,
    pub setup: Option<Setup>,
    last_input: Instant,
    next_frame: Instant,
    stars: Vec<Star>,
    size: (u16, u16),
    seed: u64,
}
struct Star {
    x: f64,
    y: f64,
    dx: f64,
    dy: f64,
    stage: u16,
    bright: bool,
}
impl Default for State {
    fn default() -> Self {
        let now = Instant::now();
        Self {
            active: false,
            setup: None,
            last_input: now,
            next_frame: now,
            stars: Vec::new(),
            size: (0, 0),
            seed: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(1, |d| d.as_nanos() as u64 | 1),
        }
    }
}
impl State {
    pub fn activity(&mut self, now: Instant) -> bool {
        self.last_input = now;
        let consumed = self.active;
        self.active = false;
        consumed
    }
    pub fn start(&mut self, now: Instant) {
        self.active = true;
        self.stars.clear();
        self.size = (0, 0);
        self.next_frame = now;
    }
    pub fn tick(&mut self, now: Instant, minutes: u8, allowed: bool) -> bool {
        if !self.active {
            if !allowed {
                self.last_input = now;
                return false;
            }
            if minutes == 0
                || now.saturating_duration_since(self.last_input)
                    < Duration::from_secs(u64::from(minutes) * 60)
            {
                return false;
            }
            self.start(now);
        }
        if now < self.next_frame {
            return false;
        }
        self.next_frame = now + Duration::from_millis(50);
        let (w, h) = self.size;
        if w == 0 || h == 0 {
            return true;
        }
        for i in 0..self.stars.len() {
            let s = &mut self.stars[i];
            s.x += s.dx * f64::from(s.stage) / 12.0;
            s.y += s.dy * f64::from(s.stage) / 12.0;
            s.stage = s.stage.saturating_add(1);
            if s.x < 0.0 || s.y < 0.0 || s.x >= f64::from(w) || s.y >= f64::from(h) {
                self.stars[i] = self.star();
            }
        }
        true
    }
    fn random(&mut self, bound: u64) -> u64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        self.seed % bound
    }
    fn star(&mut self) -> Star {
        let angle = self.random(360) as f64 * std::f64::consts::PI / 180.0;
        let (w, h) = self.size;
        let speed = (u64::from(w) / 44 + self.random(3)).max(1) as f64;
        let radius = (1 + self.random(2)) as f64 * (1.0 + 131.0 / f64::from(w.max(1)));
        let (dx, dy) = (angle.cos(), angle.sin());
        Star {
            x: f64::from(w) / 2.0 + dx * speed / radius,
            y: f64::from(h) / 2.0 + dy * speed / radius,
            dx,
            dy,
            stage: 10,
            bright: self.random(5) == 4,
        }
    }
    pub fn resize(&mut self, w: u16, h: u16) {
        if self.size == (w, h) {
            return;
        }
        self.size = (w, h);
        self.stars.clear();
        if w == 0 || h == 0 {
            return;
        }
        // Original storage held 200 stars; bound modern wide terminals too.
        let count = usize::from(w)
            .saturating_mul(2)
            .saturating_sub(64)
            .clamp(1, 200);
        for _ in 0..count {
            let star = self.star();
            self.stars.push(star);
        }
    }
    pub fn cells(&self) -> impl Iterator<Item = (u16, u16, char, u8)> + '_ {
        let (w, h) = self.size;
        self.stars.iter().filter_map(move |s| {
            if s.x < 0.0 || s.y < 0.0 || s.x >= f64::from(w) || s.y >= f64::from(h) {
                return None;
            }
            let distance = ((s.x - f64::from(w) / 2.0).abs() * 8.0 / f64::from(w.max(1)))
                .max((s.y - f64::from(h) / 2.0).abs() * 8.0 / f64::from(h.max(1)));
            Some((
                s.x as u16,
                s.y as u16,
                GLYPHS[(distance as usize + 1).min(5)],
                if s.bright { 15 } else { 7 },
            ))
        })
    }
}
/// Wake-up input is consumed; resize/focus notifications do not count as activity.
pub fn event(app: &mut App, event: &Event, now: Instant) -> bool {
    let input = match event {
        Event::Key(key) => key.kind != KeyEventKind::Release,
        Event::Paste(_) => true,
        Event::Mouse(mouse) => !matches!(mouse.kind, MouseEventKind::Up(_)),
        _ => false,
    };
    if input && app.screensaver.activity(now) {
        app.mouse.reset_click();
        true
    } else {
        false
    }
}
pub fn poll(app: &mut App, now: Instant) -> bool {
    let allowed = matches!(
        crate::focus::current(app),
        crate::focus::Focus::Manager | crate::focus::Focus::Editor | crate::focus::Focus::Viewer
    ) && !app.quit
        && !app.exit_pending
        && !app.file_tools.busy()
        && app.panels.iter().all(|p| !p.loading)
        && app.editor.as_ref().is_none_or(|e| !e.busy())
        && app.viewer.as_ref().is_none_or(|v| v.contents.is_some());
    app.screensaver.tick(now, app.config.saver_minutes, allowed)
}
pub fn open(app: &mut App) {
    crate::menu::close(app);
    app.screensaver.setup = Some(Setup {
        delay: app.config.saver_minutes,
        cursor: DELAYS
            .iter()
            .position(|v| *v == app.config.saver_minutes)
            .unwrap_or(4),
    });
}
pub fn command(app: &mut App, index: usize) {
    if index < DELAYS.len() {
        if let Some(setup) = &mut app.screensaver.setup {
            setup.delay = DELAYS[index];
            setup.cursor = index;
        }
    } else if index == PREVIEW {
        app.screensaver.start(Instant::now());
    } else {
        if index == ACCEPT {
            if let Some(setup) = &app.screensaver.setup {
                app.config.saver_minutes = setup.delay;
            }
        }
        app.screensaver.setup = None;
    }
}
pub fn key(app: &mut App, code: KeyCode) {
    let Some(setup) = &mut app.screensaver.setup else {
        return;
    };
    match code {
        KeyCode::Esc => command(app, CANCEL),
        KeyCode::Tab | KeyCode::Down | KeyCode::Right => {
            setup.cursor = (setup.cursor + 1) % CONTROL_COUNT
        }
        KeyCode::BackTab | KeyCode::Up | KeyCode::Left => {
            setup.cursor = (setup.cursor + CONTROL_COUNT - 1) % CONTROL_COUNT
        }
        KeyCode::Enter | KeyCode::Char(' ') => {
            let i = setup.cursor;
            command(app, i);
        }
        KeyCode::Char('p' | 'P') => command(app, PREVIEW),
        _ => {}
    }
}
