//! Built-in Tetris controller. No access to App or terminal ownership.
use super::{Context, Definition, Effect, Event, Presentation, ToolPlugin, View};
use crate::tetris::{Game, GameAction, Screen};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind};
use std::{
    any::Any,
    time::{Duration, Instant},
};
#[derive(Default)]
pub(crate) struct TetrisPlugin {
    pub game: Option<Game>,
    pub scores: crate::tetris::scores::Store,
}
impl ToolPlugin for TetrisPlugin {
    fn definition(&self) -> Definition {
        Definition::builtin(super::TETRIS, "Navigator's game", Presentation::Document)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn can_open(&self, c: &super::OpenContext) -> bool {
        !c.job && !c.blocking && !c.viewer
    }
    fn open(&mut self, ctx: &Context) {
        let path = ctx
            .storage
            .as_ref()
            .map(|p| p.with_file_name("tetris.conf"));
        let encoding = if ctx.language == crate::localization::Language::Russian {
            crate::tetris::dos_scores::Encoding::Cp866
        } else {
            crate::tetris::dos_scores::Encoding::Cp437
        };
        let error = self.scores.load_with_encoding(path, encoding).err();
        let mut game = Game::default();
        game.records = self.scores.board.records.clone();
        game.name = self.scores.board.last_name.clone();
        game.name_cursor = game.name.len();
        game.notice = error.map_or_else(
            || {
                if self.scores.has_pending() {
                    "Scores not saved. Retry save.".into()
                } else {
                    String::new()
                }
            },
            |e| format!("Cannot load scores: {e}"),
        );
        game.resume_clock(ctx.now);
        self.game = Some(game);
    }
    fn uses_controls(&self, key: KeyEvent) -> bool {
        key.code != KeyCode::Char(' ')
    }
    fn event(&mut self, event: Event<'_>, ctx: &Context) -> Vec<Effect> {
        let mut effects = Vec::new();
        match event {
            Event::Key(k) => key(self, k, ctx, &mut effects),
            Event::Paste(s) => paste(self, s),
            Event::Action(a) => {
                if let Some(a) = GameAction::from_id(a.0) {
                    action(self, a, ctx, &mut effects);
                }
            }
            Event::Mouse(e, Some(a)) if e.kind == MouseEventKind::Down(MouseButton::Left) => {
                if let Some(a) = GameAction::from_id(a.0) {
                    action(self, a, ctx, &mut effects);
                }
            }
            Event::Focus(f) => {
                if let Some(g) = self.game.as_mut() {
                    match g.screen {
                        Screen::Play => {
                            g.play_focus = f;
                            g.resume_clock(ctx.now);
                        }
                        Screen::Setup { .. } => g.setup_focus = f,
                        _ => g.page_focus = f,
                    }
                }
            }
            Event::Activity(active) => {
                if let Some(g) = self.game.as_mut() {
                    g.set_obscured(!active, ctx.now);
                }
            }
            Event::Tick => {
                if let Some(g) = self.game.as_mut() {
                    g.tick(ctx.now);
                }
            }
            _ => {}
        }
        effects
    }
    fn controls(&self, ctx: &Context) -> crate::turbo::dialog_spec::DialogSpec<'_> {
        crate::turbo::tetris::spec(
            self.game.as_ref().expect("open game"),
            self.scores.has_pending(),
            ctx,
        )
    }
    fn view(&self, ctx: &Context) -> View<'_> {
        View::Surface(
            crate::turbo::tetris::prepare(self.game.as_ref().expect("open game"), ctx),
            self.controls(ctx),
        )
    }
    fn next_wake(&self, now: Instant) -> Option<Duration> {
        let g = self.game.as_ref()?;
        if !g.running() {
            return None;
        }
        Some(g.wait_duration(now))
    }
    fn close(&mut self) {
        self.game = None;
    }
}
fn action(plugin: &mut TetrisPlugin, action: GameAction, ctx: &Context, effects: &mut Vec<Effect>) {
    if action == GameAction::Close {
        effects.push(Effect::Close);
        return;
    }
    if action == GameAction::RetrySave {
        if let Some(g) = plugin.game.as_mut() {
            g.page_focus = 1;
        }
        persist(plugin);
        return;
    }
    if matches!(action, GameAction::SubmitName | GameAction::SkipName) {
        submit(plugin, action == GameAction::SkipName);
        return;
    }
    let Some(game) = plugin.game.as_mut() else {
        return;
    };
    let now = ctx.now;
    if game.screen == Screen::Name {
        match action {
            GameAction::NameField => game.page_focus = 0,
            GameAction::Back => {
                game.screen = Screen::GameOver;
                game.page_focus = 0;
            }
            _ => {}
        }
        return;
    }
    if game.screen == Screen::GameOver && !matches!(action, GameAction::Continue | GameAction::Back)
    {
        return;
    }
    let previous_screen = game.screen;
    match action {
        GameAction::Continue => {
            if qualifies(game) {
                game.screen = Screen::Name;
                game.replace_name = true;
                game.name_cursor = game.name.len();
            } else {
                game.screen = Screen::Scores;
            }
        }
        GameAction::Help => game.screen = Screen::Help,
        GameAction::Scores => game.screen = Screen::Scores,
        GameAction::Setup => {
            game.setup_focus = 0;
            game.screen = Screen::Setup {
                level: game.start_level,
                preview: game.preview,
            }
        }
        GameAction::Back => game.screen = Screen::Play,
        GameAction::BoardFocus => game.play_focus = 0,
        GameAction::New => game.restart(now),
        GameAction::Pause if game.screen == Screen::Play && !game.over => {
            game.paused = !game.paused
        }
        GameAction::LevelUp => {
            if let Screen::Setup { level, .. } = &mut game.screen {
                *level = (*level + 1).min(10);
            } else if game.screen == Screen::Play {
                game.level = (game.level + 1).min(10);
            }
        }
        GameAction::LevelDown => {
            if let Screen::Setup { level, .. } = &mut game.screen {
                *level = level.saturating_sub(1).max(1);
            }
        }
        GameAction::SetLevel(value) => {
            if let Screen::Setup { level, .. } = &mut game.screen {
                if (1..=10).contains(&value) {
                    *level = value;
                    game.setup_focus = 0;
                }
            }
        }
        GameAction::Preview => {
            if let Screen::Setup { preview, .. } = &mut game.screen {
                *preview = !*preview;
                game.setup_focus = 1;
            } else if game.screen == Screen::Play {
                game.preview = !game.preview;
            }
        }
        GameAction::Apply => {
            if let Screen::Setup { level, preview } = game.screen {
                game.start_level = level;
                game.preview = preview;
                game.restart(now);
            }
        }
        _ => {}
    }
    if game.screen != previous_screen {
        game.page_focus = 0;
    }
    game.resume_clock(now);
}

