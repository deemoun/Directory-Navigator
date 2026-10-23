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
const OUTPUT_LIMIT: usize = 2 * 1024 * 1024;

fn wait_error(stage: &str, reason: &str, status: Option<&str>, output: &[u8]) -> io::Error {
    let tail = &output[output.len().saturating_sub(16 * 1024)..];
    let escaped: String = tail
        .iter()
        .flat_map(|byte| std::ascii::escape_default(*byte))
        .map(char::from)
        .collect();
    io::Error::other(format!(
        "{stage}: {reason}; child={}; output tail ({}/{} bytes): {escaped}",
        status.unwrap_or("running"),
        tail.len(),
        output.len()
    ))
}

fn wait_for<R: Read>(
    master: &mut R,
    output: &mut Vec<u8>,
    needle: &[u8],
    stage: &str,
    timeout: Duration,
    mut child_status: impl FnMut() -> io::Result<Option<String>>,
) -> io::Result<()> {
    let deadline = Instant::now() + timeout;
    let mut exited = None;
    loop {
        if output.windows(needle.len()).any(|w| w == needle) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(wait_error(stage, "timed out", exited.as_deref(), output));
        }
        let mut chunk = [0u8; 8192];
        let idle = match master.read(&mut chunk) {
            Ok(0) => true,
            Ok(n) => {
                output.extend_from_slice(&chunk[..n]);
                if output.len() > OUTPUT_LIMIT {
                    return Err(wait_error(
                        stage,
                        "output limit exceeded",
                        exited.as_deref(),
                        output,
                    ));
                }
                false
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                true
            }
            // Linux PTYs may report EIO on hangup instead of EOF.
            Err(e) if e.raw_os_error() == Some(libc::EIO) => true,
            Err(e) => return Err(wait_error(stage, &e.to_string(), exited.as_deref(), output)),
        };
        if idle {
            if exited.is_some() {
                return Err(wait_error(
                    stage,
                    "PTY ended before expected output",
                    exited.as_deref(),
                    output,
                ));
            }
            exited = child_status().map_err(|e| wait_error(stage, &e.to_string(), None, output))?;
            // Read once more after observing exit to drain any final buffered output.
            if exited.is_none() {
                std::thread::sleep(Duration::from_millis(2));
            }
        }
    }
}

fn terminal_mode(terminal: &File) -> io::Result<libc::termios> {
    let mut mode = std::mem::MaybeUninit::uninit();
    // SAFETY: tcgetattr initializes termios on success for this valid descriptor.
    if unsafe { libc::tcgetattr(terminal.as_raw_fd(), mode.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: tcgetattr succeeded above.
    Ok(unsafe { mode.assume_init() })
}

fn until(
    master: &mut File,
    output: &mut Vec<u8>,
    needle: &[u8],
    stage: &str,
    child: &mut std::process::Child,
) -> io::Result<()> {
    wait_for(
        master,
        output,
        needle,
        stage,
        Duration::from_secs(10),
        || {
            child
                .try_wait()
                .map(|status| status.map(|status| status.to_string()))
        },
    )
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
        let original_mode = terminal_mode(&slave)?;
        // SAFETY: valid descriptor; flags are added to the existing file status flags.
        unsafe {
            let flags = libc::fcntl(master.as_raw_fd(), libc::F_GETFL);
            if libc::fcntl(master.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
                return Err(io::Error::last_os_error());
            }
        }
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "\"$1\"; dn_status=$?; printf '\\nDN_STATUS:%s\\n' \"$dn_status\"; [ \"$dn_status\" -eq 0 ] || exit \"$dn_status\"; printf 'DN_EXIT_READY\\n'; IFS= read -r reply || exit 90; printf 'DN_INPUT:%s\\n' \"$reply\"", "dn-test", env!("CARGO_BIN_EXE_dn")])
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
        let scenario = if mouse { "mouse exit" } else { "keyboard exit" };
        until(
            &mut master,
            &mut output,
            b"Quit",
            &format!("{scenario}: manager ready"),
            &mut fixture.child,
        )?;
        if mouse {
            master.write_all(b"\x1b[<0;168;54M")?;
            std::thread::sleep(Duration::from_millis(20));
            master.write_all(b"\x1b[<")?;
            std::thread::sleep(Duration::from_millis(10));
            master.write_all(b"0;168;54m")?;
        } else {
            master.write_all(b"\x1b[21~")?;
        }
        until(
            &mut master,
            &mut output,
            b"DN_EXIT_READY",
            &format!("{scenario}: shell handoff"),
            &mut fixture.child,
        )?;
        assert!(output
            .windows(b"DN_STATUS:0".len())
            .any(|w| w == b"DN_STATUS:0"));
        let restored_mode = terminal_mode(&slave)?;
        if original_mode.c_iflag != restored_mode.c_iflag
            || original_mode.c_oflag != restored_mode.c_oflag
            || original_mode.c_lflag != restored_mode.c_lflag
            || original_mode.c_cflag != restored_mode.c_cflag
            || original_mode.c_cc != restored_mode.c_cc
        {
            return Err(wait_error(
                scenario,
                "terminal mode was not restored",
                None,
                &output,
            ));
        }
        master.write_all(b"TOKEN\n")?;
        until(
            &mut master,
            &mut output,
            b"DN_INPUT:TOKEN",
            &format!("{scenario}: shell input preserved"),
            &mut fixture.child,
        )?;
        assert!(!output
            .windows(b"0;168;54m".len())
            .any(|w| w == b"0;168;54m"));
        assert!(fixture.child.wait()?.success());
    }
    Ok(())
}

