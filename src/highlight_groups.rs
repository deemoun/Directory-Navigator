//! DN-style ordered filename groups; directories/links and selection retain priority.
use crate::{filesystem::Entry, pattern::Wildcard};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub enabled: bool,
    pub masks: String,
    pub color: u8,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Groups(pub [Group; 5]);
impl Default for Groups {
    fn default() -> Self {
        Self(
            [
                ("*.bak;*.tmp;*.swp", 8),
                ("*.txt;*.md;*.pdf;*.docx", 15),
                ("*.rs;*.pas;*.c;*.h;*.cpp;*.py;*.js;*.ts;*.toml;*.json", 11),
                ("*.zip;*.tar;*.gz;*.7z;*.rar;*.zst", 13),
                ("*.png;*.jpg;*.svg;*.mp3;*.wav;*.mp4", 12),
            ]
            .map(|(masks, color)| Group {
                enabled: false,
                masks: masks.into(),
                color,
            }),
        )
    }
}
impl Groups {
    pub fn validate(&self) -> std::io::Result<()> {
        for g in &self.0 {
            if g.color > 15
                || g.masks.len() > 512
                || g.masks.chars().any(char::is_control)
                || g.masks.split(';').filter(|s| !s.trim().is_empty()).count() > 32
            {
                return Err(std::io::Error::other("Highlight groups require colors 0–15 and at most 32 masks / 512 bytes per group"));
            }
        }
        Ok(())
    }
    pub fn matching_group(&self, entry: &Entry) -> Option<usize> {
        if entry.parent || entry.directory || entry.symlink {
            return None;
        }
        let name = entry.name.to_str()?.to_lowercase();
        self.0.iter().position(|g| {
            g.enabled
                && g.masks
                    .split(';')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .any(|mask| {
                        Wildcard::new(&mask.to_lowercase()).matches(std::ffi::OsStr::new(&name))
                    })
        })
    }
}

impl Groups {
    pub fn color(&self, entry: &Entry) -> Option<u8> {
        self.matching_group(entry).map(|i| 0x80 | self.0[i].color)
    }
}

pub fn builtin_color(entry: &crate::filesystem::Entry) -> u8 {
    if entry.symlink {
        return 0x89;
    }
    if entry.directory {
        return 0x8f;
    }
    let extension = entry
        .path
        .extension()
        .map(|s| s.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if entry.executable || matches!(extension.as_str(), "exe" | "com" | "bat" | "cmd" | "sh") {
        return 0x8a;
    }
    match extension.as_str() {
        "zip" | "tar" | "gz" | "xz" | "bz2" | "7z" | "rar" | "zst" => 0x8d,
        "rs" | "pas" | "asm" | "c" | "h" | "cpp" | "py" | "js" | "ts" | "json" | "toml"
        | "yaml" | "yml" => 0x8b,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "mp3" | "wav" | "mp4" | "mkv" => 0x8c,
        "md" | "txt" | "pdf" | "docx" => 0x8f,
        _ => 0x87,
    }
}