fn key(plugin: &mut TetrisPlugin, key: KeyEvent, ctx: &Context, effects: &mut Vec<Effect>) {
    let screen = plugin.game.as_ref().map(|game| game.screen);
    if screen == Some(Screen::Play)
        && plugin.game.as_ref().is_some_and(|g| g.play_focus > 0)
        && matches!(
            key.code,
            KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down | KeyCode::Char(' ')
        )
    {
        return;
    }
    if screen == Some(Screen::Name) {
        if plugin.game.as_ref().is_some_and(|g| g.page_focus == 0) {
            edit_name(plugin, key);
        }
        return;
    }
    if matches!(screen, Some(Screen::Setup { .. })) {
        let focus = plugin.game.as_ref().map_or(0, |g| g.setup_focus);
        match key.code {
            KeyCode::Right | KeyCode::Char('+') => {
                action(plugin, GameAction::LevelUp, ctx, effects)
            }
            KeyCode::Left | KeyCode::Char('-') => {
                action(plugin, GameAction::LevelDown, ctx, effects)
            }
            KeyCode::Up if focus == 0 => action(plugin, GameAction::LevelDown, ctx, effects),
            KeyCode::Down if focus == 0 => action(plugin, GameAction::LevelUp, ctx, effects),
            KeyCode::Home => action(plugin, GameAction::SetLevel(1), ctx, effects),
            KeyCode::End => action(plugin, GameAction::SetLevel(10), ctx, effects),
            KeyCode::Char(c @ '0'..='9') => action(
                plugin,
                GameAction::SetLevel(if c == '0' { 10 } else { c as u8 - b'0' }),
                ctx,
                effects,
            ),
            KeyCode::Char('*') => action(plugin, GameAction::Preview, ctx, effects),
            KeyCode::Char(' ') => action(
                plugin,
                match focus {
                    2 => GameAction::Apply,
                    3 => GameAction::Back,
                    _ => GameAction::Preview,
                },
                ctx,
                effects,
            ),
            _ => {}
        }
        return;
    }
    if screen != Some(Screen::Play) {
        return;
    }
    let command = if key.modifiers.contains(KeyModifiers::ALT) {
        match key.code {
            KeyCode::Char(c) => crate::tetris::BUTTONS
                .iter()
                .find(|(label, _)| ctx.language.matches(label, c))
                .map(|(_, action)| *action),
            _ => None,
        }
    } else {
        match key.code {
            KeyCode::F(1) => Some(GameAction::Help),
            KeyCode::F(2) => Some(GameAction::New),
            KeyCode::F(3) => Some(GameAction::Pause),
            KeyCode::F(4) => Some(GameAction::Scores),
            KeyCode::F(5) => Some(GameAction::Setup),
            KeyCode::Char('+') => Some(GameAction::LevelUp),
            KeyCode::Char('*') => Some(GameAction::Preview),
            _ => None,
        }
    };
    if let Some(command) = command {
        action(plugin, command, ctx, effects);
        return;
    }
    if key
        .modifiers
        .intersects(KeyModifiers::ALT | KeyModifiers::CONTROL)
    {
        return;
    }
    if let Some(game) = plugin.game.as_mut() {
        match key.code {
            KeyCode::Left | KeyCode::Home => {
                game.translate(-1, 0);
            }
            KeyCode::Right | KeyCode::PageUp => {
                game.translate(1, 0);
            }
            KeyCode::Up => game.rotate(),
            KeyCode::Down | KeyCode::Char(' ') => game.drop(ctx.now),
            _ => {}
        }
    }
}

