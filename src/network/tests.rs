pub(super) use super::*;
pub(super) use remote::{Backend, Connection, Member, Profile, Protocol, RemotePath};
pub(super) use std::{
    collections::BTreeMap,
    io::{self, Read, Write},
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
#[derive(Clone, Default)]
pub(super) struct Memory(pub(super) Arc<Mutex<BTreeMap<RemotePath, Option<Vec<u8>>>>>);
impl Memory {
    pub(super) fn put(&self, path: &str, data: Option<&[u8]>) {
        self.0
            .lock()
            .unwrap()
            .insert(RemotePath::new(path).unwrap(), data.map(Vec::from));
    }
    fn member(path: &RemotePath, data: &Option<Vec<u8>>) -> Member {
        Member {
            path: path.clone(),
            regular: data.is_some(),
            directory: data.is_none(),
            link: false,
            size: data.as_ref().map(|d| d.len() as u64),
            modified: None,
        }
    }
}
impl Backend for Memory {
    fn list(
        &mut self,
        path: &RemotePath,
        cancel: &crate::task::Cancellation,
    ) -> io::Result<Vec<Member>> {
        cancel.check()?;
        Ok(self
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p.as_str() != path.as_str() && p.parent() == *path)
            .map(|(p, d)| Self::member(p, d))
            .collect())
    }
    fn read(
        &mut self,
        path: &RemotePath,
        w: &mut dyn Write,
        cancel: &crate::task::Cancellation,
        limit: u64,
    ) -> io::Result<u64> {
        let data = self
            .0
            .lock()
            .unwrap()
            .get(path)
            .cloned()
            .flatten()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        remote::copy_bounded(&mut data.as_slice(), w, cancel, limit)
    }
    fn write_new(
        &mut self,
        path: &RemotePath,
        r: &mut dyn Read,
        cancel: &crate::task::Cancellation,
    ) -> io::Result<u64> {
        let mut data = Vec::new();
        let n = remote::copy_bounded(r, &mut data, cancel, 1024 * 1024)?;
        let mut entries = self.0.lock().unwrap();
        if entries.contains_key(path) {
            return Err(io::Error::from(io::ErrorKind::AlreadyExists));
        }
        entries.insert(path.clone(), Some(data));
        Ok(n)
    }
    fn mkdir(&mut self, path: &RemotePath) -> io::Result<()> {
        let mut entries = self.0.lock().unwrap();
        if entries.contains_key(path) {
            return Err(io::Error::from(io::ErrorKind::AlreadyExists));
        }
        entries.insert(path.clone(), None);
        Ok(())
    }
    fn remove(&mut self, path: &RemotePath, _directory: bool) -> io::Result<()> {
        self.0
            .lock()
            .unwrap()
            .remove(path)
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        Ok(())
    }
    fn rename(&mut self, from: &RemotePath, to: &RemotePath) -> io::Result<()> {
        self.publish(from, to, false)
    }
    fn publish(&mut self, from: &RemotePath, to: &RemotePath, replace: bool) -> io::Result<()> {
        let mut entries = self.0.lock().unwrap();
        if !replace && entries.contains_key(to) {
            return Err(io::Error::from(io::ErrorKind::AlreadyExists));
        }
        let value = entries
            .remove(from)
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        entries.insert(to.clone(), value);
        Ok(())
    }
}
pub(super) fn profile() -> Profile {
    Profile {
        name: "Fixture".into(),
        protocol: Protocol::Sftp,
        host: "localhost".into(),
        port: 22,
        user: "tester".into(),
        directory: RemotePath::new("/").unwrap(),
        key: None,
        fingerprint: None,
    }
}
struct Temp(PathBuf);
impl Temp {
    fn new() -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "dn-network-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&p)?;
        Ok(Self(std::fs::canonicalize(p)?))
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn pump(app: &mut crate::app::App, done: impl Fn(&crate::app::App) -> bool) {
    let end = Instant::now() + Duration::from_secs(8);
    loop {
        app.poll();
        if done(app) {
            break;
        }
        assert!(Instant::now() < end, "network fixture timed out");
        std::thread::sleep(Duration::from_millis(1));
    }
}
#[test]
fn remote_paths_and_profiles_never_accept_command_injection_or_store_secrets() -> io::Result<()> {
    for s in ["relative", "/x\ny", "/x\ry", "/x\\y"] {
        assert!(RemotePath::new(s).is_err());
    }
    let root = RemotePath::new("/a/../資料")?;
    assert_eq!(root.as_str(), "/資料");
    assert!(root.child("../escape").is_err());
    assert!(root.child("bad\0name").is_err());
    let t = Temp::new()?;
    let path = t.0.join("remote-connections.conf");
    let mut p = profile();
    p.fingerprint = Some("SHA256:fixture".into());
    assert!(profiles::create(&path, &p)?.is_none());
    assert_eq!(profiles::load(&path)?, [p.clone()]);
    let original = p.clone();
    p.host = "another.invalid".into();
    profiles::replace(&path, &original, &p)?;
    assert_eq!(profiles::load(&path)?.len(), 1);
    let text = std::fs::read_to_string(path)?;
    assert!(!text.contains("password"));
    assert!(!text.contains("passphrase"));
    p.host = "host\r\nDELE everything".into();
    assert!(p.validate().is_err());
    Ok(())
}
#[test]
fn pane_navigation_marks_refresh_identity_and_disconnect_keep_native_paths() -> io::Result<()> {
    let t = Temp::new()?;
    std::fs::write(t.0.join("local"), b"unchanged")?;
    let memory = Memory::default();
    memory.put("/folder", None);
    memory.put("/folder/Ёж.txt", Some(b"hello"));
    memory.put("/file.txt", Some(b"text"));
    let mut app = crate::app::App::new(t.0.clone())?;
    pump(&mut app, |a| a.panels.iter().all(|p| !p.loading));
    let conn = Connection::test(profile(), memory.clone());
    app.panels[0].open_remote(conn, true)?;
    pump(&mut app, |a| !a.panels[0].loading);
    assert_eq!(app.panels[0].path, t.0);
    assert!(app.panels[0].heading().starts_with("sftp://"));
    assert!(app.panels[0].targets().is_empty());
    assert!(app.panels[0]
        .entries
        .iter()
        .all(|e| e.path.as_os_str().is_empty()));
    let row = app.panels[0]
        .entries
        .iter()
        .position(|e| e.name == "file.txt")
        .unwrap();
    app.panels[0].cursor = row;
    app.panels[0].toggle_selection();
    assert_eq!(app.panels[0].remote_targets().len(), 1);
    let key = app.panels[0].location_key();
    app.panels[0].refresh(true);
    assert_ne!(app.panels[0].location_key(), key);
    pump(&mut app, |a| !a.panels[0].loading);
    assert!(app.panels[0].has_marks());
    assert_eq!(app.panels[0].entries[app.panels[0].cursor].name, "file.txt");
    app.panels[0].cursor = app.panels[0]
        .entries
        .iter()
        .position(|e| e.name == "folder")
        .unwrap();
    app.panels[0].enter(true);
    pump(&mut app, |a| !a.panels[0].loading);
    assert_eq!(
        app.panels[0].remote().unwrap().directory.as_str(),
        "/folder"
    );
    assert!(!app.panels[0].has_marks());
    app.panels[0].leave_remote(true);
    pump(&mut app, |a| !a.panels[0].loading);
    assert!(!app.panels[0].is_virtual());
    assert_eq!(std::fs::read(t.0.join("local"))?, b"unchanged");
    assert_eq!(app.panels[1].path, t.0);
    Ok(())
}
#[test]
fn download_and_upload_use_owned_staging_and_preserve_originals() -> io::Result<()> {
    use crate::operations::{conflict::Decision, CopyOptions, ExistingDestination};
    let t = Temp::new()?;
    let memory = Memory::default();
    memory.put("/docs", None);
    memory.put("/docs/empty", None);
    memory.put("/docs/данные.txt", Some(b"remote"));
    let connection = Connection::test(profile(), memory.clone());
    let members = connection.with(|b| b.list(&RemotePath::new("/")?, &Default::default()))?;
    let transfer = transfer::Transfer::Download {
        connection: connection.clone(),
        members,
        destination: t.0.clone(),
    };
    let cancel = std::sync::atomic::AtomicBool::new(false);
    transfer.run(CopyOptions::default(), &cancel, |_| {}, &mut |_| {
        Ok(Decision::Replace)
    })?;
    assert_eq!(std::fs::read(t.0.join("docs/данные.txt"))?, b"remote");
    assert!(t.0.join("docs/empty").is_dir());
    assert!(std::fs::read_dir(&t.0)?.all(|p| !p
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".dn-")));
    std::fs::write(t.0.join("upload.txt"), b"local")?;
    let upload = transfer::Transfer::upload(
        connection,
        vec![t.0.join("upload.txt")],
        RemotePath::new("/")?,
    )?;
    upload.run(
        CopyOptions {
            existing: ExistingDestination::Ask,
            ..Default::default()
        },
        &cancel,
        |_| {},
        &mut |_| Ok(Decision::Replace),
    )?;
    assert_eq!(
        memory.0.lock().unwrap()[&RemotePath::new("/upload.txt")?],
        Some(b"local".to_vec())
    );
    assert_eq!(std::fs::read(t.0.join("upload.txt"))?, b"local");
    assert!(!memory
        .0
        .lock()
        .unwrap()
        .keys()
        .any(|p| p.as_str().contains(".dn-upload")));
    Ok(())
}
#[test]
fn remote_edit_checks_changes_retains_working_copy_and_writes_back() -> io::Result<()> {
    let t = Temp::new()?;
    let memory = Memory::default();
    memory.put("/file.txt", Some(b"old"));
    let connection = Connection::test(profile(), memory.clone());
    let member = Memory::member(&RemotePath::new("/file.txt")?, &Some(b"old".to_vec()));
    let cancel = Default::default();
    let origin = editor::Origin::prepare(connection, member, t.0.clone(), &cancel)?;
    std::fs::write(&origin.path, b"new")?;
    memory.put("/file.txt", Some(b"outside"));
    assert!(origin
        .save(&cancel)
        .unwrap_err()
        .contains("changed outside"));
    assert_eq!(std::fs::read(&origin.path)?, b"new");
    memory.put("/file.txt", Some(b"old"));
    origin.save(&cancel).map_err(io::Error::other)?;
    assert_eq!(
        memory.0.lock().unwrap()[&RemotePath::new("/file.txt")?],
        Some(b"new".to_vec())
    );
    let path = origin.path.clone();
    drop(origin);
    assert!(!path.exists());
    Ok(())
}
#[test]
fn cancelled_and_changed_sources_never_publish_downloads() -> io::Result<()> {
    let t = Temp::new()?;
    let memory = Memory::default();
    memory.put("/file.txt", Some(b"data"));
    let connection = Connection::test(profile(), memory.clone());
    let member = Memory::member(&RemotePath::new("/file.txt")?, &Some(b"old".to_vec()));
    let transfer = transfer::Transfer::Download {
        connection,
        members: vec![member],
        destination: t.0.clone(),
    };
    assert!(transfer
        .run(Default::default(), &false.into(), |_| {}, &mut |_| panic!(
            "no conflict"
        ))
        .is_err());
    assert!(!t.0.join("file.txt").exists());
    assert!(transfer
        .run(Default::default(), &true.into(), |_| {}, &mut |_| panic!(
            "no conflict"
        ))
        .is_err());
    assert_eq!(std::fs::read_dir(&t.0)?.count(), 0);
    Ok(())
}

