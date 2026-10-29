//! Owned modal reconciliation and parent focus restoration.
use super::App;
use crate::dialog::Dialog;

impl App {
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
    pub(crate) fn visit_modals_mut(
        &mut self,
        mut visit: impl FnMut(&mut crate::modals::ModalStack),
    ) {
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
        crate::retirement::documents(self);
        loop {
            let live = self.modal_infos().iter().map(|m| m.id).collect();
            let mut removed = false;
            self.visit_modals_mut(|s| removed |= s.prune(&live));
            if !removed {
                break;
            }
        }
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
        crate::retirement::reconcile(self);
    }
}
