//! Explicit DOSBox-family integration. No shell, bundled emulator, or startup process.
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
mod metadata;
pub(crate) mod mount;
mod process;
mod profile;
pub(crate) mod windows;
pub use profile::{CpuSpeed, DisplayMode, Profile, Resolution};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Preferences {
    pub open_on_enter: bool,
    pub executable: String,
    pub profile: Profile,
    pub mount: mount::Preferences,
    pub windows: windows::Preferences,
    pub windows_images: Vec<windows::Preferences>,
}
#[derive(Clone, Default)]
pub(crate) struct Snapshot {
    pub notice: String,
    pub busy: bool,
    pub generation: u64,
    pub tools: Vec<Tool>,
    pub last_error: String,
}
struct Pending {
    task: crate::workers::Worker<Reply>,
    path: PathBuf,
    panel: usize,
    epoch: u64,
    automatic: bool,
    origin: crate::desktop::WindowId,
    owner: Option<crate::plugins::Binding>,
}
enum Reply {
    NotDos,
    Started(Box<process::Process>),
    WindowsStarted(Box<process::Process>),
    Checked(String),
    Discovered(Vec<Tool>),
}
#[derive(Default)]
pub(crate) struct State {
    requests: crate::requests::RequestScope,
    pending: Option<Pending>,
    children: Vec<Running>,
    pub view: Snapshot,
}
struct Running {
    process: Box<process::Process>,
    origin: crate::desktop::WindowId,
    epoch: u64,
}
struct ProbeChild(Child);
impl Drop for ProbeChild {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Staging,
    X,
    Classic,
}
impl Kind {
    pub(crate) fn caption(self) -> &'static str {
        match self {
            Self::Staging => "DOSBox Staging",
            Self::X => "DOSBox-X",
            Self::Classic => "DOSBox",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Verified {
    kind: Kind,
    flag: &'static str,
    version: String,
}
#[derive(Clone)]
pub(crate) struct Tool {
    pub executable: PathBuf,
    verified: Verified,
}
impl Tool {
    pub(crate) fn label(&self) -> String {
        format!(
            "{} · {} — {}",
            self.verified.kind.caption(),
            self.verified.version,
            crate::filesystem::display_name(&self.executable.to_string_lossy())
        )
    }
}

pub(crate) fn active(app: &App) -> bool {
    app.dos.pending.is_some() || !app.dos.children.is_empty()
}
pub(crate) fn candidate(path: &Path) -> bool {
    path.extension().is_some_and(|e| {
        ["com", "exe", "bat"]
            .iter()
            .any(|x| e.eq_ignore_ascii_case(x))
    })
}
/// Classification only, called by the background system opener; never probes an emulator.
pub(crate) fn program_for_open_hint(path: &Path) -> bool {
    candidate(path)
        && crate::filesystem::open_regular(path)
            .and_then(|mut file| identify(&mut file, path, true))
            .unwrap_or(false)
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
    flag: &str,
    deadline: Instant,
    cancel: &crate::task::Cancellation,
) -> io::Result<String> {
    // A regular private capture avoids pipe-buffer deadlocks in long --help output.
    // Reading it never waits for a descendant which inherited a stdout pipe.
    let workspace = crate::network::workspace::Workspace::create(std::env::temp_dir())?;
    let mut capture = crate::publication::StagedFile::create(&workspace.root.join("verification"))?;
    cancel.check()?;
    if Instant::now() >= deadline {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "DOSBox verification cancelled or timed out",
        ));
    }
    let mut command = Command::new(executable);
    if !flag.is_empty() {
        command.arg(flag);
    }
    let mut child = ProbeChild(
        command
            .arg(argument)
            .current_dir(&workspace.root)
            .stdin(Stdio::null())
            .stdout(Stdio::from(capture.file.try_clone()?))
            .stderr(Stdio::from(capture.file.try_clone()?))
            .spawn()?,
    );
    let deadline = deadline.min(Instant::now() + Duration::from_secs(3));
    let status = loop {
        if let Some(status) = child.0.try_wait()? {
            break status;
        }
        if cancel.cancelled()
            || Instant::now() >= deadline
            || capture.file.metadata()?.len() > 65_536
        {
            let _ = child.0.kill();
            let _ = child.0.wait();
            cancel.check()?;
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
    for name in ["stdout.txt", "stderr.txt"] {
        let path = workspace.root.join(name);
        if path.is_file() {
            if let Ok(file) = crate::filesystem::open_regular(&path) {
                file.take(65_536).read_to_end(&mut bytes)?;
            }
        }
    }
    let text = String::from_utf8_lossy(&bytes).into_owned();
    if !status.success() && !version_query_exit_one(argument, status.code(), &text) {
        return Err(io::Error::other(format!(
            "DOSBox verification failed ({status}): {text}"
        )));
    }
    Ok(text)
}
/// DOSBox-X's version command can deliberately return 1 after a valid banner.
/// Accept that query result only, never help failures, crashes or unversioned errors.
fn version_query_exit_one(argument: &str, code: Option<i32>, text: &str) -> bool {
    if code != Some(1) || !matches!(argument, "-version" | "--version") {
        return false;
    }
    text.lines().any(|line| {
        let lower = line.trim_start().to_ascii_lowercase();
        let Some(rest) = lower
            .strip_prefix("dosbox-x")
            .or_else(|| lower.strip_prefix("dosbox x"))
        else {
            return false;
        };
        if !rest.starts_with(char::is_whitespace) {
            return false;
        }
        let rest = rest.trim_start();
        let rest = rest
            .strip_prefix("version")
            .unwrap_or(rest)
            .trim_start_matches([' ', '\t', ':']);
        let token = rest
            .split_ascii_whitespace()
            .next()
            .unwrap_or("")
            .trim_end_matches(',');
        let core = token.split('-').next().unwrap_or("");
        let parts = core.split('.');
        parts.clone().count() >= 2
            && parts
                .into_iter()
                .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
    })
}

fn host_shell_script(path: &Path) -> bool {
    cfg!(windows)
        && path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("bat") || e.eq_ignore_ascii_case("cmd"))
}
#[cfg(all(test, unix))]
fn verify(executable: &Path, cancel: &crate::task::Cancellation) -> io::Result<Verified> {
    verify_until(executable, cancel, Instant::now() + Duration::from_secs(8))
}
fn verify_until(
    executable: &Path,
    cancel: &crate::task::Cancellation,
    deadline: Instant,
) -> io::Result<Verified> {
    verify_probe(executable, cancel, deadline, true)
}
fn verify_probe(
    executable: &Path,
    cancel: &crate::task::Cancellation,
    deadline: Instant,
    allow_unverified: bool,
) -> io::Result<Verified> {
    if host_shell_script(executable) {
        return Err(io::Error::other(
            "Choose a DOSBox executable, not a BAT/CMD wrapper",
        ));
    }
    // DOSBox-X's Windows GUI can replace redirected streams with its own
    // console during -version. Prefer read-only PE metadata on that platform.
    if let Some(version) = metadata::read(executable) {
        if let Some(kind) = classify(&version) {
            return Ok(Verified {
                kind,
                flag: if kind == Kind::Staging {
                    "--nolocalconf"
                } else {
                    ""
                },
                version: version
                    .lines()
                    .take(3)
                    .map(|line| line.chars().take(160).collect::<String>())
                    .collect::<Vec<_>>()
                    .join(" "),
            });
        }
    }
    let hinted = hint(executable);
    let arguments = if hinted == Some(Kind::Staging) {
        ["--version", "-version"]
    } else {
        ["-version", "--version"]
    };
    let mut banner = None;
    let mut unrecognized = None;
    let mut failure = None;
    for argument in arguments {
        match output(executable, argument, "", deadline, cancel) {
            Ok(text) if classify(&text).is_some() => {
                banner = Some(text);
                break;
            }
            Ok(text) if !text.trim().is_empty() => {
                unrecognized = Some(text);
            }
            Ok(_) => (),
            Err(error) => {
                cancel.check()?;
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied
                ) {
                    return Err(error);
                }
                failure = Some(error);
            }
        }
    }
    let Some(banner) = banner else {
        if let Some(text) = unrecognized {
            return Err(io::Error::other(format!(
                "Not a supported DOSBox emulator.\n{}",
                text.trim()
            )));
        }
        // Some GUI builds do not return version text through redirected handles.
        // Keep known installed entry points usable and identify the limitation.
        if let Some(kind) = hinted.filter(|_| allow_unverified) {
            return Ok(Verified {
                kind,
                flag: if kind == Kind::Staging {
                    "--nolocalconf"
                } else {
                    ""
                },
                version: "Version unavailable".into(),
            });
        }
        return Err(failure.unwrap_or_else(|| io::Error::other("Not a supported DOSBox emulator")));
    };
    let kind = classify(&banner).unwrap();
    let version = banner
        .lines()
        .find(|line| classify(line).is_some())
        .unwrap_or("")
        .trim()
        .chars()
        .take(160)
        .collect();
    let flag = if kind == Kind::Staging {
        let mut flag = None;
        for argument in ["--help", "-help"] {
            if let Ok(help) = output(executable, argument, "", deadline, cancel) {
                flag = local_config_flag(&help);
                if flag.is_some() {
                    break;
                }
            }
            cancel.check()?;
        }
        flag.unwrap_or("--nolocalconf")
    } else {
        ""
    };
    Ok(Verified {
        kind,
        flag,
        version,
    })
}
fn classify(text: &str) -> Option<Kind> {
    text.lines().find_map(|line| {
        let line = line.trim_start().to_ascii_lowercase();
        if line.starts_with("dosbox-x") || line.starts_with("dosbox x") {
            Some(Kind::X)
        } else if line.starts_with("dosbox staging") || line.starts_with("dosbox-staging") {
            Some(Kind::Staging)
        } else if line == "dosbox" || line.starts_with("dosbox ") || line.starts_with("dosbox,") {
            Some(Kind::Classic)
        } else {
            None
        }
    })
}
fn hint(path: &Path) -> Option<Kind> {
    let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
    let name = name.strip_suffix(".exe").unwrap_or(&name);
    if name.contains("staging") {
        Some(Kind::Staging)
    } else if name == "dosbox-x" || name == "dosbox_x" || name == "com.dosbox_x.dosbox-x" {
        Some(Kind::X)
    } else if name == "dosbox" || name == "com.dosbox.dosbox" {
        Some(Kind::Classic)
    } else {
        None
    }
}
fn flags(verified: &Verified) -> Vec<&'static str> {
    let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
    match verified.kind {
        Kind::Staging => vec![verified.flag],
        Kind::Classic => vec!["-userconf", "-conf", null],
        Kind::X => vec!["-userconf", "-conf", null, "-nopromptfolder"],
    }
}

