//! Bounded, nonblocking socket lifecycle. Only the Linux probe touches a real tty.
use super::*;
#[cfg(target_os = "linux")]
use std::sync::{Arc, Mutex};
use std::{
    io::{Read, Write},
    os::{fd::AsRawFd, unix::net::UnixStream},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
#[cfg(target_os = "linux")]
use std::{
    os::{
        fd::{FromRawFd, OwnedFd},
        unix::ffi::OsStrExt,
    },
    sync::atomic::{AtomicI32, Ordering},
};
const CONNECT_TIMEOUT: Duration = Duration::from_millis(250);
const RETRY: Duration = Duration::from_secs(2);
const PROBE: Duration = Duration::from_millis(100);
#[cfg(target_os = "linux")]
static PANIC_SOCKET: AtomicI32 = AtomicI32::new(-1);

#[cfg(target_os = "linux")]
pub(crate) type Shared = Arc<Mutex<Controller>>;
struct Client {
    stream: UnixStream,
    connecting: bool,
    request: [u8; GPM_CONNECT_BYTES],
    sent: usize,
    deadline: Instant,
    decoder: Decoder,
    mapper: Mapper,
}
impl Client {
    fn new(stream: UnixStream, connecting: bool, vc: u16, now: Instant) -> io::Result<Self> {
        stream.set_nonblocking(true)?;
        Ok(Self {
            stream,
            connecting,
            request: encode_connection(u32::from(vc), std::process::id() as i32),
            sent: 0,
            deadline: now + CONNECT_TIMEOUT,
            decoder: Default::default(),
            mapper: Default::default(),
        })
    }
    fn handshake(&mut self, now: Instant) -> io::Result<bool> {
        if self.sent == self.request.len() {
            return Ok(true);
        }
        if now >= self.deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "GPM connection timed out",
            ));
        }
        if self.connecting {
            let mut pollfd = libc::pollfd {
                fd: self.stream.as_raw_fd(),
                events: libc::POLLOUT,
                revents: 0,
            };
            // SAFETY: a live, owned descriptor and one initialized pollfd; never waits.
            if unsafe { libc::poll(&mut pollfd, 1, 0) } < 0 {
                return Err(io::Error::last_os_error());
            }
            if pollfd.revents == 0 {
                return Ok(false);
            }
            if let Some(e) = self.stream.take_error()? {
                return Err(e);
            }
            self.connecting = false;
        }
        match self.stream.write(&self.request[self.sent..]) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "GPM handshake stopped",
                ))
            }
            Ok(n) => self.sent += n,
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                return Ok(false)
            }
            Err(e) => return Err(e),
        }
        Ok(self.sent == self.request.len())
    }
    fn drain(
        &mut self,
        vc: u16,
        size: (u16, u16),
        pending: &mut VecDeque<MouseEvent>,
    ) -> io::Result<()> {
        // At most 16 packets / 64 reads, including fragmented and interrupted reads.
        // Reserve the maximum expansion of a wheel packet before consuming one.
        let mut packets = 0;
        for _ in 0..64 {
            if pending.len() > MAX_PENDING - MAX_EVENTS_PER_PACKET || packets == 16 {
                break;
            }
            if let Some(event) = self.decoder.take() {
                self.mapper
                    .map_into(event, vc, size, pending)
                    .map_err(io::Error::other)?;
                packets += 1;
                continue;
            }
            let needed = self.decoder.needed();
            let mut bytes = [0; 32];
            match self.stream.read(&mut bytes[..needed]) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "GPM disconnected",
                    ))
                }
                Ok(n) => self.decoder.bytes.extend_from_slice(&bytes[..n]),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
}

