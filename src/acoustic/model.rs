//! Session-only simplex settings. These are deliberately absent from configuration.
use std::path::PathBuf;
use zeroize::Zeroizing;
pub(crate) const MAX_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_PASSWORD: usize = 1024;
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Speed {
    #[default]
    Normal,
    Fast,
    Fastest,
}
impl Speed {
    pub fn next(self) -> Self {
        match self {
            Self::Normal => Self::Fast,
            Self::Fast => Self::Fastest,
            Self::Fastest => Self::Normal,
        }
    }
    pub fn caption(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Fast => "Fast",
            Self::Fastest => "Fastest",
        }
    }
    #[cfg(feature = "acoustic")]
    pub fn protocol(self) -> ggwave::Protocol {
        match self {
            Self::Normal => ggwave::Protocol::AudibleNormal,
            Self::Fast => ggwave::Protocol::AudibleFast,
            Self::Fastest => ggwave::Protocol::AudibleFastest,
        }
    }
}
#[derive(Clone)]
pub(crate) struct Options {
    pub passphrase: Zeroizing<String>,
    pub speed: Speed,
    /// Zero means repeat until Stop; WAV export requires a finite repeat count.
    pub repeats: u8,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            passphrase: Zeroizing::new(String::new()),
            speed: Speed::Normal,
            repeats: 2,
        }
    }
}
impl Options {
    pub fn validate(&self) -> std::io::Result<()> {
        if !(8..=MAX_PASSWORD).contains(&self.passphrase.len())
            || self.passphrase.chars().any(char::is_control)
        {
            return Err(std::io::Error::other(
                "Enter the same passphrase on both devices (8–1024 UTF-8 bytes).",
            ));
        }
        if self.repeats > 3 {
            return Err(std::io::Error::other("Invalid acoustic repeat count"));
        }
        Ok(())
    }
}
pub(crate) enum Operation {
    Send,
    Receive,
    Estimate,
    Export(PathBuf),
    Import(PathBuf),
}
pub(crate) enum Input {
    Text(String),
    File,
    None,
}
pub(crate) struct Request {
    pub operation: Operation,
    pub input: Input,
    pub options: Options,
    pub revision: u64,
}
