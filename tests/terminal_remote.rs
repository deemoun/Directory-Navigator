//! Real remote-pane recovery through a private FTP server. No external services.
#![cfg(any(target_os = "macos", target_os = "linux"))]
#[path = "support/pty.rs"]
pub mod pty;
use pty::Session;
use std::{
    io::{self, BufRead, Write},
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};
struct Server {
    port: u16,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<io::Result<()>>>,
}
fn command(input: &mut impl BufRead, pending: &mut String) -> io::Result<Option<String>> {
    match input.read_line(pending) {
        Ok(0) if pending.is_empty() => Ok(None),
        Ok(_) if pending.len() > 4096 || !pending.ends_with('\n') => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Incomplete or oversized private FTP command",
        )),
        Ok(_) => Ok(Some(std::mem::take(pending))),
        Err(e) => Err(e),
    }
}
fn accept(listener: &TcpListener, stop: &AtomicBool) -> io::Result<TcpStream> {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        match listener.accept() {
            Ok((s, _)) => return Ok(s),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                if stop.load(Ordering::Relaxed) || Instant::now() > end {
                    return Err(io::Error::new(
                        io::ErrorKind::Interrupted,
                        "private socket stopped",
                    ));
                }
                thread::sleep(Duration::from_millis(2));
            }
            Err(e) => return Err(e),
        }
    }
}
fn serve(mut control: TcpStream, first: bool, stop: Arc<AtomicBool>) -> io::Result<()> {
    control.set_nonblocking(false)?;
    control.set_read_timeout(Some(Duration::from_millis(100)))?;
    control.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut input = io::BufReader::new(control.try_clone()?);
    control.write_all(b"220 private DN recovery fixture\r\n")?;
    let mut data = None;
    let mut listings = 0;
    let mut pending = String::new();
    while !stop.load(Ordering::Relaxed) {
        let line = match command(&mut input, &mut pending) {
            Ok(None) => break,
            Ok(Some(line)) => line,
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                continue
            }
            Err(e) => return Err(e),
        };
        let line = line.trim_end();
        if line.starts_with("USER ") {
            control.write_all(b"331 Password required\r\n")?;
        } else if line.starts_with("PASS ") {
            control.write_all(b"230 Logged in\r\n")?;
        } else if line == "TYPE I" {
            control.write_all(b"200 Binary\r\n")?;
        } else if matches!(line, "PASV" | "EPSV") {
            let socket = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
            socket.set_nonblocking(true)?;
            let p = socket.local_addr()?.port();
            if line == "PASV" {
                write!(
                    control,
                    "227 Entering Passive Mode (127,0,0,1,{},{})\r\n",
                    p / 256,
                    p % 256
                )?;
            } else {
                write!(control, "229 Entering Extended Passive Mode (|||{p}|)\r\n")?;
            }
            data = Some(socket);
        } else if let Some(path) = line.strip_prefix("MLSD ") {
            listings += 1;
            if first && listings == 3 {
                control.write_all(b"421 Fixture connection closed\r\n")?;
                break;
            }
            control.write_all(b"150 Opening data\r\n")?;
            let listener = data
                .take()
                .ok_or_else(|| io::Error::other("Missing private passive socket"))?;
            let mut stream = accept(&listener, &stop)?;
            stream.set_nonblocking(false)?;
            if path == "/folder" {
                stream.write_all(b"type=file;size=4;modify=20260101000000; a.txt\r\ntype=file;size=4;modify=20260101000000; b.txt\r\n")?;
            } else {
                stream.write_all(b"type=dir;modify=20260101000000; folder\r\n")?;
            }
            drop(stream);
            control.write_all(b"226 Transfer complete\r\n")?;
        } else if line == "QUIT" {
            control.write_all(b"221 Goodbye\r\n")?;
            break;
        } else {
            return Err(io::Error::other(format!(
                "Unexpected private command: {}",
                line.split_whitespace().next().unwrap_or("")
            )));
        }
    }
    Ok(())
}
#[test]
fn private_ftp_commands_survive_fragmentation_and_socket_timeouts() -> io::Result<()> {
    struct Fragmented(u8);
    impl io::Read for Fragmented {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            self.0 += 1;
            let fragment: &[u8] = match self.0 {
                1 => b"USER te",
                2 => return Err(io::ErrorKind::TimedOut.into()),
                3 => b"ster\r",
                4 => return Err(io::ErrorKind::WouldBlock.into()),
                5 => b"\nTYPE I\r\n",
                _ => return Ok(0),
            };
            bytes[..fragment.len()].copy_from_slice(fragment);
            Ok(fragment.len())
        }
    }
    let mut reader = io::BufReader::new(Fragmented(0));
    let mut pending = String::new();
    assert_eq!(
        command(&mut reader, &mut pending).unwrap_err().kind(),
        io::ErrorKind::TimedOut
    );
    assert_eq!(pending, "USER te");
    assert_eq!(
        command(&mut reader, &mut pending).unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    assert_eq!(pending, "USER tester\r");
    assert_eq!(
        command(&mut reader, &mut pending)?.as_deref(),
        Some("USER tester\r\n")
    );
    assert!(pending.is_empty());
    assert_eq!(
        command(&mut reader, &mut pending)?.as_deref(),
        Some("TYPE I\r\n")
    );
    assert!(command(&mut reader, &mut pending)?.is_none());
    let mut truncated = io::Cursor::new(b"USER truncated");
    assert_eq!(
        command(&mut truncated, &mut pending).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
    pending.clear();
    let mut oversized = io::Cursor::new(format!("{}\n", "x".repeat(4096)));
    assert_eq!(
        command(&mut oversized, &mut pending).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
    Ok(())
}
impl Server {
    fn new() -> io::Result<Self> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let stop = Arc::new(AtomicBool::new(false));
        let token = stop.clone();
        let worker = thread::spawn(move || {
            let mut jobs = vec![];
            while !token.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((s, _)) => {
                        let first = jobs.is_empty();
                        let c = token.clone();
                        jobs.push(thread::spawn(move || serve(s, first, c)));
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2))
                    }
                    Err(e) => return Err(e),
                }
            }
            for j in jobs {
                j.join()
                    .map_err(|_| io::Error::other("Private FTP fixture panicked"))??;
            }
            Ok(())
        });
        Ok(Self {
            port,
            stop,
            worker: Some(worker),
        })
    }
    fn finish(mut self) -> io::Result<()> {
        self.stop.store(true, Ordering::Relaxed);
        self.worker
            .take()
            .unwrap()
            .join()
            .map_err(|_| io::Error::other("Private accept loop panicked"))?
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.worker.take() {
            let _ = t.join();
        }
    }
}
fn click(
    s: &mut Session,
    label: &str,
    stage: &str,
    ready: impl Fn(&Session) -> bool,
) -> io::Result<()> {
    let text = s.text();
    let (y, line) = text
        .lines()
        .enumerate()
        .filter(|(_, l)| l.contains(label))
        .last()
        .ok_or_else(|| io::Error::other(format!("Missing {label}: {text}")))?;
    let x = unicode_width::UnicodeWidthStr::width(&line[..line.find(label).unwrap()]) + 2;
    let diagnostics = s.root.join("settings/dn/diagnostics.txt");
    let input_count = || {
        std::fs::read_to_string(&diagnostics).ok().and_then(|text| {
            text.lines().find_map(|line| {
                line.strip_prefix("input_events=")
                    .and_then(|n| n.parse::<u64>().ok())
            })
        })
    };
    let before = input_count()
        .ok_or_else(|| io::Error::other("Missing input counter for private FTP click"))?;
    s.send(
        format!("\x1b[<0;{x};{}M\x1b[<0;{x};{}m", y + 1, y + 1).as_bytes(),
        stage,
        |s| ready(s) && input_count().is_some_and(|n| n >= before + 2),
    )
}
fn field(s: &mut Session, label: &str, value: &str, secret: bool) -> io::Result<()> {
    let text = s.text();
    let (y, line) = text
        .lines()
        .enumerate()
        .find(|(_, l)| l.contains(label))
        .ok_or_else(|| io::Error::other(format!("Missing field {label}")))?;
    let x = unicode_width::UnicodeWidthStr::width(&line[..line.find(label).unwrap()]) + 1;
    let bytes = format!(
        "\x1b[<0;{x};{}M\x1b[<0;{x};{}m\x1b[H{}",
        y + 2,
        y + 2,
        "\x1b[3~".repeat(64)
    );
    // A frame after the first Delete is not acknowledgement of the whole
    // batch: otherwise delayed Deletes can erase the subsequently pasted value.
    let diagnostics = s.root.join("settings/dn/diagnostics.txt");
    let input_count = || {
        std::fs::read_to_string(&diagnostics).ok().and_then(|text| {
            text.lines().find_map(|line| {
                line.strip_prefix("input_events=")
                    .and_then(|n| n.parse::<u64>().ok())
            })
        })
    };
    let before = input_count()
        .ok_or_else(|| io::Error::other("Missing input counter for private FTP field"))?;
    s.send(bytes.as_bytes(), "clear private connection field", |s| {
        s.text().contains(label) && input_count().is_some_and(|n| n >= before + 67)
    })?;
    let expected = if secret {
        "*".repeat(value.chars().count())
    } else {
        value.into()
    };
    s.paste(value, "fill private connection field", |s| {
        s.text().contains(&expected)
    })
}
#[test]
fn ftp_disconnect_reconnect_retains_directory_selection_and_restores_terminal() -> io::Result<()> {
    let server = Server::new()?;
    let mut s = Session::new("remote-reconnect")?;
    s.wait("manager ready", |s| {
        s.command_prompt_active() && s.text().contains("item-000.")
    })?;
    s.send(b"\x1bun", "remote connections", |s| {
        s.text().contains("Password / key passphrase")
    })?;
    click(&mut s, "SFTP", "FTPS choice", |s| s.text().contains("FTPS"))?;
    click(&mut s, "FTPS", "FTP choice", |s| {
        !s.text().contains("FTPS") && s.text().contains("FTP")
    })?;
    field(&mut s, "Host", "127.0.0.1", false)?;
    field(&mut s, "Port", &server.port.to_string(), false)?;
    field(&mut s, "Username", "tester", false)?;
    field(&mut s, "Password / key passphrase", "private-pass", true)?;
    click(&mut s, "Connect", "FTP confirmation", |s| {
        s.text().contains("FTP sends passwords")
    })?;
    click(&mut s, "Connect", "connected remote pane", |s| {
        s.command_prompt_active() && s.text().contains("ftp://") && s.text().contains("Connected")
    })?;
    s.select_file("folder")?;
    s.send(b"\r", "remote subdirectory", |s| {
        s.text().contains("/folder") && s.text().contains("a.txt")
    })?;
    s.select_file("a.txt")?;
    s.send(b"\x1b[2~", "mark remote file", |s| {
        s.text().contains("selected file")
    })?;
    s.select_file("a.txt")?;
    s.send(b"\x12", "disconnected remote error", |s| {
        s.text().contains("Cannot load the remote directory")
    })?;
    s.send(b"\x1b", "cached disconnected pane", |s| {
        s.command_prompt_active() && s.text().contains("Disconnected")
    })?;
    assert!(s.text().contains("selected file"));
    assert!(s.text().contains("/folder"));
    s.send(b"\x1bp", "Panel menu", |s| {
        s.text().contains("Reconnect remote panel")
    })?;
    click(
        &mut s,
        "Reconnect remote panel",
        "owned reconnect dialog",
        |s| {
            s.text().contains("Reconnect retains") && s.text().contains("Password / key passphrase")
        },
    )?;
    assert!(s.text().contains("/folder"));
    assert!(!s.text().contains("************"));
    field(&mut s, "Password / key passphrase", "private-pass", true)?;
    click(&mut s, "Reconnect", "FTP reconnect confirmation", |s| {
        s.text().contains("FTP sends passwords")
    })?;
    click(&mut s, "Reconnect", "reconnected same pane", |s| {
        s.command_prompt_active()
            && s.text().contains("Connected")
            && !s.text().contains("Disconnected")
            && s.text().contains("/folder")
    })?;
    assert!(s.text().contains("selected file"));
    s.send(b"\x1b[5;5~", "explicit disconnect", |s| {
        s.command_prompt_active() && !s.text().contains("ftp://")
    })?;
    s.finish()?;
    server.finish()
}
