//! Document opening and scoped document access.
use super::App;

impl App {
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
}
