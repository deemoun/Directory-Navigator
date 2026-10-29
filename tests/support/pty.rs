//! Small real-terminal harness. No external interpreter, real home or user files.
use std::{
    fs::{self, File},
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::process::CommandExt,
    },
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
use unicode_width::UnicodeWidthChar;

pub struct Session {
    pub root: PathBuf,
    child: Child,
    master: File,
    slave: File,
    original: libc::termios,
    output: Vec<u8>,
    pending: Vec<u8>,
    cells: Vec<String>,
    width: usize,
    height: usize,
    x: usize,
    y: usize,
}
impl Session {
    pub fn new(label: &str) -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "dn-walk-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("settings/dn"))?;
        fs::write(
            root.join("settings/dn/config.conf"),
            "config_version=2\n[application]\nsave_on_exit=false\nshortcuts=\"classic_dn\"\n",
        )?;
        fs::create_dir(root.join("inside"))?;
        fs::write(root.join("inside/child.txt"), "child\n")?;
        for n in 0..65 {
            fs::write(root.join(format!("item-{n:03}.txt")), "fixture\n")?;
        }
        let (mut master, mut slave) = (0, 0);
        let mut size = libc::winsize {
            ws_row: 25,
            ws_col: 80,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // SAFETY: openpty initializes distinct owned descriptors; size is valid storage.
        if unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut size,
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: descriptors were newly allocated by openpty.
        let master = unsafe { File::from_raw_fd(master) };
        let slave = unsafe { File::from_raw_fd(slave) };
        let original = mode(&slave)?;
        // SAFETY: preserve the live descriptor's flags and add nonblocking reads.
        unsafe {
            let flags = libc::fcntl(master.as_raw_fd(), libc::F_GETFL);
            if flags < 0
                || libc::fcntl(master.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) < 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "\"$1\"; dn_result=$?; printf '\\nDN_STATUS:%s\\n' \"$dn_result\"; IFS= read -r token; printf '\\nDN_INPUT:%s\\n' \"$token\"; exit \"$dn_result\"", "dn-walk", env!("CARGO_BIN_EXE_dn")])
            .current_dir(&root).env("TERM","xterm-256color").env("XDG_CONFIG_HOME",root.join("settings"))
            .stdin(Stdio::from(slave.try_clone()?)).stdout(Stdio::from(slave.try_clone()?)).stderr(Stdio::from(slave.try_clone()?));
        // SAFETY: only async-signal-safe calls are made between fork and exec.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = match command.spawn() {
            Ok(c) => c,
            Err(e) => {
                let _ = fs::remove_dir_all(&root);
                return Err(e);
            }
        };
        Ok(Self {
            root,
            child,
            master,
            slave,
            original,
            output: vec![],
            pending: vec![],
            cells: vec![" ".into(); 80 * 25],
            width: 80,
            height: 25,
            x: 0,
            y: 0,
        })
    }
    pub fn text(&self) -> String {
        self.cells
            .chunks(self.width)
            .map(|row| row.concat())
            .collect::<Vec<_>>()
            .join("\n")
    }
    pub fn occurrences(&self, bytes: &[u8]) -> usize {
        self.output
            .windows(bytes.len())
            .filter(|w| *w == bytes)
            .count()
    }
    pub fn send(
        &mut self,
        bytes: &[u8],
        stage: &str,
        ready: impl Fn(&Self) -> bool,
    ) -> io::Result<()> {
        self.master.write_all(bytes)?;
        self.wait(stage, ready)
    }
    pub fn wait(&mut self, stage: &str, ready: impl Fn(&Self) -> bool) -> io::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            self.pump()?;
            if ready(self) {
                return Ok(());
            }
            if Instant::now() >= deadline || self.child.try_wait()?.is_some() {
                return Err(io::Error::other(format!(
                    "{stage}: child={:?}\n{}",
                    self.child.try_wait()?,
                    self.text()
                )));
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    fn pump(&mut self) -> io::Result<()> {
        let mut bytes = [0; 16384];
        loop {
            match self.master.read(&mut bytes) {
                Ok(0) => break,
                Ok(n) => {
                    self.output.extend_from_slice(&bytes[..n]);
                    if self.output.len() > 2 * 1024 * 1024 {
                        return Err(io::Error::other("terminal output exceeded limit"));
                    }
                    self.pending.extend_from_slice(&bytes[..n]);
                    self.decode();
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) || e.raw_os_error() == Some(libc::EIO) =>
                {
                    break
                }
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
    fn decode(&mut self) {
        let mut i = 0;
        while i < self.pending.len() {
            let b = self.pending[i];
            if b == 27 {
                if i + 1 >= self.pending.len() {
                    break;
                }
                if self.pending[i + 1] == b'[' {
                    let Some(end) = (i + 2..self.pending.len())
                        .find(|&j| (0x40..=0x7e).contains(&self.pending[j]))
                    else {
                        break;
                    };
                    let raw = &self.pending[i + 2..end];
                    if !raw.starts_with(b"?") {
                        let nums = String::from_utf8_lossy(raw)
                            .split(';')
                            .map(|s| s.parse::<usize>().unwrap_or(0))
                            .collect::<Vec<_>>();
                        let n = nums[0].max(1);
                        match self.pending[end] {
                            b'H' | b'f' => {
                                self.y = n - 1;
                                self.x = nums.get(1).copied().unwrap_or(1).max(1) - 1;
                            }
                            b'J' if nums[0] == 2 => self.cells.fill(" ".into()),
                            b'K' => {
                                if self.y < self.height {
                                    for x in self.x.min(self.width)..self.width {
                                        self.cells[self.y * self.width + x] = " ".into();
                                    }
                                }
                            }
                            b'A' => self.y = self.y.saturating_sub(n),
                            b'B' => self.y = self.y.saturating_add(n),
                            b'C' => self.x = self.x.saturating_add(n),
                            b'D' => self.x = self.x.saturating_sub(n),
                            _ => {}
                        }
                    }
                    i = end + 1;
                    continue;
                }
                i += 2;
                continue;
            }
            if b < 32 {
                match b {
                    b'\r' => self.x = 0,
                    b'\n' => self.y += 1,
                    _ => {}
                }
                i += 1;
                continue;
            }
            let len = match b {
                0..=127 => 1,
                0xc0..=0xdf => 2,
                0xe0..=0xef => 3,
                0xf0..=0xf7 => 4,
                _ => 1,
            };
            if i + len > self.pending.len() {
                break;
            }
            let ch = std::str::from_utf8(&self.pending[i..i + len])
                .ok()
                .and_then(|s| s.chars().next())
                .unwrap_or('\u{fffd}');
            let width = ch.width().unwrap_or(0);
            if self.y < self.height {
                if width == 0 && self.x > 0 && self.x <= self.width {
                    self.cells[self.y * self.width + self.x - 1].push(ch);
                } else if self.x < self.width {
                    self.cells[self.y * self.width + self.x] = ch.to_string();
                    if width == 2 && self.x + 1 < self.width {
                        self.cells[self.y * self.width + self.x + 1].clear();
                    }
                }
            }
            self.x = self.x.saturating_add(width);
            i += len;
        }
        self.pending.drain(..i);
    }
    pub fn resize(&mut self, width: u16, height: u16) -> io::Result<()> {
        self.width = width as usize;
        self.height = height as usize;
        self.cells = vec![" ".into(); self.width * self.height];
        let size = libc::winsize {
            ws_row: height,
            ws_col: width,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // SAFETY: valid descriptor, winsize and private child session/process group.
        unsafe {
            if libc::ioctl(self.slave.as_raw_fd(), libc::TIOCSWINSZ as _, &size) < 0 {
                return Err(io::Error::last_os_error());
            }
            if libc::kill(-(self.child.id() as i32), libc::SIGWINCH) < 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }
    pub fn finish(&mut self) -> io::Result<()> {
        self.send(b"\x1b[21~", "normal exit", |s| {
            s.occurrences(b"DN_STATUS:0") > 0
        })?;
        let restored = mode(&self.slave)?;
        assert_eq!(
            (
                self.original.c_iflag,
                self.original.c_oflag,
                self.original.c_cflag,
                self.original.c_lflag,
                self.original.c_cc
            ),
            (
                restored.c_iflag,
                restored.c_oflag,
                restored.c_cflag,
                restored.c_lflag,
                restored.c_cc
            )
        );
        self.send(b"RESTORED\n", "shell accepts input", |s| {
            s.occurrences(b"DN_INPUT:RESTORED") > 0
        })?;
        assert!(self.child.wait()?.success());
        Ok(())
    }
}
fn mode(f: &File) -> io::Result<libc::termios> {
    let mut mode = std::mem::MaybeUninit::uninit();
    // SAFETY: tcgetattr initializes the storage on success for this live terminal.
    if unsafe { libc::tcgetattr(f.as_raw_fd(), mode.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: initialization succeeded above.
    Ok(unsafe { mode.assume_init() })
}
impl Drop for Session {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            // SAFETY: this test created the child's private session/process group.
            unsafe {
                libc::kill(-(self.child.id() as i32), libc::SIGKILL);
            }
        }
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.root);
    }
}
