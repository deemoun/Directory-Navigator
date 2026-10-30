//! Native file-manager drawing. Directory models, workers and guarded commands
//! are reused; the document paints the panes and shell directly.
use super::scene::Scene;
use crate::presentation::layout::{Constraint, Flex, Layout, Rect};
use crate::{
    app::App,
    columns::ColumnPreset,
    config::Config,
    filesystem::display_name,
    formatting,
    layout::Prepared,
    panel::Panel,
    panel_layout::{PanelGeometry, PanelLayout},
    quick_search::QuickSearch,
};
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
    state: (usize, bool, u16),
    config: &Config,
    search: Option<&QuickSearch>,
    g: &PanelGeometry,
) {
    let (index, active, header_reserve) = state;
    let classic = config.palette.is_classic();
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
        area.width.saturating_sub(2 + header_reserve),
        &format!(
            " {} ",
            formatting::tail(
                &display_name(&panel.path.to_string_lossy()),
                area.width.saturating_sub(4 + header_reserve)
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
        let fields = cells(
            Rect::new(column.x, column.y, column.width, 1),
            constraints.clone(),
        );
        super::file_panes::column(
            s,
            panel,
            config,
            active,
            super::file_panes::Column {
                rect: *column,
                start: panel.offset + column_index * g.rows(),
                fields: &fields,
                date_time,
            },
        );
    }
    let bar = g.scrollbar;
    if !bar.is_empty() {
        super::widgets::paint_into(
            s,
            super::widgets::scrollbar(
                panel.cursor,
                panel.entries.len().saturating_sub(1),
                g.capacity(),
            ),
            bar,
        );
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
            if let Some(cell) = s.cells.get_mut(&(bar.x, y)) {
                cell.attribute = attr;
                cell.style = None;
            }
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
    app: &App,
    prepared: &Prepared,
    width: u16,
    height: u16,
) -> (Scene, Vec<Rect>) {
    let mut s = Scene {
        width,
        height,
        profile: app.config.palette,
        cells: BTreeMap::new(),
        cursor: None,
        regions: Vec::new(),
    };
    s.fill(0, 0, width, height, 0x87);
    let (header, positions) = menu_bar(app, width);
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
            (
                index,
                index == app.active,
                if index == 1
                    && app.config.hide_menu
                    && app.config.show_clock
                    && app.desktop.game().is_none()
                {
                    app.config.datetime.clock_width()
                } else {
                    0
                },
            ),
            &app.config,
            app.modals.quick_search().filter(|q| q.panel == index),
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
        let label = if app.config.palette.is_classic() && *label == "Delete" {
            "Del"
        } else {
            label
        };
        let label = app.config.language.text(label);
        let key = if app.config.shortcuts.is_mac() {
            format!("^O{} ", (index + 1) % 10)
        } else if app.config.palette.is_classic() {
            format!("F{} ", index + 1)
        } else {
            (index + 1).to_string()
        };
        let label = if app.config.shortcuts.is_mac() && label == "MkDir" {
            "Dir"
        } else {
            label
        };
        super::widgets::paint_into(
            &mut s,
            super::widgets::status(&format!("~{key}~{label}")),
            buttons[index],
        );
    }
    s.apply_theme(crate::palette::theme::Group::FilePanel);
    s.cells.extend(header.cells);
    (s, positions)
}

/// A hidden menu is temporarily painted over row zero without moving the panes.
pub(crate) fn menu_bar(app: &App, width: u16) -> (Scene, Vec<Rect>) {
    let mut s = Scene {
        width,
        height: 1,
        profile: app.config.palette,
        cells: BTreeMap::new(),
        regions: Vec::new(),
        cursor: None,
    };
    let bar = Rect::new(0, 0, width, 1);
    let visible = crate::menu::bar_visible(app);
    if visible || !app.config.hide_menu {
        s.fill(0, 0, width, 1, 0x70);
    }
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
        let selected = app.modals.menu().is_some_and(|(root, _)| root == index);
        if visible {
            super::widgets::paint_into(
                &mut s,
                super::widgets::menu_title(label, selected, true),
                r,
            );
        }
        positions.push(r);
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
    {
        for ((x, _), glyph) in s.cells.iter_mut() {
            let group = if clock && *x >= width.saturating_sub(app.config.datetime.clock_width()) {
                crate::palette::theme::Group::Timer
            } else {
                crate::palette::theme::Group::Menus
            };
            if glyph.style.is_none() {
                glyph.style = s.profile.themed(group, glyph.attribute);
            }
        }
    }
    (s, positions)
}
