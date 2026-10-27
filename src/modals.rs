//! Owned, typed modal entries. Payloads live in their workspace/document host;
//! monotonically allocated IDs provide one ordering across nested hosts.
use crate::{desktop::WindowId, focus::Focus};
use std::{
    any::{Any, TypeId},
    sync::atomic::{AtomicU64, Ordering},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ModalId(pub u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ModalOwner {
    Workspace,
    Document(WindowId),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModalInfo {
    pub id: ModalId,
    pub owner: ModalOwner,
    pub parent: Option<ModalId>,
    pub return_document: Option<WindowId>,
    pub focus: Focus,
}
struct MessageState {
    dialog: crate::dialog::Dialog,
    offset: usize,
    help: Option<crate::turbo::help::Browser>,
}
struct Entry {
    info: ModalInfo,
    state: Box<dyn Any>,
}
pub struct ModalStack {
    owner: ModalOwner,
    entries: Vec<Entry>,
    detached: std::collections::HashMap<TypeId, ModalInfo>,
}
impl Default for ModalStack {
    fn default() -> Self {
        Self {
            owner: ModalOwner::Workspace,
            entries: Vec::new(),
            detached: Default::default(),
        }
    }
}
impl ModalStack {
    pub fn bind(&mut self, id: WindowId) {
        self.owner = ModalOwner::Document(id);
        for entry in &mut self.entries {
            entry.info.owner = self.owner;
        }
    }
    pub fn infos(&self) -> impl Iterator<Item = ModalInfo> + '_ {
        self.entries.iter().map(|e| e.info)
    }
    pub fn top(&self, active: WindowId) -> Option<ModalInfo> {
        self.infos()
            .filter(|e| e.owner == ModalOwner::Workspace || e.owner == ModalOwner::Document(active))
            .max_by_key(|e| e.id)
    }
    fn get<T: 'static>(&self) -> Option<&T> {
        self.entries
            .iter()
            .rev()
            .find_map(|e| e.state.downcast_ref())
    }
    fn get_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.entries
            .iter_mut()
            .rev()
            .find_map(|e| e.state.downcast_mut())
    }
    fn take<T: 'static>(&mut self) -> Option<T> {
        let i = self.entries.iter().rposition(|e| e.state.is::<T>())?;
        let entry = self.entries.remove(i);
        self.detached.insert(TypeId::of::<T>(), entry.info);
        self.remove_children(entry.info.id);
        entry.state.downcast::<T>().ok().map(|value| *value)
    }
    fn remove_children(&mut self, parent: ModalId) {
        let children = self
            .entries
            .iter()
            .filter(|e| e.info.parent == Some(parent))
            .map(|e| e.info.id)
            .collect::<Vec<_>>();
        for child in children {
            self.remove_children(child);
            self.entries.retain(|e| e.info.id != child);
        }
    }
    pub fn retire(&mut self, owner: ModalOwner) {
        self.entries.retain(|e| e.info.owner != owner);
    }
    pub fn prune(&mut self, live: &std::collections::HashSet<ModalId>) -> bool {
        let n = self.entries.len();
        self.entries
            .retain(|e| e.info.parent.is_none_or(|id| live.contains(&id)));
        n != self.entries.len()
    }
    pub fn assign_parent(
        &mut self,
        id: ModalId,
        parent: Option<ModalId>,
        return_document: WindowId,
    ) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.info.id == id) {
            if e.info.return_document.is_none() {
                e.info.parent = parent;
                e.info.return_document = Some(return_document);
            }
        }
    }
    fn push<T: 'static>(&mut self, mut state: T, focus: Focus, owner: ModalOwner) {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let id = NEXT
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .expect("modal identities exhausted");
        bind_payload(&mut state, crate::requests::Owner::Modal(ModalId(id)));
        let parent = self
            .entries
            .iter()
            .rev()
            .find(|e| e.info.owner == owner)
            .map(|e| e.info.id);
        self.entries.push(Entry {
            info: ModalInfo {
                id: ModalId(id),
                owner,
                parent,
                return_document: None,
                focus,
            },
            state: Box::new(state),
        });
    }
    pub fn finish_transaction(&mut self) {
        self.detached.clear();
    }
    fn set<T: 'static>(&mut self, state: Option<T>, focus: Focus) {
        match state {
            None => {
                self.take::<T>();
                self.detached.remove(&TypeId::of::<T>());
            }
            Some(mut state) => {
                if let Some(info) = self.detached.remove(&TypeId::of::<T>()) {
                    bind_payload(&mut state, crate::requests::Owner::Modal(info.id));
                    self.entries.push(Entry {
                        info,
                        state: Box::new(state),
                    });
                    return;
                }
                if let Some(top) = self.entries.last_mut() {
                    if let Some(old) = top.state.downcast_mut::<T>() {
                        {
                            let id = top.info.id;
                            let mut state = state;
                            bind_payload(&mut state, crate::requests::Owner::Modal(id));
                            *old = state;
                            return;
                        }
                    }
                }
                let owner = if matches!(focus, Focus::Settings | Focus::WindowList | Focus::Job) {
                    ModalOwner::Workspace
                } else {
                    self.owner
                };
                self.push(state, focus, owner);
            }
        }
    }
    pub fn push_message(&mut self, owner: ModalOwner, dialog: crate::dialog::Dialog) {
        let help = dialog
            .title
            .to_ascii_lowercase()
            .contains("help")
            .then(|| crate::turbo::help::Browser::new(dialog.title, &dialog.message));
        self.push(
            MessageState {
                dialog,
                offset: 0,
                help,
            },
            Focus::Message,
            owner,
        );
    }
    fn message_state(&self, active: WindowId) -> Option<&MessageState> {
        self.entries
            .iter()
            .rev()
            .filter(|e| {
                e.info.owner == ModalOwner::Workspace
                    || e.info.owner == ModalOwner::Document(active)
            })
            .find_map(|e| e.state.downcast_ref())
    }
    fn message_state_mut(&mut self, active: WindowId) -> Option<&mut MessageState> {
        self.entries
            .iter_mut()
            .rev()
            .filter(|e| {
                e.info.owner == ModalOwner::Workspace
                    || e.info.owner == ModalOwner::Document(active)
            })
            .find_map(|e| e.state.downcast_mut())
    }
    pub fn native_help(&self, active: WindowId) -> Option<&crate::turbo::help::Browser> {
        self.message_state(active).and_then(|m| m.help.as_ref())
    }
    pub(crate) fn native_help_mut(
        &mut self,
        active: WindowId,
    ) -> Option<&mut crate::turbo::help::Browser> {
        self.message_state_mut(active).and_then(|m| m.help.as_mut())
    }
    pub fn message_offset(&self, active: WindowId) -> usize {
        self.message_state(active).map_or(0, |m| m.offset)
    }
    pub fn set_message_offset(&mut self, active: WindowId, offset: usize) {
        if let Some(message) = self.message_state_mut(active) {
            message.offset = offset;
        }
    }
    pub fn messages(&self, active: WindowId) -> Vec<&crate::dialog::Dialog> {
        self.entries
            .iter()
            .filter(|e| {
                e.info.owner == ModalOwner::Workspace
                    || e.info.owner == ModalOwner::Document(active)
            })
            .filter_map(|e| e.state.downcast_ref::<MessageState>().map(|m| &m.dialog))
            .collect()
    }
    pub fn last_message(&self, active: WindowId) -> Option<&crate::dialog::Dialog> {
        self.message_state(active).map(|m| &m.dialog)
    }
    pub fn last_message_mut(&mut self, active: WindowId) -> Option<&mut crate::dialog::Dialog> {
        self.message_state_mut(active).map(|m| &mut m.dialog)
    }
    pub fn pop_message(&mut self, active: WindowId) -> Option<crate::dialog::Dialog> {
        let i = self.entries.iter().rposition(|e| {
            (e.info.owner == ModalOwner::Workspace || e.info.owner == ModalOwner::Document(active))
                && e.state.is::<MessageState>()
        })?;
        let entry = self.entries.remove(i);
        self.remove_children(entry.info.id);
        entry
            .state
            .downcast::<MessageState>()
            .ok()
            .map(|m| m.dialog)
    }

    pub fn clear_messages(&mut self, active: WindowId) {
        while self.pop_message(active).is_some() {}
    }
}
macro_rules! slot {
    ($get:ident,$get_mut:ident,$take:ident,$set:ident,$ty:ty,$focus:ident) => {
        impl ModalStack {
            pub fn $get(&self) -> Option<&$ty> {
                self.get()
            }
            pub fn $get_mut(&mut self) -> Option<&mut $ty> {
                self.get_mut()
            }
            pub fn $take(&mut self) -> Option<$ty> {
                self.take()
            }
            pub fn $set(&mut self, value: Option<$ty>) {
                self.set(value, Focus::$focus);
            }
        }
    };
}
macro_rules! copy_slot {
    ($get:ident,$get_mut:ident,$take:ident,$set:ident,$ty:ty,$focus:ident) => {
        impl ModalStack {
            pub fn $get(&self) -> Option<$ty> {
                self.get::<$ty>().copied()
            }
            pub fn $get_mut(&mut self) -> Option<&mut $ty> {
                self.get_mut()
            }
            pub fn $take(&mut self) -> Option<$ty> {
                self.take()
            }
            pub fn $set(&mut self, value: Option<$ty>) {
                self.set(value, Focus::$focus);
            }
        }
    };
}
slot!(
    config_import,
    config_import_mut,
    take_config_import,
    set_config_import,
    crate::config_import::Preview,
    ConfigImport
);
slot!(
    archive,
    archive_mut,
    take_archive,
    set_archive,
    crate::archive::Browser,
    Archive
);
slot!(
    directory_tree,
    directory_tree_mut,
    take_directory_tree,
    set_directory_tree,
    crate::directory_tree::Tree,
    Tree
);
slot!(
    color_dialog,
    color_dialog_mut,
    take_color_dialog,
    set_color_dialog,
    crate::palette::ColorDialog,
    Colors
);
slot!(
    navigation_dialog,
    navigation_dialog_mut,
    take_navigation_dialog,
    set_navigation_dialog,
    crate::navigation_tools::Modal,
    Navigation
);
slot!(
    window_list,
    window_list_mut,
    take_window_list,
    set_window_list,
    crate::windowing::WindowList,
    WindowList
);
slot!(
    settings,
    settings_mut,
    take_settings,
    set_settings,
    crate::settings::Settings,
    Settings
);
slot!(
    pattern,
    pattern_mut,
    take_pattern,
    set_pattern,
    crate::pattern::PatternDialog,
    Pattern
);
slot!(
    file_info,
    file_info_mut,
    take_file_info,
    set_file_info,
    crate::file_info::InfoDialog,
    FileInfo
);
slot!(
    columns_dialog,
    columns_dialog_mut,
    take_columns_dialog,
    set_columns_dialog,
    crate::columns::ColumnDialog,
    Columns
);
slot!(
    prompt,
    prompt_mut,
    take_prompt,
    set_prompt,
    crate::dialog::Prompt,
    Prompt
);
slot!(
    quick_search,
    quick_search_mut,
    take_quick_search,
    set_quick_search,
    crate::quick_search::QuickSearch,
    QuickSearch
);
struct MenuState {
    root: (usize, usize),
    submenus: Vec<(crate::menu::MenuId, usize)>,
}
impl ModalStack {
    pub fn menu(&self) -> Option<(usize, usize)> {
        self.get::<MenuState>().map(|m| m.root)
    }
    pub fn menu_mut(&mut self) -> Option<&mut (usize, usize)> {
        self.get_mut::<MenuState>().map(|m| &mut m.root)
    }
    pub fn set_menu(&mut self, value: Option<(usize, usize)>) {
        match value {
            Some(root) => {
                if let Some(state) = self.get_mut::<MenuState>() {
                    state.root = root;
                } else {
                    self.set(
                        Some(MenuState {
                            root,
                            submenus: Vec::new(),
                        }),
                        Focus::Menu,
                    );
                }
            }
            None => self.set::<MenuState>(None, Focus::Menu),
        }
    }
    pub fn submenus(&self) -> &[(crate::menu::MenuId, usize)] {
        self.get::<MenuState>()
            .map_or(&[], |m| m.submenus.as_slice())
    }
    pub fn submenus_mut(&mut self) -> &mut Vec<(crate::menu::MenuId, usize)> {
        &mut self
            .get_mut::<MenuState>()
            .expect("open parent menu")
            .submenus
    }
}
slot!(
    welcome,
    welcome_mut,
    take_welcome,
    set_welcome,
    crate::welcome::Welcome,
    Welcome
);
slot!(
    screensaver,
    screensaver_mut,
    take_screensaver,
    set_screensaver,
    crate::screensaver::Setup,
    SaverSetup
);
copy_slot!(
    utility_dialog,
    utility_dialog_mut,
    take_utility_dialog,
    set_utility_dialog,
    crate::utilities::Kind,
    Utility
);
slot!(
    command_history,
    command_history_mut,
    take_command_history,
    set_command_history,
    crate::command_line::history::HistoryDialog,
    CommandHistory
);
slot!(
    user_menu,
    user_menu_mut,
    take_user_menu,
    set_user_menu,
    crate::user_menu::Modal,
    UserMenu
);
slot!(
    discovery,
    discovery_mut,
    take_discovery,
    set_discovery,
    crate::discovery::Modal,
    Discovery
);
slot!(
    file_tools,
    file_tools_mut,
    take_file_tools,
    set_file_tools,
    crate::file_tools::ToolDialog,
    FileTools
);
impl ModalStack {
    pub fn editor_modal(&self) -> Option<&crate::editor::Modal> {
        self.get()
    }
    pub fn editor_modal_mut(&mut self) -> Option<&mut crate::editor::Modal> {
        self.get_mut()
    }
    pub fn take_editor_modal(&mut self) -> Option<crate::editor::Modal> {
        self.take()
    }
    pub fn set_editor_modal(&mut self, value: Option<crate::editor::Modal>) {
        if let Some(value) = value {
            if self
                .detached
                .contains_key(&TypeId::of::<crate::editor::Modal>())
            {
                self.set(Some(value), Focus::EditorModal);
                return;
            }
            if self
                .editor_modal()
                .is_some_and(|old| std::mem::discriminant(old) != std::mem::discriminant(&value))
            {
                self.push(value, Focus::EditorModal, self.owner);
            } else {
                self.set(Some(value), Focus::EditorModal);
            }
        } else {
            self.take::<crate::editor::Modal>();
            self.detached.remove(&TypeId::of::<crate::editor::Modal>());
        }
    }
}