pub(crate) struct Controller {
    pub(crate) snapshot: Snapshot,
    vc: u16,
    socket: PathBuf,
    client: Option<Client>,
    pending: VecDeque<MouseEvent>,
    owned: bool,
    active: bool,
    prefer_keyboard: bool,
    size: Option<(u16, u16)>,
    attempt: Instant,
    probe: Instant,
    #[cfg(target_os = "linux")]
    console: Option<OwnedFd>,
}
impl Controller {
    fn new(vc: u16, socket: PathBuf, now: Instant) -> Self {
        Self {
            snapshot: Snapshot {
                console: vc != 0,
                reason: if vc == 0 {
                    "Not a Linux virtual console"
                } else {
                    "Waiting for GPM"
                }
                .into(),
                ..Default::default()
            },
            vc,
            socket,
            client: None,
            pending: VecDeque::new(),
            owned: true,
            active: false,
            prefer_keyboard: true,
            size: None,
            attempt: now,
            probe: now,
            #[cfg(target_os = "linux")]
            console: None,
        }
    }
    fn disconnect(&mut self, reason: String) {
        // Unregister before dropping the descriptor: a panic on another thread
        // must not see a closed fd that the process could already have reused.
        let client = self.client.take();
        if client.is_some() || self.snapshot.connected || self.snapshot.position.is_some() {
            #[cfg(target_os = "linux")]
            PANIC_SOCKET.store(-1, Ordering::Relaxed);
            self.snapshot.reset = self.snapshot.reset.wrapping_add(1);
        }
        self.pending.clear();
        self.snapshot.connected = false;
        self.snapshot.position = None;
        self.snapshot.pressed = 0;
        self.snapshot.reason = reason;
    }
    pub(crate) fn suspend(&mut self) {
        self.owned = false;
        if self.snapshot.console {
            self.disconnect("Suspended".into());
        }
    }
    pub(crate) fn resume(&mut self) {
        self.owned = true;
        self.active = false;
        self.probe = Instant::now();
        self.attempt = self.attempt.max(self.probe);
    }
    fn service(
        &mut self,
        active: bool,
        now: Instant,
        size: (u16, u16),
        connect: impl FnOnce(&Path) -> io::Result<(UnixStream, bool)>,
    ) {
        if self.vc == 0 || !self.owned {
            return;
        }
        if self.size.is_some_and(|previous| previous != size) {
            self.pending.clear();
            if let Some(client) = &mut self.client {
                client.mapper.pressed = 0;
            }
            self.snapshot.pressed = 0;
            self.snapshot.position = self.snapshot.position.and_then(|(x, y)| {
                (size.0 != 0 && size.1 != 0).then(|| (x.min(size.0 - 1), y.min(size.1 - 1)))
            });
            self.snapshot.reset = self.snapshot.reset.wrapping_add(1);
        }
        self.size = Some(size);
        if !active {
            self.active = false;
            self.disconnect("Inactive virtual console".into());
            return;
        }
        if !self.active {
            self.active = true;
            self.attempt = self.attempt.max(now);
        }
        if self.client.is_none() && now >= self.attempt {
            self.attempt = now + RETRY;
            match connect(&self.socket)
                .and_then(|(stream, connecting)| Client::new(stream, connecting, self.vc, now))
            {
                Ok(c) => {
                    #[cfg(target_os = "linux")]
                    if self.console.is_some() {
                        PANIC_SOCKET.store(c.stream.as_raw_fd(), Ordering::Relaxed);
                    }
                    self.client = Some(c);
                    self.snapshot.reason = "Connecting to GPM".into();
                }
                Err(e) => {
                    self.snapshot.reason = format!("GPM unavailable: {e}");
                }
            }
        }
        if let Some(c) = &mut self.client {
            let result = c.handshake(now).and_then(|ready| {
                if ready {
                    self.snapshot.connected = true;
                    self.snapshot.reason = "Connected".into();
                    c.drain(self.vc, size, &mut self.pending)?;
                }
                Ok(())
            });
            if let Err(e) = result {
                self.disconnect(format!("GPM unavailable: {e}"));
                self.attempt = now + RETRY;
            }
        }
    }
    fn pop(&mut self) -> Option<crossterm::event::Event> {
        let e = self.pending.pop_front()?;
        if self.size.is_some_and(|(w, h)| e.column < w && e.row < h) {
            self.snapshot.position = Some((e.column, e.row));
        }
        let bit = |button| match button {
            MouseButton::Left => GPM_BUTTON_LEFT,
            MouseButton::Middle => GPM_BUTTON_MIDDLE,
            MouseButton::Right => GPM_BUTTON_RIGHT,
        };
        match e.kind {
            MouseEventKind::Down(b) => self.snapshot.pressed |= bit(b),
            MouseEventKind::Up(b) => self.snapshot.pressed &= !bit(b),
            _ => {}
        }
        Some(crossterm::event::Event::Mouse(e))
    }
    #[cfg(target_os = "linux")]
    pub(crate) fn current() -> Shared {
        let console = console_fd();
        let mut state = Self::new(
            console.as_ref().map_or(0, |(_, vc)| *vc),
            PathBuf::from(GPM_CONTROL_SOCKET),
            Instant::now(),
        );
        state.console = console.map(|(fd, _)| fd);
        Arc::new(Mutex::new(state))
    }
    #[cfg(target_os = "linux")]
    fn tick(&mut self, size: (u16, u16)) {
        let now = Instant::now();
        if self.snapshot.console && self.owned {
            if now >= self.probe {
                self.probe = now + PROBE;
                let active = match self
                    .console
                    .as_ref()
                    .map(|fd| active_console(fd.as_raw_fd()))
                {
                    Some(Ok(vc)) => vc == self.vc,
                    Some(Err(error)) => {
                        self.active = false;
                        self.disconnect(format!("GPM unavailable: VT_GETSTATE: {error}"));
                        return;
                    }
                    None => false,
                };
                if active && !self.active {
                    self.attempt = self.attempt.max(now);
                }
                self.active = active;
                if !self.active {
                    self.disconnect("Inactive virtual console".into());
                }
            }
            if self.active {
                self.service(true, now, size, connect_socket);
            }
        }
    }
    #[cfg(target_os = "linux")]
    pub(crate) fn input(
        &mut self,
        timeout: Duration,
        size: (u16, u16),
    ) -> io::Result<Option<crossterm::event::Event>> {
        self.multiplex(
            timeout,
            |s| s.tick(size),
            |wait| {
                if crossterm::event::poll(wait)? {
                    crossterm::event::read().map(Some)
                } else {
                    Ok(None)
                }
            },
        )
    }
    fn multiplex(
        &mut self,
        timeout: Duration,
        mut refresh: impl FnMut(&mut Self),
        mut terminal: impl FnMut(Duration) -> io::Result<Option<crossterm::event::Event>>,
    ) -> io::Result<Option<crossterm::event::Event>> {
        use crossterm::event::Event;
        if !self.snapshot.console {
            return terminal(timeout);
        }
        refresh(self);
        if self.prefer_keyboard {
            if let Some(e) = terminal(Duration::ZERO)? {
                self.prefer_keyboard = false;
                return Ok((!matches!(e, Event::Mouse(_))).then_some(e));
            }
        }
        if let Some(e) = self.pop() {
            self.prefer_keyboard = true;
            return Ok(Some(e));
        }
        if let Some(e) = terminal(self.timeout(timeout))? {
            self.prefer_keyboard = false;
            return Ok((!matches!(e, Event::Mouse(_))).then_some(e));
        }
        refresh(self);
        self.prefer_keyboard = true;
        Ok(self.pop())
    }
    fn timeout(&self, timeout: Duration) -> Duration {
        if self.snapshot.console && self.owned {
            timeout.min(if self.client.is_some() {
                Duration::from_millis(20)
            } else {
                PROBE
            })
        } else {
            timeout
        }
    }
}

