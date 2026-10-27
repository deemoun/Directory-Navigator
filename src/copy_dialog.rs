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
        self.history.retain(|p| p != &path);
        self.history.insert(0, path);
        self.history.truncate(16);
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
        if self.cursor < self.offset {
            self.offset = self.cursor;
        } else if self.cursor >= self.offset + self.rows {
            self.offset = self.cursor + 1 - self.rows;
        }
        self.offset = self.offset.min(self.paths.len().saturating_sub(self.rows));
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
    let moving = matches!(
        prompt.kind,
        crate::dialog::PromptKind::Operation(crate::operations::Kind::Move)
    );
    let forced_verification = prompt
        .copy
        .as_ref()
        .is_some_and(|c| c.options.remove_sources);
    if key.code == K::F(1) {
        crate::dialog::prompt_help(app);
        return;
    }
    if key.code == K::F(4) {
        action(app, Action::History);
        return;
    }
    if key.modifiers.contains(M::ALT) {
        if let K::Char(c) = key.code {
            let language = app.config.language;
            if let Some((_, command)) = [
                ("~R~efuse existing destinations", Action::Refuse),
                ("~S~kip all existing files", Action::Skip),
                ("Check ~f~ree disk space", Action::CheckSpace),
                ("~V~erify disk writes", Action::Verify),
                ("Remove so~u~rce files", Action::RemoveSources),
                ("~T~ree", Action::Tree),
            ]
            .into_iter()
            .find(|(label, _)| language.matches(label, c))
            {
                action(app, command);
            } else if language.matches("~H~elp", c) {
                crate::dialog::prompt_help(app);
            } else if language.matches("~C~ancel", c) {
                app.modals.set_prompt(None);
            } else if language.matches("O~K~", c) {
                app.submit_prompt();
            } else if c.eq_ignore_ascii_case(&'d') {
                action(app, Action::Field);
            }
        }
        return;
    }
    if matches!(key.code, K::Tab | K::BackTab) {
        let choices = FOCUS
            .iter()
            .copied()
            .filter(|id| (*id != 4 || !forced_verification) && (*id != 5 || !moving))
            .collect::<Vec<_>>();
        let index = choices.iter().position(|v| *v == focus).unwrap_or(0);
        let reverse = key.code == K::BackTab || key.modifiers.contains(M::SHIFT);
        let index = crate::control_focus::cycle(index, choices.len(), reverse);
        app.modals.prompt_mut().unwrap().focus = choices[index];
        return;
    }
    if key.code == K::Enter {
        match focus {
            7 => app.modals.set_prompt(None),
            8 => action(app, Action::Tree),
            9 => crate::dialog::prompt_help(app),
            10 => action(app, Action::History),
            _ => app.submit_prompt(),
        };
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
    if key.code == K::Char(' ') && focus != 0 {
        match focus {
            1 => action(app, Action::Refuse),
            2 => action(app, Action::Skip),
            3 => action(app, Action::CheckSpace),
            4 => action(app, Action::Verify),
            5 => action(app, Action::RemoveSources),
            6 => app.submit_prompt(),
            7 => app.modals.set_prompt(None),
            8 => action(app, Action::Tree),
            9 => crate::dialog::prompt_help(app),
            10 => action(app, Action::History),
            _ => {}
        };
        return;
    }
    if focus == 0 && !key.modifiers.intersects(M::CONTROL | M::SUPER) {
        app.modals.prompt_mut().unwrap().edit(key.code);
    }
}
pub fn history_key(app: &mut App, key: KeyEvent) {
    match key.code {
        K::Esc => app.modals.set_copy_history(None),
        K::Enter => {
            let selected = app
                .modals
                .copy_history()
                .and_then(|h| h.paths.get(h.cursor).cloned().map(|p| (h.parent, p)));
            app.modals.set_copy_history(None);
            if let Some((owner, path)) = selected {
                pick(app, owner, path);
            }
        }
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
                let selected = app
                    .modals
                    .copy_history()
                    .and_then(|h| h.paths.get(row).cloned().map(|p| (h.parent, p)));
                app.modals.set_copy_history(None);
                if let Some((owner, path)) = selected {
                    pick(app, owner, path);
                }
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
