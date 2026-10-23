//! Independent reader for the documented DN2 CFG block format, not Pascal objects.
use crate::{config::Config, panel::SortMode, task::Cancellation};
use std::{
    collections::HashSet,
    io::{self, Read},
    path::Path,
};
pub const SIGNATURE: &[u8] = b"DN2 Configuration\x1a\x01\x24";
pub const LIMIT: usize = 1024 * 1024;
#[derive(Clone, Debug)]
pub enum Setting {
    Clock(bool),
    Hidden(bool),
    Sort(SortMode),
    Highlight(bool),
    Tags(bool),
    Space(bool),
    Delete(bool),
    Indent(bool),
    Tab(u8),
    Saver(u8),
}
impl Setting {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Clock(_) => "Show clock",
            Self::Hidden(_) => "Show hidden files",
            Self::Sort(_) => "Both panel sorts",
            Self::Highlight(_) => "Highlight groups",
            Self::Tags(_) => "Selection markers",
            Self::Space(_) => "Space marks entries",
            Self::Delete(_) => "Delete key enabled",
            Self::Indent(_) => "Editor auto-indent",
            Self::Tab(_) => "Editor tab width",
            Self::Saver(_) => "Screensaver minutes",
        }
    }
    pub fn apply(&self, config: &mut Config) {
        match *self {
            Self::Clock(v) => config.show_clock = v,
            Self::Hidden(v) => config.show_hidden = v,
            Self::Sort(v) => config.sorts = [v; 2],
            Self::Highlight(v) => config.highlight_files = v,
            Self::Tags(v) => config.show_tags = v,
            Self::Space(v) => config.space_select = v,
            Self::Delete(v) => config.delete_key = v,
            Self::Indent(v) => config.editor.auto_indent = v,
            Self::Tab(v) => config.editor.tab_size = v,
            Self::Saver(v) => config.saver_minutes = v,
        }
    }
    pub fn value(&self, config: &Config) -> String {
        match self {
            Self::Clock(_) => config.show_clock.to_string(),
            Self::Hidden(_) => config.show_hidden.to_string(),
            Self::Sort(_) => format!(
                "{} / {}",
                sort_name(config.sorts[0]),
                sort_name(config.sorts[1])
            ),
            Self::Highlight(_) => config.highlight_files.to_string(),
            Self::Tags(_) => config.show_tags.to_string(),
            Self::Space(_) => config.space_select.to_string(),
            Self::Delete(_) => config.delete_key.to_string(),
            Self::Indent(_) => config.editor.auto_indent.to_string(),
            Self::Tab(_) => config.editor.tab_size.to_string(),
            Self::Saver(_) => config.saver_minutes.to_string(),
        }
    }
}
fn sort_name(sort: SortMode) -> &'static str {
    match sort {
        SortMode::Name => "name",
        SortMode::Extension => "extension",
        SortMode::Size => "size",
        SortMode::Date => "date",
    }
}
#[derive(Debug, Default)]
pub struct Report {
    pub settings: Vec<Setting>,
    pub skipped: Vec<String>,
}
impl Report {
    pub fn draft(&self, current: &Config) -> Config {
        let mut result = current.clone();
        for setting in &self.settings {
            setting.apply(&mut result);
        }
        result
    }
    pub fn changed(&self, current: &Config) -> bool {
        self.draft(current) != *current
    }
}
fn bad(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
fn word(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

pub fn read(path: &Path, cancel: &Cancellation) -> io::Result<Report> {
    cancel.check()?;
    let input = crate::filesystem::open_regular(path)?;
    let mut bytes = Vec::new();
    input.take(LIMIT as u64 + 1).read_to_end(&mut bytes)?;
    cancel.check()?;
    parse_with(&bytes, cancel)
}
pub fn parse(bytes: &[u8]) -> io::Result<Report> {
    parse_with(bytes, &Cancellation::default())
}
fn parse_with(bytes: &[u8], cancel: &Cancellation) -> io::Result<Report> {
    if bytes.len() > LIMIT {
        return Err(bad("DOS configuration exceeds 1 MiB"));
    }
    if !bytes.starts_with(SIGNATURE) {
        return Err(bad(
            "Unsupported DOS configuration signature (DN.CFG required, not DN.DSK)",
        ));
    }
    let mut offset = SIGNATURE.len();
    let mut seen = HashSet::new();
    let mut report = Report::default();
    let mut unknown_blocks = 0usize;
    loop {
        cancel.check()?;
        let header = bytes
            .get(offset..offset + 4)
            .ok_or_else(|| bad("Truncated DOS configuration or missing terminator"))?;
        offset += 4;
        let id = word(header, 0);
        let size = usize::from(word(header, 2));
        if id == 0 {
            if size != 0 || offset != bytes.len() {
                return Err(bad("Invalid DOS configuration terminator"));
            }
            break;
        }
        let block = bytes
            .get(offset..offset + size)
            .ok_or_else(|| bad("Truncated DOS configuration record"))?;
        offset += size;
        let expected = match id {
            4 => 2,
            5 => 4,
            7 => 6,
            27 => 264,
            30 => 22,
            32 => 120,
            _ => {
                unknown_blocks += 1;
                if unknown_blocks <= 100 {
                    report.skipped.push(format!("Unsupported block: {id}"));
                }
                continue;
            }
        };
        if !seen.insert(id) {
            return Err(bad(format!(
                "Duplicate supported DOS configuration block: {id}"
            )));
        }
        if size != expected {
            report
                .skipped
                .push(format!("Unfamiliar block layout: {id} ({size} bytes)"));
            continue;
        }
        let flags = word(block, 0);
        match id {
            4 => report.settings.push(Setting::Clock(flags & 0x01 != 0)),
            5 => match flags {
                0..=4 => report
                    .settings
                    .push(Setting::Saver([0, 1, 2, 5, 10][usize::from(flags)])),
                _ => report
                    .skipped
                    .push("Unsupported screensaver delay selector".into()),
            },
            7 => {
                if let Some(mode) = [
                    SortMode::Name,
                    SortMode::Extension,
                    SortMode::Size,
                    SortMode::Date,
                ]
                .get(usize::from(flags))
                {
                    report.settings.push(Setting::Sort(*mode));
                } else {
                    report.skipped.push("Unsupported panel sort mode".into());
                }
                report
                    .settings
                    .push(Setting::Highlight(word(block, 2) & 0x20 != 0));
            }
            27 => {
                report.settings.push(Setting::Space(flags & 0x10 != 0));
                report.settings.push(Setting::Delete(flags & 0x20 != 0));
                report
                    .settings
                    .push(Setting::Tags(word(block, 2) & 0x08 != 0));
            }
            30 => {
                report.settings.push(Setting::Indent(flags & 0x08 != 0));
                let len = usize::from(block[18]);
                let tab = (len <= 3)
                    .then(|| &block[19..19 + len])
                    .and_then(|text| std::str::from_utf8(text).ok())
                    .and_then(|text| text.trim().parse::<u8>().ok())
                    .filter(|n| (1..=16).contains(n));
                if let Some(tab) = tab {
                    report.settings.push(Setting::Tab(tab));
                } else {
                    report.skipped.push("Unsupported editor tab width".into());
                }
            }
            32 => report.settings.push(Setting::Hidden(flags & 0x08 != 0)),
            _ => {}
        }
    }
    if unknown_blocks > 100 {
        report
            .skipped
            .push(format!("Unsupported blocks skipped: {unknown_blocks}"));
    }
    Ok(report)
}
