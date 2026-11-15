//! Observed session health. Only typed transport failures mark a session offline.
use std::{fmt, io};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Ready,
    Busy,
    Disconnected,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Problem {
    Connection,
    Authentication,
    Permission,
    Missing,
    Busy,
    Cancelled,
    Other,
}
impl Problem {
    pub fn of(error: &io::Error) -> Self {
        if transport_failure(error) {
            return if error.kind() == io::ErrorKind::PermissionDenied {
                Self::Authentication
            } else {
                Self::Connection
            };
        }
        match error.kind() {
            io::ErrorKind::PermissionDenied => Self::Permission,
            io::ErrorKind::NotFound => Self::Missing,
            io::ErrorKind::WouldBlock => Self::Busy,
            io::ErrorKind::Interrupted => Self::Cancelled,
            _ => Self::Other,
        }
    }
    pub fn advice(self) -> &'static str {
        match self {
        Self::Connection|Self::Authentication=>"Use Panel → Reconnect to sign in again. Accepted transfers are not restarted automatically.",
        Self::Permission=>"Check remote directory permissions or choose another directory.",
        Self::Missing=>"The remote directory may have moved. Go to its parent or choose another remote path.",
        Self::Busy=>"Wait for the current remote operation, or cancel it in Transfer Center.",
        Self::Cancelled=>"The remote location and its previous listing are retained.",
        Self::Other=>"Refresh the remote directory, or use Panel → Reconnect if the session is no longer usable.",
    }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub problem: Problem,
    pub cause: String,
}
impl Failure {
    pub fn new(error: &io::Error) -> Self {
        Self {
            problem: Problem::of(error),
            cause: error.to_string().chars().take(8192).collect(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub phase: Phase,
    pub error: Option<Failure>,
    pub revision: u64,
}
impl Default for Snapshot {
    fn default() -> Self {
        Self {
            phase: Phase::Ready,
            error: None,
            revision: 0,
        }
    }
}
#[derive(Debug)]
struct Transport(io::Error);
impl fmt::Display for Transport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for Transport {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}
pub(crate) fn transport(error: io::Error) -> io::Error {
    if error.kind() == io::ErrorKind::Interrupted || transport_failure(&error) {
        error
    } else {
        io::Error::new(error.kind(), Transport(error))
    }
}
pub(crate) fn transport_failure(error: &io::Error) -> bool {
    error.get_ref().is_some_and(|e| e.is::<Transport>())
}
