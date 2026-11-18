//! Host-owned simplex jobs. Transport ends before local review/publication.
use super::*;
use super::{
    audio::{check, Codec, Meters, Microphone, Speaker},
    protocol::{Collector, Payload, Prepared},
    wav,
};
use std::{
    fs,
    io::{self, Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};
#[derive(Clone)]
pub(super) struct Captured {
    payload: Payload,
    path: PathBuf,
    identity: crate::platform_fs::EntryIdentity,
    metadata: fs::Metadata,
}
impl Captured {
    fn validate(&self) -> io::Result<()> {
        let current = fs::symlink_metadata(&self.path)?;
        if crate::platform_fs::entry_identity(&self.path)? != self.identity
            || current.len() != self.metadata.len()
            || current.modified().ok() != self.metadata.modified().ok()
            || !current.is_file()
            || crate::platform_fs::directory_link(&current)
        {
            return Err(io::Error::other("Source changed; use current file again"));
        }
        Ok(())
    }
}
// Retired audio workers may still be releasing backend streams. Never open a
// second device session until their worker-side destruction completes.
static AUDIO_ACTIVE: AtomicBool = AtomicBool::new(false);
struct AudioLease;
impl AudioLease {
    fn acquire() -> io::Result<Self> {
        if AUDIO_ACTIVE
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            return Err(io::Error::other(
                "Previous acoustic devices are still closing; try again",
            ));
        }
        Ok(Self)
    }
}
impl Drop for AudioLease {
    fn drop(&mut self) {
        AUDIO_ACTIVE.store(false, Ordering::Release);
    }
}
enum Answer {
    Approve(bool, PathBuf),
}
enum Update {
    Devices(String),
    Offer(super::protocol::Offer),
    Text(String),
    Plan { seconds: u64, revision: u64 },
    Notice(String),
}
enum Outcome {
    Captured(Box<Captured>),
    Finished(String),
    Published(String),
}
pub(super) struct Service {
    binding: Binding,
    task: crate::workers::Worker<Outcome>,
    answers: mpsc::SyncSender<Answer>,
    events: mpsc::Receiver<Update>,
    meters: Arc<Meters>,
    conflicts: crate::operations::conflict::Mailbox,
    started: Instant,
}
fn emit(tx: &mpsc::SyncSender<Update>, value: Update) -> io::Result<()> {
    tx.try_send(value)
        .map_err(|_| io::Error::other("Acoustic window closed or event queue full"))
}
fn captured(path: PathBuf, cancel: &AtomicBool) -> io::Result<Captured> {
    if cancel.load(Ordering::Relaxed) {
        return Err(io::ErrorKind::Interrupted.into());
    }
    let identity = crate::platform_fs::entry_identity(&path)?;
    let mut file = crate::filesystem::open_regular(&path)?;
    let metadata = file.metadata()?;
    if metadata.len() > MAX_BYTES as u64 {
        return Err(io::Error::other("Acoustic file limit is 1 MiB"));
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if cancel.load(Ordering::Relaxed) {
        return Err(io::ErrorKind::Interrupted.into());
    }
    let after = file.metadata()?;
    if bytes.len() > MAX_BYTES
        || !crate::platform_fs::owns_file(&path, &file)
        || metadata.len() != after.len()
        || metadata.modified().ok() != after.modified().ok()
    {
        return Err(io::Error::other("Source changed during acoustic capture"));
    }
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| io::Error::other("Acoustic filename must be Unicode"))?
        .to_owned();
    let modified = metadata
        .modified()
        .ok()
        .and_then(crate::peer::manifest::seconds);
    let source = Captured {
        payload: Payload::new(Some(name), bytes, modified)?,
        path,
        identity,
        metadata,
    };
    source.validate()?;
    Ok(source)
}
fn wait(rx: &mpsc::Receiver<Answer>, cancel: &AtomicBool) -> io::Result<(bool, PathBuf)> {
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(io::ErrorKind::Interrupted.into());
        }
        match rx.recv_timeout(Duration::from_millis(20)) {
            Ok(Answer::Approve(a, p)) => return Ok((a, p)),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(e) => return Err(io::Error::other(e)),
        }
    }
}
fn publish(
    payload: Payload,
    destination: PathBuf,
    conflicts: &crate::operations::conflict::Mailbox,
    meters: &Meters,
    cancel: &AtomicBool,
) -> io::Result<Outcome> {
    let name = payload
        .offer
        .name
        .as_ref()
        .ok_or_else(|| io::Error::other("No file to publish"))?;
    let entry = crate::peer::manifest::Entry {
        path: name.clone(),
        size: payload.offer.size as u64,
        directory: false,
        modified: payload.offer.modified,
    };
    let mut ask = |c: &crate::operations::conflict::Conflict| {
        conflicts.ask_pumping(c, cancel, || check(cancel, meters))
    };
    let mut sink = crate::operations::incoming::Sink::new(
        destination,
        crate::operations::ExistingDestination::Ask,
        &mut ask,
    )?;
    let Some(mut file) = sink.begin(&entry)? else {
        return Ok(Outcome::Finished("File skipped; nothing published.".into()));
    };
    check(cancel, meters)?;
    file.stage.file.write_all(&payload.bytes)?;
    let mut progress = crate::operations::Progress::default();
    sink.finish(
        file,
        payload.offer.hash,
        crate::operations::CopyOptions {
            verify_writes: true,
            ..Default::default()
        },
        cancel,
        &mut progress,
    )?;
    Ok(Outcome::Published(
        if progress.details.is_empty() {
            "File received, verified and published."
        } else {
            "File published with a durability warning; review the destination."
        }
        .into(),
    ))
}
fn export(
    path: PathBuf,
    prepared: &Prepared,
    options: &Options,
    meters: &Meters,
    cancel: &AtomicBool,
    conflicts: &crate::operations::conflict::Mailbox,
    source: Option<&Captured>,
) -> io::Result<Outcome> {
    if let Some(source) = source {
        source.validate()?;
        match fs::symlink_metadata(&path) {
            Ok(metadata) if crate::platform_fs::same_identity(&metadata, &source.metadata) => {
                return Err(io::Error::other("WAV output cannot replace its source"))
            }
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    if options.repeats == 0 {
        return Err(io::Error::other("WAV export needs a finite repeat count"));
    }
    let (bytes, frames) = wav::planned(prepared, options, cancel)?;
    meters.total.store(frames, Ordering::Relaxed);
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| io::Error::other("Choose a Unicode WAV filename"))?;
    if !path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("wav"))
    {
        return Err(io::Error::other("Choose a .wav output filename"));
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| io::Error::other("Choose a local WAV destination"))?;
    let entry = crate::peer::manifest::Entry {
        path: name.into(),
        size: bytes,
        directory: false,
        modified: None,
    };
    let mut ask = |c: &crate::operations::conflict::Conflict| {
        conflicts.ask_pumping(c, cancel, || check(cancel, meters))
    };
    let mut sink = crate::operations::incoming::Sink::new(
        parent.to_owned(),
        crate::operations::ExistingDestination::Ask,
        &mut ask,
    )?;
    let Some(mut file) = sink.begin(&entry)? else {
        return Ok(Outcome::Finished(
            "WAV export skipped; nothing published.".into(),
        ));
    };
    let hash = wav::write(
        &mut file.stage.file,
        prepared,
        options,
        bytes,
        meters,
        cancel,
    )?;
    if let Some(source) = source {
        source.validate()?;
    }
    let mut progress = crate::operations::Progress::default();
    sink.finish(
        file,
        hash,
        crate::operations::CopyOptions {
            verify_writes: true,
            ..Default::default()
        },
        cancel,
        &mut progress,
    )?;
    Ok(Outcome::Published(
        if progress.details.is_empty() {
            "WAV exported. Delivery is not confirmed."
        } else {
            "WAV published with a durability warning; review the destination."
        }
        .into(),
    ))
}
fn receive(
    mut next: impl FnMut(Duration) -> io::Result<Option<Vec<u8>>>,
    options: &Options,
    meters: &Meters,
    cancel: &AtomicBool,
) -> io::Result<Payload> {
    let mut collector = Collector::default();
    let mut idle = Instant::now();
    loop {
        check(cancel, meters)?;
        if let Some(packet) = next(Duration::from_millis(20))? {
            let complete = collector.ingest(&packet)?;
            let (a, b) = collector.progress();
            if b > 0 {
                idle = Instant::now();
            }
            meters.completed.store(a, Ordering::Relaxed);
            meters.total.store(b, Ordering::Relaxed);
            if complete {
                check(cancel, meters)?;
                return collector.finish(options);
            }
        }
        if idle.elapsed() > Duration::from_secs(300) {
            return Err(io::Error::other(
                "No complete acoustic transfer; receive again or play another repetition",
            ));
        }
    }
}
fn run(
    request: Request,
    source: Option<Captured>,
    rx: &mpsc::Receiver<Answer>,
    tx: &mpsc::SyncSender<Update>,
    meters: Arc<Meters>,
    cancel: Arc<AtomicBool>,
    conflicts: &crate::operations::conflict::Mailbox,
) -> io::Result<Outcome> {
    request.options.validate()?;
    check(&cancel, &meters)?;
    let Request {
        operation,
        input,
        options,
        revision,
    } = request;
    let outgoing = matches!(
        operation,
        Operation::Send | Operation::Estimate | Operation::Export(_)
    );
    if outgoing {
        let payload = match input {
            Input::Text(text) => {
                if text.is_empty() {
                    return Err(io::Error::other("Enter text before sending"));
                }
                Payload::new(None, text.into_bytes(), None)?
            }
            Input::File => {
                let captured = source
                    .as_ref()
                    .ok_or_else(|| io::Error::other("Use current file first."))?;
                captured.validate()?;
                captured.payload.clone()
            }
            Input::None => return Err(io::Error::other("Choose text or a captured file")),
        };
        let prepared = Prepared::new(&payload, &options)?;
        if let Some(source) = &source {
            source.validate()?;
        }
        check(&cancel, &meters)?;
        let mut codec = Codec::new(wav::RATE, options.speed)?;
        let (samples, frames) = codec.plan(&prepared, options.repeats, &cancel)?;
        emit(
            tx,
            Update::Plan {
                seconds: samples.div_ceil(u64::from(wav::RATE)),
                revision,
            },
        )?;
        if matches!(operation, Operation::Estimate) {
            return Ok(Outcome::Finished(
                "Playback estimate ready; no audio device opened.".into(),
            ));
        }
        if let Operation::Export(path) = operation {
            if source.as_ref().is_some_and(|s| {
                crate::requests::absolute_key(&path) == crate::requests::absolute_key(&s.path)
            }) {
                return Err(io::Error::other("WAV output cannot replace its source"));
            }
            return export(
                path,
                &prepared,
                &options,
                &meters,
                &cancel,
                conflicts,
                source.as_ref(),
            );
        }
        meters.total.store(frames, Ordering::Relaxed);
        let _lease = AudioLease::acquire()?;
        let mut speaker = Speaker::open(cancel.clone(), meters.clone(), options.speed)?;
        emit(tx, Update::Devices(speaker.devices.clone()))?;
        emit(
            tx,
            Update::Notice("Playing one-way audio; the sender cannot confirm delivery.".into()),
        )?;
        let mut cycle = 0_u64;
        loop {
            if let Some(s) = &source {
                s.validate()?;
            }
            for frame in prepared.frames() {
                check(&cancel, &meters)?;
                speaker.send(&frame)?;
                meters.completed.fetch_add(1, Ordering::Relaxed);
            }
            cycle += 1;
            if options.repeats > 0 && cycle >= u64::from(options.repeats) {
                break;
            }
        }
        if let Some(s) = source {
            s.validate()?;
        }
        return Ok(Outcome::Finished(
            "Transmission finished. Delivery is not confirmed.".into(),
        ));
    }
    let payload = match operation {
        Operation::Import(path) => wav::read(&path, &options, &meters, &cancel)?,
        Operation::Receive => {
            let _lease = AudioLease::acquire()?;
            let mut microphone = Microphone::open(cancel.clone(), meters.clone())?;
            emit(tx, Update::Devices(microphone.devices.clone()))?;
            emit(
                tx,
                Update::Notice("Listening only. Collecting blocks across repetitions…".into()),
            )?;
            receive(|wait| microphone.receive(wait), &options, &meters, &cancel)?
        }
        _ => unreachable!(),
    };
    // The microphone is dropped before review. Reception requires no response.
    emit(tx, Update::Devices(String::new()))?;
    emit(tx, Update::Offer(payload.offer.clone()))?;
    let (accept, destination) = wait(rx, &cancel)?;
    if !accept {
        return Ok(Outcome::Finished(
            "Received data dismissed; nothing published.".into(),
        ));
    }
    if payload.offer.name.is_some() {
        publish(payload, destination, conflicts, &meters, &cancel)
    } else {
        let text = String::from_utf8(payload.bytes.to_vec()).map_err(io::Error::other)?;
        emit(tx, Update::Text(text))?;
        Ok(Outcome::Finished(
            "Text received and verified; retained in this window.".into(),
        ))
    }
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
            if let Some(s) = app.acoustic.service.take() {
                s.conflicts
                    .answer(crate::operations::conflict::Decision::Cancel);
            }
            app.acoustic.view.busy = false;
            app.acoustic.view.offer = None;
            app.acoustic.view.conflict = None;
            app.acoustic.view.input = 0;
            app.acoustic.view.output = 0;
            app.acoustic.view.remaining = None;
            app.acoustic.view.notice =
                "Acoustic operation stopped; completed publications remain.".into();
            resume(app);
            app.refresh();
        }
        Action::Capture if !app.acoustic.view.busy => {
            let panel = &app.panels[app.active];
            if panel.is_virtual() || panel.loading {
                app.acoustic.view.notice = "Extract or download a regular file first".into();
                return;
            }
            let Some(entry) = panel
                .entries
                .get(panel.cursor)
                .filter(|e| !e.parent && !e.directory && !e.symlink)
            else {
                app.acoustic.view.notice = "Select one regular file, at most 1 MiB".into();
                return;
            };
            let path = entry.path.clone();
            app.acoustic.source = None;
            app.acoustic.view.source.clear();
            app.acoustic.view.estimate = None;
            spawn(
                app,
                b,
                false,
                move |cancel, _rx, _tx, _meters, _conflicts| {
                    captured(path, &cancel).map(|s| Outcome::Captured(Box::new(s)))
                },
            );
        }
        Action::Start(request) if !app.acoustic.view.busy => {
            if let Err(e) = request.options.validate() {
                app.acoustic.view.notice = e.to_string();
                return;
            }
            let devices = matches!(request.operation, Operation::Send | Operation::Receive);
            let source = if matches!(request.input, Input::File) {
                app.acoustic.source.clone()
            } else {
                None
            };
            app.acoustic.view.received = None;
            spawn(app, b, devices, move |cancel, rx, tx, meters, conflicts| {
                run(request, source, rx, tx, meters, cancel, conflicts)
            });
        }
        Action::Approve {
            accept,
            destination,
        } => {
            if app.acoustic.view.offer.is_some() {
                if let Some(s) = &app.acoustic.service {
                    if s.answers
                        .try_send(Answer::Approve(accept, destination))
                        .is_ok()
                    {
                        app.acoustic.view.offer = None;
                    }
                }
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
fn spawn(
    app: &mut App,
    b: Binding,
    devices: bool,
    work: impl FnOnce(
            Arc<AtomicBool>,
            &mpsc::Receiver<Answer>,
            &mpsc::SyncSender<Update>,
            Arc<Meters>,
            &crate::operations::conflict::Mailbox,
        ) -> io::Result<Outcome>
        + Send
        + 'static,
) {
    let Owner::Document(owner) = b.owner else {
        return;
    };
    let meters = Arc::new(Meters::default());
    let (answer, rx) = mpsc::sync_channel(1);
    let (tx, events) = mpsc::sync_channel(8);
    let conflicts = crate::operations::conflict::Mailbox::default();
    let (m, c) = (meters.clone(), conflicts.clone());
    match app.workers.spawn(
        "dn-acoustic",
        &app.acoustic.requests,
        crate::requests::Owner::Document(owner),
        "acoustic",
        move |cancel| work(cancel.flag(), &rx, &tx, m, &c),
    ) {
        Ok(task) => {
            if devices {
                app.acoustic.music = music(app).and_then(|p| p.pause_for_acoustic());
            }
            app.acoustic.view.generation = app.acoustic.view.generation.wrapping_add(1);
            app.acoustic.view.busy = true;
            app.acoustic.view.notice = "Preparing one-way acoustic operation…".into();
            app.acoustic.view.cause = None;
            app.acoustic.view.offer = None;
            app.acoustic.view.conflict = None;
            app.acoustic.view.devices.clear();
            app.acoustic.view.packets = (0, 0);
            app.acoustic.view.completed = 0;
            app.acoustic.view.total = 0;
            app.acoustic.view.elapsed = 0;
            app.acoustic.view.remaining = None;
            app.acoustic.view.dropped = 0;
            app.acoustic.service = Some(Service {
                binding: b,
                task,
                answers: answer,
                events,
                meters,
                conflicts,
                started: Instant::now(),
            });
        }
        Err(e) => app.acoustic.view.notice = e.to_string(),
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
        app.acoustic.view.offer = None;
        app.acoustic.view.conflict = None;
        app.acoustic.view.input = 0;
        app.acoustic.view.output = 0;
        resume(app);
        return true;
    }
    let mut changed = false;
    let mut release_music = false;
    for _ in 0..8 {
        match s.events.try_recv() {
            Ok(event) => {
                changed = true;
                match event {
                    Update::Devices(v) => {
                        release_music |= v.is_empty();
                        app.acoustic.view.devices = v;
                    }
                    Update::Offer(v) => {
                        app.acoustic.view.file_offer = v.name.is_some();
                        app.acoustic.view.offer = Some(format!(
                            "{} · {} bytes",
                            crate::filesystem::display_name(
                                v.name
                                    .as_deref()
                                    .unwrap_or(app.config.language.text("Text"))
                            ),
                            v.size
                        ));
                        app.acoustic.view.notice =
                            "Received and verified. Review before retaining or saving.".into();
                        app.acoustic.view.remaining = None;
                    }
                    Update::Text(v) => app.acoustic.view.received = Some(v),
                    Update::Plan { seconds, revision } => {
                        app.acoustic.view.estimate = Some(seconds);
                        app.acoustic.view.plan_revision = revision;
                    }
                    Update::Notice(v) => app.acoustic.view.notice = v,
                }
            }
            Err(_) => break,
        }
    }
    let v = &mut app.acoustic.view;
    let elapsed = s.started.elapsed().as_secs();
    if v.elapsed != elapsed {
        v.elapsed = elapsed;
        changed = true;
    }
    let completed = s.meters.completed.load(Ordering::Relaxed);
    let total = s.meters.total.load(Ordering::Relaxed);
    let dropped = s.meters.dropped.load(Ordering::Relaxed);
    let packets = (
        s.meters.sent.load(Ordering::Relaxed),
        s.meters.received.load(Ordering::Relaxed),
    );
    if v.completed != completed || v.total != total || v.packets != packets || v.dropped != dropped
    {
        changed = true;
        v.completed = completed;
        v.total = total;
        v.packets = packets;
        v.dropped = dropped;
    }
    v.remaining = if elapsed >= 2
        && completed > 0
        && completed < total
        && v.offer.is_none()
        && v.conflict.is_none()
    {
        Some(((total - completed) as f64 * elapsed as f64 / completed as f64).ceil() as u64)
    } else {
        None
    };
    for (dest, meter) in [
        (&mut v.input, &s.meters.input),
        (&mut v.output, &s.meters.output),
    ] {
        let level = (f32::from_bits(meter.load(Ordering::Relaxed)).clamp(0., 1.) * 100.) as u8;
        if *dest != level {
            *dest = level;
            changed = true;
        }
    }
    let pending = s.conflicts.pending();
    if pending.is_some() != v.conflict.is_some() {
        v.conflict = pending;
        changed = true;
    }
    let result = match s.task.try_recv() {
        Ok(value) => Some(value),
        Err(mpsc::TryRecvError::Empty) => None,
        Err(_) => Some(Err(io::Error::other(
            "Acoustic worker ended unexpectedly; delivery is not confirmed",
        ))),
    };
    if release_music {
        resume(app);
    }
    if let Some(result) = result {
        let mut refresh = false;
        match result {
            Ok(Outcome::Captured(source)) => {
                app.acoustic.view.source = format!(
                    "{} ({} bytes)",
                    crate::filesystem::display_name(
                        source.payload.offer.name.as_deref().unwrap_or("")
                    ),
                    source.payload.bytes.len()
                );
                app.acoustic.source = Some(*source);
                app.acoustic.view.notice =
                    "File captured. Use Estimate before sending large payloads.".into();
            }
            Ok(Outcome::Finished(notice)) => app.acoustic.view.notice = notice,
            Ok(Outcome::Published(notice)) => {
                app.acoustic.view.notice = notice;
                refresh = true;
            }
            Err(error) => {
                app.acoustic.view.notice = "Acoustic operation ended".into();
                app.acoustic.view.cause = Some(error.to_string());
            }
        }
        app.acoustic.view.busy = false;
        app.acoustic.view.input = 0;
        app.acoustic.view.output = 0;
        app.acoustic.view.offer = None;
        app.acoustic.view.conflict = None;
        app.acoustic.view.remaining = None;
        app.acoustic.service = None;
        if refresh {
            app.refresh();
        }
        resume(app);
        return true;
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::VecDeque, sync::atomic::AtomicU64};
    use zeroize::Zeroizing;
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> io::Result<Self> {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "dn-simplex-session-{}-{}",
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
    fn options() -> Options {
        Options {
            passphrase: Zeroizing::new("private session passphrase".into()),
            repeats: 1,
            ..Default::default()
        }
    }
    fn payload() -> Payload {
        Payload::new(Some("Ёжик.txt".into()), b"new contents".to_vec(), None).unwrap()
    }
    #[test]
    fn receiver_collects_without_any_return_channel_and_requires_local_approval() -> io::Result<()>
    {
        let options = options();
        let original = payload();
        let prepared = Prepared::new(&original, &options)?;
        let mut frames = prepared.frames().collect::<VecDeque<_>>();
        let cancel = AtomicBool::new(false);
        let meters = Meters::default();
        let got = receive(|_| Ok(frames.pop_front()), &options, &meters, &cancel)?;
        assert_eq!(&*got.bytes, &*original.bytes);
        let (answers, rx) = mpsc::sync_channel(1);
        answers
            .send(Answer::Approve(false, PathBuf::new()))
            .unwrap();
        assert!(!wait(&rx, &cancel)?.0);
        cancel.store(true, Ordering::Relaxed);
        assert!(receive(
            |_| panic!("cancelled receiver read input"),
            &options,
            &meters,
            &cancel
        )
        .is_err());
        Ok(())
    }
    #[test]
    fn capture_above_old_limit_unicode_change_detection_and_size_boundary() -> io::Result<()> {
        let t = Temp::new()?;
        let path = t.0.join("Ёжик.bin");
        fs::write(&path, vec![42; MAX_BYTES])?;
        let cancel = AtomicBool::new(false);
        let source = captured(path.clone(), &cancel)?;
        assert_eq!(source.payload.bytes.len(), MAX_BYTES);
        source.validate()?;
        fs::write(&path, b"changed")?;
        assert!(source.validate().is_err());
        fs::write(&path, vec![0; MAX_BYTES + 1])?;
        assert!(captured(path, &cancel).is_err());
        #[cfg(unix)]
        {
            let link = t.0.join("link");
            std::os::unix::fs::symlink(t.0.join("Ёжик.bin"), &link)?;
            assert!(captured(link, &cancel).is_err());
        }
        Ok(())
    }
    #[test]
    fn publication_preserves_existing_files_and_handles_replace_skip_cancel() -> io::Result<()> {
        use crate::operations::conflict::Decision;
        for decision in [Decision::Replace, Decision::Skip, Decision::Cancel] {
            let t = Temp::new()?;
            fs::write(t.0.join("Ёжик.txt"), b"old")?;
            fs::write(t.0.join("unrelated"), b"keep")?;
            let conflicts = crate::operations::conflict::Mailbox::default();
            let worker_box = conflicts.clone();
            let path = t.0.clone();
            let thread = std::thread::spawn(move || {
                publish(
                    payload(),
                    path,
                    &worker_box,
                    &Meters::default(),
                    &AtomicBool::new(false),
                )
            });
            let end = Instant::now() + Duration::from_secs(5);
            while conflicts.pending().is_none() {
                assert!(Instant::now() < end);
                std::thread::sleep(Duration::from_millis(1));
            }
            conflicts.answer(decision);
            let result = thread.join().unwrap();
            if decision == Decision::Cancel {
                assert!(result.is_err());
            } else {
                assert!(
                    result.is_ok(),
                    "publication failed: {:?}",
                    result.err().map(|e| e.to_string())
                );
            }
            assert_eq!(
                fs::read(t.0.join("Ёжик.txt"))?,
                if decision == Decision::Replace {
                    b"new contents".as_slice()
                } else {
                    b"old".as_slice()
                }
            );
            assert_eq!(fs::read(t.0.join("unrelated"))?, b"keep");
            assert_eq!(fs::read_dir(&t.0)?.count(), 2);
        }
        Ok(())
    }
    #[test]
    fn cancellation_cleans_owned_staging_and_keeps_destinations_unchanged() -> io::Result<()> {
        let t = Temp::new()?;
        fs::write(t.0.join("unrelated"), b"keep")?;
        assert!(publish(
            payload(),
            t.0.clone(),
            &Default::default(),
            &Meters::default(),
            &AtomicBool::new(true)
        )
        .is_err());
        assert_eq!(fs::read_dir(&t.0)?.count(), 1);
        Ok(())
    }
    #[test]
    fn estimation_and_cancelled_wav_export_never_open_audio_devices() -> io::Result<()> {
        let t = Temp::new()?;
        let (tx, events) = mpsc::sync_channel(8);
        let (_answer, rx) = mpsc::sync_channel(1);
        let meters = Arc::new(Meters::default());
        let cancel = Arc::new(AtomicBool::new(false));
        let request = Request {
            operation: Operation::Estimate,
            input: Input::Text("a link".into()),
            options: options(),
            revision: 44,
        };
        assert!(matches!(
            run(
                request,
                None,
                &rx,
                &tx,
                meters.clone(),
                cancel.clone(),
                &Default::default()
            )?,
            Outcome::Finished(_)
        ));
        assert!(events
            .try_iter()
            .any(|e| matches!(e,Update::Plan{seconds,revision:44}if seconds>0)));
        let prepared = Prepared::new(&payload(), &options())?;
        cancel.store(true, Ordering::Relaxed);
        assert!(export(
            t.0.join("out.wav"),
            &prepared,
            &options(),
            &meters,
            &cancel,
            &Default::default(),
            None
        )
        .is_err());
        assert_eq!(fs::read_dir(&t.0)?.count(), 0);
        Ok(())
    }
    #[test]
    fn retired_owner_cancels_worker_and_stale_actions_never_update_reopened_window(
    ) -> io::Result<()> {
        let t = Temp::new()?;
        let mut app = App::new(t.0.clone())?;
        app.config.save_on_exit = false;
        app.config_store = crate::config::Store::new(Some(t.0.join("private.conf")));
        app.config.set_plugin_enabled(ACOUSTIC_TRANSFERS, true);
        crate::plugins::host::open(&mut app, ACOUSTIC_TRANSFERS);
        let binding = app.desktop.plugins.binding(ACOUSTIC_TRANSFERS).unwrap();
        let owner = app.desktop.active().id;
        let (start, started) = mpsc::sync_channel(1);
        let (release, gate) = mpsc::sync_channel(1);
        spawn(&mut app, binding, false, move |cancel, _, _, _, _| {
            start.send(()).unwrap();
            gate.recv_timeout(Duration::from_secs(5)).unwrap();
            if cancel.load(Ordering::Relaxed) {
                return Err(io::ErrorKind::Interrupted.into());
            }
            Ok(Outcome::Finished("stale result".into()))
        });
        started.recv_timeout(Duration::from_secs(5)).unwrap();
        app.desktop.close(owner);
        assert!(poll(&mut app));
        assert!(!app.acoustic.view.busy);
        release.send(()).unwrap();
        crate::plugins::host::open(&mut app, ACOUSTIC_TRANSFERS);
        let active = app.desktop.active().id;
        let before = app.acoustic.view.notice.clone();
        let end = Instant::now() + Duration::from_secs(5);
        while app.workers.pending() {
            app.poll();
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(app.desktop.active().id, active);
        assert_ne!(active, owner);
        assert_eq!(app.acoustic.view.notice, before);
        crate::acoustic::action(&mut app, binding, u64::MAX, Action::Stop);
        assert_eq!(app.desktop.active().id, active);
        assert!(app.last_dialog().is_none());
        Ok(())
    }
}
