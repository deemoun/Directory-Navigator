//! DOSBox-X Windows 9x booting. Images are writable guest disks, never DOS programs.
use super::*;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Edition {
    Win95,
    Win95Osr2,
    #[default]
    Win98,
}
impl Edition {
    pub fn key(self) -> &'static str {
        match self {
            Self::Win95 => "95",
            Self::Win95Osr2 => "95_osr2",
            Self::Win98 => "98",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Win95 => "Windows 95",
            Self::Win95Osr2 => "Windows 95 OSR2",
            Self::Win98 => "Windows 98 / 98 SE",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        [Self::Win95, Self::Win95Osr2, Self::Win98]
            .into_iter()
            .find(|v| v.key() == s)
    }
    pub fn next(self) -> Self {
        match self {
            Self::Win95 => Self::Win95Osr2,
            Self::Win95Osr2 => Self::Win98,
            Self::Win98 => Self::Win95,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preferences {
    pub edition: Edition,
    pub image: String,
    pub executable: String,
    pub memory_mb: u16,
    pub mouse: bool,
    pub sound: bool,
    pub fullscreen: bool,
    pub voodoo: bool,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            edition: Edition::Win98,
            image: String::new(),
            executable: String::new(),
            memory_mb: 128,
            mouse: true,
            sound: true,
            fullscreen: false,
            voodoo: false,
        }
    }
}
impl Preferences {
    pub fn memory_choices(&self) -> &'static [u16] {
        if self.edition == Edition::Win98 {
            &[64, 128, 256, 512]
        } else {
            &[64, 128, 256]
        }
    }
    pub fn validate(&self) -> io::Result<()> {
        if !self.memory_choices().contains(&self.memory_mb) {
            return Err(io::Error::other("Choose a listed Windows memory size (Windows 95: up to 256 MiB; Windows 98: up to 512 MiB)."));
        }
        for p in [&self.image, &self.executable] {
            if p.len() > 4096 || p.chars().any(char::is_control) {
                return Err(io::Error::other(
                    "Invalid Windows disk image or DOSBox-X executable path.",
                ));
            }
        }
        if self
            .image
            .chars()
            .any(|c| matches!(c, '"' | '%' | '&' | '|' | '<' | '>'))
        {
            return Err(io::Error::other("The image path contains DOS command characters. Choose a path without quotes, percent signs, ampersands or redirection characters."));
        }
        Ok(())
    }
    fn configuration(&self) -> io::Result<String> {
        self.validate()?;
        Ok(format!(
            r#"[sdl]
output=default
fullscreen={}
autolock={}
[dosbox]
convertdrivefat=false
machine=svga_s3
memsize={}
locking disk image mount=true
[cpu]
cputype=pentium_mmx
core=normal
cycles=auto
[video]
vmemsize=8
[dos]
automount drive directories=false
startcmd=false
ver={}
[sblaster]
sbtype={}
[ide, primary]
int13fakeio=true
int13fakev86io=true
[ide, secondary]
int13fakeio=true
int13fakev86io=true
[ipx]
ipx=false
[serial]
serial1=disabled
serial2=disabled
serial3=disabled
serial4=disabled
serial5=disabled
serial6=disabled
serial7=disabled
serial8=disabled
serial9=disabled
[ne2000]
ne2000=false
[voodoo]
voodoo_card={}
"#,
            self.fullscreen,
            self.mouse,
            self.memory_mb,
            if self.edition == Edition::Win95 {
                "7.0"
            } else {
                "7.1"
            },
            if self.sound { "sb16vibra" } else { "none" },
            if self.voodoo { "auto" } else { "false" },
        ))
    }
}
pub(crate) const MAX_IMAGES: usize = 32;
pub(crate) fn validate_images(images: &[Preferences]) -> io::Result<()> {
    if images.len() > MAX_IMAGES {
        return Err(io::Error::other(
            "Windows profiles are limited to 32 images.",
        ));
    }
    let mut paths = std::collections::HashSet::new();
    for p in images {
        p.validate()?;
        if p.image.is_empty() || !paths.insert(&p.image) {
            return Err(io::Error::other(
                "Each Windows profile needs a different image path.",
            ));
        }
    }
    Ok(())
}
pub(crate) fn save_images(
    app: &mut App,
    b: crate::plugins::Binding,
    expected: Vec<Preferences>,
    current: Preferences,
    images: Vec<Preferences>,
) {
    let Owner::Document(origin) = b.owner else {
        return;
    };
    if b.plugin != DOS_LAUNCHER
        || app.desktop.plugins.binding(DOS_LAUNCHER) != Some(b)
        || !app.config.plugin_enabled(DOS_LAUNCHER)
    {
        return;
    }
    let result = (|| -> io::Result<Option<io::Error>> {
        if expected != app.config.dos.windows_images {
            return Err(io::Error::other(
                "Windows profiles changed. Reopen the launcher before saving.",
            ));
        }
        current.validate()?;
        validate_images(&images)?;
        let warning = app.config_store.update_preferences(|config| {
            if config.dos.windows_images != expected {
                return Err(io::Error::other(
                    "Windows profiles changed. Reopen the launcher before saving.",
                ));
            }
            config.dos.windows = current.clone();
            config.dos.windows_images = images.clone();
            Ok(())
        })?;
        app.config.dos.windows = current.clone();
        app.config.dos.windows_images = images.clone();
        Ok(warning)
    })();
    match result {
        Ok(warning) => {
            app.dos.view.notice = if let Some(error) = warning {
                format!(
                    "{}\n{error}",
                    app.config
                        .language
                        .text("Profile data is on disk; saving reported a warning.")
                )
            } else {
                "Windows profiles saved.".into()
            };
            app.notice = app.dos.view.notice.clone();
            crate::plugins::host::dispatch(
                app,
                b,
                crate::plugins::Event::WindowsImagesSaved { current, images },
            );
        }
        Err(error) => report(
            app,
            origin,
            app.desktop.activation_revision(),
            format!(
                "{}\n{error}",
                app.config
                    .language
                    .text("Cannot save Windows profiles. Your draft is retained.")
            ),
            true,
        ),
    }
}
fn image(p: &Preferences) -> io::Result<(PathBuf, crate::platform_fs::EntryIdentity)> {
    p.validate()?;
    let path = PathBuf::from(&p.image);
    if !path.is_absolute()
        || !path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("img") || e.eq_ignore_ascii_case("ima"))
    {
        return Err(io::Error::other("Choose an absolute path to an existing raw HDD image (.img or .ima). VHD/ISO/floppy installation is not supported here."));
    }
    let meta = fs::symlink_metadata(&path)?;
    if !meta.is_file()
        || meta.file_type().is_symlink()
        || meta.len() < 1024 * 1024
        || !meta.len().is_multiple_of(512)
        || meta.permissions().readonly()
    {
        return Err(io::Error::other("Select a writable regular raw HDD image of at least 1 MiB, with 512-byte sectors. Links, devices and read-only images are refused."));
    }
    let path = fs::canonicalize(path)?;
    let mut file = crate::filesystem::open_regular(&path)?;
    if !crate::platform_fs::same_identity(&meta, &file.metadata()?)
        || !crate::platform_fs::owns_file(&path, &file)
    {
        return Err(io::Error::other("Windows image changed before launch."));
    }
    let mut sector = [0; 512];
    file.read_exact(&mut sector)?;
    if sector[510..] != [0x55, 0xaa] {
        return Err(io::Error::other(
            "The image has no valid PC boot-sector signature.",
        ));
    }
    let mut partition = false;
    for entry in sector[446..510].as_chunks::<16>().0 {
        if entry[4] == 0 {
            continue;
        }
        let start = u64::from(u32::from_le_bytes(entry[8..12].try_into().unwrap()));
        let len = u64::from(u32::from_le_bytes(entry[12..16].try_into().unwrap()));
        if ![0x01, 0x04, 0x06, 0x0b, 0x0c, 0x0e].contains(&entry[4])
            || ![0, 0x80].contains(&entry[0])
            || start == 0
            || len == 0
            || start + len > meta.len() / 512
        {
            return Err(io::Error::other("The raw image has an unsupported or out-of-bounds partition. Use a partitioned FAT HDD image."));
        }
        partition = true;
    }
    if !partition {
        return Err(io::Error::other("Use a partitioned FAT HDD image; floppy and unpartitioned images are not supported here."));
    }
    let identity = crate::platform_fs::entry_identity(&path)?;
    Ok((path, identity))
}
fn tool(p: &Preferences, cancel: &crate::task::Cancellation) -> io::Result<(PathBuf, Verified)> {
    let preferences = super::Preferences {
        executable: p.executable.clone(),
        ..Default::default()
    };
    let mut candidates = discover(&preferences.executable);
    candidates.sort_by_key(|path| hint(path) != Some(Kind::X));
    let deadline = Instant::now() + Duration::from_secs(8);
    for candidate in candidates {
        cancel.check()?;
        if Instant::now() >= deadline {
            break;
        }
        if let Ok(v) = verify_until(&candidate, cancel, deadline) {
            if v.kind == Kind::X && v.version != "Version unavailable" {
                return Ok((candidate, v));
            }
        }
    }
    Err(io::Error::other("Windows 9x requires DOSBox-X. Install it or select its executable; DOSBox Staging and classic DOSBox are not used for this profile."))
}
fn boot_arguments(verified: &Verified, fragment: &Path, image: &str) -> Vec<std::ffi::OsString> {
    let mut args = profile::arguments(verified, fragment);
    args.extend([
        "-noautoexec".into(),
        "-c".into(),
        format!("IMGMOUNT C \"{image}\" -ide 1m").into(),
        "-c".into(),
        "BOOT C:".into(),
    ]);
    args
}
fn prepare_launch(
    p: Preferences,
    active: Vec<crate::platform_fs::EntryIdentity>,
    boot: bool,
    language: crate::localization::Language,
    cancel: crate::task::Cancellation,
) -> io::Result<Reply> {
    cancel.check()?;
    let (path, identity) = image(&p)?;
    let before = fs::metadata(&path)?;
    if active.contains(&identity) {
        return Err(io::Error::other(
            "This Windows disk image is already running in DN. Close its emulator first.",
        ));
    }
    let (executable, verified) = tool(&p, &cancel)?;
    let config = p.configuration()?;
    if !boot {
        return Ok(Reply::Checked(format!("{}\n{}\n{} · {} MiB\n{}\n{}\n{}",language.text("DOSBox-X and image header checked. Windows installation and successful guest boot are not verified."),crate::filesystem::display_name(&path.to_string_lossy()),p.edition.label(),p.memory_mb,verified.version,crate::filesystem::display_name(&executable.to_string_lossy()),config)));
    }
    #[cfg(target_os = "linux")]
    if std::env::var_os("DISPLAY").is_none_or(|v| v.is_empty())
        && std::env::var_os("WAYLAND_DISPLAY").is_none_or(|v| v.is_empty())
    {
        return Err(io::Error::other(
            "DOSBox needs a graphical display; a text console alone cannot show its window",
        ));
    }
    cancel.check()?;
    let (current, current_id) = image(&p)?;
    let after = fs::metadata(&current)?;
    if current != path
        || current_id != identity
        || before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
    {
        return Err(io::Error::other("Windows image changed before launch."));
    }
    let text = path.to_str().ok_or_else(|| {
        io::Error::other("This image path cannot be represented as a Unicode path.")
    })?;
    // IMGMOUNT is a DOSBox command, not a host shell. Reject DOS metacharacters
    // even after canonicalizing parent aliases, then keep the path quoted.
    let checked = Preferences {
        image: text.into(),
        ..p.clone()
    };
    checked.validate()?;
    let fragment = profile::Fragment::create(&config)?;
    let args = boot_arguments(&verified, &fragment.path, text);
    cancel.check()?;
    let mut child = process::Process::spawn_image(&executable, &path, &args, fragment)?;
    child.windows_identity = Some(identity);
    Ok(Reply::WindowsStarted(Box::new(child)))
}

