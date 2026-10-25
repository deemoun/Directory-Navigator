use crate::{
    config::Config, formatting, localization::Language, panel::Panel, volume_space::Space,
};
use unicode_width::UnicodeWidthStr;
#[derive(Clone)]
pub struct Span {
    pub content: String,
    pub attribute: u8,
}
impl Span {
    fn styled(text: impl Into<String>, attribute: u8) -> Self {
        Self {
            content: text.into(),
            attribute,
        }
    }
}
pub struct Line {
    pub spans: Vec<Span>,
}
impl Line {
    pub fn width(&self) -> usize {
        self.spans.iter().map(|s| s.content.width()).sum()
    }
}
impl From<Vec<Span>> for Line {
    fn from(spans: Vec<Span>) -> Self {
        Self { spans }
    }
}
impl From<Span> for Line {
    fn from(span: Span) -> Self {
        Self { spans: vec![span] }
    }
}
/// Decimal exact values when space permits; binary units with one decimal otherwise.
fn quantity(n: u128, compact: bool) -> String {
    if !compact {
        return formatting::grouped_bytes(n);
    }
    for (shift, unit) in [
        (60, "EiB"),
        (50, "PiB"),
        (40, "TiB"),
        (30, "GiB"),
        (20, "MiB"),
        (10, "KiB"),
    ] {
        let scale = 1_u128 << shift;
        if n >= scale {
            let tenths = n.saturating_mul(10).saturating_add(scale / 2) / scale;
            return format!("{}.{} {unit}", tenths / 10, tenths % 10);
        }
    }
    format!("{n} B")
}
pub fn selection(panel: &Panel, config: &Config, width: u16) -> Line {
    let mut files = 0;
    let mut folders = 0;
    let mut bytes = 0_u128;
    let mut unknown = false;
    if !panel.selected.is_empty() {
        for entry in panel
            .entries
            .iter()
            .filter(|e| !e.parent && panel.selected.contains(&e.name))
        {
            if entry.directory {
                folders += 1;
            } else {
                files += 1;
                if let Some(n) = entry.size {
                    bytes += u128::from(n);
                } else {
                    unknown = true;
                }
            }
        }
    }
    let language = config.language;
    let labels = 0x8e;
    let numbers = 0x8f;
    if files + folders == 0 {
        return Line::from(Span::styled(
            language
                .text(if panel.only_filtered_marks() {
                    "No visible entries selected"
                } else {
                    "No files selected"
                })
                .to_owned(),
            labels,
        ));
    }
    let build = |compact| {
        let mut spans = Vec::new();
        if files > 0 {
            spans.push(Span::styled(
                format!(
                    "{}{}",
                    quantity(bytes, compact),
                    if unknown { " + ?" } else { "" }
                ),
                numbers,
            ));
            spans.push(Span::styled(
                language
                    .text(if compact { " in " } else { " bytes in " })
                    .to_owned(),
                labels,
            ));
            spans.push(Span::styled(files.to_string(), numbers));
            spans.push(Span::styled(
                format!(
                    " {}",
                    if folders == 0 {
                        language.text(if files == 1 {
                            "selected file"
                        } else {
                            "selected files"
                        })
                    } else {
                        language.files(files)
                    }
                ),
                labels,
            ));
        }
        if folders > 0 {
            if files > 0 {
                spans.push(Span::styled(" + ", labels));
            }
            spans.push(Span::styled(folders.to_string(), numbers));
            spans.push(Span::styled(
                format!(
                    " {}",
                    language.text(if files == 0 {
                        if folders == 1 {
                            "selected folder"
                        } else {
                            "selected folders"
                        }
                    } else {
                        if folders == 1 {
                            "folder"
                        } else {
                            "folders"
                        }
                    })
                ),
                labels,
            ));
        }
        Line::from(spans)
    };
    let full = build(false);
    if full.width() <= usize::from(width) {
        full
    } else {
        build(true)
    }
}
fn space_line(space: Space, language: Language, normal: u8, numbers: u8, compact: bool) -> Line {
    Line::from(vec![
        Span::styled(quantity(space.available, compact), numbers),
        Span::styled(
            format!(
                " {} / ",
                language.text(if compact { "free" } else { "free bytes" })
            ),
            normal,
        ),
        Span::styled(quantity(space.total, compact), numbers),
        Span::styled(
            format!(
                " {}",
                language.text(if compact { "total" } else { "total bytes" })
            ),
            normal,
        ),
    ])
}
pub fn space(panel: &Panel, config: &Config, width: u16) -> Line {
    let language = config.language;
    let normal = 0x87;
    let numbers = 0x8f;
    match &panel.space.result {
        Some(Ok(value)) => {
            let full = space_line(*value, language, normal, numbers, false);
            if full.width() <= usize::from(width) {
                full
            } else {
                space_line(*value, language, normal, numbers, true)
            }
        }
        Some(Err(_)) => Line::from(Span::styled(
            language.text("Volume space unavailable").to_owned(),
            normal,
        )),
        None => Line::from(Span::styled(
            language.text("Reading volume space…").to_owned(),
            normal,
        )),
    }
}
