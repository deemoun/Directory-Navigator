//! Configuration loading, drafts and guarded publication.
use super::App;
use crate::config::Config;

impl App {
    pub fn apply_config(&mut self, config: Config) {
        self.shortcuts.clear();
        if self.config.restore_session != config.restore_session {
            self.session.cancel_restore();
        }

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
        self.load_initial_config_with_session(false);
    }
    pub fn load_initial_config_with_session(&mut self, bypass: bool) {
        if std::mem::replace(&mut self.welcome.startup_checked, true) {
            return;
        }
        if self.read_config() {
            crate::session::begin(self, bypass);
            if self.config_store.is_new_installation() {
                crate::welcome::open(self);
            }
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
                self.error(self.settings_error("Cannot load settings.", &error, false));
                false
            }
        }
    }

    fn settings_error(
        &self,
        summary: &'static str,
        error: &std::io::Error,
        exiting: bool,
    ) -> String {
        use crate::error_report::{Outcome, Report};
        let advice = if exiting {
            "Exit cancelled. Retry Save desktop, or turn off Save settings on exit in System Setup to exit without saving."
        } else {
            "Check the configuration path and access permissions. If another DN instance changed settings, use Load desktop before saving again."
        };
        let mut report =
            Report::new(summary, error.to_string(), advice).outcome(Outcome::PreferencesRetained);
        if !exiting && error.kind() == std::io::ErrorKind::StorageFull {
            report = report.io(error.kind());
        }
        if let Ok(path) = self.config_store.path() {
            report = report.path("Configuration", path);
        }
        report.render(self.config.language)
    }
    fn save_session_config(&mut self, force: bool) -> std::io::Result<bool> {
        let mut config = self.config.clone();
        config.session = crate::session::capture(self)?;
        config.invalid_session = false;
        let changed = self.config_store.save(&config, force)?;
        self.config.session = config.session;
        self.config.invalid_session = false;
        Ok(changed)
    }
    pub fn save_config(&mut self) {
        self.config.sorts = [self.panels[0].sort, self.panels[1].sort];
        match self.save_session_config(true) {
            Ok(_) => {
                if let Ok(path) = self.config_store.path() {
                    self.notice = format!("Settings saved to {}", path.display());
                }
            }
            Err(error) => self.error(self.settings_error("Cannot save settings.", &error, false)),
        }
    }

    /// Called only after the existing unsaved-document / operation exit guards.
    pub fn save_settings_on_exit(&mut self) -> bool {
        if !self.config.save_on_exit {
            return true;
        }
        self.config.sorts = [self.panels[0].sort, self.panels[1].sort];
        match self.save_session_config(false) {
            Ok(_) => true,
            Err(error) => {
                self.abort_quit();
                self.error(self.settings_error("Cannot save settings.", &error, true));
                false
            }
        }
    }
}
