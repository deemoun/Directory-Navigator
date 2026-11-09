//! Linux console mouse protocol, adapted from Youta's MIT GPM module.
//! Upstream blob: 76242ed25b27387cafa7496eb457205783268b3f.
//! Copyright (c) 2026 Vitaly Zdanevich and Youta contributors.
//! Socket integration and DN lifecycle safeguards are maintained here.
use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use std::collections::VecDeque;
#[cfg(any(target_os = "linux", all(test, unix)))]
pub(super) mod client;
#[cfg(target_os = "linux")]
pub(crate) use client::{Controller, Shared};
#[cfg(any(target_os = "linux", all(test, unix)))]
use std::io;
#[cfg(target_os = "linux")]
const GPM_CONTROL_SOCKET: &str = "/dev/gpmctl";
const GPM_CONNECT_BYTES: usize = 16;
const GPM_EVENT_BYTES: usize = 28;
const GPM_MAGIC: u32 = 0x4770_6d4c;
const MAX_WHEEL_EVENTS_PER_PACKET: usize = 16;

const GPM_MOVE: u32 = 1;
const GPM_DRAG: u32 = 2;
const GPM_DOWN: u32 = 4;
const GPM_UP: u32 = 8;

const GPM_BUTTON_RIGHT: u8 = 1;
const GPM_BUTTON_MIDDLE: u8 = 2;
const GPM_BUTTON_LEFT: u8 = 4;
const GPM_BUTTON_WHEEL_UP: u8 = 16;
const GPM_BUTTON_WHEEL_DOWN: u8 = 32;

const GPM_MOD_SHIFT: u8 = 1 << 0;
const GPM_MOD_ALT_GR: u8 = 1 << 1;
const GPM_MOD_CONTROL: u8 = 1 << 2;
const GPM_MOD_ALT: u8 = 1 << 3;
const GPM_MOD_SHIFT_LEFT: u8 = 1 << 4;
const GPM_MOD_SHIFT_RIGHT: u8 = 1 << 5;
const GPM_MOD_CONTROL_LEFT: u8 = 1 << 6;
const GPM_MOD_CONTROL_RIGHT: u8 = 1 << 7;

/// One decoded native GPM event packet.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct GpmEvent {
    buttons: u8,
    modifiers: u8,
    virtual_console: u16,
    delta_x: i16,
    delta_y: i16,
    x: i16,
    y: i16,
    event_type: u32,
    clicks: i32,
    margin: i32,
    wheel_x: i16,
    wheel_y: i16,
}