#[test]
fn ssh_host_key_verification_precedes_password_authentication() -> io::Result<()> {
    struct Reject(Arc<AtomicU64>);
    impl russh::server::Handler for Reject {
        type Error = russh::Error;
        async fn auth_password(
            &mut self,
            _user: &str,
            _password: &str,
        ) -> Result<russh::server::Auth, Self::Error> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(russh::server::Auth::Reject {
                proceed_with_methods: None,
                partial_success: false,
            })
        }
    }
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
    listener.set_nonblocking(true)?;
    let port = listener.local_addr()?.port();
    let auths = Arc::new(AtomicU64::new(0));
    let counter = auths.clone();
    let key = russh::keys::PrivateKey::random(
        &mut russh::keys::key::safe_rng(),
        russh::keys::Algorithm::Ed25519,
    )
    .map_err(io::Error::other)?;
    let server = std::thread::spawn(move || -> io::Result<()> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        runtime.block_on(async {
            let listener = tokio::net::TcpListener::from_std(listener)?;
            let config = Arc::new(russh::server::Config {
                keys: vec![key],
                auth_rejection_time: Duration::ZERO,
                ..Default::default()
            });
            let mut sessions = Vec::new();
            for _ in 0..3 {
                let (stream, _) = tokio::time::timeout(Duration::from_secs(8), listener.accept())
                    .await
                    .map_err(io::Error::other)??;
                let session =
                    russh::server::run_stream(config.clone(), stream, Reject(counter.clone()))
                        .await
                        .map_err(io::Error::other)?;
                sessions.push(tokio::spawn(async move {
                    let _ = session.await;
                }));
            }
            for session in sessions {
                tokio::time::timeout(Duration::from_secs(8), session)
                    .await
                    .map_err(io::Error::other)?
                    .map_err(io::Error::other)?;
            }
            Ok(())
        })
    });
    let mut p = profile();
    p.host = "127.0.0.1".into();
    p.port = port;
    let credentials = |profile: Profile| remote::Credentials {
        profile,
        password: zeroize::Zeroizing::new("private-fixture-password".into()),
    };
    let fingerprint = match remote::connect(credentials(p.clone()), &Default::default()) {
        Err(remote::ConnectError::HostKey(key)) => key,
        _ => panic!("Unknown host must request verification"),
    };
    assert_eq!(auths.load(Ordering::Relaxed), 0);
    p.fingerprint = Some(fingerprint);
    assert!(
        matches!(remote::connect(credentials(p.clone()),&Default::default()),Err(remote::ConnectError::Failure(e))if e.kind()==io::ErrorKind::PermissionDenied)
    );
    assert_eq!(auths.load(Ordering::Relaxed), 1);
    p.fingerprint = Some("SHA256:changed-fixture".into());
    assert!(
        matches!(remote::connect(credentials(p),&Default::default()),Err(remote::ConnectError::Failure(e))if e.kind()==io::ErrorKind::PermissionDenied)
    );
    assert_eq!(auths.load(Ordering::Relaxed), 1);
    server.join().expect("private SSH server")?;
    Ok(())
}

