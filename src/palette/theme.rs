//! Editable slots name actual DN render attributes, not unimplemented DOS features.
use super::PaletteProfile;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Timer,
    Menus,
    Dialogs,
    FilePanel,
    QuickView,
    Viewer,
    Editor,
    Help,
    Status,
    /// Physical swatches bypass editable UI slots.
    Physical,
}
pub const GROUPS: &[(Group, &str)] = &[
    (Group::Timer, "Timer"),
    (Group::Menus, "Menus"),
    (Group::Dialogs, "Dialogs"),
    (Group::FilePanel, "File Manager"),
    (Group::QuickView, "└─ Quick View"),
    (Group::Viewer, "File Viewer"),
    (Group::Editor, "Editor"),
    (Group::Help, "Help"),
    (Group::Status, "Function bar"),
];
#[derive(Clone, Copy)]
pub struct Item {
    pub slot: Slot,
    pub key: &'static str,
    pub group: Group,
    pub label: &'static str,
    pub attribute: u8,
}
macro_rules! items {
    ($($slot:ident, $key:literal, $group:ident, $label:literal, $attr:literal;)*) => {
        #[repr(usize)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Slot { $($slot,)* Count }
        pub const COUNT: usize = Slot::Count as usize;
        pub const ITEMS: &[Item] = &[$(Item {slot: Slot::$slot, key: $key, group: Group::$group, label: $label, attribute: $attr},)*];
        pub const DEFAULTS: [u8; COUNT] = [$($attr,)*];
    }
}
items! {
    TimerColor,"timer",Timer,"Color",0x71;
    MenuNormal,"menu_normal",Menus,"Normal",0x70;
    MenuDisabled,"menu_disabled",Menus,"Disabled",0x78;
    MenuShortcut,"menu_shortcut",Menus,"Shortcut",0x74;
    MenuSelected,"menu_selected",Menus,"Selected",0x20;
    MenuSelectedShortcut,"menu_selected_shortcut",Menus,"Shortcut selected",0x24;
    DialogText,"dialog_text",Dialogs,"Background / text",0x70;
    DialogFrame,"dialog_frame",Dialogs,"Frame / selected text",0x7f;
    DialogIcon,"dialog_icon",Dialogs,"Frame icons",0x7a;
    DialogHeading,"dialog_heading",Dialogs,"Information / heading",0x71;
    DialogDisabled,"dialog_disabled",Dialogs,"Disabled",0x78;
    DialogShortcut,"dialog_shortcut",Dialogs,"Shortcut",0x7e;
    DialogButton,"dialog_button",Dialogs,"Button normal",0x8f;
    DialogDefault,"dialog_default",Dialogs,"Button default",0x8e;
    DialogButtonMarker,"dialog_button_marker",Dialogs,"Button markers",0x8b;
    DialogShadow,"dialog_shadow",Dialogs,"Button shadow",0x00;
    DialogInput,"dialog_input",Dialogs,"Input normal",0x07;
    DialogFocusedInput,"dialog_focused_input",Dialogs,"Input / list focus",0x0f;
    DialogFileInput,"dialog_file_input",Dialogs,"File dialog input",0x17;
    DialogFileSelected,"dialog_file_selected",Dialogs,"File dialog selected",0x1f;
    DialogList,"dialog_list",Dialogs,"List normal / selection",0x30;
    DialogListSelected,"dialog_list_selected",Dialogs,"List inactive selection",0x3f;
    DialogListCursor,"dialog_list_cursor",Dialogs,"List cursor",0x2f;
    DialogHistory,"dialog_history",Dialogs,"History button",0x9f;
    PanelText,"panel_text",FilePanel,"Normal text / inactive frame",0x87;
    PanelDirectory,"panel_directory",FilePanel,"Directory / active frame",0x8f;
    PanelHeader,"panel_header",FilePanel,"Column title / marked",0x8e;
    PanelCursor,"panel_cursor",FilePanel,"Normal cursor",0x30;
    PanelMarkedCursor,"panel_marked_cursor",FilePanel,"Marked cursor",0x3e;
    PanelTitle,"panel_title",FilePanel,"Active path",0x3f;
    PanelExecutable,"panel_executable",FilePanel,"Executables",0x8a;
    PanelArchive,"panel_archive",FilePanel,"Archives",0x8d;
    PanelSource,"panel_source",FilePanel,"Source files",0x8b;
    PanelMedia,"panel_media",FilePanel,"Media files",0x8c;
    PanelLink,"panel_link",FilePanel,"Symbolic links",0x89;
    QuickText,"quick_text",QuickView,"Normal text / frame",0x87;
    QuickStatus,"quick_status",QuickView,"Status",0x30;
    ViewerText,"viewer_text",Viewer,"Normal text",0x1f;
    ViewerMatch,"viewer_match",Viewer,"Search match / status",0x30;
    ViewerNotice,"viewer_notice",Viewer,"Notice",0x70;
    EditorText,"editor_text",Editor,"Normal text",0x1f;
    EditorSelection,"editor_selection",Editor,"Selection",0x30;
    EditorNotice,"editor_notice",Editor,"Notice",0x1b;
    EditorStatus,"editor_status",Editor,"Status",0x70;
    HelpText,"help_text",Help,"Normal text",0x30;
    HelpSelected,"help_selected",Help,"Selected link",0x3e;
    HelpLink,"help_link",Help,"Link",0x3f;
    StatusText,"status_text",Status,"Normal text",0x70;
    StatusDisabled,"status_disabled",Status,"Disabled",0x78;
    StatusKey,"status_key",Status,"Shortcut",0x74;
}
pub fn items(group: usize) -> impl Iterator<Item = &'static Item> {
    let group = GROUPS[group.min(GROUPS.len() - 1)].0;
    ITEMS.iter().filter(move |i| i.group == group)
}
impl PaletteProfile {
    pub fn slot(self, slot: Slot) -> u8 {
        match self {
            Self::Custom(values) => values[slot as usize],
            _ => DEFAULTS[slot as usize],
        }
    }
    pub fn with_slot(self, slot: Slot, value: u8) -> Self {
        let mut values = match self {
            Self::Custom(v) => v,
            _ => DEFAULTS,
        };
        values[slot as usize] = value;
        if values == DEFAULTS {
            Self::ClassicDn
        } else {
            Self::Custom(values)
        }
    }
    pub(crate) fn themed(
        self,
        group: Group,
        attribute: u8,
    ) -> Option<crate::presentation::style::Style> {
        if !matches!(self, Self::Custom(_)) {
            return None;
        }
        ITEMS
            .iter()
            .find(|i| i.group == group && i.attribute == attribute)
            .map(|i| self.dos(self.slot(i.slot)))
    }
    pub fn for_group(self, group: Group, attribute: u8) -> crate::presentation::style::Style {
        self.themed(group, attribute)
            .unwrap_or_else(|| self.dos(attribute))
    }
    pub fn styles_for(
        self,
        group: Group,
    ) -> impl Fn(u8) -> crate::presentation::style::Style + Copy {
        move |attr| self.for_group(group, attr)
    }
    pub fn is_classic(self) -> bool {
        !matches!(self, Self::Terminal)
    }
}
