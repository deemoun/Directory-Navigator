//! Native versions of DN 1.51 IDLERS.PAS effects and SETUPS.PAS selection.
//! Based on Dos Navigator by RIT Research Labs, Copyright (C) 1991–99.
use crate::app::App;
use crossterm::event::{Event, KeyCode, KeyEventKind, MouseEventKind};
use std::time::{Duration, Instant};

pub const DEFAULT_DELAY: u8 = 10;
pub const DELAYS: [u8; 7] = [0, 1, 2, 5, DEFAULT_DELAY, 30, 60];
pub const PREVIEW: usize = DELAYS.len();
pub const ACCEPT: usize = PREVIEW + 1;
pub const CANCEL: usize = PREVIEW + 2;
pub const GLYPHS: [char; 6] = [' ', '·', '∙', '•', '♦', '☼'];
pub const SELECTED: usize = 10;
pub const AVAILABLE: usize = 11;
pub const ADD: usize = 12;
pub const REMOVE: usize = 13;
pub const HELP: usize = 14;
pub const SELECTED_ROW: usize = 32;
pub const AVAILABLE_ROW: usize = 64;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    StarFlight,
    FlashLight,
    Clock,
    Blackness,
}
impl Kind {
    pub const ALL: [Self; 4] = [
        Self::StarFlight,
        Self::FlashLight,
        Self::Clock,
        Self::Blackness,
    ];
    pub fn key(self) -> &'static str {
        match self {
            Self::StarFlight => "star_flight",
            Self::FlashLight => "flash_light",
            Self::Clock => "clock",
            Self::Blackness => "blackness",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::StarFlight => "Star flight",
            Self::FlashLight => "Flash-light",
            Self::Clock => "Clock",
            Self::Blackness => "Blackness",
        }
    }
    pub fn parse(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.key() == key)
    }
}
pub struct Setup {
    pub delay: u8,
    pub cursor: usize,
    pub selected: Vec<Kind>,
    pub selected_row: usize,
    pub available_row: usize,
    pub preview: Kind,
}
impl Default for Setup {
    fn default() -> Self {
        Self {
            delay: DEFAULT_DELAY,
            cursor: SELECTED,
            selected: vec![Kind::StarFlight],
            selected_row: 0,
            available_row: 0,
            preview: Kind::StarFlight,
        }
    }
}
#[derive(Default)]
struct Motion {
    x: i32,
    y: i32,
    dx: i32,
    dy: i32,
    remaining: u8,
}
#[derive(Default)]
struct Clock {
    motion: Motion,
    text: String,
    colon: bool,
}

