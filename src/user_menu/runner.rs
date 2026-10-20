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
}
pub fn run(plan: &Plan) -> io::Result<ExitStatus> {
    static RUN: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _run = RUN
        .lock()
        .map_err(|_| io::Error::other("Command runner lock unavailable"))?;
    #[cfg(unix)]
    let _signals = Signals::ignore()?;
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