fn local_config_flag(help: &str) -> Option<&'static str> {
    ["--nolocalconf", "-nolocalconf"].into_iter().find(|flag| {
        help.lines().any(|line| {
            line.trim_start().strip_prefix(*flag).is_some_and(|rest| {
                rest.is_empty() || rest.starts_with(|c: char| c.is_whitespace() || c == ',')
            })
        })
    })
}
#[cfg(test)]
fn launch_command(executable: &Path, path: &Path, verified: Verified) -> Command {
    process::command(executable, path, &flags(&verified))
}
fn verified_tool(
    preferences: &Preferences,
    cancel: &crate::task::Cancellation,
) -> io::Result<(PathBuf, Verified)> {
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut failure =
        "Install DOSBox, DOSBox-X or DOSBox Staging, or choose its executable in the DOS launcher"
            .to_owned();
    for executable in discover(&preferences.executable) {
        cancel.check()?;
        if Instant::now() >= deadline {
            break;
        }
        match verify_until(&executable, cancel, deadline) {
            Ok(verified) => return Ok((executable, verified)),
            Err(error) => {
                failure = format!(
                    "{}\n{error}",
                    crate::filesystem::display_name(&executable.to_string_lossy())
                )
            }
        }
    }
    Err(io::Error::other(failure))
}

