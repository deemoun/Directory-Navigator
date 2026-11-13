//! Bounded session-only received text; no disk paths or clipboard side effects.
#[derive(Clone, Debug)]
pub(crate) struct Received {
    pub serial: u64,
    pub peer: String,
    pub text: String,
}