#[cfg(any(target_os = "linux", all(test, unix)))]
const MAX_PENDING: usize = 256;
#[cfg(any(target_os = "linux", all(test, unix)))]
const MAX_EVENTS_PER_PACKET: usize = MAX_WHEEL_EVENTS_PER_PACKET + 3;
#[derive(Default)]
struct Decoder {
    bytes: Vec<u8>,
}
impl Decoder {
    fn frame_size(&self) -> usize {
        GPM_EVENT_BYTES + usize::from(self.bytes.starts_with(&GPM_MAGIC.to_ne_bytes())) * 4
    }
    fn needed(&self) -> usize {
        if self.bytes.len() < 4 {
            4 - self.bytes.len()
        } else {
            self.frame_size() - self.bytes.len()
        }
    }
    fn take(&mut self) -> Option<GpmEvent> {
        if self.bytes.len() < 4 || self.bytes.len() < self.frame_size() {
            return None;
        }
        let start = self.frame_size() - GPM_EVENT_BYTES;
        let packet = decode_event(&self.bytes[start..]);
        self.bytes.clear();
        Some(packet)
    }
}
#[derive(Default)]
struct Mapper {
    pressed: u8,
}
impl Mapper {
    fn map_into(
        &mut self,
        event: GpmEvent,
        vc: u16,
        size: (u16, u16),
        pending: &mut VecDeque<MouseEvent>,
    ) -> Result<(), &'static str> {
        if event.virtual_console != vc {
            return Ok(());
        }
        let bare = event.event_type & 15;
        if !matches!(bare, GPM_MOVE | GPM_DRAG | GPM_DOWN | GPM_UP) || event.buttons & !0x3f != 0 {
            return Err("Invalid GPM event");
        }
        if size.0 == 0 || size.1 == 0 {
            return Ok(());
        }
        let outside = event.x <= 0
            || event.y <= 0
            || event.x as u16 > size.0
            || event.y as u16 > size.1
            || event.margin != 0;
        let (column, row) = if bare == GPM_UP && outside {
            // Release capture outside every hit region. Clamping an outside
            // release onto a border button could accidentally activate it.
            size
        } else {
            (
                zero_based_coordinate(event.x).min(size.0 - 1),
                zero_based_coordinate(event.y).min(size.1 - 1),
            )
        };
        let modifiers = map_modifiers(event.modifiers);
        // Margins/releases still release capture; invalid presses never click a clamped cell.
        if bare == GPM_DOWN && outside {
            return Ok(());
        }
        let legacy_wheel =
            event.buttons & (GPM_BUTTON_WHEEL_UP | GPM_BUTTON_WHEEL_DOWN) != 0 && bare != GPM_UP;
        let wheel = (event.wheel_y != 0 || legacy_wheel).then(|| {
            let up = event.wheel_y > 0
                || (event.wheel_y == 0 && event.buttons & GPM_BUTTON_WHEEL_UP != 0);
            (
                if up {
                    MouseEventKind::ScrollUp
                } else {
                    MouseEventKind::ScrollDown
                },
                usize::from(event.wheel_y.unsigned_abs()).clamp(1, MAX_WHEEL_EVENTS_PER_PACKET),
            )
        });
        let append_wheel = |pending: &mut VecDeque<MouseEvent>| {
            if let Some((kind, count)) = wheel {
                for _ in 0..count {
                    pending.push_back(mouse_event(kind, column, row, modifiers));
                }
            }
        };
        if event.buttons & 7 == 0
            && (event.wheel_x != 0
                || event.buttons & (GPM_BUTTON_WHEEL_UP | GPM_BUTTON_WHEEL_DOWN) != 0)
        {
            append_wheel(pending);
            return Ok(());
        }
        if bare == GPM_MOVE {
            if wheel.is_some() {
                append_wheel(pending);
                return Ok(());
            }
            let ev = mouse_event(MouseEventKind::Moved, column, row, modifiers);
            append_motion(pending, ev);
            return Ok(());
        }
        let buttons = event.buttons & 7;
        if bare == GPM_UP && event.buttons == 0 {
            for bit in [GPM_BUTTON_LEFT, GPM_BUTTON_MIDDLE, GPM_BUTTON_RIGHT] {
                if self.pressed & bit != 0 {
                    pending.push_back(mouse_event(
                        MouseEventKind::Up(map_button(bit).unwrap()),
                        column,
                        row,
                        modifiers,
                    ));
                }
            }
            self.pressed = 0;
            append_wheel(pending);
            return Ok(());
        }
        for bit in [GPM_BUTTON_LEFT, GPM_BUTTON_MIDDLE, GPM_BUTTON_RIGHT] {
            if buttons & bit == 0 {
                continue;
            }
            let button = map_button(bit).unwrap();
            let kind = match bare {
                GPM_DOWN if self.pressed & bit == 0 => {
                    self.pressed |= bit;
                    MouseEventKind::Down(button)
                }
                GPM_UP if self.pressed & bit != 0 => {
                    self.pressed &= !bit;
                    MouseEventKind::Up(button)
                }
                GPM_DRAG if self.pressed & bit != 0 => MouseEventKind::Drag(button),
                _ => continue,
            };
            let event = mouse_event(kind, column, row, modifiers);
            if matches!(kind, MouseEventKind::Drag(_)) {
                append_motion(pending, event);
            } else {
                pending.push_back(event);
            }
        }
        append_wheel(pending);
        Ok(())
    }
}

/// Skip obsolete positions only within one uninterrupted motion sequence.
/// Clicks, releases, wheel input, button changes and modifiers are barriers.
fn append_motion(pending: &mut VecDeque<MouseEvent>, event: MouseEvent) {
    if pending.back().is_some_and(|previous| {
        previous.kind == event.kind && previous.modifiers == event.modifiers
    }) {
        pending.pop_back();
    }
    pending.push_back(event);
}
fn mouse_event(kind: MouseEventKind, column: u16, row: u16, modifiers: KeyModifiers) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers,
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub console: bool,
    pub connected: bool,
    pub position: Option<(u16, u16)>,
    pub pressed: u8,
    pub reason: String,
    pub reset: u64,
}
impl Snapshot {
    #[cfg(any(target_os = "linux", test))]
    pub fn fields(&self) -> crate::utilities::information::Fields {
        vec![
            (
                "Terminal mode".into(),
                if self.console {
                    "Linux virtual console"
                } else {
                    "No virtual console detected"
                }
                .into(),
            ),
            (
                "Color output".into(),
                if self.console {
                    "Console colors (16 foreground / 8 background)"
                } else {
                    "Extended colors (indexed / RGB)"
                }
                .into(),
            ),
            ("Mouse input".into(), self.source().into()),
            (
                "GPM status".into(),
                if self.reason.is_empty() {
                    "Not initialized".into()
                } else {
                    self.reason.clone()
                },
            ),
        ]
    }