#[cfg(test)]
mod waiting_tests {
    use super::*;
    use std::collections::VecDeque;

    struct ScriptedReader(VecDeque<io::Result<Vec<u8>>>);
    impl Read for ScriptedReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            match self.0.pop_front().unwrap_or(Ok(Vec::new())) {
                Ok(bytes) => {
                    buffer[..bytes.len()].copy_from_slice(&bytes);
                    Ok(bytes.len())
                }
                Err(error) => Err(error),
            }
        }
    }

    #[test]
    fn transient_empty_reads_and_fragmented_output_are_not_hangups() -> io::Result<()> {
        let mut reader = ScriptedReader(VecDeque::from([
            Ok(Vec::new()),
            Err(io::ErrorKind::WouldBlock.into()),
            Err(io::ErrorKind::Interrupted.into()),
            Ok(b"REA".to_vec()),
            Ok(Vec::new()),
            Ok(b"DY".to_vec()),
        ]));
        let mut output = Vec::new();
        wait_for(
            &mut reader,
            &mut output,
            b"READY",
            "ready",
            Duration::from_secs(1),
            || Ok(None),
        )?;
        assert_eq!(output, b"READY");
        Ok(())
    }

    #[test]
    fn child_exit_drains_final_output_before_failing() -> io::Result<()> {
        let mut reader = ScriptedReader(VecDeque::from([Ok(Vec::new()), Ok(b"READY".to_vec())]));
        wait_for(
            &mut reader,
            &mut Vec::new(),
            b"READY",
            "handoff",
            Duration::from_secs(1),
            || Ok(Some("exit status: 0".into())),
        )?;
        let mut reader = ScriptedReader(VecDeque::from([
            Ok(b"\x1b[31mfailed\n".to_vec()),
            Err(io::Error::from_raw_os_error(libc::EIO)),
        ]));
        let error = wait_for(
            &mut reader,
            &mut Vec::new(),
            b"READY",
            "mouse: handoff",
            Duration::from_secs(1),
            || Ok(Some("exit status: 90".into())),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("mouse: handoff"));
        assert!(error.contains("exit status: 90"));
        assert!(error.contains("\\x1b[31mfailed\\n"));
        assert!(!error.contains('\x1b'));
        assert!(error.contains("PTY ended"));
        Ok(())
    }

    #[test]
    fn a_live_child_without_output_times_out() {
        let mut reader = ScriptedReader(VecDeque::new());
        let error = wait_for(
            &mut reader,
            &mut Vec::new(),
            b"READY",
            "manager ready",
            Duration::from_millis(10),
            || Ok(None),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("timed out"));
        assert!(error.contains("child=running"));
        assert!(error.contains("manager ready"));
    }

    #[test]
    fn read_and_status_errors_keep_context() {
        let mut reader =
            ScriptedReader(VecDeque::from([
                Err(io::ErrorKind::PermissionDenied.into()),
            ]));
        let error = wait_for(
            &mut reader,
            &mut b"captured".to_vec(),
            b"READY",
            "read stage",
            Duration::from_secs(1),
            || Ok(None),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("read stage"));
        assert!(error.contains("captured"));
        let mut reader = ScriptedReader(VecDeque::new());
        let error = wait_for(
            &mut reader,
            &mut Vec::new(),
            b"READY",
            "status stage",
            Duration::from_secs(1),
            || Err(io::Error::other("status unavailable")),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("status stage"));
        assert!(error.contains("status unavailable"));
    }

    #[test]
    fn output_growth_is_bounded_even_when_needle_arrives() {
        let mut reader = ScriptedReader(VecDeque::from([Ok(b"READY".to_vec())]));
        let mut output = vec![b'x'; OUTPUT_LIMIT];
        let error = wait_for(
            &mut reader,
            &mut output,
            b"READY",
            "overflow",
            Duration::from_secs(1),
            || Ok(None),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("output limit exceeded"));
        assert!(error.len() < 20 * 1024);
    }
}
