//! Guarded quit review and cancellation.
use super::{App, QuitReview};

impl App {
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
}
