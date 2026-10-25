//! System file associations, without shell interpolation or terminal ownership.
use crate::task::Task;
#[cfg(not(windows))]
use std::process::{Command, Stdio};
use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Child,
    sync::mpsc::TryRecvError,
};

pub enum Launched {
    Directory,
    Process(Child),
    Submitted,
}
pub struct Completion {
    pub token: Option<crate::requests::RequestToken>,
    pub panel: usize,
    pub path: PathBuf,
    pub result: io::Result<Launched>,
}
struct Pending {
    token: Option<crate::requests::RequestToken>,
    panel: usize,
    path: PathBuf,
    task: Task<io::Result<Launched>>,
}
#[derive(Default)]
pub struct Launcher {
    pub(crate) requests: crate::requests::RequestScope,
    pending: Option<Pending>,
    children: Vec<(usize, PathBuf, Child, Option<crate::requests::RequestToken>)>,
}
impl Launcher {
    pub fn loading(&self) -> bool {
        self.pending.is_some()
    }
    pub fn request(&mut self, panel: usize, path: PathBuf) -> io::Result<()> {
        self.request_with(panel, path, launch)
    }
    pub fn request_owned(
        &mut self,
        panel: usize,
        path: PathBuf,
        token: crate::requests::RequestToken,
    ) -> io::Result<()> {
        self.request_scoped(panel, path, Some(token), launch)
    }
    pub fn request_with(
        &mut self,
        panel: usize,
        path: PathBuf,
        start: impl FnOnce(&Path) -> io::Result<Launched> + Send + 'static,
    ) -> io::Result<()> {
        self.request_scoped(panel, path, None, start)
    }
    fn request_scoped(
        &mut self,
        panel: usize,
        path: PathBuf,
        token: Option<crate::requests::RequestToken>,
        start: impl FnOnce(&Path) -> io::Result<Launched> + Send + 'static,
    ) -> io::Result<()> {
        if self.pending.is_some() || self.children.len() >= 32 {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "An application is still opening; try again shortly",
            ));
        }
        let source = path.clone();
        let task = self.requests.spawn("dn-open", move |cancel| {
            cancel.check()?;
            start(&source)
        })?;
        self.pending = Some(Pending {
            token,
            panel,
            path,
            task,
        });
        Ok(())
    }
    pub fn poll(&mut self) -> Vec<Completion> {
        let mut replies = Vec::new();
        if let Some(pending) = &self.pending {
            let result = match pending.task.try_recv() {
                Ok(result) => Some(result),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => Some(Err(io::Error::other(
                    "Application launcher stopped unexpectedly",
                ))),
            };
            if let Some(result) = result {
                if let Some(pending) = self.pending.take() {
                    match result {
                        Ok(Launched::Process(child)) => {
                            replies.push(Completion {
                                token: pending.token,
                                panel: pending.panel,
                                path: pending.path.clone(),
                                result: Ok(Launched::Submitted),
                            });
                            self.children
                                .push((pending.panel, pending.path, child, pending.token));
                        }
                        result => replies.push(Completion {
                            token: pending.token,
                            panel: pending.panel,
                            path: pending.path,
                            result,
                        }),
                    }
                }
            }
        }
        let mut index = 0;
        while index < self.children.len() {
            let result=match self.children[index].2.try_wait() {
                Ok(None)=> {index+=1;continue;},
                Ok(Some(status)) if status.success()=>Ok(Launched::Submitted),
                Ok(Some(status))=>Err(io::Error::other(format!("The system opener failed ({status}). Check that a default application is configured for this file type."))),
                Err(error)=>Err(error),
            };
            let (panel, path, _, token) = self.children.swap_remove(index);
            replies.push(Completion {
                token,
                panel,
                path,
                result,
            });
        }
        replies
    }
}

/// Resolve links for classification only; pass the original native path to the OS.
pub fn is_directory(path: &Path) -> io::Result<bool> {
    let metadata = fs::metadata(path)?;
    if metadata.is_dir() {
        #[cfg(target_os = "macos")]
        {
            let bundle = is_app_bundle;
            if bundle(path) || fs::canonicalize(path).is_ok_and(|p| bundle(&p)) {
                return Ok(false);
            }
        }
        return Ok(true);
    }
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Only regular files, application bundles and directories can be opened",
        ));
    }
    Ok(false)
}

pub fn is_app_bundle(path: &Path) -> bool {
    cfg!(target_os = "macos")
        && path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("app"))
}

pub fn launch(path: &Path) -> io::Result<Launched> {
    if is_directory(path)? {
        return Ok(Launched::Directory);
    }
    let absolute = std::path::absolute(path)?;
    #[cfg(windows)]
    {
        windows_open(&absolute)?;
        Ok(Launched::Submitted)
    }
    #[cfg(not(windows))]
    {
        opener_command(&absolute)?.spawn().map(Launched::Process).map_err(|e|io::Error::new(e.kind(),format!("Cannot start the system file opener: {e}. On Linux, install xdg-utils and use a desktop session.")))
    }
}

#[cfg(not(windows))]
pub fn opener_command(path: &Path) -> io::Result<Command> {
    // Absolute path arguments cannot be mistaken for flags or web URLs.
    let absolute = std::path::absolute(path)?;
    #[cfg(target_os = "macos")]
    let mut command = Command::new("/usr/bin/open");
    #[cfg(target_os = "linux")]
    let mut command = Command::new("xdg-open");
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let mut command = Command::new("xdg-open");
    command
        .arg(&absolute)
        .current_dir(absolute.parent().unwrap_or(Path::new(".")))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    Ok(command)
}

#[cfg(windows)]
fn windows_open(path: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        core::{w, PCWSTR},
        Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
    };
    let wide: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    if wide[..wide.len() - 1].contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Path contains NUL",
        ));
    }
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(wide.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    let code = result.0 as isize;
    if code <= 32 {
        Err(io::Error::other(format!(
            "Windows could not open the file (Shell error {code}); check its default application"
        )))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn navigation_retires_delayed_panel_effect_without_losing_the_worker() -> io::Result<()> {
        let root = std::env::temp_dir().join(format!("dn-launch-owner-{}", std::process::id()));
        fs::create_dir_all(root.join("new"))?;
        let mut app = crate::app::App::new(root.clone())?;
        let end = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while app.panels.iter().any(|p| p.loading) {
            app.poll();
            assert!(std::time::Instant::now() < end);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let token = app.panels[0].scope.start("external-open");
        let (send, recv) = std::sync::mpsc::channel();
        app.launcher
            .request_scoped(0, root.join("obsolete"), Some(token), move |_| {
                recv.recv().unwrap();
                Ok(Launched::Directory)
            })?;
        app.panels[0].go_to(root.join("new"), false);
        send.send(()).unwrap();
        while app.launcher.loading() || app.panels[0].loading {
            app.poll();
            assert!(std::time::Instant::now() < end);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(app.panels[0].path, root.join("new"));
        assert!(app.dialogs().is_empty());
        drop(app);
        fs::remove_dir_all(root)?;
        Ok(())
    }
}
