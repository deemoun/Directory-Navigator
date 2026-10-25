use std::{path::PathBuf, sync::mpsc};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    System,
    Memory,
}
impl Kind {
    pub fn title(self) -> &'static str {
        match self {
            Self::System => " System Information ",
            Self::Memory => " Memory Information ",
        }
    }
}
pub type Fields = Vec<(String, String)>;
pub struct Information {
    pub(crate) requests: crate::requests::RequestScope,
    pub kind: Kind,
    pub fields: Fields,
    pub offset: usize,
    pub horizontal: u16,
    pub rows: usize,
    pending: Option<crate::task::Task<Fields>>,
}
impl Information {
    pub fn open(kind: Kind, path: PathBuf, size: (u16, u16)) -> std::io::Result<Self> {
        let requests = crate::requests::RequestScope::default();
        let receive = requests.spawn("dn-information", move |_| collect(kind, path, size))?;
        Ok(Self {
            requests,
            kind,
            fields: Vec::new(),
            offset: 0,
            horizontal: 0,
            rows: 10,
            pending: Some(receive),
        })
    }
    pub fn loading(&self) -> bool {
        self.pending.is_some()
    }
    pub fn poll(&mut self) -> bool {
        let reply = self.pending.as_ref().and_then(|r| match r.try_recv() {
            Ok(fields) => Some(fields),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(vec![(
                "Snapshot".into(),
                "Unavailable: information worker stopped".into(),
            )]),
        });
        if let Some(fields) = reply {
            self.pending = None;
            self.fields = fields;
            true
        } else {
            false
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
