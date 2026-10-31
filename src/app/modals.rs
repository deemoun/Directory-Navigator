//! Owned modal reconciliation and parent focus restoration.
use super::App;
use crate::dialog::Dialog;

impl App {
    pub fn push_dialog(&mut self, dialog: Dialog) {
        if dialog.title.to_ascii_lowercase().contains("help") && !crate::windowing::blocked(self) {
            let source = self.desktop.target();
            crate::tool_windows::open(self, crate::tool_windows::Kind::Help, None);
            if self
                .desktop
                .tool(self.desktop.target())
                .is_some_and(|t| t.kind == crate::tool_windows::Kind::Help)
            {
                let id = self.desktop.target();
                self.desktop.tool_mut(id).unwrap().help_owner =
                    (source != crate::desktop::WindowId::MANAGER && source != id).then_some(source);
                self.desktop.tool_mut(id).unwrap().set_help(dialog);
                self.sync_modals();
                return;
            }
        }

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
        if let Some(h) = self.help_document() {
            vec![&h.dialog]
        } else {
            self.modals.messages(self.desktop.target())
        }
    }
    pub fn last_dialog(&self) -> Option<&Dialog> {
        self.help_document()
            .map(|h| &h.dialog)
            .or_else(|| self.modals.last_message(self.desktop.target()))
    }
    pub fn last_dialog_mut(&mut self) -> Option<&mut Dialog> {
        let id = self.desktop.target();
        if self.help_document().is_some() {
            Some(&mut self.help_document_mut()?.dialog)
        } else {
            self.modals.last_message_mut(id)
        }
    }
    pub fn message_offset(&self) -> usize {
        self.help_document().map_or_else(
            || self.modals.message_offset(self.desktop.target()),
            |h| h.offset,
        )
    }
    pub fn set_message_offset(&mut self, offset: usize) {
        let id = self.desktop.target();
        if let Some(h) = self.help_document_mut() {
            h.offset = offset;
        } else {
            self.modals.set_message_offset(id, offset);
        }
    }
    pub fn pop_dialog(&mut self) -> Option<Dialog> {
        if crate::transfers::quit_dialog(self) {
            let result = self.modals.pop_message(self.desktop.target());
            self.abort_quit();
            return result;
        }
        let id = self.desktop.target();
        if self.help_document().is_some() {
            self.desktop
                .tool_mut(id)?
                .state
                .help
                .take()
                .map(|h| h.dialog)
        } else {
            self.modals.pop_message(id)
        }
    }
    pub fn clear_dialogs(&mut self) {
        if self.transfers.quit_prompt.is_some() {
            self.abort_quit();
        }
        if self.help_document().is_some() {
            self.pop_dialog();
        } else {
            self.modals.clear_messages(self.desktop.target());
        }
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
        crate::windowing::reconcile_activation(self);
        crate::tool_windows::reconcile(self);
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
        if self.blocking_operation() && self.modals.job_progress().is_none() {
            self.modals
                .set_job_progress(Some(crate::modals::JobProgress));
        }
        if !self.blocking_operation() {
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
                            crate::windowing::restore_focus(self, id);
                        }
                    }
                }
            }
        }
        self.last_modal = self.modal_top();
        self.visit_modals_mut(|s| s.finish_transaction());
        crate::windowing::reconcile_activation(self);
        crate::retirement::reconcile(self);
    }
}
