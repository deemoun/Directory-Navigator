pub(crate) mod store;
use crate::columns::ColumnPreset;
use crate::panel::SortMode;
use crate::panel_layout::PanelLayout;
use std::{
    fs, io,
    path::{Path, PathBuf},
};
use store::read_bytes;
pub use store::Store;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub language: crate::localization::Language,
    pub datetime: crate::regional::DateTimeFormat,
    pub save_on_exit: bool,
    pub palette: crate::palette::PaletteProfile,
    pub show_clock: bool,
    pub saver_minutes: u8,
    pub bookmarks: [Option<PathBuf>; 9],
    pub editor: crate::editor::Preferences,
    pub show_hidden: bool,
    pub sorts: [SortMode; 2],
    pub columns: [ColumnPreset; 2],
    pub layouts: [PanelLayout; 2],
    pub highlight_files: bool,
    pub show_tags: bool,
    pub space_select: bool,
    pub delete_key: bool,
    pub use_trash: bool,
    pub backspace_parent: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            language: Default::default(),
            datetime: Default::default(),
            save_on_exit: true,
            palette: Default::default(),
            show_clock: true,
            saver_minutes: crate::screensaver::DEFAULT_DELAY,
            bookmarks: Default::default(),
            editor: crate::editor::Preferences::default(),
            show_hidden: true,
            sorts: [SortMode::Name; 2],
            columns: [ColumnPreset::default(); 2],
            layouts: [PanelLayout::default(); 2],
            highlight_files: true,
            show_tags: true,
            space_select: true,
            delete_key: true,
            use_trash: true,
            backspace_parent: true,
        }
    }
}

impl Config {
    pub fn path() -> Option<PathBuf> {
        #[cfg(windows)]
        {
            std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .filter(|base| base.is_absolute())
                .map(|base| base.join("DOSNavigator").join("config.conf"))
        }
        #[cfg(not(windows))]
        {
            std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .or_else(|| {
                    std::env::var_os("HOME")
                        .map(PathBuf::from)
                        .filter(|home| home.is_absolute())
                        .map(|home| home.join(".config"))
                })
                .map(|base| base.join("dn").join("config.conf"))
        }
    }

    pub fn read(path: &Path) -> io::Result<Self> {
        match read_bytes(path)? {
            Some(bytes) => Self::parse(&bytes),
            None => Ok(Self::default()),
        }
    }