copy_slot!(
    editor_menu,
    editor_menu_mut,
    take_editor_menu,
    set_editor_menu,
    (usize, usize),
    EditorMenu
);
slot!(
    viewer_search,
    viewer_search_mut,
    take_viewer_search,
    set_viewer_search,
    crate::viewer::SearchDialog,
    ViewerSearch
);

fn bind_payload(state: &mut dyn Any, owner: crate::requests::Owner) {
    if let Some(s) = state.downcast_mut::<crate::discovery::Modal>() {
        s.requests.bind(owner);
        if let Some(job) = &mut s.job {
            job.bind(owner);
        }
    }
    if let Some(s) = state.downcast_mut::<crate::archive::Browser>() {
        s.requests.bind(owner);
    }
    if let Some(s) = state.downcast_mut::<crate::config_import::Preview>() {
        s.requests.bind(owner);
    }
    if let Some(s) = state.downcast_mut::<crate::file_info::InfoDialog>() {
        s.requests.bind(owner);
    }
    if let Some(s) = state.downcast_mut::<crate::directory_tree::Tree>() {
        s.requests.bind(owner);
    }
}

pub struct JobProgress;
slot!(
    job_progress,
    job_progress_mut,
    take_job_progress,
    set_job_progress,
    JobProgress,
    Job
);

slot!(
    settings_choice,
    settings_choice_mut,
    take_settings_choice,
    set_settings_choice,
    crate::settings::ChoiceDialog,
    Settings
);

slot!(
    copy_history,
    copy_history_mut,
    take_copy_history,
    set_copy_history,
    crate::copy_dialog::History,
    CopyHistory
);
