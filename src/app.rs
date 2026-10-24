use crate::{
    config::Config,
    dialog::{Dialog, Prompt, PromptKind},
    operations::{Event, Job, Kind, Plan},
    panel::Panel,
};
use std::{io, path::PathBuf};

pub struct App {
    pub config_import: Option<crate::config_import::Preview>,
    pub welcome: crate::welcome::State,
    pub shortcuts: crate::shortcuts::State,
    pub associations: crate::associations::State,
    pub archive: Option<crate::archive::Browser>,
    pub user_menu: crate::user_menu::State,
    pub directory_tree: Option<crate::directory_tree::Tree>,
    pub discovery: crate::discovery::Discovery,
    pub quick_view: Option<crate::quick_view::QuickView>,
    pub launcher: crate::launcher::Launcher,
    pub screensaver: crate::screensaver::State,
    pub clock: crate::clock_view::Clock,
    pub color_dialog: Option<crate::palette::ColorDialog>,
    pub navigation_dialog: Option<crate::navigation_tools::Modal>,
    pub viewer_search: crate::viewer::SearchMemory,
    pub utilities: crate::utilities::Utilities,
    pub file_tools: crate::file_tools::Tools,
    pub editor_clipboard: String,
    pub editor_search: crate::editor::search::SearchMemory,
    pub desktop: crate::desktop::Desktop,
    pub window_list: Option<crate::windowing::WindowList>,
    pub game_scores: crate::tetris::scores::Store,
    pub panels: [Panel; 2],
    pub active: usize,
    pub command_line: crate::command_line::CommandLine,
    pub config: Config,
    pub config_store: crate::config::Store,
    pub dialogs: Vec<Dialog>,
    pub message_offset: usize,
    pub quit: bool,
    pub page_rows: usize,
    pub menu: Option<(usize, usize)>,
    pub submenus: Vec<(crate::menu::MenuId, usize)>,
    pub settings: Option<crate::settings::Settings>,
    pub pattern: Option<crate::pattern::PatternDialog>,
    pub file_info: Option<crate::file_info::InfoDialog>,
    pub quick_search: Option<crate::quick_search::QuickSearch>,
    pub columns_dialog: Option<crate::columns::ColumnDialog>,
    pub mouse: crate::mouse::MouseState,
    pub prompt: Option<Prompt>,
    pub job: Option<Job>,
    pub last_operation:
        Option<Result<crate::operations::Progress, crate::operations::OperationFailure>>,
    pub notice: String,
    pub exit_pending: bool,
}

impl App {
    pub fn new(path: PathBuf) -> io::Result<Self> {
        let config = Config::default();
        Ok(Self {
            config_import: None,
            welcome: Default::default(),
            shortcuts: Default::default(),
            associations: Default::default(),
            archive: None,
            user_menu: Default::default(),
            directory_tree: None,
            discovery: Default::default(),
            quick_view: None,
            launcher: Default::default(),
            clock: Default::default(),
            screensaver: Default::default(),
            color_dialog: None,
            navigation_dialog: None,
            viewer_search: Default::default(),
            utilities: crate::utilities::Utilities::default(),
            file_tools: crate::file_tools::Tools::default(),
            editor_clipboard: String::new(),
            editor_search: crate::editor::search::SearchMemory::default(),
            desktop: Default::default(),
            window_list: None,
            game_scores: Default::default(),
            panels: [
                Panel::new(path.clone(), config.show_hidden)?,
                Panel::new(path, config.show_hidden)?,
            ],
            active: 0,
            command_line: Default::default(),
            config,
            config_store: crate::config::Store::new(Config::path()),
            dialogs: Vec::new(),
            message_offset: 0,
            quit: false,
            page_rows: 1,
            menu: None,
            submenus: Vec::new(),
            settings: None,
            pattern: None,
            file_info: None,
            columns_dialog: None,
            quick_search: None,
            mouse: crate::mouse::MouseState::default(),
            prompt: None,
            job: None,
            last_operation: None,
            notice: "Ready — Ins marks files · F1 help".into(),
            exit_pending: false,
        })
    }

    pub fn show_editor(&mut self, editor: crate::editor::Editor) {
        if let Err(error) = self.desktop.open_editor(editor) {
            self.error(error.to_string());
        }
    }

    pub fn show_viewer(&mut self, viewer: crate::viewer::Viewer) {
        if let Err(error) = self.desktop.open_viewer(viewer) {
            self.error(error.to_string());
        }
    }

    pub fn show_game(&mut self, game: crate::tetris::Game) {
        if let Err(error) = self.desktop.open_game(game) {
            self.error(error.to_string());
        }
    }

