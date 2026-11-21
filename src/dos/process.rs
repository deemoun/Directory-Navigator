//! Nonblocking, bounded emulator diagnostics. No reader threads or UI waits.
use std::{
    collections::VecDeque,
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Child, ChildStderr, ChildStdout, Command, ExitStatus},
};
const LIMIT: usize = 8192;
const BUDGET: usize = 8192;

#[cfg(unix)]
fn prepare(pipe: &impl std::os::fd::AsRawFd) -> io::Result<()> {
    let fd = pipe.as_raw_fd();
    // SAFETY: pipe owns this descriptor; preserve its flags and only change the read end.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
#[cfg(windows)]
fn prepare(_: &impl std::os::windows::io::AsRawHandle) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn available(_: &impl std::os::fd::AsRawFd) -> io::Result<usize> {
    Ok(BUDGET)
}
#[cfg(windows)]
fn available(pipe: &impl std::os::windows::io::AsRawHandle) -> io::Result<usize> {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn PeekNamedPipe(
            pipe: *mut c_void,
            buffer: *mut c_void,
            size: u32,
            read: *mut u32,
            available: *mut u32,
            remaining: *mut u32,
        ) -> i32;
    }
    let mut count = 0;
    // SAFETY: the child pipe handle is live; only the available-byte output is requested.
    if unsafe {
        PeekNamedPipe(
            pipe.as_raw_handle(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            &mut count,
            std::ptr::null_mut(),
        )
    } == 0
    {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(109) {
            return Ok(0);
        } // ERROR_BROKEN_PIPE: writer exited.
        return Err(error);
    }
    Ok(count as usize)
}
trait Pipe: Read {
    fn available(&self) -> io::Result<usize>;
}
macro_rules! pipe {
    ($ty:ty) => {
        impl Pipe for $ty {
            fn available(&self) -> io::Result<usize> {
                available(self)
            }
        }
    };
}
pipe!(ChildStdout);
pipe!(ChildStderr);

pub(super) fn command<S: AsRef<std::ffi::OsStr>>(
    executable: &Path,
    path: &Path,
    flags: &[S],
) -> Command {
    command_inner(executable, path, flags, true)
}
pub(super) fn command_inner<S: AsRef<std::ffi::OsStr>>(
    executable: &Path,
    path: &Path,
    flags: &[S],
    append_source: bool,
) -> Command {
    let mut command = Command::new(executable);
    command
        .args(flags)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    if append_source {
        command.arg(path);
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        command.current_dir(parent);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // A launched game remains independent when DN exits and its diagnostic
        // read ends close. Logging then gets EPIPE instead of terminating it.
        unsafe {
            command.pre_exec(|| {
                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = libc::SIG_IGN;
                libc::sigemptyset(&mut action.sa_mask);
                if libc::sigaction(libc::SIGPIPE, &action, std::ptr::null_mut()) != 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    command
}

pub(super) struct Process {
    child: Child,
    fragment: Option<super::profile::Fragment>,
    out: Option<ChildStdout>,
    err: Option<ChildStderr>,
    tail: VecDeque<u8>,
    pub(super) executable: PathBuf,
    pub(super) source: PathBuf,
    pub(super) windows_identity: Option<crate::platform_fs::EntryIdentity>,
}
impl Process {
    pub(super) fn spawn(executable: &Path, path: &Path, flags: &[&str]) -> io::Result<Self> {
        Self::spawn_config(executable, path, flags, None)
    }
    pub(super) fn spawn_config<S: AsRef<std::ffi::OsStr>>(
        executable: &Path,
        path: &Path,
        flags: &[S],
        fragment: Option<super::profile::Fragment>,
    ) -> io::Result<Self> {
        Self::start(executable, path, command(executable, path, flags), fragment)
    }
    pub(super) fn spawn_image<S: AsRef<std::ffi::OsStr>>(
        executable: &Path,
        path: &Path,
        flags: &[S],
        fragment: super::profile::Fragment,
    ) -> io::Result<Self> {
        Self::start(
            executable,
            path,
            command_inner(executable, path, flags, false),
            Some(fragment),
        )
    }
    pub(super) fn spawn_mounted<S: AsRef<std::ffi::OsStr>>(
        executable: &Path,
        path: &Path,
        args: &[S],
        fragment: Option<super::profile::Fragment>,
        cwd: &Path,
    ) -> io::Result<Self> {
        let mut launch = command(executable, path, args);
        launch.current_dir(cwd);
        Self::start(executable, path, launch, fragment)
    }
    fn start(
        executable: &Path,
        path: &Path,
        mut command: Command,
        fragment: Option<super::profile::Fragment>,
    ) -> io::Result<Self> {
        let mut child = command.spawn()?;
        let out = child.stdout.take();
        let err = child.stderr.take();
        let prepared = (|| {
            if let Some(pipe) = &out {
                prepare(pipe)?;
            }
            if let Some(pipe) = &err {
                prepare(pipe)?;
            }
            Ok::<(), io::Error>(())
        })();
        if let Err(error) = prepared {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        Ok(Self {
            child,
            fragment,
            out,
            err,
            tail: VecDeque::new(),
            executable: executable.into(),
            source: path.into(),
            windows_identity: None,
        })
    }
    pub(super) fn poll(&mut self) -> io::Result<Option<(ExitStatus, String)>> {
        drain(&mut self.out, &mut self.tail);
        drain(&mut self.err, &mut self.tail);
        let Some(status) = self.child.try_wait()? else {
            return Ok(None);
        };
        // Exit does not imply EOF if a descendant inherited a pipe. Reads stay
        // nonblocking and bounded even in that case.
        drain(&mut self.out, &mut self.tail);
        drain(&mut self.err, &mut self.tail);
        Ok(Some((
            status,
            String::from_utf8_lossy(self.tail.make_contiguous()).into_owned(),
        )))
    }
}
fn drain<T: Pipe>(pipe: &mut Option<T>, tail: &mut VecDeque<u8>) {
    let Some(reader) = pipe else {
        return;
    };
    let mut left = BUDGET;
    let mut bytes = [0; 1024];
    while left > 0 {
        let count = match reader.available() {
            Ok(n) => n.min(left).min(bytes.len()),
            Err(_) => {
                *pipe = None;
                return;
            }
        };
        if count == 0 {
            return;
        }
        match reader.read(&mut bytes[..count]) {
            Ok(0) => {
                *pipe = None;
                return;
            }
            Ok(n) => {
                left -= n;
                for byte in &bytes[..n] {
                    if tail.len() == LIMIT {
                        tail.pop_front();
                    }
                    tail.push_back(*byte);
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                return
            }
            Err(_) => {
                *pipe = None;
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostic_retention_is_bounded_and_reads_do_not_need_eof() {
        struct Bytes(std::io::Cursor<Vec<u8>>);
        impl Read for Bytes {
            fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
                self.0.read(b)
            }
        }
        impl Pipe for Bytes {
            fn available(&self) -> io::Result<usize> {
                Ok(BUDGET)
            }
        }
        let mut reader = Some(Bytes(std::io::Cursor::new(vec![b'x'; LIMIT * 4])));
        let mut tail = VecDeque::new();
        drain(&mut reader, &mut tail);
        assert_eq!(tail.len(), LIMIT);
        assert_eq!(reader.as_ref().unwrap().0.position(), BUDGET as u64);
        drain(&mut reader, &mut tail);
        assert_eq!(tail.len(), LIMIT);
        struct Idle;
        impl Read for Idle {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::ErrorKind::WouldBlock.into())
            }
        }
        impl Pipe for Idle {
            fn available(&self) -> io::Result<usize> {
                Ok(BUDGET)
            }
        }
        drain(&mut Some(Idle), &mut tail);
        assert_eq!(tail.len(), LIMIT);
    }
}

impl Process {
    fn preserve_live_fragment(&mut self) {
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            if let Some(fragment) = self.fragment.take() {
                fragment.preserve();
            }
        }
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        self.preserve_live_fragment();
    }
}

#[cfg(test)]
mod fragment_tests {
    use super::*;
    use std::{
        fs,
        time::{Duration, Instant},
    };
    struct Temp(PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn wait(ready: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(8);
        while !ready() {
            assert!(
                Instant::now() < deadline,
                "mock emulator did not reach requested state"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    #[test]
    fn portable_emulator_reads_literal_fragment_and_retains_it_until_exit_or_detach(
    ) -> io::Result<()> {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "dn-dos-fragment-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&root)?;
        let fixture = Temp(root);
        let source = fixture.0.join("emulator.rs");
        let executable = fixture.0.join(if cfg!(windows) {
            "mock-dosbox.exe"
        } else {
            "mock-dosbox"
        });
        fs::write(
            &source,
            r#"use std::{env,fs,thread,time::{Duration,Instant},path::PathBuf};
fn main(){let a:Vec<_>=env::args_os().skip(1).collect();let source=PathBuf::from(a.last().unwrap());let root=source.parent().unwrap();
let i=a.iter().rposition(|v| v=="--conf" || v=="-conf").unwrap();let config=PathBuf::from(&a[i+1]);
let text=fs::read(&config).unwrap();fs::write(root.join("observed"),&text).unwrap();fs::write(root.join("ready"),b"ready").unwrap();
let deadline=Instant::now()+Duration::from_secs(15);while !root.join("release").exists(){assert!(Instant::now()<deadline);assert_eq!(fs::read(&config).unwrap(),text);thread::sleep(Duration::from_millis(2));}}
"#,
        )?;
        let output = Command::new("rustc")
            .args(["--edition=2021", "--crate-name", "dn_dos_mock"])
            .arg(&source)
            .arg("-o")
            .arg(&executable)
            .output()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        for detached in [false, true] {
            let game = fixture.0.join(if detached {
                "detached 世界"
            } else {
                "normal 世界"
            });
            fs::create_dir(&game)?;
            let program = game.join("literal ; $ DOS.COM");
            fs::write(&program, b"DOS fixture")?;
            fs::write(game.join("dosbox.conf"), b"local settings unchanged")?;
            let fragment = super::super::profile::Fragment::create("[dosbox]\nmemsize=32\n")?;
            let path = fragment.path.clone();
            let identity = crate::platform_fs::entry_identity(path.parent().unwrap())?;
            let args: Vec<std::ffi::OsString> = vec![
                "--nolocalconf".into(),
                "--conf".into(),
                path.as_os_str().to_owned(),
            ];
            let mut process = Process::spawn_config(&executable, &program, &args, Some(fragment))?;
            let result = (|| -> io::Result<()> {
                wait(|| game.join("ready").exists());
                assert_eq!(fs::read(game.join("observed"))?, b"[dosbox]\nmemsize=32\n");
                assert_eq!(
                    fs::read(game.join("dosbox.conf"))?,
                    b"local settings unchanged"
                );
                if detached {
                    process.preserve_live_fragment();
                    assert!(process.fragment.is_none());
                }
                assert!(path.exists());
                fs::write(game.join("release"), b"release")?;
                wait(|| game.join("release").exists());
                assert!(process.child.wait()?.success());
                Ok(())
            })();
            // Also reap on assertion-independent I/O failures.
            let _ = process.child.kill();
            let _ = process.child.wait();
            drop(process);
            result?;
            if detached {
                assert!(path.exists());
                assert_eq!(
                    crate::platform_fs::entry_identity(path.parent().unwrap())?,
                    identity
                );
                fs::remove_dir_all(path.parent().unwrap())?;
            } else {
                assert!(!path.exists());
            }
        }
        let fragment = super::super::profile::Fragment::create("[sdl]\nfullscreen=false\n")?;
        let path = fragment.path.clone();
        assert!(Process::spawn_config(
            &fixture.0.join("missing"),
            Path::new("test.COM"),
            &["-conf"],
            Some(fragment)
        )
        .is_err());
        assert!(!path.exists());
        Ok(())
    }
}
