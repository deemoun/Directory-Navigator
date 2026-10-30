//! Validated advanced criteria and bounded, cancellable literal UTF-8 searching.
use crate::{filesystem, platform_fs, task::Cancellation};
use std::{
    fs,
    io::{self, Read},
    path::Path,
    time::SystemTime,
};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FileKind {
    #[default]
    Any,
    Files,
    Directories,
    Links,
}
impl FileKind {
    pub fn next(self) -> Self {
        match self {
            Self::Any => Self::Files,
            Self::Files => Self::Directories,
            Self::Directories => Self::Links,
            Self::Links => Self::Any,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Any => "Any entry",
            Self::Files => "Files only",
            Self::Directories => "Directories only",
            Self::Links => "Links only",
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct Options {
    pub text: String,
    pub case_sensitive: bool,
    pub min_size: Option<u64>,
    pub max_size: Option<u64>,
    pub after: Option<SystemTime>,
    pub before: Option<SystemTime>,
    pub kind: FileKind,
}
pub fn size(text: &str) -> io::Result<Option<u64>> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let upper = text.to_ascii_uppercase();
    let (number, multiplier) = if let Some(s) = upper
        .strip_suffix("KIB")
        .or_else(|| upper.strip_suffix('K'))
    {
        (s, 1024)
    } else if let Some(s) = upper
        .strip_suffix("MIB")
        .or_else(|| upper.strip_suffix('M'))
    {
        (s, 1024 * 1024)
    } else if let Some(s) = upper
        .strip_suffix("GIB")
        .or_else(|| upper.strip_suffix('G'))
    {
        (s, 1024 * 1024 * 1024)
    } else {
        (upper.as_str(), 1)
    };
    number
        .trim()
        .parse::<u64>()
        .ok()
        .and_then(|v| v.checked_mul(multiplier))
        .map(Some)
        .ok_or_else(|| io::Error::other("Invalid size; use bytes or K/M/G (1024-based)"))
}
pub fn date(text: &str, end: bool) -> io::Result<Option<SystemTime>> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let d = chrono::NaiveDate::parse_from_str(text.trim(), "%Y-%m-%d")
        .map_err(|_| io::Error::other("Date must be YYYY-MM-DD (UTC)"))?;
    let t = if end {
        d.and_hms_opt(23, 59, 59)
    } else {
        d.and_hms_opt(0, 0, 0)
    }
    .ok_or_else(|| io::Error::other("Invalid date"))?;
    let stamp = t.and_utc().timestamp();
    let value = if stamp >= 0 {
        SystemTime::UNIX_EPOCH.checked_add(std::time::Duration::from_secs(stamp as u64))
    } else {
        SystemTime::UNIX_EPOCH.checked_sub(std::time::Duration::from_secs(stamp.unsigned_abs()))
    };
    let value = if end {
        value.and_then(|v| v.checked_add(std::time::Duration::from_nanos(999_999_999)))
    } else {
        value
    };
    value
        .map(Some)
        .ok_or_else(|| io::Error::other("Date is outside the supported range"))
}
impl Options {
    pub fn validate(&self) -> io::Result<()> {
        if self.text.len() > 4096 || self.text.contains('\0') {
            return Err(io::Error::other(
                "Search text exceeds 4096 bytes or contains NUL",
            ));
        }
        if self.min_size.zip(self.max_size).is_some_and(|(a, b)| a > b)
            || self.after.zip(self.before).is_some_and(|(a, b)| a > b)
        {
            return Err(io::Error::other("Minimum must not exceed maximum"));
        }
        Ok(())
    }
    pub fn accepts(&self, m: &fs::Metadata) -> bool {
        let kind = match self.kind {
            FileKind::Any => true,
            FileKind::Files => m.is_file(),
            FileKind::Directories => m.is_dir(),
            FileKind::Links => m.is_symlink(),
        };
        kind && self.min_size.is_none_or(|s| m.len() >= s)
            && self.max_size.is_none_or(|s| m.len() <= s)
            && self
                .after
                .is_none_or(|s| m.modified().is_ok_and(|v| v >= s))
            && self
                .before
                .is_none_or(|s| m.modified().is_ok_and(|v| v <= s))
    }
    pub fn content(
        &self,
        path: &Path,
        metadata: &fs::Metadata,
        cancel: &Cancellation,
    ) -> io::Result<Option<usize>> {
        cancel.check()?;
        let mut file = filesystem::open_regular(path)?;
        let stable = |file: &fs::File| -> io::Result<()> {
            let current = file.metadata()?;
            if !platform_fs::same_identity(metadata, &current)
                || current.len() != metadata.len()
                || current.modified().ok() != metadata.modified().ok()
            {
                return Err(io::Error::other("File changed during content search"));
            }
            Ok(())
        };
        if !platform_fs::same_identity(metadata, &file.metadata()?) {
            return Err(io::Error::other("File changed before content search"));
        }
        let needle = if self.case_sensitive {
            self.text.clone()
        } else {
            self.text.to_lowercase()
        };
        let mut read = [0u8; 64 * 1024];
        let mut undecoded = Vec::new();
        let mut window = String::new();
        let mut line = 1;
        loop {
            cancel.check()?;
            let count = file.read(&mut read)?;
            undecoded.extend_from_slice(&read[..count]);
            let mut consumed = 0;
            while consumed < undecoded.len() {
                match std::str::from_utf8(&undecoded[consumed..]) {
                    Ok(s) => {
                        window.push_str(&if self.case_sensitive {
                            s.into()
                        } else {
                            s.to_lowercase()
                        });
                        consumed = undecoded.len();
                    }
                    Err(e) => {
                        let valid = e.valid_up_to();
                        let s = std::str::from_utf8(&undecoded[consumed..consumed + valid])
                            .expect("valid UTF-8 prefix");
                        window.push_str(&if self.case_sensitive {
                            s.into()
                        } else {
                            s.to_lowercase()
                        });
                        consumed += valid;
                        if let Some(n) = e.error_len() {
                            window.push('\u{fffd}');
                            consumed += n;
                        } else if count == 0 {
                            window.push('\u{fffd}');
                            consumed = undecoded.len();
                        } else {
                            break;
                        }
                    }
                }
            }
            undecoded.drain(..consumed);
            if let Some(i) = window.find(&needle) {
                stable(&file)?;
                return Ok(Some(
                    line + window[..i].bytes().filter(|b| *b == b'\n').count(),
                ));
            }
            if count == 0 {
                stable(&file)?;
                return Ok(None);
            }
            let mut cut = window.len().saturating_sub(needle.len().saturating_sub(1));
            while !window.is_char_boundary(cut) {
                cut -= 1;
            }
            line += window[..cut].bytes().filter(|b| *b == b'\n').count();
            window.drain(..cut);
        }
    }
}
