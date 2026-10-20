pub mod calculator;
pub mod characters;
pub mod information;
use crate::{app::App, dialog, menu};
use characters::Page;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Calculator,
    Characters,
    System,
    Memory,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Close,
    Evaluate,
    Copy,
    Base(usize),
    Field,
    Page(Page),
    Cell(u8),
    Refresh,
}
#[derive(Default)]
pub struct Utilities {
    pub dialog: Option<Kind>,
    pub calculator: calculator::Calculator,
    pub chart: characters::Chart,
    pub information: Option<information::Information>,
    pub size: (u16, u16),
    pub focus: usize,
    pub notice: String,
}
impl Utilities {
    pub fn paste(&mut self, text: &str) {
        if self.dialog == Some(Kind::Calculator) && self.calculator.focus == 0 {
            let calc = &mut self.calculator;
            let before = calc.expression.clone();
            let cursor = calc.cursor;
            dialog::paste_input(&mut calc.expression, &mut calc.cursor, text);
            if calc.expression.len() > 2048 {
                calc.expression = before;
                calc.cursor = cursor;
            } else {
                calc.result = None;
                calc.error = None;
                calc.history_cursor = None;
            }
        }
    }
}
pub fn open(app: &mut App, kind: Kind) {
    menu::close(app);
    app.quick_search = None;
    app.utilities.dialog = Some(kind);
    app.utilities.notice.clear();
    app.utilities.focus = 0;
    app.utilities.calculator.focus = 0;
    app.utilities.chart.focus = 0;
    if matches!(kind, Kind::System | Kind::Memory) {
        app.utilities.information = None;
        refresh(app);
    } else {
        app.utilities.information = None;
    }
}
fn refresh(app: &mut App) {
    if app
        .utilities
        .information
        .as_ref()
        .is_some_and(|i| i.loading())
    {
        return;
    }
    let kind = match app.utilities.dialog {
        Some(Kind::System) => information::Kind::System,
        Some(Kind::Memory) => information::Kind::Memory,
        _ => return,
    };
    match information::Information::open(
        kind,
        app.panels[app.active].path.clone(),
        app.utilities.size,
    ) {
        Ok(info) => app.utilities.information = Some(info),
        Err(e) => app.error(format!("Cannot start information worker: {e}")),
    }
}
pub fn poll(app: &mut App) -> bool {
    app.utilities.information.as_mut().is_some_and(|i| i.poll())
}
pub fn action(app: &mut App, action: Action) {
    match action {
        Action::Close => {
            app.utilities.dialog = None;
            app.utilities.information = None;
        }
        Action::Evaluate => {
            if app.utilities.dialog == Some(Kind::Calculator) {
                app.utilities.calculator.evaluate();
                app.utilities.notice.clear();
            }
        }
        Action::Copy => {
            let result = match app.utilities.dialog {
                Some(Kind::Calculator) => app.utilities.calculator.copy_text(),
                Some(Kind::Characters) => app
                    .utilities
                    .chart
                    .copy_text()
                    .ok_or("ASCII controls cannot be copied".into()),
                _ => return,
            };
            match result {
                Ok(text) => {
                    app.editor_clipboard = text;
                    app.utilities.notice = "Copied to DN internal clipboard".into();
                }
                Err(message) => app.utilities.notice = message,
            }
        }
        Action::Base(index) => {
            if index < 4 {
                app.utilities.calculator.base = index;
                app.utilities.calculator.focus = index + 1;
            }
        }
        Action::Field => app.utilities.calculator.focus = 0,
        Action::Page(page) => {
            app.utilities.chart.page = page;
            app.utilities.chart.focus = 0;
        }
        Action::Cell(cell) => {
            app.utilities.chart.select(cell);
            app.utilities.chart.focus = 0;
        }
        Action::Refresh => refresh(app),
    }
}
pub fn key(app: &mut App, key: KeyEvent) {
    if key.code == KeyCode::Esc {
        action(app, Action::Close);
        return;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        action(app, Action::Copy);
        return;
    }
    let mut command = None;
    match app.utilities.dialog {
        Some(Kind::Calculator) => {
            let calc = &mut app.utilities.calculator;
            match key.code {
                KeyCode::Tab => calc.focus = (calc.focus + 1) % 8,
                KeyCode::BackTab => calc.focus = (calc.focus + 7) % 8,
                KeyCode::Up if calc.focus == 0 => {
                    calc.recall(true);
                    calc.result = None;
                    calc.error = None;
                }
                KeyCode::Down if calc.focus == 0 => {
                    calc.recall(false);
                    calc.result = None;
                    calc.error = None;
                }
                KeyCode::Enter => {
                    command = Some(match calc.focus {
                        1..=4 => Action::Base(calc.focus - 1),
                        6 => Action::Copy,
                        7 => Action::Close,
                        _ => Action::Evaluate,
                    })
                }
                KeyCode::Char(' ') if (1..=4).contains(&calc.focus) => {
                    command = Some(Action::Base(calc.focus - 1))
                }
                code if calc.focus == 0
                    && !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    let before = calc.expression.clone();
                    let cursor = calc.cursor;
                    dialog::edit_input(&mut calc.expression, &mut calc.cursor, code);
                    if calc.expression.len() > 2048 {
                        calc.expression = before;
                        calc.cursor = cursor;
                    } else if calc.expression != before {
                        calc.result = None;
                        calc.error = None;
                        calc.history_cursor = None;
                    }
                }
                _ => {}
            }
        }
        Some(Kind::Characters) => {
            let chart = &mut app.utilities.chart;
            let byte = chart.selected();
            match key.code {
                KeyCode::Tab => chart.focus = (chart.focus + 1) % 5,
                KeyCode::BackTab => chart.focus = (chart.focus + 4) % 5,
                KeyCode::Enter | KeyCode::Char(' ') => {
                    command = Some(match chart.focus {
                        1 => Action::Page(Page::Ascii),
                        2 => Action::Page(Page::Cp437),
                        4 => Action::Close,
                        _ => Action::Copy,
                    })
                }
                code if chart.focus == 0 => {
                    let next = match code {
                        KeyCode::Left => byte.saturating_sub(1),
                        KeyCode::Right => byte.saturating_add(1),
                        KeyCode::Up => byte.saturating_sub(16),
                        KeyCode::Down => byte.saturating_add(16),
                        KeyCode::Home => 0,
                        KeyCode::End => chart.limit(),
                        _ => byte,
                    };
                    chart.select(next);
                }
                _ => {}
            }
        }
        Some(Kind::System | Kind::Memory) => match key.code {
            KeyCode::Tab | KeyCode::BackTab => app.utilities.focus ^= 1,
            KeyCode::Char('r') | KeyCode::F(5) => command = Some(Action::Refresh),
            KeyCode::Enter => {
                command = Some(if app.utilities.focus == 0 {
                    Action::Refresh
                } else {
                    Action::Close
                })
            }
            code => {
                if let Some(info) = &mut app.utilities.information {
                    match code {
                        KeyCode::Up => info.step(-1),
                        KeyCode::Down => info.step(1),
                        KeyCode::PageUp => info.step(-(info.rows as isize)),
                        KeyCode::PageDown => info.step(info.rows as isize),
                        KeyCode::Home => {
                            info.offset = 0;
                            info.horizontal = 0;
                        }
                        KeyCode::End => info.step(isize::MAX),
                        KeyCode::Left => info.horizontal = info.horizontal.saturating_sub(8),
                        KeyCode::Right => info.horizontal = info.horizontal.saturating_add(8),
                        _ => {}
                    }
                }
            }
        },
        None => {}
    }
    if let Some(command) = command {
        action(app, command);
    }
}
pub fn mouse(app: &mut App, event: MouseEvent, target: Option<crate::mouse::Target>) {
    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if let Some(crate::mouse::Target::Utility(command)) = target {
                action(app, command);
            }
        }
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
            if let Some(info) = &mut app.utilities.information {
                info.step(if event.kind == MouseEventKind::ScrollUp {
                    -3
                } else {
                    3
                });
            }
        }
        _ => {}
    }
}
