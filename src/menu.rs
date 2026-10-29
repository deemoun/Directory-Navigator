//! Structure and mnemonics follow original-dn/RESOURCE/ENGLISH/DN.DNR, dlgMainMenu.
use crate::{app::App, panel::SortMode, settings::SettingsKind, viewer::ViewMode};
pub mod definition;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
pub use definition::{MenuId, RootId};
mod navigation;
pub use navigation::{activate, close, key, open, open_mnemonic, open_root, open_submenu};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    TerminalOutput,
    ImportDosSettings,
    EditAssociations,
    ReloadAssociations,
    CommandHistory,
    CopyNames(bool),
    Colors,
    ScreensaverSetup,
    ScreenRest,
    Bookmarks,
    FilenameFilter,
    Utility(crate::utilities::Kind),
    History(crate::file_tools::history::HistoryKind),
    CompareDirectories,
    ExportList,
    Parent,
    Refresh,
    Key(u8),
    Disabled,
    Separator,
    Submenu(MenuId),
    Sort(SortMode),
    Layout(crate::panel_layout::PanelLayout),
    QuickSearch,
    Find,
    DirectorySizes,
    DirectoryTree,
    BrowseArchive,
    OpenExternal,
    UserMenu,
    EditUserMenu(bool),
    QuickView,
    Hidden,
    GoTo,
    SelectAll,
    ClearSelection,
    SelectGroup,
    UnselectGroup,
    FileInfo,
    Columns,
    InvertSelection,
    SwapPanels,
    WindowList,
    WindowMove,
    WindowZoom,
    SaveSettings,
    LoadSettings,
    About,
    Welcome,
    Plugin(crate::plugins::PluginId),
    Settings(SettingsKind),
    Confirmations,
    View(ViewMode),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Item {
    pub label: &'static str,
    pub shortcut: &'static str,
    pub action: Action,
}
impl Item {
    pub fn selectable(&self) -> bool {
        !matches!(self.action, Action::Disabled | Action::Separator)
    }
}
const fn item(label: &'static str, shortcut: &'static str, action: Action) -> Item {
    Item {
        label,
        shortcut,
        action,
    }
}
const fn line() -> Item {
    item("", "", Action::Separator)
}

/// Compatibility view derived from the hierarchy; never edit labels separately.
pub const LABELS: [&str; definition::ROOTS.len()] = {
    let mut labels = [""; definition::ROOTS.len()];
    let mut i = 0;
    while i < labels.len() {
        labels[i] = definition::ROOTS[i].label;
        i += 1;
    }
    labels
};
pub fn root_index(id: RootId) -> Option<usize> {
    definition::ROOTS.iter().position(|r| r.id == id)
}
pub fn items(index: usize) -> Vec<Item> {
    definition::ROOTS
        .get(index)
        .map_or_else(Vec::new, |r| r.entries.to_vec())
}
pub fn submenu_items(id: MenuId) -> Vec<Item> {
    definition::submenu_items(id)
}
pub(crate) fn plugin_placement(id: crate::plugins::PluginId) -> Option<(RootId, Item)> {
    definition::ROOTS.iter().find_map(|root| {
        root.entries
            .iter()
            .find(|i| i.action == Action::Plugin(id))
            .map(|i| (root.id, *i))
    })
}

/// Hide preferences affect the manager; the game never advertises its menus.
pub(crate) fn bar_visible(app: &App) -> bool {
    app.desktop.active().kind == crate::desktop::WindowKind::Manager
        && (!app.config.hide_menu || app.modals.menu().is_some())
}

pub fn plain(label: &str) -> String {
    label.replace('~', "")
}
pub fn mnemonic(label: &str) -> Option<char> {
    label
        .split_once('~')?
        .1
        .chars()
        .next()
        .and_then(|c| c.to_lowercase().next())
}

pub fn level_items(app: &App, depth: usize) -> Vec<Item> {
    if depth == 0 {
        app.modals
            .menu()
            .map(|(root, _)| {
                let mut entries = items(root);
                for d in app.desktop.plugins.definitions().filter(|d| {
                    definition::ROOTS
                        .get(root)
                        .is_some_and(|r| Some(r.id) == d.menu)
                }) {
                    if !entries.iter().any(|e| e.action == Action::Plugin(d.id)) {
                        entries.push(item(d.menu_label, d.shortcut, Action::Plugin(d.id)));
                    }
                }
                entries.retain(|entry| match entry.action {
                    Action::Plugin(_) => crate::commands::manager_available(app, entry.action),
                    _ => true,
                });
                entries
            })
            .unwrap_or_default()
    } else {
        app.modals
            .submenus()
            .get(depth - 1)
            .map(|(id, _)| submenu_items(*id))
            .unwrap_or_default()
    }
}

