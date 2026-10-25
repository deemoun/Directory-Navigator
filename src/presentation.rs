//! Remaining Ratatui *data* compatibility boundary.
//! No Terminal, Frame, backend or widget renderer is exported here. Replacing
//! these layout/style/text values is the final dependency-removal step.
pub use ratatui::{layout, style, text};
