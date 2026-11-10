//! Bounded, owner-scoped sampling of local interface counters. No traffic probes.
use super::information::InterfaceKey;
use crate::requests::RequestScope;
use std::{
    io,
    time::{Duration, Instant},
};
const INTERVAL: Duration = Duration::from_secs(1);
const MAX_GAP: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug)]
struct Counters {
    received: u64,
    sent: u64,
}
#[derive(Clone, Copy, Debug)]
struct Sample {
    counters: Counters,
    at: Instant,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    Measuring,
    Ready { download: u64, upload: u64 },
    Unavailable(String),
}
pub struct Monitor {
    key: Option<InterfaceKey>,
    previous: Option<Sample>,
    next: Instant,
    pending: Option<crate::task::Task<io::Result<Sample>>>,
    state: State,
}
impl Default for Monitor {
    fn default() -> Self {
        Self {
            key: None,
            previous: None,
            next: Instant::now(),
            pending: None,
            state: State::Unavailable("No network interface selected".into()),
        }
    }
}
impl Monitor {
    pub fn state(&self) -> &State {
        &self.state
    }
    pub fn select(&mut self, key: Option<InterfaceKey>) {
        if self.key == key {
            return;
        }
        self.pending = None;
        self.previous = None;
        self.next = Instant::now();
        self.key = key;
        self.state = if self.key.is_some() {
            State::Measuring
        } else {
            State::Unavailable("No network interface selected".into())
        };
    }
    fn accept(&mut self, result: io::Result<Sample>) {
        match result {
            Ok(sample) => {
                self.state = self
                    .previous
                    .and_then(|previous| rates(previous, sample))
                    .map_or(State::Measuring, |(download, upload)| State::Ready {
                        download,
                        upload,
                    });
                self.previous = Some(sample);
            }
            Err(error) => {
                self.previous = None;
                self.state = State::Unavailable(error.to_string());
            }
        }
    }
    pub(crate) fn poll(&mut self, requests: &RequestScope, now: Instant) -> bool {
        let old = self.state.clone();
        if let Some(reply) = crate::task::take_reply(&mut self.pending) {
            self.accept(
                reply.unwrap_or_else(|_| Err(io::Error::other("Traffic sampling worker stopped"))),
            );
            self.next = now + INTERVAL;
        }
        if self.pending.is_none() && now >= self.next {
            if let Some(key) = self.key.clone() {
                match requests.spawn("dn-network-traffic", move |cancel| {
                    cancel.check()?;
                    let counters = read(&key)?;
                    cancel.check()?;
                    Ok(Sample {
                        counters,
                        at: Instant::now(),
                    })
                }) {
                    Ok(task) => self.pending = Some(task),
                    Err(error) => {
                        self.accept(Err(error));
                        self.next = now + INTERVAL;
                    }
                }
            }
        }
        self.state != old
    }
}
fn rates(previous: Sample, current: Sample) -> Option<(u64, u64)> {
    let elapsed = current.at.checked_duration_since(previous.at)?;
    if elapsed.is_zero() || elapsed > MAX_GAP {
        return None;
    }
    let rate = |current: u64, previous: u64| {
        let delta = current.checked_sub(previous)?;
        Some(
            (u128::from(delta) * 1_000_000_000 / elapsed.as_nanos()).min(u128::from(u64::MAX))
                as u64,
        )
    };
    Some((
        rate(current.counters.received, previous.counters.received)?,
        rate(current.counters.sent, previous.counters.sent)?,
    ))
}
pub fn format_rate(rate: u64) -> String {
    const UNITS: [&str; 7] = ["B/s", "KiB/s", "MiB/s", "GiB/s", "TiB/s", "PiB/s", "EiB/s"];
    let mut value = rate as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{rate} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
#[cfg(unix)]
fn verify(key: &InterfaceKey) -> io::Result<()> {
    let name = std::ffi::CString::new(key.name.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Invalid interface name"))?;
    // SAFETY: name is NUL-terminated and remains alive throughout the call.
    if key.index == 0 || unsafe { libc::if_nametoindex(name.as_ptr()) } != key.index {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "Network interface changed or disappeared",
        ));
    }
    Ok(())
}
#[cfg(not(target_os = "macos"))]
fn read(key: &InterfaceKey) -> io::Result<Counters> {
    #[cfg(unix)]
    verify(key)?;
    let mut interface = netdev::Interface::dummy();
    interface.name.clone_from(&key.name);
    interface.index = key.index;
    interface.update_stats()?;
    #[cfg(unix)]
    verify(key)?;
    let stats = interface.stats.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "Interface traffic counters are unavailable",
        )
    })?;
    Ok(Counters {
        received: stats.rx_bytes,
        sent: stats.tx_bytes,
    })
}

