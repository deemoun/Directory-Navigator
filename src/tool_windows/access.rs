//! Typed controller access, shared by document roots and their modal pickers.
use crate::{app::App, focus::Focus};
macro_rules! tool_access {
    ($get:ident, $get_mut:ident, $take:ident, $set:ident, $ty:ty, $focus:ident, $($host:ident).+) => {
        impl App {
            pub fn $get(&self) -> Option<&$ty> {
                if self.document_tool(Focus::$focus, self.$($host).+.$get().is_some()) {
                    self.desktop.tool(self.desktop.target())?.state.$get()
                } else { self.$($host).+.$get() }
            }
            pub fn $get_mut(&mut self) -> Option<&mut $ty> {
                let id = self.desktop.target();
                if self.document_tool(Focus::$focus, self.$($host).+.$get().is_some()) {
                    self.desktop.tool_mut(id)?.state.$get_mut()
                } else { self.$($host).+.$get_mut() }
            }
            pub fn $take(&mut self) -> Option<$ty> {
                let id = self.desktop.target();
                if self.document_tool(Focus::$focus, self.$($host).+.$get().is_some()) {
                    self.desktop.tool_mut(id)?.state.$take()
                } else { self.$($host).+.$take() }
            }
            pub fn $set(&mut self, value: Option<$ty>) {
                let id = self.desktop.target();
                if self.document_tool(Focus::$focus, self.$($host).+.$get().is_some()) {
                    self.desktop.tool_mut(id).unwrap().state.$set(value);
                } else { self.$($host).+.$set(value); }
            }
        }
    };
}
tool_access!(
    directory_tree,
    directory_tree_mut,
    take_directory_tree,
    set_directory_tree,
    crate::directory_tree::Tree,
    Tree,
    modals
);
tool_access!(
    discovery,
    discovery_mut,
    take_discovery,
    set_discovery,
    crate::discovery::Modal,
    Discovery,
    discovery.modals
);
tool_access!(
    archive,
    archive_mut,
    take_archive,
    set_archive,
    crate::archive::Browser,
    Archive,
    modals
);
tool_access!(
    file_info,
    file_info_mut,
    take_file_info,
    set_file_info,
    crate::file_info::InfoDialog,
    FileInfo,
    modals
);
tool_access!(
    navigation_dialog,
    navigation_dialog_mut,
    take_navigation_dialog,
    set_navigation_dialog,
    crate::navigation_tools::Modal,
    Navigation,
    modals
);
tool_access!(
    file_tools,
    file_tools_mut,
    take_file_tools,
    set_file_tools,
    crate::file_tools::ToolDialog,
    FileTools,
    file_tools.modals
);
tool_access!(
    command_history,
    command_history_mut,
    take_command_history,
    set_command_history,
    crate::command_line::history::HistoryDialog,
    CommandHistory,
    command_line.modals
);