    pub fn poll(&mut self) -> bool {
        let associations_changed = crate::associations::poll(self);
        let archive_changed = crate::archive::poll(self);
        let user_menu_changed = crate::user_menu::poll(self);
        let tree_changed = self.directory_tree.as_mut().is_some_and(|t| t.poll());
        let discovery_changed = crate::discovery::poll(self);
        let opened = self.launcher.poll();
        let launch_changed = !opened.is_empty();
        for result in opened {
            match result.result {
                Ok(crate::launcher::Launched::Directory) => {
                    self.panels[result.panel].go_to(result.path, self.config.show_hidden)
                }
                Ok(_) => {
                    self.notice = format!(
                        "Opened with default application: {}",
                        crate::filesystem::display_name(&result.path.to_string_lossy())
                    )
                }
                Err(error) => self.error(format!("Cannot open {}: {error}", result.path.display())),
            }
        }
        let saver_changed = crate::screensaver::poll(self, std::time::Instant::now());
        let utilities_changed = crate::utilities::poll(self);
        let tools_changed = crate::file_tools::poll(self);
        let editor_changed = crate::editor::poll(self);
        let mut changed = crate::config_import::poll(self);
        changed |= self
            .desktop
            .game_mut()
            .is_some_and(|game| game.tick(std::time::Instant::now()));
        changed |= saver_changed
            | associations_changed
            | editor_changed
            | tools_changed
            | utilities_changed
            | launch_changed
            | discovery_changed
            | tree_changed
            | user_menu_changed
            | archive_changed;
        if self.config.show_clock {
            let tick = self.clock.tick_with(self.config.datetime);
            changed |= tick && self.desktop.game().is_none();
        }
        if let Some(info) = &mut self.file_info {
            changed |= info.poll();
        }
        if let Some(result) = self.desktop.viewer_mut().and_then(|viewer| viewer.poll()) {
            changed = true;
            match result {
                Ok(()) => {
                    if let Some(viewer) = self.desktop.viewer() {
                        self.file_tools.histories.record(
                            crate::file_tools::history::HistoryKind::View,
                            viewer.path.clone(),
                        );
                    }
                }
                Err(message) => {
                    self.desktop.close_viewer();
                    self.error(message);
                }
            }
        }
        let refresh_allowed =
            crate::focus::current(self) == crate::focus::Focus::Manager && !self.screensaver.active;
        for panel in &mut self.panels {
            if panel.background_refreshing() && !refresh_allowed {
                continue;
            }
            let loading = panel.loading;
            let revision = panel.revision;
            let previous = panel.path.clone();
            let error = panel.poll();
            if loading
                && !panel.loading
                && error.is_none()
                && (previous != panel.path
                    || self
                        .file_tools
                        .histories
                        .get(crate::file_tools::history::HistoryKind::Directory)
                        .is_empty())
            {
                self.file_tools.histories.record(
                    crate::file_tools::history::HistoryKind::Directory,
                    panel.path.clone(),
                );
            }
            if let Some(message) = error {
                changed = true;
                self.dialogs.push(Dialog {
                    title: " Directory error ",
                    message,
                });
            }
            changed |= loading != panel.loading || revision != panel.revision;
            changed |= panel.space.poll(&panel.path, std::time::Instant::now());
            if refresh_allowed {
                panel.refresh_if_due(self.config.show_hidden, std::time::Instant::now());
            }
        }
        changed |= crate::quick_view::poll(self);
        if self.quick_search.as_ref().is_some_and(|search| {
            search.panel != self.active
                || self.panels[search.panel].loading
                || self.panels[search.panel].path != search.path
        }) {
            self.quick_search = None;
            changed = true;
        }
        let mut finished = None;
        let mut operation_focus = None;
        if let Some(job) = &mut self.job {
            loop {
                match job.events.try_recv() {
                    Ok(Event::Progress(progress)) => {
                        job.progress = progress;
                        changed = true;
                    }
                    Ok(Event::Done(result)) => {
                        operation_focus = job.focus.clone();
                        finished = Some(result);
                        break;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        finished = Some(Err(crate::operations::OperationFailure {kind:io::ErrorKind::BrokenPipe,progress:job.progress.clone(),message:"File operation worker stopped unexpectedly; check partial destinations.".into()}));
                        break;
                    }
                }
            }
        }
        if let Some(result) = finished {
            let trashed = self.job.as_ref().is_some_and(|job| job.kind == Kind::Trash);
            self.last_operation = Some(result.clone());
            self.job = None;
            changed = true;
            match result {
                Ok(progress) => {
                    self.notice = if trashed {
                        format!("Moved to Trash: {} item(s)", progress.files)
                    } else {
                        format!("Done: {} items, {} bytes", progress.files, progress.bytes)
                    };
                    self.panels[self.active].replace_visible_marks(&Default::default());
                    if let Some(focus) = operation_focus {
                        for panel in &mut self.panels {
                            if !panel.loading && focus.parent() == Some(panel.path.as_path()) {
                                panel.focus_after_refresh =
                                    focus.file_name().map(|name| name.to_os_string());
                            }
                        }
                    }
                }
                Err(message) => self.error(message.to_string()),
            }
            self.refresh();
            if self.exit_pending {
                self.request_exit();
            }
        }
        changed | crate::windowing::poll(self)
    }