pub struct State {
    pub modals: crate::modals::ModalStack,
    pub active: bool,
    last_input: Instant,
    next_frame: Instant,
    stars: Vec<Star>,
    size: (u16, u16),
    seed: u64,
    kind: Kind,
    redraw: bool,
    flashlight: Motion,
    clock: Clock,
    started: Instant,
    snapshot: Option<crate::turbo::scene::Scene>,
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
            modals: Default::default(),
            active: false,
            last_input: now,
            next_frame: now,
            stars: Vec::new(),
            size: (0, 0),
            kind: Kind::StarFlight,
            redraw: true,
            flashlight: Default::default(),
            clock: Default::default(),
            started: now,
            snapshot: None,
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
        if consumed {
            self.snapshot = None;
        }
        consumed
    }
    pub fn kind(&self) -> Kind {
        self.kind
    }
    pub fn start(&mut self, now: Instant) {
        self.start_kind(now, Kind::StarFlight);
    }
    pub fn start_random(&mut self, now: Instant, selected: &[Kind]) -> bool {
        if selected.is_empty() {
            return false;
        }
        let index = self.random(selected.len() as u64) as usize;
        self.start_kind(now, selected[index]);
        true
    }
    pub fn start_kind(&mut self, now: Instant, kind: Kind) {
        self.kind = kind;
        self.redraw = true;
        self.started = now;
        self.snapshot = None;
        self.flashlight = Default::default();
        self.clock = Default::default();
        self.clock.text = chrono::Local::now().format("%H:%M").to_string();
        self.clock.colon = true;
        self.active = true;
        self.stars.clear();
        self.size = (0, 0);
        self.next_frame = now;
    }
    pub fn tick(&mut self, now: Instant, minutes: u8, allowed: bool) -> bool {
        self.tick_selected(now, minutes, allowed, &[Kind::StarFlight])
    }
    pub fn tick_selected(
        &mut self,
        now: Instant,
        minutes: u8,
        allowed: bool,
        selected: &[Kind],
    ) -> bool {
        if !self.active {
            if !allowed {
                self.last_input = now;
                return false;
            }
            if selected.is_empty()
                || minutes == 0
                || now.saturating_duration_since(self.last_input)
                    < Duration::from_secs(u64::from(minutes) * 60)
            {
                return false;
            }
            self.start_random(now, selected);
        }
        if self.kind == Kind::Blackness {
            return std::mem::take(&mut self.redraw);
        }
        if now < self.next_frame {
            return false;
        }
        let interval = match self.kind {
            Kind::FlashLight => 150,
            Kind::Clock => 100,
            Kind::Blackness => 1000,
            Kind::StarFlight => 50,
        };
        self.next_frame = now + Duration::from_millis(interval);
        if matches!(self.kind, Kind::FlashLight | Kind::Clock) {
            if self.size.0 > 0 && self.size.1 > 0 {
                self.move_effect();
            }
            if self.kind == Kind::Clock {
                self.update_clock(now, chrono::Local::now().time());
            }
            return true;
        }
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
    fn update_clock(&mut self, now: Instant, local: chrono::NaiveTime) {
        self.clock.text = local.format("%H:%M").to_string();
        self.clock.colon =
            (now.saturating_duration_since(self.started).as_millis() / 500).is_multiple_of(2);
    }
    fn move_effect(&mut self) {
        let clock = self.kind == Kind::Clock;
        let mut motion = if clock {
            std::mem::take(&mut self.clock.motion)
        } else {
            std::mem::take(&mut self.flashlight)
        };
        let max_x = i32::from(self.size.0.saturating_sub(if clock { 5 } else { 1 }));
        let max_y = i32::from(self.size.1.saturating_sub(1));
        if motion.remaining == 0 {
            let speed = if clock { 1 } else { 3 };
            motion.dx = self.random((speed * 2 + 1) as u64) as i32 - speed;
            motion.dy = self.random((speed * 2 + 1) as u64) as i32 - speed;
            motion.remaining = 3 + self.random(if clock { 10 } else { 20 }) as u8;
        }
        let x = motion.x + motion.dx;
        let y = motion.y + motion.dy;
        motion.x = x.clamp(0, max_x);
        motion.y = y.clamp(0, max_y);
        motion.remaining -= 1;
        if x != motion.x || y != motion.y {
            motion.remaining = 0;
        }
        if clock {
            self.clock.motion = motion;
        } else {
            self.flashlight = motion;
        }
    }
    pub(crate) fn needs_snapshot(&self) -> bool {
        self.active && self.kind == Kind::FlashLight && self.snapshot.is_none()
    }
    pub(crate) fn capture(&mut self, mut scene: crate::turbo::scene::Scene) {
        scene.regions.clear();
        scene.cursor = None;
        self.snapshot = Some(scene);
    }
    pub(crate) fn effect_scene(
        &self,
        profile: crate::palette::PaletteProfile,
    ) -> crate::turbo::scene::Scene {
        let (width, height) = self.size;
        let mut scene = crate::turbo::scene::Scene {
            width,
            height,
            profile,
            cells: Default::default(),
            regions: Vec::new(),
            cursor: None,
        };
        scene.fill(0, 0, width, height, 0x07);
        if self.kind == Kind::FlashLight {
            if let Some(snapshot) = &self.snapshot {
                let (cx, cy) = (self.flashlight.x, self.flashlight.y);
                let stage = 1
                    + (6 * (cx - i32::from(width) / 2).abs() / i32::from(width.max(1)))
                        .max(6 * (cy - i32::from(height) / 2).abs() / i32::from(height.max(1)));
                // IDLERS.PAS RR[1] is the largest light; the later stages
                // shrink it as it approaches the screen edges.
                let radius = (5 - stage).clamp(1, 4);
                for y in (cy - radius).max(0)..=(cy + radius).min(i32::from(height) - 1) {
                    for x in (cx - radius * 2).max(0)..=(cx + radius * 2).min(i32::from(width) - 1)
                    {
                        let dx = f64::from(x - cx) / 2.0;
                        if dx * dx + f64::from((y - cy) * (y - cy)) <= f64::from(radius * radius) {
                            if let Some(glyph) = snapshot.cells.get(&(x as u16, y as u16)) {
                                scene.cells.insert((x as u16, y as u16), glyph.clone());
                            }
                        }
                    }
                }
            }
        } else if self.kind == Kind::Clock {
            let text = if self.clock.colon {
                self.clock.text.clone()
            } else {
                self.clock.text.replace(':', " ")
            };
            scene.text(
                self.clock.motion.x as u16,
                self.clock.motion.y as u16,
                width.saturating_sub(self.clock.motion.x as u16),
                &text,
                0x07,
            );
        } else if self.kind == Kind::StarFlight {
            for (x, y, glyph, attribute) in self.cells() {
                scene.text(x, y, 1, &glyph.to_string(), attribute);
            }
        }
        scene.normalize_wide();
        scene
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
        self.redraw = true;
        self.flashlight.x = i32::from(w / 2);
        self.flashlight.y = i32::from(h / 2);
        self.clock.motion.x = i32::from(w.saturating_sub(5) / 2);
        self.clock.motion.y = i32::from(h / 2);
        self.stars.clear();
        if w == 0 || h == 0 {
            return;
        }
        // Original storage held 200 stars; bound modern wide terminals too.
        if self.kind != Kind::StarFlight {
            return;
        }
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
        app.shortcuts.clear();
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
        && !app.associations.busy()
        && app.associations.pending.is_none()
        && app.panels.iter().all(|p| !p.loading)
        && app.desktop.active_editor().is_none_or(|e| !e.busy())
        && app
            .desktop
            .active_viewer()
            .is_none_or(|v| v.contents.is_some());
    app.screensaver
        .tick_selected(now, app.config.saver_minutes, allowed, &app.config.savers)
}

pub const HELP_TEXT: &str = "Choose one or more screen savers in Selected savers. Add copies the highlighted Available saver without duplicates; Remove deletes the highlighted Selected saver. DN randomly chooses from Selected savers each time screen rest starts. An empty selection disables automatic screen rest.\nStar flight: accelerating stars. Flash-light: a moving light reveals a frozen view of DN. Clock: moving local HH:MM time. Blackness: a black screen.\nPreview shows the last highlighted saver from either list without applying changes. Any key, paste or mouse input ends the preview and is consumed.\nTab / Shift+Tab moves between controls; arrows choose list entries; Insert / Delete adds / removes; P previews; F1 opens help. OK applies the draft; Cancel discards it. Save desktop or normal exit with Save on exit persists accepted settings. External DOS .SS programs are not supported.";

pub fn start(app: &mut App) {
    if !app
        .screensaver
        .start_random(Instant::now(), &app.config.savers)
    {
        app.push_dialog(crate::dialog::Dialog {
            title: " Screen savers ",
            message: app.config.language.message(
                "Select at least one screen saver in Options > Configuration > Screen savers.",
            ),
        });
    }
}
pub fn open(app: &mut App) {
    crate::menu::close(app);
    app.screensaver.modals.set_screensaver(Some(Setup {
        delay: app.config.saver_minutes,
        selected: app.config.savers.clone(),
        preview: app.config.savers.first().copied().unwrap_or_default(),
        ..Default::default()
    }));
}
/// Remember a list highlight independently of the button currently in focus.
pub(crate) fn remember_preview(app: &mut App) {
    if let Some(s) = app.screensaver.modals.screensaver_mut() {
        let kind = match s.cursor {
            SELECTED => s.selected.get(s.selected_row).copied(),
            AVAILABLE => Kind::ALL.get(s.available_row).copied(),
            _ => None,
        };
        if let Some(kind) = kind {
            s.preview = kind;
        }
    }
}
pub fn command(app: &mut App, index: usize) {
    if app.screensaver.modals.screensaver().is_none() {
        return;
    }
    if index == HELP {
        let owner = app
            .modal_top()
            .map_or(crate::modals::ModalOwner::Workspace, |m| m.owner);
        app.push_owned_dialog(
            owner,
            crate::dialog::Dialog {
                title: " Screen savers help ",
                message: HELP_TEXT.into(),
            },
        );
        return;
    }
    if index == PREVIEW {
        let kind = app.screensaver.modals.screensaver().unwrap().preview;
        app.screensaver.start_kind(Instant::now(), kind);
        return;
    }
    if index == ACCEPT || index == CANCEL {
        if index == ACCEPT {
            let setup = app.screensaver.modals.screensaver().unwrap();
            app.config.saver_minutes = setup.delay;
            app.config.savers = setup.selected.clone();
        }
        app.screensaver.modals.set_screensaver(None);
        return;
    }
    let s = app.screensaver.modals.screensaver_mut().unwrap();
    match index {
        i if i < DELAYS.len() => {
            s.delay = DELAYS[i];
            s.cursor = i;
        }
        SELECTED | AVAILABLE => s.cursor = index,
        i if (SELECTED_ROW..SELECTED_ROW + Kind::ALL.len()).contains(&i) => {
            let row = i - SELECTED_ROW;
            if row < s.selected.len() {
                s.selected_row = row;
                s.cursor = SELECTED;
            }
        }
        i if (AVAILABLE_ROW..AVAILABLE_ROW + Kind::ALL.len()).contains(&i) => {
            s.available_row = i - AVAILABLE_ROW;
            s.cursor = AVAILABLE;
        }
        ADD => {
            let kind = Kind::ALL[s.available_row];
            if !s.selected.contains(&kind) {
                s.selected.push(kind);
                s.selected_row = s.selected.len() - 1;
            }
        }
        REMOVE if s.selected_row < s.selected.len() => {
            s.selected.remove(s.selected_row);
            s.selected_row = s.selected_row.min(s.selected.len().saturating_sub(1));
            if let Some(kind) = s.selected.get(s.selected_row) {
                s.preview = *kind;
            }
        }
        _ => {}
    }
    remember_preview(app);
}
pub fn key(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::F(1) => {
            command(app, HELP);
            return;
        }
        KeyCode::Char('p' | 'P') => {
            command(app, PREVIEW);
            return;
        }
        KeyCode::Insert | KeyCode::Char('a' | 'A') => {
            command(app, ADD);
            return;
        }
        KeyCode::Delete | KeyCode::Char('r' | 'R') => {
            command(app, REMOVE);
            return;
        }
        _ => {}
    }
    if let Some(s) = app.screensaver.modals.screensaver_mut() {
        let (row, count) = match s.cursor {
            SELECTED => (&mut s.selected_row, s.selected.len()),
            AVAILABLE => (&mut s.available_row, Kind::ALL.len()),
            _ => (&mut s.selected_row, 0),
        };
        if count > 0 {
            if let Some(next) = crate::turbo::widgets::list_key(*row, count, 4, code) {
                *row = next;
                remember_preview(app);
                return;
            }
        }
    }
    crate::dialog_controls::key(
        app,
        crossterm::event::KeyEvent::new(code, crossterm::event::KeyModifiers::NONE),
        crate::focus::Focus::SaverSetup,
    );
    remember_preview(app);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{palette::PaletteProfile, turbo::scene::Scene};
    #[test]
    fn seeded_choice_is_restricted_to_selected_savers_and_all_are_reachable() {
        let mut s = State {
            seed: 1,
            ..Default::default()
        };
        let now = Instant::now();
        assert!(!s.start_random(now, &[]));
        assert!(!s.active);
        let chosen = [Kind::FlashLight, Kind::Blackness];
        let mut seen = [false; 2];
        for _ in 0..40 {
            assert!(s.start_random(now, &chosen));
            let i = chosen.iter().position(|k| *k == s.kind()).unwrap();
            seen[i] = true;
        }
        assert_eq!(seen, [true, true]);
    }
    #[test]
    fn flashlight_snapshot_is_owned_frozen_clipped_and_has_no_input_or_cursor() {
        let mut s = State::default();
        let now = Instant::now();
        s.start_kind(now, Kind::FlashLight);
        s.resize(80, 25);
        let mut image = Scene {
            width: 80,
            height: 25,
            profile: PaletteProfile::ClassicDn,
            cells: Default::default(),
            regions: vec![(
                crate::presentation::layout::Rect::new(0, 0, 80, 25),
                crate::mouse::Target::Panel(0),
            )],
            cursor: Some((40, 12)),
        };
        image.fill(0, 0, 80, 25, 0x1e);
        image.text(40, 12, 4, "界", 0x4f);
        s.capture(image.clone());
        image.fill(0, 0, 80, 25, 0x2a);
        let snapshot = s.snapshot.as_ref().unwrap();
        assert!(snapshot.regions.is_empty() && snapshot.cursor.is_none());
        assert_eq!(snapshot.cells[&(40, 12)].text, "界");
        assert_eq!(snapshot.cells[&(40, 12)].attribute, 0x4f);
        let frame = s.effect_scene(PaletteProfile::ClassicDn);
        assert_eq!(frame.cells[&(40, 12)].text, "界");
        assert_eq!(frame.cells[&(40, 12)].attribute, 0x4f);
        assert!(frame.cells[&(41, 12)].text.is_empty());
        assert_eq!(frame.cells[&(0, 0)].attribute, 0x07);
        let center_light = frame.cells.values().filter(|g| g.attribute != 0x07).count();
        s.flashlight.x = 79;
        s.flashlight.y = 24;
        let edge = s.effect_scene(PaletteProfile::ClassicDn);
        assert!(edge.cells.values().filter(|g| g.attribute != 0x07).count() < center_light);
        // Resize only clips/pads the original snapshot. No recapture or live view.
        s.resize(160, 50);
        s.flashlight.x = 140;
        s.flashlight.y = 40;
        let padded = s.effect_scene(PaletteProfile::ClassicDn);
        assert!(padded.cells.values().all(|g| g.attribute == 0x07));
        assert_eq!(s.snapshot.as_ref().unwrap().cells[&(40, 12)].text, "界");
        for (w, h) in [(1, 1), (0, 0), (80, 25)] {
            s.resize(w, h);
            for i in 0..80 {
                s.tick_selected(now + Duration::from_millis(i * 150), 0, false, &[]);
                let frame = s.effect_scene(PaletteProfile::ClassicDn);
                assert!(frame.cells.keys().all(|(x, y)| *x < w && *y < h));
            }
        }
    }
    #[test]
    fn clock_local_time_rollover_blink_motion_and_small_bounds() {
        let now = Instant::now();
        let mut s = State::default();
        s.start_kind(now, Kind::Clock);
        s.resize(80, 25);
        s.update_clock(now, chrono::NaiveTime::from_hms_opt(23, 59, 59).unwrap());
        assert_eq!(s.clock.text, "23:59");
        assert!(s.clock.colon);
        s.update_clock(
            now + Duration::from_millis(500),
            chrono::NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
        );
        assert_eq!(s.clock.text, "00:00");
        assert!(!s.clock.colon);
        s.update_clock(
            now + Duration::from_secs(1),
            chrono::NaiveTime::from_hms_opt(0, 0, 1).unwrap(),
        );
        assert!(s.clock.colon);
        for (w, h) in [(80, 25), (160, 50), (1, 1), (0, 0)] {
            s.resize(w, h);
            for _ in 0..100 {
                if w > 0 && h > 0 {
                    s.move_effect();
                }
                let frame = s.effect_scene(PaletteProfile::ClassicDn);
                assert!(frame.cells.keys().all(|(x, y)| *x < w && *y < h));
                assert!(
                    s.clock.motion.x >= 0 && s.clock.motion.x <= i32::from(w.saturating_sub(5))
                );
                assert!(
                    s.clock.motion.y >= 0 && s.clock.motion.y <= i32::from(h.saturating_sub(1))
                );
            }
        }
    }
    #[test]
    fn blackness_has_no_animation_and_empty_selection_never_starts() {
        let now = Instant::now();
        let mut s = State::default();
        s.activity(now);
        assert!(!s.tick_selected(now + Duration::from_secs(3600), 1, true, &[]));
        s.start_kind(now, Kind::Blackness);
        assert!(s.tick_selected(now, 0, false, &[]));
        assert!(!s.tick_selected(now + Duration::from_secs(3600), 0, false, &[]));
        s.resize(80, 25);
        assert!(s.tick_selected(now + Duration::from_secs(3601), 0, false, &[]));
        assert!(!s.tick_selected(now + Duration::from_secs(3602), 0, false, &[]));
    }
}
