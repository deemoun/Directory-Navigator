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

pub(super) fn command(executable: &Path, path: &Path, flags: &[&str]) -> Command {
    let mut command = Command::new(executable);
    command
        .args(flags)
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
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
    out: Option<ChildStdout>,
    err: Option<ChildStderr>,
    tail: VecDeque<u8>,
    pub(super) executable: PathBuf,
    pub(super) source: PathBuf,
}
impl Process {
    pub(super) fn spawn(executable: &Path, path: &Path, flags: &[&str]) -> io::Result<Self> {
        let mut child = command(executable, path, flags).spawn()?;
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
            out,
            err,
            tail: VecDeque::new(),
            executable: executable.into(),
            source: path.into(),
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