pub(crate) fn request(app: &mut App, b: crate::plugins::Binding, p: Preferences, boot: bool) {
    let Owner::Document(origin) = b.owner else {
        return;
    };
    if b.plugin != DOS_LAUNCHER
        || app.desktop.plugins.binding(DOS_LAUNCHER) != Some(b)
        || !app.config.plugin_enabled(DOS_LAUNCHER)
        || app.dos.pending.is_some()
    {
        return;
    }
    if app.dos.children.len() >= 32 || app.blocking_operation() {
        app.dos.view.notice = "Wait for the current operation before launching DOSBox.".into();
        return;
    }
    let active: Vec<_> = app
        .dos
        .children
        .iter()
        .filter_map(|r| r.process.windows_identity)
        .collect();
    let language = app.config.language;
    let task = app.workers.spawn(
        "dn-windows-boot",
        &app.dos.requests,
        crate::requests::Owner::Workspace,
        "dos-launch",
        move |cancel| prepare_launch(p, active, boot, language, cancel),
    );
    match task {
        Ok(task) => {
            app.dos.pending = Some(Pending {
                task,
                path: PathBuf::new(),
                panel: app.active,
                epoch: app.desktop.activation_revision(),
                automatic: false,
                origin,
                owner: Some(b),
            });
            app.dos.view.busy = true;
            app.dos.view.notice = "Checking disk image and DOSBox-X…".into();
        }
        Err(e) => report(
            app,
            origin,
            app.desktop.activation_revision(),
            e.to_string(),
            true,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    fn fixture() -> io::Result<(crate::network::workspace::Workspace, Preferences)> {
        let w = crate::network::workspace::Workspace::create(std::env::temp_dir())?;
        let path = w.root.join("Windows Ёжик image.img");
        let mut file = File::create(&path)?;
        file.set_len(2 * 1024 * 1024)?;
        let mut sector = [0; 512];
        sector[510..].copy_from_slice(&[0x55, 0xaa]);
        sector[446] = 0x80;
        sector[450] = 0x06;
        sector[454..458].copy_from_slice(&1u32.to_le_bytes());
        sector[458..462].copy_from_slice(&4095u32.to_le_bytes());
        file.write_all(&sector)?;
        drop(file);
        let p = Preferences {
            image: path.to_str().unwrap().into(),
            ..Default::default()
        };
        Ok((w, p))
    }
    #[test]
    fn raw_partitioned_image_checks_are_bounded_and_do_not_write() -> io::Result<()> {
        let (_w, mut p) = fixture()?;
        let before = fs::read(&p.image)?;
        let (path, id) = image(&p)?;
        assert_eq!(fs::read(&path)?, before);
        assert_eq!(id, crate::platform_fs::entry_identity(&path)?);
        let mut bad = before.clone();
        bad[450] = 0x07;
        fs::write(&path, &bad)?;
        assert!(image(&p).unwrap_err().to_string().contains("partition"));
        bad[450] = 0x06;
        bad[458..462].copy_from_slice(&u32::MAX.to_le_bytes());
        fs::write(&path, &bad)?;
        assert!(image(&p).is_err());
        bad.fill(0);
        fs::write(&path, &bad)?;
        assert!(image(&p).is_err());
        p.image = "relative.img".into();
        assert!(image(&p).is_err());
        p.image = "bad%PATH%.img".into();
        assert!(p.validate().is_err());
        p.image = "safe.img\nBOOT C:".into();
        assert!(p.validate().is_err());
        Ok(())
    }
    #[cfg(unix)]
    #[test]
    fn image_links_and_read_only_disks_are_refused() -> io::Result<()> {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let (w, p) = fixture()?;
        let alias = w.root.join("alias.img");
        symlink(&p.image, &alias)?;
        assert!(image(&Preferences {
            image: alias.to_str().unwrap().into(),
            ..p.clone()
        })
        .is_err());
        fs::set_permissions(&p.image, fs::Permissions::from_mode(0o400))?;
        assert!(image(&p).is_err());
        Ok(())
    }
    #[test]
    fn profiles_and_boot_arguments_are_separate_from_dos_launches() -> io::Result<()> {
        let mut p = Preferences::default();
        let conf = p.configuration()?;
        for required in [
            "core=normal",
            "cycles=auto",
            "cputype=pentium_mmx",
            "memsize=128",
            "convertdrivefat=false",
            "locking disk image mount=true",
            "ne2000=false",
            "ipx=false",
            "serial1=disabled",
            "automount drive directories=false",
            "voodoo_card=false",
            "ver=7.1",
        ] {
            assert!(conf.contains(required), "{required}");
        }
        p.edition = Edition::Win95;
        p.memory_mb = 512;
        assert!(p.validate().is_err());
        p.memory_mb = 64;
        p.voodoo = true;
        let conf = p.configuration()?;
        assert!(conf.contains("ver=7.0"));
        assert!(conf.contains("voodoo_card=auto"));
        let v = Verified {
            kind: Kind::X,
            flag: "-version",
            version: "DOSBox-X 2026.10.01".into(),
        };
        let args = boot_arguments(
            &v,
            Path::new("private fragment.conf"),
            "/Windows Ёжик/disk.img",
        );
        let text: Vec<_> = args.iter().map(|v| v.to_str().unwrap()).collect();
        assert_eq!(
            &text[text.len() - 5..],
            [
                "-noautoexec",
                "-c",
                "IMGMOUNT C \"/Windows Ёжик/disk.img\" -ide 1m",
                "-c",
                "BOOT C:"
            ]
        );
        let command = process::command_inner(
            Path::new("dosbox-x"),
            Path::new("/Windows Ёжик/disk.img"),
            &args,
            false,
        );
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            args.iter().map(|v| v.as_os_str()).collect::<Vec<_>>()
        );
        assert!(super::super::Profile::default().render(&v)?.is_empty());
        Ok(())
    }
    #[cfg(unix)]
    #[test]
    fn windows_verification_refuses_other_emulators_without_launching() -> io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let (w, mut p) = fixture()?;
        let executable = w.root.join("mock DOSBox");
        p.executable = executable.to_str().unwrap().into();
        for name in [
            "DOSBox Staging 0.83.0",
            "DOSBox version 0.74-3",
            "DOSBox-X 2026.10.01",
        ] {
            fs::write(&executable, format!("#!/bin/sh\necho '{name}'\n"))?;
            fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))?;
            let result = tool(&p, &Default::default());
            assert_eq!(result.is_ok(), name.starts_with("DOSBox-X"));
        }
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod lifecycle_tests {
    use super::*;
    use std::{io::Write, os::unix::fs::PermissionsExt};
    fn settle(app: &mut App, ready: impl Fn(&App) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(8);
        while !ready(app) {
            super::super::poll(app);
            assert!(Instant::now() < deadline, "Windows worker did not finish");
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    #[test]
    fn image_boot_check_duplicate_guard_and_stale_owner_keep_drafts_and_focus() -> io::Result<()> {
        // Only the private mock child receives a graphical-session marker. Never
        // mutate the parallel test process environment or require a real display.
        #[cfg(target_os = "linux")]
        if std::env::var_os("DN_WIN9X_MOCK_CHILD").is_none() {
            let output = Command::new(std::env::current_exe()?)
                .args(["--exact", "dos::windows::lifecycle_tests::image_boot_check_duplicate_guard_and_stale_owner_keep_drafts_and_focus", "--nocapture"])
                .env("DN_WIN9X_MOCK_CHILD", "1")
                .env("DISPLAY", ":dn-private-mock")
                .output()?;
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return Ok(());
        }
        let w = crate::network::workspace::Workspace::create(std::env::temp_dir())?;
        let disk = w.root.join("Windows Ёжик.img");
        let mut f = File::create(&disk)?;
        f.set_len(2 * 1024 * 1024)?;
        let mut sector = [0; 512];
        sector[510..].copy_from_slice(&[0x55, 0xaa]);
        sector[446] = 0x80;
        sector[450] = 6;
        sector[454..458].copy_from_slice(&1u32.to_le_bytes());
        sector[458..462].copy_from_slice(&4095u32.to_le_bytes());
        f.write_all(&sector)?;
        drop(f);
        let executable = w.root.join("mock dosbox-x");
        fs::write(
            &executable,
            r#"#!/bin/sh
case "${2:-$1}" in --version|-version) echo 'DOSBox-X 2026.10.01'; exit 0;; esac
printf '%s\n' "$@" > observed-args
prev=''
for arg in "$@"; do
 if [ "$prev" = '-conf' ] && [ -f "$arg" ]; then cat "$arg" > observed-config; printf '%s' "$arg" > fragment-path; fi
 prev="$arg"
done
n=0
while [ ! -f release ] && [ "$n" -lt 10 ]; do sleep 1; n=$((n+1)); done
"#,
        )?;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))?;
        let mut app = App::new(w.root.clone())?;
        app.config = crate::config::Config::default();
        app.config.save_on_exit = false;
        app.config.set_plugin_enabled(DOS_LAUNCHER, true);
        app.config_store = crate::config::Store::new(Some(w.root.join("private.conf")));
        crate::plugins::host::open(&mut app, DOS_LAUNCHER);
        let binding = app.desktop.plugins.binding(DOS_LAUNCHER).unwrap();
        let original = app.config.dos.clone();
        let p = Preferences {
            image: disk.to_str().unwrap().into(),
            executable: executable.to_str().unwrap().into(),
            ..Default::default()
        };
        request(&mut app, binding, p.clone(), false);
        settle(&mut app, |a| !a.dos.view.busy);
        assert!(app.last_dialog().unwrap().message.contains("not verified"));
        assert!(!w.root.join("observed-args").exists());
        app.pop_dialog();
        request(&mut app, binding, p.clone(), true);
        settle(&mut app, |a| !a.dos.children.is_empty());
        let deadline = Instant::now() + Duration::from_secs(5);
        while !fs::read_to_string(w.root.join("fragment-path"))
            .is_ok_and(|v| !v.is_empty() && Path::new(&v).exists())
        {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        let fragment = PathBuf::from(fs::read_to_string(w.root.join("fragment-path"))?);
        assert!(fragment.exists());
        let args = fs::read_to_string(w.root.join("observed-args"))?;
        assert!(args.contains("-noautoexec\n-c\nIMGMOUNT C \""));
        assert!(args.ends_with("BOOT C:\n"));
        assert!(!args.ends_with(&format!("{}\n", disk.display())));
        assert!(fs::read_to_string(w.root.join("observed-config"))?.contains("core=normal"));
        assert_eq!(app.config.dos, original);
        assert_eq!(fs::read(&disk)?[..512], sector);
        request(&mut app, binding, p.clone(), true);
        settle(&mut app, |a| !a.dos.view.busy);
        assert!(app
            .last_dialog()
            .unwrap()
            .message
            .contains("already running"));
        app.pop_dialog();
        fs::write(w.root.join("release"), b"done")?;
        settle(&mut app, |a| a.dos.children.is_empty());
        assert!(!fragment.exists());
        let Owner::Document(owner) = binding.owner else {
            unreachable!()
        };
        app.desktop.close(owner);
        request(&mut app, binding, p, false);
        assert!(app.dos.pending.is_none());
        assert_eq!(app.config.dos, original);
        Ok(())
    }
}
