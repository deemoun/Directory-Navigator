//! Read Windows executable version resources without running a GUI version dialog.
#[cfg(not(windows))]
pub(super) fn read(_: &std::path::Path) -> Option<String> {
    None
}
#[cfg(windows)]
pub(super) fn read(path: &std::path::Path) -> Option<String> {
    use std::{ffi::c_void, os::windows::ffi::OsStrExt};
    #[link(name = "version")]
    unsafe extern "system" {
        fn GetFileVersionInfoSizeW(path: *const u16, unused: *mut u32) -> u32;
        fn GetFileVersionInfoW(path: *const u16, unused: u32, size: u32, data: *mut c_void) -> i32;
        fn VerQueryValueW(
            data: *const c_void,
            key: *const u16,
            value: *mut *mut c_void,
            len: *mut u32,
        ) -> i32;
    }
    let path = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let mut unused = 0;
    // SAFETY: path is a terminated UTF-16 string and unused is writable.
    let size = unsafe { GetFileVersionInfoSizeW(path.as_ptr(), &mut unused) };
    if size == 0 || size > 1024 * 1024 {
        return None;
    }
    let mut data = vec![0_u64; (size as usize).div_ceil(8)];
    // SAFETY: buffer is aligned and sized for the bounded version resource.
    if unsafe { GetFileVersionInfoW(path.as_ptr(), 0, size, data.as_mut_ptr().cast()) } == 0 {
        return None;
    }
    let query = |key: &str| -> Option<(*mut c_void, u32)> {
        let key = key.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        let mut value = std::ptr::null_mut();
        let mut len = 0;
        // SAFETY: data holds a valid version resource; key is terminated and outputs are writable.
        if unsafe { VerQueryValueW(data.as_ptr().cast(), key.as_ptr(), &mut value, &mut len) } == 0
            || value.is_null()
        {
            None
        } else {
            Some((value, len))
        }
    };
    let mut translations = vec![(0x0409_u16, 0x04b0_u16), (0x0409, 0x04e4)];
    if let Some((pointer, len)) = query("\\VarFileInfo\\Translation") {
        for i in 0..(len as usize / 4).min(64) {
            // SAFETY: each pair lies in the returned, bounded translation table.
            unsafe {
                translations.push((
                    pointer.cast::<u16>().add(i * 2).read_unaligned(),
                    pointer.cast::<u16>().add(i * 2 + 1).read_unaligned(),
                ));
            }
        }
    }
    let mut text = String::new();
    for (language, page) in translations {
        for field in ["ProductName", "FileDescription", "ProductVersion"] {
            if let Some((pointer, len)) = query(&format!(
                "\\StringFileInfo\\{language:04x}{page:04x}\\{field}"
            )) {
                if len > 0 && len <= 1024 {
                    // SAFETY: length is returned in UTF-16 characters by the string query.
                    let words = (0..len as usize)
                        .map(|i| unsafe { pointer.cast::<u16>().add(i).read_unaligned() })
                        .take_while(|c| *c != 0)
                        .collect::<Vec<_>>();
                    text.push_str(&String::from_utf16_lossy(&words));
                    text.push('\n');
                }
            }
        }
        if text.to_ascii_lowercase().contains("dosbox") {
            return Some(text);
        }
    }
    None
}
