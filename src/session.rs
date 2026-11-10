//! Bounded restart state. Documents always reopen current disk contents.
mod codec;
mod runtime;
use crate::{presentation::layout::Rect, text_encoding::TextFormat};
pub(crate) use codec::{decode, encode};
pub(crate) use runtime::{begin, capture, interaction, poll, remember_quit, Runtime};
use std::path::PathBuf;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RestoreLevel {
    Off,
    #[default]
    Panels,
    Documents,
}
impl RestoreLevel {
    pub fn key(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Panels => "panels",
            Self::Documents => "documents",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Panels => "Panels only",
            Self::Documents => "Panels and documents",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "off" => Some(Self::Off),
            "panels" => Some(Self::Panels),
            "documents" => Some(Self::Documents),
            _ => None,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanelRecord {
    pub path: PathBuf,
    pub cursor: Option<PathBuf>,
    pub filter: String,
    pub offset: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Geometry {
    pub bounds: Rect,
    pub maximized: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Editor,
    Viewer,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentRecord {
    pub path: PathBuf,
    pub kind: Kind,
    pub geometry: Option<Geometry>,
    pub format: Option<TextFormat>,
    pub position: usize,
    pub column: usize,
    pub scroll: usize,
    pub horizontal: u16,
    pub mode: crate::viewer::ViewMode,
    pub markdown: bool,
    pub wrap: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub panels: [PanelRecord; 2],
    pub active_panel: usize,
    pub documents: Vec<DocumentRecord>,
    pub active_document: Option<usize>,
}
pub const MAX_DOCUMENTS: usize = 128;

/// Mark explicit input so delayed restoration never takes focus afterward.
pub fn user_interaction(app: &mut crate::app::App) {
    interaction(app);
}

#[cfg(test)]
mod tests;

pub(crate) const HELP:&str="Session: System Setup offers Off / Panels only / Panels and documents. Save desktop captures it; saving on exit follows its setting. Load desktop reloads preferences only. dn --no-session starts fresh. Marks and passwords are never saved; unsaved text uses normal recovery.";
