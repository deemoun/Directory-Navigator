//! Single source for the manager's menu hierarchy.
//! Reorder ROOTS or their entries here; named IDs keep shortcuts/plugin placement
//! independent of display positions. Captions use ~mnemonics~ and locale keys.
use super::{item, line, Action, Item};
use crate::{panel::SortMode, settings::SettingsKind, viewer::ViewMode};
use Action::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RootId {
    Application,
    File,
    Utilities,
    Panel,
    Manager,
    Options,
}

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

pub struct Root {
    pub id: RootId,
    pub label: &'static str,
    pub entries: &'static [Item],
}

pub const ROOTS: &[Root] = &[
    Root {
        id: RootId::Application,
        label: "≡",
        entries: &[
            item("~W~elcome...", "", Welcome),
            item("~A~bout...", "", About),
            item("~R~efresh", "", Refresh),
            line(),
            item("~S~creensaver", "", ScreenRest),
            item("~G~ame", "Alt+F9", Plugin(crate::plugins::TETRIS)),
        ],
    },
    Root {
        id: RootId::File,
        label: "~F~ile",
        entries: &[
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
    },
    Root {
        id: RootId::Utilities,
        label: "~U~tilities",
        entries: &[
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
            item("~C~alculator", "Ctrl+K", Plugin(crate::plugins::CALCULATOR)),
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
    },
    Root {
        id: RootId::Panel,
        label: "~P~anel",
        entries: &[
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
    },
    Root {
        id: RootId::Manager,
        label: "~M~anager",
        entries: &[
            item("~D~irectory tree", "Ctrl+T", DirectoryTree),
            item("~Q~uick view", "Ctrl+Q", QuickView),
            item("Terminal ~o~utput", "Ctrl+O", TerminalOutput),
            line(),
            item("~S~wap panels", "", SwapPanels),
            item("~W~indows...", "Alt+0", WindowList),
        ],
    },
    Root {
        id: RootId::Options,
        label: "~O~ptions",
        entries: &[
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
    },
];

pub(super) fn submenu_items(menu: MenuId) -> Vec<Item> {
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
            item("~P~lugins...", "", Settings(SettingsKind::Plugins)),
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