#[test]
fn ftp_listing_and_download_use_passive_binary_streams() -> io::Result<()> {
    use std::io::BufRead;
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
    let port = listener.local_addr()?.port();
    let server = std::thread::spawn(move || -> io::Result<()> {
        let (mut control, _) = listener.accept()?;
        control.set_read_timeout(Some(Duration::from_secs(8)))?;
        let mut input = std::io::BufReader::new(control.try_clone()?);
        control.write_all(b"220 private DN test\r\n")?;
        let mut data = None;
        loop {
            let mut line = String::new();
            if input.read_line(&mut line)? == 0 {
                break;
            }
            let line = line.trim_end();
            if line.starts_with("USER ") {
                control.write_all(b"331 Password required\r\n")?;
            } else if line.starts_with("PASS ") {
                control.write_all(b"230 Logged in\r\n")?;
            } else if line == "TYPE I" {
                control.write_all(b"200 Binary\r\n")?;
            } else if line == "PASV" {
                let socket = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
                let port = socket.local_addr()?.port();
                write!(
                    control,
                    "227 Entering Passive Mode (127,0,0,1,{},{})\r\n",
                    port / 256,
                    port % 256
                )?;
                data = Some(socket);
            } else if line == "EPSV" {
                let socket = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
                write!(
                    control,
                    "229 Entering Extended Passive Mode (|||{}|)\r\n",
                    socket.local_addr()?.port()
                )?;
                data = Some(socket);
            } else if line.starts_with("MLSD ") || line.starts_with("RETR ") {
                control.write_all(b"150 Opening data\r\n")?;
                let (mut stream, _) = data
                    .take()
                    .ok_or_else(|| io::Error::other("No passive socket"))?
                    .accept()?;
                if line.starts_with("MLSD ") {
                    stream.write_all(b"type=file;size=4;modify=20260101000000; file.txt\r\n")?;
                } else {
                    stream.write_all(b"data")?;
                }
                drop(stream);
                control.write_all(b"226 Transfer complete\r\n")?;
            } else if line == "QUIT" {
                control.write_all(b"221 Goodbye\r\n")?;
                break;
            } else {
                return Err(io::Error::other(format!(
                    "Unexpected FTP test command: {}",
                    line.split_whitespace().next().unwrap_or("")
                )));
            }
        }
        Ok(())
    });
    let mut p = profile();
    p.host = "127.0.0.1".into();
    p.port = port;
    p.protocol = Protocol::Ftp;
    let connection = remote::connect(
        remote::Credentials {
            profile: p,
            password: zeroize::Zeroizing::new("fixture".into()),
        },
        &Default::default(),
    )
    .map_err(|_| io::Error::other("Private FTP connection failed"))?;
    let entries = connection.with(|b| b.list(&RemotePath::new("/")?, &Default::default()))?;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].size, Some(4));
    let mut bytes = Vec::new();
    connection.with(|b| b.read(&entries[0].path, &mut bytes, &Default::default(), 10))?;
    assert_eq!(bytes, b"data");
    drop(connection);
    server.join().expect("private FTP server")?;
    Ok(())
}

