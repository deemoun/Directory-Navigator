use super::*;
use std::{
    net::{TcpListener, TcpStream},
    sync::atomic::Ordering,
    thread,
    time::{Duration, Instant},
};
fn context() -> (engine::JobContext, mpsc::SyncSender<(u64, bool, PathBuf)>) {
    let (send, receive) = mpsc::sync_channel(1);
    (
        engine::JobContext {
            cancel: Arc::new(AtomicBool::new(false)),
            progress: Default::default(),
            approvals: receive,
            conflicts: Default::default(),
            flow: Default::default(),
        },
        send,
    )
}
fn sockets() -> io::Result<(TcpStream, TcpStream)> {
    let server = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
    let sender = TcpStream::connect(server.local_addr()?)?;
    let (receiver, _) = server.accept()?;
    for stream in [&sender, &receiver] {
        stream.set_read_timeout(Some(Duration::from_millis(100)))?;
        stream.set_write_timeout(Some(Duration::from_millis(100)))?;
    }
    Ok((sender, receiver))
}
#[test]
fn obex_headers_strict_unicode_lengths_and_portable_names() -> io::Result<()> {
    let name = "Ёжик 世界.mp3";
    let mut bytes = obex::name_header(name)?;
    bytes.extend([0xc3, 0, 0, 0, 4]);
    bytes.extend([0x49, 0, 7, 1, 2, 3, 4]);
    let h = obex::headers(&bytes)?;
    assert_eq!(h.name.as_deref(), Some(name));
    assert_eq!(h.size, Some(4));
    assert_eq!(h.body, vec![1, 2, 3, 4]);
    assert!(h.end);
    for invalid in ["../escape", "a/b", "a\\b", "CON", "a\n.txt", "", "nul.txt"] {
        assert!(obex::name_header(invalid).is_err(), "{invalid:?}");
    }
    for invalid in [
        vec![1, 0, 2],
        vec![1, 0, 6, 0, 65, 0],
        vec![1, 0, 7, 0xd8, 0, 0, 0],
        vec![0xc3, 0, 0],
        vec![0x49, 0, 3, 0x48, 0, 3],
    ] {
        assert!(obex::headers(&invalid).is_err());
    }
    assert_eq!(platform::address("01-23-45-67-89-ab")?, "01:23:45:67:89:AB");
    assert!(platform::address("01:23").is_err());
    Ok(())
}
#[test]
fn obex_streams_queue_unicode_empty_files_and_keeps_originals() -> io::Result<()> {
    let work = crate::network::workspace::Workspace::create(std::env::temp_dir())?;
    let input = work.root.join("input");
    let output = work.root.join("output");
    fs::create_dir(&input)?;
    fs::create_dir(&output)?;
    let payload = vec![37; obex::MTU * 4 + 17];
    let path = input.join("Ёжик 世界.bin");
    fs::write(&path, &payload)?;
    let empty = input.join("empty.txt");
    fs::write(&empty, [])?;
    let files = capture(
        vec![path.clone(), empty.clone(), path.clone()],
        &AtomicBool::new(false),
    )?;
    assert_eq!(files.len(), 2);
    let (mut sender, mut receiver) = sockets()?;
    let (client, _) = context();
    let (server, approval) = context();
    let progress = server.progress.clone();
    let destination = output.clone();
    let done = Arc::new(AtomicBool::new(false));
    let stop = done.clone();
    let approvals = thread::spawn(move || {
        let end = Instant::now() + Duration::from_secs(10);
        let mut previous = 0;
        while !stop.load(Ordering::Relaxed) {
            if Instant::now() > end {
                break;
            }
            let offer = progress.lock().unwrap().offer.clone();
            if let Some(o) = offer {
                if o.id != previous {
                    approval.send((o.id, true, destination.clone())).unwrap();
                    previous = o.id;
                }
            }
            thread::sleep(Duration::from_millis(1));
        }
    });
    let receiving = thread::spawn(move || engine::receive(&mut receiver, "private peer", &server));
    let result = engine::send(&mut sender, &files, &client);
    done.store(true, Ordering::Relaxed);
    approvals.join().unwrap();
    result?;
    receiving.join().unwrap()?;
    assert_eq!(fs::read(output.join("Ёжик 世界.bin"))?, payload);
    assert_eq!(fs::read(&path)?, payload);
    assert!(fs::read(output.join("empty.txt"))?.is_empty());
    assert_eq!(client.progress.lock().unwrap().completed, 2);
    assert_eq!(fs::read_dir(&output)?.count(), 2);
    Ok(())
}
#[test]
fn source_changes_folders_and_links_are_not_queued_or_replayed() -> io::Result<()> {
    let work = crate::network::workspace::Workspace::create(std::env::temp_dir())?;
    let path = work.root.join("file.txt");
    fs::write(&path, b"first")?;
    assert!(capture(vec![work.root.clone()], &AtomicBool::new(false)).is_err());
    let files = capture(vec![path.clone()], &AtomicBool::new(false))?;
    fs::write(&path, b"changed contents")?;
    let (mut sender, mut receiver) = sockets()?;
    let (client, _) = context();
    let cancelled = client.cancel.clone();
    let peer = thread::spawn(move || {
        let cancel = AtomicBool::new(false);
        let _ = obex::read(&mut receiver, &cancel, obex::MTU)?;
        obex::write(&mut receiver, 0xa0, &[0x10, 0, 0x40, 0], &cancel)
    });
    assert!(engine::send(&mut sender, &files, &client).is_err());
    peer.join().unwrap()?;
    cancelled.store(true, Ordering::Relaxed);
    assert!(capture(vec![path], &cancelled).is_err());
    #[cfg(unix)]
    {
        let link = work.root.join("link");
        std::os::unix::fs::symlink(work.root.join("file.txt"), &link)?;
        assert!(capture(vec![link], &AtomicBool::new(false)).is_err());
    }
    Ok(())
}
fn wait(app: &mut App, ready: impl Fn(&App) -> bool) {
    let end = Instant::now() + Duration::from_secs(5);
    while !ready(app) {
        app.poll();
        assert!(Instant::now() < end, "Bluetooth fixture did not settle");
        thread::sleep(Duration::from_millis(1));
    }
}
#[test]
fn plugin_defaults_on_captures_marks_and_draws_immutable_owned_controls() -> io::Result<()> {
    assert!(crate::config::Config::default().plugin_enabled(BLUETOOTH_TRANSFERS));
    let work = crate::network::workspace::Workspace::create(std::env::temp_dir())?;
    for name in ["first.txt", "Ёжик.txt"] {
        fs::write(work.root.join(name), b"private")?;
    }
    let mut app = App::new(work.root.clone())?;
    app.config = crate::config::Config::default();
    app.config.save_on_exit = false;
    app.config_store = crate::config::Store::new(Some(work.root.join("dn.conf")));
    wait(&mut app, |a| a.panels.iter().all(|p| !p.loading));
    app.config.set_plugin_enabled(BLUETOOTH_TRANSFERS, true);
    app.panels[0].selected = ["first.txt".into(), "Ёжик.txt".into()].into();
    crate::plugins::host::open(&mut app, BLUETOOTH_TRANSFERS);
    wait(&mut app, |a| !a.bluetooth.view.busy);
    assert_eq!(app.bluetooth.queue.len(), 2);
    assert!(app
        .bluetooth
        .queue
        .iter()
        .all(|s| s.path.parent() == Some(work.root.as_path())));
    app.bluetooth.view.devices = vec![Device {
        name: "Private peer Ё".into(),
        address: "01:23:45:67:89:AB".into(),
    }];
    let binding = app.desktop.plugins.binding(BLUETOOTH_TRANSFERS).unwrap();
    for language in [
        crate::localization::Language::English,
        crate::localization::Language::Russian,
    ] {
        app.config.language = language;
        for palette in [
            crate::palette::PaletteProfile::ClassicDn,
            crate::palette::PaletteProfile::Terminal,
        ] {
            app.config.palette = palette;
            for (w, h) in [(80, 25), (120, 35), (40, 12), (80, 25)] {
                for page in [0, 1, 2] {
                    app.bluetooth.view.progress.offer = (page == 1).then(|| Offer {
                        id: 77,
                        name: "Ёжик.txt".into(),
                        size: 12,
                        peer: "Private peer".into(),
                    });
                    app.bluetooth.view.conflict =
                        (page == 2).then(|| crate::operations::conflict::Conflict {
                            source: PathBuf::new(),
                            source_label: Some("Incoming Bluetooth file".into()),
                            destination: work.root.join("first.txt"),
                            destination_label: None,
                            source_size: 12,
                            destination_size: 7,
                            source_modified: None,
                            destination_modified: None,
                        });
                    let area = crate::presentation::layout::Rect::new(0, 0, w, h);
                    let snap = crate::layout::prepare(&mut app, area);
                    let frame = crate::ui::render(&app, &snap);
                    assert!(frame
                        .hit_regions()
                        .iter()
                        .all(|(r, _)| r.right() <= w && r.bottom() <= h));
                    let first = crate::testing::Buffer::rendered(frame);
                    for _ in 0..3 {
                        assert_eq!(
                            first,
                            crate::testing::Buffer::rendered(crate::ui::render(&app, &snap))
                        );
                    }
                }
            }
        }
    }
    app.bluetooth.view.progress.offer = None;
    app.bluetooth.view.conflict = None;
    let generation = app.bluetooth.view.generation;
    start(&mut app, binding, |ctx| {
        while !ctx.cancel.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(1));
        }
        Err(io::ErrorKind::Interrupted.into())
    });
    let cancellation = app
        .bluetooth
        .job
        .as_ref()
        .unwrap()
        .worker
        .cancellation_flag();
    if let Owner::Document(id) = binding.owner {
        app.desktop.close(id);
    }
    app.poll();
    assert!(cancellation.load(Ordering::Relaxed));
    assert!(app.bluetooth.job.is_none());
    crate::plugins::host::open(&mut app, BLUETOOTH_TRANSFERS);
    let new = app.desktop.plugins.binding(BLUETOOTH_TRANSFERS).unwrap();
    assert_ne!(new, binding);
    action(&mut app, binding, generation, Action::Stop);
    wait(&mut app, |a| !a.bluetooth.view.busy);
    assert_eq!(app.desktop.plugins.binding(BLUETOOTH_TRANSFERS), Some(new));
    start(&mut app, new, |ctx| {
        while !ctx.cancel.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(1));
        }
        Err(io::ErrorKind::Interrupted.into())
    });
    let cancellation = app
        .bluetooth
        .job
        .as_ref()
        .unwrap()
        .worker
        .cancellation_flag();
    let generation = app.bluetooth.view.generation;
    action(
        &mut app,
        new,
        generation,
        Action::Conflict(crate::operations::conflict::Decision::Cancel),
    );
    assert!(
        cancellation.load(Ordering::Relaxed),
        "Cancel in a conflict must stop the job, not skip to another file"
    );
    wait(&mut app, |a| !a.bluetooth.view.busy);
    start(&mut app, new, |ctx| {
        ctx.progress.lock().unwrap().completed = 3;
        Ok(Reply::Transferred)
    });
    wait(&mut app, |a| !a.bluetooth.view.busy);
    assert_eq!(
        app.bluetooth.view.progress.completed, 3,
        "Final worker counts must accompany its completed result"
    );
    Ok(())
}
#[test]
fn truncated_incoming_file_never_publishes_and_cleans_private_stage() -> io::Result<()> {
    let work = crate::network::workspace::Workspace::create(std::env::temp_dir())?;
    let (mut sender, mut receiver) = sockets()?;
    let (server, approval) = context();
    let progress = server.progress.clone();
    let root = work.root.clone();
    let approver = thread::spawn(move || {
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            assert!(Instant::now() < end);
            if let Some(o) = progress.lock().unwrap().offer.clone() {
                approval.send((o.id, true, root)).unwrap();
                return;
            }
            thread::sleep(Duration::from_millis(1));
        }
    });
    let receiving = thread::spawn(move || engine::receive(&mut receiver, "private peer", &server));
    let cancel = AtomicBool::new(false);
    obex::write(&mut sender, 0x80, &[0x10, 0, 0x40, 0], &cancel)?;
    obex::response(&mut sender, &cancel, 0xa0)?;
    let mut headers = obex::name_header("partial.txt")?;
    headers.extend([0xc3, 0, 0, 0, 10]);
    obex::write(&mut sender, 0x02, &headers, &cancel)?;
    obex::response(&mut sender, &cancel, 0x90)?;
    obex::write(&mut sender, 0x82, &[0x49, 0, 6, 1, 2, 3], &cancel)?;
    drop(sender);
    approver.join().unwrap();
    assert!(receiving.join().unwrap().is_err());
    assert!(fs::read_dir(&work.root)?.next().is_none());
    Ok(())
}
#[test]
fn lost_final_receipt_is_reported_as_unconfirmed_delivery() -> io::Result<()> {
    let work = crate::network::workspace::Workspace::create(std::env::temp_dir())?;
    let file = work.root.join("tiny.txt");
    fs::write(&file, b"ok")?;
    let files = capture(vec![file], &AtomicBool::new(false))?;
    let (mut sender, mut receiver) = sockets()?;
    let (client, _) = context();
    let peer = thread::spawn(move || {
        let c = AtomicBool::new(false);
        obex::read(&mut receiver, &c, obex::MTU)?;
        obex::write(&mut receiver, 0xa0, &[0x10, 0, 0x40, 0], &c)?;
        obex::read(&mut receiver, &c, obex::MTU)?;
        obex::write(&mut receiver, 0x90, &[], &c)?;
        obex::read(&mut receiver, &c, obex::MTU)?;
        Ok::<_, io::Error>(())
    });
    let error = engine::send(&mut sender, &files, &client).unwrap_err();
    assert!(error.to_string().contains("Delivery unconfirmed"));
    assert_eq!(client.progress.lock().unwrap().completed, 0);
    peer.join().unwrap()?;
    Ok(())
}
#[test]
fn existing_destination_skip_preserves_old_file_and_continues_queue() -> io::Result<()> {
    let work = crate::network::workspace::Workspace::create(std::env::temp_dir())?;
    let input = work.root.join("input");
    let output = work.root.join("output");
    fs::create_dir(&input)?;
    fs::create_dir(&output)?;
    let first = input.join("first.txt");
    let second = input.join("second.txt");
    fs::write(&first, b"new first")?;
    fs::write(&second, b"new second")?;
    fs::write(output.join("first.txt"), b"old first")?;
    let files = capture(vec![first, second], &AtomicBool::new(false))?;
    let (mut sender, mut receiver) = sockets()?;
    let (client, _) = context();
    let (server, approvals) = context();
    let progress = server.progress.clone();
    let conflicts = server.conflicts.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let done = stop.clone();
    let destination = output.clone();
    let monitor = thread::spawn(move || {
        let mut previous = 0;
        let end = Instant::now() + Duration::from_secs(5);
        while !done.load(Ordering::Relaxed) {
            assert!(Instant::now() < end);
            let offer = progress.lock().unwrap().offer.clone();
            if let Some(o) = offer {
                if o.id != previous {
                    approvals.send((o.id, true, destination.clone())).unwrap();
                    previous = o.id;
                }
            }
            if conflicts.pending().is_some() {
                conflicts.answer(crate::operations::conflict::Decision::Skip);
            }
            thread::sleep(Duration::from_millis(1));
        }
    });
    let receiving = thread::spawn(move || engine::receive(&mut receiver, "private peer", &server));
    let result = engine::send(&mut sender, &files, &client);
    stop.store(true, Ordering::Relaxed);
    monitor.join().unwrap();
    result?;
    receiving.join().unwrap()?;
    assert_eq!(fs::read(output.join("first.txt"))?, b"old first");
    assert_eq!(fs::read(output.join("second.txt"))?, b"new second");
    assert_eq!(client.progress.lock().unwrap().completed, 1);
    assert_eq!(client.progress.lock().unwrap().skipped, 1);
    assert_eq!(fs::read_dir(output)?.count(), 2);
    Ok(())
}

