use crate::{
    config::Config,
    dialog::{Dialog, Prompt, PromptKind},
    operations::{Job, Kind, Plan},
    panel::Panel,
};
use std::{io, path::PathBuf};

mod configuration;
mod documents;
mod lifecycle;
mod modals;
mod polling;

pub struct App {
    pub(crate) button_feedback: crate::button_feedback::State,
    pub modals: crate::modals::ModalStack,
    pub welcome: crate::welcome::State,
    pub copy_dialog: crate::copy_dialog::State,
    pub output_visible: bool,
    pub shortcuts: crate::shortcuts::State,
    pub associations: crate::associations::State,
    pub user_menu: crate::user_menu::State,
    pub discovery: crate::discovery::Discovery,
    pub quick_view: Option<crate::quick_view::QuickView>,
    pub launcher: crate::launcher::Launcher,
    pub screensaver: crate::screensaver::State,
    pub clock: crate::clock_view::Clock,
    pub viewer_search: crate::viewer::SearchMemory,
    pub utilities: crate::utilities::Utilities,
    pub file_tools: crate::file_tools::Tools,
    pub editor_clipboard: String,
    pub editor_recovery: Option<crate::editor::recovery::Manager>,
    pub editor_search: crate::editor::search::SearchMemory,
    pub desktop: crate::desktop::Desktop,
    pub panels: [Panel; 2],
    pub active: usize,
    pub command_line: crate::command_line::CommandLine,
    pub config: Config,
    pub config_store: crate::config::Store,
    pub quit: bool,
    pub page_rows: usize,
    pub mouse: crate::mouse::MouseState,
    pub job: Option<Job>,
    pub last_operation:
        Option<Result<crate::operations::Progress, crate::operations::OperationFailure>>,
    pub notice: String,
    pub exit_pending: bool,
    last_modal: Option<crate::modals::ModalInfo>,
    pub(crate) retirement: crate::retirement::State,
    pub(crate) activation: crate::windowing::ActivationState,
    pub layout_revision: u64,
    pub(crate) layout_state: Option<crate::layout::ViewportState>,
    pub layout_key: Option<(
        crate::desktop::WindowId,
        crate::presentation::layout::Rect,
        crate::presentation::layout::Rect,
    )>,
    pub quit_review: Option<QuitReview>,
}

pub struct QuitReview {
    pub approved: std::collections::BTreeSet<crate::desktop::WindowId>,
}
impl App {
    pub fn new(path: PathBuf) -> io::Result<Self> {
        let config = Config::default();
        let mut app = Self {
            button_feedback: Default::default(),
            modals: Default::default(),
            welcome: Default::default(),
            copy_dialog: Default::default(),
            output_visible: false,
            shortcuts: Default::default(),
            associations: Default::default(),
            user_menu: Default::default(),
            discovery: Default::default(),
            quick_view: None,
            launcher: Default::default(),
            clock: Default::default(),
            screensaver: Default::default(),
            viewer_search: Default::default(),
            utilities: crate::utilities::Utilities::default(),
            file_tools: crate::file_tools::Tools::default(),
            editor_clipboard: String::new(),
            editor_recovery: None,
            editor_search: crate::editor::search::SearchMemory::default(),
            desktop: Default::default(),
            panels: [
                Panel::new_for(0, path.clone(), config.show_hidden)?,
                Panel::new_for(1, path, config.show_hidden)?,
            ],
            active: 0,
            command_line: Default::default(),
            config,
            config_store: crate::config::Store::new(Config::path()),
            quit: false,
            page_rows: 1,
            mouse: crate::mouse::MouseState::default(),
            job: None,
            last_operation: None,
            notice: "Ready — Ins marks files · F1 help".into(),
            exit_pending: false,
            last_modal: None,
            retirement: Default::default(),
            activation: Default::default(),
            layout_revision: 0,
            layout_state: None,
            layout_key: None,
            quit_review: None,
        };
        for stack in [
            &mut app.screensaver.modals,
            &mut app.utilities.modals,
            &mut app.command_line.modals,
            &mut app.user_menu.modals,
            &mut app.discovery.modals,
            &mut app.file_tools.modals,
        ] {
            stack.bind(crate::desktop::WindowId::MANAGER);
        }
        Ok(app)
    }

    pub fn error(&mut self, message: impl Into<String>) {
        self.push_dialog(Dialog {
            title: " Directory Navigator — error ",
            message: message.into(),
        });
    }

    pub fn refresh(&mut self) {
        for panel in &mut self.panels {
            panel.refresh(self.config.show_hidden);
            panel.space.refresh();
        }
    }

