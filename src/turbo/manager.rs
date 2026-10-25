//! Native file-manager drawing. Directory models, workers and guarded commands
//! are reused; no Ratatui widgets compose the panes or shell on this path.
use super::scene::Scene;
use crate::{
    app::App,
    columns::ColumnPreset,
    config::Config,
    filesystem::display_name,
    formatting,
    layout::Prepared,
    mouse::Target,
    palette::PaletteProfile,
    panel::Panel,
    panel_layout::{PanelGeometry, PanelLayout},
    quick_search::QuickSearch,
};
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use std::collections::BTreeMap;
use unicode_width::UnicodeWidthStr;

fn plain(s: &mut Scene, r: Rect, text: &str, attr: u8) {
    s.fill(r.x, r.y, r.width, r.height, attr);
    s.text(r.x, r.y, r.width, text, attr);
}
fn runs(s: &mut Scene, r: Rect, items: &[(String, u8)], base: u8, center: bool) {
    s.fill(r.x, r.y, r.width, r.height, base);
    let width: usize = items.iter().map(|(text, _)| text.width()).sum();
    let mut x = r.x
        + if center {
            (r.width / 2).saturating_sub(width.min(usize::from(r.width)) as u16 / 2)
        } else {
            0
        };
    for (text, attr) in items {
        s.text(x, r.y, r.right().saturating_sub(x), text, *attr);
        x = x.saturating_add(text.width() as u16);
        if x >= r.right() {
            break;
        }
    }
}
fn cells(r: Rect, constraints: Vec<Constraint>) -> Vec<Rect> {
    Layout::horizontal(constraints)
        .flex(Flex::Start)
        .spacing(1)
        .split(r)
        .to_vec()
}
fn pane(
    s: &mut Scene,
    panel: &Panel,
    area: Rect,
    state: (usize, bool),
    config: &Config,
    search: Option<&QuickSearch>,
    g: &PanelGeometry,
) {
    let (index, active) = state;
    let classic = config.palette == PaletteProfile::ClassicDn;
    s.fill(area.x, area.y, area.width, area.height, 0x87);
    s.border(
        area.x,
        area.y,
        area.width,
        area.height,
        active,
        if active {
            if classic {
                0x8f
            } else {
                0x3f
            }
        } else {
            0x87
        },
    );
    s.text(
        area.x + 1,
        area.y,
        area.width - 2,
        &format!(
            " {} ",
            formatting::tail(
                &display_name(&panel.path.to_string_lossy()),
                area.width.saturating_sub(4)
            )
        ),
        if active { 0x3f } else { 0x8f },
    );
    let inner_width = area.width - 2;
    let preset = if config.layouts[index] == PanelLayout::Brief {
        ColumnPreset::Name
    } else {
        config.columns[index]
    };
    for (column_index, column) in g.columns.iter().enumerate() {
        let date_time = inner_width >= 50;
        let mut constraints = vec![Constraint::Min(1)];
        let mut headers = vec![config.language.text(" Name")];
        if preset != ColumnPreset::Name {
            constraints.push(Constraint::Length(9));
            headers.push(config.language.text("Size"));
        }
        if preset == ColumnPreset::NameSizeModified {
            constraints.push(Constraint::Length(if date_time {
                config.datetime.detail_width()
            } else {
                10
            }));
            headers.push(config.language.text("Modified"));
        }
        let header = Rect::new(column.x, g.header.y, column.width, 1);
        s.fill(header.x, header.y, header.width, 1, 0x8e);
        for (r, text) in cells(header, constraints.clone()).iter().zip(headers) {
            s.text(r.x, r.y, r.width, text, 0x8e);
        }
        for (row, (entry_index, entry)) in panel
            .entries
            .iter()
            .enumerate()
            .skip(panel.offset + column_index * g.rows())
            .take(g.rows())
            .enumerate()
        {
            let selected = panel.selected.contains(&entry.name);
            let marker = if selected && config.show_tags {
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
            let color = if active && entry_index == panel.cursor {
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
            let marker_color =
                if selected && config.show_tags && classic && (normal & 15) != (color >> 4) {
                    (color & 0xf0) | (normal & 15)
                } else {
                    color
                };
            let row = Rect::new(column.x, column.y + row as u16, column.width, 1);
            s.fill(row.x, row.y, row.width, 1, color);
            let cols = cells(row, constraints.clone());
            if let Some(name) = cols.first() {
                s.text(name.x, name.y, name.width, marker, marker_color);
                if name.width > 1 {
                    s.text(
                        name.x + 1,
                        name.y,
                        name.width - 1,
                        &display_name(&entry.name.to_string_lossy()),
                        color,
                    );
                }
            }
            if let Some(size) = cols.get(1) {
                let value = if entry.directory {
                    "<DIR>".into()
                } else if entry.symlink {
                    "<LINK>".into()
                } else {
                    entry
                        .size
                        .map(formatting::size)
                        .unwrap_or_else(|| "?".into())
                };
                s.text(size.x, size.y, size.width, &value, color);
            }
            if let Some(modified) = cols.get(2) {
                s.text(
                    modified.x,
                    modified.y,
                    modified.width,
                    &formatting::modified_with(entry.modified, date_time, config.datetime),
                    color,
                );
            }
        }
    }
    let bar = g.scrollbar;
    if !bar.is_empty() {
        let enabled = !panel.loading && panel.entries.len() > 1;
        let attr = if classic {
            if enabled {
                0xf0
            } else {
                0xf8
            }
        } else if active && enabled {
            0x70
        } else {
            0x78
        };
        for y in bar.y..bar.bottom() {
            let glyph = if y == bar.y {
                "▲"
            } else if y + 1 == bar.bottom() {
                "▼"
            } else if !classic && panel.entries.len() <= 1 {
                "▓"
            } else {
                "▒"
            };
            s.text(bar.x, y, 1, glyph, attr);
        }
        if let Some(thumb) = g.scroll_thumb(panel.cursor, panel.entries.len()) {
            s.text(thumb.x, thumb.y, 1, "■", attr);
        }
    }
    for divider in &g.dividers {
        for y in divider.y..divider.bottom() {
            s.text(divider.x, y, 1, "│", 0x8f);
        }
    }
    plain(s, g.separator, &"─".repeat(inner_width as usize), 0x8f);
    if !panel.filter.is_empty() {
        let suffix = format!(
            " · {} {} ",
            panel.filtered_marks(),
            config.language.text("filtered marks")
        );
        for x in g.separator.x..g.separator.right() {
            if let Some(glyph) = s.cells.get_mut(&(x, g.separator.y)) {
                glyph.attribute = 0x8b;
            }
        }
        s.text(
            g.separator.x,
            g.separator.y,
            g.separator.width,
            &format!(
                " {}:{}{suffix}",
                config.language.text("Filter"),
                formatting::tail(
                    &display_name(&panel.filter),
                    inner_width.saturating_sub(suffix.width() as u16 + 8)
                )
            ),
            0x8b,
        );
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
            .map(|time| config.datetime.stamp(&time, true, false))
            .unwrap_or_default();
        let size_width = (size.len() as u16)
            .min(inner_width.saturating_sub(22))
            .max(9);
        let size = if size.len() > usize::from(size_width) {
            entry.size.map(formatting::size).unwrap_or(size)
        } else {
            size
        };
        let name_width =
            inner_width.saturating_sub(size_width + config.datetime.footer_width() + 3);
        let name = format!(
            " {}",
            formatting::tail(&display_name(&entry.name.to_string_lossy()), name_width)
        );
        s.fill(g.detail.x, g.detail.y, g.detail.width, 1, 0x8b);
        for (r, text) in cells(
            g.detail,
            vec![
                Constraint::Min(1),
                Constraint::Length(size_width),
                Constraint::Length(config.datetime.footer_width()),
            ],
        )
        .iter()
        .zip([name, size, modified])
        {
            s.text(r.x, r.y, r.width, &text, 0x8b);
        }
    }
    if let Some(search) = search {
        let suffix = if search.no_match {
            config.language.text(" · No match")
        } else {
            ""
        };
        plain(
            s,
            g.totals,
            &format!(
                " {}: {}▌{suffix}",
                config.language.text("Find"),
                formatting::tail(
                    &search.query,
                    inner_width.saturating_sub(8 + suffix.width() as u16)
                )
            ),
            0x30,
        );
    } else if panel.loading {
        plain(
            s,
            g.totals,
            config.language.text(" Reading directory…"),
            0x8b,
        );
    } else {
        let text = crate::panel_status::selection(panel, config, g.totals.width);
        runs(
            s,
            g.totals,
            &text
                .spans
                .into_iter()
                .map(|v| (v.content, v.attribute))
                .collect::<Vec<_>>(),
            0x87,
            true,
        );
    }
    let text = crate::panel_status::space(panel, config, g.space.width);
    runs(
        s,
        g.space,
        &text
            .spans
            .into_iter()
            .map(|v| (v.content, v.attribute))
            .collect::<Vec<_>>(),
        0x87,
        true,
    );
}

pub(crate) fn prepare(
    app: &mut App,
    prepared: &Prepared,
    width: u16,
    height: u16,
) -> (Scene, Vec<Rect>) {
    app.desktop.native().sync_panes(app.active, (width, height));
    let mut s = Scene {
        width,
        height,
        profile: app.config.palette,
        cells: BTreeMap::new(),
        cursor: None,
        underlay: true,
    };
    s.fill(0, 0, width, height, 0x87);
    let bar = prepared.regions[0];
    s.fill(bar.x, bar.y, bar.width, 1, 0x70);
    let clock = app.config.show_clock && app.desktop.game().is_none();
    let header = Rect {
        width: bar.width.saturating_sub(if clock {
            app.config.datetime.clock_width()
        } else {
            0
        }),
        ..bar
    };
    let mut x = bar.x;
    let mut positions = Vec::new();
    for (index, label) in crate::menu::LABELS.iter().enumerate() {
        let label = app.config.language.text(label);
        let width = crate::menu::plain(label).width() as u16 + 3;
        let r = Rect::new(x, bar.y, width, 1).intersection(header);
        let selected = app.menu.is_some_and(|(root, _)| root == index);
        let normal = if selected { 0x30 } else { 0x70 };
        let hot = if selected { 0x3e } else { 0x74 };
        let mut items = vec![(" ".into(), normal)];
        items.extend(
            label
                .split('~')
                .enumerate()
                .map(|(i, v)| (v.into(), if i % 2 == 1 { hot } else { normal })),
        );
        items.push(("  ".into(), normal));
        runs(&mut s, r, &items, 0x70, false);
        positions.push(r);
        app.mouse.regions.push((r, Target::Menu(index)));
        x += width;
    }
    if clock {
        plain(
            &mut s,
            Rect::new(
                bar.right() - app.config.datetime.clock_width(),
                bar.y,
                app.config.datetime.clock_width(),
                1,
            ),
            &app.clock.text,
            0x71,
        );
    }
    for (index, area) in prepared.panel_areas.iter().enumerate() {
        if app
            .quick_view
            .as_ref()
            .is_some_and(|q| q.source ^ 1 == index)
        {
            continue;
        }
        pane(
            &mut s,
            &app.panels[index],
            *area,
            (index, index == app.active),
            &app.config,
            app.quick_search.as_ref().filter(|q| q.panel == index),
            &prepared.panels[index],
        );
    }
    let (command, cursor) = crate::command_line::presentation(app, prepared.regions[2]);
    plain(&mut s, prepared.regions[2], &command, 0x07);
    s.cursor = cursor;
    let buttons = Layout::horizontal([Constraint::Percentage(10); 10]).split(prepared.regions[3]);
    for (index, label) in [
        "Help", "User", "View", "Edit", "Copy", "Move", "MkDir", "Delete", "Menu", "Quit",
    ]
    .iter()
    .enumerate()
    {
        let label = if app.config.palette == PaletteProfile::ClassicDn && *label == "Delete" {
            "Del"
        } else {
            label
        };
        let label = app.config.language.text(label);
        let key = if app.config.shortcuts.is_mac() {
            format!("^O{} ", (index + 1) % 10)
        } else if app.config.palette == PaletteProfile::ClassicDn {
            format!("F{} ", index + 1)
        } else {
            (index + 1).to_string()
        };
        let label = if app.config.shortcuts.is_mac() && label == "MkDir" {
            "Dir"
        } else {
            label
        };
        let keyattr = if app.config.palette == PaletteProfile::ClassicDn {
            0x74
        } else {
            0x07
        };
        let attr = if app.config.palette == PaletteProfile::ClassicDn {
            0x70
        } else {
            0x30
        };
        runs(
            &mut s,
            buttons[index],
            &[(key, keyattr), (label.into(), attr)],
            attr,
            false,
        );
    }
    (s, positions)
}