    pub fn source(&self) -> &'static str {
        if self.connected {
            "GPM"
        } else if self.console {
            "Keyboard only"
        } else {
            "Terminal"
        }
    }
}
/// Overlay only on the output copy, after layout/hits and saver snapshots are prepared.
pub(crate) fn pointer(scene: &mut crate::turbo::scene::Scene, position: Option<(u16, u16)>) {
    use crate::presentation::style::Modifier;
    use unicode_width::UnicodeWidthStr;
    let Some((x, y)) = pointer_cell(scene, position) else {
        return;
    };
    let glyph = &scene.cells[&(x, y)];
    let width = glyph.text.width().max(1).min(usize::from(scene.width - x)) as u16;
    let mut style = glyph.resolved(scene.profile);
    if style.add_modifier.contains(Modifier::REVERSED) {
        style.add_modifier.remove(Modifier::REVERSED);
        style.sub_modifier.insert(Modifier::REVERSED);
    } else {
        style.sub_modifier.remove(Modifier::REVERSED);
        style.add_modifier.insert(Modifier::REVERSED);
    }
    if style.fg == style.bg {
        style.fg = Some(scene.profile.color(0));
        style.bg = Some(scene.profile.color(7));
    }
    for col in x..x + width {
        if let Some(g) = scene.cells.get_mut(&(col, y)) {
            g.style = Some(style);
        }
    }
}
fn pointer_cell(
    scene: &crate::turbo::scene::Scene,
    position: Option<(u16, u16)>,
) -> Option<(u16, u16)> {
    let (mut x, y) = position.filter(|(x, y)| *x < scene.width && *y < scene.height)?;
    while x > 0 && scene.cells.get(&(x, y)).is_some_and(|g| g.text.is_empty()) {
        x -= 1;
    }
    scene.cells.contains_key(&(x, y)).then_some((x, y))
}

// Older Linux consoles have no alternate screen. Restore the underlying glyph
// explicitly at handoff, rather than leaving the software pointer on the shell.
#[cfg(target_os = "linux")]
static POINTER_RESTORE: std::sync::Mutex<Vec<u8>> = std::sync::Mutex::new(Vec::new());
#[cfg(target_os = "linux")]
pub(crate) fn remember_pointer(scene: &crate::turbo::scene::Scene, position: Option<(u16, u16)>) {
    let patch = pointer_restore(scene, position).unwrap_or_default();
    *POINTER_RESTORE.lock().unwrap_or_else(|e| e.into_inner()) = patch;
}
#[cfg(target_os = "linux")]
pub(crate) fn hide_pointer() {
    use std::io::Write;
    // Also called by the panic hook, which must not wait on the drawing thread.
    if let Ok(mut patch) = POINTER_RESTORE.try_lock() {
        if !patch.is_empty() {
            let mut output = std::io::stdout().lock();
            let _ = output.write_all(&patch);
            let _ = output.flush();
            patch.clear();
        }
    }
}
#[cfg(any(target_os = "linux", all(test, unix)))]
fn pointer_restore(
    scene: &crate::turbo::scene::Scene,
    position: Option<(u16, u16)>,
) -> io::Result<Vec<u8>> {
    use crate::presentation::style::{Color, Modifier};
    use crossterm::{
        cursor::{MoveTo, RestorePosition, SavePosition},
        style::{Attribute, Print, SetAttribute},
    };
    use std::io::Write;
    let Some((x, y)) = pointer_cell(scene, position) else {
        return Ok(Vec::new());
    };
    let glyph = &scene.cells[&(x, y)];
    let style = glyph.resolved(scene.profile);
    let mut patch = Vec::new();
    crossterm::queue!(
        patch,
        SavePosition,
        MoveTo(x, y),
        SetAttribute(Attribute::Reset)
    )?;
    // Pointer restoration bypasses the scene codec, so use the same console
    // conversion explicitly. This patch is only used by the console pointer.
    let colors = crate::terminal::colors::OutputColors::linux_console();
    write!(
        patch,
        "\x1b[{}m\x1b[{}m",
        colors.foreground(
            style.fg.unwrap_or(Color::Reset),
            style.bg.unwrap_or(Color::Reset)
        ),
        colors.parameters(style.bg.unwrap_or(Color::Reset), false)
    )?;
    for (modifier, attribute) in [
        (Modifier::BOLD, Attribute::Bold),
        (Modifier::DIM, Attribute::Dim),
        (Modifier::ITALIC, Attribute::Italic),
        (Modifier::UNDERLINED, Attribute::Underlined),
        (Modifier::REVERSED, Attribute::Reverse),
        (Modifier::CROSSED_OUT, Attribute::CrossedOut),
    ] {
        if style.add_modifier.contains(modifier) {
            crossterm::queue!(patch, SetAttribute(attribute))?;
        }
    }
    crossterm::queue!(patch, Print(&glyph.text), RestorePosition)?;
    Ok(patch)
}
fn map_button(buttons: u8) -> Option<MouseButton> {
    if buttons & GPM_BUTTON_LEFT != 0 {
        Some(MouseButton::Left)
    } else if buttons & GPM_BUTTON_MIDDLE != 0 {
        Some(MouseButton::Middle)
    } else if buttons & GPM_BUTTON_RIGHT != 0 {
        Some(MouseButton::Right)
    } else {
        None
    }
}

