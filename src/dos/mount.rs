//! Optional extra DOS drive. No drive data is copied, erased or mounted by DN.
use super::*;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Off,
    Temp,
    Folder,
}
impl Mode {
    pub fn key(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Temp => "temp_drive",
            Self::Folder => "folder",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Temp => "DN Temp Drive",
            Self::Folder => "Folder",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        [Self::Off, Self::Temp, Self::Folder]
            .into_iter()
            .find(|m| m.key() == s)
    }
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::Temp,
            Self::Temp => Self::Folder,
            Self::Folder => Self::Off,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preferences {
    pub mode: Mode,
    pub folder: String,
    pub drive: u8,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            mode: Mode::Off,
            folder: String::new(),
            drive: b'D',
        }
    }
}
fn native_path(text: &str) -> io::Result<()> {
    if text.len() > 4096 || text.chars().any(char::is_control) {
        return Err(io::Error::other("Invalid DOS mount folder path."));
    }
    Ok(())
}

impl Preferences {
    pub fn validate(&self) -> io::Result<()> {
        if !(b'D'..=b'X').contains(&self.drive) {
            return Err(io::Error::other(
                "Choose an extra drive from D: to X:. C:, Y: and Z: are reserved.",
            ));
        }
        if self.folder.len() > 4096 || self.folder.chars().any(char::is_control) {
            return Err(io::Error::other("Invalid DOS mount folder path."));
        }
        if self.mode == Mode::Folder {
            native_path(&self.folder)?;
        }
        if self.mode == Mode::Folder && !Path::new(&self.folder).is_absolute() {
            return Err(io::Error::other(
                "Choose an absolute path to a real folder for the DOS drive.",
            ));
        }
        Ok(())
    }
    pub fn summary(&self, l: crate::localization::Language) -> String {
        if self.mode == Mode::Off {
            format!("{}: {}", l.text("Extra DOS drive"), l.text("Off"))
        } else {
            format!(
                "{}: {}: · {}",
                l.text("Extra DOS drive"),
                char::from(self.drive),
                l.text(self.mode.label())
            )
        }
    }
}
pub(super) struct Mounted {
    path: PathBuf,
    identity: crate::platform_fs::EntryIdentity,
    drive: u8,
}
impl Mounted {
    pub(super) fn validate(&self) -> io::Result<()> {
        directory(&self.path)?;
        if crate::platform_fs::entry_identity(&self.path)? != self.identity {
            return Err(io::Error::other(
                "DOS mount folder changed before launch; select it again.",
            ));
        }
        Ok(())
    }
    pub(super) fn path(&self) -> &Path {
        &self.path
    }
    pub(super) fn commands(&self) -> Vec<String> {
        vec![
            "Z:".into(),
            format!("MOUNT -u {}", char::from(self.drive)),
            format!("MOUNT {} .", char::from(self.drive)),
        ]
    }
    pub(super) fn arguments(&self) -> Vec<std::ffi::OsString> {
        self.commands()
            .into_iter()
            .flat_map(|c| ["-c".into(), c.into()])
            .collect()
    }
}
fn directory(path: &Path) -> io::Result<()> {
    let m = fs::symlink_metadata(path)?;
    if !m.is_dir() || crate::platform_fs::directory_link(&m) {
        return Err(io::Error::other(
            "DOS mounts require a real folder, not a directory link or file.",
        ));
    }
    Ok(())
}
pub(super) fn prepare(
    p: &Preferences,
    config: Option<&Path>,
    create: bool,
    cancel: &crate::task::Cancellation,
) -> io::Result<Option<Mounted>> {
    p.validate()?;
    cancel.check()?;
    let path = match p.mode {
        Mode::Off => return Ok(None),
        Mode::Folder => {
            let path = PathBuf::from(&p.folder);
            directory(&path)?;
            fs::canonicalize(path)?
        }
        Mode::Temp => {
            let config = config
                .ok_or_else(|| io::Error::other("Cannot determine the Temp Drive directory"))?;
            let Some(root) = crate::temp_drive::Root::from_config(config, create)? else {
                return Ok(None);
            };
            root.validate()?;
            root.path().to_path_buf()
        }
    };
    let text = path.to_str().ok_or_else(|| {
        io::Error::other("This folder path cannot be represented as a Unicode DOS mount path.")
    })?;
    native_path(text)?;
    directory(&path)?;
    let identity = crate::platform_fs::entry_identity(&path)?;
    let _reader = fs::read_dir(&path)?;
    cancel.check()?;
    let mounted = Mounted {
        path,
        identity,
        drive: p.drive,
    };
    mounted.validate()?;
    Ok(Some(mounted))
}