// Fail only after the peer could have consumed the final PUT, or after its
// publication receipt. These outcomes must not be confused with no delivery.
struct ScriptedPeer {
    replies: io::Cursor<Vec<u8>>,
    fail_opcode: u8,
    fail_kind: io::ErrorKind,
}
impl io::Read for ScriptedPeer {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        io::Read::read(&mut self.replies, b)
    }
}
impl io::Write for ScriptedPeer {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        if b.first() == Some(&self.fail_opcode) {
            return Err(self.fail_kind.into());
        }
        Ok(b.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
#[test]
fn final_write_failure_is_unconfirmed_but_disconnect_failure_preserves_receipts() -> io::Result<()>
{
    let work = crate::network::workspace::Workspace::create(std::env::temp_dir())?;
    let path = work.root.join("private.txt");
    fs::write(&path, b"ok")?;
    let files = capture(vec![path], &AtomicBool::new(false))?;
    for (opcode, fail_kind) in [
        (0x82, io::ErrorKind::BrokenPipe),
        (0x82, io::ErrorKind::Interrupted),
        (0x81, io::ErrorKind::BrokenPipe),
    ] {
        let (ctx, _) = context();
        let mut peer = ScriptedPeer {
            replies: io::Cursor::new(vec![0xa0, 0, 7, 0x10, 0, 0x40, 0, 0x90, 0, 3, 0xa0, 0, 3]),
            fail_opcode: opcode,
            fail_kind,
        };
        let result = engine::send(&mut peer, &files, &ctx);
        if opcode == 0x82 {
            let error = result.unwrap_err();
            assert_eq!(error.kind(), fail_kind);
            assert!(error.to_string().contains("Delivery unconfirmed"));
            assert_eq!(ctx.progress.lock().unwrap().completed, 0);
        } else {
            result?;
            assert_eq!(ctx.progress.lock().unwrap().completed, 1);
            assert!(!ctx.progress.lock().unwrap().warning.is_empty());
        }
    }
    Ok(())
}

#[test]
fn disconnected_worker_clears_pending_controls_and_rejects_old_actions() -> io::Result<()> {
    let work = crate::network::workspace::Workspace::create(std::env::temp_dir())?;
    let mut app = App::new(work.root.clone())?;
    app.config = crate::config::Config::default();
    app.config.save_on_exit = false;
    app.config_store = crate::config::Store::new(Some(work.root.join("dn.conf")));
    app.config.set_plugin_enabled(BLUETOOTH_TRANSFERS, true);
    crate::plugins::host::open(&mut app, BLUETOOTH_TRANSFERS);
    wait(&mut app, |a| !a.bluetooth.view.busy);
    let owner = app.desktop.plugins.binding(BLUETOOTH_TRANSFERS).unwrap();
    let generation = app.bluetooth.view.generation;
    start(&mut app, owner, |ctx| {
        ctx.progress.lock().unwrap().offer = Some(Offer {
            id: 1,
            name: "private.txt".into(),
            size: 0,
            peer: "peer".into(),
        });
        panic!("intentional Bluetooth worker failure");
    });
    wait(&mut app, |a| !a.bluetooth.view.busy);
    assert!(!app.bluetooth.view.busy);
    assert!(app.bluetooth.view.progress.offer.is_none());
    assert!(app.bluetooth.view.conflict.is_none());
    assert_ne!(app.bluetooth.view.generation, generation);
    Ok(())
}

#[test]
fn defaults_upgrade_and_explicit_disable_persist_without_starting_radio() -> io::Result<()> {
    let work = crate::network::workspace::Workspace::create(std::env::temp_dir())?;
    let path = work.root.join("private.conf");
    for bytes in [
        b"config_version=2\n".as_slice(),
        b"config_version=2\n[plugins]\ndisabled=[]\n",
    ] {
        let mut config = crate::config::Config::parse(bytes)?;
        assert!(config.plugin_enabled(BLUETOOTH_TRANSFERS));
        assert!(!config.plugin_enabled(crate::plugins::ACOUSTIC_TRANSFERS));
        config.set_plugin_enabled(BLUETOOTH_TRANSFERS, false);
        config.save(&path)?;
        let mut loaded = crate::config::Config::read(&path)?;
        assert!(!loaded.plugin_enabled(BLUETOOTH_TRANSFERS));
        loaded.set_plugin_enabled(BLUETOOTH_TRANSFERS, true);
        loaded.save(&path)?;
        assert!(crate::config::Config::read(&path)?.plugin_enabled(BLUETOOTH_TRANSFERS));
    }
    let disabled = crate::config::Config::parse(
        b"config_version=2\n[plugins]\ndisabled=['bluetooth_transfers']\n",
    )?;
    assert!(!disabled.plugin_enabled(BLUETOOTH_TRANSFERS));
    let app = App::new(work.root.clone())?;
    assert!(app.bluetooth.job.is_none());
    assert!(!app.bluetooth.view.busy);
    assert!(app.bluetooth.view.devices.is_empty());
    Ok(())
}

#[test]
fn retired_cleanup_is_tracked_through_quit_and_cannot_update_reopened_bluetooth() -> io::Result<()>
{
    let work = crate::network::workspace::Workspace::create(std::env::temp_dir())?;
    let mut app = App::new(work.root.clone())?;
    app.config = crate::config::Config::default();
    app.config.save_on_exit = false;
    app.config_store = crate::config::Store::new(Some(work.root.join("config.conf")));
    crate::plugins::host::open(&mut app, BLUETOOTH_TRANSFERS);
    wait(&mut app, |a| !a.bluetooth.view.busy);
    let owner = app.desktop.plugins.binding(BLUETOOTH_TRANSFERS).unwrap();
    let (started, ready) = mpsc::sync_channel(1);
    let (release, gate) = mpsc::sync_channel(1);
    let target = work.root.join("never-published.txt");
    let output = target.clone();
    start(&mut app, owner, move |ctx| {
        let mut stage = crate::publication::StagedFile::create(&output)?;
        io::Write::write_all(&mut stage.file, b"private partial data")?;
        started.send(stage.path().to_path_buf()).unwrap();
        gate.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(ctx.cancel.load(Ordering::Relaxed));
        drop(stage);
        Ok(Reply::Transferred)
    });
    let staging = ready.recv_timeout(Duration::from_secs(5)).unwrap();
    let Owner::Document(id) = owner.owner else {
        panic!("document");
    };
    app.desktop.close(id);
    app.poll();
    assert!(!app.bluetooth.view.busy);
    assert!(app.workers.pending());
    assert!(staging.exists());
    crate::plugins::host::open(&mut app, BLUETOOTH_TRANSFERS);
    let fresh = app.desktop.active().id;
    assert_ne!(fresh, id);
    let notice = app.bluetooth.view.notice.clone();
    app.request_exit();
    assert!(!app.quit && app.exit_pending);
    assert!(app.wait_duration(Instant::now()) <= Duration::from_millis(50));
    app.abort_quit();
    assert!(!app.quit && !app.exit_pending);
    app.request_exit();
    assert!(!app.quit);
    release.send(()).unwrap();
    // Another admission/poll may already have reaped cleanup. Quit must not
    // depend on observing that exact transition in App::poll.
    let end = Instant::now() + Duration::from_secs(5);
    while app.workers.pending() {
        app.workers.poll();
        assert!(Instant::now() < end);
        thread::sleep(Duration::from_millis(1));
    }
    wait(&mut app, |a| a.quit);
    assert!(!app.workers.pending());
    assert!(!staging.exists() && !target.exists());
    assert_eq!(app.desktop.active().id, fresh);
    assert_eq!(app.bluetooth.view.notice, notice);
    Ok(())
}

fn transfer_app(root: PathBuf) -> io::Result<(App, Binding)> {
    let mut app = App::new(root.clone())?;
    app.config = crate::config::Config::default();
    app.config.save_on_exit = false;
    app.config_store = crate::config::Store::new(Some(root.join("dn.conf")));
    wait(&mut app, |a| a.panels.iter().all(|p| !p.loading));
    crate::plugins::host::open(&mut app, BLUETOOTH_TRANSFERS);
    wait(&mut app, |a| !a.bluetooth.view.busy);
    let owner = app.desktop.plugins.binding(BLUETOOTH_TRANSFERS).unwrap();
    Ok((app, owner))
}
fn receive_socket(app: &mut App, owner: Binding, mut receiver: TcpStream) -> io::Result<()> {
    let job = make_job(app, owner, true, true, move |ctx| {
        engine::receive(&mut receiver, "private peer", &ctx)?;
        Ok(Reply::Transferred)
    })?;
    app.bluetooth.job = Some(job);
    app.bluetooth.view.busy = true;
    Ok(())
}
fn accept_current(app: &mut App, owner: Binding, destination: PathBuf) -> u64 {
    wait(app, |a| a.bluetooth.view.progress.offer.is_some());
    let offer = app.bluetooth.view.progress.offer.as_ref().unwrap().id;
    action(
        app,
        owner,
        app.bluetooth.view.generation,
        Action::Approve {
            offer,
            accept: true,
            destination,
        },
    );
    offer
}
#[test]
fn accepted_receiver_waits_for_queue_then_survives_close_without_accepting_more_files(
) -> io::Result<()> {
    use crate::transfers::{self, Status};
    let work = crate::network::workspace::Workspace::create(std::env::temp_dir())?;
    let input = work.root.join("input");
    let output = work.root.join("output");
    fs::create_dir(&input)?;
    fs::create_dir(&output)?;
    let first = input.join("Ёжик.txt");
    let next = input.join("not-approved.txt");
    fs::write(&first, b"private data")?;
    fs::write(&next, b"not approved")?;
    let files = capture(vec![first.clone(), next], &AtomicBool::new(false))?;
    let (mut app, owner) = transfer_app(work.root.clone())?;
    app.transfers.paused = true;
    let filesystem = transfers::submit(
        &mut app,
        crate::operations::Plan {
            kind: crate::operations::Kind::Copy,
            sources: vec![first.clone()],
            destination: work.root.clone(),
        },
        Default::default(),
    )
    .unwrap();
    let (mut sender, receiver) = sockets()?;
    receive_socket(&mut app, owner, receiver)?;
    let (ctx, _) = context();
    let sent = ctx.progress.clone();
    let sending = thread::spawn(move || engine::send(&mut sender, &files, &ctx));
    let offer = accept_current(&mut app, owner, output.clone());
    let id = app.transfers.bluetooth_pending().unwrap();
    assert!(app
        .transfers
        .bluetooth
        .as_ref()
        .unwrap()
        .answer(offer, true, output.clone())
        .is_err());
    assert!(app.bluetooth.job.is_none());
    app.poll();
    assert_eq!(
        app.transfers
            .entries
            .iter()
            .find(|e| e.id == id)
            .unwrap()
            .status,
        Status::Queued
    );
    assert_eq!(fs::read_dir(&output)?.count(), 0);
    let Owner::Document(old) = owner.owner else {
        unreachable!()
    };
    app.desktop.close(old);
    app.poll();
    crate::plugins::host::open(&mut app, BLUETOOTH_TRANSFERS);
    let fresh = app.desktop.active().id;
    assert_ne!(fresh, old);
    let current = app.desktop.plugins.binding(BLUETOOTH_TRANSFERS).unwrap();
    action(&mut app, owner, 0, Action::Stop); // Retired window cannot cancel accepted work.
    assert!(!app
        .transfers
        .bluetooth
        .as_ref()
        .unwrap()
        .inner
        .worker
        .cancellation_flag()
        .load(Ordering::Relaxed));
    transfers::action(&mut app, transfers::Action::Resume);
    let end = Instant::now() + Duration::from_secs(5);
    while !app
        .transfers
        .entries
        .iter()
        .find(|e| e.id == id)
        .unwrap()
        .status
        .terminal()
    {
        app.poll();
        if output.join("Ёжик.txt").exists() {
            assert_eq!(
                app.transfers
                    .entries
                    .iter()
                    .find(|e| e.id == filesystem)
                    .unwrap()
                    .status,
                Status::Completed
            );
        }
        assert!(Instant::now() < end, "accepted receiver did not finish");
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        app.transfers
            .entries
            .iter()
            .find(|e| e.id == id)
            .unwrap()
            .status,
        Status::Completed
    );
    assert_eq!(fs::read(output.join("Ёжик.txt"))?, b"private data");
    assert!(!output.join("not-approved.txt").exists());
    let _ = sending.join().unwrap();
    assert_eq!(sent.lock().unwrap().completed, 1);
    assert_eq!(app.desktop.active().id, fresh);
    assert_eq!(
        app.desktop.plugins.binding(BLUETOOTH_TRANSFERS),
        Some(current)
    );
    Ok(())
}
#[test]
fn accepted_receiver_replacement_prompt_is_owned_by_transfer_center_after_window_close(
) -> io::Result<()> {
    use crate::{
        operations::conflict::Decision,
        transfers::{self, Status},
    };
    let work = crate::network::workspace::Workspace::create(std::env::temp_dir())?;
    let input = work.root.join("input");
    let output = work.root.join("output");
    fs::create_dir(&input)?;
    fs::create_dir(&output)?;
    fs::write(input.join("file.txt"), b"new")?;
    fs::write(output.join("file.txt"), b"old")?;
    let files = capture(vec![input.join("file.txt")], &AtomicBool::new(false))?;
    let (mut app, owner) = transfer_app(work.root.clone())?;
    let (mut sender, receiver) = sockets()?;
    receive_socket(&mut app, owner, receiver)?;
    let (ctx, _) = context();
    let sending = thread::spawn(move || engine::send(&mut sender, &files, &ctx));
    accept_current(&mut app, owner, output.clone());
    let id = app.transfers.bluetooth_pending().unwrap();
    let Owner::Document(old) = owner.owner else {
        unreachable!()
    };
    app.desktop.close(old);
    wait(&mut app, |a| a.transfers.conflict_prompt.is_some());
    assert_eq!(fs::read(output.join("file.txt"))?, b"old");
    assert!(crate::transfers::conflict_dialog(&app));
    transfers::action(&mut app, transfers::Action::Conflict(id, Decision::Replace));
    wait(&mut app, |a| {
        a.transfers
            .entries
            .iter()
            .any(|e| e.id == id && e.status.terminal())
    });
    sending.join().unwrap()?;
    assert_eq!(
        app.transfers
            .entries
            .iter()
            .find(|e| e.id == id)
            .unwrap()
            .status,
        Status::Completed
    );
    assert_eq!(fs::read(output.join("file.txt"))?, b"new");
    assert!(app.transfers.conflict_prompt.is_none());
    assert!(app.bluetooth.view.progress.offer.is_none());
    Ok(())
}
#[test]
fn queued_bluetooth_cancellation_waits_for_cleanup_and_terminal_outcomes_are_typed(
) -> io::Result<()> {
    use crate::transfers::{self, Status};
    for expected in [
        Status::Cancelled,
        Status::Unconfirmed,
        Status::Skipped,
        Status::Failed,
    ] {
        let work = crate::network::workspace::Workspace::create(std::env::temp_dir())?;
        let (mut app, owner) = transfer_app(work.root.clone())?;
        app.transfers.paused = true;
        let request = transfer::Transfer {
            direction: transfer::Direction::Send,
            device: Device {
                name: "private peer".into(),
                address: "01:23:45:67:89:AB".into(),
            },
            files: vec![],
            name: String::new(),
            destination: PathBuf::new(),
        };
        let id = transfers::submit_bluetooth(&mut app, request, false).unwrap();
        let (release, gate) = mpsc::sync_channel(1);
        let (started, ready) = mpsc::sync_channel(1);
        let output = work.root.join("never-published.txt");
        let target = output.clone();
        let job = make_job(&mut app, owner, true, false, move |ctx| {
            let stage = if expected == Status::Cancelled {
                Some(crate::publication::StagedFile::create(&target)?)
            } else {
                None
            };
            started
                .send(stage.as_ref().map(|s| s.path().to_path_buf()))
                .map_err(io::Error::other)?;
            gate.recv_timeout(Duration::from_secs(5))
                .map_err(io::Error::other)?;
            if expected == Status::Cancelled {
                assert!(ctx.cancel.load(Ordering::Relaxed));
                return Err(io::ErrorKind::Interrupted.into());
            }
            if expected == Status::Unconfirmed {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    transfer::Unconfirmed(io::ErrorKind::BrokenPipe.into()),
                ));
            }
            if expected == Status::Failed {
                return Err(io::ErrorKind::PermissionDenied.into());
            }
            ctx.progress.lock().unwrap().skipped = 2;
            Ok(Reply::Transferred)
        })?;
        app.transfers.bluetooth = Some(transfer::Job::adopt(id, job, None));
        let staging = ready
            .recv_timeout(Duration::from_secs(5))
            .map_err(io::Error::other)?;
        if expected == Status::Cancelled {
            transfers::action(&mut app, transfers::Action::Cancel);
            assert_eq!(app.transfers.entries[0].status, Status::Cancelling);
            assert!(app.transfers.has_work());
            assert!(staging.as_ref().unwrap().exists());
        } else {
            transfers::action(&mut app, transfers::Action::Resume);
        }
        release.send(()).unwrap();
        wait(&mut app, |a| a.transfers.entries[0].status.terminal());
        assert_eq!(app.transfers.entries[0].status, expected);
        assert!(!app.transfers.has_work());
        assert!(app.transfers.bluetooth.is_none());
        assert!(!output.exists());
        if let Some(staging) = staging {
            assert!(!staging.exists());
        }
        if expected == Status::Unconfirmed {
            assert!(app.transfers.entries[0]
                .error
                .as_ref()
                .unwrap()
                .contains("Delivery unconfirmed"));
        }
    }
    Ok(())
}

