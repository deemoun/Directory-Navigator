pub mod calculator;
pub mod characters;
pub mod information;
use crate::{app::App, menu};
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
    pub chart: characters::Chart,
    pub information: Option<information::Information>,
    pub size: (u16, u16),
    pub focus: usize,
    pub notice: String,
}
pub fn open(app: &mut App, kind: Kind) {
    if kind == Kind::Calculator {
        crate::plugins::host::open(app, crate::plugins::CALCULATOR);
        return;
    }
    menu::close(app);
    app.modals.set_quick_search(None);
    app.utilities.modals.set_utility_dialog(Some(kind));
    app.utilities.notice.clear();
    app.utilities.focus = 0;
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
/// Compatibility launcher/actions; the calculator controller lives in plugins.
pub fn calculator_target(a: Action) -> crate::mouse::Target {
    crate::mouse::Target::Plugin(
        crate::plugins::CALCULATOR,
        crate::plugins::ActionId(calculator_action(a).expect("calculator action").id()),
    )
}
fn calculator_action(a: Action) -> Option<crate::plugins::calculator::Action> {
    use crate::plugins::calculator::Action as C;
    Some(match a {
        Action::Close => C::Close,
        Action::Evaluate => C::Evaluate,
        Action::Copy => C::Copy,
        Action::Base(i) => C::Base(i),
        Action::Field => C::Field,
        Action::CalculatorHelp => C::Help,
        Action::CalculatorHistory => C::History,
        Action::CalculatorRecall(i) => C::Recall(i),
        _ => return None,
    })
}
pub fn action(app: &mut App, a: Action) {
    if crate::plugins::host::active(app).is_some_and(|b| b.plugin == crate::plugins::CALCULATOR) {
        if let Some(a) = calculator_action(a) {
            let b = crate::plugins::host::active(app).unwrap();
            crate::plugins::host::dispatch(
                app,
                b,
                crate::plugins::Event::Action(crate::plugins::ActionId(a.id())),
            );
        }
        return;
    }
    match a {
        Action::Close => {
            app.utilities.modals.set_utility_dialog(None);
            app.utilities.information = None;
        }
        Action::Copy => {
            if app.utilities.modals.utility_dialog() != Some(Kind::Characters) {
                return;
            }
            match app
                .utilities
                .chart
                .copy_text()
                .ok_or("ASCII controls cannot be copied")
            {
                Ok(text) => {
                    app.editor_clipboard = text;
                    app.utilities.notice = "Copied to DN internal clipboard".into();
                }
                Err(e) => app.utilities.notice = e.into(),
            }
        }
        Action::Page(page) => {
            app.utilities.chart.page = page;
            app.utilities.chart.focus = 0;
        }
        Action::Cell(cell) => {
            crate::turbo::characters::select(&mut app.utilities.chart, cell);
            app.utilities.chart.focus = 0;
        }
        Action::Refresh => refresh(app),
        _ => {}
    }
}
pub const CALCULATOR_HELP: &str = "Enter evaluates · Tab / Shift+Tab move focus\nAlt+P expression · Alt+E evaluate · Alt+C copy\nAlt+L close · F1 / Alt+H help\nUp/Down recall history while editing\nThe arrow button opens session history; Enter recalls\nDEC / HEX / BIN / OCT / EXP choose the copy format\nCtrl+C copies the full value to the internal clipboard\nLong values are shortened on screen, copied in full\nEXP is approximate scientific notation (15 significant digits)\nOperators: + - * / % ^ and parentheses\nFunctions: sqrt(), abs() · Bases: 0x, 0b, 0o\nOnly expressions are evaluated; commands are never run.";
pub fn key(app: &mut App, key: KeyEvent) {
    if crate::dialog_controls::key(app, key, crate::focus::Focus::Utility) {
        return;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        action(app, Action::Copy);
        return;
    }
    let mut command = None;
    match app.utilities.modals.utility_dialog() {
        Some(Kind::Calculator) => {}
        Some(Kind::Characters) => {
            let chart = &mut app.utilities.chart;
            match key.code {
                code if chart.focus == 0 => {
                    crate::turbo::characters::key(chart, code);
                }
                _ => {}
            }
        }
        Some(Kind::System | Kind::Memory) => match key.code {
            KeyCode::Char('r') | KeyCode::F(5) => command = Some(Action::Refresh),
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
