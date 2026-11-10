use std::path::PathBuf;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    System,
    Memory,
    Network,
}
impl Kind {
    pub fn title(self) -> &'static str {
        match self {
            Self::System => " System Information ",
            Self::Memory => " Memory Information ",
            Self::Network => " Network Information ",
        }
    }
}
pub type Fields = Vec<(String, String)>;
struct Reply {
    fields: Fields,
    network: Option<crate::network::information::Snapshot>,
}
pub struct Information {
    pub(crate) requests: crate::requests::RequestScope,
    pub kind: Kind,
    pub fields: Fields,
    #[cfg(any(target_os = "linux", test))]
    pub(crate) terminal_fields: Fields,
    pub offset: usize,
    pub horizontal: u16,
    pub rows: usize,
    pub network: Option<crate::network::information::Snapshot>,
    pub selected_interface: usize,
    pub interface_choice: Option<usize>,
    pub interface_epoch: u64,
    pub traffic: crate::network::traffic::Monitor,
    preferred_interface: Option<crate::network::information::InterfaceKey>,
    pending: Option<crate::task::Task<Reply>>,
}
impl Information {
    pub fn open(kind: Kind, path: PathBuf, size: (u16, u16)) -> std::io::Result<Self> {
        Self::open_with_config(kind, path, size, None)
    }
    pub(crate) fn open_with_config(
        kind: Kind,
        path: PathBuf,
        size: (u16, u16),
        config: Option<PathBuf>,
    ) -> std::io::Result<Self> {
        let requests = crate::requests::RequestScope::default();
        let receive = requests.spawn("dn-information", move |_| {
            let network = (kind == Kind::Network).then(crate::network::information::snapshot);
            let mut fields = if let Some(network) = &network {
                network.details(0)
            } else {
                collect(kind, path, size)
            };
            if kind == Kind::System {
                fields.extend(crate::network::information::paths(config.as_deref()));
            }
            Reply { fields, network }
        })?;
        Ok(Self {
            requests,
            kind,
            fields: Vec::new(),
            #[cfg(any(target_os = "linux", test))]
            terminal_fields: Vec::new(),
            offset: 0,
            horizontal: 0,
            rows: 10,
            network: None,
            selected_interface: 0,
            interface_choice: None,
            interface_epoch: 0,
            traffic: Default::default(),
            preferred_interface: None,
            pending: Some(receive),
        })
    }
    pub fn open_interfaces(&mut self) -> bool {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        if self.network.as_ref().is_none_or(|n| n.adapters.is_empty()) {
            return false;
        }
        self.interface_epoch = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.interface_choice = Some(self.selected_interface);
        true
    }
    pub fn selected_key(&self) -> Option<crate::network::information::InterfaceKey> {
        self.network
            .as_ref()?
            .adapters
            .get(self.selected_interface)
            .map(|a| a.key.clone())
    }
    pub fn retain_interface(&mut self, previous: &Self) {
        self.preferred_interface = previous.selected_key();
    }
    pub fn choose_interface(&mut self, index: usize) {
        if let Some(network) = &self.network {
            if index < network.adapters.len() {
                self.selected_interface = index;
                self.fields = network.details(index);
                self.offset = 0;
                self.horizontal = 0;
            }
        }
        self.traffic.select(self.selected_key());
        self.sync_traffic_fields();
        self.interface_choice = None;
    }
    fn sync_traffic_fields(&mut self) {
        if self.kind != Kind::Network {
            return;
        }
        self.fields.retain(|(name, _)| name != "Traffic counters");
        if let crate::network::traffic::State::Unavailable(error) = self.traffic.state() {
            self.fields.push(("Traffic counters".into(), error.clone()));
        }
    }
    pub fn step_interface(&mut self, amount: isize) {
        if let (Some(choice), Some(network)) = (&mut self.interface_choice, &self.network) {
            *choice = choice
                .saturating_add_signed(amount)
                .min(network.adapters.len().saturating_sub(1));
        }
    }
    pub fn loading(&self) -> bool {
        self.pending.is_some()
    }
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        let reply = crate::task::take_reply(&mut self.pending).map(|reply| {
            reply.unwrap_or_else(|_| Reply {
                fields: vec![(
                    "Snapshot".into(),
                    "Unavailable: information worker stopped".into(),
                )],
                network: None,
            })
        });
        if let Some(reply) = reply {
            self.fields = reply.fields;
            self.network = reply.network;
            if let Some(network) = &self.network {
                self.selected_interface = self
                    .preferred_interface
                    .as_ref()
                    .and_then(|key| {
                        network
                            .adapters
                            .iter()
                            .position(|adapter| &adapter.key == key)
                    })
                    .unwrap_or(0);
                self.fields = network.details(self.selected_interface);
                self.traffic.select(self.selected_key());
            }
            #[cfg(any(target_os = "linux", test))]
            self.fields.extend(self.terminal_fields.clone());
            changed = true;
        }
        if self.kind == Kind::Network
            && self.traffic.poll(&self.requests, std::time::Instant::now())
        {
            self.sync_traffic_fields();
            self.step(0);
            changed = true;
        }
        changed
    }
    #[cfg(any(target_os = "linux", test))]
    pub(crate) fn set_terminal_fields(&mut self, fields: Fields) {
        if self.kind != Kind::System || self.terminal_fields == fields {
            return;
        }
        let old_fields = std::mem::replace(&mut self.terminal_fields, fields);
        if !self.loading() {
            self.fields.retain(|(key, _)| {
                !old_fields
                    .iter()
                    .chain(&self.terminal_fields)
                    .any(|(name, _)| name == key)
            });
            self.fields.extend(self.terminal_fields.clone());
        }
    }
    pub fn step(&mut self, amount: isize) {
        self.offset = self
            .offset
            .saturating_add_signed(amount)
            .min(self.fields.len().saturating_sub(self.rows));
    }
}
fn field(fields: &mut Fields, label: &str, value: Result<String, String>) {
    fields.push((
        label.into(),
        value.unwrap_or_else(|e| format!("Unavailable: {e}")),
    ));
}
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn bytes(n: u64) -> String {
    format!("{n} bytes ({:.2} MiB)", n as f64 / 1048576.0)
}
fn environment(key: &str) -> Result<String, String> {
    std::env::var_os(key)
        .map(|s| s.to_string_lossy().into_owned())
        .ok_or_else(|| format!("{key} is not set"))
}
pub fn collect(kind: Kind, path: PathBuf, size: (u16, u16)) -> Fields {
    if kind == Kind::Network {
        return crate::network::information::collect();
    }
    let mut fields = Vec::new();
    if kind == Kind::Memory {
        platform::memory(&mut fields);
        return fields;
    }
    field(
        &mut fields,
        "Application",
        Ok(format!(
            "{} {}",
            crate::APPLICATION_NAME,
            crate::DISPLAY_VERSION
        )),
    );
    field(
        &mut fields,
        "Operating system",
        Ok(std::env::consts::OS.into()),
    );
    field(&mut fields, "OS version", platform::version());
    field(
        &mut fields,
        "Architecture",
        Ok(std::env::consts::ARCH.into()),
    );
    field(&mut fields, "Hostname", platform::hostname());
    field(
        &mut fields,
        "Logical CPUs",
        std::thread::available_parallelism()
            .map(|n| n.to_string())
            .map_err(|e| e.to_string()),
    );
    field(
        &mut fields,
        "Process ID",
        Ok(std::process::id().to_string()),
    );
    field(&mut fields, "Terminal", environment("TERM"));
    #[cfg(target_os = "linux")]
    fields.extend(linux_diagnostics(
        crate::plugins::music_playback_available(),
        |fd| {
            std::fs::read_link(format!("/proc/self/fd/{fd}"))
                .map(|path| path.to_string_lossy().into_owned())
                .map_err(|error| error.to_string())
        },
    ));
    #[cfg(target_os = "linux")]
    fields.extend(crate::terminal::unicode::collect());
    field(
        &mut fields,
        "Terminal size",
        Ok(format!("{} × {}", size.0, size.1)),
    );
    field(
        &mut fields,
        "Configured shell",
        environment(if cfg!(windows) { "COMSPEC" } else { "SHELL" }),
    );
    field(
        &mut fields,
        "Active directory",
        Ok(path.to_string_lossy().into_owned()),
    );
    fields
}

