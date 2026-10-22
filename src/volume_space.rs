//! Volume capacity snapshots; native I/O never runs in rendering or directory scans.
use std::{
    io,
    path::{Path, PathBuf},
    sync::mpsc::TryRecvError,
    time::{Duration, Instant},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Space {
    pub available: u128,
    pub total: u128,
}
pub fn read(path: &Path) -> io::Result<Space> {
    #[cfg(unix)]
    {
        use std::{ffi::CString, mem::MaybeUninit, os::unix::ffi::OsStrExt};
        let path = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "Path contains NUL"))?;
        let mut stat = MaybeUninit::<libc::statvfs>::uninit();
        // SAFETY: a live NUL-terminated native path and writable statvfs storage.
        if unsafe { libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: statvfs initialized the structure on success.
        let stat = unsafe { stat.assume_init() };
        let block = u128::from(stat.f_frsize);
        Ok(Space {
            available: u128::from(stat.f_bavail) * block,
            total: u128::from(stat.f_blocks) * block,
        })
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "kernel32")]
        extern "system" {
            fn GetDiskFreeSpaceExW(
                path: *const u16,
                available: *mut u64,
                total: *mut u64,
                free: *mut u64,
            ) -> i32;
        }
        let mut path: Vec<u16> = path.as_os_str().encode_wide().collect();
        if path.contains(&0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Path contains NUL",
            ));
        }
        if !path
            .last()
            .is_some_and(|c| *c == u16::from(b'\\') || *c == u16::from(b'/'))
        {
            path.push(u16::from(b'\\'));
        }
        path.push(0);
        let (mut available, mut total) = (0_u64, 0_u64);
        // SAFETY: terminated UTF-16 directory and valid output pointers; final output is optional.
        if unsafe {
            GetDiskFreeSpaceExW(
                path.as_ptr(),
                &mut available,
                &mut total,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Space {
            available: u128::from(available),
            total: u128::from(total),
        })
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Volume space is unavailable on this platform",
        ))
    }
}
#[derive(Default)]
pub struct Snapshot {
    pub result: Option<Result<Space, String>>,
    path: Option<PathBuf>,
    attempted: Option<Instant>,
    worker: Option<crate::task::Task<io::Result<Space>>>,
}
impl Snapshot {
    pub fn refresh(&mut self) {
        self.attempted = None;
    }
    pub fn poll(&mut self, path: &Path, now: Instant) -> bool {
        let mut changed = false;
        if self.path.as_deref() != Some(path) {
            self.worker = None;
            self.path = Some(path.to_owned());
            self.attempted = None;
            changed = self.result.take().is_some();
        }
        if let Some(worker) = &self.worker {
            let reply = match worker.try_recv() {
                Ok(value) => Some(value.map_err(|e| e.to_string())),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => {
                    Some(Err("Volume information worker stopped".into()))
                }
            };
            if let Some(reply) = reply {
                changed |= self.result.as_ref() != Some(&reply);
                self.result = Some(reply);
                self.worker = None;
            }
        }
        if self.worker.is_none()
            && self
                .attempted
                .is_none_or(|t| now.saturating_duration_since(t) >= Duration::from_secs(5))
        {
            self.attempted = Some(now);
            let path = path.to_owned();
            match crate::task::Task::spawn("dn-volume-space", move |_| read(&path)) {
                Ok(worker) => self.worker = Some(worker),
                Err(error) => {
                    let result = Err(error.to_string());
                    changed |= self.result.as_ref() != Some(&result);
                    self.result = Some(result);
                }
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn superseded_path_discards_replies_and_errors_are_throttled() {
        let old = PathBuf::from("old-volume");
        let new = std::env::temp_dir().join(format!("dn-missing-volume-{}", std::process::id()));
        let mut snapshot = Snapshot {
            path: Some(old),
            attempted: Some(Instant::now()),
            ..Default::default()
        };
        let (send, receive) = std::sync::mpsc::channel();
        snapshot.worker = Some(
            crate::task::Task::spawn("test-volume", move |_| {
                let _ = receive.recv();
                Ok(Space {
                    available: 123,
                    total: 456,
                })
            })
            .unwrap(),
        );
        snapshot.result = Some(Ok(Space {
            available: 1,
            total: 2,
        }));
        let now = Instant::now();
        snapshot.poll(&new, now);
        assert_ne!(
            snapshot.result,
            Some(Ok(Space {
                available: 1,
                total: 2
            }))
        );
        let _ = send.send(());
        let end = Instant::now() + Duration::from_secs(3);
        while snapshot.worker.is_some() {
            snapshot.poll(&new, now);
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(snapshot.result.as_ref().is_some_and(Result::is_err));
        assert!(!snapshot.poll(&new, now + Duration::from_secs(1)));
        assert!(snapshot.worker.is_none());
        snapshot.refresh();
        snapshot.poll(&new, now);
        assert!(snapshot.worker.is_some());
    }
}