    pub fn wait_duration(&self, now: std::time::Instant) -> std::time::Duration {
        use std::time::Duration;
        if let Some(game) = self.desktop.game() {
            return game.wait_duration(now);
        }
        let pending = self
            .discovery
            .modal
            .as_ref()
            .is_some_and(|m| m.job.is_some())
            || (matches!(
                crate::focus::current(self),
                crate::focus::Focus::Manager | crate::focus::Focus::QuickSearch
            ) && self.quick_view.as_ref().is_some_and(|q| q.pending()))
            || self.config_import.as_ref().is_some_and(|p| p.loading())
            || self.directory_tree.as_ref().is_some_and(|t| t.loading())
            || crate::user_menu::pending(&self.user_menu)
            || self.associations.busy()
            || self.archive.as_ref().is_some_and(|a| a.loading())
            || self.launcher.loading()
            || self.job.is_some()
            || self.file_tools.busy()
            || self.panels.iter().any(|p| {
                p.loading
                    || (p.background_refreshing()
                        && crate::focus::current(self) == crate::focus::Focus::Manager)
            })
            || self.desktop.editor().is_some_and(|e| e.busy())
            || self.desktop.viewer().is_some_and(|v| v.contents.is_none())
            || self
                .file_info
                .as_ref()
                .is_some_and(|i| i.contents.is_none())
            || self
                .utilities
                .information
                .as_ref()
                .is_some_and(|i| i.loading());
        if pending || self.screensaver.active {
            Duration::from_millis(50)
        } else if self.config.show_clock {
            Duration::from_millis(1000 - u64::from(chrono::Local::now().timestamp_subsec_millis()))
        } else {
            Duration::from_secs(1)
        }
    }