#[cfg(target_os = "linux")]
fn connect_socket(path: &Path) -> io::Result<(UnixStream, bool)> {
    let name = path.as_os_str().as_bytes();
    // SAFETY: initialized sockaddr_un; socket is owned immediately on success.
    let mut addr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    if name.len() >= addr.sun_path.len() || name.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid GPM socket path",
        ));
    }
    addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (dest, src) in addr.sun_path.iter_mut().zip(name) {
        *dest = *src as libc::c_char;
    }
    // SAFETY: flags and protocol are valid for a Linux Unix stream socket.
    let fd = unsafe {
        libc::socket(
            libc::AF_UNIX,
            libc::SOCK_STREAM | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
            0,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: socket returned a fresh owned descriptor; conversion transfers ownership.
    let stream = unsafe { UnixStream::from_raw_fd(fd) };
    // SAFETY: addr contains a terminated path of the stated initialized length.
    let result = unsafe {
        libc::connect(
            fd,
            (&addr as *const libc::sockaddr_un).cast(),
            (std::mem::offset_of!(libc::sockaddr_un, sun_path) + name.len() + 1) as libc::socklen_t,
        )
    };
    let connecting = if result == 0 {
        false
    } else {
        let e = io::Error::last_os_error();
        if matches!(e.raw_os_error(), Some(libc::EINPROGRESS | libc::EAGAIN)) {
            true
        } else {
            return Err(e);
        }
    };
    Ok((stream, connecting))
}
#[cfg(target_os = "linux")]
fn console_fd() -> Option<(OwnedFd, u16)> {
    probe_console(0, 1)
}
#[cfg(target_os = "linux")]
fn probe_console(input: i32, output: i32) -> Option<(OwnedFd, u16)> {
    const KDGKBTYPE: libc::c_ulong = 0x4b33;
    let mut kind: libc::c_char = 0;
    let mut dev: libc::c_uint = 0;
    let mut output_dev: libc::c_uint = 0;
    // SAFETY: ioctl outputs refer to initialized objects of Linux's documented sizes.
    if unsafe { libc::ioctl(input, KDGKBTYPE, &mut kind) } < 0
        || unsafe { libc::ioctl(input, libc::TIOCGDEV, &mut dev) } < 0
        || unsafe { libc::ioctl(output, libc::TIOCGDEV, &mut output_dev) } < 0
        || dev != output_dev
    {
        return None;
    }
    let device = dev as libc::dev_t;
    if libc::major(device) != 4 || !(1..=63).contains(&libc::minor(device)) {
        return None;
    }
    // SAFETY: duplicate stdin, owning only the duplicate; never read it.
    let fd = unsafe { libc::fcntl(input, libc::F_DUPFD_CLOEXEC, 3) };
    (fd >= 0).then(|| {
        (
            unsafe { OwnedFd::from_raw_fd(fd) },
            libc::minor(device) as u16,
        )
    })
}
#[cfg(target_os = "linux")]
fn active_console(fd: i32) -> io::Result<u16> {
    #[repr(C)]
    struct State {
        active: u16,
        signal: u16,
        state: u16,
    }
    let mut state = State {
        active: 0,
        signal: 0,
        state: 0,
    };
    // SAFETY: VT_GETSTATE writes precisely this Linux vt_stat structure.
    if unsafe { libc::ioctl(fd, 0x5603 as libc::c_ulong, &mut state) } < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(state.active)
    }
}
#[cfg(target_os = "linux")]
pub(crate) fn panic_close() {
    let fd = PANIC_SOCKET.swap(-1, Ordering::Relaxed);
    if fd >= 0 {
        // SAFETY: this is the sole main-thread session's registered socket.
        // Shutdown releases daemon capture without double-closing its owned fd.
        unsafe {
            libc::shutdown(fd, libc::SHUT_RDWR);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::Cell,
        fs,
        os::unix::net::UnixListener,
        sync::atomic::{AtomicU64, Ordering},
    };
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let p = std::env::temp_dir().join(format!(
                "dn-gpm-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&p).unwrap();
            Self(p)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn connect(path: &Path) -> io::Result<(UnixStream, bool)> {
        UnixStream::connect(path).map(|s| (s, false))
    }
    fn packet(kind: u32, button: u8) -> [u8; 28] {
        let mut b = [0; 28];
        b[0] = button;
        b[2..4].copy_from_slice(&7u16.to_ne_bytes());
        b[8..10].copy_from_slice(&5i16.to_ne_bytes());
        b[10..12].copy_from_slice(&6i16.to_ne_bytes());
        b[12..16].copy_from_slice(&kind.to_ne_bytes());
        b
    }
    fn fixture() -> (Temp, UnixListener, Controller, Instant) {
        let t = Temp::new();
        let path = t.0.join("gpmctl");
        let listener = UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let now = Instant::now();
        let c = Controller::new(7, path, now);
        (t, listener, c, now)
    }
    fn accept(listener: &UnixListener) -> UnixStream {
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut peer = loop {
            match listener.accept() {
                Ok((peer, _)) => break peer,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    assert!(
                        Instant::now() < deadline,
                        "private daemon connection timed out"
                    );
                    std::thread::yield_now();
                }
                Err(e) => panic!("private daemon accept: {e}"),
            }
        };
        peer.set_nonblocking(false).unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let mut request = [0; 16];
        peer.read_exact(&mut request).unwrap();
        assert_eq!(read_i32(&request, 12), 7);
        assert_eq!(read_u16(&request, 2), 0);
        peer
    }
    #[test]
    fn private_socket_fragments_are_nonblocking_and_disconnect_cancels_owned_state() {
        let (_t, listener, mut c, now) = fixture();
        c.service(true, now, (80, 25), connect);
        let mut peer = accept(&listener);
        assert!(c.snapshot.connected);
        let packet = packet(GPM_DOWN, 4);
        peer.write_all(&packet[..2]).unwrap();
        c.service(true, now, (80, 25), connect);
        assert!(c.pop().is_none());
        peer.write_all(&packet[2..]).unwrap();
        c.service(true, now, (80, 25), connect);
        assert!(matches!(
            c.pop(),
            Some(crossterm::event::Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                ..
            }))
        ));
        assert_eq!(c.snapshot.pressed, 4);
        peer.shutdown(std::net::Shutdown::Write).unwrap();
        c.service(true, now, (80, 25), connect);
        assert!(!c.snapshot.connected);
        assert_eq!(c.snapshot.pressed, 0);
        assert!(c.snapshot.position.is_none());
        assert_eq!(c.snapshot.reset, 1);
        assert!(c.pop().is_none());
    }
    #[test]
    fn retries_are_rate_limited_and_vt_suspend_releases_daemon_without_keyboard_input() {
        let (_t, listener, mut c, now) = fixture();
        let calls = Cell::new(0);
        let fail = |_: &Path| {
            calls.set(calls.get() + 1);
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "fixture denied",
            ))
        };
        c.service(true, now, (80, 25), fail);
        for offset in [10, 100, 1000, 1999] {
            c.service(true, now + Duration::from_millis(offset), (80, 25), fail);
        }
        assert_eq!(calls.get(), 1);
        c.service(true, now + RETRY, (80, 25), connect);
        let mut peer = accept(&listener);
        c.suspend();
        assert!(!c.owned);
        assert!(!c.snapshot.connected);
        let mut b = [0];
        assert_eq!(peer.read(&mut b).unwrap(), 0);
        c.service(true, now + RETRY * 2, (80, 25), fail);
        assert_eq!(calls.get(), 1);
        c.resume();
        c.service(false, now + RETRY * 2, (80, 25), fail);
        assert_eq!(calls.get(), 1);
        assert_eq!(c.snapshot.reason, "Inactive virtual console");
        c.service(true, now + RETRY * 3, (80, 25), connect);
        let _peer = accept(&listener);
        assert!(c.snapshot.connected);
    }
    #[test]
    fn connection_deadline_and_unknown_packets_leave_keyboard_fallback() {
        let (_t, listener, mut c, now) = fixture();
        let (a, _b) = UnixStream::pair().unwrap();
        let mut client = Client::new(a, true, 7, now).unwrap();
        assert_eq!(
            client.handshake(now + CONNECT_TIMEOUT).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        c.service(true, now, (80, 25), connect);
        let mut peer = accept(&listener);
        peer.write_all(&packet(0x2000, 4)).unwrap();
        c.service(true, now, (80, 25), connect);
        assert!(!c.snapshot.connected);
        assert!(c.snapshot.reason.contains("Invalid GPM"));
    }
    #[test]
    fn queue_and_packet_budgets_keep_all_clicks_in_order() {
        let (_t, listener, mut c, now) = fixture();
        c.service(true, now, (80, 25), connect);
        let mut peer = accept(&listener);
        let bytes = (0..180)
            .flat_map(|_| [packet(GPM_DOWN, 4), packet(GPM_UP, 4)].concat())
            .collect::<Vec<_>>();
        peer.set_nonblocking(true).unwrap();
        let mut written = 0;
        let mut peak = 0;
        let mut events = Vec::new();
        for turn in 0..1000 {
            if written < bytes.len() {
                let end = (written + 512).min(bytes.len());
                match peer.write(&bytes[written..end]) {
                    Ok(n) => {
                        assert!(n > 0);
                        written += n;
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                    Err(e) => panic!("fixture write: {e}"),
                }
            }
            c.service(true, now, (80, 25), connect);
            assert!(c.pending.len() <= MAX_PENDING);
            peak = peak.max(c.pending.len());
            if turn >= 24 {
                while let Some(crossterm::event::Event::Mouse(e)) = c.pop() {
                    events.push(e.kind);
                }
            }
            if events.len() == 360 {
                break;
            }
        }
        assert!(peak > 220, "fixture must exercise backpressure");
        assert_eq!(written, bytes.len());
        assert_eq!(events.len(), 360);
        for pair in events.as_chunks::<2>().0 {
            assert_eq!(
                *pair,
                [
                    MouseEventKind::Down(MouseButton::Left),
                    MouseEventKind::Up(MouseButton::Left)
                ]
            );
        }
    }
    #[test]
    fn keyboard_and_mouse_share_transport_fairly_without_terminal_duplicates() {
        use crossterm::event::{Event, KeyCode, KeyEvent};
        let (_t, listener, mut c, now) = fixture();
        c.service(true, now, (80, 25), connect);
        let _peer = accept(&listener);
        let key = Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        let movement = mouse_event(MouseEventKind::Moved, 3, 4, KeyModifiers::NONE);
        for _ in 0..4 {
            c.pending.push_back(movement);
            assert_eq!(
                c.multiplex(Duration::from_secs(1), |_| {}, |_| Ok(Some(key.clone())))
                    .unwrap(),
                Some(key.clone())
            );
            assert_eq!(
                c.multiplex(Duration::from_secs(1), |_| {}, |_| panic!("mouse turn"))
                    .unwrap(),
                Some(Event::Mouse(movement))
            );
        }
        let mut waits = Vec::new();
        assert!(c
            .multiplex(
                Duration::from_secs(1),
                |_| {},
                |wait| {
                    waits.push(wait);
                    Ok(None)
                }
            )
            .unwrap()
            .is_none());
        assert_eq!(waits, [Duration::ZERO, Duration::from_millis(20)]);
        assert!(c
            .multiplex(Duration::ZERO, |_| {}, |_| Ok(Some(Event::Mouse(movement))))
            .unwrap()
            .is_none());
        c.disconnect("fixture disconnect".into());
        assert_eq!(c.timeout(Duration::from_secs(1)), PROBE);
        assert_eq!(
            c.multiplex(Duration::ZERO, |_| {}, |_| Ok(Some(key.clone())))
                .unwrap(),
            Some(key)
        );
        let mut terminal = Controller::new(0, PathBuf::new(), now);
        terminal.suspend();
        terminal.resume();
        assert_eq!(terminal.snapshot.reason, "Not a Linux virtual console");
        assert_eq!(
            terminal
                .multiplex(
                    Duration::from_secs(1),
                    |_| panic!("PTY must not probe GPM"),
                    |wait| {
                        assert_eq!(wait, Duration::from_secs(1));
                        Ok(Some(Event::Mouse(movement)))
                    }
                )
                .unwrap(),
            Some(Event::Mouse(movement))
        );
    }
    #[test]
    fn resize_discards_stale_clicks_and_clamps_pointer_without_replaying_capture() {
        let (_t, listener, mut c, now) = fixture();
        c.service(true, now, (80, 25), connect);
        let mut peer = accept(&listener);
        peer.write_all(&[packet(GPM_DOWN, 4), packet(GPM_DRAG, 4)].concat())
            .unwrap();
        c.service(true, now, (80, 25), connect);
        assert!(c.pop().is_some());
        assert_eq!(c.snapshot.pressed, 4);
        c.service(true, now, (2, 2), connect);
        assert!(c.pop().is_none());
        assert_eq!(c.snapshot.position, Some((1, 1)));
        assert_eq!(c.snapshot.pressed, 0);
        assert_eq!(c.snapshot.reset, 1);
        peer.write_all(&packet(GPM_DRAG, 4)).unwrap();
        c.service(true, now, (2, 2), connect);
        assert!(c.pop().is_none());
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn pty_and_socket_cannot_impersonate_a_virtual_console() {
        let (a, b) = UnixStream::pair().unwrap();
        assert!(probe_console(a.as_raw_fd(), b.as_raw_fd()).is_none());
        let (mut master, mut slave) = (-1, -1);
        // SAFETY: openpty initializes two fresh owned descriptors; optional outputs are null.
        assert_eq!(
            unsafe {
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    std::ptr::null(),
                )
            },
            0
        );
        // SAFETY: successful openpty transfers both new descriptors to this test.
        let (_master, slave) =
            unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) };
        assert!(probe_console(slave.as_raw_fd(), slave.as_raw_fd()).is_none());
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn native_nonblocking_socket_connects_to_private_daemon() {
        let (_t, listener, mut c, now) = fixture();
        c.service(true, now, (80, 25), connect_socket);
        let mut peer = accept(&listener);
        peer.write_all(&packet(GPM_DOWN, 4)).unwrap();
        c.service(true, now, (80, 25), connect_socket);
        assert!(c.pop().is_some());
        c.service(false, now, (80, 25), connect_socket);
        assert!(c.pop().is_none());
        assert_eq!(c.snapshot.reset, 1);
        assert_eq!(c.snapshot.pressed, 0);
    }
}