    pub fn prompt_operation(&mut self, kind: Kind, rename_here: bool) {
        let kind = if kind == Kind::Delete && self.config.use_trash {
            Kind::Trash
        } else {
            kind
        };
        if self.job.is_some() || self.panels[self.active].loading {
            return;
        }
        let sources = if kind == Kind::Mkdir {
            Vec::new()
        } else {
            self.panels[self.active].targets()
        };
        if kind != Kind::Mkdir && sources.is_empty() {
            self.error(if self.panels[self.active].only_filtered_marks() {
                "All marked entries are filtered out. Clear the filter or marks first."
            } else {
                "Select a file or directory first (the parent entry cannot be changed)."
            });
            return;
        }
        let base = self.panels[if matches!(kind, Kind::Mkdir | Kind::Delete | Kind::Trash)
            || rename_here
        {
            self.active
        } else {
            self.active ^ 1
        }]
        .path
        .clone();
        self.modals.set_prompt(Some(Prompt {
            kind: PromptKind::Operation(kind),
            copy: matches!(kind, Kind::Copy | Kind::Move).then(crate::copy_dialog::Draft::default),
            sources,
            base,
            field: String::new(),
            cursor: 0,
            focus: if matches!(kind, Kind::Delete | Kind::Trash) {
                1
            } else {
                0
            },
        }));
    }

    pub fn prompt_goto(&mut self) {
        self.modals.set_prompt(Some(Prompt {
            kind: PromptKind::GoTo,
            copy: None,
            sources: Vec::new(),
            base: self.panels[self.active].path.clone(),
            field: String::new(),
            cursor: 0,
            focus: 0,
        }));
    }

    pub fn open_pattern(&mut self, selecting: bool) {
        if self.job.is_none() && !self.panels[self.active].loading {
            self.modals
                .set_pattern(Some(crate::pattern::PatternDialog::new(
                    self.active,
                    self.panels[self.active].path.clone(),
                    selecting,
                )));
        }
    }

    pub fn open_file_info(&mut self) {
        let panel = &self.panels[self.active];
        if self.job.is_some() || panel.loading {
            return;
        }
        if let Some(path) = panel.entries.get(panel.cursor).map(|e| e.path.clone()) {
            if !crate::tool_windows::open(
                self,
                crate::tool_windows::Kind::FileInfo,
                Some(path.clone()),
            ) {
                return;
            }
            match crate::file_info::InfoDialog::open(path) {
                Ok(info) => self.set_file_info(Some(info)),
                Err(error) => crate::tool_windows::failed(
                    self,
                    format!("Cannot read file information: {error}"),
                ),
            }
        }
    }

    pub fn open_columns(&mut self) {
        self.modals
            .set_columns_dialog(Some(crate::columns::ColumnDialog::new(
                self.config.columns,
                self.active,
            )));
    }

    pub fn submit_prompt(&mut self) {
        let Some(prompt) = self.modals.take_prompt() else {
            return;
        };
        if matches!(prompt.kind, PromptKind::Operation(Kind::Mkdir)) && prompt.field.is_empty() {
            self.error("Enter a new directory name.");
            return;
        }
        let destination = if let Some(path) = prompt.copy.as_ref().and_then(|c| c.picked.clone()) {
            path
        } else if prompt.field.is_empty() {
            prompt.base
        } else {
            prompt.base.join(&prompt.field)
        };
        match prompt.kind {
            PromptKind::ImportDos => {
                if let Err(error) = crate::config_import::start(self, destination) {
                    self.error(error.to_string());
                }
            }
            PromptKind::GoTo => {
                self.panels[self.active].go_to(destination, self.config.show_hidden)
            }
            PromptKind::Operation(kind @ (Kind::Copy | Kind::Move)) => {
                let options = prompt
                    .copy
                    .as_ref()
                    .map_or_else(Default::default, |c| c.options);
                let plan = Plan {
                    kind,
                    sources: prompt.sources,
                    destination: destination.clone(),
                };
                match Job::start_with_options(plan, options) {
                    Ok(job) => {
                        self.copy_dialog.record(destination);
                        self.job = Some(job);
                    }
                    Err(error) => self.error(error.to_string()),
                }
            }
            PromptKind::Operation(kind) => match Job::start(Plan {
                kind,
                sources: prompt.sources,
                destination,
            }) {
                Ok(job) => self.job = Some(job),
                Err(error) => self.error(error.to_string()),
            },
        }
    }

    pub fn toggle_hidden(&mut self) {
        self.config.show_hidden = !self.config.show_hidden;
        self.refresh();
    }
}
