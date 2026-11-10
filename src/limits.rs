//! Shared bounded document budgets; sizes are bytes, not character counts.
pub const VIEWER_BYTES: usize = 64 * 1024 * 1024;
pub const DECODED_BYTES: usize = 192 * 1024 * 1024;
pub const VIEWER_LINES: usize = 1_000_000;
pub const EDITOR_BYTES: usize = 64 * 1024 * 1024;
pub const EDITOR_LINES: usize = 1_000_000;
pub const RECOVERY_BYTES: u64 = 512 * 1024 * 1024;
pub const MARKDOWN_BYTES: usize = 64 * 1024 * 1024;
pub const MARKDOWN_ROWS: usize = 500_000;
pub const MARKDOWN_SPANS: usize = 1_000_000;
