//! Accepted sessions outlive the plugin window and are owned by Transfer Center.
use super::{
    lan::Lan,
    manifest::{Manifest, Selection},
    service::Reservation,
    wire::{self, Message},
};
use crate::operations::{
    conflict::{Conflict, Decision},
    CopyOptions, Phase, Progress,
};
use iroh::endpoint::{Connection, RecvStream, SendStream};
use std::{
    io::{self, Read, Write},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
pub struct Session {
    pub manifest: Manifest,
    pub peer: String,
    selection: Option<Selection>,
    connection: Connection,
    streams: Mutex<Option<(SendStream, RecvStream)>>,
    runtime: tokio::runtime::Handle,
    reservation: Mutex<Option<Reservation>>,
    lan: Lan,
}
impl Session {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        connection: Connection,
        send: SendStream,
        recv: RecvStream,
        manifest: Manifest,
        selection: Option<Selection>,
        reservation: Reservation,
        lan: Lan,
        peer: String,
    ) -> Self {
        Self {
            connection,
            streams: Mutex::new(Some((send, recv))),
            manifest,
            selection,
            reservation: Mutex::new(Some(reservation)),
            runtime: tokio::runtime::Handle::current(),
            lan,
            peer,
        }
    }
    pub fn sending(&self) -> bool {
        self.selection.is_some()
    }
    pub fn abort(&self) {
        self.connection.close(1u32.into(), b"Transfer cancelled");
    }
    pub fn ended(&self) -> bool {
        self.connection.close_reason().is_some()
    }
    pub fn run(
        &self,
        destination: Option<PathBuf>,
        options: CopyOptions,
        cancel: &AtomicBool,
        mut notify: impl FnMut(&Progress),
        ask: &mut impl FnMut(&Conflict) -> io::Result<Decision>,
    ) -> io::Result<Progress> {
        let (mut send, mut recv) = self
            .streams
            .lock()
            .map_err(wire::error)?
            .take()
            .ok_or_else(|| wire::error("Peer transfer was already consumed"))?;
        let mut progress = Progress {
            phase: Phase::WaitingPeer,
            total_bytes: Some(self.manifest.total),
            ..Default::default()
        };
        notify(&progress);
        let result = self.runtime.block_on(async {
            if let Some(selection) = &self.selection {
                if !matches!(
                    wire::waiting(wire::receive(&mut recv), cancel).await?,
                    Message::Accepted
                ) {
                    return Err(wire::error("Receiver did not accept this offer"));
                }
                wire::bounded(
                    wire::send(&mut send, &Message::Ready),
                    cancel,
                    Duration::from_secs(30),
                )
                .await?;
                progress.phase = Phase::Copying;
                let mut bytes = [0; 128 * 1024];
                for (index, entry) in self.manifest.entries.iter().enumerate() {
                    wire::check_path(&self.lan, &self.connection)?;
                    let should_send = match wire::waiting(wire::receive(&mut recv), cancel).await? {
                        Message::Next {
                            index: expected,
                            send,
                        } if expected == index => send,
                        _ => return Err(wire::error("Peer file request is out of order")),
                    };
                    if !should_send {
                        continue;
                    }
                    if entry.directory {
                        return Err(wire::error("Peer requested a directory payload"));
                    }
                    let mut source = selection.open(index)?;
                    let mut hash = blake3::Hasher::new();
                    let mut left = entry.size;
                    while left > 0 {
                        if cancel.load(Ordering::Relaxed) {
                            return Err(io::Error::new(
                                io::ErrorKind::Interrupted,
                                "Peer transfer cancelled",
                            ));
                        }
                        wire::check_path(&self.lan, &self.connection)?;
                        let n = source.read(&mut bytes[..left.min(128 * 1024) as usize])?;
                        if n == 0 {
                            return Err(wire::error("Peer source was truncated"));
                        }
                        hash.update(&bytes[..n]);
                        wire::bounded(
                            async { send.write_all(&bytes[..n]).await.map_err(wire::error) },
                            cancel,
                            Duration::from_secs(30),
                        )
                        .await?;
                        left -= n as u64;
                        progress.bytes += n as u64;
                        progress.written += n as u64;
                        progress.member = Some(entry.path.clone());
                        notify(&progress);
                    }
                    selection.verify(index)?;
                    wire::bounded(
                        wire::send(&mut send, &Message::Digest(*hash.finalize().as_bytes())),
                        cancel,
                        Duration::from_secs(30),
                    )
                    .await?;
                }
                let receipt = wire::waiting(wire::receive(&mut recv), cancel)
                    .await
                    .map_err(|e| {
                        io::Error::new(
                            e.kind(),
                            format!(
                                "Delivery not confirmed; the receiver may already have files: {e}"
                            ),
                        )
                    })?;
                match receipt {
                    Message::Receipt {
                        files,
                        skipped,
                        warnings,
                    } if files
                        .checked_add(skipped)
                        .is_some_and(|n| n == self.manifest.entries.len() as u64)
                        && warnings.len() <= 100 =>
                    {
                        progress.files = files;
                        progress.skipped = skipped;
                        for warning in warnings {
                            progress.detail(
                                PathBuf::new(),
                                crate::filesystem::display_name(&warning)
                                    .chars()
                                    .take(4096)
                                    .collect(),
                            );
                        }
                    }
                    _ => return Err(wire::error("Invalid peer delivery receipt")),
                }
                if let Err(error) = wire::bounded(
                    wire::send(&mut send, &Message::Acknowledged),
                    cancel,
                    Duration::from_secs(10),
                )
                .await
                {
                    progress.detail(
                        PathBuf::new(),
                        format!(
                            "Delivery confirmed; final acknowledgement could not be sent: {error}"
                        ),
                    );
                }
                Ok(progress.clone())
            } else {
                let root =
                    destination.ok_or_else(|| wire::error("Choose a local receive directory"))?;
                self.manifest.validate()?;
                if options.check_free_space
                    && u128::from(self.manifest.total) > crate::volume_space::read(&root)?.available
                {
                    return Err(wire::error("Not enough free space for the peer offer"));
                }
                let mut sink = crate::operations::incoming::Sink::new(root, options.existing, ask)?;
                wire::bounded(
                    wire::send(&mut send, &Message::Accepted),
                    cancel,
                    Duration::from_secs(30),
                )
                .await?;
                if !matches!(
                    wire::waiting(wire::receive(&mut recv), cancel).await?,
                    Message::Ready
                ) {
                    return Err(wire::error("Sender did not become ready"));
                }
                progress.phase = Phase::Copying;
                let mut bytes = [0; 128 * 1024];
                for (index, entry) in self.manifest.entries.iter().enumerate() {
                    wire::check_path(&self.lan, &self.connection)?;
                    if entry.directory {
                        if sink.directory(entry)? {
                            progress.files += 1;
                        } else {
                            progress.skipped += 1;
                        }
                        wire::bounded(
                            wire::send(&mut send, &Message::Next { index, send: false }),
                            cancel,
                            Duration::from_secs(30),
                        )
                        .await?;
                        notify(&progress);
                        continue;
                    }
                    let Some(mut output) = sink.begin(entry)? else {
                        progress.skipped += 1;
                        wire::bounded(
                            wire::send(&mut send, &Message::Next { index, send: false }),
                            cancel,
                            Duration::from_secs(30),
                        )
                        .await?;
                        notify(&progress);
                        continue;
                    };
                    wire::bounded(
                        wire::send(&mut send, &Message::Next { index, send: true }),
                        cancel,
                        Duration::from_secs(30),
                    )
                    .await?;
                    let mut hash = blake3::Hasher::new();
                    let mut left = entry.size;
                    while left > 0 {
                        wire::check_path(&self.lan, &self.connection)?;
                        let n = left.min(128 * 1024) as usize;
                        wire::bounded(
                            async { recv.read_exact(&mut bytes[..n]).await.map_err(wire::error) },
                            cancel,
                            Duration::from_secs(30),
                        )
                        .await?;
                        output.stage.file.write_all(&bytes[..n])?;
                        hash.update(&bytes[..n]);
                        left -= n as u64;
                        progress.bytes += n as u64;
                        progress.written += n as u64;
                        progress.member = Some(entry.path.clone());
                        notify(&progress);
                    }
                    let digest = match wire::bounded(
                        wire::receive(&mut recv),
                        cancel,
                        Duration::from_secs(30),
                    )
                    .await?
                    {
                        Message::Digest(d) => d,
                        _ => return Err(wire::error("Missing peer checksum")),
                    };
                    if hash.finalize().as_bytes() != &digest {
                        return Err(wire::error(
                            "Peer checksum failed; incomplete file was not published",
                        ));
                    }
                    sink.finish(output, digest, options, cancel, &mut progress)?;
                    notify(&progress);
                }
                sink.complete_directories(&self.manifest.entries, cancel)?;
                let warnings = progress
                    .details
                    .iter()
                    .map(|d| d.message.chars().take(4096).collect())
                    .collect();
                wire::bounded(
                    wire::send(
                        &mut send,
                        &Message::Receipt {
                            files: progress.files,
                            skipped: progress.skipped,
                            warnings,
                        },
                    ),
                    cancel,
                    Duration::from_secs(30),
                )
                .await?;
                // Publication succeeded even when the sender's final acknowledgement is lost.
                if !matches!(
                    wire::bounded(wire::receive(&mut recv), cancel, Duration::from_secs(10)).await,
                    Ok(Message::Acknowledged)
                ) {
                    progress.detail(
                        PathBuf::new(),
                        "Files received; sender acknowledgement was not confirmed.".into(),
                    );
                }
                Ok(progress.clone())
            }
        });
        self.abort();
        self.reservation.lock().map_err(wire::error)?.take();
        result
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.abort();
    }
}
#[derive(Clone)]
pub struct Transfer {
    pub session: Arc<Session>,
    pub destination: Option<PathBuf>,
    pub receipt: Option<crate::archive::extract::Destination>,
}
impl Transfer {
    pub fn summary(&self) -> String {
        format!(
            "{} · {} · {} items",
            if self.session.sending() {
                "Send to nearby DN"
            } else {
                "Receive from nearby DN"
            },
            self.session.peer,
            self.session.manifest.entries.len()
        )
    }
    pub fn run(
        &self,
        options: CopyOptions,
        cancel: &AtomicBool,
        notify: impl FnMut(&Progress),
        ask: &mut impl FnMut(&Conflict) -> io::Result<Decision>,
    ) -> io::Result<Progress> {
        self.session
            .run(self.destination.clone(), options, cancel, notify, ask)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> io::Result<Self> {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let p = std::env::temp_dir().join(format!(
                "dn-peer-protocol-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&p)?;
            Ok(Self(p))
        }
        fn selection(&self) -> io::Result<Selection> {
            let source = self.0.join("payload.txt");
            fs::write(&source, b"data")?;
            Selection::capture(vec![source])
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[tokio::test]
    async fn corrupt_stream_never_replaces_an_approved_destination() -> io::Result<()> {
        let source = Temp::new()?;
        let destination = Temp::new()?;
        fs::write(destination.0.join("payload.txt"), b"original")?;
        let (a, b, sender, receiver) =
            super::super::service::tests::sessions(source.selection()?).await?;
        let root = destination.0.clone();
        let receiving = tokio::task::spawn_blocking(move || {
            receiver.run(
                Some(root),
                CopyOptions::default(),
                &AtomicBool::new(false),
                |_| {},
                &mut |_| Ok(Decision::Replace),
            )
        });
        let (mut send, mut recv) = sender.streams.lock().map_err(wire::error)?.take().unwrap();
        assert!(matches!(wire::receive(&mut recv).await?, Message::Accepted));
        wire::send(&mut send, &Message::Ready).await?;
        assert!(matches!(
            wire::receive(&mut recv).await?,
            Message::Next {
                index: 0,
                send: true
            }
        ));
        send.write_all(b"data").await.map_err(wire::error)?;
        wire::send(&mut send, &Message::Digest([0; 32])).await?;
        let result = tokio::time::timeout(Duration::from_secs(10), receiving)
            .await
            .map_err(wire::error)?
            .map_err(wire::error)?;
        assert!(result.unwrap_err().to_string().contains("checksum failed"));
        assert_eq!(fs::read(destination.0.join("payload.txt"))?, b"original");
        assert_eq!(fs::read_dir(&destination.0)?.count(), 1);
        a.close().await;
        b.close().await;
        Ok(())
    }
    #[tokio::test]
    async fn missing_final_receipt_reports_unconfirmed_delivery() -> io::Result<()> {
        let source = Temp::new()?;
        let destination = Temp::new()?;
        let (a, b, sender, receiver) =
            super::super::service::tests::sessions(source.selection()?).await?;
        let sending = tokio::task::spawn_blocking(move || {
            sender.run(
                None,
                CopyOptions::default(),
                &AtomicBool::new(false),
                |_| {},
                &mut |_| Ok(Decision::Skip),
            )
        });
        let (mut send, mut recv) = receiver
            .streams
            .lock()
            .map_err(wire::error)?
            .take()
            .unwrap();
        wire::send(&mut send, &Message::Accepted).await?;
        assert!(matches!(wire::receive(&mut recv).await?, Message::Ready));
        wire::send(
            &mut send,
            &Message::Next {
                index: 0,
                send: true,
            },
        )
        .await?;
        let mut bytes = [0; 4];
        recv.read_exact(&mut bytes).await.map_err(wire::error)?;
        assert!(matches!(
            wire::receive(&mut recv).await?,
            Message::Digest(_)
        ));
        // Model a peer that published successfully but disconnected before its receipt.
        fs::write(destination.0.join("payload.txt"), bytes)?;
        receiver.abort();
        let result = tokio::time::timeout(Duration::from_secs(10), sending)
            .await
            .map_err(wire::error)?
            .map_err(wire::error)?;
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Delivery not confirmed"));
        assert_eq!(fs::read(destination.0.join("payload.txt"))?, b"data");
        a.close().await;
        b.close().await;
        Ok(())
    }
}
