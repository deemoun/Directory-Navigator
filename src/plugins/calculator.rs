//! Session calculator, implemented entirely through the built-in tool interface.
use super::*;
use crate::{dialog, utilities::calculator::Calculator};
use crossterm::event::{KeyCode as K, KeyModifiers as M, MouseButton, MouseEventKind};
#[derive(Clone, Copy)]
pub(crate) enum Action {
    Close,
    Evaluate,
    Copy,
    Base(usize),
    Field,
    Help,
    History,
    Recall(usize),
}
impl Action {
    pub(crate) fn id(self) -> usize {
        match self {
            Self::Close => 0,
            Self::Evaluate => 1,
            Self::Copy => 2,
            Self::Field => 3,
            Self::Help => 4,
            Self::History => 5,
            Self::Base(i) => 100 + i,
            Self::Recall(i) => 1000 + i,
        }
    }
    fn from_id(i: usize) -> Option<Self> {
        Some(match i {
            0 => Self::Close,
            1 => Self::Evaluate,
            2 => Self::Copy,
            3 => Self::Field,
            4 => Self::Help,
            5 => Self::History,
            100..=104 => Self::Base(i - 100),
            1000.. => Self::Recall(i - 1000),
            _ => return None,
        })
    }
}
#[derive(Default)]
pub(crate) struct CalculatorPlugin {
    pub state: Calculator,
    pub notice: String,
}
#[path = "calculator_view.rs"]
mod view;
impl CalculatorPlugin {
    fn action(&mut self, a: Action) -> Vec<Effect> {
        let calc = &mut self.state;
        match a {
            Action::Close => return vec![Effect::Close],
            Action::Evaluate => {
                calc.evaluate();
                self.notice.clear();
            }
            Action::Copy => match calc.copy_text() {
                Ok(s) => {
                    self.notice = "Copied to DN internal clipboard".into();
                    return vec![Effect::CopyText(s)];
                }
                Err(e) => self.notice = e,
            },
            Action::Base(i) => {
                calc.base = i;
                calc.focus = i + 1;
            }
            Action::Field => calc.focus = 0,
            Action::Help => {
                return vec![Effect::Help {
                    title: " Calculator Help ",
                    text: crate::utilities::CALCULATOR_HELP.into(),
                }]
            }
            Action::History => {
                calc.history_open = !calc.history_open;
                calc.history_selection = 0;
            }
            Action::Recall(index) => {
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
                    self.notice.clear();
                }
            }
        }
        Vec::new()
    }
    fn paste(&mut self, text: &str) {
        let c = &mut self.state;
        if c.focus != 0 || c.history_open {
            return;
        }
        let before = c.expression.clone();
        let cursor = c.cursor;
        dialog::paste_input(&mut c.expression, &mut c.cursor, text);
        if c.expression.len() > 2048 {
            c.expression = before;
            c.cursor = cursor;
        } else {
            c.result = None;
            c.error = None;
            c.history_cursor = None;
        }
    }
    fn key(&mut self, k: KeyEvent) -> Vec<Effect> {
        if self.state.history_open {
            let c = &mut self.state;
            c.history_selection = match k.code {
                K::Up => c.history_selection.saturating_sub(1),
                K::Down => (c.history_selection + 1).min(c.history.len().saturating_sub(1)),
                K::Home => 0,
                K::End => c.history.len().saturating_sub(1),
                K::PageUp => c.history_selection.saturating_sub(6),
                K::PageDown => (c.history_selection + 6).min(c.history.len().saturating_sub(1)),
                _ => c.history_selection,
            };
            return Vec::new();
        }
        let action = if k.code == K::F(1) {
            Some(Action::Help)
        } else if k.modifiers == M::ALT {
            match k.code {
                K::Char('e' | 'E') => Some(Action::Evaluate),
                K::Char('c' | 'C') => Some(Action::Copy),
                K::Char('l' | 'L') => Some(Action::Close),
                K::Char('h' | 'H') => Some(Action::Help),
                K::Char('p' | 'P') => Some(Action::Field),
                _ => None,
            }
        } else if k.modifiers.contains(M::CONTROL) && k.code == K::Char('c') {
            Some(Action::Copy)
        } else {
            None
        };
        if let Some(a) = action {
            return self.action(a);
        }
        let c = &mut self.state;
        match k.code {
            K::Up | K::Down if c.focus == 0 => {
                c.recall(k.code == K::Up);
                c.result = None;
                c.error = None;
            }
            K::Up | K::Down if (1..=5).contains(&c.focus) => {
                c.focus = crate::control_focus::select(c.focus, &[1, 2, 3, 4, 5], k.code == K::Up);
                c.base = c.focus - 1;
            }
            code if c.focus == 0 && !k.modifiers.intersects(M::CONTROL | M::ALT) => {
                let before = c.expression.clone();
                let cursor = c.cursor;
                dialog::edit_input(&mut c.expression, &mut c.cursor, code);
                if c.expression.len() > 2048 {
                    c.expression = before;
                    c.cursor = cursor;
                } else if c.expression != before {
                    c.result = None;
                    c.error = None;
                    c.history_cursor = None;
                }
            }
            _ => {}
        }
        Vec::new()
    }
}
impl ToolPlugin for CalculatorPlugin {
    fn definition(&self) -> Definition {
        Definition::builtin(CALCULATOR, "Calculator", Presentation::Dialog)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn can_open(&self, c: &OpenContext) -> bool {
        !c.blocking
    }
    fn open(&mut self, _: &Context) {
        self.notice.clear();
        self.state.focus = 0;
        self.state.history_open = false;
    }
    fn event(&mut self, e: Event<'_>, _: &Context) -> Vec<Effect> {
        match e {
            Event::Key(k) => self.key(k),
            Event::Paste(s) => {
                self.paste(s);
                Vec::new()
            }
            Event::Action(a) => Action::from_id(a.0).map_or_else(Vec::new, |a| self.action(a)),
            Event::Focus(f) => {
                if !self.state.history_open {
                    self.state.focus = f;
                }
                Vec::new()
            }
            Event::Mouse(e, a) if e.kind == MouseEventKind::Down(MouseButton::Left) => {
                let a = a.and_then(|a| Action::from_id(a.0));
                if self.state.history_open
                    && !matches!(a, Some(Action::Recall(_) | Action::History))
                {
                    self.state.history_open = false;
                    return Vec::new();
                }
                a.map_or_else(Vec::new, |a| self.action(a))
            }
            _ => Vec::new(),
        }
    }
    fn controls(&self, ctx: &Context) -> DialogSpec<'_> {
        view::history_spec(self, ctx).unwrap_or_else(|| view::calculator_spec(self, ctx))
    }
    fn view(&self, ctx: &Context) -> View<'_> {
        View::Dialog(Box::new(view::calculator_spec(self, ctx)))
    }
    fn close(&mut self) {
        self.state.history_open = false;
    }
}
