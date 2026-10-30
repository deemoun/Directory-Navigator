use std::{
    ffi::{OsStr, OsString},
    io,
    path::PathBuf,
    process::{Command, ExitStatus},
};
#[derive(Clone, Debug)]
pub struct CommandSpec {
    pub program: OsString,
    pub args: Vec<OsString>,
}
#[derive(Clone, Debug)]
pub struct Plan {
    pub cwd: PathBuf,
    pub commands: Vec<CommandSpec>,
    pub env: Vec<(String, OsString)>,
    pub list_files: Vec<SelectedList>,
}
pub fn run(plan: &Plan) -> io::Result<ExitStatus> {
    if plan.commands.is_empty() {
        return Err(io::Error::other("No commands to run"));
    }
    static RUN: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _run = RUN
        .lock()
        .map_err(|_| io::Error::other("Command runner lock unavailable"))?;
    #[cfg(unix)]
    let _signals = Signals::ignore()?;
    let _lists = ListGuard::prepare(&plan.list_files)?;
    let mut last = None;
    for spec in &plan.commands {
        let mut command = Command::new(&spec.program);
        command.args(&spec.args).current_dir(&plan.cwd);
        for (k, v) in &plan.env {
            command.env(k, v);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // SAFETY: only async-signal-safe signal dispositions are changed before exec.
            unsafe {
                command.pre_exec(|| {
                    libc::signal(libc::SIGINT, libc::SIG_DFL);
                    libc::signal(libc::SIGQUIT, libc::SIG_DFL);
                    Ok(())
                });
            }
        }
        let status = command.status()?;
        last = Some(status);
        if !status.success() {
            break;
        }
    }
    last.ok_or_else(|| io::Error::other("No commands to run"))
}
#[cfg(unix)]
struct Signals(Vec<(libc::c_int, libc::sigaction)>);
#[cfg(unix)]
impl Signals {
    fn ignore() -> io::Result<Self> {
        let mut guard = Self(Vec::new());
        for signal in [libc::SIGINT, libc::SIGQUIT] {
            // SAFETY: initialized sigaction storage is passed to libc; the saved disposition
            // remains owned by this guard and is restored on every return path.
            unsafe {
                let mut old: libc::sigaction = std::mem::zeroed();
                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = libc::SIG_IGN;
                libc::sigemptyset(&mut action.sa_mask);
                if libc::sigaction(signal, &action, &mut old) != 0 {
                    return Err(io::Error::last_os_error());
                }
                guard.0.push((signal, old));
            }
        }
        Ok(guard)
    }
}
#[cfg(unix)]
impl Drop for Signals {
    fn drop(&mut self) {
        for (signal, old) in &self.0 {
            unsafe {
                libc::sigaction(*signal, old, std::ptr::null_mut());
            }
        }
    }
}
pub fn display(value: &OsStr) -> String {
    format!(
        "{:?}",
        crate::filesystem::display_name(&value.to_string_lossy())
    )
}

/// Planned paths are inert until Run; lists use private directories and are
/// removed on success, launch failure and nonzero exit alike.
#[derive(Clone, Debug)]
pub struct SelectedList {
    path: PathBuf,
    entries: Vec<PathBuf>,
}
impl SelectedList {
    pub fn new(entries: Vec<PathBuf>) -> io::Result<Self> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        if entries.is_empty()
            || entries.len() > 2048
            || entries.iter().any(|p| {
                p.as_os_str()
                    .as_encoded_bytes()
                    .iter()
                    .any(|b| matches!(b, b'\n' | b'\r'))
            })
            || entries
                .iter()
                .map(|p| p.as_os_str().as_encoded_bytes().len() + 1)
                .sum::<usize>()
                > 256 * 1024
        {
            return Err(io::Error::other(
                "File lists require 1–2048 paths, at most 256 KiB and no newline characters",
            ));
        }
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "dn-menu-list-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        Ok(Self {
            path: root.join("files.txt"),
            entries,
        })
    }
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
    pub fn entries(&self) -> &[PathBuf] {
        &self.entries
    }
}
struct ListGuard(Vec<PathBuf>);
impl ListGuard {
    fn prepare(lists: &[SelectedList]) -> io::Result<Self> {
        use std::io::Write;
        let mut guard = Self(Vec::new());
        for list in lists {
            let root = list.path.parent().expect("planned list directory");
            let mut dir = std::fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                dir.mode(0o700);
            }
            dir.create(root)?;
            guard.0.push(root.to_owned());
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&list.path)?;
            for path in &list.entries {
                file.write_all(path.as_os_str().as_encoded_bytes())?;
                file.write_all(b"\n")?;
            }
            file.sync_all()?;
        }
        Ok(guard)
    }
}
impl Drop for ListGuard {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}