#[test]
fn closing_receiver_while_idle_finishes_accepted_job_without_waiting_for_another_packet(
) -> io::Result<()> {
    use std::io::{Read, Write};
    struct HoldDisconnect {
        socket: TcpStream,
        reached: mpsc::SyncSender<()>,
        resume: mpsc::Receiver<()>,
    }
    impl Read for HoldDisconnect {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            self.socket.read(bytes)
        }
    }
    impl Write for HoldDisconnect {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if bytes.first() == Some(&0x81) {
                self.reached.send(()).map_err(io::Error::other)?;
                self.resume
                    .recv_timeout(Duration::from_secs(5))
                    .map_err(io::Error::other)?;
            }
            self.socket.write(bytes)
        }
        fn flush(&mut self) -> io::Result<()> {
            self.socket.flush()
        }
    }
    let work = crate::network::workspace::Workspace::create(std::env::temp_dir())?;
    let input = work.root.join("private.txt");
    let output = work.root.join("output");
    fs::write(&input, b"private")?;
    fs::create_dir(&output)?;
    let files = capture(vec![input], &AtomicBool::new(false))?;
    let (mut app, owner) = transfer_app(work.root.clone())?;
    let (sender, receiver) = sockets()?;
    receive_socket(&mut app, owner, receiver)?;
    let (reached, waiting) = mpsc::sync_channel(1);
    let (resume, gate) = mpsc::sync_channel(1);
    let (ctx, _) = context();
    let sending = thread::spawn(move || {
        engine::send(
            &mut HoldDisconnect {
                socket: sender,
                reached,
                resume: gate,
            },
            &files,
            &ctx,
        )
    });
    accept_current(&mut app, owner, output.clone());
    wait(&mut app, |a| a.bluetooth.view.progress.completed == 1);
    waiting
        .recv_timeout(Duration::from_secs(5))
        .map_err(io::Error::other)?;
    let id = app.transfers.bluetooth_pending().unwrap();
    let Owner::Document(window) = owner.owner else {
        unreachable!()
    };
    app.desktop.close(window);
    wait(&mut app, |a| {
        a.transfers
            .entries
            .iter()
            .any(|e| e.id == id && e.status.terminal())
    });
    assert_eq!(
        app.transfers.entries[0].status,
        crate::transfers::Status::Completed
    );
    assert_eq!(app.transfers.entries[0].progress.files, 1);
    assert_eq!(fs::read(output.join("private.txt"))?, b"private");
    resume.send(()).unwrap();
    sending.join().unwrap()?;
    Ok(())
}