#[cfg(any(target_os = "linux", test))]
fn linux_diagnostics(audio: bool, mut device: impl FnMut(u8) -> Result<String, String>) -> Fields {
    let mut fields = Vec::new();
    field(&mut fields, "Terminal input", device(0));
    field(&mut fields, "Terminal output", device(1));
    fields.push((
        "Audio support".into(),
        if audio {
            "Included"
        } else {
            "Not included in this build"
        }
        .into(),
    ));
    fields.push((
        "Audio backend".into(),
        if audio {
            "ALSA (system default)"
        } else {
            "Unavailable without audio support"
        }
        .into(),
    ));
    fields.push((
        "Audio device".into(),
        if audio {
            "Not probed; opened only on Play"
        } else {
            "Unavailable without audio support"
        }
        .into(),
    ));
    fields
}

/// Translate status vocabulary, retaining diagnostic paths and native errors verbatim.
pub(crate) fn display_value(
    language: crate::localization::Language,
    name: &str,
    value: &str,
) -> String {
    if matches!(
        name,
        "Internet reachability"
            | "Network interfaces"
            | "Address protocols"
            | "Link state"
            | "MAC address"
            | "Default gateway"
            | "MTU"
            | "Traffic counters"
            | "Mouse input"
            | "GPM status"
            | "Terminal mode"
            | "Color output"
            | "Audio support"
            | "Audio backend"
            | "Audio device"
            | "DN text output"
            | "Console display decoding"
            | "Keyboard mode"
            | "Russian alphabet mappings"
            | "Console frame mappings"
    ) {
        if let Some((prefix, detail)) = value.split_once(": ") {
            format!("{}: {detail}", language.text(prefix))
        } else {
            language.text(value).to_owned()
        }
    } else if let Some(detail) = value
        .strip_prefix("Unavailable: ")
        .filter(|_| matches!(name, "Terminal input" | "Terminal output"))
    {
        format!("{}: {detail}", language.text("Unavailable"))
    } else {
        value.into()
    }
}

