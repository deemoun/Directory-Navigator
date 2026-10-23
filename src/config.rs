mod format;
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
    pub shortcuts: crate::shortcuts::Profile,
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
            shortcuts: Default::default(),
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

    pub fn parse(bytes: &[u8]) -> io::Result<Self> {
        format::parse(bytes)
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        self.save_preserving(path, read_bytes(path)?.as_deref())
    }

    fn save_preserving(&self, path: &Path, previous: Option<&[u8]>) -> io::Result<()> {
        use std::io::Write;
        let bytes = format::encode(self, previous)?;
        if bytes.len() > store::MAX_CONFIG_BYTES as usize {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Configuration exceeds 1 MiB",
            ));
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
