use super::{
    lan::Lan,
    manifest::{Manifest, MANIFEST_LIMIT},
};
use iroh::endpoint::{Connection, RecvStream, SendStream};
use serde::{Deserialize, Serialize};
use std::{
    io,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
pub const ALPN: &[u8] = b"dn-transfer/1";
#[derive(Serialize, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub enum Message {
    Hello {
        version: u32,
        sender: bool,
        name: String,
        trusted: bool,
    },
    Busy,
    Confirmed,
    Offer(Manifest),
    Accepted,
    Ready,
    Next {
        index: usize,
        send: bool,
    },
    Digest([u8; 32]),
    Receipt {
        files: u64,
        skipped: u64,
        warnings: Vec<String>,
    },
    Acknowledged,
}
pub fn error(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}
pub async fn bounded<T>(
    future: impl std::future::Future<Output = io::Result<T>>,
    cancel: &AtomicBool,
    timeout: Duration,
) -> io::Result<T> {
    if cancel.load(Ordering::Relaxed) {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Peer transfer cancelled",
        ));
    }
    tokio::pin!(future);
    let deadline = tokio::time::sleep(timeout);
    tokio::pin!(deadline);
    let mut tick = tokio::time::interval(Duration::from_millis(20));
    loop {
        tokio::select! {
            result = &mut future => return if cancel.load(Ordering::Relaxed) { Err(io::Error::new(io::ErrorKind::Interrupted,"Peer transfer cancelled")) } else { result },
            _ = &mut deadline => return Err(io::Error::new(io::ErrorKind::TimedOut, "Peer did not respond in time")),
            _ = tick.tick() => if cancel.load(Ordering::Relaxed) { return Err(io::Error::new(io::ErrorKind::Interrupted, "Peer transfer cancelled")); },
        }
    }
}
pub async fn send(stream: &mut SendStream, message: &Message) -> io::Result<()> {
    let bytes = serde_json::to_vec(message).map_err(error)?;
    if bytes.len() > MANIFEST_LIMIT {
        return Err(error("Peer message exceeds 16 MiB"));
    }
    stream
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .await
        .map_err(error)?;
    stream.write_all(&bytes).await.map_err(error)
}
pub async fn receive(stream: &mut RecvStream) -> io::Result<Message> {
    let mut size = [0; 4];
    stream.read_exact(&mut size).await.map_err(error)?;
    let size = u32::from_be_bytes(size) as usize;
    if size == 0 || size > MANIFEST_LIMIT {
        return Err(error("Invalid peer message length"));
    }
    let mut bytes = vec![0; size];
    stream.read_exact(&mut bytes).await.map_err(error)?;
    serde_json::from_slice(&bytes).map_err(error)
}
pub fn code(
    connection: &Connection,
    sender: iroh::EndpointId,
    receiver: iroh::EndpointId,
) -> io::Result<String> {
    let context = format!("dn-transfer/1:{sender}:{receiver}");
    let mut bytes = [0; 8];
    connection
        .export_keying_material(&mut bytes, b"EXPORTER-DN-Pairing-v1", context.as_bytes())
        .map_err(|_| error("Could not derive the connection verification code"))?;
    let value = u64::from_be_bytes(bytes) % 100_000_000;
    Ok(format!("{:04} {:04}", value / 10000, value % 10000))
}
pub fn check_path(lan: &Lan, connection: &Connection) -> io::Result<()> {
    lan.connection(connection)
}

/// Queueing and conflict decisions can wait for a human without blocking the UI.
/// QUIC's finite idle timeout detects a lost peer; cancellation is polled here.
pub async fn waiting<T>(
    future: impl std::future::Future<Output = io::Result<T>>,
    cancel: &AtomicBool,
) -> io::Result<T> {
    tokio::pin!(future);
    let mut tick = tokio::time::interval(Duration::from_millis(20));
    loop {
        tokio::select! {
            result=&mut future => return if cancel.load(Ordering::Relaxed) { Err(io::Error::new(io::ErrorKind::Interrupted,"Peer transfer cancelled")) } else {result},
            _=tick.tick() => if cancel.load(Ordering::Relaxed) { return Err(io::Error::new(io::ErrorKind::Interrupted,"Peer transfer cancelled")); },
        }
    }
}
