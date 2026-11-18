//! Bluetooth presentation only: the host owns connections, files and publication.
use super::*;
use crate::{
    painting::{Paragraph, Wrap},
    turbo::dialog_spec::{centered, Control, ControlKind},
};
use crossterm::event::{KeyCode as K, MouseButton, MouseEventKind};
#[derive(Default)]
pub struct BluetoothTransfers {
    focus: usize,
    page: u64,
    page_key: (u64, u8),
    device: usize,
    selected_address: Option<String>,
    queue: usize,
    device_offset: usize,
    queue_offset: usize,
    destination: String,
    cursor: usize,
    native: PathBuf,
}
impl BluetoothTransfers {
    fn action(&mut self, id: usize, ctx: &Context) -> Vec<Effect> {
        use crate::bluetooth::Action as A;
        let action = match id {
            0 => return vec![Effect::Close],
            1 => A::Refresh,
            2 => A::Capture,
            3 => A::Remove(self.queue.min(ctx.bluetooth.queue.len().saturating_sub(1))),
            4 => A::Send(self.device),
            5 => A::Receive,
            6 => A::Stop,
            7 => {
                return vec![Effect::Help {
                    title: " Bluetooth transfers Help ",
                    text: format!(
                        "{}\n\n{}",
                        ctx.language.text(crate::bluetooth::HELP),
                        safe(ctx.language.text(&ctx.bluetooth.notice))
                    ),
                }]
            }
            11 | 12 => {
                let Some(offer) = &ctx.bluetooth.progress.offer else {
                    return vec![];
                };
                A::Approve {
                    offer: offer.id,
                    accept: id == 11,
                    destination: if self.native.to_string_lossy() == self.destination {
                        self.native.clone()
                    } else {
                        PathBuf::from(&self.destination)
                    },
                }
            }
            13 => A::Conflict(crate::operations::conflict::Decision::Replace),
            14 => A::Conflict(crate::operations::conflict::Decision::Skip),
            15 => A::Conflict(crate::operations::conflict::Decision::Cancel),
            _ => return vec![],
        };
        vec![Effect::Bluetooth {
            generation: ctx.bluetooth.generation,
            action,
        }]
    }
}
fn safe(value: &str) -> String {
    value
        .lines()
        .map(crate::filesystem::display_name)
        .collect::<Vec<_>>()
        .join("\n")
}
fn button(
    s: &mut DialogSpec<'static>,
    ctx: &Context,
    id: usize,
    bounds: Rect,
    label: &str,
    enabled: bool,
) {
    let mut c = Control::new(
        id,
        bounds,
        ControlKind::Button(ctx.language.text(label).into()),
        ctx.target(id),
    );
    c.enabled = enabled;
    s.control(c);
}
impl ToolPlugin for BluetoothTransfers {
    fn definition(&self) -> Definition {
        Definition::builtin(
            BLUETOOTH_TRANSFERS,
            "Bluetooth transfers",
            Presentation::Document,
        )
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn open(&mut self, ctx: &Context) {
        *self = Self {
            destination: ctx.bluetooth.destination.to_string_lossy().into_owned(),
            native: ctx.bluetooth.destination.clone(),
            ..Default::default()
        };
        self.cursor = self.destination.len();
    }
    fn prepare(&mut self, ctx: &Context) {
        let page_key = if ctx.bluetooth.conflict.is_some() {
            (ctx.bluetooth.generation, 2)
        } else if let Some(offer) = &ctx.bluetooth.progress.offer {
            (offer.id, 1)
        } else {
            (ctx.bluetooth.generation, 0)
        };
        if self.page_key != page_key {
            self.page = self.page.wrapping_add(1);
            self.page_key = page_key;
        }

        if let Some(address) = &self.selected_address {
            self.device = ctx
                .bluetooth
                .devices
                .iter()
                .position(|d| &d.address == address)
                .unwrap_or(0);
        }
        self.device = self
            .device
            .min(ctx.bluetooth.devices.len().saturating_sub(1));
        self.selected_address = ctx
            .bluetooth
            .devices
            .get(self.device)
            .map(|d| d.address.clone());
        self.queue = self.queue.min(ctx.bluetooth.queue.len().saturating_sub(1));
        self.device_offset = self
            .device_offset
            .min(self.device)
            .max(self.device.saturating_sub(2));
        self.queue_offset = self
            .queue_offset
            .min(self.queue)
            .max(self.queue.saturating_sub(3));
    }
    fn native_input_page(&self) -> Option<u64> {
        Some(self.page)
    }
    fn close_action(&self) -> Option<ActionId> {
        Some(ActionId(0))
    }
    fn uses_controls(&self, key: KeyEvent) -> bool {
        !(matches!(self.focus, 8 | 9)
            && matches!(
                key.code,
                K::Up | K::Down | K::Home | K::End | K::PageUp | K::PageDown
            ))
    }
    fn event(&mut self, event: Event<'_>, ctx: &Context) -> Vec<Effect> {
        match event {
            Event::Focus(id) => self.focus = id,
            Event::Action(ActionId(id)) => return self.action(id, ctx),
            Event::Mouse(m, Some(ActionId(id)))
                if m.kind == MouseEventKind::Down(MouseButton::Left) =>
            {
                if (100..356).contains(&id) {
                    self.device = id - 100;
                    self.focus = 8;
                    self.selected_address = ctx
                        .bluetooth
                        .devices
                        .get(self.device)
                        .map(|d| d.address.clone());
                } else if (400..656).contains(&id) {
                    self.queue = id - 400;
                    self.focus = 9;
                } else if id == 10 {
                    self.focus = 10;
                    self.cursor = self.destination.len();
                } else {
                    return self.action(id, ctx);
                }
            }
            Event::Key(k) if matches!(self.focus, 8 | 9) && !ctx.bluetooth.busy => {
                let (cursor, offset, len, rows) = if self.focus == 8 {
                    (
                        &mut self.device,
                        &mut self.device_offset,
                        ctx.bluetooth.devices.len(),
                        3,
                    )
                } else {
                    (
                        &mut self.queue,
                        &mut self.queue_offset,
                        ctx.bluetooth.queue.len(),
                        4,
                    )
                };
                match k.code {
                    K::Up => *cursor = cursor.saturating_sub(1),
                    K::Down => *cursor = (*cursor + 1).min(len.saturating_sub(1)),
                    K::Home => *cursor = 0,
                    K::End => *cursor = len.saturating_sub(1),
                    K::PageUp => *cursor = cursor.saturating_sub(rows),
                    K::PageDown => *cursor = (*cursor + rows).min(len.saturating_sub(1)),
                    _ => (),
                }
                if *cursor < *offset {
                    *offset = *cursor;
                }
                if *cursor >= *offset + rows {
                    *offset = cursor.saturating_sub(rows - 1);
                }
                if self.focus == 8 {
                    self.selected_address = ctx
                        .bluetooth
                        .devices
                        .get(self.device)
                        .map(|d| d.address.clone());
                }
            }
            Event::Key(k) if self.focus == 10 => {
                let old = self.destination.clone();
                let cursor = self.cursor;
                crate::dialog::edit_input(&mut self.destination, &mut self.cursor, k.code);
                if self.destination.len() > 4096 || self.destination.chars().any(char::is_control) {
                    self.destination = old;
                    self.cursor = cursor;
                }
            }
            Event::Paste(text)
                if self.focus == 10
                    && text.len() + self.destination.len() <= 4096
                    && !text.chars().any(char::is_control) =>
            {
                self.destination.insert_str(self.cursor, text);
                self.cursor += text.len();
            }
            _ => (),
        }
        vec![]
    }
    fn controls(&self, ctx: &Context) -> DialogSpec<'_> {
        let r = centered(ctx.area, 76, 23);
        let v = &ctx.bluetooth;
        let mut s = ctx.dialog(
            r,
            self.focus,
            ctx.language.text("Bluetooth transfers"),
            if v.conflict.is_some() {
                14
            } else if v.progress.offer.is_some() {
                12
            } else {
                0
            },
            if v.conflict.is_some() {
                15
            } else if v.progress.offer.is_some() {
                12
            } else {
                0
            },
        );
        if let Some(conflict) = &v.conflict {
            let text = format!(
                "{}\n{}\n{} B\n\n{}",
                ctx.language.text("Existing Bluetooth destination"),
                safe(&conflict.destination.to_string_lossy()),
                conflict.destination_size,
                ctx.language
                    .text("Replace requires confirmation; Skip keeps the existing file.")
            );
            s.caption(
                Paragraph::new(safe(&text)).wrap(Wrap { trim: false }),
                Rect::new(r.x + 2, r.y + 2, r.width - 4, 13),
            );
            for (id, x, label) in [(13, 3, "Replace"), (14, 27, "Skip"), (15, 51, "Cancel")] {
                button(
                    &mut s,
                    ctx,
                    id,
                    Rect::new(r.x + x, r.y + 19, 19, 1),
                    label,
                    true,
                );
            }
            return s;
        }
        if let Some(offer) = &v.progress.offer {
            let text = format!(
                "{}\n{}\n\n{}\n{} B\n\n{}",
                ctx.language.text("Incoming Bluetooth file"),
                crate::filesystem::display_name(&offer.peer),
                safe(&offer.name),
                offer.size,
                ctx.language
                    .text("Choose a local directory and explicitly approve reception.")
            );
            s.caption(
                Paragraph::new(text).wrap(Wrap { trim: false }),
                Rect::new(r.x + 2, r.y + 2, r.width - 4, 11),
            );
            input(&mut s, ctx, self, r.x + 2, r.y + 15, r.width - 5);
            button(
                &mut s,
                ctx,
                11,
                Rect::new(r.x + 8, r.y + 19, 22, 1),
                "Accept",
                true,
            );
            button(
                &mut s,
                ctx,
                12,
                Rect::new(r.x + 43, r.y + 19, 22, 1),
                "Decline",
                true,
            );
            return s;
        }
        s.caption(
            Paragraph::new(ctx.language.text("Paired Bluetooth devices")),
            Rect::new(r.x + 2, r.y + 1, r.width - 4, 1),
        );
        for (id, y, rows, items, cursor, offset, base) in [
            (
                8,
                2,
                3,
                v.devices
                    .iter()
                    .map(|d| {
                        format!(
                            "{} · {}",
                            crate::filesystem::display_name(&d.name),
                            d.address
                        )
                    })
                    .collect::<Vec<_>>(),
                self.device,
                self.device_offset,
                100,
            ),
            (
                9,
                7,
                4,
                v.queue.iter().map(|s| safe(s)).collect(),
                self.queue,
                self.queue_offset,
                400,
            ),
        ] {
            let cursor = cursor.min(items.len().saturating_sub(1));
            let offset = offset.min(cursor);
            let mut c = Control::new(
                id,
                Rect::new(r.x + 2, r.y + y, r.width - 5, rows),
                ControlKind::List {
                    items: items.clone(),
                    cursor,
                    offset,
                },
                ctx.target(id),
            );
            c.enabled = !v.busy;
            c.dn_list = true;
            c.hits = (0..usize::from(rows))
                .filter(|i| offset + i < items.len())
                .map(|i| {
                    (
                        Rect::new(r.x + 2, r.y + y + i as u16, r.width - 5, 1),
                        ctx.target(base + offset + i),
                    )
                })
                .collect();
            s.control(c);
        }
        s.caption(
            Paragraph::new(ctx.language.text("Files in send order")),
            Rect::new(r.x + 2, r.y + 6, r.width - 4, 1),
        );
        input(&mut s, ctx, self, r.x + 2, r.y + 13, r.width - 5);
        let p = &v.progress;
        let text = if v.busy {
            format!(
                "{}\n{} / {} B · {} {}",
                ctx.language.text(&p.phase),
                p.done,
                p.total,
                p.completed,
                ctx.language.text("Completed")
            )
        } else {
            safe(ctx.language.text(&v.notice))
        };
        s.caption(
            Paragraph::new(text).wrap(Wrap { trim: false }),
            Rect::new(r.x + 2, r.y + 14, r.width - 4, 3),
        );
        for (id, x, w, label, enabled) in [
            (1, 2, 17, "Refresh", !v.busy),
            (2, 22, 25, "Use selected files", !v.busy),
            (3, 50, 20, "Remove", !v.busy && !v.queue.is_empty()),
        ] {
            button(
                &mut s,
                ctx,
                id,
                Rect::new(r.x + x, r.y + 18, w, 1),
                label,
                enabled,
            );
        }
        let controls = [
            (
                4,
                "Send queue",
                !v.busy && !v.queue.is_empty() && !v.devices.is_empty(),
            ),
            (5, "Receive", !v.busy),
            (
                6,
                if v.transfer.is_some() {
                    "Transfers"
                } else {
                    "Stop"
                },
                v.busy,
            ),
            (7, "Help", true),
            (0, "Close", true),
        ];
        let labels = controls.map(|(_, label, _)| ctx.language.text(label));
        let bounds = crate::dialog_theme::button_row(
            Rect::new(r.x + 2, r.y + 20, r.width.saturating_sub(4), 1),
            &labels,
            10,
            2,
        );
        for ((id, label, enabled), bounds) in controls.into_iter().zip(bounds) {
            button(&mut s, ctx, id, bounds, label, enabled);
        }
        s
    }
    fn next_wake(&self, _now: Instant) -> Option<Duration> {
        Some(Duration::from_millis(50))
    }
}
fn input(
    s: &mut DialogSpec<'static>,
    ctx: &Context,
    tool: &BluetoothTransfers,
    x: u16,
    y: u16,
    width: u16,
) {
    s.caption(
        Paragraph::new(ctx.language.text("Bluetooth destination directory")),
        Rect::new(x, y - 1, width, 1),
    );
    let column = crate::presentation::text::Line::from(&tool.destination[..tool.cursor]).width();
    let c = Control::new(
        10,
        Rect::new(x, y, width, 1),
        ControlKind::ScrolledInput {
            value: tool.destination.clone(),
            scroll: column.saturating_sub(usize::from(width - 1)),
        },
        ctx.target(10),
    );
    s.control(c);
    if tool.focus == 10 {
        s.cursor = Some((x + column.min(usize::from(width - 1)) as u16, y));
    }
}