fn discover(configured: &str) -> Vec<PathBuf> {
    let mut folders = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .take(256)
        .collect::<Vec<_>>();
    if cfg!(target_os = "linux") {
        folders.extend(
            [
                "/usr/local/bin",
                "/usr/bin",
                "/usr/local/games",
                "/usr/games",
            ]
            .map(PathBuf::from),
        );
    }
    if cfg!(target_os = "linux") {
        folders.extend(["/snap/bin", "/var/lib/flatpak/exports/bin"].map(PathBuf::from));
        if let Some(home) = std::env::var_os("HOME") {
            folders.push(PathBuf::from(home).join(".local/share/flatpak/exports/bin"));
        }
    }
    if cfg!(target_os = "macos") {
        for root in [
            Some(PathBuf::from("/Applications")),
            std::env::var_os("HOME").map(|p| PathBuf::from(p).join("Applications")),
        ]
        .into_iter()
        .flatten()
        {
            for bundle in [
                "DOSBox Staging.app",
                "DOSBox-X.app",
                "dosbox-x.app",
                "DOSBox.app",
            ] {
                folders.push(root.join(bundle).join("Contents/MacOS"));
            }
        }
    }
    if cfg!(windows) {
        for variable in [
            "ProgramFiles",
            "ProgramFiles(x86)",
            "ProgramW6432",
            "LOCALAPPDATA",
        ] {
            if let Some(parent) = std::env::var_os(variable) {
                let parent = PathBuf::from(parent);
                let mut parents = vec![parent.clone()];
                if variable == "LOCALAPPDATA" {
                    parents.push(parent.join("Programs"));
                }
                for parent in parents {
                    if let Ok(entries) = fs::read_dir(parent) {
                        for entry in entries.take(256).flatten() {
                            if entry
                                .file_name()
                                .to_string_lossy()
                                .to_ascii_lowercase()
                                .contains("dosbox")
                                && entry.file_type().is_ok_and(|t| t.is_dir())
                            {
                                folders.push(entry.path());
                                folders.push(entry.path().join("bin"));
                            }
                        }
                    }
                }
            }
        }
        if let Some(drive) = std::env::var_os("SystemDrive") {
            let mut root = drive;
            root.push("\\");
            folders.push(PathBuf::from(root).join("DOSBox-X"));
        }
    }
    discover_in(configured, &folders)
}
fn discover_in(configured: &str, folders: &[PathBuf]) -> Vec<PathBuf> {
    if !configured.trim().is_empty() {
        let path = PathBuf::from(configured.trim());
        let path = if cfg!(windows) && path.extension().is_none() {
            path.with_extension("exe")
        } else {
            path
        };
        if path.components().count() == 1 && !path.is_absolute() {
            if let Some(found) = folders.iter().map(|p| p.join(&path)).find(|p| p.is_file()) {
                return vec![fs::canonicalize(&found)
                    .unwrap_or_else(|_| crate::requests::absolute_key(&found))];
            }
        }
        return vec![
            fs::canonicalize(&path).unwrap_or_else(|_| crate::requests::absolute_key(&path))
        ];
    }
    let mut paths = Vec::new();
    for name in if cfg!(windows) {
        vec!["dosbox-staging.exe", "dosbox-x.exe", "dosbox.exe"]
    } else {
        vec![
            "dosbox-staging",
            "dosbox-x",
            "dosbox",
            "DosBox",
            "DOSBox",
            "io.github.dosbox-staging",
            "com.dosbox_x.DOSBox-X",
            "com.dosbox.DOSBox",
        ]
    } {
        for folder in folders {
            let path = crate::requests::absolute_key(&folder.join(name));
            if path.is_file() && paths.len() < 16 {
                let path = fs::canonicalize(&path).unwrap_or(path);
                if !paths.contains(&path) {
                    paths.push(path);
                }
            }
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
    config_path: Option<&Path>,
    cancel: &crate::task::Cancellation,
) -> io::Result<Reply> {
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
        if std::env::var_os("DISPLAY").is_none_or(|v| v.is_empty())
            && std::env::var_os("WAYLAND_DISPLAY").is_none_or(|v| v.is_empty())
        {
            return Err(io::Error::other(
                "DOSBox needs a graphical display; a text console alone cannot show its window",
            ));
        }
        let (executable, verified) = verified_tool(preferences, cancel)?;
        cancel.check()?;
        let mounted = mount::prepare(&preferences.mount, config_path, true, cancel)?;
        let current = fs::symlink_metadata(path)?;
        if !crate::platform_fs::same_identity(&metadata, &current)
            || metadata.len() != current.len()
            || metadata.modified().ok() != current.modified().ok()
        {
            return Err(io::Error::other(
                "File changed before DOSBox launch; refresh and try again",
            ));
        }
        let fragment_text = preferences.profile.render(&verified)?;
        cancel.check()?;
        let child = if fragment_text.is_empty() && mounted.is_none() {
            process::Process::spawn(&executable, path, &flags(&verified))?
        } else {
            let fragment = if fragment_text.is_empty() {
                None
            } else {
                Some(profile::Fragment::create(&fragment_text)?)
            };
            let mut arguments = fragment.as_ref().map_or_else(
                || flags(&verified).into_iter().map(Into::into).collect(),
                |f| profile::arguments(&verified, &f.path),
            );
            if let Some(mounted) = &mounted {
                mounted.validate()?;
                arguments.extend(mounted.arguments());
            }
            cancel.check()?;
            if let Some(mounted) = &mounted {
                process::Process::spawn_mounted(
                    &executable,
                    path,
                    &arguments,
                    fragment,
                    mounted.path(),
                )?
            } else {
                process::Process::spawn_config(&executable, path, &arguments, fragment)?
            }
        };
        Ok(Reply::Started(Box::new(child)))
    };
    execute()
}
pub(crate) fn open(app: &mut App, path: PathBuf, automatic: bool) -> bool {
    if !app.config.plugin_enabled(DOS_LAUNCHER)
        || (automatic && !app.config.dos.open_on_enter)
        || !candidate(&path)
    {
        return false;
    }
    if app.dos.pending.is_some() || app.dos.children.len() >= 32 || app.blocking_operation() {
        app.dos.view.notice = "Wait for the current operation before launching DOSBox.".into();
        app.notice = app.dos.view.notice.clone();
        return true;
    }
    let captured = match fs::symlink_metadata(&path) {
        Ok(m) if m.is_file() && !m.file_type().is_symlink() => m,
        Ok(_) => {
            report(
                app,
                app.desktop.active().id,
                app.desktop.activation_revision(),
                "Select a regular DOS program; links are not launched internally.".into(),
                true,
            );
            return true;
        }
        Err(e) => {
            report(
                app,
                app.desktop.active().id,
                app.desktop.activation_revision(),
                e.to_string(),
                true,
            );
            return true;
        }
    };
    let preferences = app.config.dos.clone();
    let source = path.clone();
    let config_path = app.config_store.path().ok().map(Path::to_path_buf);
    match app.workers.spawn(
        "dn-dos-launch",
        &app.dos.requests,
        crate::requests::Owner::Workspace,
        "dos-launch",
        move |cancel| {
            launch(
                &source,
                &preferences,
                &captured,
                automatic,
                config_path.as_deref(),
                &cancel,
            )
        },
    ) {
        Ok(task) => {
            app.dos.pending = Some(Pending {
                task,
                path,
                panel: app.active,
                epoch: app.desktop.activation_revision(),
                automatic,
                origin: app.desktop.active().id,
                owner: app
                    .desktop
                    .plugins
                    .binding(DOS_LAUNCHER)
                    .filter(|b| b.owner == Owner::Document(app.desktop.active().id)),
            });
            app.dos.view.busy = true;
            app.dos.view.notice = "Checking DOS program and emulator…".into();
        }
        Err(error) => report(
            app,
            app.desktop.active().id,
            app.desktop.activation_revision(),
            error.to_string(),
            true,
        ),
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
    let mut failures = Vec::new();
    app.dos.children.retain_mut(|running|match running.process.poll() {
        Ok(Some((status,output)))=>{
            if !status.success() {
                let report=crate::error_report::Report::new("DOSBox exited unsuccessfully.",format!("{status}\n{output}"),"Check graphical output, SDL/audio drivers and DOSBox's global configuration. Use Check DOSBox in the DOS launcher to verify the installation.")
                    .path("Program",&running.process.source).path("DOSBox executable",&running.process.executable).render(app.config.language);
                failures.push((running.origin,running.epoch,report));
            }
            false
        },
        Ok(None)=>true,
        Err(error)=>{failures.push((running.origin,running.epoch,error.to_string()));false},
    });
    let failed = !failures.is_empty();
    for (origin, epoch, message) in failures {
        report(app, origin, epoch, message, true);
    }
    if !app.config.plugin_enabled(DOS_LAUNCHER)
        || app.dos.pending.as_ref().is_some_and(|p| {
            p.owner
                .is_some_and(|b| app.desktop.plugins.binding(DOS_LAUNCHER) != Some(b))
        })
    {
        if app.dos.pending.take().is_some() {
            app.dos.view.notice = "DOSBox check or launch cancelled.".into();
        }
        app.dos.view.busy = false;
    }
    let result = app
        .dos
        .pending
        .as_ref()
        .and_then(|p| match p.task.try_recv() {
            Ok(result) => Some(result),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err(io::Error::other(
                "DOS launcher task ended unexpectedly",
            ))),
        });
    let Some(result) = result else {
        return failed;
    };
    let pending = app.dos.pending.take().unwrap();
    app.dos.view.busy = false;
    match result {
        Ok(Reply::Started(child) | Reply::WindowsStarted(child)) => {
            app.dos.children.push(Running {
                process: child,
                origin: pending.origin,
                epoch: pending.epoch,
            });
            app.dos.view.notice = "DOSBox launched in its own window.".into();
            app.notice = app.dos.view.notice.clone();
            return true;
        }
        Ok(Reply::Checked(message)) => {
            let message = message.replacen("Emulator found; version verification was unavailable. Try launching a DOS program.", app.config.language.text("Emulator found; version verification was unavailable. Try launching a DOS program."), 1);
            let message = message.replacen(
                "DOSBox emulator verified. Game-local configuration loading is disabled.",
                app.config.language.text(
                    "DOSBox emulator verified. Game-local configuration loading is disabled.",
                ),
                1,
            );
            report(app, pending.origin, pending.epoch, message, false);
            return true;
        }
        Ok(Reply::Discovered(tools)) => {
            app.dos.view.tools = tools;
            app.dos.view.generation = app.dos.view.generation.wrapping_add(1);
            app.dos.view.notice = if app.dos.view.tools.is_empty() {
                "No supported DOSBox emulator found. Set an executable path or install an emulator."
            } else {
                "Select an installed emulator, or leave the path blank for Automatic."
            }
            .into();
            return true;
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
        Err(error) if error.kind() == io::ErrorKind::Interrupted => {
            app.dos.view.notice = "Emulator check or launch cancelled.".into();
            return true;
        }
        Err(error) => {
            report(app, pending.origin, pending.epoch, error.to_string(), true);
            return true;
        }
    }
    if !pending.automatic && !app.dos.view.notice.is_empty() {
        report(
            app,
            pending.origin,
            pending.epoch,
            app.dos.view.notice.clone(),
            true,
        );
    } else {
        app.notice = app.dos.view.notice.clone();
    }
    true
}
fn report(
    app: &mut App,
    origin: crate::desktop::WindowId,
    epoch: u64,
    message: String,
    error: bool,
) {
    let message = message
        .lines()
        .map(crate::filesystem::display_name)
        .collect::<Vec<_>>()
        .join("\n");
    if error {
        app.dos.view.last_error = message.clone();
    }
    app.dos.view.notice = message.clone();
    app.notice = message.clone();
    if !app.quit
        && !app.exit_pending
        && app.quit_review.is_none()
        && app.desktop.active().id == origin
        && app.desktop.contains(origin)
        && app.desktop.activation_revision() == epoch
        && app.modal_top().is_none()
    {
        app.push_owned_dialog(
            crate::modals::ModalOwner::Document(origin),
            crate::dialog::Dialog {
                title: if error {
                    "DOSBox launch failed"
                } else {
                    "DOSBox check"
                },
                message,
            },
        );
    }
}
pub(crate) fn stop(app: &mut App, b: crate::plugins::Binding) {
    if b.plugin == DOS_LAUNCHER && app.desktop.plugins.binding(DOS_LAUNCHER) == Some(b) {
        if let Some(pending) = &app.dos.pending {
            pending.task.cancel();
            app.dos.view.notice = "Cancelling emulator check…".into();
        }
    }
}
fn scan(candidates: Vec<PathBuf>, cancel: &crate::task::Cancellation) -> io::Result<Vec<Tool>> {
    let mut tools = Vec::new();
    for executable in candidates.into_iter().take(16) {
        cancel.check()?;
        if let Ok(verified) =
            verify_until(&executable, cancel, Instant::now() + Duration::from_secs(5))
        {
            tools.push(Tool {
                executable,
                verified,
            });
        }
    }
    cancel.check()?;
    Ok(tools)
}
pub(crate) fn discover_emulators(app: &mut App, b: crate::plugins::Binding) {
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
    let epoch = app.desktop.activation_revision();
    match app.workers.spawn(
        "dn-dos-discovery",
        &app.dos.requests,
        crate::requests::Owner::Workspace,
        "dos-launch",
        move |cancel| scan(discover(""), &cancel).map(Reply::Discovered),
    ) {
        Ok(task) => {
            app.dos.pending = Some(Pending {
                task,
                path: PathBuf::new(),
                panel: app.active,
                epoch,
                automatic: false,
                origin,
                owner: Some(b),
            });
            app.dos.view.busy = true;
            app.dos.view.notice = "Searching installed DOSBox emulators…".into();
            app.dos.view.generation = app.dos.view.generation.wrapping_add(1);
        }
        Err(error) => report(app, origin, epoch, error.to_string(), true),
    }
}
pub(crate) fn check(app: &mut App, b: crate::plugins::Binding, preferences: Preferences) {
    let Owner::Document(origin) = b.owner else {
        return;
    };
    if b.plugin != DOS_LAUNCHER
        || app.desktop.plugins.binding(DOS_LAUNCHER) != Some(b)
        || !app.config.plugin_enabled(DOS_LAUNCHER)
    {
        return;
    }
    if app.dos.pending.is_some() {
        return;
    }
    if preferences.executable.len() > 4096 || preferences.executable.chars().any(char::is_control) {
        app.dos.view.notice = "Invalid DOSBox executable path".into();
        return;
    }
    let config_path = app.config_store.path().ok().map(Path::to_path_buf);
    let language = app.config.language;
    let epoch = app.desktop.activation_revision();
    match app.workers.spawn(
        "dn-dos-check",
        &app.dos.requests,
        crate::requests::Owner::Workspace,
        "dos-launch",
        move |cancel| {
            let (executable, verified) = verified_tool(&preferences, &cancel)?;
            let fragment = preferences.profile.render(&verified)?;
            let mounted=mount::prepare(&preferences.mount,config_path.as_deref(),false,&cancel)?;
            let drives=mounted.map_or_else(||preferences.mount.summary(language),|m|format!("{}\n{}\n{}",preferences.mount.summary(language),crate::filesystem::display_name(&m.path().to_string_lossy()),m.commands().join("\n")));
            Ok(Reply::Checked(format!(
                "{}\n{} · {}\n{}\n{}\n{}\n{}",
                if verified.version == "Version unavailable" { "Emulator found; version verification was unavailable. Try launching a DOS program." } else { "DOSBox emulator verified. Game-local configuration loading is disabled." },
                verified.kind.caption(),
                verified.version,
                crate::filesystem::display_name(&executable.to_string_lossy()),
                preferences.profile.summary(language),
                fragment,
                drives
            )))
        },
    ) {
        Ok(task) => {
            app.dos.pending = Some(Pending {
                task,
                path: PathBuf::new(),
                panel: app.active,
                epoch,
                automatic: false,
                origin,
                owner: Some(b),
            });
            app.dos.view.busy = true;
            app.dos.view.notice = "Checking DOS program and emulator…".into();
        }
        Err(error) => report(app, origin, epoch, error.to_string(), true),
    }
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
    if let Err(error) = preferences
        .profile
        .validate()
        .and_then(|()| preferences.windows.validate())
        .and_then(|()| windows::validate_images(&preferences.windows_images))
        .and_then(|()| preferences.mount.validate())
    {
        app.dos.view.notice = error.to_string();
        return;
    }
    app.config.dos = preferences;
    app.dos.view.notice = "DOS launcher preferences applied.".into();
}

pub(crate) fn diagnostics(
    preferences: &Preferences,
    cancel: &crate::task::Cancellation,
) -> crate::utilities::information::Fields {
    match verified_tool(preferences, cancel) {
        Ok((path, verified)) => vec![
            ("DOS emulator".into(), verified.version),
            (
                "DOS emulator path".into(),
                path.to_string_lossy().into_owned(),
            ),
        ],
        Err(e) => vec![("DOS emulator".into(), e.to_string())],
    }
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
    fn version_query_exit_one_requires_a_numbered_x_banner_and_version_flag() {
        let banner = "\nDOSBox-X version 2026.10.01 SDL2, copyright 2011-2026\n";
        for flag in ["-version", "--version"] {
            assert!(version_query_exit_one(flag, Some(1), banner));
        }
        for code in [None, Some(0), Some(2), Some(139)] {
            assert!(!version_query_exit_one("-version", code, banner));
        }
        assert!(!version_query_exit_one("-help", Some(1), banner));
        for bad in [
            "DOSBox-X failed to initialize SDL",
            "DOSBox-X version unavailable",
            "DOSBox-X version 2026",
            "DOSBox Staging 0.83.0",
            "not DOSBox-X version 2026.10.01",
        ] {
            assert!(!version_query_exit_one("-version", Some(1), bad), "{bad}");
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
        let command = launch_command(
            Path::new("/apps/DOSBox Staging"),
            path,
            Verified {
                kind: Kind::Staging,
                flag: "--nolocalconf",
                version: String::new(),
            },
        );
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
        fs::write(&executable,"#!/bin/sh\ncase \"${2:-$1}\" in --version|-version) echo 'DOSBox Staging 0.83.0';; --help|-help) echo '--nolocalconf';; *) exit 1;; esac\n")?;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))?;
        verify(&executable, &Default::default())?;
        let script=format!("#!/bin/sh\ncase \"${{2:-$1}}\" in --version|-version) echo 'DOSBox Staging 0.83.0';; --help|-help) printf '%s\\n' '--nolocalconf' '{}';; esac\n","x".repeat(32_768));
        fs::write(&executable, script)?;
        verify(&executable, &Default::default())?;
        fs::write(&executable, "#!/bin/sh\necho 'not an emulator'\n")?;
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
            None,
            &Default::default(),
        );
        assert!(matches!(result,Err(ref e) if e.to_string().contains("changed")));
        Ok(())
    }
    #[cfg(unix)]
    #[test]
    fn packaged_staging_legacy_local_config_flag_is_supported() -> io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let f = Fixture::new()?;
        let executable = f.0.join("dosbox-staging");
        fs::write(&executable,"#!/bin/sh\ncase \"${2:-$1}\" in --version|-version) echo 'dosbox-staging, version 0.81.1';; --help|-help) echo ' -nolocalconf Do not load local dosbox.conf';; *) exit 2;; esac\n")?;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))?;
        verify(&executable, &Default::default())?;
        Ok(())
    }
    fn app(f: &Fixture) -> io::Result<App> {
        let mut app = App::new(f.0.clone())?;
        app.config = crate::config::Config::default();
        app.config.save_on_exit = false;
        app.config.set_plugin_enabled(DOS_LAUNCHER, true);
        app.config_store = crate::config::Store::new(Some(f.0.join("private.conf")));
        Ok(app)
    }
    fn settle(app: &mut App, ready: impl Fn(&App) -> bool) {
        let end = Instant::now() + Duration::from_secs(5);
        while !ready(app) {
            app.poll();
            assert!(Instant::now() < end, "DOS fixture did not settle");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    #[test]
    fn launch_failures_are_owned_visible_and_never_steal_inactive_focus() -> io::Result<()> {
        let f = Fixture::new()?;
        let mut app = app(&f)?;
        let origin = app.desktop.active().id;
        let epoch = app.desktop.activation_revision();
        report(
            &mut app,
            origin,
            epoch,
            "private launch failure".into(),
            true,
        );
        assert!(app
            .last_dialog()
            .unwrap()
            .message
            .contains("private launch failure"));
        assert_eq!(
            app.modal_top().unwrap().owner,
            crate::modals::ModalOwner::Document(origin)
        );
        app.pop_dialog();
        crate::plugins::host::open(&mut app, DOS_LAUNCHER);
        let active = app.desktop.active().id;
        report(&mut app, origin, epoch, "inactive failure".into(), true);
        assert!(app.last_dialog().is_none());
        assert_eq!(app.desktop.active().id, active);
        assert_eq!(app.dos.view.notice, "inactive failure");
        Ok(())
    }
    #[cfg(unix)]
    #[test]
    fn check_uses_unsaved_draft_without_saving_it_and_rejects_retired_owners() -> io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let f = Fixture::new()?;
        let mut app = app(&f)?;
        let executable = f.0.join("private dosbox");
        fs::write(&executable,"#!/bin/sh\ncase \"${2:-$1}\" in --version|-version) echo 'DOSBox Staging 0.83';; --help|-help) echo '--nolocalconf';; esac\n")?;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))?;
        crate::plugins::host::open(&mut app, DOS_LAUNCHER);
        let b = app.desktop.plugins.binding(DOS_LAUNCHER).unwrap();
        let original = app.config.dos.clone();
        check(
            &mut app,
            b,
            Preferences {
                executable: executable.to_string_lossy().into_owned(),
                open_on_enter: true,
                ..Default::default()
            },
        );
        settle(&mut app, |a| !a.dos.view.busy);
        assert!(app.last_dialog().unwrap().message.contains("verified"));
        assert_eq!(app.config.dos, original);
        app.pop_dialog();
        let Owner::Document(owner) = b.owner else {
            unreachable!()
        };
        app.desktop.close(owner);
        check(&mut app, b, Preferences::default());
        assert!(app.dos.pending.is_none());
        Ok(())
    }
    #[cfg(unix)]
    #[test]
    fn emulator_failure_keeps_stderr_and_literal_source_directory() -> io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let f = Fixture::new()?;
        let mut app = app(&f)?;
        let executable = f.0.join("mock dosbox");
        let path = f.0.join("Ёжик ; $(unsafe).com");
        fs::write(&path, b"\xcd\x20")?;
        fs::write(&executable,"#!/bin/sh\necho 'SDL video initialization failed: private fixture' >&2\npwd\nexit 37\n")?;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))?;
        let process = process::Process::spawn(&executable, &path, &["--nolocalconf"])?;
        app.dos.children.push(Running {
            process: Box::new(process),
            origin: app.desktop.active().id,
            epoch: app.desktop.activation_revision(),
        });
        settle(&mut app, |a| a.dos.children.is_empty());
        let dialog = app.last_dialog().unwrap();
        assert!(dialog.message.contains("SDL video initialization failed"));
        assert!(dialog.message.contains("37"));
        assert!(dialog.message.contains(&path.to_string_lossy().to_string()));
        Ok(())
    }
    #[test]
    fn discovery_prioritizes_staging_and_resolves_bare_names_without_changing_environment(
    ) -> io::Result<()> {
        let f = Fixture::new()?;
        let early = f.0.join("early");
        let later = f.0.join("later");
        fs::create_dir(&early)?;
        fs::create_dir(&later)?;
        let classic = early.join(if cfg!(windows) {
            "dosbox.exe"
        } else {
            "dosbox"
        });
        let staging = later.join(if cfg!(windows) {
            "dosbox-staging.exe"
        } else {
            "dosbox-staging"
        });
        fs::write(&classic, [])?;
        fs::write(&staging, [])?;
        let folders = [early, later.clone(), later];
        let found = discover_in("", &folders);
        assert_eq!(found, vec![staging.clone(), classic]);
        assert_eq!(
            discover_in(staging.file_name().unwrap().to_str().unwrap(), &folders),
            vec![staging.clone()]
        );
        assert_eq!(
            discover_in("dosbox-staging", &folders),
            vec![staging.clone()]
        );
        assert_eq!(
            discover_in("missing", &folders),
            vec![crate::requests::absolute_key(Path::new(if cfg!(windows) {
                "missing.exe"
            } else {
                "missing"
            }))]
        );
        Ok(())
    }
    #[test]
    fn quitting_cancels_pending_verification_without_a_late_error_dialog() -> io::Result<()> {
        let f = Fixture::new()?;
        let mut app = app(&f)?;
        crate::plugins::host::open(&mut app, DOS_LAUNCHER);
        let b = app.desktop.plugins.binding(DOS_LAUNCHER).unwrap();
        let (started, ready) = std::sync::mpsc::sync_channel(1);
        let (release, gate) = std::sync::mpsc::sync_channel(1);
        let task = app.workers.spawn(
            "dn-dos-quit-test",
            &app.dos.requests,
            crate::requests::Owner::Workspace,
            "dos-launch",
            move |cancel| {
                started.send(()).unwrap();
                gate.recv_timeout(Duration::from_secs(5)).unwrap();
                cancel.check()?;
                Ok(Reply::Checked("unexpected completion".into()))
            },
        )?;
        app.dos.pending = Some(Pending {
            task,
            path: PathBuf::new(),
            panel: 0,
            epoch: app.desktop.activation_revision(),
            automatic: false,
            origin: app.desktop.active().id,
            owner: Some(b),
        });
        app.dos.view.busy = true;
        ready.recv_timeout(Duration::from_secs(5)).unwrap();
        app.request_exit();
        assert!(!app.quit && app.exit_pending);
        release.send(()).unwrap();
        settle(&mut app, |a| a.quit);
        assert!(app.last_dialog().is_none());
        assert!(!app.workers.pending());
        Ok(())
    }
    #[cfg(unix)]
    #[test]
    fn legacy_help_alias_is_used_when_modern_help_is_not_supported() -> io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let f = Fixture::new()?;
        let executable = f.0.join("dosbox-staging");
        fs::write(&executable,"#!/bin/sh\ncase \"${2:-$1}\" in --version|-version) echo 'DOSBox Staging 0.81';; -help) echo '-nolocalconf';; *) exit 2;; esac\n")?;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))?;
        assert_eq!(
            verify(&executable, &Default::default())?.flag,
            "-nolocalconf"
        );
        assert_eq!(local_config_flag("Unknown option --nolocalconf"), None);
        Ok(())
    }
    #[test]
    fn all_variants_have_native_literal_arguments_and_packaged_name_hints() {
        let source = Path::new("Ёжик ; $(literal).COM");
        for kind in [Kind::Staging, Kind::X, Kind::Classic] {
            let verified = Verified {
                kind,
                flag: "-nolocalconf",
                version: "test".into(),
            };
            let command = launch_command(Path::new("emulator"), source, verified.clone());
            let mut expected = flags(&verified)
                .iter()
                .map(std::ffi::OsString::from)
                .collect::<Vec<_>>();
            expected.push(source.as_os_str().into());
            assert_eq!(command.get_args().collect::<Vec<_>>(), expected);
            if kind != Kind::Staging {
                assert_eq!(&flags(&verified)[..2], &["-userconf", "-conf"]);
                assert_eq!(
                    flags(&verified)[2],
                    if cfg!(windows) { "NUL" } else { "/dev/null" }
                );
            }
        }
        assert_eq!(hint(Path::new("com.dosbox_x.DOSBox-X")), Some(Kind::X));
        assert_eq!(
            hint(Path::new("io.github.dosbox-staging")),
            Some(Kind::Staging)
        );
        assert_eq!(hint(Path::new("DOSBox.exe")), Some(Kind::Classic));
        assert_eq!(classify("DOSBox version 0.74-3"), Some(Kind::Classic));
        assert_eq!(classify("DOSBox-X version 2026.01"), Some(Kind::X));
    }
    #[cfg(unix)]
    #[test]
    fn scans_classic_x_and_staging_without_a_distro_or_global_environment_changes() -> io::Result<()>
    {
        use std::os::unix::fs::PermissionsExt;
        let f = Fixture::new()?;
        let mut paths = Vec::new();
        for (name, banner) in [
            ("dosbox", "DOSBox version 0.74-3"),
            ("dosbox-x", "DOSBox-X version 2026.01"),
            ("dosbox-staging", "DOSBox Staging 0.83"),
        ] {
            let path = f.0.join(name);
            fs::write(&path, format!("#!/bin/sh\ncase \"$1\" in -version|--version) echo '{banner}';; -help|--help) echo '--nolocalconf';; *) exit 12;; esac\n"))?;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
            paths.push(path);
        }
        let tools = scan(paths, &Default::default())?;
        assert_eq!(
            tools.iter().map(|t| t.verified.kind).collect::<Vec<_>>(),
            [Kind::Classic, Kind::X, Kind::Staging]
        );
        for tool in tools {
            assert!(tool.label().contains(tool.verified.kind.caption()));
            assert!(!tool.verified.version.is_empty());
        }
        let inaccessible = f.0.join("dosbox");
        fs::set_permissions(&inaccessible, fs::Permissions::from_mode(0o600))?;
        assert_eq!(
            verify(&inaccessible, &Default::default())
                .unwrap_err()
                .kind(),
            io::ErrorKind::PermissionDenied
        );
        Ok(())
    }
    #[test]
    fn installed_list_selection_is_a_draft_with_keyboard_mouse_and_cancel() -> io::Result<()> {
        use crate::plugins::{ActionId, Event, ToolPlugin};
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let f = Fixture::new()?;
        let mut app = app(&f)?;
        crate::plugins::host::open(&mut app, DOS_LAUNCHER);
        let binding = app.desktop.plugins.binding(DOS_LAUNCHER).unwrap();
        for (name, kind) in [
            ("dosbox-staging", Kind::Staging),
            ("dosbox-x", Kind::X),
            ("dosbox", Kind::Classic),
        ] {
            app.dos.view.tools.push(Tool {
                executable: f.0.join(name),
                verified: Verified {
                    kind,
                    flag: "--nolocalconf",
                    version: "test".into(),
                },
            });
        }
        let ctx = crate::plugins::host::context(
            &app,
            binding,
            crate::presentation::layout::Rect::new(0, 0, 80, 25),
            Instant::now(),
        )
        .unwrap();
        let mut plugin = crate::plugins::dos_launcher::DosLauncher::default();
        plugin.open(&ctx);
        plugin.prepare(&ctx);
        plugin.event(Event::Focus(6), &ctx);
        plugin.event(
            Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
            &ctx,
        );
        plugin.prepare(&ctx);
        let effects = plugin.event(Event::Action(ActionId(2)), &ctx);
        assert!(effects.iter().any(|e| matches!(e, crate::plugins::Effect::DosSetup(p) if p.executable.ends_with("dosbox-staging"))));
        plugin.event(Event::Action(ActionId(1002)), &ctx);
        let effects = plugin.event(Event::Action(ActionId(2)), &ctx);
        assert!(effects.iter().any(|e| matches!(e, crate::plugins::Effect::DosSetup(p) if p.executable.ends_with("dosbox-x"))));
        plugin.event(Event::Action(ActionId(9999)), &ctx);
        let effects = plugin.event(Event::Action(ActionId(3)), &ctx);
        assert!(matches!(
            effects.as_slice(),
            [crate::plugins::Effect::Close]
        ));
        assert_eq!(app.config.dos, Preferences::default());
        plugin.open(&ctx);
        assert!(plugin
            .event(Event::Action(ActionId(2)), &ctx)
            .iter()
            .any(|e| matches!(e, crate::plugins::Effect::DosSetup(p) if p.executable.is_empty())));
        Ok(())
    }
    #[cfg(unix)]
    #[test]
    fn custom_alias_tries_both_version_spellings_before_rejecting_it() -> io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let f = Fixture::new()?;
        let path = f.0.join("my emulator");
        fs::write(&path, "#!/bin/sh\ncase \"$1\" in --version) echo 'DOSBox Staging 0.83';; --help) echo '--nolocalconf';; *) echo 'Unknown option';; esac\n")?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
        assert_eq!(verify(&path, &Default::default())?.kind, Kind::Staging);
        Ok(())
    }
}
