//! Structure and mnemonics follow original-dn/RESOURCE/ENGLISH/DN.DNR, dlgMainMenu.
use crate::{app::App, panel::SortMode, settings::SettingsKind, viewer::ViewMode};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
mod navigation;
pub use navigation::{activate, close, key, open, open_mnemonic};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuId {
    Associations,
    CopyNames,
    View,
    Sort,
    Layout,
    Configuration,
    FileManager,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
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
    SaveSettings,
    LoadSettings,
    About,
    Welcome,
    Game,
    Settings(SettingsKind),
    Confirmations,
    View(ViewMode),
}

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
fn item(label: &'static str, shortcut: &'static str, action: Action) -> Item {
    Item {
        label,
        shortcut,
        action,
    }
}
fn line() -> Item {
    item("", "", Action::Separator)
}

pub const LABELS: [&str; 6] = [
    "≡",
    "~F~ile",
    "~U~tilities",
    "~P~anel",
    "~M~anager",
    "~O~ptions",
];

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

pub fn items(menu: usize) -> Vec<Item> {
    use Action::*;
    match menu {
        0 => vec![
            item("~W~elcome...", "", Welcome),
            item("~A~bout...", "", About),
            item("~R~efresh", "", Refresh),
            line(),
            item("~S~creensaver", "", ScreenRest),
            item("~G~ame", "Alt+F9", Game),
        ],
        1 => vec![
            item("~V~iew", "", Submenu(MenuId::View)),
            item("~E~dit", "F4", Key(4)),
            item("~B~rowse archive...", "Enter", BrowseArchive),
            item("~O~pen with default app", "Ctrl+Enter", OpenExternal),
            item("~F~ind...", "Ctrl+F7", Find),
            item("~C~opy...", "F5", Key(5)),
            item("Copy ~n~ames / paths", "", Submenu(MenuId::CopyNames)),
            item("~R~ename/Move...", "F6", Key(6)),
            item("~M~ake directory", "F7", Key(7)),
            item("~D~elete", "F8", Key(8)),
            item("File ~A~ttributes / Info...", "Alt+E", FileInfo),
            line(),
            item("E~x~it", "Alt+X / F10", Key(10)),
        ],
        2 => vec![
            item("~U~ser menu", "F2", UserMenu),
            item(
                "~M~emory Information",
                "Alt+Y",
                Utility(crate::utilities::Kind::Memory),
            ),
            item(
                "S~y~stem Information",
                "",
                Utility(crate::utilities::Kind::System),
            ),
            line(),
            item(
                "~C~alculator",
                "Ctrl+K",
                Utility(crate::utilities::Kind::Calculator),
            ),
            item(
                "ASCII Ta~b~le",
                "Ctrl+B",
                Utility(crate::utilities::Kind::Characters),
            ),
            line(),
            item("Commands ~H~istory", "Alt+F8", CommandHistory),
            item(
                "File ~E~dit History",
                "Alt+PgUp",
                History(crate::file_tools::history::HistoryKind::Edit),
            ),
            item(
                "File ~V~iew History",
                "Alt+PgDn",
                History(crate::file_tools::history::HistoryKind::View),
            ),
        ],
        3 => vec![
            item("~Q~uick directories...", "", Bookmarks),
            item("~F~ilename filter...", "Alt+Del", FilenameFilter),
            item("~M~ake list file...", "Alt+L", ExportList),
            item("~C~ompare directories...", "Ctrl+C", CompareDirectories),
            line(),
            item("Setup c~o~lumns...", "Alt+K", Columns),
            item(
                "~S~etup Panel...",
                "Alt+S",
                Settings(SettingsKind::FileManager),
            ),
            item("Sort ~b~y...", "Alt+B", Submenu(MenuId::Sort)),
            line(),
            item("Select grou~p~...", "+", SelectGroup),
            item("~U~nselect group...", "-", UnselectGroup),
            item("In~v~ert selection", "*", InvertSelection),
            item("Select all", "Ctrl+A", SelectAll),
            item("Clear all marks", "Ctrl+U", ClearSelection),
            line(),
            item("Change direc~t~ory...", "Alt+T", GoTo),
            item("~R~e-read", "Alt+R", Refresh),
            item("Parent directory", "Backspace", Parent),
            item(
                "History of ~d~irectories...",
                "Alt+Backspace",
                History(crate::file_tools::history::HistoryKind::Directory),
            ),
            item("~L~ayout", "", Submenu(MenuId::Layout)),
            item("Quick s~e~arch...", "Alt+F7", QuickSearch),
            item("Calculate directory si~z~es...", "", DirectorySizes),
        ],
        4 => vec![
            item("~D~irectory tree", "Ctrl+T", DirectoryTree),
            item("~Q~uick view", "Ctrl+Q", QuickView),
            line(),
            item("~S~wap panels", "", SwapPanels),
            item("~W~indows...", "Alt+0", WindowList),
        ],
        5 => vec![
            item("~C~onfiguration", "", Submenu(MenuId::Configuration)),
            item("~F~ile Manager", "", Submenu(MenuId::FileManager)),
            line(),
            item("~G~lobal menu definition...", "", EditUserMenu(false)),
            item("Local ~m~enu definition...", "", EditUserMenu(true)),
            item("File ~a~ssociations", "", Submenu(MenuId::Associations)),
            line(),
            item("~S~ave desktop", "", SaveSettings),
            item("~L~oad desktop", "", LoadSettings),
            item("~I~mport DOS settings...", "", ImportDosSettings),
            line(),
            item("C~o~lors...", "", Colors),
        ],
        _ => Vec::new(),
    }
}

