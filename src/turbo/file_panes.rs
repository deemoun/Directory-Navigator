//! DN's column-major file lists rendered by native ListBox/ListViewer controls.
//! Rich row colours and native path identity remain model policy, as in FLPANEL.
use super::scene::Scene;
use crate::{
    config::Config, filesystem::display_name, formatting, palette::PaletteProfile, panel::Panel,
    presentation::layout::Rect,
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
fn field(value: &str, width: u16) -> String {
    let mut result = String::new();
    let mut used = 0;
    for glyph in value.graphemes(true) {
        let w = glyph.width();
        if used + w > usize::from(width) {
            break;
        }
        result.push_str(glyph);
        used += w;
    }
    result.push_str(&" ".repeat(usize::from(width) - used));
    result
}
pub(crate) struct Column<'a> {
    pub rect: Rect,
    pub start: usize,
    pub fields: &'a [Rect],
    pub date_time: bool,
}
pub(crate) fn column(
    scene: &mut Scene,
    panel: &Panel,
    config: &Config,
    active: bool,
    layout: Column<'_>,
) {
    let Column {
        rect,
        start,
        fields,
        date_time,
    } = layout;
    let classic = config.palette == PaletteProfile::ClassicDn;
    let mut rows = Vec::new();
    let mut colours = Vec::new();
    for (index, entry) in panel
        .entries
        .iter()
        .enumerate()
        .skip(start)
        .take(rect.height as usize)
    {
        let marked = panel.selected.contains(&entry.name);
        let marker = if marked && config.show_tags {
            if classic {
                "√"
            } else {
                "✓"
            }
        } else if entry.symlink {
            "@"
        } else {
            " "
        };
        let normal = if config.highlight_files {
            crate::ui::entry_color(entry)
        } else if entry.directory {
            0x8f
        } else {
            0x87
        };
        let colour = if active && index == panel.cursor {
            if marked {
                0x3e
            } else {
                0x30
            }
        } else if marked {
            0x8e
        } else {
            normal
        };
        let marker_colour =
            if marked && config.show_tags && classic && (normal & 15) != (colour >> 4) {
                (colour & 0xf0) | (normal & 15)
            } else {
                colour
            };
        let mut text = String::new();
        let mut position = rect.x;
        for (i, f) in fields.iter().enumerate() {
            text.push_str(&" ".repeat(f.x.saturating_sub(position) as usize));
            let value = match i {
                0 => format!("{marker}{}", display_name(&entry.name.to_string_lossy())),
                1 => {
                    if entry.directory {
                        "<DIR>".into()
                    } else if entry.symlink {
                        "<LINK>".into()
                    } else {
                        entry
                            .size
                            .map(formatting::size)
                            .unwrap_or_else(|| "?".into())
                    }
                }
                _ => formatting::modified_with(entry.modified, date_time, config.datetime),
            };
            text.push_str(&field(&value, f.width));
            position = f.right();
        }
        rows.push(text);
        colours.push((colour, marker_colour));
    }
    super::widgets::paint_into(
        scene,
        super::widgets::list_selection(rows, None, 0, active),
        rect,
    );
    for y in rect.y..rect.bottom() {
        let (colour, marker) = colours
            .get((y - rect.y) as usize)
            .copied()
            .unwrap_or((0x87, 0x87));
        for x in rect.x..rect.right() {
            if let Some(cell) = scene.cells.get_mut(&(x, y)) {
                cell.attribute = if x == rect.x { marker } else { colour };
                cell.style = None;
            }
        }
    }
}
