use super::*;
use super::{
    audio::{Audio, Meters},
    protocol::{Radio, Secure},
};
use std::{
    fs,
    io::{self, Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::{Duration, Instant},
};
#[derive(Clone)]
pub(super) struct Payload {
    pub name: Option<String>,
    pub bytes: Vec<u8>,
    pub modified: Option<i64>,
    source: Option<(PathBuf, crate::platform_fs::EntryIdentity, fs::Metadata)>,
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Offer {
    name: Option<String>,
    size: usize,
    hash: [u8; 32],
    modified: Option<i64>,
}
impl Offer {
    fn validate(&self) -> io::Result<()> {
        if self.size > 1024 {
            return Err(io::Error::other("Acoustic offer exceeds 1 KiB"));
        }
        if let Some(name) = &self.name {
            if name.len() > 255
                || name.contains('/')
                || name.contains('\\')
                || crate::peer::manifest::relative(name).is_err()
            {
                return Err(io::Error::other("Acoustic filename is not portable"));
            }
        }
        Ok(())
    }
}
enum Answer {
    Confirm(bool),
    Approve(bool, PathBuf),
}
enum Update {
    Notice(String),
    Devices(String),
    Pair(String),
    Offer(Offer),
    Text(String),
    Done(Result<String, String>),
}
pub(super) struct Service {
    binding: Binding,
    cancel: Arc<AtomicBool>,
    answers: mpsc::SyncSender<Answer>,
    events: mpsc::Receiver<Update>,
    meters: Arc<Meters>,
    conflicts: crate::operations::conflict::Mailbox,
}
impl Drop for Service {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.conflicts
            .answer(crate::operations::conflict::Decision::Cancel);
    }
}
fn send(tx: &mpsc::SyncSender<Update>, event: Update) -> io::Result<()> {
    tx.try_send(event)
        .map_err(|_| io::Error::other("Acoustic window closed or event queue full"))
}
fn wait<R: Radio>(secure: &mut Secure<R>, rx: &mpsc::Receiver<Answer>) -> io::Result<Answer> {
    let until = Instant::now() + Duration::from_secs(90);
    loop {
        match rx.try_recv() {
            Ok(a) => return Ok(a),
            Err(mpsc::TryRecvError::Disconnected) => {
                return Err(io::Error::other("Acoustic window closed"))
            }
            Err(_) => {}
        }
        if Instant::now() >= until {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Acoustic confirmation expired",
            ));
        }
        secure.link.pump(Duration::from_millis(20))?;
    }
}
fn run<R: Radio>(
    radio: R,
    payload: Option<Payload>,
    rx: &mpsc::Receiver<Answer>,
    tx: &mpsc::SyncSender<Update>,
    cancel: &AtomicBool,
    conflicts: &crate::operations::conflict::Mailbox,
) -> io::Result<String> {
    if payload.as_ref().is_some_and(|p| p.bytes.len() > 1024) {
        return Err(io::Error::other("Acoustic payload exceeds 1 KiB"));
    }
    let sending = payload.is_some();
    let mut secure = Secure::handshake(radio, sending)?;
    send(tx, Update::Pair(secure.code.clone()))?;
    if !matches!(wait(&mut secure, rx)?, Answer::Confirm(true)) {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Pairing cancelled; no offer sent",
        ));
    }
    if sending {
        secure.send(b"confirm-v1")?;
        if secure.receive()? != b"confirm-v1" {
            return Err(io::Error::other("Acoustic pairing rejected"));
        }
    } else {
        if secure.receive()? != b"confirm-v1" {
            return Err(io::Error::other("Acoustic pairing rejected"));
        }
        secure.send(b"confirm-v1")?;
    }
    if let Some(payload) = payload {
        if let Some((path, identity, metadata)) = &payload.source {
            let current = fs::symlink_metadata(path)?;
            if crate::platform_fs::entry_identity(path)? != *identity
                || current.len() != metadata.len()
                || current.modified().ok() != metadata.modified().ok()
            {
                return Err(io::Error::other("Source changed; use current file again"));
            }
        }
        let offer = Offer {
            name: payload.name,
            size: payload.bytes.len(),
            hash: *blake3::hash(&payload.bytes).as_bytes(),
            modified: payload.modified,
        };
        offer.validate()?;
        let offer = serde_json::to_vec(&offer).map_err(io::Error::other)?;
        secure.send(&offer)?;
        send(tx, Update::Notice("Waiting for receiver approval…".into()))?;
        if secure.receive()? != b"accept-v1" {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Receiver declined; nothing sent",
            ));
        }
        send(
            tx,
            Update::Notice("Sending encrypted payload; tiny transfers may take minutes…".into()),
        )?;
        let mut data = vec![1];
        data.extend(payload.bytes);
        secure
            .send(&data)
            .map_err(|e| io::Error::other(format!("Delivery unconfirmed: {e}")))?;
        let receipt = secure
            .receive()
            .map_err(|e| io::Error::other(format!("Delivery unconfirmed: {e}")))?;
        if receipt != b"retained-v1" && receipt != b"published-v1" && receipt != b"skipped-v1" {
            return Err(io::Error::other(
                "No valid final receipt; delivery is unconfirmed",
            ));
        }
        let acknowledged = secure.send(b"receipt-ack-v1").is_ok();
        if !acknowledged {
            return Ok(if receipt == b"skipped-v1" {
                "Receiver skipped the file; receipt acknowledgement unconfirmed."
            } else {
                "Delivery confirmed by receiver; receipt acknowledgement unconfirmed."
            }
            .into());
        }
        Ok(if receipt == b"skipped-v1" {
            "Receiver skipped the file; nothing published."
        } else {
            "Delivery confirmed by receiver."
        }
        .into())
    } else {
        let offer: Offer = serde_json::from_slice(&secure.receive()?).map_err(io::Error::other)?;
        offer.validate()?;
        send(tx, Update::Offer(offer.clone()))?;
        let destination = match wait(&mut secure, rx)? {
            Answer::Approve(true, path) => path,
            _ => {
                secure.send(b"decline-v1")?;
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "Offer declined; nothing retained",
                ));
            }
        };
        let guard = if offer.name.is_some() {
            let guard = crate::archive::extract::Destination::capture(&destination)?;
            if !fs::symlink_metadata(&destination)?.is_dir() {
                return Err(io::Error::other("Choose an existing local directory"));
            }
            Some(guard)
        } else {
            None
        };
        secure.send(b"accept-v1")?;
        let data = secure.receive_payload()?;
        if data.first() != Some(&1)
            || data.len() != offer.size + 1
            || blake3::hash(&data[1..]).as_bytes() != &offer.hash
        {
            return Err(io::Error::other(
                "Acoustic payload length or checksum failed; nothing retained",
            ));
        }
        let (receipt, outcome) = if let Some(name) = &offer.name {
            let entry = crate::peer::manifest::Entry {
                path: name.clone(),
                size: offer.size as u64,
                directory: false,
                modified: offer.modified,
            };
            guard
                .as_ref()
                .expect("file destination prepared")
                .validate()?;
            let until = Instant::now() + Duration::from_secs(90);
            let mut ask = |c: &crate::operations::conflict::Conflict| {
                conflicts.ask_pumping(c, cancel, || {
                    if Instant::now() >= until {
                        return Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "Replacement confirmation expired",
                        ));
                    }
                    secure.link.pump(Duration::ZERO)
                })
            };
            let mut sink = crate::operations::incoming::Sink::new(
                destination,
                crate::operations::ExistingDestination::Ask,
                &mut ask,
            )?;
            if let Some(mut file) = sink.begin(&entry)? {
                file.stage.file.write_all(&data[1..])?;
                let mut progress = crate::operations::Progress::default();
                sink.finish(
                    file,
                    offer.hash,
                    crate::operations::CopyOptions {
                        verify_writes: true,
                        ..Default::default()
                    },
                    cancel,
                    &mut progress,
                )?;
                (
                    b"published-v1".as_slice(),
                    if progress.details.is_empty() {
                        "File published. Delivery receipt pending."
                    } else {
                        "File published with a durability warning; review the destination. Delivery receipt pending."
                    },
                )
            } else {
                (b"skipped-v1".as_slice(), "File skipped; nothing published.")
            }
        } else {
            let text = String::from_utf8(data[1..].to_vec()).map_err(io::Error::other)?;
            send(tx, Update::Text(text))?;
            (
                b"retained-v1".as_slice(),
                "Text retained in this window. Delivery receipt pending.",
            )
        };
        send(tx, Update::Notice(outcome.into()))?;
        if let Err(e) = secure.send(receipt) {
            return Ok(format!("{outcome} Receipt unconfirmed: {e}"));
        }
        if !matches!(secure.receive(),Ok(ref b) if b==b"receipt-ack-v1") {
            return Ok(format!("{outcome} Receipt acknowledgement unconfirmed."));
        }
        Ok(outcome.replace(
            "Delivery receipt pending.",
            "Delivery receipt acknowledged.",
        ))
    }
}
fn capture(app: &App) -> io::Result<Payload> {
    let panel = &app.panels[app.active];
    if panel.is_virtual() || panel.loading {
        return Err(io::Error::other("Extract or download a regular file first"));
    }
    if panel.selected.len() > 1 {
        return Err(io::Error::other("Acoustic transfers accept only one file"));
    }
    let entry = panel
        .entries
        .get(panel.cursor)
        .filter(|e| !e.parent && !e.directory && !e.symlink)
        .ok_or_else(|| io::Error::other("Select one regular file, at most 1 KiB"))?;
    let path = entry.path.clone();
    let identity = crate::platform_fs::entry_identity(&path)?;
    let mut file = crate::filesystem::open_regular(&path)?;
    let metadata = file.metadata()?;
    if metadata.len() > 1024 {
        return Err(io::Error::other("Acoustic file limit is 1 KiB"));
    }
    let mut bytes = Vec::new();
    (&mut file).take(1025).read_to_end(&mut bytes)?;
    let after = file.metadata()?;
    if bytes.len() > 1024
        || !crate::platform_fs::same_identity(&metadata, &fs::symlink_metadata(&path)?)
        || identity != crate::platform_fs::entry_identity(&path)?
        || metadata.len() != after.len()
        || metadata.modified().ok() != after.modified().ok()
    {
        return Err(io::Error::other("Source changed during acoustic capture"));
    }
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| io::Error::other("Acoustic filename must be Unicode"))?
        .to_owned();
    let offer = Offer {
        name: Some(name.clone()),
        size: bytes.len(),
        hash: *blake3::hash(&bytes).as_bytes(),
        modified: metadata
            .modified()
            .ok()
            .and_then(crate::peer::manifest::seconds),
    };
    offer.validate()?;
    Ok(Payload {
        name: Some(name),
        bytes,
        modified: offer.modified,
        source: Some((path, identity, metadata)),
    })
}
pub(super) fn reset(app: &mut App) {
    app.acoustic.service = None;
    app.acoustic.source = None;
    resume(app);
}
pub(super) fn action(app: &mut App, b: Binding, action: Action) {
    match action {
        Action::Stop => {
            app.acoustic.view.generation = app.acoustic.view.generation.wrapping_add(1);
            app.acoustic.service = None;
            app.acoustic.view.busy = false;
            app.acoustic.view.code = None;
            app.acoustic.view.offer = None;
            app.acoustic.view.conflict = None;
            app.acoustic.view.input = 0;
            app.acoustic.view.output = 0;
            app.refresh();
            app.acoustic.view.notice =
                "Acoustic session stopped; completed publications remain.".into();
            resume(app);
        }
        Action::Capture if !app.acoustic.view.busy => {
            app.acoustic.source = None;
            app.acoustic.view.source.clear();
            match capture(app) {
                Ok(p) => {
                    app.acoustic.view.source = format!(
                        "{} ({} bytes)",
                        p.name.as_deref().unwrap_or(""),
                        p.bytes.len()
                    );
                    app.acoustic.source = Some(p);
                }
                Err(e) => app.acoustic.view.notice = e.to_string(),
            }
        }
        Action::SendText(ref text) if text.len() > 1024 || text.is_empty() => {
            app.acoustic.view.notice = "Enter 1–1024 UTF-8 bytes.".into()
        }
        Action::SendText(_) | Action::SendFile | Action::Receive if !app.acoustic.view.busy => {
            let payload = match action {
                Action::SendText(text) => Some(Payload {
                    name: None,
                    bytes: text.into_bytes(),
                    modified: None,
                    source: None,
                }),
                Action::SendFile => {
                    let Some(p) = app.acoustic.source.clone() else {
                        app.acoustic.view.notice = "Use current file first.".into();
                        return;
                    };
                    Some(p)
                }
                _ => None,
            };
            let cancel = Arc::new(AtomicBool::new(false));
            let meters = Arc::new(Meters::default());
            let (answer, rx) = mpsc::sync_channel(1);
            let (tx, events) = mpsc::sync_channel(8);
            let conflicts = crate::operations::conflict::Mailbox::default();
            let (c, m, conflict) = (cancel.clone(), meters.clone(), conflicts.clone());
            let worker = thread::Builder::new()
                .name("dn-acoustic".into())
                .spawn(move || {
                    static ACTIVE: AtomicBool = AtomicBool::new(false);
                    struct Active;
                    impl Drop for Active {
                        fn drop(&mut self) {
                            ACTIVE.store(false, Ordering::Release);
                        }
                    }
                    let result = if ACTIVE
                        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                        .is_err()
                    {
                        Err(io::Error::other(
                            "Previous acoustic devices are still closing; try again",
                        ))
                    } else {
                        let _active = Active;
                        if c.load(Ordering::Relaxed) {
                            Err(io::Error::new(
                                io::ErrorKind::Interrupted,
                                "Acoustic session cancelled",
                            ))
                        } else {
                            Audio::open(c.clone(), m, payload.is_some()).and_then(|radio| {
                                send(&tx, Update::Devices(radio.devices.clone()))?;
                                run(radio, payload, &rx, &tx, &c, &conflict)
                            })
                        }
                    };
                    let result = result.map_err(|e| e.to_string());
                    let _ = send(&tx, Update::Done(result));
                });
            match worker {
                Ok(_) => {
                    app.acoustic.view.generation = app.acoustic.view.generation.wrapping_add(1);
                    app.acoustic.music = music(app).and_then(|p| p.pause_for_acoustic());
                    app.acoustic.view.busy = true;
                    app.acoustic.view.notice =
                        "Opening default microphone and speakers; establishing acoustic pairing…"
                            .into();
                    app.acoustic.view.code = None;
                    app.acoustic.view.offer = None;
                    app.acoustic.view.received = None;
                    app.acoustic.view.cause = None;
                    app.acoustic.view.devices.clear();
                    app.acoustic.view.conflict = None;
                    app.acoustic.service = Some(Service {
                        binding: b,
                        cancel,
                        answers: answer,
                        events,
                        meters,
                        conflicts,
                    });
                }
                Err(e) => app.acoustic.view.notice = e.to_string(),
            }
        }
        Action::Confirm(value) => {
            if app.acoustic.view.code.is_some() {
                if let Some(s) = &app.acoustic.service {
                    let _ = s.answers.try_send(Answer::Confirm(value));
                }
                app.acoustic.view.code = None;
            }
        }
        Action::Approve {
            accept,
            destination,
        } => {
            if app.acoustic.view.offer.is_some() {
                if let Some(s) = &app.acoustic.service {
                    let _ = s.answers.try_send(Answer::Approve(accept, destination));
                }
                app.acoustic.view.offer = None;
            }
        }
        Action::Conflict(decision) => {
            if let Some(s) = &app.acoustic.service {
                s.conflicts.answer(decision);
                app.acoustic.view.conflict = None;
            }
        }
        Action::Dismiss => app.acoustic.view.received = None,
        Action::Tree => {
            if let Owner::Document(owner) = b.owner {
                match crate::directory_tree::Tree::new(
                    app.active,
                    app.panels[app.active].path.clone(),
                    app.config.show_hidden,
                ) {
                    Ok(mut tree) => {
                        tree.destination_acoustic = Some(owner);
                        app.modals.push_peer_tree(tree, owner);
                        app.sync_modals();
                    }
                    Err(e) => app.acoustic.view.notice = e.to_string(),
                }
            }
        }
        _ => {}
    }
}
fn music(app: &mut App) -> Option<&mut crate::plugins::mp3_player::Mp3Player> {
    app.desktop.plugins.acoustic_music()
}

