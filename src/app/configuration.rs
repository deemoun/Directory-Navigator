//! Configuration loading, drafts and guarded publication.
use super::App;
use crate::config::Config;

impl App {
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
