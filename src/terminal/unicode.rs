//! Read-only console diagnostics. Locale, keyboard and display decoding are
//! deliberately separate; none of these probes changes terminal state.
use crate::utilities::information::Fields;
use std::ffi::OsString;

fn effective_locale(mut environment: impl FnMut(&str) -> Option<OsString>) -> String {
    for key in ["LC_ALL", "LC_CTYPE", "LANG"] {
        if let Some(value) = environment(key).filter(|value| !value.is_empty()) {
            return format!("{} ({key})", value.to_string_lossy());
        }
    }
    "C (default)".into()
}

fn russian_letters() -> Vec<u16> {
    [0x0401, 0x0451]
        .into_iter()
        .chain(0x0410..=0x044f)
        .collect()
}
const FRAME: &[u16] = &[
    '┌' as u16,
    '┐' as u16,
    '└' as u16,
    '┘' as u16,
    '─' as u16,
    '│' as u16,
    '╔' as u16,
    '╗' as u16,
    '╚' as u16,
    '╝' as u16,
    '═' as u16,
    '║' as u16,
];

fn coverage(mapping: &[u16], required: &[u16]) -> String {
    let present: std::collections::BTreeSet<_> = mapping.iter().copied().collect();
    let missing: Vec<_> = required.iter().filter(|ch| !present.contains(ch)).collect();
    let mut value = format!("{}/{}", required.len() - missing.len(), required.len());
    if !missing.is_empty() {
        value.push_str("; ");
        value.push_str(
            &missing
                .iter()
                .take(8)
                .map(|ch| format!("U+{ch:04X}"))
                .collect::<Vec<_>>()
                .join(" "),
        );
        if missing.len() > 8 {
            value.push_str(&format!(" (+{})", missing.len() - 8));
        }
    }
    value
}

fn keyboard_mode(mode: i32) -> String {
    match mode {
        0 => "K_RAW (0)",
        1 => "K_XLATE (1)",
        2 => "K_MEDIUMRAW (2)",
        3 => "K_UNICODE (3)",
        4 => "K_OFF (4)",
        _ => return format!("Unknown ({mode})"),
    }
    .into()
}

fn console_fields(
    verified: bool,
    keyboard: impl FnOnce() -> Result<i32, String>,
    font: impl FnOnce() -> Result<Vec<u16>, String>,
) -> Fields {
    let mut fields = vec![
        ("DN text output".into(), "UTF-8".into()),
        ("Console display decoding".into(), "Not verified".into()),
        ("Cyrillic sample".into(), "Привет, Ёжик! / Test 123".into()),
    ];
    let unavailable = |error: String| format!("Unavailable: {error}");
    if !verified {
        fields.extend(
            [
                "Keyboard mode",
                "Russian alphabet mappings",
                "Console frame mappings",
            ]
            .map(|key| {
                (
                    key.into(),
                    "Not applicable outside a virtual console".into(),
                )
            }),
        );
        return fields;
    }
    fields.push((
        "Keyboard mode".into(),
        keyboard().map(keyboard_mode).unwrap_or_else(unavailable),
    ));
    match font() {
        Ok(mapping) => {
            fields.push((
                "Russian alphabet mappings".into(),
                coverage(&mapping, &russian_letters()),
            ));
            fields.push(("Console frame mappings".into(), coverage(&mapping, FRAME)));
        }
        Err(error) => {
            let error = unavailable(error);
            fields.push(("Russian alphabet mappings".into(), error.clone()));
            fields.push(("Console frame mappings".into(), error));
        }
    }
    fields
}

#[cfg(target_os = "linux")]
pub(crate) fn collect() -> Fields {
    use std::os::fd::AsRawFd;
    let mut fields = vec![(
        "Effective locale".into(),
        effective_locale(|key| std::env::var_os(key)),
    )];
    // Reuse GPM's actual-device verification, without connecting to the daemon.
    let console = super::gpm::client::console_fd();
    let fd = console.as_ref().map(|(fd, _)| fd.as_raw_fd());
    fields.extend(console_fields(
        fd.is_some(),
        || native::keyboard(fd.unwrap()),
        || native::mapping(fd.unwrap()),
    ));
    fields
}

