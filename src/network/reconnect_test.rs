//! Private state-machine fixtures: no external servers or audio devices.
use super::{
    remote::*,
    status::*,
    tests::{profile, Memory},
    ReconnectTarget,
};
use std::{
    io::{self, Read, Write},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
#[derive(Clone)]
struct Failing {
    memory: Memory,
    mode: Arc<AtomicUsize>,
}
impl Backend for Failing {
    fn list(&mut self, p: &RemotePath, c: &crate::task::Cancellation) -> io::Result<Vec<Member>> {
        match self.mode.load(Ordering::Relaxed) {
            1 => Err(transport(io::Error::new(
                io::ErrorKind::ConnectionReset,
                "private fixture connection lost",
            ))),
            2 => Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "private directory access denied",
            )),
            3 => Err(io::Error::new(
                io::ErrorKind::NotFound,
                "private directory missing",
            )),
            4 => {
                while !c.cancelled() {
                    std::thread::sleep(Duration::from_millis(1));
                }
                c.check()?;
                unreachable!()
            }
            _ => self.memory.list(p, c),
        }
    }
    fn read(
        &mut self,
        p: &RemotePath,
        w: &mut dyn Write,
        c: &crate::task::Cancellation,
        n: u64,
    ) -> io::Result<u64> {
        self.memory.read(p, w, c, n)
    }
    fn write_new(
        &mut self,
        p: &RemotePath,
        r: &mut dyn Read,
        c: &crate::task::Cancellation,
    ) -> io::Result<u64> {
        self.memory.write_new(p, r, c)
    }
    fn mkdir(&mut self, p: &RemotePath) -> io::Result<()> {
        self.memory.mkdir(p)
    }
    fn publish(&mut self, a: &RemotePath, b: &RemotePath, r: bool) -> io::Result<()> {
        self.memory.publish(a, b, r)
    }
    fn remove(&mut self, p: &RemotePath, d: bool) -> io::Result<()> {
        self.memory.remove(p, d)
    }
    fn rename(&mut self, a: &RemotePath, b: &RemotePath) -> io::Result<()> {
        self.memory.rename(a, b)
    }
}
fn wait(app: &mut crate::app::App, done: impl Fn(&crate::app::App) -> bool) {
    let end = Instant::now() + Duration::from_secs(5);
    while !done(app) {
        app.poll();
        assert!(Instant::now() < end, "private remote task did not finish");
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn fixture() -> io::Result<(super::workspace::Workspace, crate::app::App, Failing)> {
    let work = super::workspace::Workspace::create(std::env::temp_dir())?;
    std::fs::write(work.root.join("local.txt"), b"private local")?;
    let mut app = crate::app::App::new(work.root.clone())?;
    app.config.save_on_exit = false;
    app.config_store = crate::config::Store::new(Some(work.root.join("config.conf")));
    wait(&mut app, |a| a.panels.iter().all(|p| !p.loading));
    let memory = Memory::default();
    memory.put("/folder", None);
    memory.put("/file.txt", Some(b"data"));
    memory.put("/other.bin", Some(b"other"));
    Ok((
        work,
        app,
        Failing {
            memory,
            mode: Arc::new(AtomicUsize::new(0)),
        },
    ))
}
fn target(app: &crate::app::App, index: usize) -> ReconnectTarget {
    let r = app.panels[index].remote().unwrap();
    ReconnectTarget {
        panel: index,
        generation: r.generation,
        connection: r.connection.clone(),
        reason: r.issue.clone().or_else(|| r.status.error.clone()),
    }
}
#[test]
fn first_listing_failure_keeps_remote_location_and_explicit_disconnect_restores_local(
) -> io::Result<()> {
    let (work, mut app, f) = fixture()?;
    f.mode.store(1, Ordering::Relaxed);
    app.panels[0].selected = ["local.txt".into()].into();
    app.panels[0].open_remote(Connection::test(profile(), f), true)?;
    wait(&mut app, |a| !a.panels[0].loading);
    let r = app.panels[0]
        .remote()
        .expect("failed first listing must retain remote pane");
    assert_eq!(r.directory.as_str(), "/");
    assert_eq!(r.status.phase, Phase::Disconnected);
    assert_eq!(app.panels[0].path, work.root);
    assert_eq!(app.panels[0].entries.len(), 1);
    assert!(app.panels[0]
        .remote_error_report(crate::localization::Language::English)
        .contains("sftp://"));
    app.panels[0].leave_remote(true);
    wait(&mut app, |a| !a.panels[0].loading);
    assert!(!app.panels[0].is_remote());
    assert!(app.panels[0]
        .selected
        .contains(std::ffi::OsStr::new("local.txt")));
    assert_eq!(
        std::fs::read(work.root.join("local.txt"))?,
        b"private local"
    );
    Ok(())
}
#[test]
fn reconnect_keeps_filter_cursor_marks_and_targets_only_the_captured_pane() -> io::Result<()> {
    let (_work, mut app, f) = fixture()?;
    app.panels[0].open_remote(Connection::test(profile(), f.clone()), true)?;
    wait(&mut app, |a| !a.panels[0].loading);
    app.panels[0].set_filter("*.txt".into());
    app.panels[0].cursor = app.panels[0]
        .entries
        .iter()
        .position(|e| e.name == "file.txt")
        .unwrap();
    app.panels[0].selected = ["file.txt".into()].into();
    let cursor = app.panels[0].entries[app.panels[0].cursor].name.clone();
    f.mode.store(2, Ordering::Relaxed);
    app.panels[0].refresh(true);
    wait(&mut app, |a| !a.panels[0].loading);
    assert_eq!(
        app.panels[0].remote().unwrap().status.phase,
        Phase::Ready,
        "permissions do not mean disconnection"
    );
    assert_eq!(app.panels[0].filter, "*.txt");
    assert_eq!(app.panels[0].entries[app.panels[0].cursor].name, cursor);
    let selected = app.panels[0].selected.clone();
    let other = app.panels[1].path.clone();
    let captured = target(&app, 0);
    let stale = captured.clone();
    app.modals.pop_message(app.desktop.active().id);
    app.sync_modals();
    app.active = 1;
    super::reconnected(
        &mut app,
        captured,
        Connection::test(profile(), f.memory.clone()),
    )?;
    wait(&mut app, |a| !a.panels[0].loading);
    assert_eq!(app.active, 1);
    assert_eq!(app.panels[1].path, other);
    assert_eq!(app.panels[0].filter, "*.txt");
    assert_eq!(app.panels[0].selected, selected);
    assert_eq!(app.panels[0].entries[app.panels[0].cursor].name, cursor);
    assert!(super::reconnected(&mut app, stale, Connection::test(profile(), f.memory)).is_err());
    Ok(())
}
#[test]
fn cancelled_list_and_background_failures_retain_view_and_never_steal_focus() -> io::Result<()> {
    let (_work, mut app, f) = fixture()?;
    app.panels[0].open_remote(Connection::test(profile(), f.clone()), true)?;
    wait(&mut app, |a| !a.panels[0].loading);
    let entries = app.panels[0]
        .entries
        .iter()
        .map(|e| e.name.clone())
        .collect::<Vec<_>>();
    f.mode.store(4, Ordering::Relaxed);
    app.panels[0].refresh(true);
    app.panels[0].remote_cancel();
    assert!(app.panels[0].is_remote());
    assert!(!app.panels[0].loading);
    assert_eq!(
        app.panels[0]
            .entries
            .iter()
            .map(|e| e.name.clone())
            .collect::<Vec<_>>(),
        entries
    );
    wait(&mut app, |a| {
        !a.panels[0].remote().unwrap().connection.busy()
    });
    f.mode.store(1, Ordering::Relaxed);
    app.show_viewer(crate::viewer::Viewer::from_contents(
        "private.txt".into(),
        crate::viewer::from_bytes(b"private".to_vec(), crate::viewer::ViewMode::Text),
    ));
    let owner = app.desktop.active().id;
    app.panels[0].refresh(true);
    wait(&mut app, |a| !a.panels[0].loading);
    assert_eq!(app.desktop.active().id, owner);
    assert!(app.modals.last_message(owner).is_none());
    assert!(app.panels[0].is_remote());
    Ok(())
}
#[test]
fn transport_classification_never_treats_local_write_failures_or_permissions_as_disconnect(
) -> io::Result<()> {
    let c = Connection::test(profile(), Memory::default());
    for kind in [
        io::ErrorKind::StorageFull,
        io::ErrorKind::PermissionDenied,
        io::ErrorKind::BrokenPipe,
        io::ErrorKind::NotFound,
    ] {
        let _: io::Result<()> = c.with(|_| Err(io::Error::from(kind)));
        assert_eq!(c.snapshot().phase, Phase::Ready);
    }
    let _: io::Result<()> =
        c.with(|_| Err(transport(io::Error::from(io::ErrorKind::ConnectionReset))));
    assert_eq!(c.snapshot().phase, Phase::Disconnected);
    let _: io::Result<()> = c.with(|_| panic!("offline backend must not be reused"));
    Ok(())
}
#[test]
fn reconnect_dialog_is_owned_readonly_except_credentials_and_cancel_preserves_session(
) -> io::Result<()> {
    let (_work, mut app, f) = fixture()?;
    let conn = Connection::test(profile(), f);
    app.panels[0].open_remote(conn.clone(), true)?;
    wait(&mut app, |a| !a.panels[0].loading);
    app.panels[0].selected = ["file.txt".into()].into();
    super::reconnect(&mut app);
    let binding = app
        .desktop
        .plugins
        .binding(crate::plugins::REMOTE_CONNECTIONS)
        .expect("owned reconnect dialog");
    assert!(matches!(binding.owner, crate::plugins::Owner::Modal(_)));
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
            for (w, h) in [(80, 25), (110, 35), (40, 12), (80, 25)] {
                let snap = crate::layout::prepare(
                    &mut app,
                    crate::presentation::layout::Rect::new(0, 0, w, h),
                );
                let render = crate::ui::render(&app, &snap);
                if w >= 80 {
                    let regions = render.hit_regions();
                    assert!(regions.iter().any(|(_, t)| *t
                        == crate::mouse::Target::Plugin(
                            crate::plugins::REMOTE_CONNECTIONS,
                            crate::plugins::ActionId(106)
                        )));
                    assert!(!regions.iter().any(|(_, t)| matches!(
                        t,
                        crate::mouse::Target::Plugin(
                            crate::plugins::REMOTE_CONNECTIONS,
                            crate::plugins::ActionId(100..=105)
                        )
                    )));
                }
                let frame = crate::testing::Buffer::rendered(render);
                for _ in 0..3 {
                    assert_eq!(
                        frame,
                        crate::testing::Buffer::rendered(crate::ui::render(&app, &snap))
                    );
                }
            }
        }
    }
    crate::input::handle(
        &mut app,
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
        ),
    );
    assert!(app
        .desktop
        .plugins
        .binding(crate::plugins::REMOTE_CONNECTIONS)
        .is_none());
    assert!(Arc::ptr_eq(
        &app.panels[0].remote().unwrap().connection,
        &conn
    ));
    assert!(app.panels[0]
        .selected
        .contains(std::ffi::OsStr::new("file.txt")));
    Ok(())
}
#[test]
fn lost_pane_and_different_server_never_accept_a_reconnect_result() -> io::Result<()> {
    let (_work, mut app, f) = fixture()?;
    app.panels[0].open_remote(Connection::test(profile(), f.memory.clone()), true)?;
    wait(&mut app, |a| !a.panels[0].loading);
    let captured = target(&app, 0);
    let mut p = profile();
    p.host = "different.invalid".into();
    assert!(super::reconnected(
        &mut app,
        captured.clone(),
        Connection::test(p, f.memory.clone())
    )
    .is_err());
    app.panels[0].leave_remote(true);
    wait(&mut app, |a| !a.panels[0].loading);
    assert!(super::reconnected(&mut app, captured, Connection::test(profile(), f.memory)).is_err());
    assert!(!app.panels[0].is_remote());
    Ok(())
}