    fn parse(bytes: &[u8]) -> io::Result<Self> {
        let text = std::str::from_utf8(bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let mut config = Self {
            layouts: [PanelLayout::Details; 2],
            ..Self::default()
        };
        for (number, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            if !line.contains('=') {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Invalid configuration record at line {}", number + 1),
                ));
            }
            if let Some((key, value)) = line.split_once('=') {
                match key.trim() {
                    "language" => {
                        config.language = crate::localization::Language::parse(value.trim())
                    }
                    "date_order" => {
                        config.datetime.date_order = crate::regional::DateOrder::parse(value.trim())
                    }
                    "date_separator" => {
                        config.datetime.date_separator = value
                            .trim()
                            .chars()
                            .next()
                            .filter(|c| matches!(c, '-' | '.' | '/'))
                            .unwrap_or('-')
                    }
                    "time_format" => config.datetime.twelve_hour = value.trim() == "12",
                    "time_separator" => {
                        config.datetime.time_separator = if value.trim() == "." { '.' } else { ':' }
                    }
                    "config_version" if value.trim() != "1" => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "Unsupported configuration version",
                        ))
                    }
                    "save_on_exit" => config.save_on_exit = value.trim() != "false",
                    "palette" => {
                        config.palette = crate::palette::PaletteProfile::parse(value.trim())
                    }
                    "saver_minutes" => {
                        config.saver_minutes = value
                            .trim()
                            .parse::<u8>()
                            .ok()
                            .filter(|v| crate::screensaver::DELAYS.contains(v))
                            .unwrap_or(crate::screensaver::DEFAULT_DELAY)
                    }
                    "show_clock" => config.show_clock = value.trim() != "false",
                    "editor_auto_indent" => config.editor.auto_indent = value.trim() != "false",
                    "editor_syntax" => config.editor.syntax = value.trim() != "false",
                    "editor_expand_tabs" => config.editor.expand_tabs = value.trim() != "false",
                    "editor_tab_size" => {
                        config.editor.tab_size = value
                            .trim()
                            .parse::<u8>()
                            .ok()
                            .filter(|v| (1..=16).contains(v))
                            .unwrap_or(8)
                    }
                    "hidden" => config.show_hidden = value.trim() != "false",
                    "highlight" => config.highlight_files = value.trim() != "false",
                    "tags" => config.show_tags = value.trim() != "false",
                    "space_select" => config.space_select = value.trim() != "false",
                    "delete_key" => config.delete_key = value.trim() != "false",
                    "use_trash" => config.use_trash = value.trim() != "false",
                    "backspace_parent" => config.backspace_parent = value.trim() != "false",
                    "left_layout" | "right_layout" => {
                        config.layouts[usize::from(key.trim() == "right_layout")] =
                            PanelLayout::parse(value.trim());
                    }
                    "left_columns" | "right_columns" => {
                        config.columns[usize::from(key.trim() == "right_columns")] =
                            ColumnPreset::parse(value.trim());
                    }
                    "left_sort" | "right_sort" => {
                        let mode = match value.trim() {
                            "extension" => SortMode::Extension,
                            "size" => SortMode::Size,
                            "date" => SortMode::Date,
                            _ => SortMode::Name,
                        };
                        config.sorts[usize::from(key.trim() == "right_sort")] = mode;
                    }
                    key if key.starts_with("bookmark_") => {
                        if let Some(slot) = key
                            .strip_prefix("bookmark_")
                            .and_then(|s| s.parse::<usize>().ok())
                            .filter(|s| (1..=9).contains(s))
                        {
                            config.bookmarks[slot - 1] = crate::bookmarks::decode(value.trim());
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(config)
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        self.save_preserving(path, &[])
    }

    fn encode(&self) -> io::Result<Vec<u8>> {
        use std::io::Write;
        let name = |sort| match sort {
            SortMode::Name => "name",
            SortMode::Extension => "extension",
            SortMode::Size => "size",
            SortMode::Date => "date",
        };
        let mut output = Vec::new();

        writeln!(output, "# Directory Navigator preferences\n# Saved automatically on normal exit; Options > Save desktop saves now.\nconfig_version=1\nsave_on_exit={}", self.save_on_exit)?;
        writeln!(
            output,
            "language={}\ndate_order={}\ndate_separator={}\ntime_format={}\ntime_separator={}",
            self.language.key(),
            self.datetime.date_order.key(),
            self.datetime.date_separator,
            if self.datetime.twelve_hour {
                "12"
            } else {
                "24"
            },
            self.datetime.time_separator
        )?;
        write!(
            output,
            "hidden={}\nleft_sort={}\nright_sort={}\n",
            self.show_hidden,
            name(self.sorts[0]),
            name(self.sorts[1])
        )?;
        writeln!(
            output,
            "highlight={}\ntags={}\nspace_select={}\ndelete_key={}\nbackspace_parent={}",
            self.highlight_files,
            self.show_tags,
            self.space_select,
            self.delete_key,
            self.backspace_parent
        )?;
        writeln!(
            output,
            "left_columns={}\nright_columns={}",
            self.columns[0].key(),
            self.columns[1].key()
        )?;
        writeln!(output, "use_trash={}", self.use_trash)?;
        writeln!(
            output,
            "left_layout={}\nright_layout={}",
            self.layouts[0].key(),
            self.layouts[1].key()
        )?;
        writeln!(
            output,
            "editor_auto_indent={}\neditor_syntax={}\neditor_expand_tabs={}\neditor_tab_size={}",
            self.editor.auto_indent,
            self.editor.syntax,
            self.editor.expand_tabs,
            self.editor.tab_size
        )?;
        for (i, path) in self.bookmarks.iter().enumerate() {
            if let Some(path) = path {
                writeln!(
                    output,
                    "bookmark_{}={}",
                    i + 1,
                    crate::bookmarks::encode(path)
                )?;
            }
        }
        writeln!(
            output,
            "palette={}\nshow_clock={}",
            self.palette.key(),
            self.show_clock
        )?;
        writeln!(output, "saver_minutes={}", self.saver_minutes)?;
        Ok(output)
    }

    fn save_preserving(&self, path: &Path, previous: &[u8]) -> io::Result<()> {
        use std::io::Write;
        let mut bytes = self.encode()?;
        let encoded = String::from_utf8_lossy(&bytes);
        let known: std::collections::HashSet<_> = encoded
            .lines()
            .filter_map(|line| line.split_once('=').map(|(key, _)| key.to_owned()))
            .collect();
        // Keep extension keys from newer versions or hand-edited configurations.
        for line in String::from_utf8_lossy(previous).lines() {
            if let Some((key, _)) = line.split_once('=') {
                let key = key.trim();
                if !known.contains(key) && !key.starts_with("bookmark_") {
                    bytes.extend_from_slice(line.as_bytes());
                    bytes.push(b'\n');
                }
            }
        }
        match fs::symlink_metadata(path) {
            Ok(metadata) if !metadata.file_type().is_file() => {
                return Err(io::Error::other(
                    "Configuration destination must be a regular file, not a directory or symlink",
                ))
            }
            Ok(metadata) if metadata.permissions().readonly() => {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "Configuration file is read-only",
                ));
            }
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        let mut staged = crate::publication::StagedFile::create(path)?;
        staged.file.write_all(&bytes)?;
        staged.file.sync_all()?;
        fs::rename(staged.path(), path)?;
        crate::platform_fs::sync_parent(path)
    }
}
