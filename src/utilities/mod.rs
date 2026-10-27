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
    CalculatorHelp,
    CalculatorHistory,
    CalculatorRecall(usize),
}
#[derive(Default)]
pub struct Utilities {
    pub modals: crate::modals::ModalStack,
    pub calculator: calculator::Calculator,
    pub chart: characters::Chart,
    pub information: Option<information::Information>,
    pub size: (u16, u16),
    pub focus: usize,
    pub notice: String,
}
impl Utilities {
    pub fn paste(&mut self, text: &str) {
        if self.modals.utility_dialog() == Some(Kind::Calculator)
            && self.calculator.focus == 0
            && !self.calculator.history_open
        {
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
    app.modals.set_quick_search(None);
    app.utilities.modals.set_utility_dialog(Some(kind));
    app.utilities.notice.clear();
    app.utilities.focus = 0;
    app.utilities.calculator.focus = 0;
    app.utilities.calculator.history_open = false;
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
    let kind = match app.utilities.modals.utility_dialog() {
        Some(Kind::System) => information::Kind::System,
        Some(Kind::Memory) => information::Kind::Memory,
        _ => return,
    };
    match information::Information::open(
        kind,
        app.panels[app.active].path.clone(),
        app.utilities.size,
    ) {
        Ok(info) => {
            if let Some(modal) = app.utilities.modals.infos().last() {
                info.requests.bind(crate::requests::Owner::Modal(modal.id));
            }
            app.utilities.information = Some(info);
        }
        Err(e) => app.error(format!("Cannot start information worker: {e}")),
    }
}
pub fn poll(app: &mut App) -> bool {
    let owner = app
        .utilities
        .modals
        .infos()
        .find(|m| m.focus == crate::focus::Focus::Utility)
        .map(|m| crate::requests::Owner::Modal(m.id));
    if app
        .utilities
        .information
        .as_ref()
        .is_some_and(|info| Some(info.requests.owner()) != owner)
    {
        app.utilities.information = None;
        return true;
    }
    app.utilities.information.as_mut().is_some_and(|i| i.poll())
}
pub fn action(app: &mut App, action: Action) {
    match action {
        Action::Close => {
            app.utilities.modals.set_utility_dialog(None);
            app.utilities.information = None;
        }
        Action::Evaluate => {
            if app.utilities.modals.utility_dialog() == Some(Kind::Calculator) {
                app.utilities.calculator.evaluate();
                app.utilities.notice.clear();
            }
        }
        Action::Copy => {
            let result = match app.utilities.modals.utility_dialog() {
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
            if index < 5 {
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
            crate::turbo::characters::select(&mut app.utilities.chart, cell);
            app.utilities.chart.focus = 0;
        }
        Action::Refresh => refresh(app),
        Action::CalculatorHelp
            if app.utilities.modals.utility_dialog() == Some(Kind::Calculator) =>
        {
            app.push_dialog(dialog::Dialog {
                title: " Calculator Help ",
                message: CALCULATOR_HELP.into(),
            });
        }
        Action::CalculatorHistory
            if app.utilities.modals.utility_dialog() == Some(Kind::Calculator) =>
        {
            let calc = &mut app.utilities.calculator;
            calc.history_open = !calc.history_open;
            calc.history_selection = 0;
        }
        Action::CalculatorRecall(index)
            if app.utilities.modals.utility_dialog() == Some(Kind::Calculator) =>
        {
            let calc = &mut app.utilities.calculator;
            if let Some(expression) = calc.history.get(index).cloned() {
                if calc.history_cursor.is_none() {
                    calc.draft = calc.expression.clone();
                }
                calc.expression = expression;
                calc.cursor = calc.expression.len();
                calc.history_cursor = Some(index);
                calc.result = None;
                calc.error = None;
                calc.focus = 0;
                calc.history_open = false;
                app.utilities.notice.clear();
            }
        }
        Action::CalculatorHelp | Action::CalculatorHistory | Action::CalculatorRecall(_) => {}
    }
}
pub const CALCULATOR_HELP: &str = "Enter evaluates · Tab / Shift+Tab move focus\nAlt+P expression · Alt+E evaluate · Alt+C copy\nAlt+L close · F1 / Alt+H help\nUp/Down recall history while editing\nThe arrow button opens session history; Enter recalls\nDEC / HEX / BIN / OCT / EXP choose the copy format\nCtrl+C copies the full value to the internal clipboard\nLong values are shortened on screen, copied in full\nEXP is approximate scientific notation (15 significant digits)\nOperators: + - * / % ^ and parentheses\nFunctions: sqrt(), abs() · Bases: 0x, 0b, 0o\nOnly expressions are evaluated; commands are never run.";
pub fn key(app: &mut App, key: KeyEvent) {
    if app.utilities.modals.utility_dialog() == Some(Kind::Calculator) {
        if app.utilities.calculator.history_open {
            let calc = &mut app.utilities.calculator;
            match key.code {
                KeyCode::Esc => calc.history_open = false,
                KeyCode::Up => calc.history_selection = calc.history_selection.saturating_sub(1),
                KeyCode::Down => {
                    calc.history_selection =
                        (calc.history_selection + 1).min(calc.history.len().saturating_sub(1))
                }
                KeyCode::Home => calc.history_selection = 0,
                KeyCode::End => calc.history_selection = calc.history.len().saturating_sub(1),
                KeyCode::PageUp => {
                    calc.history_selection = calc.history_selection.saturating_sub(6)
                }
                KeyCode::PageDown => {
                    calc.history_selection =
                        (calc.history_selection + 6).min(calc.history.len().saturating_sub(1))
                }
                KeyCode::Enter => {
                    let index = calc.history_selection;
                    action(app, Action::CalculatorRecall(index));
                }
                _ => {}
            }
            return;
        }
        let command = if key.code == KeyCode::F(1) {
            Some(Action::CalculatorHelp)
        } else if key.modifiers == KeyModifiers::ALT {
            match key.code {
                KeyCode::Char('e' | 'E') => Some(Action::Evaluate),
                KeyCode::Char('c' | 'C') => Some(Action::Copy),
                KeyCode::Char('l' | 'L') => Some(Action::Close),
                KeyCode::Char('h' | 'H') => Some(Action::CalculatorHelp),
                KeyCode::Char('p' | 'P') => Some(Action::Field),
                _ => None,
            }
        } else {
            None
        };
        if let Some(command) = command {
            action(app, command);
            return;
        }
    }
    if key.code == KeyCode::Esc {
        action(app, Action::Close);
        return;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        action(app, Action::Copy);
        return;
    }
    let mut command = None;
    match app.utilities.modals.utility_dialog() {
        Some(Kind::Calculator) => {
            let calc = &mut app.utilities.calculator;
            match key.code {
                KeyCode::Tab => calc.focus = crate::control_focus::cycle(calc.focus, 10, false),
                KeyCode::BackTab => calc.focus = crate::control_focus::cycle(calc.focus, 10, true),
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
                KeyCode::Up | KeyCode::Down if (1..=5).contains(&calc.focus) => {
                    calc.focus = crate::control_focus::select(
                        calc.focus,
                        &[1, 2, 3, 4, 5],
                        key.code == KeyCode::Up,
                    );
                    command = Some(Action::Base(calc.focus - 1));
                }
                KeyCode::Enter => {
                    command = Some(match calc.focus {
                        7 => Action::Copy,
                        8 => Action::Close,
                        9 => Action::CalculatorHelp,
                        _ => Action::Evaluate,
                    })
                }
                KeyCode::Char(' ') if (1..=5).contains(&calc.focus) => {
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
            match key.code {
                KeyCode::Tab | KeyCode::BackTab => {
                    let choices: &[usize] = if chart.copy_text().is_some() {
                        &[0, 1, 2, 3, 4]
                    } else {
                        &[0, 1, 2, 4]
                    };
                    chart.focus = crate::control_focus::select(
                        chart.focus,
                        choices,
                        key.code == KeyCode::BackTab,
                    );
                }
                KeyCode::Enter | KeyCode::Char(' ') => {
                    command = Some(match chart.focus {
                        1 => Action::Page(Page::Ascii),
                        2 => Action::Page(Page::Cp437),
                        4 => Action::Close,
                        _ => Action::Copy,
                    })
                }
                code if chart.focus == 0 => {
                    crate::turbo::characters::key(chart, code);
                }
                _ => {}
            }
        }
        Some(Kind::System | Kind::Memory) => match key.code {
            KeyCode::Tab | KeyCode::BackTab => {
                app.utilities.focus = crate::control_focus::cycle(app.utilities.focus, 2, false)
            }
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
    if app.utilities.modals.utility_dialog() == Some(Kind::Calculator)
        && app.utilities.calculator.history_open
    {
        if event.kind == MouseEventKind::Down(MouseButton::Left) {
            if let Some(crate::mouse::Target::Utility(
                command @ (Action::CalculatorRecall(_) | Action::CalculatorHistory),
            )) = target
            {
                action(app, command);
            } else {
                app.utilities.calculator.history_open = false;
            }
        }
        return;
    }
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
