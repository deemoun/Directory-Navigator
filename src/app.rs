use crate::{
    config::Config,
    dialog::{Dialog, Prompt, PromptKind},
    operations::{Event, Job, Kind, Plan},
    panel::Panel,
};
use std::{io, path::PathBuf};

pub struct App {
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
            layout_revision: 0,
            layout_state: None,
            layout_key: None,
            quit_review: None,
        };
        for stack in [
            &mut app.modals,
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

    pub(crate) fn with_document<T>(
        &mut self,
        id: crate::desktop::WindowId,
        run: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let previous = self.desktop.set_context(Some(id));
        let result = run(self);
        self.desktop.set_context(previous);
        result
    }
    pub fn approve_quit(&mut self, id: crate::desktop::WindowId) {
        if self.quit_review.is_none() {
            return;
        }
        if let Some(review) = &mut self.quit_review {
            review.approved.insert(id);
        }
        self.request_exit();
    }
    pub fn abort_quit(&mut self) {
        self.quit_review = None;
        for id in self
            .desktop
            .document_ids(crate::desktop::WindowKind::Editor)
        {
            if let Some(e) = self.desktop.editor_mut(id) {
                e.cancel_quit();
            }
        }
        self.exit_pending = false;
        self.quit = false;
    }
    pub fn push_dialog(&mut self, dialog: Dialog) {
        self.sync_modals();
        let target = self.desktop.target();
        let owner = if target == crate::desktop::WindowId::MANAGER {
            crate::modals::ModalOwner::Workspace
        } else {
            crate::modals::ModalOwner::Document(target)
        };
        self.push_owned_dialog(owner, dialog);
    }
    pub fn push_owned_dialog(&mut self, owner: crate::modals::ModalOwner, dialog: Dialog) {
        self.sync_modals();
        self.modals.push_message(owner, dialog);
        self.sync_modals();
    }
    pub fn dialogs(&self) -> Vec<&Dialog> {
        self.modals.messages(self.desktop.active().id)
    }
    pub fn last_dialog(&self) -> Option<&Dialog> {
        self.modals.last_message(self.desktop.active().id)
    }
    pub fn last_dialog_mut(&mut self) -> Option<&mut Dialog> {
        let id = self.desktop.active().id;
        self.modals.last_message_mut(id)
    }
    pub fn message_offset(&self) -> usize {
        self.modals.message_offset(self.desktop.active().id)
    }
    pub fn set_message_offset(&mut self, offset: usize) {
        self.modals
            .set_message_offset(self.desktop.active().id, offset);
    }
    pub fn pop_dialog(&mut self) -> Option<Dialog> {
        self.modals.pop_message(self.desktop.active().id)
    }
    pub fn clear_dialogs(&mut self) {
        self.modals.clear_messages(self.desktop.active().id);
    }
    pub fn modal_infos(&self) -> Vec<crate::modals::ModalInfo> {
        let mut infos = Vec::new();
        for stack in [
            &self.modals,
            &self.welcome.modals,
            &self.screensaver.modals,
            &self.utilities.modals,
            &self.command_line.modals,
            &self.user_menu.modals,
            &self.discovery.modals,
            &self.file_tools.modals,
        ] {
            infos.extend(stack.infos());
        }
        for window in self.desktop.windows() {
            if let Some(editor) = self.desktop.editor(window.id) {
                infos.extend(editor.modals.infos());
            }
            if let Some(viewer) = self.desktop.viewer(window.id) {
                infos.extend(viewer.modals.infos());
            }
        }
        infos.sort_by_key(|m| m.id);
        infos
    }
    pub fn visible_modals(&self) -> Vec<crate::modals::ModalInfo> {
        let active = self.desktop.active().id;
        self.modal_infos()
            .into_iter()
            .filter(|m| {
                m.owner == crate::modals::ModalOwner::Workspace
                    || m.owner == crate::modals::ModalOwner::Document(active)
            })
            .collect()
    }
    pub fn modal_top(&self) -> Option<crate::modals::ModalInfo> {
        self.visible_modals().last().copied()
    }
    fn visit_modals_mut(&mut self, mut visit: impl FnMut(&mut crate::modals::ModalStack)) {
        for stack in [
            &mut self.modals,
            &mut self.welcome.modals,
            &mut self.screensaver.modals,
            &mut self.utilities.modals,
            &mut self.command_line.modals,
            &mut self.user_menu.modals,
            &mut self.discovery.modals,
            &mut self.file_tools.modals,
        ] {
            visit(stack);
        }
        for id in self
            .desktop
            .document_ids(crate::desktop::WindowKind::Editor)
        {
            if let Some(e) = self.desktop.editor_mut(id) {
                visit(&mut e.modals);
            }
        }
        for id in self
            .desktop
            .document_ids(crate::desktop::WindowKind::Viewer)
        {
            if let Some(v) = self.desktop.viewer_mut(id) {
                visit(&mut v.modals);
            }
        }
    }
    pub fn sync_modals(&mut self) {
        for stack in [
            &mut self.screensaver.modals,
            &mut self.utilities.modals,
            &mut self.command_line.modals,
            &mut self.user_menu.modals,
            &mut self.discovery.modals,
            &mut self.file_tools.modals,
        ] {
            stack.bind(crate::desktop::WindowId::MANAGER);
        }
        use crate::modals::ModalOwner;
        if self.job.is_some() && self.modals.job_progress().is_none() {
            self.modals
                .set_job_progress(Some(crate::modals::JobProgress));
        }
        if self.job.is_none() {
            self.modals.set_job_progress(None);
        }
        for info in self.modal_infos() {
            if let ModalOwner::Document(id) = info.owner {
                if !self.desktop.contains(id) {
                    self.visit_modals_mut(|s| s.retire(info.owner));
                }
            }
        }
        loop {
            let live = self.modal_infos().iter().map(|m| m.id).collect();
            let mut removed = false;
            self.visit_modals_mut(|s| removed |= s.prune(&live));
            if !removed {
                break;
            }
        }
        crate::plugins::host::reconcile(self);
        let active = self.desktop.active().id;
        let infos = self.modal_infos();
        for info in &infos {
            let parent = infos
                .iter()
                .filter(|p| {
                    p.id < info.id
                        && (p.owner == info.owner
                            || (p.owner == ModalOwner::Workspace
                                && info.owner == ModalOwner::Document(active))
                            || (info.owner == ModalOwner::Workspace
                                && p.owner == ModalOwner::Document(active)))
                })
                .max_by_key(|p| p.id)
                .map(|p| p.id);
            let return_document = match info.owner {
                ModalOwner::Workspace => active,
                ModalOwner::Document(id) => id,
            };
            self.visit_modals_mut(|s| s.assign_parent(info.id, parent, return_document));
        }
        if let Some(previous) = self.last_modal {
            if !infos.iter().any(|m| m.id == previous.id) {
                if let Some(parent) = previous
                    .parent
                    .and_then(|id| infos.iter().find(|m| m.id == id))
                {
                    if self.modal_top().is_some_and(|top| top.id == parent.id) {
                        if let Some(id) = parent
                            .return_document
                            .filter(|id| self.desktop.contains(*id))
                        {
                            self.desktop.activate(id);
                        }
                    }
                }
            }
        }
        self.last_modal = self.modal_top();
        self.visit_modals_mut(|s| s.finish_transaction());
    }
    pub fn poll(&mut self) -> bool {
        let associations_changed = crate::associations::poll(self);
        let archive_changed = crate::archive::poll(self);
        let user_menu_changed = crate::user_menu::poll(self);
        let tree_changed = self.modals.directory_tree_mut().is_some_and(|t| t.poll());
        let discovery_changed = crate::discovery::poll(self);
        let opened = self.launcher.poll();
        let launch_changed = !opened.is_empty();
        for result in opened {
            if result
                .token
                .is_some_and(|token| !self.panels[result.panel].scope.accepts(token))
            {
                continue;
            }
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
        let mut editor_changed = self.desktop.poll_identities();
        let ids = self
            .desktop
            .document_ids(crate::desktop::WindowKind::Editor);
        for id in ids {
            if !self.desktop.identity_pending(id) {
                editor_changed |= self.with_document(id, crate::editor::poll);
            }
        }
        let mut changed = crate::config_import::poll(self);
        changed |= crate::plugins::host::poll(self, std::time::Instant::now());
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
            changed |= tick
                && !matches!(
                    self.desktop.active().kind,
                    crate::desktop::WindowKind::Plugin(_)
                );
        }
        if let Some(info) = self.modals.file_info_mut() {
            changed |= info.poll();
        }
        for id in self
            .desktop
            .document_ids(crate::desktop::WindowKind::Viewer)
        {
            if self.desktop.identity_pending(id) {
                continue;
            }
            if let Some(result) = self.desktop.viewer_mut(id).and_then(|viewer| viewer.poll()) {
                changed = true;
                match result {
                    Ok(()) => {
                        if let Some(viewer) = self.desktop.viewer(id) {
                            self.file_tools.histories.record(
                                crate::file_tools::history::HistoryKind::View,
                                viewer.path.clone(),
                            );
                        }
                    }
                    Err(message) => {
                        if self.desktop.active().id == id {
                            self.desktop.close(id);
                            self.error(message);
                        } else {
                            if let Some(viewer) = self.desktop.viewer_mut(id) {
                                viewer.contents = Some(crate::viewer::from_bytes(
                                    Vec::new(),
                                    crate::viewer::ViewMode::Text,
                                ));
                            }
                            self.with_document(id, |app| app.error(message));
                        }
                    }
                }
            }
        }
        let refresh_allowed =
            crate::focus::current(self) == crate::focus::Focus::Manager && !self.screensaver.active;
        let mut panel_errors = Vec::new();
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
                panel_errors.push(Dialog {
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
        if self.modals.quick_search().is_some_and(|search| {
            search.panel != self.active
                || self.panels[search.panel].loading
                || self.panels[search.panel].path != search.path
        }) {
            self.modals.set_quick_search(None);
            changed = true;
        }
        let mut finished = None;
        let mut operation_focus = None;
        for error in panel_errors {
            self.push_owned_dialog(
                crate::modals::ModalOwner::Document(crate::desktop::WindowId::MANAGER),
                error,
            );
        }

        if let Some(job) = &mut self.job {
            loop {
                match job.try_recv() {
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
            let moved = self.job.as_ref().is_some_and(|job| job.kind == Kind::Move);
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
                    if progress.skipped > 0 {
                        self.notice
                            .push_str(&format!(" · {} skipped", progress.skipped));
                        if !self.exit_pending {
                            self.push_owned_dialog(
                                crate::modals::ModalOwner::Workspace,
                                Dialog {
                                    title: if moved {
                                        "Move complete"
                                    } else {
                                        "Copy complete"
                                    },
                                    message: format!(
                                        "{}: {}\n{}: {}\n{}",
                                        self.config.language.text(if moved {
                                            "Moved items"
                                        } else {
                                            "Copied items"
                                        }),
                                        progress.files,
                                        self.config.language.text("Skipped existing items"),
                                        progress.skipped,
                                        self.config.language.text(
                                            "Skipped sources and existing destinations were kept."
                                        )
                                    ),
                                },
                            );
                        }
                    }
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
        self.sync_modals();
        changed | crate::windowing::poll(self)
    }

    pub fn wait_duration(&self, now: std::time::Instant) -> std::time::Duration {
        use std::time::Duration;
        if let Some(wait) = crate::plugins::host::next_wake(self, now) {
            return wait.min(Duration::from_millis(50));
        }
        let pending = self
            .discovery
            .modals
            .discovery()
            .is_some_and(|m| m.job.is_some())
            || (matches!(
                crate::focus::current(self),
                crate::focus::Focus::Manager | crate::focus::Focus::QuickSearch
            ) && self.quick_view.as_ref().is_some_and(|q| q.pending()))
            || self.modals.config_import().is_some_and(|p| p.loading())
            || self.modals.directory_tree().is_some_and(|t| t.loading())
            || crate::user_menu::pending(&self.user_menu)
            || self.associations.busy()
            || self.modals.archive().is_some_and(|a| a.loading())
            || self.launcher.loading()
            || self.job.is_some()
            || self.file_tools.busy()
            || self.panels.iter().any(|p| {
                p.loading
                    || (p.background_refreshing()
                        && crate::focus::current(self) == crate::focus::Focus::Manager)
            })
            || self
                .desktop
                .document_ids(crate::desktop::WindowKind::Editor)
                .iter()
                .any(|id| {
                    self.desktop.identity_pending(*id)
                        || self.desktop.editor(*id).is_some_and(|e| e.busy())
                })
            || self
                .desktop
                .document_ids(crate::desktop::WindowKind::Viewer)
                .iter()
                .any(|id| {
                    self.desktop.identity_pending(*id)
                        || self
                            .desktop
                            .viewer(*id)
                            .is_some_and(|v| v.contents.is_none())
                })
            || self
                .modals
                .file_info()
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

    pub fn request_exit(&mut self) {
        self.sync_modals();
        self.modals.set_window_list(None);
        if self.quit_review.is_none() {
            self.quit_review = Some(QuitReview {
                approved: Default::default(),
            });
        }
        let next = self
            .desktop
            .document_ids(crate::desktop::WindowKind::Editor)
            .into_iter()
            .rev()
            .find(|id| !self.quit_review.as_ref().unwrap().approved.contains(id));
        if let Some(id) = next {
            self.desktop.activate(id);
            self.exit_pending = false;
            self.with_document(id, |app| {
                crate::editor::request_close(app, crate::editor::CloseIntent::Quit)
            });
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
        if let Some(entry) = panel.entries.get(panel.cursor) {
            match crate::file_info::InfoDialog::open(entry.path.clone()) {
                Ok(info) => self.modals.set_file_info(Some(info)),
                Err(error) => self.error(format!("Cannot read file information: {error}")),
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
        if mode == crate::viewer::ViewMode::Auto {
            if let Some(id) = self
                .desktop
                .find_path(crate::desktop::WindowKind::Viewer, &entry.path)
            {
                self.desktop.activate(id);
                return;
            }
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
            let token = panel.scope.start("external-open");
            match self
                .launcher
                .request_owned(self.active, entry.path.clone(), token)
            {
                Ok(()) => self.notice = "Opening…".into(),
                Err(error) => self.error(error.to_string()),
            }
        }
    }

    pub fn apply_config(&mut self, config: Config) {
        self.shortcuts.clear();
        let hidden_changed = self.config.show_hidden != config.show_hidden;
        self.config = config;
        crate::plugins::host::apply_enabled(self);
        self.clock.tick_with(self.config.datetime);
        for id in self
            .desktop
            .document_ids(crate::desktop::WindowKind::Editor)
        {
            if let Some(editor) = self.desktop.editor_mut(id) {
                editor.apply_preferences(self.config.editor);
            }
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
        let mut settings = crate::settings::Settings::new(kind, &self.config);
        if kind == crate::settings::SettingsKind::Plugins {
            settings.plugins = self
                .desktop
                .plugins
                .definitions()
                .map(|d| (d.id, d.title))
                .collect();
        }
        self.modals.set_settings(Some(settings));
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
                self.abort_quit();
                self.error(format!("Cannot save settings: {error}\n\nExit cancelled; your preferences remain in this session.\nRetry Save desktop, or turn off Save settings on exit\nin System Setup to exit without saving."));
                false
            }
        }
    }
}