#[cfg(target_os = "linux")]
mod native {
    use std::io;
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct Pair {
        unicode: u16,
        _font_position: u16,
    }
    #[repr(C)]
    struct Map {
        count: u16,
        entries: *mut Pair,
    }
    pub(super) fn keyboard(fd: i32) -> Result<i32, String> {
        let mut mode: libc::c_int = 0;
        // SAFETY: Linux KDGKBMODE writes an int to valid initialized storage.
        if unsafe { libc::ioctl(fd, 0x4b44 as libc::c_ulong, &mut mode) } < 0 {
            Err(io::Error::last_os_error().to_string())
        } else {
            Ok(mode)
        }
    }
    pub(super) fn mapping(fd: i32) -> Result<Vec<u16>, String> {
        // GIO_UNIMAP's count is u16: at most 65535 four-byte pairs (256 KiB).
        // Allocate once; never retry/grow in response to a kernel count.
        let mut entries = vec![Pair::default(); usize::from(u16::MAX)];
        let mut map = Map {
            count: u16::MAX,
            entries: entries.as_mut_ptr(),
        };
        // SAFETY: repr(C) matches unimapdesc/unipair in linux/kd.h; the pointer
        // remains live and writable for exactly the supplied entry capacity.
        if unsafe { libc::ioctl(fd, 0x4b66 as libc::c_ulong, &mut map) } < 0 {
            return Err(io::Error::last_os_error().to_string());
        }
        entries.truncate(usize::from(map.count));
        Ok(entries.into_iter().map(|pair| pair.unicode).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn locale_precedence_ignores_empty_values_without_mutating_environment() {
        for (values, expected) in [
            (
                [Some("ru_RU.UTF-8"), Some("en_US.UTF-8"), Some("C")],
                "ru_RU.UTF-8 (LC_ALL)",
            ),
            ([Some(""), Some("C.UTF-8"), Some("C")], "C.UTF-8 (LC_CTYPE)"),
            ([None, None, Some("ru_RU.KOI8-R")], "ru_RU.KOI8-R (LANG)"),
            ([None, Some(""), None], "C (default)"),
        ] {
            assert_eq!(
                effective_locale(|key| values[["LC_ALL", "LC_CTYPE", "LANG"]
                    .iter()
                    .position(|name| *name == key)
                    .unwrap()]
                .map(OsString::from)),
                expected
            );
        }
    }
    #[test]
    fn font_coverage_is_mapping_presence_with_bounded_ascii_missing_codes() {
        let russian = russian_letters();
        assert_eq!(russian.len(), 66);
        assert_eq!(coverage(&russian, &russian), "66/66");
        let without_yo: Vec<_> = russian
            .iter()
            .copied()
            .filter(|ch| ![0x0401, 0x0451].contains(ch))
            .collect();
        assert_eq!(coverage(&without_yo, &russian), "64/66; U+0401 U+0451");
        assert_eq!(
            coverage(&[0x0401, 0x0401], &russian),
            "1/66; U+0451 U+0410 U+0411 U+0412 U+0413 U+0414 U+0415 U+0416 (+57)"
        );
        assert_eq!(coverage(FRAME, FRAME), "12/12");
        assert!(coverage(&[], &russian).is_ascii());
        assert!(coverage(&[], &russian).len() < 100);
    }
    #[test]
    fn pty_never_probes_keyboard_or_font_and_errors_are_independent() {
        let fields = console_fields(
            false,
            || panic!("PTY keyboard ioctl"),
            || panic!("PTY font ioctl"),
        );
        assert!(fields
            .iter()
            .any(|(key, value)| key == "Console display decoding" && value == "Not verified"));
        let fields = console_fields(
            true,
            || Err("Permission denied".into()),
            || Ok(russian_letters()),
        );
        assert!(fields.contains(&(
            "Keyboard mode".into(),
            "Unavailable: Permission denied".into()
        )));
        assert!(fields.contains(&("Russian alphabet mappings".into(), "66/66".into())));
        let fields = console_fields(true, || Ok(3), || Err("Unsupported ioctl".into()));
        assert!(fields.contains(&("Keyboard mode".into(), "K_UNICODE (3)".into())));
        assert!(fields.contains(&(
            "Russian alphabet mappings".into(),
            "Unavailable: Unsupported ioctl".into()
        )));
        assert!(fields.contains(&("Console display decoding".into(), "Not verified".into())));
        for mode in 0..=4 {
            assert!(keyboard_mode(mode).starts_with("K_"));
        }
        assert_eq!(keyboard_mode(99), "Unknown (99)");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn native_queries_reject_non_console_fds_without_consuming_bytes() {
        use std::{
            io::{Read, Write},
            os::fd::AsRawFd,
            os::unix::net::UnixStream,
        };
        let (mut input, mut peer) = UnixStream::pair().unwrap();
        peer.write_all(b"unchanged").unwrap();
        assert!(native::keyboard(input.as_raw_fd()).is_err());
        assert!(native::mapping(input.as_raw_fd()).is_err());
        let mut bytes = [0; 9];
        input.read_exact(&mut bytes).unwrap();
        assert_eq!(&bytes, b"unchanged");
    }
}