#[test]
fn ftps_refusal_does_not_fall_back_or_send_password() -> io::Result<()> {
    use std::io::BufRead;
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
    let port = listener.local_addr()?.port();
    let server = std::thread::spawn(move || -> io::Result<()> {
        let (mut socket, _) = listener.accept()?;
        socket.set_read_timeout(Some(Duration::from_secs(5)))?;
        let mut input = std::io::BufReader::new(socket.try_clone()?);
        socket.write_all(b"220 private FTPS refusal\r\n")?;
        let mut line = String::new();
        input.read_line(&mut line)?;
        assert_eq!(line.trim(), "AUTH TLS");
        socket.write_all(b"534 TLS refused\r\n")?;
        line.clear();
        let n = input.read_line(&mut line)?;
        assert_eq!(n, 0, "FTPS must not send USER or PASS after TLS failure");
        Ok(())
    });
    let mut p = profile();
    p.protocol = Protocol::Ftps;
    p.host = "127.0.0.1".into();
    p.port = port;
    assert!(matches!(
        remote::connect(
            remote::Credentials {
                profile: p,
                password: zeroize::Zeroizing::new("never-send".into())
            },
            &Default::default()
        ),
        Err(remote::ConnectError::Failure(_))
    ));
    server.join().expect("private FTPS refusal server")?;
    Ok(())
}