fn resume(app: &mut App) {
    if let Some(token) = app.acoustic.music.take() {
        if let Some(p) = music(app) {
            p.resume_after_acoustic(token);
        }
    }
}
pub(super) fn poll(app: &mut App) -> bool {
    let Some(s) = app.acoustic.service.as_ref() else {
        return false;
    };
    if !super::alive(app, s.binding) {
        app.acoustic.service = None;
        app.acoustic.view.busy = false;
        app.acoustic.view.input = 0;
        app.acoustic.view.output = 0;
        app.acoustic.view.code = None;
        app.acoustic.view.offer = None;
        resume(app);
        return true;
    }
    let mut changed = false;
    let mut done = false;
    loop {
        let event = match s.events.try_recv() {
            Ok(event) => event,
            Err(mpsc::TryRecvError::Empty) => break,
            Err(mpsc::TryRecvError::Disconnected) => {
                if !done {
                    app.acoustic.view.notice = "Acoustic session ended".into();
                    app.acoustic.view.cause = Some(
                        "Acoustic worker stopped unexpectedly; delivery may be unconfirmed.".into(),
                    );
                    done = true;
                    changed = true;
                }
                break;
            }
        };
        changed = true;
        match event {
            Update::Notice(v) => app.acoustic.view.notice = v,
            Update::Devices(v) => app.acoustic.view.devices = v,
            Update::Pair(v) => {
                app.acoustic.view.code = Some(v);
                app.acoustic.view.notice =
                    "Compare the eight-digit code on both devices before confirming.".into();
            }
            Update::Offer(v) => {
                app.acoustic.view.offer = Some(format!(
                    "{} · {} bytes",
                    v.name.as_deref().unwrap_or("Text"),
                    v.size
                ))
            }
            Update::Text(v) => app.acoustic.view.received = Some(v),
            Update::Done(result) => {
                match result {
                    Ok(notice) => app.acoustic.view.notice = notice,
                    Err(cause) => {
                        app.acoustic.view.notice = "Acoustic session ended".into();
                        app.acoustic.view.cause = Some(cause);
                    }
                }
                done = true;
            }
        }
    }
    for (dest, value) in [
        (
            &mut app.acoustic.view.input,
            s.meters.input.load(Ordering::Relaxed),
        ),
        (
            &mut app.acoustic.view.output,
            s.meters.output.load(Ordering::Relaxed),
        ),
    ] {
        let v = (f32::from_bits(value).clamp(0., 1.) * 100.) as u8;
        if *dest != v {
            *dest = v;
            changed = true;
        }
    }
    let packets = (
        s.meters.sent.load(Ordering::Relaxed),
        s.meters.received.load(Ordering::Relaxed),
    );
    if app.acoustic.view.packets != packets {
        app.acoustic.view.packets = packets;
        changed = true;
    }
    let conflict = s.conflicts.pending();
    if conflict.is_some() != app.acoustic.view.conflict.is_some() {
        app.acoustic.view.conflict = conflict;
        changed = true;
    }
    if done {
        app.refresh();
        app.acoustic.service = None;
        app.acoustic.view.busy = false;
        app.acoustic.view.input = 0;
        app.acoustic.view.output = 0;
        app.acoustic.view.code = None;
        app.acoustic.view.offer = None;
        app.acoustic.view.conflict = None;
        resume(app);
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Clone, Copy)]
    enum Fault {
        None,
        Receipt,
        ReceiptAck,
        PayloadCancel,
    }
    struct RadioMemory {
        fault: Fault,
        tx: mpsc::Sender<Vec<u8>>,
        rx: mpsc::Receiver<Vec<u8>>,
        cancel: Arc<AtomicBool>,
    }
    impl Radio for RadioMemory {
        fn send(&mut self, b: &[u8]) -> io::Result<()> {
            if b.len() >= 18 && b[5] & 2 == 0 {
                let role = b[5] & 1 != 0;
                let seq = u16::from_le_bytes([b[14], b[15]]);
                match (self.fault, role, seq, b[16]) {
                    (Fault::PayloadCancel, true, 4, 1) => {
                        self.cancel.store(true, Ordering::Relaxed);
                        return Err(io::Error::new(
                            io::ErrorKind::Interrupted,
                            "synthetic mid-payload cancellation",
                        ));
                    }
                    (Fault::Receipt, false, 3, _) | (Fault::ReceiptAck, true, 5, _) => {
                        return Err(io::Error::new(
                            io::ErrorKind::BrokenPipe,
                            "synthetic lost receipt",
                        ))
                    }
                    _ => {}
                }
            }
            self.tx.send(b.to_vec()).map_err(io::Error::other)
        }
        fn receive(&mut self, d: Duration) -> io::Result<Option<Vec<u8>>> {
            if self.cancel.load(Ordering::Relaxed) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "test stopped"));
            }
            match self.rx.recv_timeout(d) {
                Ok(b) => Ok(Some(b)),
                Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
                Err(e) => Err(io::Error::other(e)),
            }
        }
    }
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> io::Result<Self> {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "dn-acoustic-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path)?;
            Ok(Self(fs::canonicalize(path)?))
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn exchange(
        payload: Payload,
        destination: PathBuf,
        confirm: bool,
        accept: bool,
        decision: crate::operations::conflict::Decision,
    ) -> (
        Result<String, String>,
        Result<String, String>,
        Option<String>,
    ) {
        exchange_fault(payload, destination, confirm, accept, decision, Fault::None)
    }
    fn exchange_fault(
        payload: Payload,
        destination: PathBuf,
        confirm: bool,
        accept: bool,
        decision: crate::operations::conflict::Decision,
        fault: Fault,
    ) -> (
        Result<String, String>,
        Result<String, String>,
        Option<String>,
    ) {
        let cancel = Arc::new(AtomicBool::new(false));
        struct Cancel(Arc<AtomicBool>);
        impl Drop for Cancel {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Relaxed);
            }
        }
        let _guard = Cancel(cancel.clone());
        let (a, ar) = mpsc::channel();
        let (b, br) = mpsc::channel();
        let mut threads = vec![];
        let mut events = vec![];
        let mut answers = vec![];
        let mut conflicts = vec![];
        for (radio, payload) in [
            (
                RadioMemory {
                    fault,
                    tx: a,
                    rx: br,
                    cancel: cancel.clone(),
                },
                Some(payload),
            ),
            (
                RadioMemory {
                    fault,
                    tx: b,
                    rx: ar,
                    cancel: cancel.clone(),
                },
                None,
            ),
        ] {
            let (tx, rx) = mpsc::sync_channel(8);
            let (answer, receive) = mpsc::sync_channel(1);
            let conflict = crate::operations::conflict::Mailbox::default();
            let (c, box_) = (cancel.clone(), conflict.clone());
            threads.push(thread::spawn(move || {
                let result =
                    run(radio, payload, &receive, &tx, &c, &box_).map_err(|e| e.to_string());
                let _ = tx.send(Update::Done(result));
            }));
            events.push(rx);
            answers.push(answer);
            conflicts.push(conflict);
        }
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut codes: [Option<String>; 2] = [None, None];
        let mut done: [Option<Result<String, String>>; 2] = [None, None];
        let mut text = None;
        let mut confirmed = false;
        while done.iter().any(Option::is_none) {
            assert!(
                Instant::now() < deadline,
                "synthetic acoustic session stalled"
            );
            for (i, rx) in events.iter().enumerate() {
                if let Ok(event) = rx.recv_timeout(Duration::from_millis(1)) {
                    match event {
                        Update::Pair(code) => codes[i] = Some(code),
                        Update::Offer(_) => answers[i]
                            .send(Answer::Approve(accept, destination.clone()))
                            .unwrap(),
                        Update::Text(v) => text = Some(v),
                        Update::Done(v) => {
                            done[i] = Some(v);
                            if !confirm {
                                cancel.store(true, Ordering::Relaxed);
                            }
                        }
                        _ => {}
                    }
                }
                if conflicts[i].pending().is_some() {
                    conflicts[i].answer(decision);
                }
            }
            if !confirmed && codes.iter().all(Option::is_some) {
                assert_eq!(codes[0], codes[1]);
                for answer in &answers {
                    answer.send(Answer::Confirm(confirm)).unwrap();
                }
                confirmed = true;
            }
        }
        for thread in threads {
            thread.join().unwrap();
        }
        let [sender, receiver] = done;
        (sender.unwrap(), receiver.unwrap(), text)
    }
    fn text(value: &str) -> Payload {
        Payload {
            name: None,
            bytes: value.as_bytes().to_vec(),
            modified: None,
            source: None,
        }
    }
    #[test]
    fn text_is_retained_only_after_pairing_and_approval() {
        let dest = PathBuf::new();
        let (s, r, value) = exchange(
            text("Ёжик — link https://example.org"),
            dest.clone(),
            true,
            true,
            crate::operations::conflict::Decision::Skip,
        );
        assert!(s.is_ok(), "{s:?}");
        assert!(r.is_ok(), "{r:?}");
        assert_eq!(value.as_deref(), Some("Ёжик — link https://example.org"));
        let (s, _, value) = exchange(
            text("secret"),
            dest,
            false,
            true,
            crate::operations::conflict::Decision::Skip,
        );
        assert!(s.is_err());
        assert!(value.is_none());
    }
    #[test]
    fn declined_offer_and_size_and_name_limits() {
        let (s, r, value) = exchange(
            text("not accepted"),
            PathBuf::new(),
            true,
            false,
            crate::operations::conflict::Decision::Skip,
        );
        assert!(s.is_err() && r.is_err());
        assert!(value.is_none());
        for name in ["../bad", "a/b", "a\\b", "CON", "."] {
            assert!(
                Offer {
                    name: Some(name.into()),
                    size: 1,
                    hash: [0; 32],
                    modified: None
                }
                .validate()
                .is_err(),
                "{name}"
            );
        }
        assert!(Offer {
            name: None,
            size: 1025,
            hash: [0; 32],
            modified: None
        }
        .validate()
        .is_err());
    }
    #[test]
    fn retired_worker_cannot_leave_busy_or_steal_document_focus() -> io::Result<()> {
        let t = Temp::new()?;
        let mut app = App::new(t.0.clone())?;
        app.config.set_plugin_enabled(ACOUSTIC_TRANSFERS, true);
        crate::plugins::host::open(&mut app, ACOUSTIC_TRANSFERS);
        let binding = app.desktop.plugins.binding(ACOUSTIC_TRANSFERS).unwrap();
        let owner = app.desktop.active().id;
        let (tx, events) = mpsc::sync_channel(1);
        drop(tx);
        let (answers, _) = mpsc::sync_channel(1);
        app.acoustic.service = Some(Service {
            binding,
            cancel: Arc::new(AtomicBool::new(false)),
            answers,
            events,
            meters: Arc::new(Meters::default()),
            conflicts: Default::default(),
        });
        app.acoustic.view.busy = true;
        assert!(poll(&mut app));
        assert!(!app.acoustic.view.busy);
        assert!(app
            .acoustic
            .view
            .cause
            .as_deref()
            .unwrap()
            .contains("unconfirmed"));
        assert_eq!(app.desktop.active().id, owner);
        let notice = app.acoustic.view.notice.clone();
        crate::acoustic::action(&mut app, binding, u64::MAX, Action::Stop);
        assert_eq!(app.acoustic.view.notice, notice);
        let (tx, events) = mpsc::sync_channel(1);
        let (answers, _) = mpsc::sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        app.acoustic.service = Some(Service {
            binding,
            cancel: cancel.clone(),
            answers,
            events,
            meters: Arc::new(Meters::default()),
            conflicts: Default::default(),
        });
        app.acoustic.view.busy = true;
        app.acoustic.view.input = 42;
        app.acoustic.view.output = 50;
        app.acoustic.view.conflict = Some(crate::operations::conflict::Conflict {
            source: PathBuf::from("source"),
            source_label: None,
            destination: PathBuf::from("destination"),
            destination_label: None,
            source_size: 1,
            destination_size: 1,
            source_modified: None,
            destination_modified: None,
        });
        let generation = app.acoustic.view.generation;
        crate::acoustic::action(&mut app, binding, generation, Action::Stop);
        assert!(cancel.load(Ordering::Relaxed));
        assert!(!app.acoustic.view.busy);
        assert!(app.acoustic.view.conflict.is_none());
        assert_eq!((app.acoustic.view.input, app.acoustic.view.output), (0, 0));
        drop(tx);
        Ok(())
    }
    #[test]
    fn lost_receipts_keep_received_text_and_report_unconfirmed_delivery() {
        let (sender, receiver, retained) = exchange_fault(
            text("retained despite lost receipt"),
            PathBuf::new(),
            true,
            true,
            crate::operations::conflict::Decision::Skip,
            Fault::Receipt,
        );
        assert!(sender.unwrap_err().contains("Delivery unconfirmed"));
        assert!(receiver.unwrap().contains("Receipt unconfirmed"));
        assert_eq!(retained.as_deref(), Some("retained despite lost receipt"));
    }
    #[test]
    fn acknowledged_receipt_remains_confirmed_if_its_return_ack_is_lost() {
        let (sender, receiver, retained) = exchange_fault(
            text("receipt received"),
            PathBuf::new(),
            true,
            true,
            crate::operations::conflict::Decision::Skip,
            Fault::ReceiptAck,
        );
        assert!(sender
            .unwrap()
            .starts_with("Delivery confirmed by receiver"));
        assert!(receiver.unwrap().contains("unconfirmed"));
        assert_eq!(retained.as_deref(), Some("receipt received"));
    }
    #[test]
    fn cancellation_mid_payload_never_publishes_an_incomplete_file() -> io::Result<()> {
        let t = Temp::new()?;
        let payload = Payload {
            name: Some("partial.bin".into()),
            bytes: vec![9; 1024],
            modified: None,
            source: None,
        };
        let (sender, receiver, text) = exchange_fault(
            payload,
            t.0.clone(),
            true,
            true,
            crate::operations::conflict::Decision::Skip,
            Fault::PayloadCancel,
        );
        assert!(sender.is_err() && receiver.is_err());
        assert!(text.is_none());
        assert_eq!(fs::read_dir(&t.0)?.count(), 0);
        Ok(())
    }
    #[test]
    fn file_publication_reuses_replace_skip_and_preserves_unrelated_files() -> io::Result<()> {
        let t = Temp::new()?;
        fs::write(t.0.join("unrelated"), b"keep")?;
        let payload = Payload {
            name: Some("Ёжик.txt".into()),
            bytes: b"new".to_vec(),
            modified: None,
            source: None,
        };
        for existing in [
            None,
            Some(crate::operations::conflict::Decision::Skip),
            Some(crate::operations::conflict::Decision::Replace),
        ] {
            if existing.is_some() {
                fs::write(t.0.join("Ёжик.txt"), b"old")?;
            }
            let (s, r, _) = exchange(
                payload.clone(),
                t.0.clone(),
                true,
                true,
                existing.unwrap_or(crate::operations::conflict::Decision::Skip),
            );
            assert!(s.is_ok(), "{s:?}");
            assert!(r.is_ok(), "{r:?}");
            assert_eq!(
                fs::read(t.0.join("Ёжик.txt"))?,
                if existing == Some(crate::operations::conflict::Decision::Skip) {
                    b"old"
                } else {
                    b"new"
                }
            );
            assert_eq!(fs::read(t.0.join("unrelated"))?, b"keep");
        }
        assert_eq!(fs::read_dir(&t.0)?.count(), 2);
        Ok(())
    }
}