pub fn selected_row(app: &App, depth: usize) -> usize {
    if depth == 0 {
        app.modals.menu().map(|(_, row)| row).unwrap_or(0)
    } else {
        app.modals
            .submenus()
            .get(depth - 1)
            .map(|(_, row)| *row)
            .unwrap_or(0)
    }
}

pub fn execute(app: &mut App, action: Action) {
    if matches!(
        action,
        Action::Disabled | Action::Separator | Action::Submenu(_)
    ) {
        return;
    }
    close(app);
    app.modals.set_quick_search(None);
    match action {
        Action::TerminalOutput => crate::terminal_output::request(app),
        Action::CommandHistory => crate::command_line::history::open(app),
        Action::CopyNames(full) => crate::file_tools::clipboard::copy(app,full),
        Action::ScreensaverSetup => crate::screensaver::open(app),
        Action::ScreenRest => app.screensaver.start(std::time::Instant::now()),
        Action::Colors => crate::palette::open(app),
        Action::Bookmarks => crate::navigation_tools::open_bookmarks(app),
        Action::FilenameFilter => crate::navigation_tools::open_filter(app),
        Action::Utility(kind) => crate::utilities::open(app, kind),
        Action::History(kind) => crate::file_tools::open_history(app, kind),
        Action::CompareDirectories => crate::file_tools::open_compare(app),
        Action::ExportList => crate::file_tools::open_export(app),
        Action::Parent => app.panels[app.active].parent(app.config.show_hidden),
        Action::Refresh => app.refresh(),
        Action::Key(number) => crate::input::handle(app, KeyEvent::new(KeyCode::F(number), KeyModifiers::NONE)),
        Action::Layout(layout) => app.config.layouts[app.active] = layout,
        Action::QuickSearch => crate::quick_search::open(app),
        Action::Find => crate::discovery::open_find(app),
        Action::UserMenu => crate::user_menu::open(app,crate::user_menu::Source::Auto),
        Action::EditAssociations => crate::associations::edit(app),
        Action::ReloadAssociations => crate::associations::reload(app),
        Action::EditUserMenu(local) => crate::user_menu::edit_definition(app,local),
        Action::OpenExternal => app.open_external_current(),
        Action::BrowseArchive => crate::archive::open_current(app),
        Action::DirectoryTree => crate::directory_tree::open(app),
        Action::DirectorySizes => crate::discovery::open_sizes(app),
        Action::QuickView => crate::quick_view::toggle(app),
        Action::Sort(mode) => app.panels[app.active].set_sort(mode),
        Action::Hidden => app.toggle_hidden(),
        Action::GoTo => app.prompt_goto(),
        Action::SelectGroup => app.open_pattern(true),
        Action::UnselectGroup => app.open_pattern(false),
        Action::FileInfo => app.open_file_info(),
        Action::Columns => app.open_columns(),
        Action::SaveSettings => app.save_config(),
        Action::LoadSettings => app.load_config(),
        Action::ImportDosSettings => crate::config_import::open(app),
        Action::Settings(kind) => app.open_settings(kind),
        Action::View(mode) => app.open_viewer_mode(mode),
        Action::Confirmations => app.push_dialog(crate::dialog::Dialog {
            title: " Confirmation policy — read-only ",
            message: "[x] Confirm copy/move destinations\n[x] Confirm Trash / permanent deletion\n[x] Refuse existing destinations\n\nThese safeguards are always enabled.\nDeletion mode: Options → File Manager → Setup.\nEsc cancels an operation; completed items remain.\n\nEnter / Esc: close".into(),
        }),
        Action::Plugin(id)=>crate::plugins::host::open(app,id),
        Action::About => app.push_dialog(crate::dialog::Dialog {
            title: crate::dialog::ABOUT_TITLE,
            message: format!("{}\n{} {}\n{}\n\n{}: Dmitry Yarygin\n\n{}\n\n{}\n{}", crate::APPLICATION_NAME, app.config.language.text("Version"), crate::DISPLAY_VERSION, app.config.language.text("Native Rust terminal file manager"), app.config.language.text("Author"), crate::dialog::REPOSITORY_URL, app.config.language.text("F1: keyboard help"), app.config.language.text("Enter / Esc: close")),
        }),
        Action::Welcome => crate::welcome::open(app),
        Action::SelectAll => app.panels[app.active].select_visible(),
        Action::ClearSelection => app.panels[app.active].selected.clear(),
        Action::InvertSelection => app.panels[app.active].invert_visible(),
        Action::WindowList => crate::windowing::open(app),
        Action::WindowMove=>crate::windowing::move_active(app,false),
        Action::WindowZoom=>crate::windowing::move_active(app,true),
        Action::SwapPanels => { app.panels.swap(0, 1); app.active ^= 1; }
        _ => {}
    }
}