#[cfg(test)]
mod diagnostics_tests {
    use super::*;
    use crate::localization::Language;

    #[test]
    fn linux_device_and_audio_fields_do_not_claim_a_device_was_tested() {
        for audio in [false, true] {
            let mut descriptors = Vec::new();
            let fields = linux_diagnostics(audio, |fd| {
                descriptors.push(fd);
                if fd == 0 {
                    Ok("/dev/tty2".into())
                } else {
                    Err("Permission denied: fixture".into())
                }
            });
            assert_eq!(descriptors, [0, 1]);
            assert_eq!(fields[0], ("Terminal input".into(), "/dev/tty2".into()));
            assert_eq!(
                fields[1],
                (
                    "Terminal output".into(),
                    "Unavailable: Permission denied: fixture".into()
                )
            );
            assert_eq!(
                fields[2].1,
                if audio {
                    "Included"
                } else {
                    "Not included in this build"
                }
            );
            assert_eq!(
                fields[3].1,
                if audio {
                    "ALSA (system default)"
                } else {
                    "Unavailable without audio support"
                }
            );
            assert_eq!(
                fields[4].1,
                if audio {
                    "Not probed; opened only on Play"
                } else {
                    "Unavailable without audio support"
                }
            );
        }
        assert_eq!(
            linux_diagnostics(true, |_| Ok("/dev/pts/4".into()))[0].1,
            "/dev/pts/4"
        );
    }

