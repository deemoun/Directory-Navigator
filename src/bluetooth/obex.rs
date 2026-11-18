//! Bounded OBEX 1.0 Object Push framing, independent from the Bluetooth adapter.
use std::{
    io::{self, Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
// Negotiated DN extension: marked Continue responses keep human-decision
// waits alive without granting permission to send file contents.
pub const WAIT_HEADER: &[u8] = b"\x70\x00\x0aDNwait1";
pub const DECISION_WAIT: std::time::Duration = std::time::Duration::from_secs(600);
pub const MTU: usize = 16 * 1024;
pub const MAX_FILE: u64 = u32::MAX as u64;
pub trait Link: Read + Write {}
impl<T: Read + Write> Link for T {}
pub type Cancel = Arc<AtomicBool>;
pub fn check(cancel: &AtomicBool) -> io::Result<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Bluetooth transfer stopped",
        ))
    } else {
        Ok(())
    }
}
fn read_exact_while(
    link: &mut dyn Link,
    mut bytes: &mut [u8],
    cancel: &AtomicBool,
    check_state: &mut impl FnMut() -> io::Result<()>,
) -> io::Result<()> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    while !bytes.is_empty() {
        check(cancel)?;
        check_state()?;
        match link.read(bytes) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "Bluetooth connection closed",
                ))
            }
            Ok(n) => {
                let (_, rest) = bytes.split_at_mut(n);
                bytes = rest;
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) && std::time::Instant::now() < deadline => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
pub fn read(link: &mut dyn Link, cancel: &AtomicBool, limit: usize) -> io::Result<(u8, Vec<u8>)> {
    read_while(link, cancel, limit, || Ok(()))
}
pub(super) fn read_while(
    link: &mut dyn Link,
    cancel: &AtomicBool,
    limit: usize,
    mut check_state: impl FnMut() -> io::Result<()>,
) -> io::Result<(u8, Vec<u8>)> {
    let mut h = [0; 3];
    read_exact_while(link, &mut h, cancel, &mut check_state)?;
    let n = usize::from(u16::from_be_bytes([h[1], h[2]]));
    if !(3..=limit).contains(&n) {
        return Err(invalid("Invalid OBEX packet length"));
    }
    let mut body = vec![0; n - 3];
    read_exact_while(link, &mut body, cancel, &mut check_state)?;
    Ok((h[0], body))
}
pub fn write(link: &mut dyn Link, opcode: u8, body: &[u8], cancel: &AtomicBool) -> io::Result<()> {
    check(cancel)?;
    let n = u16::try_from(body.len() + 3)
        .map_err(|_| invalid("OBEX packet exceeds negotiated size"))?;
    let mut packet = Vec::with_capacity(usize::from(n));
    packet.push(opcode);
    packet.extend(n.to_be_bytes());
    packet.extend(body);
    let mut remaining = packet.as_slice();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    while !remaining.is_empty() {
        check(cancel)?;
        match link.write(remaining) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "Bluetooth connection closed while sending",
                ))
            }
            Ok(n) => remaining = &remaining[n..],
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) && std::time::Instant::now() < deadline => {}
            Err(e) => return Err(e),
        }
    }
    link.flush()
}
pub fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
pub fn response(link: &mut dyn Link, cancel: &AtomicBool, expected: u8) -> io::Result<Vec<u8>> {
    let (code, body) = read(link, cancel, MTU)?;
    if code != expected {
        return Err(io::Error::other(format!(
            "Bluetooth receiver refused the request (OBEX {code:02X})"
        )));
    }
    Ok(body)
}
pub fn name_header(name: &str) -> io::Result<Vec<u8>> {
    filename(name)?;
    let words = name.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let n = u16::try_from(words.len() * 2 + 3)
        .map_err(|_| invalid("Bluetooth filename is too long"))?;
    let mut out = vec![1];
    out.extend(n.to_be_bytes());
    for w in words {
        out.extend(w.to_be_bytes());
    }
    Ok(out)
}
pub fn filename(name: &str) -> io::Result<std::path::PathBuf> {
    if name.len() > 1024 || name.chars().any(|c| c.is_control()) {
        return Err(invalid("Invalid Bluetooth filename"));
    }
    crate::archive::extract::component(name.as_bytes()).map(Into::into)
}
#[derive(Default, Debug)]
pub struct Headers {
    pub name: Option<String>,
    pub size: Option<u64>,
    pub body: Vec<u8>,
    pub end: bool,
    pub connection: Option<u32>,
    pub waiting: bool,
}
pub fn headers(bytes: &[u8]) -> io::Result<Headers> {
    let mut out = Headers::default();
    let mut cursor = bytes;
    while let Some((&id, rest)) = cursor.split_first() {
        let (payload, next) = match id >> 6 {
            0 | 1 => {
                if rest.len() < 2 {
                    return Err(invalid("Truncated OBEX header"));
                }
                let n = usize::from(u16::from_be_bytes([rest[0], rest[1]]));
                if n < 3 || n > cursor.len() {
                    return Err(invalid("Invalid OBEX header size"));
                }
                (&cursor[3..n], &cursor[n..])
            }
            2 => {
                if rest.is_empty() {
                    return Err(invalid("Truncated OBEX byte header"));
                }
                (&rest[..1], &rest[1..])
            }
            _ => {
                if rest.len() < 4 {
                    return Err(invalid("Truncated OBEX integer header"));
                }
                (&rest[..4], &rest[4..])
            }
        };
        match id {
            1 => {
                if out.name.is_some()
                    || payload.len() < 2
                    || !payload.len().is_multiple_of(2)
                    || !payload.ends_with(&[0, 0])
                {
                    return Err(invalid("Invalid OBEX Unicode name"));
                }
                let words = payload[..payload.len() - 2]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| u16::from_be_bytes([b[0], b[1]]))
                    .collect::<Vec<_>>();
                let name =
                    String::from_utf16(&words).map_err(|_| invalid("Invalid OBEX UTF-16 name"))?;
                filename(&name)?;
                out.name = Some(name);
            }
            0xc3 => {
                if out.size.is_some() {
                    return Err(invalid("Repeated OBEX length"));
                }
                out.size = Some(u64::from(u32::from_be_bytes(payload.try_into().unwrap())));
            }
            0x48 | 0x49 => {
                if out.end {
                    return Err(invalid("OBEX data follows end of body"));
                }
                out.body.extend(payload);
                out.end = id == 0x49;
            }
            0x70 if payload == b"DNwait1" => out.waiting = true,
            0xcb => out.connection = Some(u32::from_be_bytes(payload.try_into().unwrap())),
            _ => (),
        }
        cursor = next;
    }
    Ok(out)
}
