use super::*;
use std::{
    io::{Read, Write},
    mem::size_of,
    sync::OnceLock,
    time::{Duration, Instant},
};
use windows::{
    core::{s, w, BOOL, GUID, PCWSTR, PSTR, PWSTR},
    Win32::{
        Devices::Bluetooth::*,
        Foundation::{FreeLibrary, HMODULE},
        Networking::WinSock::*,
        System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32},
    },
};
const SERVICE: GUID = GUID::from_u128(u128::from_be_bytes(super::SERVICE_BYTES));
fn last() -> io::Error {
    io::Error::from_raw_os_error(unsafe { WSAGetLastError() }.0)
}
fn success(code: i32) -> io::Result<()> {
    if code == SOCKET_ERROR {
        Err(last())
    } else {
        Ok(())
    }
}
fn startup() -> io::Result<()> {
    static START: OnceLock<i32> = OnceLock::new();
    let code = *START.get_or_init(|| {
        let mut data = WSADATA::default();
        unsafe { WSAStartup(0x0202, &mut data) }
    });
    if code == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(code))
    }
}
struct Socket(SOCKET);
impl Socket {
    fn new() -> io::Result<Self> {
        startup()?;
        let socket = Self(
            unsafe {
                WSASocketW(
                    AF_BTH as i32,
                    SOCK_STREAM.0,
                    BTHPROTO_RFCOMM as i32,
                    None,
                    0,
                    0,
                )
            }
            .map_err(failure)?,
        );
        for option in [SO_BTH_AUTHENTICATE, SO_BTH_ENCRYPT] {
            success(unsafe {
                setsockopt(
                    socket.0,
                    SOL_RFCOMM as i32,
                    option as i32,
                    Some(&1_u32.to_ne_bytes()),
                )
            })?;
        }
        let mut mode = 1;
        success(unsafe { ioctlsocket(socket.0, FIONBIO, &mut mode) })?;
        Ok(socket)
    }
    fn wait(&self, events: WSAPOLL_EVENT_FLAGS) -> io::Result<()> {
        let mut fds = [WSAPOLLFD {
            fd: self.0,
            events,
            revents: Default::default(),
        }];
        match unsafe { WSAPoll(fds.as_mut_ptr(), 1, 100) } {
            SOCKET_ERROR => Err(last()),
            0 => Err(io::ErrorKind::TimedOut.into()),
            _ => Ok(()),
        }
    }
}
impl Drop for Socket {
    fn drop(&mut self) {
        unsafe {
            closesocket(self.0);
        }
    }
}
impl Read for Socket {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        self.wait(POLLRDNORM)?;
        let n = unsafe { recv(self.0, b, SEND_RECV_FLAGS(0)) };
        if n == SOCKET_ERROR {
            Err(last())
        } else {
            Ok(n as usize)
        }
    }
}
impl Write for Socket {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.wait(POLLWRNORM)?;
        let n = unsafe { send(self.0, b, SEND_RECV_FLAGS(0)) };
        if n == SOCKET_ERROR {
            Err(last())
        } else {
            Ok(n as usize)
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
// Optional on Windows Server/minimal installations. Keeping the DLL out of
// the import table lets DN start even when Bluetooth is not installed.
struct Library(HMODULE);
impl Drop for Library {
    fn drop(&mut self) {
        let _ = unsafe { FreeLibrary(self.0) };
    }
}
type First = unsafe extern "system" fn(
    *const BLUETOOTH_DEVICE_SEARCH_PARAMS,
    *mut BLUETOOTH_DEVICE_INFO,
) -> HBLUETOOTH_DEVICE_FIND;
type Next = unsafe extern "system" fn(HBLUETOOTH_DEVICE_FIND, *mut BLUETOOTH_DEVICE_INFO) -> BOOL;
type Close = unsafe extern "system" fn(HBLUETOOTH_DEVICE_FIND) -> BOOL;
struct Enumeration {
    _library: Library,
    first: First,
    next: Next,
    close: Close,
}
impl Enumeration {
    fn load(name: PCWSTR) -> io::Result<Self> {
        let library = Library(
            unsafe { LoadLibraryExW(name, None, LOAD_LIBRARY_SEARCH_SYSTEM32) }.map_err(|e| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("Bluetooth system component is unavailable: {e}"),
                )
            })?,
        );
        let first = unsafe { GetProcAddress(library.0, s!("BluetoothFindFirstDevice")) }
            .ok_or_else(|| failure("Bluetooth enumeration API is unavailable"))?;
        let next = unsafe { GetProcAddress(library.0, s!("BluetoothFindNextDevice")) }
            .ok_or_else(|| failure("Bluetooth enumeration API is unavailable"))?;
        let close = unsafe { GetProcAddress(library.0, s!("BluetoothFindDeviceClose")) }
            .ok_or_else(|| failure("Bluetooth enumeration API is unavailable"))?;
        // These fixed system exports have the signatures declared in bluetoothapis.h.
        Ok(Self {
            _library: library,
            first: unsafe {
                std::mem::transmute::<unsafe extern "system" fn() -> isize, First>(first)
            },
            next: unsafe {
                std::mem::transmute::<unsafe extern "system" fn() -> isize, Next>(next)
            },
            close: unsafe {
                std::mem::transmute::<unsafe extern "system" fn() -> isize, Close>(close)
            },
        })
    }
}
struct Search<'a> {
    handle: HBLUETOOTH_DEVICE_FIND,
    api: &'a Enumeration,
}
impl Drop for Search<'_> {
    fn drop(&mut self) {
        let _ = unsafe { (self.api.close)(self.handle) };
    }
}
pub fn devices(cancel: &Cancel) -> io::Result<Vec<Device>> {
    super::super::obex::check(cancel)?;
    let search = BLUETOOTH_DEVICE_SEARCH_PARAMS {
        dwSize: size_of::<BLUETOOTH_DEVICE_SEARCH_PARAMS>() as u32,
        fReturnAuthenticated: true.into(),
        fReturnRemembered: false.into(),
        fReturnUnknown: false.into(),
        fReturnConnected: false.into(),
        fIssueInquiry: false.into(),
        ..Default::default()
    };
    let mut info = BLUETOOTH_DEVICE_INFO {
        dwSize: size_of::<BLUETOOTH_DEVICE_INFO>() as u32,
        ..Default::default()
    };
    let api = Enumeration::load(w!("bthprops.cpl"))?;
    let handle = unsafe { (api.first)(&search, &mut info) };
    if handle.0.is_null() {
        let error = io::Error::last_os_error();
        return if error.raw_os_error() == Some(259) {
            Ok(vec![])
        } else {
            Err(error)
        };
    }
    let search = Search { handle, api: &api };
    let mut out = Vec::new();
    loop {
        let value = unsafe { info.Address.Anonymous.ullLong };
        let address = (0..6)
            .rev()
            .map(|i| format!("{:02X}", (value >> (8 * i)) & 255))
            .collect::<Vec<_>>()
            .join(":");
        let n = info
            .szName
            .iter()
            .position(|c| *c == 0)
            .unwrap_or(info.szName.len());
        out.push(Device {
            address,
            name: String::from_utf16_lossy(&info.szName[..n]),
        });
        if out.len() == 256 {
            break;
        }
        super::super::obex::check(cancel)?;
        if !unsafe { (api.next)(search.handle, &mut info) }.as_bool() {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(259) {
                break;
            }
            return Err(error);
        }
    }
    Ok(out)
}
pub fn connect(value: &str, cancel: &Cancel) -> io::Result<Box<dyn Link>> {
    let normalized = address(value)?;
    if !devices(cancel)?.iter().any(|d| d.address == normalized) {
        return Err(failure(
            "Pair the Bluetooth device in system settings first",
        ));
    }
    let number = u64::from_str_radix(&normalized.replace(':', ""), 16).map_err(failure)?;
    let addr = SOCKADDR_BTH {
        addressFamily: AF_BTH,
        btAddr: number,
        serviceClassId: SERVICE,
        port: 0,
    };
    let socket = Socket::new()?;
    let code = unsafe {
        windows::Win32::Networking::WinSock::connect(
            socket.0,
            (&addr as *const SOCKADDR_BTH).cast(),
            size_of::<SOCKADDR_BTH>() as i32,
        )
    };
    if code == SOCKET_ERROR {
        let error = last();
        if error.raw_os_error() != Some(WSAEWOULDBLOCK.0) {
            return Err(error);
        }
        let end = Instant::now() + Duration::from_secs(30);
        loop {
            super::super::obex::check(cancel)?;
            if Instant::now() > end {
                return Err(io::ErrorKind::TimedOut.into());
            }
            match socket.wait(POLLWRNORM) {
                Ok(()) => break,
                Err(e) if e.kind() == io::ErrorKind::TimedOut => (),
                Err(e) => return Err(e),
            }
        }
        let mut error = 0_i32;
        let mut len = 4;
        success(unsafe {
            getsockopt(
                socket.0,
                SOL_SOCKET,
                SO_ERROR,
                PSTR((&mut error as *mut i32).cast()),
                &mut len,
            )
        })?;
        if error != 0 {
            return Err(io::Error::from_raw_os_error(error));
        }
    }
    Ok(Box::new(socket))
}
pub struct Listener {
    socket: Socket,
    local: SOCKADDR_BTH,
}
impl Listener {
    fn service(&mut self, operation: WSAESETSERVICEOP) -> io::Result<()> {
        let mut name = "DN Bluetooth transfers\0"
            .encode_utf16()
            .collect::<Vec<_>>();
        let mut id = SERVICE;
        let mut addresses = CSADDR_INFO {
            LocalAddr: SOCKET_ADDRESS {
                lpSockaddr: (&mut self.local as *mut SOCKADDR_BTH).cast(),
                iSockaddrLength: size_of::<SOCKADDR_BTH>() as i32,
            },
            iSocketType: SOCK_STREAM.0,
            iProtocol: BTHPROTO_RFCOMM as i32,
            ..Default::default()
        };
        let query = WSAQUERYSETW {
            dwSize: size_of::<WSAQUERYSETW>() as u32,
            lpszServiceInstanceName: PWSTR(name.as_mut_ptr()),
            lpServiceClassId: &mut id,
            dwNameSpace: NS_BTH,
            dwNumberOfCsAddrs: 1,
            lpcsaBuffer: &mut addresses,
            ..Default::default()
        };
        success(unsafe { WSASetServiceW(&query, operation, 0) })
    }
    pub fn new(_cancel: &Cancel) -> io::Result<Self> {
        let socket = Socket::new()?;
        let mut local = SOCKADDR_BTH {
            addressFamily: AF_BTH,
            btAddr: 0,
            serviceClassId: GUID::zeroed(),
            port: u32::MAX,
        };
        success(unsafe {
            bind(
                socket.0,
                (&local as *const SOCKADDR_BTH).cast(),
                size_of::<SOCKADDR_BTH>() as i32,
            )
        })?;
        let mut n = size_of::<SOCKADDR_BTH>() as i32;
        success(unsafe {
            getsockname(socket.0, (&mut local as *mut SOCKADDR_BTH).cast(), &mut n)
        })?;
        success(unsafe { listen(socket.0, 1) })?;
        let mut listener = Self { socket, local };
        listener.service(RNRSERVICE_REGISTER)?;
        Ok(listener)
    }
    pub fn accept(&mut self, cancel: &Cancel) -> io::Result<(Box<dyn Link>, String)> {
        loop {
            super::super::obex::check(cancel)?;
            match self.socket.wait(POLLRDNORM) {
                Ok(()) => (),
                Err(e) if e.kind() == io::ErrorKind::TimedOut => continue,
                Err(e) => return Err(e),
            }
            let mut peer = SOCKADDR_BTH::default();
            let mut length = size_of::<SOCKADDR_BTH>() as i32;
            let value = unsafe {
                accept(
                    self.socket.0,
                    Some((&mut peer as *mut SOCKADDR_BTH).cast()),
                    Some(&mut length),
                )
            }
            .map_err(failure)?;
            let address = (0..6)
                .rev()
                .map(|i| format!("{:02X}", (peer.btAddr >> (8 * i)) & 255))
                .collect::<Vec<_>>()
                .join(":");
            let mut mode = 1;
            let socket = Socket(value);
            success(unsafe { ioctlsocket(socket.0, FIONBIO, &mut mode) })?;
            return Ok((Box::new(socket), address));
        }
    }
}
impl Drop for Listener {
    fn drop(&mut self) {
        let _ = self.service(RNRSERVICE_DELETE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_bluetooth_component_is_recoverable() {
        let result = Enumeration::load(w!("dn-bluetooth-nonexistent-test-component.cpl"));
        let error = result
            .err()
            .expect("Missing optional library must be an ordinary error");
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(error
            .to_string()
            .contains("Bluetooth system component is unavailable"));
    }
    #[test]
    fn missing_bluetooth_exports_are_recoverable() {
        assert!(Enumeration::load(w!("kernel32.dll")).is_err());
    }
}
