#![cfg(unix)]
use std::{
    fs::{self, File},
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::process::CommandExt,
    },
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

struct Fixture {
    directory: std::path::PathBuf,
    child: std::process::Child,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            // SAFETY: the child started a private session/process group for this test.
            unsafe {
                libc::kill(-(self.child.id() as i32), libc::SIGKILL);
            }
        }
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.directory);
    }
}
fn until(master: &mut File, output: &mut Vec<u8>, needle: &[u8]) -> io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !output.windows(needle.len()).any(|w| w == needle) {
        let mut chunk = [0u8; 8192];
        match master.read(&mut chunk) {
            Ok(0) => return Err(io::Error::other("PTY closed early")),
            Ok(n) => output.extend_from_slice(&chunk[..n]),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(2))
            }
            Err(e) => return Err(e),
        }
        if Instant::now() > deadline {
            return Err(io::Error::other(format!(
                "Timed out waiting for {}",
                String::from_utf8_lossy(needle)
            )));
        }
        assert!(output.len() < 2 * 1024 * 1024);
    }
    Ok(())
}
#[test]
fn delayed_fragmented_mouse_release_does_not_leak_to_shell() -> io::Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    for mouse in [true, false] {
        let directory = std::env::temp_dir().join(format!(
            "dn-exit-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&directory)?;
        // Exercise the manager's Quit target, rather than the first-run modal.
        fs::create_dir_all(directory.join("config/dn"))?;
        fs::write(
            directory.join("config/dn/config.conf"),
            "save_on_exit=false\n",
        )?;
        let mut master_fd = 0;
        let mut slave_fd = 0;
        let size = libc::winsize {
            ws_row: 54,
            ws_col: 170,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // Linux takes a const winsize pointer; other Unix signatures take mutable.
        #[cfg(not(target_os = "linux"))]
        let mut size = size;
        #[cfg(target_os = "linux")]
        let size_ptr = &size;
        #[cfg(not(target_os = "linux"))]
        let size_ptr = &mut size;
        // SAFETY: openpty initializes two owned descriptors with the supplied size.
        if unsafe {
            libc::openpty(
                &mut master_fd,
                &mut slave_fd,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                size_ptr,
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: these are distinct, newly owned descriptors from openpty.
        let mut master = unsafe { File::from_raw_fd(master_fd) };
        let slave = unsafe { File::from_raw_fd(slave_fd) };
        // SAFETY: valid descriptor; flags are added to the existing file status flags.
        unsafe {
            let flags = libc::fcntl(master.as_raw_fd(), libc::F_GETFL);
            if libc::fcntl(master.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
                return Err(io::Error::last_os_error());
            }
        }
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "\"$1\"; printf '\\nDN_EXIT_READY\\n'; IFS= read -r reply; printf 'DN_INPUT:%s\\n' \"$reply\"", "dn-test", env!("CARGO_BIN_EXE_dn")])
            .current_dir(&directory).env("TERM","xterm-256color").env("XDG_CONFIG_HOME",directory.join("config"))
            .stdin(Stdio::from(slave.try_clone()?)).stdout(Stdio::from(slave.try_clone()?)).stderr(Stdio::from(slave.try_clone()?));
        // SAFETY: only async-signal-safe session/terminal calls run between fork and exec.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command.spawn()?;
        let mut fixture = Fixture { directory, child };
        let mut output = Vec::new();
        until(&mut master, &mut output, b"Quit")?;
        if mouse {
            master.write_all(b"\x1b[<0;168;54M")?;
            std::thread::sleep(Duration::from_millis(20));
            master.write_all(b"\x1b[<")?;
            std::thread::sleep(Duration::from_millis(10));
            master.write_all(b"0;168;54m")?;
        } else {
            master.write_all(b"\x1b[21~")?;
        }
        until(&mut master, &mut output, b"DN_EXIT_READY")?;
        master.write_all(b"TOKEN\n")?;
        until(&mut master, &mut output, b"DN_INPUT:")?;
        until(&mut master, &mut output, b"DN_INPUT:TOKEN")?;
        assert!(!output
            .windows(b"0;168;54m".len())
            .any(|w| w == b"0;168;54m"));
        assert!(fixture.child.wait()?.success());
    }
    Ok(())
}