#[test]
fn remote_queue_preview_editor_and_mutation_restore_owners_and_keep_shell_local() -> io::Result<()>
{
    use crate::{editor::EditorAction, operations::Kind};
    let t = Temp::new()?;
    let memory = Memory::default();
    memory.put("/file.txt", Some(b"original"));
    let mut app = crate::app::App::new(t.0.clone())?;
    app.config_store = crate::config::Store::new(Some(t.0.join("config.conf")));
    app.config.save_on_exit = false;
    pump(&mut app, |a| a.panels.iter().all(|p| !p.loading));
    app.panels[0].open_remote(Connection::test(profile(), memory.clone()), true)?;
    pump(&mut app, |a| !a.panels[0].loading);
    app.panels[0].cursor = app.panels[0]
        .entries
        .iter()
        .position(|e| e.name == "file.txt")
        .unwrap();
    assert!(!crate::archive::require_native(&mut app));
    crate::input::handle(
        &mut app,
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
        ),
    );
    assert_eq!(app.panels[0].path, t.0);
    view(&mut app, false, crate::viewer::ViewMode::Auto);
    pump(&mut app, |a| a.desktop.active_viewer().is_some());
    assert_eq!(
        app.desktop
            .active_viewer()
            .unwrap()
            .contents
            .as_ref()
            .unwrap()
            .source
            .as_deref()
            .unwrap(),
        "original"
    );
    crate::windowing::activate(&mut app, crate::desktop::WindowId::MANAGER);
    view(&mut app, true, crate::viewer::ViewMode::Auto);
    pump(&mut app, |a| {
        a.desktop
            .active_editor()
            .is_some_and(|e| e.buffer.is_some())
    });
    let editor_id = app.desktop.active().id;
    assert!(app.desktop.active_editor().unwrap().remote_origin.is_some());
    crate::editor::paste(&mut app, "changed-");
    crate::editor::action(&mut app, EditorAction::Save);
    pump(&mut app, |a| {
        a.desktop.editor(editor_id).is_some_and(|e| !e.busy())
    });
    assert_eq!(
        memory.0.lock().unwrap()[&RemotePath::new("/file.txt")?],
        Some(b"changed-original".to_vec())
    );
    crate::windowing::activate(&mut app, crate::desktop::WindowId::MANAGER);
    pump(&mut app, |a| !a.panels[0].loading);
    app.panels[0].cursor = app.panels[0]
        .entries
        .iter()
        .position(|e| e.name == "file.txt")
        .unwrap();
    prompt_copy(&mut app, Some(t.0.clone()));
    assert!(app
        .modals
        .prompt()
        .unwrap()
        .copy
        .as_ref()
        .unwrap()
        .remote
        .is_some());
    app.submit_prompt();
    pump(&mut app, |a| !a.transfers.has_work());
    assert_eq!(std::fs::read(t.0.join("file.txt"))?, b"changed-original");
    crate::windowing::activate(&mut app, crate::desktop::WindowId::MANAGER);
    pump(&mut app, |a| !a.panels[0].loading);
    prompt_mutation(&mut app, Kind::Mkdir, false);
    let p = app.modals.prompt_mut().unwrap();
    p.field = "new-dir".into();
    p.cursor = p.field.len();
    app.submit_prompt();
    pump(&mut app, |a| !a.transfers.has_work());
    assert!(memory
        .0
        .lock()
        .unwrap()
        .contains_key(&RemotePath::new("/new-dir")?));
    assert_eq!(app.panels[0].path, t.0);
    assert!(!std::fs::read_dir(&t.0)?.any(|e| e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".dn-remote")));
    crate::windowing::activate(&mut app, crate::desktop::WindowId::MANAGER);
    app.active = 1;
    pump(&mut app, |a| !a.panels[1].loading);
    app.panels[1].cursor = app.panels[1]
        .entries
        .iter()
        .position(|e| e.name == "file.txt")
        .unwrap();
    app.prompt_operation(Kind::Move, false);
    assert!(app.modals.prompt().is_none()); // A remote pane must never become its outer disk directory.
    crate::input::handle(
        &mut app,
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
        ),
    );
    prompt_copy(&mut app, None);
    assert!(app
        .modals
        .prompt()
        .unwrap()
        .copy
        .as_ref()
        .unwrap()
        .remote
        .as_ref()
        .unwrap()
        .uploading());
    crate::copy_dialog::action(&mut app, crate::copy_dialog::Action::CheckSpace);
    crate::copy_dialog::action(&mut app, crate::copy_dialog::Action::Verify);
    let options = app.modals.prompt().unwrap().copy.as_ref().unwrap().options;
    assert!(!options.check_free_space && !options.verify_writes);

    Ok(())
}