#[test]
fn bluetooth_all_choices_cover_subsequent_explicitly_approved_files() -> io::Result<()> {
    use crate::{
        operations::conflict::Decision,
        transfers::{self, Status},
    };
    for decision in [Decision::ReplaceAll, Decision::SkipAll] {
        let work = crate::network::workspace::Workspace::create(std::env::temp_dir())?;
        let input = work.root.join("input");
        let output = work.root.join("output");
        fs::create_dir(&input)?;
        fs::create_dir(&output)?;
        let mut paths = vec![];
        for name in ["first.txt", "second.txt"] {
            let path = input.join(name);
            fs::write(&path, b"new")?;
            fs::write(output.join(name), b"old")?;
            paths.push(path);
        }
        let files = capture(paths, &AtomicBool::new(false))?;
        let (mut app, owner) = transfer_app(work.root.clone())?;
        let (mut sender, receiver) = sockets()?;
        receive_socket(&mut app, owner, receiver)?;
        let (ctx, _) = context();
        let sending = thread::spawn(move || engine::send(&mut sender, &files, &ctx));
        accept_current(&mut app, owner, output.clone());
        let id = app.transfers.bluetooth_pending().unwrap();
        transfers::open(&mut app);
        wait(&mut app, |a| a.transfers.conflict_prompt.is_some());
        transfers::action(&mut app, transfers::Action::Conflict(id, decision));
        wait(&mut app, |a| {
            a.bluetooth
                .view
                .progress
                .offer
                .as_ref()
                .is_some_and(|o| o.name == "second.txt")
        });
        // Approval remains explicit even when replacement was approved for all.
        assert_eq!(fs::read(output.join("second.txt"))?, b"old");
        accept_current(&mut app, owner, output.clone());
        wait(&mut app, |a| {
            a.transfers
                .entries
                .iter()
                .any(|e| e.id == id && e.status.terminal())
        });
        sending.join().unwrap()?;
        assert_eq!(
            app.transfers.entries[0].status,
            if decision == Decision::ReplaceAll {
                Status::Completed
            } else {
                Status::Skipped
            }
        );
        assert_eq!(
            app.transfers.entries[0].progress.files,
            if decision == Decision::ReplaceAll {
                2
            } else {
                0
            }
        );
        assert_eq!(
            app.transfers.entries[0].progress.skipped,
            if decision == Decision::SkipAll { 2 } else { 0 }
        );
        assert!(app.transfers.conflict_prompt.is_none());
        for name in ["first.txt", "second.txt"] {
            assert_eq!(
                fs::read(output.join(name))?,
                if decision == Decision::ReplaceAll {
                    b"new"
                } else {
                    b"old"
                }
            );
        }
    }
    Ok(())
}