fn map_modifiers(modifiers: u8) -> KeyModifiers {
    let mut mapped = KeyModifiers::empty();
    if modifiers & (GPM_MOD_SHIFT | GPM_MOD_SHIFT_LEFT | GPM_MOD_SHIFT_RIGHT) != 0 {
        mapped.insert(KeyModifiers::SHIFT);
    }
    if modifiers & (GPM_MOD_CONTROL | GPM_MOD_CONTROL_LEFT | GPM_MOD_CONTROL_RIGHT) != 0 {
        mapped.insert(KeyModifiers::CONTROL);
    }
    if modifiers & (GPM_MOD_ALT | GPM_MOD_ALT_GR) != 0 {
        mapped.insert(KeyModifiers::ALT);
    }
    mapped
}

fn zero_based_coordinate(coordinate: i16) -> u16 {
    u16::try_from(coordinate.saturating_sub(1)).unwrap_or_default()
}

/// Encodes GPM's native-endian 16-byte `Gpm_Connect` request.
fn encode_connection(virtual_console: u32, pid: i32) -> [u8; GPM_CONNECT_BYTES] {
    let mut encoded = [0_u8; GPM_CONNECT_BYTES];
    encoded[0..2].copy_from_slice(&15_u16.to_ne_bytes());
    encoded[2..4].copy_from_slice(&0_u16.to_ne_bytes());
    encoded[4..6].copy_from_slice(&0_u16.to_ne_bytes());
    encoded[6..8].copy_from_slice(&u16::MAX.to_ne_bytes());
    encoded[8..12].copy_from_slice(&pid.to_ne_bytes());
    encoded[12..16].copy_from_slice(
        &i32::try_from(virtual_console)
            .unwrap_or(i32::MAX)
            .to_ne_bytes(),
    );
    encoded
}

fn decode_event(packet: &[u8]) -> GpmEvent {
    GpmEvent {
        buttons: packet[0],
        modifiers: packet[1],
        virtual_console: read_u16(packet, 2),
        delta_x: read_i16(packet, 4),
        delta_y: read_i16(packet, 6),
        x: read_i16(packet, 8),
        y: read_i16(packet, 10),
        event_type: read_u32(packet, 12),
        clicks: read_i32(packet, 16),
        margin: read_i32(packet, 20),
        wheel_x: read_i16(packet, 24),
        wheel_y: read_i16(packet, 26),
    }
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_ne_bytes([bytes[offset], bytes[offset + 1]])
}

