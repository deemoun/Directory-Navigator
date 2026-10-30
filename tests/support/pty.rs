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
    screen: Screen,
}
impl Session {
    pub fn new(label: &str) -> io::Result<Self> {
        Self::with_checkpoint(label, false)
    }
    pub fn new_recovery(label: &str) -> io::Result<Self> {
        Self::with_checkpoint(label, true)
    }
    fn with_checkpoint(label: &str, checkpoint: bool) -> io::Result<Self> {
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
        if checkpoint {
            let root_recovery = root.join("settings/dn/editor-recovery");
            let store = dn::editor::recovery::store::Store::open(&root_recovery)?;
            fs::write(root.join("source.txt"), "original source\n")?;
            dn::editor::recovery::store::write(
                &store.current.dir.join("1.rec"),
                &dn::editor::recovery::Record {
                    text: "recovery 界\n".into(),
                    path: Some(root.join("source.txt")),
                    base: root.clone(),
                    cursor: 0,
                    anchor: None,
                    bom: true,
                    time: 1700000000,
                },
            )?;
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
            screen: Screen::new(80, 25),
        })
    }
    pub fn text(&self) -> String {
        self.screen.text()
    }
    pub fn command_prompt_active(&self) -> bool {
        self.screen.command_prompt_active()
    }
    pub fn select_file(&mut self, name: &str) -> io::Result<()> {
        // A burst of relative arrows assumes a specific selection throughout
        // asynchronous panel preparation. Select and acknowledge a stable name.
        self.send(b"\x1b[18;3~", "fixture quick search", |s| {
            s.text().contains("Find:")
        })?;
        let query = format!("Find: {name}▌");
        self.send(name.as_bytes(), "fixture file selected", |s| {
            let text = s.text();
            text.contains(&query) && !text.contains("No match")
        })?;
        self.send(b"\r", "fixture selection accepted", |s| {
            !s.text().contains("Find:") && s.command_prompt_active()
        })
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
        self.pump()?;
        let previous = self.screen.revision;
        self.master.write_all(bytes)?;
        self.wait(stage, |s| s.screen.revision > previous && ready(s))
    }
    // Shell/output handoff has no DN screen commit. Its assertions inspect raw
    // alternate-screen or shell acknowledgements rather than application pixels.
    pub fn send_output(
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
                    self.screen.pending.extend_from_slice(&bytes[..n]);
                    self.screen.decode();
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
    pub fn resize(&mut self, width: u16, height: u16) -> io::Result<()> {
        self.pump()?;
        self.screen.resize(width as usize, height as usize);
        let size = libc::winsize {
            ws_row: height,
            ws_col: width,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // TIOCSWINSZ notifies the private foreground process group itself.
        // Sending SIGWINCH again can queue a second resize behind test input.
        // SAFETY: valid owned terminal descriptor and initialized winsize.
        unsafe {
            if libc::ioctl(self.slave.as_raw_fd(), libc::TIOCSWINSZ as _, &size) < 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }
    pub fn finish(&mut self) -> io::Result<()> {
        self.send_output(b"\x1b[21~", "normal exit", |s| {
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
        self.send_output(b"RESTORED\n", "shell accepts input", |s| {
            s.occurrences(b"DN_INPUT:RESTORED") > 0
        })?;
        assert!(self.child.wait()?.success());
        Ok(())
    }
}
// Transport::write_raw terminates each complete pixel batch with this reset.
// A title or footer can arrive several PTY reads before that terminator.
const FRAME_END: &[u8] = b"\x1b[39m\x1b[49m\x1b[0m";
struct Screen {
    pending: Vec<u8>,
    cells: Vec<String>,
    committed: Vec<String>,
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    cursor_visible: bool,
    revision: u64,
}
impl Screen {
    fn new(width: usize, height: usize) -> Self {
        Self {
            pending: vec![],
            cells: vec![" ".into(); width * height],
            committed: vec![" ".into(); width * height],
            width,
            height,
            x: 0,
            y: 0,
            cursor_visible: false,
            revision: 0,
        }
    }
    fn text(&self) -> String {
        self.committed
            .chunks(self.width)
            .map(|row| row.concat())
            .collect::<Vec<_>>()
            .join("\n")
    }
    fn command_prompt_active(&self) -> bool {
        self.cursor_visible && self.y == self.height - 2
    }
    fn resize(&mut self, width: usize, height: usize) {
        self.width = width;
        self.height = height;
        self.cells = vec![" ".into(); width * height];
        self.committed = self.cells.clone();
        self.cursor_visible = false;
    }
    fn commit(&mut self) {
        self.committed.clone_from(&self.cells);
        self.revision += 1;
    }
    fn decode(&mut self) {
        let mut i = 0;
        while i < self.pending.len() {
            let b = self.pending[i];
            if b == 27 {
                let remaining = &self.pending[i..];
                if remaining.starts_with(FRAME_END) {
                    self.commit();
                    i += FRAME_END.len();
                    continue;
                }
                if FRAME_END.starts_with(remaining) {
                    break;
                }
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
                    if raw == b"?25" {
                        if self.pending[end] == b'h' {
                            self.cursor_visible = true;
                            // Showing/repositioning the cursor can be the only
                            // output when an exposed manager gains focus.
                            self.commit();
                        } else if self.pending[end] == b'l' {
                            self.cursor_visible = false;
                        }
                    } else if !raw.starts_with(b"?") {
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

#[cfg(test)]
mod tests {
    use super::{Screen, FRAME_END};
    fn feed(screen: &mut Screen, bytes: &[u8]) {
        screen.pending.extend_from_slice(bytes);
        screen.decode();
    }
    #[test]
    fn partial_screen_and_split_frame_terminator_cannot_satisfy_a_new_step() {
        for split in 0..FRAME_END.len() {
            let mut s = Screen::new(80, 25);
            feed(&mut s, b"\x1b[1;1HColors");
            feed(&mut s, FRAME_END);
            let previous = s.revision;
            feed(&mut s, b"\x1b[2;1HArrows move");
            // Even though the title already matches, this is still the old
            // committed screen until the entire new pixel batch arrives.
            assert!(s.text().contains("Colors"));
            assert!(!s.text().contains("Arrows move"));
            feed(&mut s, &FRAME_END[..split]);
            assert_eq!(s.revision, previous);
            assert!(!s.text().contains("Arrows move"));
            feed(&mut s, &FRAME_END[split..]);
            assert_eq!(s.revision, previous + 1);
            assert!(s.text().contains("Arrows move"));
        }
    }
    #[test]
    fn background_function_bar_is_not_manager_focus_and_cursor_only_reply_is_valid() {
        let mut s = Screen::new(80, 25);
        feed(&mut s, b"\x1b[?25l\x1b[25;1HF5 Copy");
        feed(&mut s, FRAME_END);
        assert!(s.text().contains("F5 Copy"));
        assert!(!s.command_prompt_active());
        let previous = s.revision;
        feed(&mut s, b"\x1b[24;3H\x1b[?25");
        assert!(!s.command_prompt_active());
        assert_eq!(s.revision, previous);
        feed(&mut s, b"h");
        assert!(s.command_prompt_active());
        assert!(s.revision > previous);
    }
    #[test]
    fn old_resize_frame_and_byte_fragmented_unicode_do_not_publish_partial_screens() {
        let mut s = Screen::new(80, 25);
        s.resize(100, 32);
        feed(&mut s, b"\x1b[25;1HF10 Quit");
        feed(&mut s, FRAME_END);
        feed(&mut s, b"\x1b[24;1H\x1b[?25h");
        assert!(!s.command_prompt_active());
        assert!(!s.text().lines().last().unwrap().contains("F10 Quit"));
        let previous = s.revision;
        for byte in "\x1b[1;1H界 e\u{301}\x1b[32;1HF10 Quit".as_bytes() {
            feed(&mut s, &[*byte]);
            assert_eq!(s.revision, previous);
        }
        for byte in FRAME_END {
            feed(&mut s, &[*byte]);
        }
        assert!(s.text().contains("界 e\u{301}"));
        assert!(s.text().lines().last().unwrap().contains("F10 Quit"));
        feed(&mut s, b"\x1b[31;1H\x1b[?25h");
        assert!(s.command_prompt_active());
    }
}
