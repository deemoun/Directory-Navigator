//! Private wire-level SFTP fixture: no host files or installed SSH daemon.
use super::tests::*;
use russh_sftp::protocol::*;
use std::io::Read;
struct Files {
    atomic: bool,
    memory: Memory,
    read_dirs: std::collections::HashSet<String>,
}
fn status(id: u32) -> Status {
    Status {
        id,
        status_code: StatusCode::Ok,
        error_message: String::new(),
        language_tag: String::new(),
    }
}
fn attrs(data: &Option<Vec<u8>>) -> FileAttributes {
    let mut a = FileAttributes::empty();
    a.size = data.as_ref().map(|b| b.len() as u64);
    a.permissions = Some(if data.is_some() { 0o100600 } else { 0o040700 });
    a
}
impl russh_sftp::server::Handler for Files {
    type Error = StatusCode;
    fn unimplemented(&self) -> StatusCode {
        StatusCode::OpUnsupported
    }
    async fn init(
        &mut self,
        _version: u32,
        _extensions: std::collections::HashMap<String, String>,
    ) -> Result<Version, StatusCode> {
        let mut version = Version::new();
        if self.atomic {
            version
                .extensions
                .insert("posix-rename@openssh.com".into(), "1".into());
        }
        Ok(version)
    }
    async fn extended(
        &mut self,
        id: u32,
        request: String,
        data: Vec<u8>,
    ) -> Result<Packet, StatusCode> {
        if !self.atomic || request != "posix-rename@openssh.com" {
            return Err(StatusCode::OpUnsupported);
        }
        let mut input = data.as_slice();
        let mut paths = Vec::new();
        for _ in 0..2 {
            let mut length = [0; 4];
            input
                .read_exact(&mut length)
                .map_err(|_| StatusCode::BadMessage)?;
            let n = u32::from_be_bytes(length) as usize;
            if n > 4096 || n > input.len() {
                return Err(StatusCode::BadMessage);
            }
            let value = std::str::from_utf8(&input[..n]).map_err(|_| StatusCode::BadMessage)?;
            paths.push(RemotePath::new(value).map_err(|_| StatusCode::BadMessage)?);
            input = &input[n..];
        }
        if !input.is_empty() {
            return Err(StatusCode::BadMessage);
        }
        self.memory
            .publish(&paths[0], &paths[1], true)
            .map_err(|_| StatusCode::Failure)?;
        Ok(Packet::Status(status(id)))
    }
    async fn lstat(&mut self, id: u32, path: String) -> Result<Attrs, StatusCode> {
        let path = RemotePath::new(&path).map_err(|_| StatusCode::Failure)?;
        let memory = self.memory.0.lock().unwrap();
        let data = memory.get(&path).ok_or(StatusCode::NoSuchFile)?;
        Ok(Attrs {
            id,
            attrs: attrs(data),
        })
    }
    async fn close(&mut self, id: u32, _handle: String) -> Result<Status, StatusCode> {
        Ok(status(id))
    }
    async fn opendir(&mut self, id: u32, path: String) -> Result<Handle, StatusCode> {
        self.read_dirs.remove(&path);
        Ok(Handle { id, handle: path })
    }
    async fn readdir(&mut self, id: u32, handle: String) -> Result<Name, StatusCode> {
        if !self.read_dirs.insert(handle.clone()) {
            return Err(StatusCode::Eof);
        }
        let path = RemotePath::new(&handle).map_err(|_| StatusCode::Failure)?;
        let files = self
            .memory
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _)| p.parent() == path && **p != path)
            .map(|(p, d)| File::new(p.name(), attrs(d)))
            .collect::<Vec<_>>();
        if files.is_empty() {
            return Err(StatusCode::Eof);
        }
        Ok(Name { id, files })
    }
    async fn open(
        &mut self,
        id: u32,
        filename: String,
        pflags: OpenFlags,
        _attrs: FileAttributes,
    ) -> Result<Handle, StatusCode> {
        let path = RemotePath::new(&filename).map_err(|_| StatusCode::Failure)?;
        let mut m = self.memory.0.lock().unwrap();
        if pflags.contains(OpenFlags::CREATE) {
            if m.contains_key(&path) {
                return Err(StatusCode::Failure);
            }
            m.insert(path.clone(), Some(vec![]));
        }
        if !m.get(&path).is_some_and(Option::is_some) {
            return Err(StatusCode::NoSuchFile);
        }
        Ok(Handle {
            id,
            handle: filename,
        })
    }
    async fn read(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        len: u32,
    ) -> Result<Data, StatusCode> {
        let m = self.memory.0.lock().unwrap();
        let path = RemotePath::new(&handle).unwrap();
        let bytes = m
            .get(&path)
            .and_then(Option::as_ref)
            .ok_or(StatusCode::NoSuchFile)?;
        let start = usize::try_from(offset).map_err(|_| StatusCode::Failure)?;
        if start >= bytes.len() {
            return Err(StatusCode::Eof);
        }
        Ok(Data {
            id,
            data: bytes[start..bytes.len().min(start + len as usize)].to_vec(),
        })
    }
    async fn write(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        data: Vec<u8>,
    ) -> Result<Status, StatusCode> {
        let mut m = self.memory.0.lock().unwrap();
        let path = RemotePath::new(&handle).unwrap();
        let bytes = m
            .get_mut(&path)
            .and_then(Option::as_mut)
            .ok_or(StatusCode::NoSuchFile)?;
        if offset != bytes.len() as u64 {
            return Err(StatusCode::Failure);
        }
        bytes.extend(data);
        Ok(status(id))
    }
    async fn mkdir(
        &mut self,
        id: u32,
        path: String,
        _attrs: FileAttributes,
    ) -> Result<Status, StatusCode> {
        self.memory
            .mkdir(&RemotePath::new(&path).unwrap())
            .map_err(|_| StatusCode::Failure)?;
        Ok(status(id))
    }
    async fn remove(&mut self, id: u32, filename: String) -> Result<Status, StatusCode> {
        self.memory
            .remove(&RemotePath::new(&filename).unwrap(), false)
            .map_err(|_| StatusCode::Failure)?;
        Ok(status(id))
    }
    async fn rmdir(&mut self, id: u32, path: String) -> Result<Status, StatusCode> {
        self.memory
            .remove(&RemotePath::new(&path).unwrap(), true)
            .map_err(|_| StatusCode::Failure)?;
        Ok(status(id))
    }
    async fn rename(
        &mut self,
        id: u32,
        oldpath: String,
        newpath: String,
    ) -> Result<Status, StatusCode> {
        self.memory
            .rename(
                &RemotePath::new(&oldpath).unwrap(),
                &RemotePath::new(&newpath).unwrap(),
            )
            .map_err(|_| StatusCode::Failure)?;
        Ok(status(id))
    }
}
struct Ssh {
    atomic: bool,
    memory: Memory,
    channel: Option<russh::Channel<russh::server::Msg>>,
}
impl russh::server::Handler for Ssh {
    type Error = russh::Error;
    async fn auth_password(
        &mut self,
        user: &str,
        password: &str,
    ) -> Result<russh::server::Auth, Self::Error> {
        if user == "tester" && password == "fixture-secret" {
            Ok(russh::server::Auth::Accept)
        } else {
            Ok(russh::server::Auth::Reject {
                proceed_with_methods: None,
                partial_success: false,
            })
        }
    }
    async fn channel_open_session(
        &mut self,
        channel: russh::Channel<russh::server::Msg>,
        reply: russh::server::ChannelOpenHandle,
        _session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        self.channel = Some(channel);
        reply.accept().await;
        Ok(())
    }
    async fn subsystem_request(
        &mut self,
        id: russh::ChannelId,
        name: &str,
        session: &mut russh::server::Session,
    ) -> Result<(), Self::Error> {
        if name == "sftp" {
            session.channel_success(id)?;
            let channel = self.channel.take().unwrap();
            let files = Files {
                atomic: self.atomic,
                memory: self.memory.clone(),
                read_dirs: Default::default(),
            };
            tokio::spawn(async move {
                russh_sftp::server::run(channel.into_stream(), files).await;
            });
        } else {
            session.channel_failure(id)?;
        }
        Ok(())
    }
}
#[test]
fn sftp_wire_listing_streamed_read_exclusive_write_and_rename() -> io::Result<()> {
    for atomic in [false, true] {
        wire_case(atomic)?;
    }
    Ok(())
}
fn wire_case(atomic: bool) -> io::Result<()> {
    let memory = Memory::default();
    let bytes = vec![0x93; 100_000];
    memory.put("/данные.txt", Some(&bytes));
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
    listener.set_nonblocking(true)?;
    let port = listener.local_addr()?.port();
    let key = russh::keys::PrivateKey::random(
        &mut russh::keys::key::safe_rng(),
        russh::keys::Algorithm::Ed25519,
    )
    .map_err(io::Error::other)?;
    let fingerprint = key
        .public_key()
        .fingerprint(russh::keys::HashAlg::Sha256)
        .to_string();
    let server_memory = memory.clone();
    let server = std::thread::spawn(move || -> io::Result<()> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        runtime.block_on(async {
            let listener = tokio::net::TcpListener::from_std(listener)?;
            let (stream, _) = tokio::time::timeout(Duration::from_secs(10), listener.accept())
                .await
                .map_err(io::Error::other)??;
            let config = Arc::new(russh::server::Config {
                keys: vec![key],
                ..Default::default()
            });
            let session = russh::server::run_stream(
                config,
                stream,
                Ssh {
                    atomic,
                    memory: server_memory,
                    channel: None,
                },
            )
            .await
            .map_err(io::Error::other)?;
            let _ = tokio::time::timeout(Duration::from_secs(15), session)
                .await
                .map_err(io::Error::other)?;
            Ok(())
        })
    });
    let mut p = profile();
    p.host = "127.0.0.1".into();
    p.port = port;
    p.fingerprint = Some(fingerprint);
    let connection = remote::connect(
        remote::Credentials {
            profile: p,
            password: zeroize::Zeroizing::new("fixture-secret".into()),
        },
        &Default::default(),
    )
    .map_err(|_| io::Error::other("private SFTP connection"))?;
    connection.with(|b| {
        let c = Default::default();
        let root = RemotePath::new("/")?;
        let list = b.list(&root, &c)?;
        assert_eq!(list.len(), 1);
        assert!(list[0].regular);
        let mut actual = vec![];
        assert_eq!(b.read(&list[0].path, &mut actual, &c, 200_000)?, 100_000);
        assert_eq!(actual, bytes);
        let stage = transfer::stage_directory(b, &root)?;
        let file = stage.child("content")?;
        assert_eq!(b.write_new(&file, &mut bytes.as_slice(), &c)?, 100_000);
        assert!(b.write_new(&file, &mut bytes.as_slice(), &c).is_err());
        let target = root.child("uploaded.bin")?;
        b.publish(&file, &target, false)?;
        let mut actual = vec![];
        b.read(&target, &mut actual, &c, 200_000)?;
        assert_eq!(actual, bytes);
        if atomic {
            b.publish(&target, &list[0].path, true)?;
            assert!(b.stat(&target, &c)?.is_none());
            let mut replaced = vec![];
            b.read(&list[0].path, &mut replaced, &c, 200_000)?;
            assert_eq!(replaced, bytes);
        } else {
            // Without the extension, the previous remote file must remain.
            assert!(b.publish(&target, &list[0].path, true).is_err());
            assert!(b.stat(&target, &c)?.is_some());
            b.remove(&target, false)?;
        }
        b.remove(&stage, true)?;
        Ok(())
    })?;
    drop(connection);
    server.join().unwrap()?;
    Ok(())
}
