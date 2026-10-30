//! A view over the command prompt's session history; accepting never executes.
use super::CommandLine;
use crate::presentation::layout::Rect;
use crate::{app::App, mouse::Target};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use std::{io, time::Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Field,
    Row(usize),
    Use,
    Cancel,
}
pub struct HistoryDialog {
    pub(crate) source: Vec<String>,
    pub query: String,
    pub input_cursor: usize,
    pub matches: Vec<usize>,
    pub cursor: usize,
    pub offset: usize,
    pub rows: usize,
    pub focus: usize,
    last_click: Option<(usize, Instant)>,
}
impl HistoryDialog {
    fn new(history: &[String]) -> Self {
        Self {
            source: history.to_vec(),
            query: String::new(),
            input_cursor: 0,
            matches: (0..history.len()).collect(),
            cursor: 0,
            offset: 0,
            rows: 1,
            focus: 0,
            last_click: None,
        }
    }
    fn refilter(&mut self, history: &[String]) {
        self.source = history.to_vec();
        let query = self.query.to_lowercase();
        self.matches = history
            .iter()
            .enumerate()
            .filter(|(_, s)| s.to_lowercase().contains(&query))
            .map(|(i, _)| i)
            .collect();
        self.cursor = 0;
        self.offset = 0;
        self.last_click = None;
    }
    pub(crate) fn refresh(&mut self, history: &[String]) -> bool {
        if self.source == history {
            return false;
        }
        let selected = self
            .matches
            .get(self.cursor)
            .and_then(|i| self.source.get(*i))
            .cloned();
        let offset = self.offset;
        self.refilter(history);
        self.cursor = selected
            .and_then(|s| self.matches.iter().position(|i| self.source[*i] == s))
            .unwrap_or(0);
        self.offset = offset;
        self.keep_visible();
        true
    }
    pub fn keep_visible(&mut self) {
        self.rows = self.rows.max(1);
        crate::list_state::ListViewport::fit(
            &mut self.cursor,
            &mut self.offset,
            self.rows,
            Some(self.matches.len().saturating_sub(1)),
            Some(self.matches.len().saturating_sub(self.rows)),
        );
    }
    fn step(&mut self, delta: isize) {
        self.cursor = self.cursor.saturating_add_signed(delta);
        self.keep_visible();
    }
    fn edit(&mut self, code: KeyCode, paste: Option<&str>, history: &[String]) -> io::Result<()> {
        let mut field = CommandLine {
            text: self.query.clone(),
            cursor: self.input_cursor,
            ..Default::default()
        };
        match code {
            _ if paste.is_some() => field.insert(paste.unwrap_or_default())?,
            KeyCode::Char(c) => field.insert(&c.to_string())?,
            KeyCode::Left => field.cursor = field.previous(),
            KeyCode::Right => field.cursor = field.next(),
            KeyCode::Backspace => {
                let start = field.previous();
                field.text.replace_range(start..field.cursor, "");
                field.cursor = start;
                field.snap_cursor();
            }
            KeyCode::Delete => {
                let end = field.next();
                field.text.replace_range(field.cursor..end, "");
                field.snap_cursor();
            }
            _ => {}
        }
        let changed = self.query != field.text;
        self.query = field.text;
        self.input_cursor = field.cursor;
        if changed {
            self.refilter(history);
        }
        Ok(())
    }
}
pub fn geometry(area: Rect) -> (Rect, usize) {
    let width = area.width.saturating_sub(6).min(100);
    let height = area.height.saturating_sub(4).min(26);
    (
        Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        ),
        usize::from(height.saturating_sub(8)).max(1),
    )
}
pub fn open(app: &mut App) {
    if !crate::tool_windows::open(app, crate::tool_windows::Kind::CommandHistory, None) {
        return;
    }
    crate::menu::close(app);
    app.modals.set_quick_search(None);
    let state = HistoryDialog::new(&app.command_line.history);
    app.set_command_history(Some(state));
}
pub fn action(app: &mut App, action: Action) {
    if action == Action::Cancel {
        app.set_command_history(None);
        return;
    }
    if action == Action::Use {
        let selected = app
            .command_history()
            .and_then(|d| d.matches.get(d.cursor))
            .and_then(|i| app.command_history().unwrap().source.get(*i))
            .cloned();
        if let Some(text) = selected {
            let line = &mut app.command_line;

            line.text = text;
            line.cursor = line.text.len();
            line.view_start = 0;
            line.recalled = None;
            crate::windowing::activate(app, crate::desktop::WindowId::MANAGER);
        }
        return;
    }
    if let Some(d) = app.command_history_mut() {
        match action {
            Action::Field => d.focus = 0,
            Action::Row(i) if i < d.matches.len() => {
                d.cursor = i;
                d.focus = 1;
                d.keep_visible();
            }
            _ => {}
        }
    }
}
pub fn paste(app: &mut App, text: &str) {
    let history = app.command_line.history.clone();
    let error = app
        .command_history_mut()
        .filter(|d| d.focus == 0)
        .and_then(|d| d.edit(KeyCode::Null, Some(text), &history).err());
    if let Some(e) = error {
        app.error(e.to_string());
    }
}
pub fn key(app: &mut App, key: KeyEvent) {
    let history = app.command_line.history.clone();
    if let Some(m) = app.command_history_mut() {
        m.last_click = None;
    }
    if crate::dialog_controls::key(app, key, crate::focus::Focus::CommandHistory) {
        return;
    }
    let Some(d) = app.command_history_mut() else {
        return;
    };
    d.last_click = None;
    if let Some(row) = crate::turbo::widgets::list_key(d.cursor, d.matches.len(), d.rows, key.code)
    {
        d.cursor = row;
        d.keep_visible();
        return;
    }
    match key.code {
        code if !key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            if matches!(code, KeyCode::Char(_)) {
                d.focus = 0;
            }
            if d.focus == 0 {
                if let Err(e) = d.edit(code, None, &history) {
                    app.error(e.to_string());
                }
            }
        }
        _ => {}
    }
}
pub fn mouse(app: &mut App, event: MouseEvent, target: Option<Target>) {
    match event.kind {
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
            if let Some(d) = app.command_history_mut() {
                d.last_click = None;
                d.step(crate::list_state::wheel_delta(event.kind));
            }
        }
        MouseEventKind::Down(MouseButton::Left) => {
            if let Some(Target::CommandHistory(a)) = target {
                let double = if let Action::Row(i) = a {
                    if let Some(d) = app.command_history_mut() {
                        let now = Instant::now();
                        let double = crate::list_state::ClickTracker::STRICT.matches(
                            d.last_click.as_ref(),
                            &i,
                            now,
                        );
                        d.last_click = Some((i, now));
                        double
                    } else {
                        false
                    }
                } else {
                    if let Some(d) = app.command_history_mut() {
                        d.last_click = None;
                    }
                    false
                };
                action(app, if double { Action::Use } else { a });
            } else if let Some(d) = app.command_history_mut() {
                d.last_click = None;
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filtering_keeps_unicode_mapping_and_original_history_unchanged() {
        let history = vec![
            "echo İSTANBUL".into(),
            "echo ПрИвЕт".into(),
            "printf 世界".into(),
        ];
        let original = history.clone();
        let mut dialog = HistoryDialog::new(&history);
        for (query, expected) in [
            ("i̇stan", vec![0]),
            ("ПРИВЕТ", vec![1]),
            ("世界", vec![2]),
            ("absent", vec![]),
            ("", vec![0, 1, 2]),
        ] {
            dialog.query = query.into();
            dialog.cursor = 99;
            dialog.offset = 99;
            dialog.refilter(&history);
            assert_eq!(dialog.matches, expected);
            assert_eq!(dialog.cursor, 0);
            assert_eq!(dialog.offset, 0);
        }
        assert_eq!(history, original);
    }
    #[test]
    fn invalid_paste_is_atomic_even_with_an_existing_query() {
        let history = vec!["echo one".into()];
        let mut d = HistoryDialog::new(&history);
        d.edit(KeyCode::Null, Some("echo"), &history).unwrap();
        for bad in [
            "\n",
            "\r",
            "\t",
            "\x1b[31m",
            &"x".repeat(super::super::MAX_BYTES),
        ] {
            let before = (d.query.clone(), d.input_cursor, d.matches.clone());
            assert!(d.edit(KeyCode::Null, Some(bad), &history).is_err());
            assert_eq!((d.query.clone(), d.input_cursor, d.matches.clone()), before);
        }
    }
    #[test]
    fn grapheme_editing_deletes_whole_clusters_at_both_ends() {
        let history = vec![];
        let mut d = HistoryDialog::new(&history);
        d.edit(KeyCode::Null, Some("a👩‍💻é"), &history).unwrap();
        d.edit(KeyCode::Left, None, &history).unwrap();
        assert_eq!(d.input_cursor, "a👩‍💻".len());
        d.edit(KeyCode::Backspace, None, &history).unwrap();
        assert_eq!(d.query, "aé");
        assert_eq!(d.input_cursor, 1);
        d.edit(KeyCode::Delete, None, &history).unwrap();
        assert_eq!(d.query, "a");
        d.edit(KeyCode::Backspace, None, &history).unwrap();
        d.edit(KeyCode::Backspace, None, &history).unwrap();
        d.edit(KeyCode::Delete, None, &history).unwrap();
        assert_eq!(d.query, "");
        assert_eq!(d.input_cursor, 0);
    }
    #[test]
    fn selection_remains_visible_after_resizing_and_saturating_navigation() {
        for count in 0..=100 {
            let history = vec!["command".into(); count];
            let mut d = HistoryDialog::new(&history);
            for rows in [1, 17, 3, 200, 0, 5] {
                d.rows = rows;
                for delta in [isize::MAX, isize::MIN, 9, -3] {
                    d.step(delta);
                    assert!(d.rows > 0);
                    assert!(d.cursor <= count.saturating_sub(1));
                    assert!(d.offset <= d.cursor);
                    assert!(d.cursor < d.offset + d.rows);
                }
            }
        }
    }
}