fn qualifies(game: &Game) -> bool {
    game.score > 0
        && (game.records.len() < 10 || game.records.last().is_some_and(|r| game.score > r.score))
}
fn persist(plugin: &mut TetrisPlugin) {
    let result = plugin.scores.save();
    if let Some(g) = plugin.game.as_mut() {
        g.records = plugin.scores.board.records.clone();
        if g.submitted {
            g.highlighted = g
                .records
                .iter()
                .rposition(|r| r.score == g.score && r.name == g.name && r.lines == Some(g.lines));
        }
        if result.is_ok() {
            g.notice = "Scores saved".into();
        } else if let Err(e) = result {
            g.notice = format!("Scores not saved: {e}. Retry save.");
        }
    }
}
fn submit(plugin: &mut TetrisPlugin, anonymous: bool) {
    let Some(g) = plugin.game.as_mut() else {
        return;
    };
    if g.screen != Screen::Name || g.submitted {
        return;
    }
    let name = if anonymous || g.name.trim().is_empty() {
        crate::tetris::scores::ANONYMOUS.to_owned()
    } else {
        g.name.trim().to_owned()
    };
    if !crate::tetris::scores::validate_name(&name) {
        g.notice = "Name must fit 30 characters / 240 bytes, without controls".into();
        return;
    }
    let record = crate::tetris::Record {
        name: name.clone(),
        score: g.score,
        lines: Some(g.lines),
        start_level: g.start_level,
        level: g.level,
    };
    if let Err(e) = plugin.scores.add(record) {
        g.notice = e.to_string();
        return;
    }
    g.name = name.clone();
    g.submitted = true;
    g.highlighted = Some(
        plugin
            .scores
            .board
            .records
            .iter()
            .rposition(|r| r.score == g.score && r.name == name)
            .unwrap_or(0),
    );
    g.screen = Screen::Scores;
    g.page_focus = 0;
    persist(plugin);
}
pub fn paste(plugin: &mut TetrisPlugin, text: &str) {
    if text.is_empty() {
        return;
    }
    let Some(g) = plugin.game.as_mut() else {
        return;
    };
    if g.screen != Screen::Name || g.page_focus != 0 {
        return;
    }
    let mut value = if g.replace_name {
        String::new()
    } else {
        g.name.clone()
    };
    let cursor = if g.replace_name { 0 } else { g.name_cursor };
    value.insert_str(cursor, text);
    if !crate::tetris::scores::validate_name(&value) {
        g.notice = "Name must fit 30 characters / 240 bytes, without controls".into();
        return;
    }
    g.name = value;
    g.name_cursor = cursor + text.len();
    use unicode_segmentation::UnicodeSegmentation;
    g.name_cursor = g
        .name
        .grapheme_indices(true)
        .map(|(i, _)| i)
        .find(|i| *i >= g.name_cursor)
        .unwrap_or(g.name.len());
    g.replace_name = false;
    g.name_cursor = g
        .name
        .grapheme_indices(true)
        .map(|(i, _)| i)
        .find(|i| *i >= g.name_cursor)
        .unwrap_or(g.name.len());
    if g.notice.starts_with("Name must fit") {
        g.notice.clear();
    }
}
fn edit_name(plugin: &mut TetrisPlugin, key: KeyEvent) {
    use unicode_segmentation::UnicodeSegmentation;
    if key
        .modifiers
        .intersects(KeyModifiers::ALT | KeyModifiers::CONTROL)
    {
        return;
    }
    if let KeyCode::Char(c) = key.code {
        paste(plugin, &c.to_string());
        return;
    }
    if !matches!(
        key.code,
        KeyCode::Left
            | KeyCode::Right
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::Backspace
            | KeyCode::Delete
    ) {
        return;
    }
    let Some(g) = plugin.game.as_mut() else {
        return;
    };
    let previous = g.name[..g.name_cursor]
        .grapheme_indices(true)
        .next_back()
        .map_or(0, |(i, _)| i);
    let next = g.name[g.name_cursor..]
        .grapheme_indices(true)
        .nth(1)
        .map_or(g.name.len(), |(i, _)| g.name_cursor + i);
    match key.code {
        KeyCode::Left => g.name_cursor = previous,
        KeyCode::Right => g.name_cursor = next,
        KeyCode::Home => g.name_cursor = 0,
        KeyCode::End => g.name_cursor = g.name.len(),
        KeyCode::Backspace | KeyCode::Delete if g.replace_name => {
            g.name.clear();
            g.name_cursor = 0;
        }
        KeyCode::Backspace => {
            g.name.replace_range(previous..g.name_cursor, "");
            g.name_cursor = previous;
        }
        KeyCode::Delete => {
            g.name.replace_range(g.name_cursor..next, "");
        }
        _ => {}
    }
    g.replace_name = false;
    g.name_cursor = g
        .name
        .grapheme_indices(true)
        .map(|(i, _)| i)
        .find(|i| *i >= g.name_cursor)
        .unwrap_or(g.name.len());
    if g.notice.starts_with("Name must fit") {
        g.notice.clear();
    }
}
