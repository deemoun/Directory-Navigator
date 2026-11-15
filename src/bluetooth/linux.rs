use super::*;
use std::{
    collections::HashMap,
    fs::File,
    io::{Read, Write},
    os::fd::{AsRawFd, OwnedFd},
    sync::{mpsc, Mutex},
    time::Duration,
};
use zbus::{
    blocking::{Connection, Proxy},
    zvariant::{OwnedObjectPath, OwnedValue, Value},
};
const UUID: &str = super::SERVICE;
struct Profile {
    incoming: Mutex<mpsc::SyncSender<(OwnedFd, String)>>,
}
#[zbus::interface(name = "org.bluez.Profile1")]
impl Profile {
    fn release(&self) {}
    fn request_disconnection(&self, _device: OwnedObjectPath) {}
    fn new_connection(
        &self,
        device: OwnedObjectPath,
        fd: zbus::zvariant::OwnedFd,
        _properties: HashMap<String, OwnedValue>,
    ) -> zbus::fdo::Result<()> {
        self.incoming
            .lock()
            .map_err(|_| zbus::fdo::Error::Failed("DN Bluetooth worker stopped".into()))?
            .try_send((fd.into(), device.to_string()))
            .map_err(|_| zbus::fdo::Error::Failed("DN Bluetooth receiver is busy".into()))
    }
}
pub fn devices(cancel: &Cancel) -> io::Result<Vec<Device>> {
    super::super::obex::check(cancel)?;
    let bus = Connection::system().map_err(failure)?;
    let proxy = Proxy::new(&bus, "org.bluez", "/", "org.freedesktop.DBus.ObjectManager")
        .map_err(failure)?;
    type Objects = HashMap<OwnedObjectPath, HashMap<String, HashMap<String, OwnedValue>>>;
    let objects: Objects = proxy.call("GetManagedObjects", &()).map_err(failure)?;
    let mut out = Vec::new();
    for interfaces in objects.values() {
        if let Some(p) = interfaces.get("org.bluez.Device1") {
            if p.get("Paired").and_then(|v| bool::try_from(v).ok()) != Some(true) {
                continue;
            }
            if let Some(a) = p.get("Address").and_then(|v| <&str>::try_from(v).ok()) {
                out.push(Device {
                    address: address(a)?,
                    name: p
                        .get("Alias")
                        .and_then(|v| <&str>::try_from(v).ok())
                        .unwrap_or(a)
                        .chars()
                        .take(256)
                        .collect(),
                });
            }
        }
    }
    out.sort_by(|a, b| a.address.cmp(&b.address));
    out.truncate(256);
    Ok(out)
}
// A custom UUID has no BlueZ default channel. Explicit zero requests an
// automatically allocated RFCOMM listener; omitting it creates no listener.
fn profile_options(client: bool) -> HashMap<&'static str, Value<'static>> {
    let mut options = HashMap::from([
        ("Name", Value::from("DN Bluetooth transfers")),
        (
            "Role",
            Value::from(if client { "client" } else { "server" }),
        ),
        ("RequireAuthentication", Value::from(true)),
        ("RequireAuthorization", Value::from(false)),
        ("AutoConnect", Value::from(false)),
    ]);
    if !client {
        options.insert("Channel", Value::from(0_u16));
    }
    options
}
pub struct Listener {
    bus: Connection,
    path: String,
    incoming: mpsc::Receiver<(OwnedFd, String)>,
}
impl Listener {
    fn register(client: bool) -> io::Result<Self> {
        let bus = Connection::system().map_err(failure)?;
        let path = format!("/org/directory_navigator/obex/p{}", std::process::id());
        let (send, incoming) = mpsc::sync_channel(1);
        bus.object_server()
            .at(
                path.as_str(),
                Profile {
                    incoming: Mutex::new(send),
                },
            )
            .map_err(failure)?;
        let manager = Proxy::new(&bus, "org.bluez", "/org/bluez", "org.bluez.ProfileManager1")
            .map_err(failure)?;
        let options = profile_options(client);
        let p = zbus::zvariant::ObjectPath::try_from(path.as_str()).map_err(failure)?;
        manager
            .call::<_, _, ()>("RegisterProfile", &(p, UUID, options))
            .map_err(failure)?;
        Ok(Self {
            bus,
            path,
            incoming,
        })
    }
    pub fn new(_cancel: &Cancel) -> io::Result<Self> {
        Self::register(false)
    }
    pub fn accept(&mut self, cancel: &Cancel) -> io::Result<(Box<dyn Link>, String)> {
        self.next(cancel, None)
    }
    fn next(
        &mut self,
        cancel: &Cancel,
        deadline: Option<std::time::Instant>,
    ) -> io::Result<(Box<dyn Link>, String)> {
        loop {
            if deadline.is_some_and(|end| std::time::Instant::now() > end) {
                return Err(io::ErrorKind::TimedOut.into());
            }
            super::super::obex::check(cancel)?;
            match self.incoming.recv_timeout(Duration::from_millis(100)) {
                Ok((fd, peer)) => {
                    let file = File::from(fd);
                    let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
                    if flags < 0
                        || unsafe {
                            libc::fcntl(file.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK)
                        } < 0
                    {
                        return Err(io::Error::last_os_error());
                    }
                    return Ok((Box::new(Socket(file)), peer));
                }
                Err(mpsc::RecvTimeoutError::Timeout) => (),
                Err(e) => return Err(failure(e)),
            }
        }
    }
}
impl Drop for Listener {
    fn drop(&mut self) {
        if let Ok(p) = Proxy::new(
            &self.bus,
            "org.bluez",
            "/org/bluez",
            "org.bluez.ProfileManager1",
        ) {
            if let Ok(path) = zbus::zvariant::ObjectPath::try_from(self.path.as_str()) {
                let _: Result<(), _> = p.call("UnregisterProfile", &(path,));
            }
        }
    }
}
struct Socket(File);
fn ready(file: &File, events: i16) -> io::Result<()> {
    let mut p = libc::pollfd {
        fd: file.as_raw_fd(),
        events,
        revents: 0,
    };
    let n = unsafe { libc::poll(&mut p, 1, 100) };
    if n < 0 {
        return Err(io::Error::last_os_error());
    }
    if n == 0 {
        return Err(io::ErrorKind::TimedOut.into());
    }
    Ok(())
}
impl Read for Socket {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        ready(&self.0, libc::POLLIN)?;
        self.0.read(b)
    }
}
impl Write for Socket {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        ready(&self.0, libc::POLLOUT)?;
        self.0.write(b)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
struct Client {
    _profile: Listener,
    socket: Box<dyn Link>,
}
impl Read for Client {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        self.socket.read(b)
    }
}
impl Write for Client {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.socket.write(b)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.socket.flush()
    }
}
pub fn connect(device: &str, cancel: &Cancel) -> io::Result<Box<dyn Link>> {
    let address = address(device)?;
    let mut listener = Listener::register(true)?;
    let proxy = Proxy::new(
        &listener.bus,
        "org.bluez",
        "/",
        "org.freedesktop.DBus.ObjectManager",
    )
    .map_err(failure)?;
    type Objects = HashMap<OwnedObjectPath, HashMap<String, HashMap<String, OwnedValue>>>;
    let objects: Objects = proxy.call("GetManagedObjects", &()).map_err(failure)?;
    let path = objects
        .iter()
        .find_map(|(path, interfaces)| {
            interfaces
                .get("org.bluez.Device1")
                .filter(|p| {
                    p.get("Address")
                        .and_then(|v| <&str>::try_from(v).ok())
                        .is_some_and(|a| a.eq_ignore_ascii_case(&address))
                })
                .map(|_| path.clone())
        })
        .ok_or_else(|| failure("Pair the Bluetooth device in system settings first"))?;
    let proxy = Proxy::new(
        &listener.bus,
        "org.bluez",
        path.as_str(),
        "org.bluez.Device1",
    )
    .map_err(failure)?;
    proxy
        .call::<_, _, ()>("ConnectProfile", &(UUID,))
        .map_err(failure)?;
    drop(proxy);
    let (socket, peer) = listener.next(
        cancel,
        Some(std::time::Instant::now() + Duration::from_secs(30)),
    )?;
    if peer != path.as_str() {
        return Err(failure(
            "Bluetooth connection came from another paired device",
        ));
    }
    Ok(Box::new(Client {
        _profile: listener,
        socket,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn custom_server_explicitly_requests_auto_rfcomm_channel() {
        let server = profile_options(false);
        assert_eq!(server.get("Channel"), Some(&Value::from(0_u16)));
        assert_eq!(server.get("Role"), Some(&Value::from("server")));
        assert!(!profile_options(true).contains_key("Channel"));
        assert_eq!(
            server.get("RequireAuthentication"),
            Some(&Value::from(true))
        );
    }
}