// macOS getifaddrs (including netdev's stats API) exposes 32-bit counters.
// NET_RT_IFLIST2 supplies native 64-bit counters, avoiding a wrap at 4 GiB.
#[cfg(target_os = "macos")]
fn read(key: &InterfaceKey) -> io::Result<Counters> {
    verify(key)?;
    let mut mib = [
        libc::CTL_NET,
        libc::PF_ROUTE,
        0,
        0,
        libc::NET_RT_IFLIST2,
        key.index
            .try_into()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Invalid interface index"))?,
    ];
    for _ in 0..2 {
        let mut len = 0;
        // SAFETY: valid six-element MIB and output length; this call only queries size.
        if unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                6,
                std::ptr::null_mut(),
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        if len == 0 || len > 1024 * 1024 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid interface counters snapshot size",
            ));
        }
        let mut bytes = vec![0u8; len];
        // SAFETY: bytes owns len writable bytes; all new-value pointers are null (read-only).
        if unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                6,
                bytes.as_mut_ptr().cast(),
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        } != 0
        {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ENOMEM) {
                continue;
            }
            return Err(error);
        }
        if len > bytes.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid interface counters snapshot size",
            ));
        }
        verify(key)?;
        return parse(&bytes[..len], key.index);
    }
    Err(io::Error::other("Interface counters changed while reading"))
}
#[cfg(target_os = "macos")]
fn parse(mut bytes: &[u8], index: u32) -> io::Result<Counters> {
    while bytes.len() >= 4 {
        let len = usize::from(u16::from_ne_bytes([bytes[0], bytes[1]]));
        if len < 4 || len > bytes.len() {
            break;
        }
        if bytes[2] == libc::RTM_VERSION as u8 && bytes[3] == libc::RTM_IFINFO2 as u8 {
            if len < std::mem::size_of::<libc::if_msghdr2>() {
                break;
            }
            // SAFETY: checked complete header length; unaligned read copies the packed ABI.
            let header = unsafe { bytes.as_ptr().cast::<libc::if_msghdr2>().read_unaligned() };
            if u32::from(header.ifm_index) == index {
                return Ok(Counters {
                    received: header.ifm_data.ifi_ibytes,
                    sent: header.ifm_data.ifi_obytes,
                });
            }
        }
        bytes = &bytes[len..];
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "Interface traffic counters are unavailable",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample(at: Instant, received: u64, sent: u64) -> Sample {
        Sample {
            at,
            counters: Counters { received, sent },
        }
    }
    fn key(name: &str) -> InterfaceKey {
        InterfaceKey {
            index: 7,
            name: name.into(),
        }
    }
    #[test]
    fn rates_use_monotonic_elapsed_time_including_subseconds_and_zero_traffic() {
        let now = Instant::now();
        let before = sample(now, 10_000, 20_000);
        assert_eq!(
            rates(
                before,
                sample(now + Duration::from_millis(1250), 12_500, 25_000)
            ),
            Some((2000, 4000))
        );
        assert_eq!(
            rates(before, sample(now + INTERVAL, 10_000, 20_000)),
            Some((0, 0))
        );
        assert_eq!(
            rates(
                sample(now, 0, 0),
                sample(now + Duration::from_nanos(1), u64::MAX, u64::MAX)
            ),
            Some((u64::MAX, u64::MAX))
        );
    }
    #[test]
    fn resets_missing_samples_long_gaps_and_invalid_time_never_create_spikes() {
        let now = Instant::now();
        let before = sample(now, 100, 200);
        for after in [
            sample(now, 200, 300),
            sample(now - INTERVAL, 200, 300),
            sample(now + MAX_GAP + INTERVAL, 1000, 2000),
            sample(now + INTERVAL, 99, 300),
            sample(now + INTERVAL, 200, 199),
        ] {
            assert_eq!(rates(before, after), None);
        }
        let mut monitor = Monitor::default();
        monitor.select(Some(key("first")));
        monitor.accept(Ok(before));
        assert_eq!(monitor.state(), &State::Measuring);
        monitor.accept(Ok(sample(now + INTERVAL, 200, 400)));
        assert_eq!(
            monitor.state(),
            &State::Ready {
                download: 100,
                upload: 200
            }
        );
        monitor.accept(Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "fixture denied",
        )));
        assert_eq!(
            monitor.state(),
            &State::Unavailable("fixture denied".into())
        );
        monitor.accept(Ok(sample(now + INTERVAL * 2, 9000, 9000)));
        assert_eq!(monitor.state(), &State::Measuring);
        monitor.accept(Ok(sample(now + INTERVAL * 3, 100, 200)));
        assert_eq!(monitor.state(), &State::Measuring);
        monitor.accept(Ok(sample(now + INTERVAL * 4, 110, 220)));
        assert_eq!(
            monitor.state(),
            &State::Ready {
                download: 10,
                upload: 20
            }
        );
    }
    #[test]
    fn selection_clears_baseline_and_preserves_measurement_when_reselected() {
        let now = Instant::now();
        let mut monitor = Monitor::default();
        assert!(matches!(monitor.state(), State::Unavailable(_)));
        monitor.select(Some(key("first")));
        monitor.accept(Ok(sample(now, 1, 2)));
        monitor.accept(Ok(sample(now + INTERVAL, 101, 202)));
        monitor.select(Some(key("first")));
        assert_eq!(
            monitor.state(),
            &State::Ready {
                download: 100,
                upload: 200
            }
        );
        monitor.select(Some(key("second")));
        assert_eq!(monitor.state(), &State::Measuring);
        assert!(monitor.previous.is_none());
        monitor.accept(Ok(sample(now + INTERVAL * 2, 1000, 2000)));
        assert_eq!(monitor.state(), &State::Measuring);
    }
    #[test]
    fn formatting_uses_bytes_and_keeps_large_values_bounded() {
        for (rate, expected) in [
            (0, "0 B/s"),
            (1023, "1023 B/s"),
            (1024, "1.0 KiB/s"),
            (1536, "1.5 KiB/s"),
            (1 << 20, "1.0 MiB/s"),
            (1 << 30, "1.0 GiB/s"),
            (u64::MAX, "16.0 EiB/s"),
        ] {
            assert_eq!(format_rate(rate), expected);
        }
    }
    #[test]
    fn late_worker_is_cancelled_on_switch_and_sampling_is_throttled() {
        let requests = RequestScope::default();
        let mut monitor = Monitor::default();
        monitor.select(Some(key("first")));
        let (start, started) = std::sync::mpsc::sync_channel(1);
        let (release, released) = std::sync::mpsc::sync_channel(1);
        let (done, finished) = std::sync::mpsc::sync_channel(1);
        monitor.pending = Some(
            requests
                .spawn("dn-network-traffic", move |cancel| {
                    start.send(()).unwrap();
                    released.recv_timeout(Duration::from_secs(5)).unwrap();
                    done.send(cancel.cancelled()).unwrap();
                    Ok(sample(Instant::now(), u64::MAX, u64::MAX))
                })
                .unwrap(),
        );
        started.recv_timeout(Duration::from_secs(5)).unwrap();
        monitor.select(Some(key("second")));
        release.send(()).unwrap();
        assert!(finished.recv_timeout(Duration::from_secs(5)).unwrap());
        assert!(monitor.pending.is_none());
        assert_eq!(monitor.state(), &State::Measuring);
        let now = Instant::now();
        monitor.next = now + INTERVAL;
        for _ in 0..10 {
            assert!(!monitor.poll(&requests, now));
            assert!(monitor.pending.is_none());
        }
        monitor.select(None);
        assert!(!monitor.poll(&requests, now + INTERVAL * 2));
        assert!(monitor.pending.is_none());
    }
    #[cfg(unix)]
    #[test]
    fn missing_and_invalid_interfaces_are_rejected_without_reading_other_devices() {
        assert!(read(&InterfaceKey {
            index: 0,
            name: "does-not-exist".into()
        })
        .is_err());
        assert!(read(&InterfaceKey {
            index: 1,
            name: "bad\0name".into()
        })
        .is_err());
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn native_snapshot_parser_uses_64_bit_counters_and_rejects_malformed_packets() {
        let len = std::mem::size_of::<libc::if_msghdr2>();
        let mut bytes = vec![0u8; len];
        bytes[..2].copy_from_slice(&(len as u16).to_ne_bytes());
        bytes[2] = libc::RTM_VERSION as u8;
        bytes[3] = libc::RTM_IFINFO2 as u8;
        let index_offset = std::mem::offset_of!(libc::if_msghdr2, ifm_index);
        bytes[index_offset..index_offset + 2].copy_from_slice(&7u16.to_ne_bytes());
        let data = std::mem::offset_of!(libc::if_msghdr2, ifm_data);
        for (field, value) in [
            (
                std::mem::offset_of!(libc::if_data64, ifi_ibytes),
                u64::from(u32::MAX) + 200,
            ),
            (
                std::mem::offset_of!(libc::if_data64, ifi_obytes),
                u64::from(u32::MAX) + 300,
            ),
        ] {
            bytes[data + field..data + field + 8].copy_from_slice(&value.to_ne_bytes());
        }
        let counters = parse(&bytes, 7).unwrap();
        assert_eq!(counters.received, u64::from(u32::MAX) + 200);
        assert_eq!(counters.sent, u64::from(u32::MAX) + 300);
        assert!(parse(&bytes, 8).is_err());
        for len in 0..bytes.len() {
            assert!(parse(&bytes[..len], 7).is_err());
        }
        let mut invalid = bytes.clone();
        invalid[..2].copy_from_slice(&0u16.to_ne_bytes());
        assert!(parse(&invalid, 7).is_err());
        invalid[..2].copy_from_slice(&u16::MAX.to_ne_bytes());
        assert!(parse(&invalid, 7).is_err());
    }
}