    #[test]
    fn live_terminal_fields_survive_worker_completion_without_duplicates_or_memory_dialog_fields(
    ) -> std::io::Result<()> {
        for kind in [Kind::System, Kind::Memory, Kind::Network] {
            let mut info = Information::open(kind, PathBuf::from("diagnostic-fixture"), (80, 25))?;
            let fields = vec![
                ("Terminal mode".into(), "Linux virtual console".into()),
                (
                    "Color output".into(),
                    "Console colors (16 foreground / 8 background)".into(),
                ),
                ("GPM status".into(), "Connected".into()),
            ];
            info.set_terminal_fields(fields.clone());
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while info.loading() {
                info.poll();
                assert!(
                    std::time::Instant::now() < deadline,
                    "information worker completion"
                );
                std::thread::yield_now();
            }
            for (name, _) in &fields {
                assert_eq!(
                    info.fields.iter().filter(|(key, _)| key == name).count(),
                    usize::from(kind == Kind::System)
                );
            }
            for reason in ["GPM disconnected", "Connected", "Connected"] {
                let mut update = fields.clone();
                update[2].1 = reason.into();
                info.set_terminal_fields(update);
                for (name, _) in &fields {
                    assert_eq!(
                        info.fields.iter().filter(|(key, _)| key == name).count(),
                        usize::from(kind == Kind::System)
                    );
                }
                if kind == Kind::System {
                    assert_eq!(
                        info.fields
                            .iter()
                            .find(|(key, _)| key == "GPM status")
                            .unwrap()
                            .1,
                        reason
                    );
                }
            }
        }
        Ok(())
    }

