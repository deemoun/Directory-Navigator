//! Explicit, cancellable DOS preference import. No desktop objects or commands are loaded.
pub mod dos;
use crate::{
    app::App,
    dialog::{Prompt, PromptKind},
    mouse::Target,
    task::Task,
};
use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use std::{io, path::PathBuf, sync::mpsc};

pub struct Preview {
    pub(crate) requests: crate::requests::RequestScope,
    pub path: PathBuf,
    baseline: crate::config::Config,
    pub result: Option<Result<dos::Report, String>>,
    pub offset: usize,
    pub horizontal: u16,
    pub focus: usize,
    pending: Option<Task<Result<dos::Report, String>>>,
}
impl Preview {
    pub fn loading(&self) -> bool {
        self.pending.is_some()
    }
    pub fn can_apply(&self) -> bool {
        matches!(&self.result, Some(Ok(report)) if report.changed(&self.baseline))
    }
    pub fn lines(&self, language: crate::localization::Language) -> Vec<String> {
        let config = &self.baseline;
        let tr = |text| language.message(text);
        let mut lines = vec![crate::filesystem::display_name(
            &self.path.to_string_lossy(),
        )];
        match &self.result {
            None => lines.push(tr("Reading DOS settings…")),
            Some(Err(error)) => lines.push(format!("{}: {error}", tr("Import error"))),
            Some(Ok(report)) => {
                let draft = report.draft(config);
                lines.push(tr("Setting: current → imported"));
                for setting in &report.settings {
                    lines.push(format!(
                        "{}: {} → {}",
                        tr(setting.label()),
                        setting.value(config),
                        setting.value(&draft)
                    ));
                }
                if !report.changed(&self.baseline) {
                    lines.push(tr("No settings changes available."));
                }
                lines.push(String::new());
                lines.push(tr("Skipped settings"));
                for skipped in &report.skipped {
                    lines.push(if let Some((message, detail)) = skipped.split_once(": ") {
                        format!("{}: {detail}", tr(message))
                    } else {
                        tr(skipped)
                    });
                }
                lines.push(tr(
                    "DOS paths, palettes, commands and desktop windows are not imported.",
                ));
                lines.push(tr(
                    "Trash, confirmations and unrelated preferences stay unchanged.",
                ));
                lines.push(tr(
                    "Apply changes this session; normal settings saving persists them.",
                ));
            }
        }
        lines
    }
    fn step(&mut self, config: &crate::config::Config, amount: isize) {
        self.offset = self
            .offset
            .saturating_add_signed(amount)
            .min(self.lines(config.language).len().saturating_sub(1));
    }
}
pub fn open(app: &mut App) {
    app.modals.set_prompt(Some(Prompt {
        kind: PromptKind::ImportDos,
        sources: Vec::new(),
        base: app.panels[app.active].path.clone(),
        field: "DN.CFG".into(),
        cursor: 6,
        focus: 0,
    }));
}
pub fn start(app: &mut App, path: PathBuf) -> io::Result<()> {
    let source = path.clone();
    let requests = crate::requests::RequestScope::default();
    let pending = requests.spawn("dn-import-settings", move |cancel| {
        dos::read(&source, &cancel).map_err(|e| e.to_string())
    })?;
    let mut baseline = app.config.clone();
    baseline.sorts = [app.panels[0].sort, app.panels[1].sort];
    app.modals.set_config_import(Some(Preview {
        requests,
        baseline,
        path,
        result: None,
        offset: 0,
        horizontal: 0,
        focus: 1,
        pending: Some(pending),
    }));
    Ok(())
}
pub fn poll(app: &mut App) -> bool {
    let Some(preview) = app.modals.config_import_mut() else {
        return false;
    };
    let Some(task) = &preview.pending else {
        return false;
    };
    let result = match task.try_recv() {
        Ok(result) => result,
        Err(mpsc::TryRecvError::Empty) => return false,
        Err(mpsc::TryRecvError::Disconnected) => Err("Import worker stopped.".into()),
    };
    preview.pending = None;
    preview.result = Some(result);
    preview.focus = usize::from(!preview.can_apply());
    true
}
pub fn apply(app: &mut App) {
    if !app.modals.config_import().is_some_and(|p| p.can_apply()) {
        return;
    }
    if let Some(Preview {
        result: Some(Ok(report)),
        ..
    }) = app.modals.take_config_import()
    {
        let mut current = app.config.clone();
        current.sorts = [app.panels[0].sort, app.panels[1].sort];
        app.apply_config(report.draft(&current));
        app.notice = app
            .config
            .language
            .message("DOS settings applied for this session.");
    }
}
pub fn key(app: &mut App, key: KeyEvent) {
    let Some(preview) = app.modals.config_import_mut() else {
        return;
    };
    match key.code {
        KeyCode::Esc => app.modals.set_config_import(None),
        KeyCode::Tab | KeyCode::BackTab => {
            if preview.can_apply() {
                preview.focus =
                    crate::control_focus::cycle(preview.focus, 2, key.code == KeyCode::BackTab);
            }
        }
        KeyCode::Enter if preview.focus == 0 => apply(app),
        KeyCode::Enter => app.modals.set_config_import(None),
        KeyCode::Up => preview.step(&app.config, -1),
        KeyCode::Down => preview.step(&app.config, 1),
        KeyCode::PageUp => preview.step(&app.config, -8),
        KeyCode::PageDown => preview.step(&app.config, 8),
        KeyCode::Home => {
            preview.offset = 0;
            preview.horizontal = 0;
        }
        KeyCode::End => preview.step(&app.config, isize::MAX),
        KeyCode::Left => preview.horizontal = preview.horizontal.saturating_sub(8),
        KeyCode::Right => preview.horizontal = preview.horizontal.saturating_add(8),
        _ => {}
    }
}
pub fn mouse(app: &mut App, event: MouseEvent, target: Option<Target>) {
    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => match target {
            Some(Target::ApplyImport) => apply(app),
            Some(Target::CancelImport) => app.modals.set_config_import(None),
            _ => {}
        },
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
            if let Some(preview) = app.modals.config_import_mut() {
                preview.step(
                    &app.config,
                    if event.kind == MouseEventKind::ScrollUp {
                        -3
                    } else {
                        3
                    },
                );
            }
        }
        _ => {}
    }
}
