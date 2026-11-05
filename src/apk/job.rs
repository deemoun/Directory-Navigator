//! Kill-on-close Windows job for an SDK process and its descendants.
#[cfg(windows)]
mod windows {
    use std::{ffi::c_void, io, os::windows::io::AsRawHandle, process::Child};
    type Handle = *mut c_void;
    #[repr(C)]
    #[derive(Default)]
    struct Limits {
        process_time: i64,
        job_time: i64,
        flags: u32,
        min_working: usize,
        max_working: usize,
        processes: u32,
        affinity: usize,
        priority: u32,
        scheduling: u32,
    }
    #[repr(C)]
    #[derive(Default)]
    struct Extended {
        basic: Limits,
        io: [u64; 6],
        process_memory: usize,
        job_memory: usize,
        peak_process: usize,
        peak_job: usize,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateJobObjectW(attributes: *const c_void, name: *const u16) -> Handle;
        fn SetInformationJobObject(
            job: Handle,
            class: i32,
            information: *const c_void,
            length: u32,
        ) -> i32;
        fn AssignProcessToJobObject(job: Handle, process: Handle) -> i32;
        fn CloseHandle(handle: Handle) -> i32;
    }
    pub(super) struct Job(Handle);
    impl Job {
        pub(super) fn new(child: &Child) -> io::Result<Self> {
            // SAFETY: null optional attributes/name; the returned handle is owned here.
            let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            let job = Self(handle);
            let mut limits = Extended::default();
            limits.basic.flags = 0x2000;
            // SAFETY: layout matches JOBOBJECT_EXTENDED_LIMIT_INFORMATION; Child's handle is live.
            if unsafe {
                SetInformationJobObject(
                    handle,
                    9,
                    &limits as *const _ as *const c_void,
                    std::mem::size_of::<Extended>() as u32,
                )
            } == 0
                || unsafe { AssignProcessToJobObject(handle, child.as_raw_handle()) } == 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(job)
        }
    }
    impl Drop for Job {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}
pub(super) struct Job {
    #[cfg(windows)]
    _job: windows::Job,
}
impl Job {
    pub(super) fn new(child: &std::process::Child) -> std::io::Result<Self> {
        #[cfg(windows)]
        {
            Ok(Self {
                _job: windows::Job::new(child)?,
            })
        }
        #[cfg(not(windows))]
        {
            let _ = child;
            Ok(Self {})
        }
    }
}