pub(crate) fn pick_folder(
    app: &mut App,
    b: crate::plugins::Binding,
    generation: u64,
    base: PathBuf,
) {
    let crate::plugins::Owner::Document(owner) = b.owner else {
        return;
    };
    if b.plugin != crate::plugins::DOS_LAUNCHER || app.desktop.plugins.binding(b.plugin) != Some(b)
    {
        return;
    }
    match crate::directory_tree::Tree::new(app.active, base, app.config.show_hidden) {
        Ok(mut tree) => {
            tree.destination_dos = Some((b, generation));
            app.modals.push_peer_tree(tree, owner);
            app.sync_modals();
        }
        Err(error) => super::report(
            app,
            owner,
            app.desktop.activation_revision(),
            error.to_string(),
            true,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_preview_and_persistent_temp_drive_are_isolated() -> io::Result<()> {
        let w = crate::network::workspace::Workspace::create(std::env::temp_dir())?;
        let config = w.root.join("settings/config.conf");
        let cancel = Default::default();
        assert!(prepare(&Preferences::default(), None, true, &cancel)?.is_none());
        assert!(!w.root.join("settings").exists());
        let p = Preferences {
            mode: Mode::Temp,
            drive: b'F',
            ..Default::default()
        };
        assert!(prepare(&p, Some(&config), false, &cancel)?.is_none());
        assert!(!w.root.join("settings").exists());
        let m = prepare(&p, Some(&config), true, &cancel)?.unwrap();
        assert_eq!(
            m.path,
            fs::canonicalize(config.parent().unwrap())?.join("temp-drive")
        );
        fs::write(m.path.join("keep.txt"), b"kept")?;
        let commands = m.commands();
        assert_eq!(&commands[..2], ["Z:", "MOUNT -u F"]);
        assert_eq!(commands[2], "MOUNT F .");
        drop(m);
        assert_eq!(
            fs::read(config.parent().unwrap().join("temp-drive/keep.txt"))?,
            b"kept"
        );
        Ok(())
    }
    #[test]
    fn real_folder_paths_are_literal_and_changed_roots_are_refused() -> io::Result<()> {
        let w = crate::network::workspace::Workspace::create(std::env::temp_dir())?;
        let folder = w.root.join("Shared Ёжик $ & % literal");
        fs::create_dir(&folder)?;
        let p = Preferences {
            mode: Mode::Folder,
            folder: folder.to_str().unwrap().into(),
            drive: b'X',
        };
        let m = prepare(&p, None, true, &Default::default())?.unwrap();
        for kind in [Kind::Classic, Kind::X, Kind::Staging] {
            let v = Verified {
                kind,
                version: "DOSBox Staging 0.83.0".into(),
                flag: "--nolocalconf",
            };
            let mut args: Vec<std::ffi::OsString> = flags(&v).into_iter().map(Into::into).collect();
            args.extend(m.arguments());
            let source = w.root.join("game.COM");
            let command = super::super::process::command(Path::new("emulator"), &source, &args);
            assert_eq!(command.get_args().last().unwrap(), source.as_os_str());
            assert_eq!(
                command
                    .get_args()
                    .filter(|v| *v == std::ffi::OsStr::new("-c"))
                    .count(),
                3
            );
            assert_eq!(args.last().unwrap(), "MOUNT X .");
            assert_eq!(m.path(), fs::canonicalize(&folder)?);
        }
        fs::rename(&folder, w.root.join("old"))?;
        fs::create_dir(&folder)?;
        assert!(m.validate().is_err());
        for drive in [b'A', b'B', b'C', b'Y', b'Z', 0, b'd'] {
            assert!(Preferences { drive, ..p.clone() }.validate().is_err());
        }
        for path in [
            "bad%folder",
            "bad\"folder",
            "bad&folder",
            "bad|folder",
            "bad<folder",
            "bad\nfolder",
        ] {
            assert!(Preferences {
                folder: path.into(),
                ..p.clone()
            }
            .validate()
            .is_err());
        }
        Ok(())
    }
    #[cfg(unix)]
    #[test]
    fn links_and_cancelled_mounts_are_not_followed() -> io::Result<()> {
        let w = crate::network::workspace::Workspace::create(std::env::temp_dir())?;
        let folder = w.root.join("folder");
        fs::create_dir(&folder)?;
        let link = w.root.join("link");
        std::os::unix::fs::symlink(&folder, &link)?;
        let mut p = Preferences {
            mode: Mode::Folder,
            folder: link.to_str().unwrap().into(),
            ..Default::default()
        };
        assert!(prepare(&p, None, true, &Default::default()).is_err());
        p.folder = folder.to_str().unwrap().into();
        let cancel = crate::task::Cancellation::default();
        cancel.cancel();
        assert!(prepare(&p, None, true, &cancel).is_err());
        Ok(())
    }
}