fn read_i16(bytes: &[u8], offset: usize) -> i16 {
    i16::from_ne_bytes([bytes[offset], bytes[offset + 1]])
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_ne_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn read_i32(bytes: &[u8], offset: usize) -> i32 {
    i32::from_ne_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

#[cfg(target_os = "linux")]
pub(crate) fn sync(app: &mut crate::app::App, snapshot: Snapshot) -> bool {
    if snapshot == app.console_mouse {
        return false;
    }
    if snapshot.reset != app.console_mouse.reset {
        crate::button_feedback::cancel_mouse(app);
        app.mouse.reset_click();
        app.desktop.native().cancel_mouse_capture();
    }
    if snapshot.console != app.console_mouse.console
        || snapshot.connected != app.console_mouse.connected
        || snapshot.reason != app.console_mouse.reason
    {
        let fields = snapshot.fields();
        if let Some(info) = &mut app.utilities.state.information {
            info.set_terminal_fields(fields.clone());
        }
        for id in app.desktop.document_ids(crate::desktop::WindowKind::Tool(
            crate::tool_windows::Kind::System,
        )) {
            if let Some(info) = app
                .desktop
                .tool_mut(id)
                .and_then(|t| t.utility.information.as_mut())
            {
                info.set_terminal_fields(fields.clone());
            }
        }
    }
    app.console_mouse = snapshot;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    fn packet(kind: u32, buttons: u8, x: i16, y: i16) -> [u8; 28] {
        let mut bytes = [0; 28];
        bytes[0] = buttons;
        bytes[1] = GPM_MOD_CONTROL;
        bytes[2..4].copy_from_slice(&7u16.to_ne_bytes());
        bytes[8..10].copy_from_slice(&x.to_ne_bytes());
        bytes[10..12].copy_from_slice(&y.to_ne_bytes());
        bytes[12..16].copy_from_slice(&kind.to_ne_bytes());
        bytes
    }
    fn event(kind: u32, buttons: u8, x: i16, y: i16) -> GpmEvent {
        decode_event(&packet(kind, buttons, x, y))
    }
    #[test]
    fn handshake_matches_native_abi_and_consumes_default_console_actions() {
        let request = encode_connection(7, 123);
        assert_eq!(request.len(), 16);
        assert_eq!(read_u16(&request, 0), 15);
        assert_eq!(read_u16(&request, 2), 0);
        assert_eq!(read_u16(&request, 4), 0);
        assert_eq!(read_u16(&request, 6), u16::MAX);
        assert_eq!(read_i32(&request, 8), 123);
        assert_eq!(read_i32(&request, 12), 7);
    }
    #[test]
    fn fragmented_packets_with_optional_magic_have_no_partial_events() {
        for magic in [false, true] {
            let raw = packet(GPM_MOVE, 0, 8, 3);
            let bytes = if magic {
                [GPM_MAGIC.to_ne_bytes().as_slice(), &raw].concat()
            } else {
                raw.to_vec()
            };
            let mut decoder = Decoder::default();
            for (i, byte) in bytes.iter().enumerate() {
                assert!(decoder.needed() > 0);
                decoder.bytes.push(*byte);
                if i + 1 < bytes.len() {
                    assert!(decoder.take().is_none());
                }
            }
            let e = decoder.take().unwrap();
            assert_eq!((e.x, e.y, e.virtual_console), (8, 3, 7));
            assert!(decoder.bytes.is_empty());
            assert!(decoder.take().is_none());
        }
    }
    #[test]
    fn mouse_order_multi_button_release_and_double_click_flags_are_not_replayed() {
        let mut mapper = Mapper::default();
        let mut out = VecDeque::new();
        for (kind, buttons) in [
            (GPM_DOWN | 32, 4),
            (GPM_DOWN, 1),
            (GPM_DRAG, 4),
            (GPM_UP, 0),
        ] {
            mapper
                .map_into(event(kind, buttons, 5, 6), 7, (80, 25), &mut out)
                .unwrap();
        }
        assert_eq!(
            out.iter().map(|e| e.kind).collect::<Vec<_>>(),
            vec![
                MouseEventKind::Down(MouseButton::Left),
                MouseEventKind::Down(MouseButton::Right),
                MouseEventKind::Drag(MouseButton::Left),
                MouseEventKind::Up(MouseButton::Left),
                MouseEventKind::Up(MouseButton::Right)
            ]
        );
        assert!(out
            .iter()
            .all(|e| e.column == 4 && e.row == 5 && e.modifiers == KeyModifiers::CONTROL));
        assert_eq!(mapper.pressed, 0);
        // Missing-button packets never invent a left click.
        mapper
            .map_into(event(GPM_DOWN, 0, 3, 3), 7, (80, 25), &mut out)
            .unwrap();
        assert_eq!(out.len(), 5);
    }
    #[test]
    fn movement_coalesces_only_adjacent_movement_and_keeps_click_order() {
        let mut mapper = Mapper::default();
        let mut out = VecDeque::new();
        for (kind, button, x) in [
            (GPM_MOVE, 0, 1),
            (GPM_MOVE, 0, 2),
            (GPM_DOWN, 4, 2),
            (GPM_DRAG, 4, 3),
            (GPM_UP, 4, 3),
            (GPM_MOVE, 0, 4),
            (GPM_MOVE, 0, 5),
        ] {
            mapper
                .map_into(event(kind, button, x, 1), 7, (80, 25), &mut out)
                .unwrap();
        }
        assert_eq!(out.len(), 5);
        assert_eq!(out.front().unwrap().column, 1);
        assert_eq!(out.back().unwrap().column, 4);
    }
    #[test]
    fn drag_bursts_keep_latest_position_before_release() {
        let mut mapper = Mapper::default();
        let mut out = VecDeque::new();
        mapper
            .map_into(event(GPM_DOWN, 4, 1, 1), 7, (1024, 25), &mut out)
            .unwrap();
        for x in 2..=513 {
            mapper
                .map_into(event(GPM_DRAG, 4, x, 1), 7, (1024, 25), &mut out)
                .unwrap();
        }
        mapper
            .map_into(event(GPM_UP, 4, 513, 1), 7, (1024, 25), &mut out)
            .unwrap();
        assert_eq!(
            out.len(),
            3,
            "obsolete drag positions must not queue for painting"
        );
        assert_eq!(out[0].kind, MouseEventKind::Down(MouseButton::Left));
        assert_eq!(out[1].kind, MouseEventKind::Drag(MouseButton::Left));
        assert_eq!(out[1].column, 512);
        assert_eq!(out[2].kind, MouseEventKind::Up(MouseButton::Left));
        assert_eq!(mapper.pressed, 0);
    }

    #[test]
    fn movement_and_drag_coalescing_preserves_modifiers_wheels_and_button_edges() {
        let mut mapper = Mapper::default();
        let mut out = VecDeque::new();
        for kind in [GPM_MOVE, GPM_DRAG] {
            if kind == GPM_DRAG {
                mapper
                    .map_into(event(GPM_DOWN, 4, 1, 1), 7, (80, 25), &mut out)
                    .unwrap();
            }
            for (x, modifiers) in [
                (2, 0),
                (3, GPM_MOD_SHIFT),
                (4, GPM_MOD_SHIFT),
                (5, GPM_MOD_CONTROL),
                (6, GPM_MOD_ALT),
            ] {
                let mut packet = event(kind, if kind == GPM_DRAG { 4 } else { 0 }, x, 1);
                packet.modifiers = modifiers;
                mapper.map_into(packet, 7, (80, 25), &mut out).unwrap();
            }
            let start = usize::from(kind == GPM_DRAG);
            assert_eq!(out.len(), start + 4);
            assert_eq!(out[start].modifiers, KeyModifiers::NONE);
            assert_eq!(out[start + 1].column, 3);
            assert_eq!(out[start + 1].modifiers, KeyModifiers::SHIFT);
            assert_eq!(out[start + 2].modifiers, KeyModifiers::CONTROL);
            assert_eq!(out[start + 3].modifiers, KeyModifiers::ALT);
            out.clear();
        }
        for (kind, button, wheel) in [
            (GPM_DRAG, 4, 0),
            (GPM_DRAG, 4, 1),
            (GPM_DRAG, 4, 0),
            (GPM_DOWN, 1, 0),
            (GPM_DRAG, 5, 0),
            (GPM_UP, 5, 0),
        ] {
            let mut packet = event(kind, button, 7, 1);
            packet.wheel_y = wheel;
            mapper.map_into(packet, 7, (80, 25), &mut out).unwrap();
        }
        let kinds: Vec<_> = out.iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            [
                MouseEventKind::Drag(MouseButton::Left),
                MouseEventKind::ScrollUp,
                MouseEventKind::Drag(MouseButton::Left),
                MouseEventKind::Down(MouseButton::Right),
                MouseEventKind::Drag(MouseButton::Left),
                MouseEventKind::Drag(MouseButton::Right),
                MouseEventKind::Up(MouseButton::Left),
                MouseEventKind::Up(MouseButton::Right)
            ]
        );
    }
    #[test]
    fn wheel_expansion_is_bounded_and_legacy_release_is_not_a_second_scroll() {
        let mut mapper = Mapper::default();
        let mut out = VecDeque::new();
        let mut e = event(GPM_MOVE, 0, 1, 1);
        e.wheel_y = i16::MIN;
        mapper.map_into(e, 7, (80, 25), &mut out).unwrap();
        assert_eq!(out.len(), 16);
        assert!(out.iter().all(|e| e.kind == MouseEventKind::ScrollDown));
        out.clear();
        mapper
            .map_into(event(GPM_DOWN, 16, 1, 1), 7, (80, 25), &mut out)
            .unwrap();
        mapper
            .map_into(event(GPM_UP, 16, 1, 1), 7, (80, 25), &mut out)
            .unwrap();
        assert_eq!(out.len(), 1);
        out.clear();
        let mut e = event(GPM_DOWN, 4, 1, 1);
        e.wheel_y = 1;
        mapper.map_into(e, 7, (80, 25), &mut out).unwrap();
        mapper
            .map_into(event(GPM_UP, 8, 1, 1), 7, (80, 25), &mut out)
            .unwrap();
        assert_eq!(
            mapper.pressed, 4,
            "an unsupported button cannot release Left"
        );
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].kind, MouseEventKind::Down(MouseButton::Left));
        assert_eq!(out[1].kind, MouseEventKind::ScrollUp);
    }
    #[test]
    fn invalid_coordinates_foreign_vt_and_packets_never_click_a_clamped_cell() {
        let mut mapper = Mapper::default();
        let mut out = VecDeque::new();
        for (x, y) in [(0, 1), (-1, 1), (81, 1), (1, 26)] {
            mapper
                .map_into(event(GPM_DOWN, 4, x, y), 7, (80, 25), &mut out)
                .unwrap();
        }
        let mut other = event(GPM_DOWN, 4, 1, 1);
        other.virtual_console = 8;
        mapper.map_into(other, 7, (80, 25), &mut out).unwrap();
        assert!(out.is_empty());
        assert!(mapper
            .map_into(event(3, 4, 1, 1), 7, (80, 25), &mut out)
            .is_err());
        mapper
            .map_into(event(GPM_DOWN, 4, 1, 1), 7, (80, 25), &mut out)
            .unwrap();
        mapper
            .map_into(event(GPM_UP, 4, -3, 100), 7, (80, 25), &mut out)
            .unwrap();
        assert_eq!(
            out.back().unwrap().kind,
            MouseEventKind::Up(MouseButton::Left)
        );
        assert_eq!(
            (out.back().unwrap().column, out.back().unwrap().row),
            (80, 25)
        );
    }
    #[test]
    fn modifiers_and_runtime_status_preserve_keyboard_semantics() {
        assert_eq!(
            map_modifiers(0xff),
            KeyModifiers::SHIFT | KeyModifiers::CONTROL | KeyModifiers::ALT
        );
        assert_eq!(map_modifiers(0), KeyModifiers::NONE);
        let mut s = Snapshot::default();
        assert_eq!(s.source(), "Terminal");
        s.console = true;
        assert_eq!(s.source(), "Keyboard only");
        s.connected = true;
        assert_eq!(s.source(), "GPM");
        assert_eq!((s.pressed, s.reset), (0, 0));
    }
    #[test]
    fn terminal_diagnostics_report_detection_and_output_independently_of_gpm() {
        let mut state = Snapshot::default();
        let value = |state: &Snapshot, name: &str| {
            state
                .fields()
                .into_iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value)
                .expect("diagnostic field")
        };
        assert_eq!(
            value(&state, "Terminal mode"),
            "No virtual console detected"
        );
        assert_eq!(
            value(&state, "Color output"),
            "Extended colors (indexed / RGB)"
        );
        state.console = true;
        for connected in [false, true, false] {
            state.connected = connected;
            assert_eq!(value(&state, "Terminal mode"), "Linux virtual console");
            assert_eq!(
                value(&state, "Color output"),
                "Console colors (16 foreground / 8 background)"
            );
        }
        state.reason = "GPM unavailable: Permission denied".into();
        assert_eq!(value(&state, "GPM status"), state.reason);
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn disconnect_cancels_mouse_geometry_and_scrollbar_but_preserves_keyboard_geometry() {
        use crate::{presentation::layout::Rect, windowing::geometry::SurfaceKey};
        let mut app = crate::app::App::new(std::env::current_dir().unwrap()).unwrap();
        // Opening an editor is intentionally blocked during the initial panel
        // scan. Wait for that state transition before testing GPM capture: the
        // manager surface cannot move and is not the intended test document.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while app.panels.iter().any(|panel| panel.loading) {
            app.poll();
            assert!(
                std::time::Instant::now() < deadline,
                "initial panel scan did not finish"
            );
            std::thread::yield_now();
        }
        crate::editor::open(&mut app, true);
        assert_eq!(
            app.desktop.active().kind,
            crate::desktop::WindowKind::Editor
        );
        let owner = app.desktop.active().id;
        crate::layout::prepare(&mut app, Rect::new(0, 0, 80, 25));
        let native = app.desktop.native();
        let surface = native.surface(SurfaceKey::Document(owner)).unwrap();
        assert!(native.begin_move(
            surface.id,
            Some(mouse_event(
                MouseEventKind::Down(MouseButton::Left),
                20,
                1,
                KeyModifiers::NONE
            ))
        ));
        app.mouse.scrollbar_drag = Some((0, Rect::new(2, 2, 1, 10)));
        let mut snapshot = Snapshot {
            console: true,
            reset: 1,
            ..Default::default()
        };
        assert!(sync(&mut app, snapshot.clone()));
        assert!(!native.manipulating());
        assert!(app.mouse.scrollbar_drag.is_none());
        assert_eq!(app.desktop.active().id, owner);
        assert!(
            !sync(&mut app, snapshot.clone()),
            "idle probes must not dirty the frame"
        );
        assert!(native.begin_move(surface.id, None));
        snapshot.reset += 1;
        assert!(sync(&mut app, snapshot));
        assert!(
            native.manipulating(),
            "GPM loss must not cancel keyboard movement"
        );
    }
    #[test]
    fn pointer_covers_wide_graphemes_preserves_hits_and_cursor_and_never_mutates_source() {
        use crate::{
            palette::{theme, PaletteProfile},
            turbo::scene::Scene,
        };
        for profile in [
            PaletteProfile::ClassicDn,
            PaletteProfile::Custom(theme::DEFAULTS.map(|v| v ^ 1)),
        ] {
            let mut source = Scene {
                width: 80,
                height: 25,
                profile,
                cells: Default::default(),
                regions: Vec::new(),
                cursor: Some((3, 2)),
            };
            source.text(0, 0, 80, "a界e\u{301}", 0x17);
            source.normalize_wide();
            let original = source.cells.get(&(1, 0)).unwrap().resolved(profile);
            for _ in 0..4 {
                let mut shown = source.clone();
                pointer(&mut shown, Some((2, 0)));
                assert_eq!(shown.cursor, source.cursor);
                assert_eq!(shown.regions, source.regions);
                assert_eq!(
                    source.cells.get(&(1, 0)).unwrap().resolved(profile),
                    original
                );
                assert_eq!(shown.cells.get(&(1, 0)).unwrap().text, "界");
                assert!(shown.cells.get(&(2, 0)).unwrap().text.is_empty());
                assert_eq!(
                    shown.cells.get(&(1, 0)).unwrap().style,
                    shown.cells.get(&(2, 0)).unwrap().style
                );
                assert_ne!(
                    shown.cells.get(&(1, 0)).unwrap().resolved(profile),
                    original
                );
                #[cfg(unix)]
                {
                    let restore =
                        String::from_utf8(pointer_restore(&source, Some((2, 0))).unwrap()).unwrap();
                    assert!(restore.starts_with("\x1b7\x1b[1;2H"));
                    assert!(restore.ends_with("界\x1b8"));
                    assert!(
                        !restore.contains("\x1b[7m"),
                        "restore uses the original attributes"
                    );
                    assert!(pointer_restore(&source, None).unwrap().is_empty());
                }
            }
            let mut hidden = source.clone();
            pointer(&mut hidden, None);
            pointer(&mut hidden, Some((80, 25)));
            assert_eq!(
                hidden.cells.get(&(1, 0)).unwrap().resolved(profile),
                original
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn pointer_restores_console_gray_with_native_background_codes() {
        let mut scene = crate::turbo::scene::Scene {
            width: 80,
            height: 25,
            profile: crate::palette::PaletteProfile::ClassicDn,
            cells: Default::default(),
            regions: Vec::new(),
            cursor: None,
        };
        scene.text(0, 0, 80, "界", 0x87);
        scene.normalize_wide();
        let patch = String::from_utf8(pointer_restore(&scene, Some((1, 0))).unwrap()).unwrap();
        assert!(patch.contains("\x1b[47m"), "gray background: {patch:?}");
        assert!(!patch.contains(";5;"));
        assert!(patch.contains("\x1b[97m"));
        assert!(patch.ends_with("界\x1b8"));
    }
}
