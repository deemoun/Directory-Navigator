//! Single descriptions of ordinary boolean preferences and settings forms.
use super::SettingsKind;
use crate::dialog_theme as theme;
use crate::{config::Config, presentation::layout::Rect};
pub struct Field {
    pub caption: &'static str,
    pub group: &'static str,
    pub key: &'static str,
    pub get: fn(&Config) -> bool,
    pub set: fn(&mut Config, bool),
}
macro_rules! fields {
    ($( $id:ident: ($caption:literal, $group:literal, $key:literal, $field:ident) ),* $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Setting { $( $id ),* }
        pub const ALL: &[Setting] = &[$(Setting::$id),*];
        impl Setting {
            pub fn definition(self) -> Field {
                match self { $(Self::$id => Field {
                    caption: $caption, group: $group, key: $key,
                    get: |c| c.$field, set: |c,v| c.$field = v,
                }),* }
            }
            pub fn label(self) -> &'static str { self.definition().caption }
        }
    }
}
fields! {
    SaveOnExit: ("Save settings on e~x~it", "application", "save_on_exit", save_on_exit),
    Clock: ("Show ~c~lock", "interface", "show_clock", show_clock),
    HideMenu: ("Hide ~m~enu bar", "interface", "hide_menu", hide_menu),
    Hidden: ("Show ~h~idden files", "file_manager", "hidden", show_hidden),
    Highlight: ("~H~ighlight file types", "file_manager", "highlight", highlight_files),
    Tags: ("~T~ag character", "file_manager", "tags", show_tags),
    Space: ("~S~PACE toggles selection", "file_manager", "space_select", space_select),
    DeleteKey: ("~D~EL erases file(s)", "file_manager", "delete_key", delete_key),
    Trash: ("Move deleted files to T~r~ash", "file_manager", "use_trash", use_trash),
    Backspace: ("BS - go to ~u~pper directory", "file_manager", "backspace_parent", backspace_parent),
}
pub struct Form {
    pub title: &'static str,
    pub width: u16,
    pub height: u16,
    pub rows: &'static [Setting],
    pub columns: bool,
}
impl SettingsKind {
    pub(crate) fn definition(self) -> Form {
        use Setting::*;
        let (title, width, height, rows, columns): (_, _, _, &'static [Setting], _) = match self {
            Self::Keyboard => (" Keyboard shortcuts ", 74, 20, &[], false),
            Self::Regional => (" Language / date & time ", 68, 17, &[], false),
            Self::Plugins => (" Plugins ", 64, 10, &[], false),
            Self::System => (" System Setup ", 60, 14, &[Hidden, SaveOnExit], false),
            Self::Interface => (
                " Interface Setup ",
                60,
                13,
                &[Highlight, Tags, Clock, HideMenu],
                false,
            ),
            Self::FileManager => (
                " File Manager Setup ",
                74,
                14,
                &[Space, DeleteKey, Backspace, Tags, Highlight, Trash],
                true,
            ),
            Self::Highlight => (" Highlight Groups ", 76, 19, &[Highlight], false),
        };
        Form {
            title,
            width,
            height,
            rows,
            columns,
        }
    }
}
impl Form {
    pub fn bounds(&self, rect: Rect, index: usize) -> Rect {
        let trash = self.rows[index] == Setting::Trash;
        let right = self.columns && index >= 3 && !trash;
        Rect::new(
            rect.x + if right { 39 } else { theme::CONTENT_X },
            rect.y
                + theme::CONTENT_Y
                + if right {
                    index as u16 - 3
                } else if trash {
                    3
                } else {
                    index as u16
                },
            if self.columns { 32 } else { self.width - 6 },
            1,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn boolean_fields_have_unique_keys_and_independent_values() {
        let defaults = Config::default();
        let mut keys = std::collections::BTreeSet::new();
        for setting in ALL {
            let field = setting.definition();
            assert!(keys.insert((field.group, field.key)));
            let mut draft = defaults.clone();
            (field.set)(&mut draft, !(field.get)(&defaults));
            assert_ne!((field.get)(&draft), (field.get)(&defaults));
            for other in ALL.iter().filter(|other| *other != setting) {
                let other = other.definition();
                assert_eq!((other.get)(&draft), (other.get)(&defaults));
            }
        }
    }
}
