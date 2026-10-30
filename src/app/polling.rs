//! Ordered background completion delivery and wake scheduling.
use super::App;
use crate::{
    dialog::Dialog,
    operations::{Event, Kind},
};
use std::io;

impl App {
    pub fn poll(&mut self) -> bool {
        // Retire closed owners and orphaned children before polling any reply.
        self.sync_modals();
        let associations_changed = crate::associations::poll(self);
        let archive_changed = crate::archive::poll(self);
        let user_menu_changed = crate::user_menu::poll(self);
        let tree_changed = self.directory_tree_mut().is_some_and(|t| t.poll());
        let discovery_changed = crate::discovery::poll(self);
        let launch_changed = self.poll_launches();
        let saver_changed = crate::screensaver::poll(self, std::time::Instant::now());
        let utilities_changed = crate::utilities::poll(self);
        let tools_changed = crate::file_tools::poll(self);
        let editor_changed = self.poll_editors();
        let recovery_changed = crate::editor::recovery::poll(self, std::time::Instant::now());
        let mut changed = crate::config_import::poll(self);
        changed |= crate::plugins::host::poll(self, std::time::Instant::now());
        changed |= saver_changed
            | associations_changed
            | recovery_changed
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
        if let Some(info) = self.file_info_mut() {
            changed |= info.poll();
        }
        changed |= self.poll_viewers();
        let refresh_allowed =
            crate::focus::current(self) == crate::focus::Focus::Manager && !self.screensaver.active;
        let (panels_changed, panel_errors) = self.poll_panels(refresh_allowed);
        changed |= panels_changed;
        changed |= crate::quick_view::poll(self);
        if self.modals.quick_search().is_some_and(|search| {
            search.panel != self.active
                || self.panels[search.panel].loading
                || self.panels[search.panel].path != search.path
        }) {
            self.modals.set_quick_search(None);
            changed = true;
        }
        for error in panel_errors {
            self.push_owned_dialog(
                crate::modals::ModalOwner::Document(crate::desktop::WindowId::MANAGER),
                error,
            );
        }

        changed |= self.poll_operation();
        changed |= crate::tool_windows::poll(self);
        self.sync_modals();
        changed
            | crate::windowing::poll(self)
            | crate::button_feedback::poll(self, std::time::Instant::now())
    }

    pub fn wait_duration(&self, now: std::time::Instant) -> std::time::Duration {
        use std::time::Duration;
        if let Some(wait) = crate::button_feedback::next_wake(self, now) {
            return wait.min(Duration::from_millis(16));
        }
        if let Some(wait) = crate::plugins::host::next_wake(self, now) {
            return wait.min(Duration::from_millis(50));
        }
        let pending = self.editor_recovery.as_ref().is_some_and(|m| m.busy())
            || self
                .desktop
                .windows()
                .any(|w| matches!(w.kind, crate::desktop::WindowKind::Tool(_)))
            || self.discovery().is_some_and(|m| m.job.is_some())
            || (matches!(
                crate::focus::current(self),
                crate::focus::Focus::Manager | crate::focus::Focus::QuickSearch
            ) && self.quick_view.as_ref().is_some_and(|q| q.pending()))
            || self.modals.config_import().is_some_and(|p| p.loading())
            || self.directory_tree().is_some_and(|t| t.loading())
            || crate::user_menu::pending(&self.user_menu)
            || self.associations.busy()
            || self.archive().is_some_and(|a| a.loading())
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
            || self.file_info().is_some_and(|i| i.contents.is_none())
            || self
                .utility_state()
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

    fn poll_launches(&mut self) -> bool {
        let opened = self.launcher.poll();
        let changed = !opened.is_empty();
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
        changed
    }

    fn poll_editors(&mut self) -> bool {
        let mut changed = self.desktop.poll_identities();
        if self.desktop.retirement_pending() {
            self.sync_modals();
        }
        let ids = self
            .desktop
            .document_ids(crate::desktop::WindowKind::Editor);
        for id in ids {
            if !self.desktop.identity_pending(id) {
                changed |= self.with_document(id, crate::editor::poll);
            }
        }
        changed
    }

    fn poll_viewers(&mut self) -> bool {
        let mut changed = false;
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
                            self.retire_document(id);
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
        changed
    }

    fn poll_panels(&mut self, refresh_allowed: bool) -> (bool, Vec<Dialog>) {
        let mut changed = false;
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
        (changed, panel_errors)
    }

    fn poll_operation(&mut self) -> bool {
        let mut changed = false;
        let mut finished = None;
        let mut operation_focus = None;
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
        changed
    }
}
