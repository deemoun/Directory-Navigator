//! Single source for manager/editor menus and editor/viewer command bars.
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

// Editor: categories, enablement, entries and separators.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorRootId {
    File,
    Edit,
    Search,
    Paragraph,
    Block,
    Misc,
    Options,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditorItem {
    pub label: &'static str,
    pub shortcut: &'static str,
    pub action: Option<crate::editor::EditorAction>,
}
pub struct EditorRoot {
    pub id: EditorRootId,
    pub label: &'static str,
    pub enabled: bool,
    pub entries: &'static [EditorItem],
}
const fn editor_item(
    label: &'static str,
    shortcut: &'static str,
    action: crate::editor::EditorAction,
) -> EditorItem {
    EditorItem {
        label,
        shortcut,
        action: Some(action),
    }
}
const fn editor_line() -> EditorItem {
    EditorItem {
        label: "",
        shortcut: "",
        action: None,
    }
}
use crate::editor::EditorAction as A;
pub const EDITOR_ROOTS: &[EditorRoot] = &[
    EditorRoot {
        id: EditorRootId::File,
        label: "~F~ile",
        enabled: true,
        entries: &[
            editor_item("~O~pen...", "F3", A::Open),
            editor_item("~S~ave", "F2", A::Save),
            editor_item("Save ~a~s...", "Shift+F2", A::SaveAs),
            editor_line(),
            editor_item("~W~indows...", "Alt+0", A::Windows),
            editor_item("~C~lose editor", "Ctrl+F4", A::Close),
            editor_item("E~x~it DN", "Alt+X", A::Quit),
        ],
    },
    EditorRoot {
        id: EditorRootId::Edit,
        label: "~E~dit",
        enabled: true,
        entries: &[
            editor_item("~U~ndo", "Alt+Backspace", A::Undo),
            editor_item("~R~edo", "Ctrl+Y", A::Redo),
            editor_line(),
            editor_item("Cu~t~", "Shift+Del", A::Cut),
            editor_item("~C~opy", "Ctrl+Ins", A::Copy),
            editor_item("~P~aste", "Shift+Ins", A::Paste),
            editor_item("Select ~a~ll", "Ctrl+A", A::SelectAll),
            editor_item("De~l~ete selection", "Ctrl+Del", A::Clear),
        ],
    },
    EditorRoot {
        id: EditorRootId::Search,
        label: "~S~earch",
        enabled: true,
        entries: &[
            editor_item("~F~ind...", "F7", A::Find),
            editor_item("~R~eplace...", "Ctrl+F7", A::Replace),
            editor_item("~S~earch again", "Shift+F7", A::FindNext),
            editor_item("Re~v~ersed search", "Alt+F7", A::FindPrevious),
            editor_item("~G~o to line...", "Alt+G", A::GoTo),
        ],
    },
    EditorRoot {
        id: EditorRootId::Paragraph,
        label: "~P~aragraph",
        enabled: false,
        entries: &[],
    },
    EditorRoot {
        id: EditorRootId::Block,
        label: "~B~lock",
        enabled: true,
        entries: &[
            editor_item("Select ~a~ll", "Ctrl+A", A::SelectAll),
            editor_item("De~l~ete selection", "Ctrl+Del", A::Clear),
            editor_line(),
            editor_item("Cu~t~", "Shift+Del", A::Cut),
            editor_item("~C~opy", "Ctrl+Ins", A::Copy),
            editor_item("~P~aste", "Shift+Ins", A::Paste),
        ],
    },
    EditorRoot {
        id: EditorRootId::Misc,
        label: "~M~isc",
        enabled: true,
        entries: &[editor_item("~D~uplicate line", "F6", A::Duplicate)],
    },
    EditorRoot {
        id: EditorRootId::Options,
        label: "~O~ptions",
        enabled: true,
        entries: &[
            editor_item("Auto~i~ndent", "", A::AutoIndent),
            editor_item("Syntax ~h~ighlight", "", A::Colors),
            editor_item("~T~ab width...", "", A::TabWidth),
            editor_item("~E~xpand typed tabs to spaces", "", A::ExpandTabs),
            editor_line(),
            editor_item("~S~ave desktop", "", A::SaveDesktop),
        ],
    },
];

/// Status-bar entries are display order, captions and mouse actions. Shortcut
/// labels describe existing bindings; moving an entry never rebinds its key.
pub struct BarItem<A> {
    pub key: &'static str,
    pub label: &'static str,
    pub action: Option<A>,
    pub width: crate::presentation::layout::Constraint,
}
use crate::presentation::layout::Constraint as C;
pub const EDITOR_BAR: &[BarItem<A>] = &[
    BarItem {
        key: "F1",
        label: "Help",
        action: Some(A::Help),
        width: C::Fill(1),
    },
    BarItem {
        key: "F2",
        label: "Save",
        action: Some(A::Save),
        width: C::Fill(1),
    },
    BarItem {
        key: "F3",
        label: "Open",
        action: Some(A::Open),
        width: C::Fill(1),
    },
    BarItem {
        key: "F4",
        label: "",
        action: None,
        width: C::Fill(1),
    },
    BarItem {
        key: "F5",
        label: "",
        action: None,
        width: C::Fill(1),
    },
    BarItem {
        key: "F6",
        label: "Dupe",
        action: Some(A::Duplicate),
        width: C::Fill(1),
    },
    BarItem {
        key: "F7",
        label: "Find",
        action: Some(A::Find),
        width: C::Fill(1),
    },
    BarItem {
        key: "F8",
        label: "",
        action: None,
        width: C::Fill(1),
    },
    BarItem {
        key: "F9",
        label: "",
        action: None,
        width: C::Fill(1),
    },
    BarItem {
        key: "F10",
        label: "Menu",
        action: Some(A::Menu),
        width: C::Fill(1),
    },
];
use crate::viewer::Action as V;
pub const VIEWER_BAR: &[BarItem<V>] = &[
    BarItem {
        key: "F2",
        label: "Wrap",
        action: Some(V::Wrap),
        width: C::Length(12),
    },
    BarItem {
        key: "F7",
        label: "Find",
        action: Some(V::Find),
        width: C::Length(12),
    },
    BarItem {
        key: "S-F7",
        label: "Next",
        action: Some(V::Next),
        width: C::Length(16),
    },
    BarItem {
        key: "C-F7",
        label: "Reverse",
        action: Some(V::Reverse),
        width: C::Min(1),
    },
];
