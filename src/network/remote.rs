//! Protocol implementations. Remote paths never implement AsRef<Path>.
use crate::task::Cancellation;
use russh_sftp::{
    client::RawSftpSession,
    protocol::{FileAttributes, OpenFlags, StatusCode},
};
use std::{
    io::{self, Read, Write},
    net::{TcpStream, ToSocketAddrs},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime},
};
use zeroize::Zeroizing;
pub const ENTRY_LIMIT: usize = 10_000;
pub const LIST_BYTES: u64 = 8 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(15);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protocol {
    Sftp,
    Ftp,
    Ftps,
}
impl Protocol {
    pub fn label(self) -> &'static str {
        match self {
            Self::Sftp => "SFTP",
            Self::Ftp => "FTP",
            Self::Ftps => "FTPS",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profile {
    pub name: String,
    pub protocol: Protocol,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub directory: RemotePath,
    pub key: Option<PathBuf>,
    pub fingerprint: Option<String>,
}
impl Profile {
    pub fn validate(&self) -> io::Result<()> {
        if self.name.is_empty() || self.name.len() > 256 || self.name.chars().any(char::is_control)
        {
            return Err(io::Error::other(
                "Enter a profile name without control characters (up to 256 bytes)",
            ));
        }
        if self.host.is_empty()
            || self.host.len() > 253
            || self
                .host
                .chars()
                .any(|c| c.is_control() || c.is_whitespace() || matches!(c, '/' | '\\' | '@'))
            || self.port == 0
        {
            return Err(io::Error::other("Enter a host and a port from 1 to 65535"));
        }
        if self.user.is_empty() || self.user.len() > 256 || self.user.chars().any(char::is_control)
        {
            return Err(io::Error::other("Enter a valid username"));
        }
        Ok(())
    }
    pub fn label(&self) -> String {
        let user = self
            .user
            .as_bytes()
            .iter()
            .map(|b| {
                if b.is_ascii_alphanumeric() || b"-._~".contains(b) {
                    (*b as char).to_string()
                } else {
                    format!("%{b:02X}")
                }
            })
            .collect::<String>();
        let host = if self.host.contains(':') && !self.host.starts_with('[') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        format!(
            "{}://{}@{}:{}",
            self.protocol.label().to_lowercase(),
            user,
            host,
            self.port
        )
    }
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RemotePath(String);
impl RemotePath {
    pub fn new(value: &str) -> io::Result<Self> {
        if !value.starts_with('/')
            || value.len() > 4096
            || value.chars().any(char::is_control)
            || value.contains('\\')
        {
            return Err(io::Error::other(
                "Use an absolute remote path without control characters or backslashes",
            ));
        }
        let mut parts = Vec::new();
        for p in value.split('/') {
            match p {
                "" | "." => {}
                ".." => {
                    parts.pop();
                }
                _ => parts.push(p),
            }
        }
        Ok(Self(format!("/{}", parts.join("/"))))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn child(&self, name: &str) -> io::Result<Self> {
        if name.is_empty()
            || matches!(name, "." | "..")
            || name.contains('/')
            || name.contains('\\')
            || name.chars().any(char::is_control)
        {
            return Err(io::Error::other(
                "Server returned an unsafe remote filename",
            ));
        }
        Self::new(&format!("{}/{}", self.0.trim_end_matches('/'), name))
    }
    pub fn parent(&self) -> Self {
        Self::new(
            self.0
                .rsplit_once('/')
                .map_or("/", |(p, _)| if p.is_empty() { "/" } else { p }),
        )
        .expect("normalized parent")
    }
    pub fn name(&self) -> &str {
        self.0.rsplit('/').next().unwrap_or("")
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub path: RemotePath,
    pub regular: bool,
    pub directory: bool,
    pub link: bool,
    pub size: Option<u64>,
    pub modified: Option<SystemTime>,
}
pub trait Backend: Send {
    fn stat(&mut self, path: &RemotePath, cancel: &Cancellation) -> io::Result<Option<Member>> {
        Ok(self
            .list(&path.parent(), cancel)?
            .into_iter()
            .find(|m| m.path == *path))
    }
    fn list(&mut self, path: &RemotePath, cancel: &Cancellation) -> io::Result<Vec<Member>>;
    fn read(
        &mut self,
        path: &RemotePath,
        writer: &mut dyn Write,
        cancel: &Cancellation,
        limit: u64,
    ) -> io::Result<u64>;
    fn write_new(
        &mut self,
        path: &RemotePath,
        reader: &mut dyn Read,
        cancel: &Cancellation,
    ) -> io::Result<u64>;
    fn mkdir(&mut self, path: &RemotePath) -> io::Result<()>;
    fn publish(&mut self, from: &RemotePath, to: &RemotePath, replace: bool) -> io::Result<()>;
    fn remove(&mut self, path: &RemotePath, directory: bool) -> io::Result<()>;
    fn rename(&mut self, from: &RemotePath, to: &RemotePath) -> io::Result<()>;
}
pub struct Connection {
    pub profile: Profile,
    backend: Mutex<Box<dyn Backend>>,
}
impl Connection {
    pub fn with<T>(&self, run: impl FnOnce(&mut dyn Backend) -> io::Result<T>) -> io::Result<T> {
        let mut b = self.backend.try_lock().map_err(|e| match e {
            std::sync::TryLockError::WouldBlock => io::Error::new(
                io::ErrorKind::WouldBlock,
                "Remote connection is busy; retry after the current transfer finishes",
            ),
            std::sync::TryLockError::Poisoned(_) => {
                io::Error::other("Remote connection worker stopped")
            }
        })?;
        run(b.as_mut())
    }
    pub fn label(&self, path: &RemotePath) -> String {
        format!("{}{}", self.profile.label(), path.as_str())
    }
}
pub enum ConnectError {
    HostKey(String),
    Failure(io::Error),
}
impl From<io::Error> for ConnectError {
    fn from(e: io::Error) -> Self {
        Self::Failure(e)
    }
}
pub struct Credentials {
    pub profile: Profile,
    pub password: Zeroizing<String>,
}
struct HostKey {
    expected: Option<String>,
    observed: Arc<Mutex<Option<String>>>,
}
impl russh::client::Handler for HostKey {
    type Error = russh::Error;
    async fn check_server_key(
        &mut self,
        key: &russh::keys::PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let fingerprint = key
            .public_key()
            .fingerprint(russh::keys::HashAlg::Sha256)
            .to_string();
        *self.observed.lock().unwrap() = Some(fingerprint.clone());
        Ok(self.expected.as_ref() == Some(&fingerprint))
    }
}
struct Sftp {
    runtime: tokio::runtime::Runtime,
    session: RawSftpSession,
    _ssh: russh::client::Handle<HostKey>,
    atomic_replace: bool,
}
fn err(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}
fn eof(e: &russh_sftp::client::error::Error) -> bool {
    matches!(e,russh_sftp::client::error::Error::Status(s) if s.status_code==StatusCode::Eof)
}
impl Backend for Sftp {
    fn stat(&mut self, path: &RemotePath, cancel: &Cancellation) -> io::Result<Option<Member>> {
        cancel.check()?;
        match self.runtime.block_on(self.session.lstat(path.as_str())) {
            Ok(reply) => {
                let attrs = reply.attrs;
                let kind = attrs.file_type();
                Ok(Some(Member {
                    path: path.clone(),
                    regular: kind.is_file(),
                    directory: kind.is_dir(),
                    link: kind.is_symlink(),
                    size: attrs.size,
                    modified: attrs.modified().ok(),
                }))
            }
            Err(russh_sftp::client::error::Error::Status(s))
                if s.status_code == StatusCode::NoSuchFile =>
            {
                Ok(None)
            }
            Err(error) => Err(err(error)),
        }
    }
    fn list(&mut self, path: &RemotePath, cancel: &Cancellation) -> io::Result<Vec<Member>> {
        self.runtime.block_on(async {
            let handle = self
                .session
                .opendir(path.as_str())
                .await
                .map_err(err)?
                .handle;
            let result = async {
                let mut out = Vec::new();
                let mut budget = 0;
                loop {
                    cancel.check()?;
                    let page = match self.session.readdir(&handle).await {
                        Ok(p) => p,
                        Err(e) if eof(&e) => break,
                        Err(e) => return Err(err(e)),
                    };
                    if page.files.is_empty() {
                        return Err(err("Empty nonterminal SFTP listing"));
                    }
                    for file in page.files {
                        if matches!(file.filename.as_str(), "." | "..") {
                            continue;
                        }
                        budget += file.filename.len();
                        if out.len() >= ENTRY_LIMIT || budget > LIST_BYTES as usize {
                            return Err(err("Remote directory exceeds the listing limit"));
                        }
                        let kind = file.attrs.file_type();
                        out.push(Member {
                            path: path.child(&file.filename)?,
                            regular: kind.is_file(),
                            directory: kind.is_dir(),
                            link: kind.is_symlink(),
                            size: file.attrs.size,
                            modified: file.attrs.modified().ok(),
                        });
                    }
                }
                Ok(out)
            }
            .await;
            let closed = self.session.close(handle).await.map_err(err);
            match result {
                Ok(v) => {
                    closed?;
                    Ok(v)
                }
                Err(e) => Err(e),
            }
        })
    }
    fn read(
        &mut self,
        path: &RemotePath,
        writer: &mut dyn Write,
        cancel: &Cancellation,
        limit: u64,
    ) -> io::Result<u64> {
        self.runtime.block_on(async {
            let handle = self
                .session
                .open(path.as_str(), OpenFlags::READ, FileAttributes::default())
                .await
                .map_err(err)?
                .handle;
            let result = async {
                let mut offset = 0;
                loop {
                    cancel.check()?;
                    let chunk = match self.session.read(&handle, offset, 32768).await {
                        Ok(d) => d.data,
                        Err(e) if eof(&e) => break,
                        Err(e) => return Err(err(e)),
                    };
                    if chunk.is_empty() {
                        return Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "SFTP returned an empty data packet before EOF",
                        ));
                    }
                    offset += chunk.len() as u64;
                    if offset > limit {
                        let remaining = limit.saturating_sub(offset - chunk.len() as u64) as usize;
                        writer.write_all(&chunk[..remaining])?;
                        return Err(io::Error::new(
                            io::ErrorKind::FileTooLarge,
                            "Remote file exceeds the requested read limit",
                        ));
                    }
                    writer.write_all(&chunk)?;
                }
                Ok(offset)
            }
            .await;
            let closed = self.session.close(handle).await.map_err(err);
            match result {
                Ok(n) => {
                    closed?;
                    Ok(n)
                }
                Err(e) => Err(e),
            }
        })
    }
    fn write_new(
        &mut self,
        path: &RemotePath,
        reader: &mut dyn Read,
        cancel: &Cancellation,
    ) -> io::Result<u64> {
        self.runtime.block_on(async {
            let handle = self
                .session
                .open(
                    path.as_str(),
                    OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::EXCLUDE,
                    FileAttributes::default(),
                )
                .await
                .map_err(err)?
                .handle;
            let result = async {
                let mut offset = 0;
                let mut bytes = [0; 32768];
                loop {
                    cancel.check()?;
                    let n = reader.read(&mut bytes)?;
                    if n == 0 {
                        break;
                    }
                    self.session
                        .write(&handle, offset, bytes[..n].to_vec())
                        .await
                        .map_err(err)?;
                    offset += n as u64;
                }
                Ok(offset)
            }
            .await;
            let closed = self.session.close(handle).await.map_err(err);
            match result {
                Ok(n) => {
                    closed?;
                    Ok(n)
                }
                Err(e) => Err(e),
            }
        })
    }
    fn publish(&mut self, from: &RemotePath, to: &RemotePath, replace: bool) -> io::Result<()> {
        if !replace {
            return self.rename(from, to);
        }
        if !self.atomic_replace {
            return Err(err(
                "Server does not support atomic SFTP replacement; existing file retained",
            ));
        }
        let mut data = Vec::new();
        for name in [from.as_str(), to.as_str()] {
            data.extend_from_slice(&(name.len() as u32).to_be_bytes());
            data.extend_from_slice(name.as_bytes());
        }
        match self
            .runtime
            .block_on(self.session.extended("posix-rename@openssh.com", data))
            .map_err(err)?
        {
            russh_sftp::protocol::Packet::Status(status)
                if status.status_code == StatusCode::Ok =>
            {
                Ok(())
            }
            _ => Err(err("Server refused atomic SFTP replacement")),
        }
    }
    fn mkdir(&mut self, path: &RemotePath) -> io::Result<()> {
        self.runtime
            .block_on(self.session.mkdir(
                path.as_str(),
                FileAttributes {
                    permissions: Some(0o700),
                    ..Default::default()
                },
            ))
            .map(|_| ())
            .map_err(err)
    }
    fn remove(&mut self, path: &RemotePath, directory: bool) -> io::Result<()> {
        self.runtime
            .block_on(async {
                if directory {
                    self.session.rmdir(path.as_str()).await
                } else {
                    self.session.remove(path.as_str()).await
                }
            })
            .map(|_| ())
            .map_err(err)
    }
    fn rename(&mut self, from: &RemotePath, to: &RemotePath) -> io::Result<()> {
        self.runtime
            .block_on(self.session.rename(from.as_str(), to.as_str()))
            .map(|_| ())
            .map_err(err)
    }
}
struct Ftp<T: suppaftp::TlsStream> {
    ftp: suppaftp::ImplFtpStream<T>,
    reserved: std::collections::HashSet<RemotePath>,
}
impl<T: suppaftp::TlsStream + Send> Backend for Ftp<T> {
    fn list(&mut self, path: &RemotePath, cancel: &Cancellation) -> io::Result<Vec<Member>> {
        let (_, mut stream) = self
            .ftp
            .custom_data_command(
                format!("MLSD {}", path.as_str()),
                &[suppaftp::Status::AboutToSend, suppaftp::Status::AlreadyOpen],
            )
            .map_err(err)?;
        let mut bytes = Vec::new();
        let result = copy_bounded(&mut stream, &mut bytes, cancel, LIST_BYTES);
        let finished = stream.finish().map_err(err);
        result?;
        finished?;
        let text = std::str::from_utf8(&bytes).map_err(err)?;
        let mut out = Vec::new();
        for line in text.lines() {
            cancel.check()?;
            let file = suppaftp::list::ListParser::parse_mlsd(line).map_err(err)?;
            if matches!(file.name(), "." | "..") {
                continue;
            }
            if out.len() >= ENTRY_LIMIT {
                return Err(err("Remote directory exceeds the listing limit"));
            }
            out.push(Member {
                path: path.child(file.name())?,
                regular: file.is_file(),
                directory: file.is_directory(),
                link: file.is_symlink(),
                size: Some(file.size() as u64),
                modified: Some(file.modified()),
            });
        }
        Ok(out)
    }
    fn read(
        &mut self,
        path: &RemotePath,
        writer: &mut dyn Write,
        cancel: &Cancellation,
        limit: u64,
    ) -> io::Result<u64> {
        let mut stream = self.ftp.retr_as_stream(path.as_str()).map_err(err)?;
        let result = copy_bounded(&mut stream, writer, cancel, limit);
        let finished = stream.finish().map_err(err);
        let n = result?;
        finished?;
        Ok(n)
    }
    fn write_new(
        &mut self,
        path: &RemotePath,
        reader: &mut dyn Read,
        cancel: &Cancellation,
    ) -> io::Result<u64> {
        if !self.reserved.contains(&path.parent()) {
            return Err(err("FTP uploads require an owned staging directory"));
        }
        let mut stream = self.ftp.put_with_stream(path.as_str()).map_err(err)?;
        let result = copy_bounded(reader, &mut stream, cancel, 64 * 1024 * 1024 * 1024);
        let finished = stream.finish().map_err(err);
        let n = result?;
        finished?;
        Ok(n)
    }
    fn mkdir(&mut self, path: &RemotePath) -> io::Result<()> {
        self.ftp.mkdir(path.as_str()).map_err(err)?;
        self.reserved.insert(path.clone());
        Ok(())
    }
    fn publish(&mut self, from: &RemotePath, to: &RemotePath, _replace: bool) -> io::Result<()> {
        self.rename(from, to)
    }
    fn remove(&mut self, path: &RemotePath, directory: bool) -> io::Result<()> {
        if directory {
            self.ftp.rmdir(path.as_str())
        } else {
            self.ftp.rm(path.as_str())
        }
        .map_err(err)?;
        if directory {
            self.reserved.remove(path);
        }
        Ok(())
    }
    fn rename(&mut self, from: &RemotePath, to: &RemotePath) -> io::Result<()> {
        self.ftp.rename(from.as_str(), to.as_str()).map_err(err)
    }
}
pub fn copy_bounded(
    reader: &mut dyn Read,
    writer: &mut dyn Write,
    cancel: &Cancellation,
    limit: u64,
) -> io::Result<u64> {
    let mut total = 0;
    let mut chunk = [0; 32768];
    loop {
        cancel.check()?;
        let n = reader.read(&mut chunk)?;
        if n == 0 {
            return Ok(total);
        }
        total += n as u64;
        if total > limit {
            let remaining = limit.saturating_sub(total - n as u64) as usize;
            writer.write_all(&chunk[..remaining])?;
            return Err(io::Error::new(
                io::ErrorKind::FileTooLarge,
                "Remote read limit exceeded",
            ));
        }
        writer.write_all(&chunk[..n])?;
    }
}
pub fn connect(
    credentials: Credentials,
    cancel: &Cancellation,
) -> Result<Arc<Connection>, ConnectError> {
    credentials.profile.validate()?;
    cancel.check()?;
    let profile = credentials.profile;
    let backend: Box<dyn Backend> = match profile.protocol {
        Protocol::Sftp => {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            let observed = Arc::new(Mutex::new(None));
            let seen = observed.clone();
            let result = runtime.block_on(async {
                let handler = HostKey {
                    expected: profile.fingerprint.clone(),
                    observed: seen,
                };
                let config = russh::client::Config {
                    inactivity_timeout: Some(Duration::from_secs(60)),
                    ..Default::default()
                };
                let mut ssh = tokio::time::timeout(
                    TIMEOUT,
                    russh::client::connect(
                        Arc::new(config),
                        (profile.host.as_str(), profile.port),
                        handler,
                    ),
                )
                .await
                .map_err(err)?
                .map_err(err)?;
                cancel.check()?;
                let auth = if let Some(path) = &profile.key {
                    let file = crate::filesystem::open_regular(path)?;
                    let mut text = zeroize::Zeroizing::new(String::new());
                    file.take(1024 * 1024 + 1).read_to_string(&mut text)?;
                    if text.len() > 1024 * 1024 {
                        return Err(err("SSH key exceeds 1 MiB"));
                    }
                    let key = russh::keys::decode_secret_key(
                        &text,
                        (!credentials.password.is_empty()).then_some(credentials.password.as_str()),
                    )
                    .map_err(err)?;
                    let hash = ssh.best_supported_rsa_hash().await.map_err(err)?.flatten();
                    ssh.authenticate_publickey(
                        &profile.user,
                        russh::keys::PrivateKeyWithHashAlg::new(Arc::new(key), hash),
                    )
                    .await
                    .map_err(err)?
                } else {
                    ssh.authenticate_password(&profile.user, credentials.password.as_str())
                        .await
                        .map_err(err)?
                };
                if !auth.success() {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "SSH authentication failed",
                    ));
                }
                let channel = ssh.channel_open_session().await.map_err(err)?;
                channel.request_subsystem(true, "sftp").await.map_err(err)?;
                let sftp = RawSftpSession::new_with_config(
                    channel.into_stream(),
                    russh_sftp::client::Config {
                        max_concurrent_reads: 1,
                        max_concurrent_writes: 1,
                        ..Default::default()
                    },
                );
                let version = sftp.init().await.map_err(err)?;
                let atomic_replace = version.extensions.contains_key("posix-rename@openssh.com");
                Ok((ssh, sftp, atomic_replace))
            });
            match result {
                Ok((ssh, session, atomic_replace)) => Box::new(Sftp {
                    runtime,
                    session,
                    _ssh: ssh,
                    atomic_replace,
                }),
                Err(e) => {
                    if let Some(key) = observed.lock().unwrap().clone() {
                        if profile.fingerprint.as_ref() != Some(&key) {
                            if profile.fingerprint.is_some() {
                                return Err(ConnectError::Failure(io::Error::new(
                                    io::ErrorKind::PermissionDenied,
                                    "SSH host key changed; connection refused",
                                )));
                            }
                            return Err(ConnectError::HostKey(key));
                        }
                    }
                    return Err(ConnectError::Failure(e));
                }
            }
        }
        protocol => {
            let addr = (profile.host.as_str(), profile.port)
                .to_socket_addrs()?
                .next()
                .ok_or_else(|| err("Host has no address"))?;
            let socket = TcpStream::connect_timeout(&addr, TIMEOUT)?;
            socket.set_read_timeout(Some(TIMEOUT))?;
            socket.set_write_timeout(Some(TIMEOUT))?;
            let ftp = suppaftp::RustlsFtpStream::connect_with_stream(socket)
                .map_err(err)?
                .passive_stream_builder(|addr| {
                    let s = TcpStream::connect_timeout(&addr, TIMEOUT)
                        .map_err(suppaftp::FtpError::ConnectionError)?;
                    s.set_read_timeout(Some(TIMEOUT))
                        .map_err(suppaftp::FtpError::ConnectionError)?;
                    s.set_write_timeout(Some(TIMEOUT))
                        .map_err(suppaftp::FtpError::ConnectionError)?;
                    Ok(s)
                });
            let mut ftp = if protocol == Protocol::Ftps {
                let roots = rustls::RootCertStore::from_iter(
                    webpki_roots::TLS_SERVER_ROOTS.iter().cloned(),
                );
                let config = rustls::ClientConfig::builder_with_provider(Arc::new(
                    rustls::crypto::ring::default_provider(),
                ))
                .with_safe_default_protocol_versions()
                .map_err(err)?
                .with_root_certificates(roots)
                .with_no_client_auth();
                ftp.into_secure(
                    suppaftp::RustlsConnector::from(Arc::new(config)),
                    &profile.host,
                )
                .map_err(err)?
            } else {
                ftp
            };
            ftp.set_passive_nat_workaround(true);
            cancel.check()?;
            ftp.login(profile.user.as_str(), credentials.password.as_str())
                .map_err(err)?;
            ftp.transfer_type(suppaftp::types::FileType::Binary)
                .map_err(err)?;
            Box::new(Ftp {
                ftp,
                reserved: Default::default(),
            })
        }
    };
    Ok(Arc::new(Connection {
        profile,
        backend: Mutex::new(backend),
    }))
}

#[cfg(test)]
impl Connection {
    pub(crate) fn test(profile: Profile, backend: impl Backend + 'static) -> Arc<Self> {
        Arc::new(Self {
            profile,
            backend: Mutex::new(Box::new(backend)),
        })
    }
}
