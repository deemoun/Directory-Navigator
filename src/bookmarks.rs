use std::path::{Path, PathBuf};
pub fn encode(path: &Path) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        format!(
            "unix:{}",
            path.as_os_str()
                .as_bytes()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        )
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        format!(
            "windows:{}",
            path.as_os_str()
                .encode_wide()
                .map(|b| format!("{b:04x}"))
                .collect::<String>()
        )
    }
    #[cfg(not(any(unix, windows)))]
    {
        format!(
            "utf8:{}",
            path.to_string_lossy()
                .as_bytes()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        )
    }
}
pub fn decode(value: &str) -> Option<PathBuf> {
    let (tag, hex) = value.split_once(':')?;
    if hex.is_empty() || hex.len() > 131072 || !hex.is_ascii() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};
        if tag != "unix" || hex.len() % 2 != 0 {
            return None;
        }
        let bytes: Option<Vec<_>> = (0..hex.len())
            .step_by(2)
            .map(|n| u8::from_str_radix(&hex[n..n + 2], 16).ok())
            .collect();
        let bytes = bytes?;
        if bytes.contains(&0) {
            return None;
        }
        Some(PathBuf::from(OsString::from_vec(bytes)))
    }
    #[cfg(windows)]
    {
        use std::{ffi::OsString, os::windows::ffi::OsStringExt};
        if tag != "windows" || hex.len() % 4 != 0 {
            return None;
        }
        let words: Option<Vec<_>> = (0..hex.len())
            .step_by(4)
            .map(|n| u16::from_str_radix(&hex[n..n + 4], 16).ok())
            .collect();
        let words = words?;
        if words.contains(&0) {
            return None;
        }
        Some(PathBuf::from(OsString::from_wide(&words)))
    }
    #[cfg(not(any(unix, windows)))]
    {
        if tag != "utf8" || hex.len() % 2 != 0 {
            return None;
        }
        let bytes: Option<Vec<_>> = (0..hex.len())
            .step_by(2)
            .map(|n| u8::from_str_radix(&hex[n..n + 2], 16).ok())
            .collect();
        String::from_utf8(bytes?).ok().map(PathBuf::from)
    }
}