    #[test]
    fn diagnostic_values_localize_without_translating_paths_or_native_error_details() {
        let ru = Language::Russian;
        assert_eq!(
            display_value(ru, "Terminal mode", "Linux virtual console"),
            "Виртуальная консоль Linux"
        );
        assert_eq!(
            display_value(
                ru,
                "GPM status",
                "GPM unavailable: Permission denied: /dev/gpmctl"
            ),
            format!(
                "{}: Permission denied: /dev/gpmctl",
                ru.text("GPM unavailable")
            )
        );
        assert_eq!(
            display_value(ru, "Terminal input", "/dev/tty2"),
            "/dev/tty2"
        );
        assert_eq!(
            display_value(ru, "Terminal output", "Unavailable: fixture error"),
            "Недоступно: fixture error"
        );
        assert_eq!(
            display_value(
                ru,
                "Russian alphabet mappings",
                "Unavailable: native ioctl error"
            ),
            "Недоступно: native ioctl error"
        );
        assert_eq!(
            display_value(ru, "Russian alphabet mappings", "64/66; U+0401 U+0451"),
            "64/66; U+0401 U+0451"
        );
        assert_eq!(
            display_value(ru, "Console display decoding", "Not verified"),
            "Не проверено"
        );
        assert_eq!(
            display_value(
                Language::English,
                "Audio device",
                "Not probed; opened only on Play"
            ),
            "Not probed; opened only on Play"
        );
    }
}
/// Parse Linux's explicitly labelled KiB fields; reject malformed units/overflow.
pub fn kib_field(text: &str, key: &str) -> Result<u64, String> {
    let value = text
        .lines()
        .find_map(|line| {
            line.split_once(':')
                .filter(|(name, _)| *name == key)
                .map(|(_, value)| value)
        })
        .ok_or_else(|| format!("{key} not reported"))?;
    let mut words = value.split_whitespace();
    let n: u64 = words
        .next()
        .ok_or("Missing value")?
        .parse()
        .map_err(|_| "Invalid value")?;
    if words.next() != Some("kB") || words.next().is_some() {
        return Err("Expected Linux kB (KiB) units".into());
    }
    n.checked_mul(1024)
        .ok_or_else(|| "Byte count overflow".into())
}
#[cfg(unix)]
fn unix_hostname() -> Result<String, String> {
    let mut buffer = [0u8; 256];
    let result = unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len()) };
    if result != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let end = buffer
        .iter()
        .position(|b| *b == 0)
        .ok_or("Hostname exceeds buffer")?;
    Ok(String::from_utf8_lossy(&buffer[..end]).into_owned())
}
// libc retains these native Mach interfaces but recommends an additional crate.
// Keep their use confined here to preserve the project's dependency policy.
#[cfg(target_os = "macos")]
#[allow(deprecated)]
mod platform {
    use super::*;
    use std::{
        ffi::CStr,
        mem::{size_of, MaybeUninit},
        ptr,
    };
    fn sysctl<T: Copy>(name: &CStr) -> Result<T, String> {
        let mut output = MaybeUninit::<T>::zeroed();
        let mut size = size_of::<T>();
        // Kernel writes a fixed-size scalar/struct into an aligned output buffer.
        let result = unsafe {
            libc::sysctlbyname(
                name.as_ptr(),
                output.as_mut_ptr().cast(),
                &mut size,
                ptr::null_mut(),
                0,
            )
        };
        if result != 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        if size != size_of::<T>() {
            return Err("Unexpected native value size".into());
        }
        Ok(unsafe { output.assume_init() })
    }
    fn string(name: &CStr) -> Result<String, String> {
        let mut length = 0;
        let code = unsafe {
            libc::sysctlbyname(
                name.as_ptr(),
                ptr::null_mut(),
                &mut length,
                ptr::null_mut(),
                0,
            )
        };
        if code != 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        if length == 0 || length > 65536 {
            return Err("Invalid native string size".into());
        }
        let mut data = vec![0u8; length];
        if unsafe {
            libc::sysctlbyname(
                name.as_ptr(),
                data.as_mut_ptr().cast(),
                &mut length,
                ptr::null_mut(),
                0,
            )
        } != 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        data.truncate(length.min(data.len()));
        let end = data.iter().position(|b| *b == 0).unwrap_or(data.len());
        Ok(String::from_utf8_lossy(&data[..end]).into_owned())
    }
    pub fn hostname() -> Result<String, String> {
        unix_hostname()
    }
    pub fn version() -> Result<String, String> {
        let product = string(c"kern.osproductversion");
        let release = string(c"kern.osrelease");
        match (product, release) {
            (Ok(p), Ok(r)) => Ok(format!("macOS {p} · Darwin {r}")),
            (Err(_), Ok(r)) => Ok(format!("Darwin {r}")),
            (Err(e), Err(_)) => Err(e),
            (Ok(p), Err(_)) => Ok(format!("macOS {p}")),
        }
    }
    extern "C" {
        fn mach_port_deallocate(
            task: libc::mach_port_t,
            name: libc::mach_port_t,
        ) -> libc::kern_return_t;
    }
    struct Host(libc::mach_port_t);
    impl Drop for Host {
        fn drop(&mut self) {
            unsafe {
                mach_port_deallocate(libc::mach_task_self(), self.0);
            }
        }
    }
    pub fn memory(fields: &mut Fields) {
        field(
            fields,
            "Physical RAM",
            sysctl::<u64>(c"hw.memsize").map(bytes),
        );
        let mut vm = unsafe { std::mem::zeroed::<libc::vm_statistics64>() };
        let mut count = libc::HOST_VM_INFO64_COUNT;
        let host = Host(unsafe { libc::mach_host_self() });
        let result = unsafe {
            libc::host_statistics64(
                host.0,
                libc::HOST_VM_INFO64,
                (&mut vm as *mut libc::vm_statistics64).cast(),
                &mut count,
            )
        };
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        // Only fields in the original VM_INFO64 prefix are read (through compressor_page_count).
        let required = (std::mem::offset_of!(libc::vm_statistics64, compressor_page_count)
            + size_of::<libc::natural_t>())
            / size_of::<libc::integer_t>();
        let valid = result == libc::KERN_SUCCESS && count as usize >= required && page > 0;
        for (name, pages) in [
            ("Free pages", vm.free_count),
            ("Active pages", vm.active_count),
            ("Inactive pages", vm.inactive_count),
            ("Wired pages", vm.wire_count),
            ("Compressor pages", vm.compressor_page_count),
        ] {
            field(
                fields,
                name,
                if valid {
                    u64::from(pages)
                        .checked_mul(page as u64)
                        .map(bytes)
                        .ok_or_else(|| "Page count overflow".into())
                } else {
                    Err(format!(
                        "Mach VM query failed ({result}); returned {count} words"
                    ))
                },
            );
        }
        let swap = sysctl::<libc::xsw_usage>(c"vm.swapusage");
        field(
            fields,
            "Swap total",
            swap.as_ref()
                .map(|s| bytes(s.xsu_total))
                .map_err(Clone::clone),
        );
        field(fields, "Swap used", swap.map(|s| bytes(s.xsu_used)));
        let mut task = unsafe { std::mem::zeroed::<libc::mach_task_basic_info>() };
        let mut count = libc::MACH_TASK_BASIC_INFO_COUNT;
        let result = unsafe {
            libc::task_info(
                libc::mach_task_self(),
                libc::MACH_TASK_BASIC_INFO,
                (&mut task as *mut libc::mach_task_basic_info).cast(),
                &mut count,
            )
        };
        field(
            fields,
            "DN resident memory",
            if result == libc::KERN_SUCCESS && count >= libc::MACH_TASK_BASIC_INFO_COUNT {
                Ok(bytes(task.resident_size))
            } else {
                Err(format!("Mach task query failed ({result})"))
            },
        );
        field(
            fields,
            "Note",
            Ok("macOS VM counters; free pages are not available-memory estimates.".into()),
        );
    }
}
#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    pub fn hostname() -> Result<String, String> {
        unix_hostname()
    }
    pub fn version() -> Result<String, String> {
        std::fs::read_to_string("/proc/sys/kernel/osrelease")
            .map(|s| s.trim().into())
            .map_err(|e| e.to_string())
    }
    pub fn memory(fields: &mut Fields) {
        let info = std::fs::read_to_string("/proc/meminfo").map_err(|e| e.to_string());
        for (label, key) in [
            ("Physical RAM", "MemTotal"),
            ("Available memory", "MemAvailable"),
            ("Free memory", "MemFree"),
            ("Buffers", "Buffers"),
            ("Cached", "Cached"),
            ("Swap total", "SwapTotal"),
        ] {
            field(
                fields,
                label,
                info.as_ref()
                    .map_err(Clone::clone)
                    .and_then(|s| kib_field(s, key))
                    .map(bytes),
            );
        }
        field(
            fields,
            "Swap used",
            info.as_ref()
                .map_err(Clone::clone)
                .and_then(|s| {
                    kib_field(s, "SwapTotal").and_then(|total| {
                        total
                            .checked_sub(kib_field(s, "SwapFree")?)
                            .ok_or_else(|| "SwapFree exceeds SwapTotal".into())
                    })
                })
                .map(bytes),
        );
        field(
            fields,
            "DN resident memory",
            std::fs::read_to_string("/proc/self/status")
                .map_err(|e| e.to_string())
                .and_then(|s| kib_field(&s, "VmRSS"))
                .map(bytes),
        );
    }
}
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
mod platform {
    use super::*;
    pub fn hostname() -> Result<String, String> {
        #[cfg(unix)]
        {
            unix_hostname()
        }
        #[cfg(not(unix))]
        {
            environment("COMPUTERNAME")
        }
    }
    pub fn version() -> Result<String, String> {
        Err("OS version query is not supported on this platform".into())
    }
    pub fn memory(fields: &mut Fields) {
        for label in [
            "Physical RAM",
            "Available memory",
            "Swap total",
            "Swap used",
            "DN resident memory",
        ] {
            field(fields, label, Err("Not supported on this platform".into()));
        }
    }
}