#[test]
fn profile_edit_rename_delete_preserve_other_records_and_reject_stale_writes() -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("profiles.conf");
    let first = profile();
    let mut second = first.clone();
    second.name = "Второй 世界".into();
    second.host = "second.invalid".into();
    profiles::create(&path, &first)?;
    profiles::create(&path, &second)?;
    let mut text: toml::Table = std::fs::read_to_string(&path)?.parse().unwrap();
    text.insert("future".into(), toml::Value::String("preserve".into()));
    text["connections"].as_array_mut().unwrap()[0]
        .as_table_mut()
        .unwrap()
        .insert("annotation".into(), toml::Value::Integer(42));
    std::fs::write(&path, text.to_string())?;
    let mut renamed = first.clone();
    renamed.name = "Renamed Ёж".into();
    renamed.key = Some(t.0.join("keys 世界/private key"));
    profiles::replace(&path, &first, &renamed)?;
    let loaded = profiles::load(&path)?;
    assert_eq!(loaded, [renamed.clone(), second.clone()]);
    let text: toml::Table = std::fs::read_to_string(&path)?.parse().unwrap();
    assert_eq!(text["future"].as_str(), Some("preserve"));
    assert_eq!(
        text["connections"].as_array().unwrap()[0]["annotation"].as_integer(),
        Some(42)
    );
    let before = std::fs::read(&path)?;
    let mut duplicate = renamed.clone();
    duplicate.name = second.name.clone();
    assert!(profiles::replace(&path, &renamed, &duplicate).is_err());
    assert!(profiles::create(&path, &second).is_err());
    assert_eq!(std::fs::read(&path)?, before);
    let mut changed = second.clone();
    changed.host = "changed.invalid".into();
    profiles::replace(&path, &second, &changed)?;
    let before = std::fs::read(&path)?;
    assert!(profiles::delete(&path, &second).is_err());
    assert!(profiles::replace(&path, &second, &second).is_err());
    assert_eq!(std::fs::read(&path)?, before);
    profiles::delete(&path, &renamed)?;
    assert_eq!(profiles::load(&path)?, [changed.clone()]);
    profiles::delete(&path, &changed)?;
    assert!(profiles::load(&path)?.is_empty());
    let text: toml::Table = std::fs::read_to_string(&path)?.parse().unwrap();
    assert_eq!(text["future"].as_str(), Some("preserve"));
    Ok(())
}
#[test]
fn profile_delete_lock_failure_and_malformed_lists_leave_storage_unchanged() -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("profiles.conf");
    let p = profile();
    profiles::create(&path, &p)?;
    let lock = t.0.join("profiles.conf.lock");
    std::fs::write(&lock, "private test lock")?;
    let before = std::fs::read(&path)?;
    assert!(profiles::delete(&path, &p).is_err());
    assert_eq!(std::fs::read(&path)?, before);
    std::fs::remove_file(lock)?;
    std::fs::write(&path, "connections='invalid'\n")?;
    let before = std::fs::read(&path)?;
    assert!(profiles::load(&path).is_err());
    assert!(profiles::delete(&path, &p).is_err());
    assert!(profiles::create(&path, &p).is_err());
    assert_eq!(std::fs::read(&path)?, before);
    Ok(())
}
