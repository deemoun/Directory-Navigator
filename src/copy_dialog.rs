//! Shared DN-style Copy and Rename/move controls; filesystem policy stays in operations.
use crate::{
    app::App,
    modals::ModalId,
    mouse::Target,
    operations::{CopyOptions, ExistingDestination},
};
use crossterm::event::{
    KeyCode as K, KeyEvent, KeyModifiers as M, MouseButton, MouseEvent, MouseEventKind,
};
use std::path::PathBuf;
#[derive(Default)]
pub struct State {
    pub history: Vec<PathBuf>,
}
impl State {
    pub fn record(&mut self, path: PathBuf) {
        crate::history::remember(&mut self.history, path, 16);
    }
}
#[derive(Default)]
pub struct Draft {
    pub options: CopyOptions,
    pub picked: Option<PathBuf>,
}
pub struct History {
    pub parent: ModalId,
    pub paths: Vec<PathBuf>,
    pub cursor: usize,
    pub offset: usize,
    pub rows: usize,
}
impl History {
    pub fn step(&mut self, delta: isize) {
        self.cursor = self
            .cursor
            .saturating_add_signed(delta)
            .min(self.paths.len().saturating_sub(1));
        self.keep_visible();
    }
    pub fn keep_visible(&mut self) {
        crate::list_state::ListViewport::fit(
            &mut self.cursor,
            &mut self.offset,
            self.rows,
            None,
            Some(self.paths.len().saturating_sub(self.rows)),
        );
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Field,
    Refuse,
    Skip,
    CheckSpace,
    Verify,
    RemoveSources,
    History,
    Tree,
}
pub const FOCUS: [usize; 11] = [0, 10, 1, 2, 3, 4, 5, 6, 7, 8, 9];
pub fn parent(app: &App) -> Option<ModalId> {
    app.modals
        .infos()
        .find(|m| m.focus == crate::focus::Focus::Prompt)
        .map(|m| m.id)
}
pub fn pick(app: &mut App, owner: ModalId, path: PathBuf) {
    if parent(app) != Some(owner) {
        return;
    }
    if let Some(prompt) = app.modals.prompt_mut() {
        if let Some(copy) = &mut prompt.copy {
            prompt.field = crate::filesystem::display_name(&path.to_string_lossy());
            prompt.cursor = prompt.field.len();
            prompt.focus = 0;
            copy.picked = Some(path);
        }
    }
}
pub fn action(app: &mut App, action: Action) {
    let Some(owner) = parent(app) else {
        return;
    };
    let Some(prompt) = app.modals.prompt_mut() else {
        return;
    };
    let moving = matches!(
        prompt.kind,
        crate::dialog::PromptKind::Operation(crate::operations::Kind::Move)
    );
    let Some(copy) = &mut prompt.copy else {
        return;
    };
    match action {
        Action::Field => prompt.focus = 0,
        Action::Refuse => {
            copy.options.existing = ExistingDestination::Refuse;
            prompt.focus = 1;
        }
        Action::Skip => {
            copy.options.existing = ExistingDestination::Skip;
            prompt.focus = 2;
        }
        Action::CheckSpace => {
            copy.options.check_free_space ^= true;
            prompt.focus = 3;
        }
        Action::Verify => {
            if !copy.options.remove_sources {
                copy.options.verify_writes ^= true;
                prompt.focus = 4;
            }
        }
        Action::RemoveSources => {
            if moving {
                return;
            }
            copy.options.remove_sources ^= true;
            if copy.options.remove_sources {
                copy.options.verify_writes = true;
            }
            prompt.focus = 5;
        }
        Action::History => {
            prompt.focus = 10;
            let paths = app.copy_dialog.history.clone();
            app.modals.set_copy_history(Some(History {
                parent: owner,
                paths,
                cursor: 0,
                offset: 0,
                rows: 6,
            }));
        }
        Action::Tree => {
            prompt.focus = 8;
            let path = prompt.base.clone();
            match crate::directory_tree::Tree::new(app.active, path, app.config.show_hidden) {
                Ok(mut tree) => {
                    tree.destination_prompt = Some(owner);
                    app.modals.set_directory_tree(Some(tree));
                }
                Err(e) => app.error(e.to_string()),
            }
        }
    }
}
pub fn key(app: &mut App, key: KeyEvent) {
    if key.code == K::Esc {
        app.modals.set_prompt(None);
        return;
    }
    if app.utilities.size.0 < 80 || app.utilities.size.1 < 25 {
        return;
    }
    let Some(prompt) = app.modals.prompt() else {
        return;
    };
    let focus = prompt.focus;
    if key.code == K::F(1) {
        crate::dialog::prompt_help(app);
        return;
    }
    if key.code == K::F(4) {
        action(app, Action::History);
        return;
    }
    let proposal =
        crate::ui::copy_spec(app, crate::turbo::dialog_spec::input_area(app)).and_then(|s| {
            if key.modifiers.contains(M::ALT) {
                if let K::Char(c) = key.code {
                    s.mnemonic(c).map(|id| s.activate(id))
                } else {
                    None
                }
            } else {
                s.key(key)
            }
        });
    if let Some(p) = proposal {
        crate::dialog_controls::apply(app, p, |app, target| match Some(target) {
            Some(Target::SubmitPrompt) => app.submit_prompt(),
            Some(Target::CancelPrompt) => app.modals.set_prompt(None),
            Some(Target::PromptHelp) => crate::dialog::prompt_help(app),
            Some(Target::Copy(a)) => action(app, a),
            Some(Target::PromptField) => action(app, Action::Field),
            _ => {}
        });
        return;
    }
    if key.modifiers.contains(M::ALT) {
        return;
    }
    if (focus == 1 || focus == 2) && matches!(key.code, K::Up | K::Down | K::Left | K::Right) {
        action(
            app,
            if focus == 1 {
                Action::Skip
            } else {
                Action::Refuse
            },
        );
        return;
    }
    if focus == 0 && !key.modifiers.intersects(M::CONTROL | M::SUPER) {
        app.modals.prompt_mut().unwrap().edit(key.code);
    }
}
pub fn history_key(app: &mut App, key: KeyEvent) {
    if crate::dialog_controls::key(app, key, crate::focus::Focus::CopyHistory) {
        return;
    }
    match key.code {
        K::Esc => app.modals.set_copy_history(None),
        code => {
            if let Some(h) = app.modals.copy_history_mut() {
                match code {
                    K::Up => h.step(-1),
                    K::Down => h.step(1),
                    K::PageUp => h.step(-(h.rows as isize)),
                    K::PageDown => h.step(h.rows as isize),
                    K::Home => h.step(isize::MIN),
                    K::End => h.step(isize::MAX),
                    _ => {}
                }
            }
        }
    }
}
pub fn history_mouse(app: &mut App, event: MouseEvent, target: Option<Target>) {
    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => match target {
            Some(Target::CloseCopyHistory) => app.modals.set_copy_history(None),
            Some(Target::CopyHistory(row)) => {
                select_history(app, row);
            }
            _ => {}
        },
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
            if let Some(h) = app.modals.copy_history_mut() {
                h.step(if event.kind == MouseEventKind::ScrollUp {
                    -3
                } else {
                    3
                });
            }
        }
        _ => {}
    }
}

pub(crate) fn select_history(app: &mut App, row: usize) {
    let selected = app
        .modals
        .copy_history()
        .and_then(|h| h.paths.get(row).cloned().map(|p| (h.parent, p)));
    app.modals.set_copy_history(None);
    if let Some((owner, path)) = selected {
        pick(app, owner, path);
    }
}
