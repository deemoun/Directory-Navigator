//! Explicit DOSBox Staging integration. No shell, bundled emulator, or startup process.
use crate::{
    app::App,
    plugins::{Owner, DOS_LAUNCHER},
};
use std::{
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Preferences {
    pub open_on_enter: bool,
    pub executable: String,
}
#[derive(Clone, Default)]
pub(crate) struct Snapshot {
    pub notice: String,
    pub busy: bool,
}
struct Pending {
    task: crate::task::Task<Result<Reply, String>>,
    path: PathBuf,
    panel: usize,
    epoch: u64,
    automatic: bool,
    owner: Option<crate::plugins::Binding>,
}
enum Reply {
    NotDos,
    Started(Child),
}
#[derive(Default)]
pub(crate) struct State {
    pending: Option<Pending>,
    children: Vec<Child>,
    pub view: Snapshot,
}
pub(crate) fn candidate(path: &Path) -> bool {
    path.extension().is_some_and(|e| {
        ["com", "exe", "bat"]
            .iter()
            .any(|x| e.eq_ignore_ascii_case(x))
    })
}
fn identify(file: &mut File, path: &Path, automatic: bool) -> io::Result<bool> {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if extension == "bat" {
        return Ok(!automatic);
    }
    if !matches!(extension.as_str(), "com" | "exe") {
        return Ok(false);
    }
    let mut header = [0u8; 64];
    let length = file.read(&mut header)?;
    if length >= 2 && matches!(&header[..2], b"MZ" | b"ZM") {
        if length < 28 {
            return Err(io::Error::other("Truncated DOS executable header"));
        }
        if length >= 64 {
            let offset = u32::from_le_bytes(header[60..64].try_into().unwrap());
            if offset >= 64 && u64::from(offset) < file.metadata()?.len() {
                file.seek(SeekFrom::Start(offset.into()))?;
                let mut signature = [0; 4];
                let n = file.read(&mut signature)?;
                if n >= 2 && matches!(&signature[..2], b"PE" | b"NE" | b"LE" | b"LX") {
                    return Ok(false);
                }
            }
        }
        return Ok(true);
    }
    Ok(extension == "com"
        && length > 0
        && !header.starts_with(b"\x7fELF")
        && !header.starts_with(b"#!"))
}
fn output(
    executable: &Path,
    argument: &str,
    cancel: &crate::task::Cancellation,
) -> io::Result<String> {
    // A regular private capture avoids pipe-buffer deadlocks in long --help output.
    // Reading it never waits for a descendant which inherited a stdout pipe.
    let mut capture =
        crate::publication::StagedFile::create(&std::env::temp_dir().join("dn-dos-verification"))?;
    let mut child = Command::new(executable)
        .arg("--nolocalconf")
        .arg(argument)
        .stdin(Stdio::null())
        .stdout(Stdio::from(capture.file.try_clone()?))
        .stderr(Stdio::from(capture.file.try_clone()?))
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(3);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if cancel.cancelled()
            || Instant::now() >= deadline
            || capture.file.metadata()?.len() > 65_536
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::other(
                "DOSBox verification cancelled or timed out",
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    if capture.file.metadata()?.len() > 65_536 {
        return Err(io::Error::other(
            "DOSBox verification output exceeds its limit",
        ));
    }
    capture.file.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    (&mut capture.file).take(65_536).read_to_end(&mut bytes)?;
    if !status.success() {
        return Err(io::Error::other(
            "DOSBox verification failed; choose an installed DOSBox Staging executable",
        ));
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}
fn host_shell_script(path: &Path) -> bool {
    cfg!(windows)
        && path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("bat") || e.eq_ignore_ascii_case("cmd"))
}
fn verify(executable: &Path, cancel: &crate::task::Cancellation) -> io::Result<()> {
    // Windows Command would otherwise route a batch wrapper through cmd.exe.
    if host_shell_script(executable) {
        return Err(io::Error::other(
            "Choose the DOSBox Staging executable, not a BAT/CMD wrapper",
        ));
    }
    let banner = output(executable, "--version", cancel)?;
    if !banner.lines().any(|line| {
        let line = line.trim_start().to_ascii_lowercase();
        line.starts_with("dosbox staging") || line.starts_with("dosbox-staging")
    }) {
        return Err(io::Error::other(
            "Choose DOSBox Staging; this executable did not identify itself as Staging",
        ));
    }
    if !output(executable, "--help", cancel)?.contains("--nolocalconf") {
        return Err(io::Error::other(
            "Incompatible DOSBox Staging: install a version supporting --nolocalconf",
        ));
    }
    Ok(())
}
fn launch_command(executable: &Path, path: &Path) -> Command {
    let mut command = Command::new(executable);
    command
        .arg("--nolocalconf")
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

fn discover(configured: &str) -> Vec<PathBuf> {
    if !configured.trim().is_empty() {
        return vec![PathBuf::from(configured)];
    }
    let mut paths = Vec::new();
    for folder in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        for name in if cfg!(windows) {
            ["dosbox-staging.exe", "dosbox.exe"]
        } else {
            ["dosbox-staging", "dosbox"]
        } {
            let path = folder.join(name);
            if path.is_file() {
                paths.push(path);
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        let path = PathBuf::from("/Applications/DOSBox Staging.app/Contents/MacOS/dosbox");
        if path.is_file() {
            paths.push(path);
        }
    }
    paths.truncate(16);
    paths
}
fn launch(
    path: &Path,
    preferences: &Preferences,
    captured: &fs::Metadata,
    automatic: bool,
    cancel: &crate::task::Cancellation,
) -> Result<Reply, String> {
    let execute = || -> io::Result<Reply> {
        cancel.check()?;
        let mut file = crate::filesystem::open_regular(path)?;
        let metadata = file.metadata()?;
        if !crate::platform_fs::same_identity(captured, &metadata)
            || captured.len() != metadata.len()
            || captured.modified().ok() != metadata.modified().ok()
        {
            return Err(io::Error::other(
                "File changed before DOSBox launch; refresh and try again",
            ));
        }
        if !identify(&mut file, path, automatic)? {
            return Ok(Reply::NotDos);
        }
        #[cfg(target_os = "linux")]
        if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
            return Err(io::Error::other(
                "DOSBox needs a graphical display; a text console alone cannot show its window",
            ));
        }
        let mut failure =
            "Install DOSBox Staging or set its executable path in the DOS launcher".to_owned();
        let mut found = None;
        for executable in discover(&preferences.executable) {
            cancel.check()?;
            match verify(&executable, cancel) {
                Ok(()) => {
                    found = Some(executable);
                    break;
                }
                Err(error) => failure = error.to_string(),
            }
        }
        let executable = found.ok_or_else(|| io::Error::other(failure))?;
        cancel.check()?;
        let current = fs::symlink_metadata(path)?;
        if !crate::platform_fs::same_identity(&metadata, &current)
            || metadata.len() != current.len()
            || metadata.modified().ok() != current.modified().ok()
        {
            return Err(io::Error::other(
                "File changed before DOSBox launch; refresh and try again",
            ));
        }
        let child = launch_command(&executable, path).spawn()?;
        Ok(Reply::Started(child))
    };
    execute().map_err(|e| e.to_string())
}
pub(crate) fn open(app: &mut App, path: PathBuf, automatic: bool) -> bool {
    if !app.config.plugin_enabled(DOS_LAUNCHER)
        || (automatic && !app.config.dos.open_on_enter)
        || !candidate(&path)
    {
        return false;
    }
    if app.dos.pending.is_some() || app.dos.children.len() >= 32 || app.blocking_operation() {
        app.notice = "Wait for the current operation before launching DOSBox.".into();
        return true;
    }
    let captured = match fs::symlink_metadata(&path) {
        Ok(m) if m.is_file() && !m.file_type().is_symlink() => m,
        Ok(_) => {
            app.notice = "Select a regular DOS program; links are not launched internally.".into();
            return true;
        }
        Err(e) => {
            app.notice = e.to_string();
            return true;
        }
    };
    let preferences = app.config.dos.clone();
    let source = path.clone();
    match crate::task::Task::spawn("dn-dos-launch", move |cancel| {
        launch(&source, &preferences, &captured, automatic, &cancel)
    }) {
        Ok(task) => {
            app.dos.pending = Some(Pending {
                task,
                path,
                panel: app.active,
                epoch: app.desktop.activation_revision(),
                automatic,
                owner: app.desktop.plugins.binding(DOS_LAUNCHER),
            });
            app.dos.view.busy = true;
            app.dos.view.notice = "Checking DOS program and DOSBox Staging…".into();
        }
        Err(error) => app.dos.view.notice = error.to_string(),
    }
    true
}
pub(crate) fn current(app: &mut App) {
    if !crate::archive::require_native(app) {
        return;
    }
    let panel = &app.panels[app.active];
    let Some(entry) = panel
        .entries
        .get(panel.cursor)
        .filter(|e| !e.parent && !e.directory && !e.symlink)
    else {
        app.error("Select a regular DOS program first");
        return;
    };
    let path = entry.path.clone();
    if !open(app, path, false) {
        app.error("Enable the experimental DOS launcher and choose a COM, DOS EXE or BAT file");
    }
}
pub(crate) fn poll(app: &mut App) -> bool {
    let mut failed = false;
    app.dos.children.retain_mut(|child| match child.try_wait() {
        Ok(Some(status)) => {
            failed |= !status.success();
            false
        }
        Ok(None) => true,
        Err(_) => false,
    });
    if failed {
        app.notice =
            "DOSBox exited unsuccessfully; check graphical output and its global configuration."
                .into();
        app.dos.view.notice = app.notice.clone();
    }
    if !app.config.plugin_enabled(DOS_LAUNCHER)
        || app.dos.pending.as_ref().is_some_and(|p| {
            p.owner
                .is_some_and(|b| app.desktop.plugins.binding(DOS_LAUNCHER) != Some(b))
        })
    {
        app.dos.pending = None;
        app.dos.view.busy = false;
    }
    let result = app
        .dos
        .pending
        .as_ref()
        .and_then(|p| match p.task.try_recv() {
            Ok(result) => Some(result),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("DOS launcher task ended unexpectedly".into())),
        });
    let Some(result) = result else {
        return failed;
    };
    let pending = app.dos.pending.take().unwrap();
    app.dos.view.busy = false;
    match result {
        Ok(Reply::Started(child)) => {
            app.dos.children.push(child);
            app.dos.view.notice = "DOSBox launched in its own window.".into();
        }
        Ok(Reply::NotDos) => {
            app.dos.view.notice = "This is not an eligible DOS program.".into();
            if pending.automatic
                && app.active == pending.panel
                && app.desktop.activation_revision() == pending.epoch
                && app.panels[app.active]
                    .entries
                    .get(app.panels[app.active].cursor)
                    .is_some_and(|e| e.path == pending.path)
                && app.desktop.active().id == crate::desktop::WindowId::MANAGER
            {
                app.dos.view.notice.clear();
                app.open_current_after_dos();
                return true;
            }
            if pending.automatic {
                app.dos.view.notice.clear();
                return true;
            }
        }
        Err(error) => app.dos.view.notice = error,
    }
    app.notice = app.dos.view.notice.clone();
    true
}
pub(crate) fn setup(app: &mut App, b: crate::plugins::Binding, preferences: Preferences) {
    if b.plugin != DOS_LAUNCHER
        || !matches!(b.owner,Owner::Document(id) if app.desktop.contains(id))
    {
        return;
    }
    if preferences.executable.len() > 4096 || preferences.executable.chars().any(char::is_control) {
        app.dos.view.notice = "Invalid DOSBox executable path".into();
        return;
    }
    app.config.dos = preferences;
    app.dos.view.notice = "DOS launcher preferences applied.".into();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> io::Result<Self> {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "dn-dos-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path)?;
            Ok(Self(fs::canonicalize(path)?))
        }
        fn program(&self, name: &str, bytes: &[u8], automatic: bool) -> io::Result<bool> {
            let p = self.0.join(name);
            fs::write(&p, bytes)?;
            identify(&mut crate::filesystem::open_regular(&p)?, &p, automatic)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn dos_identification_excludes_native_executables_and_ambiguous_batch() -> io::Result<()> {
        let f = Fixture::new()?;
        assert!(f.program("Ёжик.CoM", b"\xb4\x4c\xcd\x21", true)?);
        assert!(!f.program("empty.com", b"", true)?);
        assert!(!f.program("shell.com", b"#!/bin/sh", true)?);
        assert!(!f.program("linux.com", b"\x7fELF", true)?);
        assert!(!f.program("run.BAT", b"echo hi", true)?);
        assert!(f.program("run.BAT", b"echo hi", false)?);
        assert!(!f.program("native.exe", b"other", false)?);
        let mut header = vec![0; 100];
        header[..2].copy_from_slice(b"MZ");
        header[60..64].copy_from_slice(&80u32.to_le_bytes());
        assert!(f.program("dos.EXE", &header, true)?);
        for signature in [b"PE", b"NE", b"LE", b"LX"] {
            header[80..82].copy_from_slice(signature);
            assert!(!f.program("windows.exe", &header, true)?);
        }
        assert!(f.program("short.exe", b"MZ", true).is_err());
        Ok(())
    }
    #[test]
    fn windows_batch_wrappers_cannot_introduce_a_host_shell() {
        assert_eq!(host_shell_script(Path::new("DOSBox.CMD")), cfg!(windows));
        assert_eq!(host_shell_script(Path::new("DOSBox.BAT")), cfg!(windows));
        assert!(!host_shell_script(Path::new("DOSBox.exe")));
    }
    #[test]
    fn arguments_are_literal_and_local_config_is_disabled() {
        let path = Path::new("/games/Ёжик ; $(unsafe)/run.COM");
        let command = launch_command(Path::new("/apps/DOSBox Staging"), path);
        assert_eq!(command.get_program(), "/apps/DOSBox Staging");
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [std::ffi::OsStr::new("--nolocalconf"), path.as_os_str()]
        );
    }
    #[cfg(unix)]
    #[test]
    fn verifies_staging_capability_with_private_mock() -> io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let f = Fixture::new()?;
        let executable = f.0.join("DOSBox Staging");
        fs::write(&executable,"#!/bin/sh\ncase \"$2\" in --version) echo 'DOSBox Staging 0.83.0';; --help) echo '--nolocalconf';; *) exit 1;; esac\n")?;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))?;
        verify(&executable, &Default::default())?;
        let script=format!("#!/bin/sh\ncase \"$2\" in --version) echo 'DOSBox Staging 0.83.0';; --help) printf '%s\\n' '--nolocalconf' '{}';; esac\n","x".repeat(32_768));
        fs::write(&executable, script)?;
        verify(&executable, &Default::default())?;
        fs::write(&executable, "#!/bin/sh\necho 'DOSBox classic'\n")?;
        assert!(verify(&executable, &Default::default()).is_err());
        Ok(())
    }
    #[test]
    fn changed_source_is_rejected_before_discovery() -> io::Result<()> {
        let f = Fixture::new()?;
        let path = f.0.join("game.com");
        fs::write(&path, b"\xcd\x20")?;
        let captured = fs::metadata(&path)?;
        fs::write(&path, b"changed length")?;
        let result = launch(
            &path,
            &Preferences::default(),
            &captured,
            false,
            &Default::default(),
        );
        assert!(matches!(result,Err(ref e) if e.contains("changed")));
        Ok(())
    }
}
