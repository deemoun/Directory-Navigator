use super::theme::entry_color;
use crate::{
    columns::ColumnPreset,
    config::Config,
    filesystem::display_name,
    formatting,
    panel::Panel,
    panel_layout::{PanelGeometry, PanelLayout},
    quick_search::QuickSearch,
};
use ratatui::{
    layout::{Constraint, Rect},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Cell, Paragraph, Row, Table},
    Frame,
};

pub(super) fn draw_panel(
    frame: &mut Frame,
    panel: &Panel,
    area: Rect,
    state: (usize, bool),
    config: &Config,
    search: Option<&QuickSearch>,
    geometry: &PanelGeometry,
) {
    let (index, active) = state;
    let profile = config.palette;
    let dos = profile.styles();
    let border = Block::default()
        .title(format!(
            " {} ",
            formatting::tail(
                &display_name(&panel.path.to_string_lossy()),
                area.width.saturating_sub(4)
            )
        ))
        .title_style(dos(if active { 0x3f } else { 0x8f }))
        .borders(Borders::ALL)
        .border_type(if active {
            BorderType::Double
        } else {
            BorderType::Plain
        })
        .border_style(dos(if active {
            if profile == crate::palette::PaletteProfile::ClassicDn {
                0x8f
            } else {
                0x3f
            }
        } else {
            0x87
        }))
        .style(dos(0x87));
    let inner = border.inner(area);
    frame.render_widget(border, area);
    let layout = config.layouts[index];
    let preset = if layout == PanelLayout::Brief {
        ColumnPreset::Name
    } else {
        config.columns[index]
    };
    for (column_index, column) in geometry.columns.iter().enumerate() {
        let date_time = inner.width >= 50;
        let mut headers = vec![" Name"];
        let mut constraints = vec![Constraint::Min(1)];
        if preset != ColumnPreset::Name {
            headers.push("Size");
            constraints.push(Constraint::Length(9));
        }
        if preset == ColumnPreset::NameSizeModified {
            headers.push("Modified");
            constraints.push(Constraint::Length(if date_time { 16 } else { 10 }));
        }
        frame.render_widget(
            Table::new([Row::new(headers)], constraints.clone())
                .column_spacing(1)
                .style(dos(0x8e)),
            Rect::new(column.x, geometry.header.y, column.width, 1),
        );
        let count = geometry.rows();
        let rows = panel
            .entries
            .iter()
            .enumerate()
            .skip(panel.offset + column_index * count)
            .take(count)
            .map(|(index, entry)| {
                let name = display_name(&entry.name.to_string_lossy());
                let selected = panel.selected.contains(&entry.name);
                let marker = if selected && config.show_tags {
                    if profile == crate::palette::PaletteProfile::ClassicDn {
                        "√"
                    } else {
                        "✓"
                    }
                } else if entry.symlink {
                    "@"
                } else {
                    " "
                };
                let size = if entry.directory {
                    "<DIR>".into()
                } else if entry.symlink {
                    "<LINK>".into()
                } else {
                    entry
                        .size
                        .map(formatting::size)
                        .unwrap_or_else(|| "?".into())
                };
                let normal = if config.highlight_files {
                    entry_color(entry)
                } else if entry.directory {
                    0x8f
                } else {
                    0x87
                };
                let color = if active && index == panel.cursor {
                    if selected {
                        0x3e
                    } else {
                        0x30
                    }
                } else if selected {
                    0x8e
                } else {
                    normal
                };
                // FLPANEL.PAS DrawAtIdx keeps the tag's normal file-type
                // foreground, unless it would disappear into the cursor background.
                let marker_color = if selected
                    && config.show_tags
                    && profile == crate::palette::PaletteProfile::ClassicDn
                    && (normal & 0x0f) != (color >> 4)
                {
                    (color & 0xf0) | (normal & 0x0f)
                } else {
                    color
                };
                let mut cells = vec![Cell::from(Line::from(vec![
                    Span::styled(marker, dos(marker_color)),
                    Span::styled(name, dos(color)),
                ]))];
                if preset != ColumnPreset::Name {
                    cells.push(Cell::from(size));
                }
                if preset == ColumnPreset::NameSizeModified {
                    cells.push(Cell::from(formatting::modified(entry.modified, date_time)));
                }
                Row::new(cells).style(dos(color))
            });
        frame.render_widget(
            Table::new(rows, constraints)
                .column_spacing(1)
                .style(dos(0x87)),
            *column,
        );
    }
    let bar = geometry.scrollbar;
    if !bar.is_empty() {
        let attr = if active && !panel.loading && panel.entries.len() > 1 {
            0x70
        } else {
            0x78
        };
        for y in bar.y..bar.bottom() {
            let glyph = if y == bar.y {
                "▲"
            } else if y + 1 == bar.bottom() {
                "▼"
            } else if panel.entries.len() <= 1 {
                "▓"
            } else {
                "▒"
            };
            frame.render_widget(
                Paragraph::new(glyph).style(dos(attr)),
                Rect::new(bar.x, y, 1, 1),
            );
        }
        if let Some(thumb) = geometry.scroll_thumb(panel.cursor, panel.entries.len()) {
            frame.render_widget(Paragraph::new("■").style(dos(attr)), thumb);
        }
    }
    for divider in &geometry.dividers {
        for y in divider.y..divider.bottom() {
            frame.render_widget(
                Paragraph::new("│").style(dos(0x8f)),
                Rect::new(divider.x, y, 1, 1),
            );
        }
    }
    frame.render_widget(
        Paragraph::new("─".repeat(inner.width as usize)).style(dos(0x8f)),
        geometry.separator,
    );
    if !panel.filter.is_empty() {
        let suffix = format!(" · {} filtered marks ", panel.filtered_marks());
        let label = format!(
            " Filter:{}{suffix}",
            formatting::tail(
                &display_name(&panel.filter),
                inner.width.saturating_sub(suffix.len() as u16 + 8)
            )
        );
        frame.render_widget(Paragraph::new(label).style(dos(0x8b)), geometry.separator);
    }
    if let Some(entry) = panel.entries.get(panel.cursor).filter(|_| !panel.loading) {
        let size = if entry.parent || entry.directory {
            "<SUB-DIR>".into()
        } else if entry.symlink {
            "<LINK>".into()
        } else {
            entry
                .size
                .map(|n| formatting::grouped_bytes(u128::from(n)))
                .unwrap_or_else(|| "?".into())
        };
        let modified = entry
            .modified
            .and_then(formatting::local_timestamp)
            .map(|time| time.format("%d.%m.%y %H:%M").to_string())
            .unwrap_or_default();
        let size_width = (size.len() as u16)
            .min(inner.width.saturating_sub(22))
            .max(9);
        let size = if size.len() > usize::from(size_width) {
            entry.size.map(formatting::size).unwrap_or(size)
        } else {
            size
        };
        let name_width = inner.width.saturating_sub(size_width + 17);
        let name = formatting::tail(&display_name(&entry.name.to_string_lossy()), name_width);
        frame.render_widget(
            Table::new(
                [Row::new(vec![format!(" {name}"), size, modified])],
                [
                    Constraint::Min(1),
                    Constraint::Length(size_width),
                    Constraint::Length(14),
                ],
            )
            .column_spacing(1)
            .style(dos(0x8b)),
            geometry.detail,
        );
    }
    let info = if let Some(search) = search {
        let suffix = if search.no_match { " · No match" } else { "" };
        format!(
            " Find: {}▌{suffix}",
            formatting::tail(
                &search.query,
                inner.width.saturating_sub(8 + suffix.len() as u16)
            )
        )
    } else if panel.loading {
        " Reading directory…".into()
    } else {
        let marked = if panel.selected.is_empty() {
            String::new()
        } else {
            format!(
                "{} marked · ",
                panel.selected.len() - panel.filtered_marks()
            )
        };
        format!(
            " {marked}{} {}, {}{} bytes",
            panel.total_files,
            if panel.total_files == 1 {
                "file"
            } else {
                "files"
            },
            formatting::grouped_bytes(panel.total_bytes),
            if panel.unknown_sizes { " + ?" } else { "" }
        )
    };
    frame.render_widget(
        Paragraph::new(info).style(dos(if search.is_some() { 0x30 } else { 0x8b })),
        geometry.totals,
    );
}