pub fn submenu_items(menu: MenuId) -> Vec<Item> {
    use Action::*;
    match menu {
        MenuId::Associations => vec![
            item("~E~dit associations...", "", EditAssociations),
            item("~R~eload associations", "", ReloadAssociations),
        ],
        MenuId::CopyNames => vec![
            item("~N~ames", "", CopyNames(false)),
            item("~F~ull paths", "", CopyNames(true)),
        ],
        MenuId::View => vec![
            item("~A~s is", "F3", View(ViewMode::Auto)),
            item("As ~T~ext", "", View(ViewMode::Text)),
            item("As He~x~", "", View(ViewMode::Hex)),
        ],
        MenuId::Sort => vec![
            item("~N~ame", "Ctrl+F3", Sort(SortMode::Name)),
            item("~E~xtension", "Ctrl+F4", Sort(SortMode::Extension)),
            item("~S~ize", "Ctrl+F6", Sort(SortMode::Size)),
            item("~D~ate", "Ctrl+F5", Sort(SortMode::Date)),
        ],
        MenuId::Layout => vec![
            item(
                "~B~rief",
                "",
                Layout(crate::panel_layout::PanelLayout::Brief),
            ),
            item(
                "~D~etails",
                "",
                Layout(crate::panel_layout::PanelLayout::Details),
            ),
        ],
        MenuId::Configuration => vec![
            item(
                "~K~eyboard shortcuts...",
                "",
                Settings(SettingsKind::Keyboard),
            ),
            item("S~y~stem Setup...", "", Settings(SettingsKind::System)),
            item("~I~nterface...", "", Settings(SettingsKind::Interface)),
            item(
                "~L~anguage / date & time...",
                "",
                Settings(SettingsKind::Regional),
            ),
            item("~C~onfirmation policy...", "", Confirmations),
            line(),
            item("Screen sa~v~ers...", "", ScreensaverSetup),
        ],
        MenuId::FileManager => vec![
            item("~S~etup...", "", Settings(SettingsKind::FileManager)),
            item("~C~olumn defaults...", "", Columns),
            item(
                "~H~ighlight groups...",
                "",
                Settings(SettingsKind::Highlight),
            ),
        ],
    }
}

pub fn level_items(app: &App, depth: usize) -> Vec<Item> {
    if depth == 0 {
        app.menu.map(|(root, _)| items(root)).unwrap_or_default()
    } else {
        app.submenus
            .get(depth - 1)
            .map(|(id, _)| submenu_items(*id))
            .unwrap_or_default()
    }
}

pub fn selected_row(app: &App, depth: usize) -> usize {
    if depth == 0 {
        app.menu.map(|(_, row)| row).unwrap_or(0)
    } else {
        app.submenus
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
    app.quick_search = None;
    match action {
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
        Action::Confirmations => app.dialogs.push(crate::dialog::Dialog {
            title: " Confirmation policy — read-only ",
            message: "[x] Confirm copy/move destinations\n[x] Confirm Trash / permanent deletion\n[x] Refuse existing destinations\n\nThese safeguards are always enabled.\nDeletion mode: Options → File Manager → Setup.\nEsc cancels an operation; completed items remain.\n\nEnter / Esc: close".into(),
        }),
        Action::Game => crate::tetris::open(app),
        Action::About => app.dialogs.push(crate::dialog::Dialog {
            title: " About Directory Navigator ",
            message: format!("{}\n{} {}\n{}\n\n{}: Dmitry Yarygin\n\n{}\n{}", crate::APPLICATION_NAME, app.config.language.text("Version"), crate::DISPLAY_VERSION, app.config.language.text("Native Rust terminal file manager"), app.config.language.text("Author"), app.config.language.text("F1: keyboard help"), app.config.language.text("Enter / Esc: close")),
        }),
        Action::Welcome => crate::welcome::open(app),
        Action::SelectAll => app.panels[app.active].select_visible(),
        Action::ClearSelection => app.panels[app.active].selected.clear(),
        Action::InvertSelection => app.panels[app.active].invert_visible(),
        Action::WindowList => crate::windowing::open(app),
        Action::SwapPanels => { app.panels.swap(0, 1); app.active ^= 1; }
        _ => {}
    }
}
