//! IOBluetooth objects stay on the worker and its CFRunLoop, never the UI thread.
use super::*;
use objc2::{define_class, msg_send, rc::Retained, sel, AnyThread, DefinedClass};
use objc2_core_foundation::{kCFRunLoopDefaultMode, CFRunLoop};
use objc2_foundation::{
    NSData, NSDictionary, NSObject, NSObjectProtocol, NSPropertyListMutabilityOptions,
    NSPropertyListSerialization, NSString,
};
use objc2_io_bluetooth::*;
use std::{
    cell::RefCell,
    collections::VecDeque,
    ffi::c_void,
    io::{Read, Write},
    time::{Duration, Instant},
};
#[derive(Default)]
struct Events {
    bytes: VecDeque<u8>,
    closed: bool,
    overflow: bool,
    incoming: VecDeque<Socket>,
    sdp: bool,
    opened: Option<i32>,
}
define_class!(
    #[unsafe(super(NSObject))]
    #[name="DNBluetoothOBEXDelegate"]
    #[ivars=RefCell<Events>]
    struct Delegate;
    unsafe impl NSObjectProtocol for Delegate {}
    unsafe impl IOBluetoothRFCOMMChannelDelegate for Delegate {
        #[unsafe(method(rfcommChannelData:data:length:))]
        unsafe fn data(&self,_channel:Option<&IOBluetoothRFCOMMChannel>,data:*mut c_void,length:usize) {
            let mut events=self.ivars().borrow_mut();
            if length>256*1024 || events.bytes.len()+length>256*1024 || (data.is_null()&&length!=0) {events.overflow=true;return;}
            if length!=0 {events.bytes.extend(unsafe{std::slice::from_raw_parts(data.cast::<u8>(),length)});}
        }
        #[unsafe(method(rfcommChannelOpenComplete:status:))]
        unsafe fn open_complete(&self,_channel:Option<&IOBluetoothRFCOMMChannel>,status:i32) {self.ivars().borrow_mut().opened=Some(status);}
        #[unsafe(method(rfcommChannelClosed:))]
        unsafe fn closed(&self,_channel:Option<&IOBluetoothRFCOMMChannel>) {self.ivars().borrow_mut().closed=true;}
    }
    impl Delegate {
        #[unsafe(method(channelOpened:channel:))]
        unsafe fn opened(&self,_notice:Option<&IOBluetoothUserNotification>,channel:Option<&IOBluetoothRFCOMMChannel>) {
            if let Some(channel)=channel {
                let mut events=self.ivars().borrow_mut();
                if events.incoming.is_empty() && unsafe{channel.getDevice()}.is_some_and(|d|unsafe{d.isPaired()&&d.getEncryptionMode()!=0}) {
                    let delegate=Self::new();
                    let status=unsafe{channel.setDelegate(Some(&delegate))};
                    if status==0 {if let Some(channel)=unsafe{Retained::retain(channel as *const _ as *mut IOBluetoothRFCOMMChannel)} {events.incoming.push_back(Socket{channel,delegate,cancel:std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false))});return;}}
                }
                unsafe{channel.closeChannel();}
            }
        }
        #[unsafe(method(sdpQueryComplete:status:))]
        unsafe fn sdp_done(&self,_device:Option<&IOBluetoothDevice>,_status:i32) {self.ivars().borrow_mut().sdp=true;}
    }
);
impl Delegate {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(RefCell::new(Events::default()));
        unsafe { msg_send![super(this), init] }
    }
}
fn pump() {
    CFRunLoop::run_in_mode(unsafe { kCFRunLoopDefaultMode }, 0.02, true);
}
fn status(code: i32) -> io::Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(failure(format!("Bluetooth system operation failed ({code:#x}); check Bluetooth permissions and pairing")))
    }
}
pub fn devices(cancel: &Cancel) -> io::Result<Vec<Device>> {
    super::super::obex::check(cancel)?;
    let controller = unsafe { IOBluetoothHostController::defaultController() }
        .ok_or_else(|| failure("No Bluetooth Classic controller is available"))?;
    if unsafe { controller.powerState() } != BluetoothHCIPowerState::ON {
        return Err(failure(
            "Bluetooth is off or unavailable; enable it in system settings",
        ));
    }
    let mut out = Vec::new();
    if let Some(devices) = unsafe { IOBluetoothDevice::pairedDevices() } {
        for object in devices.iter().take(256) {
            if let Some(d) = object.downcast_ref::<IOBluetoothDevice>() {
                if let Some(a) = unsafe { d.addressString() } {
                    out.push(Device {
                        address: address(&a.to_string())?,
                        name: unsafe { d.nameOrAddress() }
                            .map_or_else(|| a.to_string(), |n| n.to_string())
                            .chars()
                            .take(256)
                            .collect(),
                    });
                }
            }
        }
    }
    Ok(out)
}
struct Socket {
    channel: Retained<IOBluetoothRFCOMMChannel>,
    delegate: Retained<Delegate>,
    cancel: Cancel,
}
impl Drop for Socket {
    fn drop(&mut self) {
        unsafe {
            self.channel.closeChannel();
            self.channel.setDelegate(None);
        }
    }
}
impl Read for Socket {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        if b.is_empty() {
            return Ok(0);
        }
        super::super::obex::check(&self.cancel)?;
        pump();
        let mut e = self.delegate.ivars().borrow_mut();
        if e.overflow {
            return Err(failure("Bluetooth input buffer exceeded its limit"));
        }
        let n = b.len().min(e.bytes.len());
        for value in &mut b[..n] {
            *value = e.bytes.pop_front().unwrap();
        }
        if n == 0 && !e.closed {
            return Err(io::ErrorKind::TimedOut.into());
        }
        Ok(n)
    }
}
impl Write for Socket {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        if b.is_empty() {
            return Ok(0);
        }
        super::super::obex::check(&self.cancel)?;
        let n = b
            .len()
            .min(usize::from(unsafe { self.channel.getMTU() }))
            .min(u16::MAX as usize);
        if n == 0 {
            return Err(failure("Bluetooth channel has no writable capacity"));
        }
        // This synchronous, non-sleeping API consumes the borrowed buffer before
        // returning and never waits for queue capacity. The modern async API
        // would require retaining that buffer beyond cancellation/close.
        let mut bytes = b[..n].to_vec();
        #[allow(deprecated)]
        let result = unsafe {
            self.channel
                .write_length_sleep(bytes.as_mut_ptr().cast(), n as u16, false)
        };
        // IOKit kIOReturnBusy / kIOReturnNoSpace: no bytes were accepted.
        if matches!(result as u32, 0xe000_02d5 | 0xe000_02db) {
            pump();
            return Err(io::ErrorKind::WouldBlock.into());
        }
        status(result)?;
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
pub fn connect(value: &str, cancel: &Cancel) -> io::Result<Box<dyn Link>> {
    let address = address(value)?;
    let device =
        unsafe { IOBluetoothDevice::deviceWithAddressString(Some(&NSString::from_str(&address))) }
            .ok_or_else(|| failure("Bluetooth device is unavailable"))?;
    if !unsafe { device.isPaired() } {
        return Err(failure(
            "Pair the Bluetooth device in system settings first",
        ));
    }
    let delegate = Delegate::new();
    status(unsafe { device.performSDPQuery(Some(&delegate)) })?;
    let end = Instant::now() + Duration::from_secs(20);
    while !delegate.ivars().borrow().sdp {
        super::super::obex::check(cancel)?;
        if Instant::now() > end {
            return Err(io::ErrorKind::TimedOut.into());
        }
        pump();
    }
    let uuid = unsafe {
        IOBluetoothSDPUUID::uuidWithData(Some(&NSData::with_bytes(&super::SERVICE_BYTES)))
    }
    .ok_or_else(|| failure("Cannot create Object Push service identifier"))?;
    let service = unsafe { device.getServiceRecordForUUID(Some(&uuid)) }.ok_or_else(|| {
        failure(
            "The device is not advertising DN Bluetooth transfers; start Receive on the other DN",
        )
    })?;
    let mut id = 0;
    status(unsafe { service.getRFCOMMChannelID(&mut id) })?;
    let mut channel = None;
    let opened = unsafe {
        device.openRFCOMMChannelAsync_withChannelID_delegate(
            Some(&mut channel),
            id,
            Some(&delegate),
        )
    };
    if opened != 0 {
        if let Some(channel) = &channel {
            unsafe {
                channel.closeChannel();
            }
        }
        status(opened)?;
    }
    let channel = channel.ok_or_else(|| failure("Bluetooth channel did not open"))?;
    let socket = Socket {
        channel,
        delegate,
        cancel: cancel.clone(),
    };
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        super::super::obex::check(cancel)?;
        if let Some(result) = socket.delegate.ivars().borrow().opened {
            status(result)?;
            break;
        }
        if socket.delegate.ivars().borrow().closed {
            return Err(failure("Bluetooth channel closed while connecting"));
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Bluetooth connection timed out",
            ));
        }
        pump();
    }
    if unsafe { device.getEncryptionMode() } == 0 {
        return Err(failure(
            "Bluetooth connection is not encrypted; reconnect after system pairing",
        ));
    }
    Ok(Box::new(socket))
}
struct Published(Retained<IOBluetoothSDPServiceRecord>);
impl Drop for Published {
    fn drop(&mut self) {
        unsafe {
            self.0.removeServiceRecord();
        }
    }
}
pub struct Listener {
    _notice: Retained<IOBluetoothUserNotification>,
    _service: Published,
    delegate: Retained<Delegate>,
}
impl Listener {
    pub fn new(_cancel: &Cancel) -> io::Result<Self> {
        let xml = r#"<?xml version="1.0"?><plist version="1.0"><dict>
<key>0001</key><array><data>jUP1Cf1FTlmkKEECA9WtcQ==</data><data>EQU=</data></array>
<key>0004</key><array><array><data>AQA=</data></array><array><data>AAM=</data><dict><key>DataElementType</key><integer>1</integer><key>DataElementSize</key><integer>1</integer><key>DataElementValue</key><integer>1</integer></dict></array></array>
<key>0005</key><array><data>EAI=</data></array>
<key>0009</key><array><array><data>EQU=</data><dict><key>DataElementType</key><integer>1</integer><key>DataElementSize</key><integer>2</integer><key>DataElementValue</key><integer>258</integer></dict></array></array>
<key>0100</key><string>DN Bluetooth transfers</string>
<key>0303</key><array><dict><key>DataElementType</key><integer>1</integer><key>DataElementSize</key><integer>1</integer><key>DataElementValue</key><integer>255</integer></dict></array>
</dict></plist>"#;
        let plist = unsafe {
            NSPropertyListSerialization::propertyListWithData_options_format_error(
                &NSData::with_bytes(xml.as_bytes()),
                NSPropertyListMutabilityOptions(0),
                std::ptr::null_mut(),
            )
        }
        .map_err(failure)?;
        let dictionary = plist
            .downcast_ref::<NSDictionary>()
            .ok_or_else(|| failure("Invalid Bluetooth service description"))?;
        let service=unsafe{IOBluetoothSDPServiceRecord::publishedServiceRecordWithDictionary(Some(dictionary))}.ok_or_else(||failure("Cannot advertise Bluetooth Object Push; check Bluetooth access and disable competing receive services"))?;
        let service = Published(service);
        let mut id = 0;
        status(unsafe { service.0.getRFCOMMChannelID(&mut id) })?;
        let delegate = Delegate::new();
        let notice=unsafe{IOBluetoothRFCOMMChannel::registerForChannelOpenNotifications_selector_withChannelID_direction(Some(&delegate),Some(sel!(channelOpened:channel:)),id,IOBluetoothUserNotificationChannelDirection::Incoming)}.ok_or_else(||failure("Cannot listen for Bluetooth connections"))?;
        Ok(Self {
            _notice: notice,
            _service: service,
            delegate,
        })
    }
    pub fn accept(&mut self, cancel: &Cancel) -> io::Result<(Box<dyn Link>, String)> {
        loop {
            super::super::obex::check(cancel)?;
            pump();
            if let Some(mut socket) = self.delegate.ivars().borrow_mut().incoming.pop_front() {
                socket.cancel = cancel.clone();
                let name = unsafe { socket.channel.getDevice() }
                    .and_then(|d| unsafe { d.nameOrAddress() })
                    .map_or_else(|| "Bluetooth device".into(), |s| s.to_string());
                return Ok((Box::new(socket), name));
            }
        }
    }
}
impl Drop for Listener {
    fn drop(&mut self) {
        unsafe {
            self._notice.unregister();
        }
    }
}