    pub fn error(&mut self, message: impl Into<String>) {
        self.dialogs.push(Dialog {
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

    pub fn request_exit(&mut self) {
        self.window_list = None;
        if let Some(id) = self.desktop.id(crate::desktop::WindowKind::Editor) {
            self.desktop.activate(id);
            // Re-entering this guard after a cancelled quit must be explicit.
            self.exit_pending = false;
            crate::editor::request_close(self, crate::editor::CloseIntent::Quit);
            return;
        }
        if self.file_tools.busy() {
            self.exit_pending = true;
            self.notice = "Finishing list export before exit…".into();
        } else if let Some(job) = &self.job {
            job.cancel();
            self.exit_pending = true;
            self.notice = "Cancelling operation before exit…".into();
        } else {
            self.quit = true;
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
        self.prompt = Some(Prompt {
            kind: PromptKind::Operation(kind),
            sources,
            base,
            field: String::new(),
            cursor: 0,
            focus: if matches!(kind, Kind::Delete | Kind::Trash) {
                1
            } else {
                0
            },
        });
    }

    pub fn prompt_goto(&mut self) {
        self.prompt = Some(Prompt {
            kind: PromptKind::GoTo,
            sources: Vec::new(),
            base: self.panels[self.active].path.clone(),
            field: String::new(),
            cursor: 0,
            focus: 0,
        });
    }

    pub fn open_pattern(&mut self, selecting: bool) {
        if self.job.is_none() && !self.panels[self.active].loading {
            self.pattern = Some(crate::pattern::PatternDialog::new(
                self.active,
                self.panels[self.active].path.clone(),
                selecting,
            ));
        }
    }

    pub fn open_file_info(&mut self) {
        let panel = &self.panels[self.active];
        if self.job.is_some() || panel.loading {
            return;
        }
        if let Some(entry) = panel.entries.get(panel.cursor) {
            match crate::file_info::InfoDialog::open(entry.path.clone()) {
                Ok(info) => self.file_info = Some(info),
                Err(error) => self.error(format!("Cannot read file information: {error}")),
            }
        }
    }

    pub fn open_columns(&mut self) {
        self.columns_dialog = Some(crate::columns::ColumnDialog::new(
            self.config.columns,
            self.active,
        ));
    }

    pub fn submit_prompt(&mut self) {
        let Some(prompt) = self.prompt.take() else {
            return;
        };
        if matches!(prompt.kind, PromptKind::Operation(Kind::Mkdir)) && prompt.field.is_empty() {
            self.error("Enter a new directory name.");
            return;
        }
        let destination = if prompt.field.is_empty() {
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

    pub fn open_viewer(&mut self) {
        self.open_viewer_mode(crate::viewer::ViewMode::Auto);
    }

    pub fn open_viewer_mode(&mut self, mode: crate::viewer::ViewMode) {
        if self.panels[self.active].loading {
            return;
        }
        let Some(entry) = self.panels[self.active]
            .entries
            .get(self.panels[self.active].cursor)
        else {
            return;
        };
        if entry.directory || entry.parent {
            return;
        }
        match crate::viewer::Viewer::open_mode(entry.path.clone(), mode) {
            Ok(viewer) => self.show_viewer(viewer),
            Err(error) => self.error(error.to_string()),
        }
    }

    pub fn open_current(&mut self) {
        self.open_current_with(true);
    }
    pub fn open_external_current(&mut self) {
        self.open_current_with(false);
    }
    fn open_current_with(&mut self, browse: bool) {
        let panel = &self.panels[self.active];
        if panel.loading {
            return;
        }
        if let Some(entry) = panel.entries.get(panel.cursor) {
            if entry.directory && !crate::launcher::is_app_bundle(&entry.path) {
                self.panels[self.active].enter(self.config.show_hidden);
                return;
            }
            if browse && !entry.directory && !entry.symlink {
                let path = entry.path.clone();
                if crate::associations::open(self, path) {
                    return;
                }
            }
            let panel = &self.panels[self.active];
            let Some(entry) = panel.entries.get(panel.cursor) else {
                return;
            };
            if browse
                && !entry.symlink
                && crate::archive::reader::Format::detect(&entry.path).is_some()
            {
                crate::archive::open(self, entry.path.clone());
                return;
            }
            match self.launcher.request(self.active, entry.path.clone()) {
                Ok(()) => self.notice = "Opening…".into(),
                Err(error) => self.error(error.to_string()),
            }
        }
    }

    pub fn apply_config(&mut self, config: Config) {
        self.shortcuts.clear();
        let hidden_changed = self.config.show_hidden != config.show_hidden;
        self.config = config;
        self.clock.tick_with(self.config.datetime);
        if let Some(editor) = self.desktop.editor_mut() {
            editor.apply_preferences(self.config.editor);
        }
        for (panel, sort) in self.panels.iter_mut().zip(self.config.sorts) {
            if panel.sort != sort {
                panel.set_sort(sort);
            }
        }
        if hidden_changed {
            self.refresh();
        }
    }

    pub fn open_settings(&mut self, kind: crate::settings::SettingsKind) {
        self.config.sorts = [self.panels[0].sort, self.panels[1].sort];
        self.settings = Some(crate::settings::Settings::new(kind, &self.config));
    }

    pub fn load_config(&mut self) {
        self.read_config();
    }

    pub fn load_initial_config(&mut self) {
        if std::mem::replace(&mut self.welcome.startup_checked, true) {
            return;
        }
        if self.read_config() && self.config_store.is_new_installation() {
            crate::welcome::open(self);
        }
    }

    fn read_config(&mut self) -> bool {
        crate::associations::reload(self);
        match self.config_store.load() {
            Ok(config) => {
                self.apply_config(config);
                self.notice = "Desktop preferences loaded".into();
                true
            }
            Err(error) => {
                self.error(format!("Cannot load settings: {error}"));
                false
            }
        }
    }

    pub fn save_config(&mut self) {
        self.config.sorts = [self.panels[0].sort, self.panels[1].sort];
        match self.config_store.save(&self.config, true) {
            Ok(_) => {
                if let Ok(path) = self.config_store.path() {
                    self.notice = format!("Settings saved to {}", path.display());
                }
            }
            Err(error) => self.error(format!("Cannot save settings: {error}")),
        }
    }

    /// Called only after the existing unsaved-document / operation exit guards.
    pub fn save_settings_on_exit(&mut self) -> bool {
        if !self.config.save_on_exit {
            return true;
        }
        self.config.sorts = [self.panels[0].sort, self.panels[1].sort];
        match self.config_store.save(&self.config, false) {
            Ok(_) => true,
            Err(error) => {
                self.quit = false;
                self.error(format!("Cannot save settings: {error}\n\nExit cancelled; your preferences remain in this session.\nRetry Save desktop, or turn off Save settings on exit\nin System Setup to exit without saving."));
                false
            }
        }
    }
}
