//! Typed document payloads. Modal stacks contain only blocking children.
use crate::{desktop::WindowId, dialog::Dialog, requests::Owner};
#[derive(Default)]
pub(crate) struct State {
    owner: Option<WindowId>,
    pub started: bool,
    pub utility: Option<crate::utilities::Kind>,
    pub help: Option<Help>,
    directory_tree: Option<crate::directory_tree::Tree>,
    discovery: Option<crate::discovery::Modal>,
    archive: Option<crate::archive::Browser>,
    file_info: Option<crate::file_info::InfoDialog>,
    navigation_dialog: Option<crate::navigation_tools::Modal>,
    file_tools: Option<crate::file_tools::ToolDialog>,
    command_history: Option<crate::command_line::history::HistoryDialog>,
}
pub(crate) struct Help {
    pub dialog: Dialog,
    pub offset: usize,
    pub browser: crate::turbo::help::Browser,
}
impl Help {
    pub fn new(dialog: Dialog) -> Self {
        let browser = crate::turbo::help::Browser::new(dialog.title, &dialog.message);
        Self {
            dialog,
            offset: 0,
            browser,
        }
    }
}
impl State {
    pub fn bind(&mut self, id: WindowId) {
        self.owner = Some(id);
        self.bind_requests();
    }
    fn bind_requests(&mut self) {
        let Some(id) = self.owner else {
            return;
        };
        let owner = Owner::Document(id);
        if let Some(s) = &mut self.directory_tree {
            s.requests.bind(owner);
        }
        if let Some(s) = &mut self.discovery {
            s.requests.bind(owner);
            if let Some(job) = &mut s.job {
                job.bind(owner);
            }
        }
        if let Some(s) = &mut self.archive {
            s.requests.bind(owner);
        }
        if let Some(s) = &mut self.file_info {
            s.requests.bind(owner);
        }
    }
    pub fn alive(&self) -> bool {
        self.utility.is_some()
            || self.help.is_some()
            || self.directory_tree.is_some()
            || self.discovery.is_some()
            || self.archive.is_some()
            || self.file_info.is_some()
            || self.navigation_dialog.is_some()
            || self.file_tools.is_some()
            || self.command_history.is_some()
    }
    pub fn directory_tree(&self) -> Option<&crate::directory_tree::Tree> {
        self.directory_tree.as_ref()
    }
    pub fn directory_tree_mut(&mut self) -> Option<&mut crate::directory_tree::Tree> {
        self.directory_tree.as_mut()
    }
    pub fn take_directory_tree(&mut self) -> Option<crate::directory_tree::Tree> {
        self.directory_tree.take()
    }
    pub fn set_directory_tree(&mut self, value: Option<crate::directory_tree::Tree>) {
        self.started |= value.is_some();
        self.directory_tree = value;
        self.bind_requests();
    }
    pub fn discovery(&self) -> Option<&crate::discovery::Modal> {
        self.discovery.as_ref()
    }
    pub fn discovery_mut(&mut self) -> Option<&mut crate::discovery::Modal> {
        self.discovery.as_mut()
    }
    pub fn take_discovery(&mut self) -> Option<crate::discovery::Modal> {
        self.discovery.take()
    }
    pub fn set_discovery(&mut self, value: Option<crate::discovery::Modal>) {
        self.started |= value.is_some();
        self.discovery = value;
        self.bind_requests();
    }
    pub fn archive(&self) -> Option<&crate::archive::Browser> {
        self.archive.as_ref()
    }
    pub fn archive_mut(&mut self) -> Option<&mut crate::archive::Browser> {
        self.archive.as_mut()
    }
    pub fn take_archive(&mut self) -> Option<crate::archive::Browser> {
        self.archive.take()
    }
    pub fn set_archive(&mut self, value: Option<crate::archive::Browser>) {
        self.started |= value.is_some();
        self.archive = value;
        self.bind_requests();
    }
    pub fn file_info(&self) -> Option<&crate::file_info::InfoDialog> {
        self.file_info.as_ref()
    }
    pub fn file_info_mut(&mut self) -> Option<&mut crate::file_info::InfoDialog> {
        self.file_info.as_mut()
    }
    pub fn take_file_info(&mut self) -> Option<crate::file_info::InfoDialog> {
        self.file_info.take()
    }
    pub fn set_file_info(&mut self, value: Option<crate::file_info::InfoDialog>) {
        self.started |= value.is_some();
        self.file_info = value;
        self.bind_requests();
    }
    pub fn navigation_dialog(&self) -> Option<&crate::navigation_tools::Modal> {
        self.navigation_dialog.as_ref()
    }
    pub fn navigation_dialog_mut(&mut self) -> Option<&mut crate::navigation_tools::Modal> {
        self.navigation_dialog.as_mut()
    }
    pub fn take_navigation_dialog(&mut self) -> Option<crate::navigation_tools::Modal> {
        self.navigation_dialog.take()
    }
    pub fn set_navigation_dialog(&mut self, value: Option<crate::navigation_tools::Modal>) {
        self.started |= value.is_some();
        self.navigation_dialog = value;
        self.bind_requests();
    }
    pub fn file_tools(&self) -> Option<&crate::file_tools::ToolDialog> {
        self.file_tools.as_ref()
    }
    pub fn file_tools_mut(&mut self) -> Option<&mut crate::file_tools::ToolDialog> {
        self.file_tools.as_mut()
    }
    pub fn take_file_tools(&mut self) -> Option<crate::file_tools::ToolDialog> {
        self.file_tools.take()
    }
    pub fn set_file_tools(&mut self, value: Option<crate::file_tools::ToolDialog>) {
        self.started |= value.is_some();
        self.file_tools = value;
        self.bind_requests();
    }
    pub fn command_history(&self) -> Option<&crate::command_line::history::HistoryDialog> {
        self.command_history.as_ref()
    }
    pub fn command_history_mut(
        &mut self,
    ) -> Option<&mut crate::command_line::history::HistoryDialog> {
        self.command_history.as_mut()
    }
    pub fn take_command_history(&mut self) -> Option<crate::command_line::history::HistoryDialog> {
        self.command_history.take()
    }
    pub fn set_command_history(
        &mut self,
        value: Option<crate::command_line::history::HistoryDialog>,
    ) {
        self.started |= value.is_some();
        self.command_history = value;
        self.bind_requests();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typed_root_requests_bind_to_document_and_retire_with_payload() -> std::io::Result<()> {
        struct Temp(std::path::PathBuf);
        impl Drop for Temp {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let temp = Temp(std::env::temp_dir().join(format!("dn-typed-root-{}", std::process::id())));
        std::fs::create_dir(&temp.0)?;
        let mut state = State::default();
        state.bind(WindowId::MANAGER);
        state.set_directory_tree(Some(crate::directory_tree::Tree::new(
            0,
            temp.0.clone(),
            false,
        )?));
        let tree = state.directory_tree().unwrap();
        assert_eq!(tree.requests.owner(), Owner::Document(WindowId::MANAGER));
        let lease = tree.requests.lease("late");
        let tree = state.take_directory_tree().unwrap();
        assert!(lease.current());
        state.set_directory_tree(Some(tree));
        assert!(lease.current());
        state.set_directory_tree(None);
        assert!(!lease.current());
        assert!(state.started && !state.alive());
        Ok(())
    }
}
