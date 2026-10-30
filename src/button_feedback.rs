//! Terminal input feedback for shared push buttons. Controllers still execute
//! their existing guarded commands; this layer only holds the visual press.
use crate::{
    app::App,
    focus::Focus,
    layout::FrameContext,
    mouse::Target,
    presentation::layout::Rect,
    turbo::{
        dialog_spec::{input_area, ControlKind, DialogSpec},
        scene::Scene,
    },
};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, MouseButton, MouseEvent, MouseEventKind};
use std::time::{Duration, Instant};
const PRESS_TIME: Duration = Duration::from_millis(90);
#[derive(Default)]
pub(crate) struct State {
    press: Option<Press>,
}
struct Press {
    context: FrameContext,
    target: Option<Target>,
    rect: Rect,
    event: Activation,
    inside: bool,
    earliest: Instant,
}
#[derive(Clone, Copy)]
enum Activation {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Released(MouseEvent),
}
fn spec(app: &App) -> Option<DialogSpec<'_>> {
    let area = input_area(app);
    match crate::focus::current(app) {
        Focus::Prompt => {
            crate::ui::copy_spec(app, area).or_else(|| crate::ui::prompt_spec(app, area))
        }
        Focus::Settings => crate::ui::settings_spec(app, area),
        Focus::EditorModal => crate::ui::editor_spec(app, area),
        Focus::ViewerSearch => crate::ui::viewer_search_spec(app, area),
        focus => crate::ui::remaining_spec(app, area, focus),
    }
}
fn native_rect(app: &App, key: KeyEvent) -> Option<Rect> {
    if crate::focus::current(app) != Focus::EditorModal {
        return None;
    }
    let crate::editor::Modal::File(picker) = app.desktop.active_editor()?.modals.editor_modal()?
    else {
        return None;
    };
    let mut r = picker.activation_rect(key)?;
    let (_, bounds, _) = app.layout_key?;
    r.x += bounds.x;
    r.y += bounds.y;
    Some(r)
}
fn button(app: &App, target: Target) -> bool {
    spec(app).is_some_and(|s| {
        s.proposal(None).current(app)
            && s.controls.iter().any(|c| {
                c.enabled
                    && c.paint
                    && c.action == target
                    && matches!(c.kind, ControlKind::Button(_))
            })
    })
}
fn rect(app: &App, target: Target) -> Option<Rect> {
    app.mouse
        .regions
        .iter()
        .rev()
        .find(|(_, t)| *t == target)
        .map(|(r, _)| *r)
}
impl Press {
    fn valid(&self, app: &App) -> bool {
        crate::eligibility::Gate::new(app).frame(Some(self.context))
            && match self.target {
                Some(t) => button(app, t) && rect(app, t) == Some(self.rect),
                None => match self.event {
                    Activation::Key(k) => native_rect(app, k) == Some(self.rect),
                    _ => false,
                },
            }
    }
    fn dispatch(self, app: &mut App) {
        if self.valid(app) {
            match self.event {
                Activation::Key(k) => crate::input::handle(app, k),
                Activation::Mouse(m) | Activation::Released(m) => crate::mouse::handle(app, m),
            }
        }
    }
}
pub(crate) fn busy(app: &App) -> bool {
    app.button_feedback.press.is_some()
}
pub(crate) fn cancel(app: &mut App) {
    app.button_feedback.press = None;
}
fn start(app: &mut App, target: Target, event: Activation) -> bool {
    let Some(context) = app.mouse.frame else {
        return false;
    };
    let Some(rect) = rect(app, target) else {
        return false;
    };
    let p = Press {
        context,
        target: Some(target),
        rect,
        event,
        inside: true,
        earliest: Instant::now() + PRESS_TIME,
    };
    if !p.valid(app) {
        return false;
    }
    app.button_feedback.press = Some(p);
    true
}
pub(crate) fn key(app: &mut App, key: KeyEvent) -> bool {
    if key.kind == KeyEventKind::Release {
        return busy(app);
    }
    if let Some(p) = app.button_feedback.press.take() {
        if key.code == KeyCode::Esc {
            return true;
        }
        if matches!(p.event, Activation::Mouse(_)) {
            app.button_feedback.press = Some(p);
            return true;
        }
        // Repeats must not activate a freshly opened child dialog.
        if key.kind == KeyEventKind::Repeat {
            app.button_feedback.press = Some(p);
            return true;
        }
        p.dispatch(app);
    }
    if !matches!(key.code, KeyCode::Enter | KeyCode::Char(' ')) {
        return false;
    }
    let target = spec(app).and_then(|s| s.key(key)).and_then(|p| p.action);
    if target.is_some_and(|t| {
        button(app, t) && (key.kind == KeyEventKind::Repeat || start(app, t, Activation::Key(key)))
    }) {
        return true;
    }
    if let (Some(context), Some(rect)) = (app.mouse.frame, native_rect(app, key)) {
        if key.kind == KeyEventKind::Repeat {
            return true;
        }
        let p = Press {
            context,
            target: None,
            rect,
            event: Activation::Key(key),
            inside: true,
            earliest: Instant::now() + PRESS_TIME,
        };
        if p.valid(app) {
            app.button_feedback.press = Some(p);
            return true;
        }
    }
    false
}
pub(crate) fn mouse(app: &mut App, event: MouseEvent) -> bool {
    if let Some(mut p) = app.button_feedback.press.take() {
        if !p.valid(app) {
            return true;
        }
        if let Activation::Mouse(down) = p.event {
            p.inside = p.rect.contains((event.column, event.row).into());
            match event.kind {
                MouseEventKind::Up(MouseButton::Left) => {
                    if p.inside {
                        if Instant::now() >= p.earliest {
                            p.dispatch(app);
                        } else {
                            p.event = Activation::Released(down);
                            app.button_feedback.press = Some(p);
                        }
                    }
                }
                _ => app.button_feedback.press = Some(p),
            }
            return true;
        }
        // Finish a previous keyboard/click animation before another gesture.
        p.dispatch(app);
    }
    if event.kind != MouseEventKind::Down(MouseButton::Left) {
        return false;
    }
    let target = app
        .mouse
        .regions
        .iter()
        .rev()
        .find(|(r, _)| r.contains((event.column, event.row).into()))
        .map(|(_, t)| *t);
    target.is_some_and(|t| button(app, t) && start(app, t, Activation::Mouse(event)))
}
pub(crate) fn poll(app: &mut App, now: Instant) -> bool {
    let Some(p) = app.button_feedback.press.take() else {
        return false;
    };
    if !p.valid(app) {
        return true;
    }
    if !matches!(p.event, Activation::Mouse(_)) && now >= p.earliest {
        p.dispatch(app);
        true
    } else {
        app.button_feedback.press = Some(p);
        false
    }
}
pub(crate) fn next_wake(app: &App, now: Instant) -> Option<Duration> {
    app.button_feedback
        .press
        .as_ref()
        .filter(|p| !matches!(p.event, Activation::Mouse(_)))
        .map(|p| p.earliest.saturating_duration_since(now))
}
pub(crate) fn pressed(app: &App, context: FrameContext) -> Option<Rect> {
    app.button_feedback
        .press
        .as_ref()
        .filter(|p| p.context == context && p.inside && p.valid(app))
        .map(|p| p.rect)
}
/// DN shifts the face right by one cell and removes the shadow and arrows.
/// Copy actual glyphs/styles, preserving translated labels and custom colours.
pub(crate) fn paint(scene: &mut Scene, rect: Rect) {
    if rect.width < 4 || rect.height == 0 {
        return;
    }
    for y in rect.y..rect.bottom().min(scene.height) {
        let Some(mut background) = scene.cells.get(&(rect.x, y)).cloned() else {
            continue;
        };
        background.text = " ".into();
        let row = (rect.x..rect.right())
            .map(|x| scene.cells.get(&(x, y)).cloned())
            .collect::<Vec<_>>();
        for x in rect.x..rect.right().min(scene.width) {
            let offset = x - rect.x;
            let mut glyph = if offset < 2 {
                Some(background.clone())
            } else {
                row[usize::from(offset - 1)].clone()
            };
            if let Some(g) = &mut glyph {
                if (offset == 2 && g.text == "►") || (offset + 1 == rect.width && g.text == "◄")
                {
                    g.text = " ".into();
                }
            }
            if let Some(g) = glyph {
                scene.cells.insert((x, y), g);
            }
            if rect.bottom() < scene.height {
                scene.cells.insert((x, rect.bottom()), background.clone());
            }
        }
    }
}
