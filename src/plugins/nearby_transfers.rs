//! Presentation only: the host captures paths, owns networking and publishes files.
use super::*;
use crate::{
    painting::{Paragraph, Wrap},
    turbo::dialog_spec::{Control, ControlKind},
};
use crossterm::event::{KeyCode as K, MouseButton, MouseEventKind};
mod text;
#[derive(Default)]
pub struct NearbyTransfers {
    page: u8,
    focus: usize,
    cursor: usize,
    selected_peer: Option<iroh::EndpointId>,
    offer_cursor: usize,
    manual: String,
    manual_cursor: usize,
    manual_target: bool,
    nickname: String,
    nickname_cursor: usize,
    name_observed: String,
    destination: String,
    destination_cursor: usize,
    picked: Option<PathBuf>,
    text: text::TextUi,
}
impl NearbyTransfers {
    fn action(&mut self, id: usize, ctx: &Context) -> Vec<Effect> {
        use crate::peer::Action as A;
        let action = match id {
            text::OPEN => {
                if !ctx.nearby.busy && self.test_address(ctx).is_some() {
                    self.text.open();
                    self.text.address = self.test_address(ctx).expect("checked receiver");
                    self.focus = text::BODY;
                    self.page = 3;
                }
                return vec![];
            }
            text::PASTE => return vec![Effect::PasteText(self.text.token())],
            text::CLEAR => {
                self.text.draft.move_to(0, false);
                self.text.draft.move_to(self.text.draft.text.len(), true);
                self.text.insert("");
                self.focus = text::BODY;
                return vec![];
            }
            text::CANCEL => {
                self.text.composing = false;
                self.page = 0;
                self.focus = text::OPEN;
                return vec![];
            }
            text::SEND => {
                let address = self.text.address.clone();
                if ctx.nearby.busy || self.text.draft.text.is_empty() {
                    return vec![];
                }
                self.text.composing = false;
                self.page = 0;
                self.focus = text::OPEN;
                A::SendText {
                    address,
                    text: self.text.draft.text.clone(),
                }
            }
            text::ACCEPT => {
                self.page = 0;
                self.focus = 1;
                A::TextAccept(true)
            }
            text::DECLINE => {
                self.page = 0;
                self.focus = 1;
                A::TextAccept(false)
            }
            text::COPY => {
                let Some(received) = &ctx.nearby.received_text else {
                    return vec![];
                };
                return vec![Effect::CopyTextOwned {
                    text: received.text.clone(),
                    serial: received.serial,
                }];
            }
            text::DISMISS => {
                self.page = 0;
                self.focus = 1;
                self.text.notice.clear();
                A::DismissText
            }

            1 => A::Receive(!ctx.nearby.receiving),
            13 => A::Refresh,
            14 => {
                let Some(address) = self.test_address(ctx) else {
                    return vec![];
                };
                A::Test(address)
            }
            23 => A::Nickname(self.nickname.clone()),
            16 => {
                let mut text = format!(
                    "{} {}\n{} {}\n\n{}\n",
                    ctx.language.text("This DN:"),
                    ctx.nearby.local_name,
                    ctx.language.text("Endpoint identity:"),
                    ctx.nearby.local_id,
                    ctx.language.text("Local interfaces and addresses:")
                );
                if ctx.nearby.network.local_addresses.is_empty() {
                    text.push_str(ctx.language.text(
                        "Start Receive, Refresh or Test connection to initialize networking.",
                    ));
                } else {
                    text.push_str(&ctx.nearby.network.local_addresses.join("\n"));
                }
                text.push_str("\n\n");
                text.push_str(ctx.language.text("Edit the name field and press Enter to apply. A blank name restores the anonymous default. Nicknames are session-only and do not prove identity."));
                return vec![Effect::Help {
                    title: " Nearby device ",
                    text,
                }];
            }
            2 => A::Capture,
            3 => {
                let Some(peer) = self.peer(ctx) else {
                    return vec![];
                };
                A::Send(crate::peer::lan::card(&peer.address))
            }
            4 => A::Send(self.manual.clone()),
            5 => A::Confirm(true),
            6 => A::Confirm(false),
            7 => A::Accept(
                self.picked
                    .clone()
                    .unwrap_or_else(|| PathBuf::from(&self.destination)),
            ),
            8 => A::Decline,
            9 => A::Tree,
            10 => return vec![Effect::Close],
            11 => {
                return vec![Effect::Help {
                    title: " Nearby Transfers Help ",
                    text: crate::peer::HELP.into(),
                }]
            }
            12 => return vec![Effect::CopyText(ctx.nearby.details.clone())],
            _ => return vec![],
        };
        vec![Effect::Nearby(action)]
    }
    fn test_address(&self, ctx: &Context) -> Option<String> {
        if !self.manual.trim().is_empty() && (self.manual_target || self.peer(ctx).is_none()) {
            Some(self.manual.clone())
        } else {
            self.peer(ctx)
                .map(|peer| crate::peer::lan::card(&peer.address))
        }
    }
    fn peer<'a>(&self, ctx: &'a Context) -> Option<&'a crate::peer::service::Peer> {
        self.selected_peer
            .and_then(|id| ctx.nearby.peers.iter().find(|p| p.address.id == id))
    }
    fn select(&mut self, ctx: &Context) {
        self.selected_peer = ctx.nearby.peers.get(self.cursor).map(|p| p.address.id);
    }
    fn paste(&mut self, text: &str) {
        match self.focus {
            20 => {
                self.manual_target = true;
                crate::dialog::paste_input(&mut self.manual, &mut self.manual_cursor, text);
            }
            23 => crate::dialog::paste_input(&mut self.nickname, &mut self.nickname_cursor, text),
            21 => {
                crate::dialog::paste_input(
                    &mut self.destination,
                    &mut self.destination_cursor,
                    text,
                );
                self.picked = None;
            }
            _ => {}
        }
    }
}
impl ToolPlugin for NearbyTransfers {
    fn definition(&self) -> Definition {
        Definition::builtin(NEARBY_TRANSFERS, "Nearby transfers", Presentation::Document)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn open(&mut self, _: &Context) {
        self.focus = 1;
        self.cursor = 0;
        self.selected_peer = None;
        self.name_observed.clear();
        self.text = text::TextUi::default();
    }
    fn prepare(&mut self, ctx: &Context) {
        if self.name_observed != ctx.nearby.local_name {
            self.nickname = ctx.nearby.local_name.clone();
            self.nickname_cursor = self.nickname.len();
            self.name_observed = ctx.nearby.local_name.clone();
        }
        let page = if ctx.nearby.pairing.is_some() {
            1
        } else if !ctx.nearby.offer.is_empty() {
            2
        } else if ctx.nearby.text_offer.is_some() {
            4
        } else if ctx.nearby.received_text.is_some() {
            5
        } else if self.text.composing {
            3
        } else {
            0
        };
        self.text.prepare(ctx, page);
        if self.page != page {
            self.page = page;
            self.focus = match page {
                1 => 6,
                2 => 21,
                3 | 5 => text::BODY,
                4 => text::DECLINE,
                _ => 1,
            };
        }
        if let Some(index) = self
            .selected_peer
            .and_then(|id| ctx.nearby.peers.iter().position(|p| p.address.id == id))
        {
            self.cursor = index;
        }
        self.cursor = self.cursor.min(ctx.nearby.peers.len().saturating_sub(1));
        if self.selected_peer.is_none() {
            self.select(ctx);
        }
        self.offer_cursor = self
            .offer_cursor
            .min(ctx.nearby.offer.len().saturating_sub(1));
    }
    fn clipboard_token(&self) -> Option<ClipboardToken> {
        (self.page == 3 && self.text.composing).then(|| self.text.token())
    }
    fn native_input_page(&self) -> Option<u64> {
        (self.page >= 3).then_some(1)
    }
    fn close(&mut self) {
        self.text = text::TextUi::default();
    }
    fn uses_controls(&self, key: KeyEvent) -> bool {
        if matches!(self.page, 3 | 5)
            && self.focus == text::BODY
            && !matches!(key.code, K::Tab | K::BackTab | K::Esc)
        {
            return false;
        }
        !(matches!(self.focus, 20 | 21 | 23) && key.code == K::Char(' ')
            || self.focus == 0 && key.code == K::Enter
            || self.focus == 23 && key.code == K::Enter)
    }
    fn event(&mut self, event: Event<'_>, ctx: &Context) -> Vec<Effect> {
        match event {
            Event::ClipboardCopied { serial, message } => {
                if ctx
                    .nearby
                    .received_text
                    .as_ref()
                    .is_some_and(|r| r.serial == serial)
                {
                    self.text.notice = message;
                }
            }
            Event::ClipboardPasteNotice { token, message } => {
                if self.clipboard_token() == Some(token) {
                    self.text.notice = message;
                }
            }
            Event::Key(key) if matches!(self.page, 3 | 5) && self.focus == text::BODY => {
                return self.text.key(key, self.page)
            }
            Event::Mouse(event, Some(ActionId(text::BODY))) if matches!(self.page, 3 | 5) => {
                self.focus = text::BODY;
                self.text.mouse(event, ctx, self.page);
            }
            Event::NearbyDirectory(path) => {
                self.destination = crate::filesystem::display_name(&path.to_string_lossy());
                self.destination_cursor = self.destination.len();
                self.picked = Some(path);
            }
            Event::Focus(focus) => {
                self.focus = focus;
                if focus == 20 {
                    self.manual_target = true;
                } else if focus == 0 {
                    self.manual_target = false;
                }
            }
            Event::Action(id) => return self.action(id.0, ctx),
            Event::Mouse(event, Some(id))
                if event.kind == MouseEventKind::Down(MouseButton::Left) =>
            {
                if id.0 >= 500 {
                    self.offer_cursor = (id.0 - 500).min(ctx.nearby.offer.len().saturating_sub(1));
                    self.focus = 22;
                } else if id.0 >= 100 {
                    self.cursor = (id.0 - 100).min(ctx.nearby.peers.len().saturating_sub(1));
                    self.focus = 0;
                    self.select(ctx);
                    self.manual_target = false;
                } else {
                    self.focus = id.0;
                    if id.0 == 20 {
                        self.manual_target = true;
                    }
                    if id.0 != 23 {
                        return self.action(id.0, ctx);
                    }
                }
            }
            Event::Key(key) if key.code == K::F(1) => return self.action(11, ctx),
            Event::Key(key) if self.focus == 23 => {
                if key.code == K::Enter {
                    return self.action(23, ctx);
                }
                crate::dialog::edit_input(&mut self.nickname, &mut self.nickname_cursor, key.code);
            }
            Event::Key(key) if matches!(self.focus, 20 | 21) => {
                if self.focus == 20 {
                    self.manual_target = true;
                    crate::dialog::edit_input(&mut self.manual, &mut self.manual_cursor, key.code);
                } else {
                    let before = self.destination.clone();
                    crate::dialog::edit_input(
                        &mut self.destination,
                        &mut self.destination_cursor,
                        key.code,
                    );
                    if before != self.destination {
                        self.picked = None;
                    }
                }
            }
            Event::Key(key) if self.focus == 22 => match key.code {
                K::Up => self.offer_cursor = self.offer_cursor.saturating_sub(1),
                K::Down => {
                    self.offer_cursor =
                        (self.offer_cursor + 1).min(ctx.nearby.offer.len().saturating_sub(1))
                }
                K::Home => self.offer_cursor = 0,
                K::End => self.offer_cursor = ctx.nearby.offer.len().saturating_sub(1),
                K::PageDown => {
                    self.offer_cursor =
                        (self.offer_cursor + 7).min(ctx.nearby.offer.len().saturating_sub(1))
                }
                K::PageUp => self.offer_cursor = self.offer_cursor.saturating_sub(7),
                _ => {}
            },
            Event::Key(key) if self.focus == 0 => {
                match key.code {
                    K::Enter
                        if ctx.nearby.selected > 0
                            && !ctx.nearby.busy
                            && ctx.nearby.network.ready =>
                    {
                        return self.action(3, ctx)
                    }
                    K::Up => self.cursor = self.cursor.saturating_sub(1),
                    K::Down => {
                        self.cursor =
                            (self.cursor + 1).min(ctx.nearby.peers.len().saturating_sub(1))
                    }
                    K::Home => self.cursor = 0,
                    K::End => self.cursor = ctx.nearby.peers.len().saturating_sub(1),
                    _ => {}
                }
                if matches!(key.code, K::Up | K::Down | K::Home | K::End) {
                    self.select(ctx);
                    self.manual_target = false;
                }
            }
            Event::Paste(text) if self.page == 3 => self.text.insert(text),
            Event::Paste(text) => self.paste(text),
            _ => {}
        }
        vec![]
    }
    fn controls(&self, ctx: &Context) -> DialogSpec<'_> {
        if self.page >= 3 {
            self.text.controls(ctx, self.page, self.focus)
        } else {
            self.main_controls(ctx)
        }
    }
    fn view(&self, ctx: &Context) -> View<'_> {
        let mut spec = self.main_controls(ctx);
        if self.page >= 3 {
            spec.overlay(self.text.controls(ctx, self.page, self.focus));
        }
        View::Dialog(Box::new(spec))
    }
}
impl NearbyTransfers {
    fn main_controls(&self, ctx: &Context) -> DialogSpec<'_> {
        let r = Rect::new(
            ctx.area.x,
            ctx.area.y,
            ctx.area.width.min(76),
            ctx.area.height.min(21),
        );
        let pairing = ctx.nearby.pairing.is_some();
        let offering = !ctx.nearby.offer.is_empty();
        let focus = if pairing && !matches!(self.focus, 5 | 6 | 10) {
            6
        } else if offering && !matches!(self.focus, 7 | 8 | 9 | 21 | 22 | 10) {
            21
        } else {
            self.focus
        };
        let mut spec = ctx.dialog(r, focus, " Nearby transfers ", 10, 10);
        let text = |spec: &mut DialogSpec<'_>, value: String, y, h| {
            spec.caption(
                Paragraph::new(value).wrap(Wrap { trim: false }),
                Rect::new(r.x + 2, r.y + y, r.width - 4, h),
            )
        };
        let button = |spec: &mut DialogSpec<'_>, id, label: &str, x, y, width, enabled| {
            let mut control = Control::new(
                id,
                Rect::new(r.x + x, r.y + y, width, 1),
                ControlKind::Button(ctx.language.text(label).into()),
                ctx.target(id),
            );
            control.enabled = enabled;
            spec.control(control);
        };
        let input = |spec: &mut DialogSpec<'_>, id, value: &str, cursor: usize, bounds: Rect| {
            let column =
                crate::presentation::text::Line::from(&value[..cursor.min(value.len())]).width();
            let mut control = Control::new(
                id,
                bounds,
                ControlKind::ScrolledInput {
                    value: value.to_owned(),
                    scroll: column.saturating_sub(usize::from(bounds.width - 1)),
                },
                ctx.target(id),
            );
            control.enabled = id != 23 || !ctx.nearby.busy;
            spec.control(control);
            if focus == id {
                spec.cursor = Some((
                    bounds.x + column.min(usize::from(bounds.width - 1)) as u16,
                    bounds.y,
                ));
            }
        };
        let field = |spec: &mut DialogSpec<'_>, id, value: &str, cursor: usize, y| {
            input(
                spec,
                id,
                value,
                cursor,
                Rect::new(r.x + 3, r.y + y, r.width - 19, 1),
            )
        };
        if let Some((code, peer)) = &ctx.nearby.pairing {
            text(&mut spec, crate::filesystem::display_name(peer), 2, 1);
            text(
                &mut spec,
                ctx.language
                    .text("Compare all eight digits on both screens.")
                    .into(),
                4,
                2,
            );
            if ctx.nearby.test_pairing {
                text(
                    &mut spec,
                    ctx.language
                        .text("Connection test — no files will be sent.")
                        .into(),
                    3,
                    1,
                );
            }
            text(&mut spec, code.clone(), 7, 1);
            text(
                &mut spec,
                ctx.language
                    .text("Confirm only if the verification codes match.")
                    .into(),
                10,
                2,
            );
            button(&mut spec, 5, "Codes match", 8, 15, 25, true);
            button(&mut spec, 6, "Reject", 43, 15, 20, true);
        } else if offering {
            spec.default = ctx.target(7);
            text(
                &mut spec,
                format!(
                    "{}: {} · {} bytes",
                    ctx.language.text("Items"),
                    ctx.nearby.count,
                    ctx.nearby.bytes
                ),
                1,
                1,
            );
            let offset = self.offer_cursor.saturating_sub(6);
            let mut list = Control::new(
                22,
                Rect::new(r.x + 2, r.y + 3, r.width - 4, 7),
                ControlKind::List {
                    items: ctx.nearby.offer.clone(),
                    cursor: self.offer_cursor,
                    offset,
                },
                ctx.target(22),
            );
            for row in 0..7 {
                if offset + row < ctx.nearby.offer.len() {
                    list.hits.push((
                        Rect::new(r.x + 2, r.y + 3 + row as u16, r.width - 4, 1),
                        ctx.target(500 + offset + row),
                    ));
                }
            }
            spec.control(list);
            if ctx.nearby.count > ctx.nearby.offer.len() {
                text(
                    &mut spec,
                    ctx.language
                        .text("Preview limited to the first 100 entries.")
                        .into(),
                    10,
                    1,
                );
            }
            text(
                &mut spec,
                ctx.language.text("Local receive directory").into(),
                11,
                1,
            );
            field(
                &mut spec,
                21,
                &self.destination,
                self.destination_cursor,
                12,
            );
            button(&mut spec, 9, "Tree…", r.width - 14, 12, 10, true);
            button(
                &mut spec,
                7,
                "Accept",
                8,
                15,
                22,
                !self.destination.is_empty(),
            );
            button(&mut spec, 8, "Decline", 42, 15, 22, true);
        } else {
            text(
                &mut spec,
                format!(
                    "{} · {}",
                    ctx.language.text(if ctx.nearby.network.receiving {
                        "Receive: on"
                    } else {
                        "Receive: off"
                    }),
                    ctx.language.text(ctx.nearby.network.phase.caption())
                ),
                14,
                1,
            );
            spec.caption(
                Paragraph::new(ctx.language.text("This DN:")),
                Rect::new(r.x + 2, r.y + 1, 9, 1),
            );
            input(
                &mut spec,
                23,
                &self.nickname,
                self.nickname_cursor,
                Rect::new(r.x + 11, r.y + 1, 24, 1),
            );
            let short_id = ctx.nearby.local_id.chars().take(8).collect::<String>();
            spec.caption(
                Paragraph::new(format!("ID {short_id}")),
                Rect::new(r.x + 37, r.y + 1, 14, 1),
            );
            spec.caption(
                Paragraph::new(ctx.language.text("Enter applies name")),
                Rect::new(r.x + 53, r.y + 1, 21, 1),
            );
            let counts = format!(
                "{} {} / {} / {}",
                ctx.language.text("Peers seen/usable/filtered:"),
                ctx.nearby.network.seen,
                ctx.nearby.peers.len(),
                ctx.nearby.network.filtered
            );
            spec.caption(
                Paragraph::new(format!(
                    "{}: {}",
                    ctx.language.text("Selected items"),
                    ctx.nearby.selected
                )),
                Rect::new(r.x + 2, r.y + 10, 27, 1),
            );
            spec.caption(Paragraph::new(counts), Rect::new(r.x + 30, r.y + 10, 44, 1));
            let items = ctx
                .nearby
                .peers
                .iter()
                .map(|p| {
                    format!(
                        "{} {}",
                        p.name,
                        p.address
                            .ip_addrs()
                            .next()
                            .map(ToString::to_string)
                            .unwrap_or_default()
                    )
                })
                .collect::<Vec<_>>();
            if self.focus == 20 {
                spec.default = ctx.target(4);
            }
            let mut list = Control::new(
                0,
                Rect::new(r.x + 2, r.y + 2, 42, 8),
                ControlKind::List {
                    items,
                    cursor: self.cursor.min(ctx.nearby.peers.len().saturating_sub(1)),
                    offset: self.cursor.saturating_sub(7),
                },
                ctx.target(0),
            );
            for i in 0..8 {
                if let Some(index) = self
                    .cursor
                    .saturating_sub(7)
                    .checked_add(i)
                    .filter(|i| *i < ctx.nearby.peers.len())
                {
                    list.hits.push((
                        Rect::new(r.x + 2, r.y + 2 + i as u16, 42, 1),
                        ctx.target(100 + index),
                    ));
                }
            }
            spec.control(list);
            button(
                &mut spec,
                1,
                if ctx.nearby.receiving {
                    "Stop receiving"
                } else {
                    "Receive"
                },
                46,
                2,
                26,
                true,
            );
            button(
                &mut spec,
                2,
                "Use panel selection",
                46,
                4,
                26,
                !ctx.nearby.busy,
            );
            button(
                &mut spec,
                3,
                "Send selected…",
                46,
                6,
                26,
                ctx.nearby.selected > 0
                    && self.peer(ctx).is_some()
                    && !ctx.nearby.busy
                    && ctx.nearby.network.ready
                    && !matches!(
                        ctx.nearby.network.phase,
                        crate::peer::discovery::Phase::Pending
                            | crate::peer::discovery::Phase::Starting
                    ),
            );
            button(
                &mut spec,
                12,
                "Copy connection details",
                46,
                8,
                26,
                !ctx.nearby.details.is_empty(),
            );
            text(
                &mut spec,
                ctx.nearby
                    .network
                    .local_addresses
                    .first()
                    .cloned()
                    .unwrap_or_else(|| ctx.language.text("Networking not started").into()),
                11,
                1,
            );
            text(
                &mut spec,
                ctx.language.text("Manual connection details").into(),
                12,
                1,
            );
            field(&mut spec, 20, &self.manual, self.manual_cursor, 13);
            button(
                &mut spec,
                4,
                "Connect",
                r.width - 14,
                13,
                11,
                ctx.nearby.selected > 0
                    && !self.manual.is_empty()
                    && !ctx.nearby.busy
                    && ctx.nearby.network.ready
                    && !matches!(
                        ctx.nearby.network.phase,
                        crate::peer::discovery::Phase::Pending
                            | crate::peer::discovery::Phase::Starting
                    ),
            );
            button(
                &mut spec,
                14,
                "Test connection",
                2,
                15,
                24,
                !ctx.nearby.busy && self.test_address(ctx).is_some(),
            );
            button(
                &mut spec,
                text::OPEN,
                "Send text…",
                28,
                15,
                24,
                !ctx.nearby.busy && self.test_address(ctx).is_some(),
            );
            button(&mut spec, 13, "Refresh", 54, 15, 20, true);
        }
        let notice = crate::filesystem::display_name(&ctx.language.field(
            &ctx.nearby.notice,
            crate::regional::DateTimeFormat::default(),
        ));
        let notice = if (pairing || offering)
            && ctx.nearby.network.phase == crate::peer::discovery::Phase::Pending
        {
            format!(
                "{}\n{}",
                ctx.language.text("Network refresh pending"),
                notice
            )
        } else {
            notice
        };
        text(&mut spec, notice, 17, 2);
        button(&mut spec, 11, "Help", 5, r.height - 2, 14, true);
        button(&mut spec, 16, "Device info…", 23, r.height - 2, 23, true);
        button(&mut spec, 10, "Close", r.width - 22, r.height - 2, 16, true);
        spec
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn context() -> Context {
        Context {
            binding: Binding {
                plugin: NEARBY_TRANSFERS,
                owner: Owner::Document(crate::desktop::WindowId::MANAGER),
            },
            scope: crate::turbo::dialog_spec::ControlScope::Document {
                window: crate::desktop::WindowId::MANAGER,
                revision: 0,
            },
            area: Rect::new(0, 0, 80, 25),
            language: crate::localization::Language::English,
            palette: crate::palette::PaletteProfile::ClassicDn,
            active: true,
            now: Instant::now(),
            storage: None,
            mp3: Default::default(),
            nearby: Default::default(),
        }
    }
    #[test]
    fn dismissing_text_restores_parent_mouse_buttons() -> std::io::Result<()> {
        let root = std::env::temp_dir().join(format!("dn-text-close-{}", std::process::id()));
        std::fs::create_dir(&root)?;
        struct Temp(std::path::PathBuf);
        impl Drop for Temp {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _temp = Temp(root.clone());
        let mut app = crate::app::App::new(root)?;
        crate::plugins::host::open(&mut app, NEARBY_TRANSFERS);
        app.peer.view.text_offer = Some(("peer".into(), 4));
        app.peer.view.receiving = true;
        let mut clock = crate::testing::InputClock::default();
        for id in [text::ACCEPT, text::COPY, text::DISMISS, 10] {
            let mut terminal =
                crate::testing::Terminal::new(crate::testing::backend::TestBackend::new(80, 25))?;
            terminal.draw(|f| crate::ui::draw(f, &mut app))?;
            let target = crate::mouse::Target::Plugin(NEARBY_TRANSFERS, ActionId(id));
            let bounds = app
                .mouse
                .regions
                .iter()
                .rev()
                .find(|(_, t)| *t == target)
                .unwrap()
                .0;
            let event = crossterm::event::MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: bounds.x + bounds.width / 2,
                row: bounds.y,
                modifiers: crossterm::event::KeyModifiers::NONE,
            };
            clock.mouse(&mut app, event);
            clock.mouse(
                &mut app,
                crossterm::event::MouseEvent {
                    kind: MouseEventKind::Up(MouseButton::Left),
                    ..event
                },
            );
            clock.advance(&mut app, std::time::Duration::from_millis(100));
            if id == text::ACCEPT {
                app.peer.view.received_text = Some(crate::peer::text::Received {
                    serial: 1,
                    peer: "peer".into(),
                    text: "link".into(),
                });
            }
            if id == text::DISMISS {
                assert!(
                    !app.peer.view.busy,
                    "Dismiss must publish the released Busy state with its parent controls"
                );
            }
            app.poll();
        }
        assert_eq!(app.desktop.active().id, crate::desktop::WindowId::MANAGER);
        Ok(())
    }
    #[test]
    fn text_pages_are_owned_safe_localized_and_immutable_with_draft_and_copy_guards() {
        for language in [
            crate::localization::Language::English,
            crate::localization::Language::Russian,
        ] {
            for palette in [
                crate::palette::PaletteProfile::ClassicDn,
                crate::palette::PaletteProfile::Custom([0x1f; crate::palette::theme::COUNT]),
            ] {
                let mut c = context();
                c.language = language;
                c.palette = palette;
                let mut p = NearbyTransfers {
                    manual: "captured-peer".into(),
                    manual_target: true,
                    ..Default::default()
                };
                p.prepare(&c);
                p.action(text::OPEN, &c);
                p.prepare(&c);
                let token = p.clipboard_token().unwrap();
                p.event(Event::Paste("Ёжик\r\nhttps://example.test/"), &c);
                p.prepare(&c);
                assert_ne!(p.clipboard_token(), Some(token));
                assert!(!p.uses_controls(KeyEvent::new(
                    K::Enter,
                    crossterm::event::KeyModifiers::NONE
                )));
                p.event(
                    Event::Key(KeyEvent::new(
                        K::Enter,
                        crossterm::event::KeyModifiers::NONE,
                    )),
                    &c,
                );
                assert!(p.text.draft.text.ends_with('\n'));
                let draft = p.text.draft.text.clone();
                p.manual = "changed-peer".into();
                assert!(
                    matches!(&p.action(text::SEND,&c)[0],Effect::Nearby(crate::peer::Action::SendText{address,text}) if address=="captured-peer" && text==&draft)
                );
                assert_eq!(p.text.draft.text, draft);
                c.nearby.text_offer = Some(("peer".into(), 10));
                p.prepare(&c);
                assert_eq!(p.focus, text::DECLINE);
                assert!(matches!(
                    &p.action(text::DECLINE, &c)[0],
                    Effect::Nearby(crate::peer::Action::TextAccept(false))
                ));
                c.nearby.text_offer = None;
                c.nearby.received_text = Some(crate::peer::text::Received {
                    serial: 9,
                    peer: "peer".into(),
                    text: "Ёжик\r\nhttps://example.test/\x1b[31m".into(),
                });
                p.prepare(&c);
                assert_eq!(p.page, 5);
                assert!(
                    matches!(&p.action(text::COPY,&c)[0],Effect::CopyTextOwned{text,serial:9} if text.ends_with("\x1b[31m"))
                );
                p.event(
                    Event::ClipboardCopied {
                        serial: 8,
                        message: "stale".into(),
                    },
                    &c,
                );
                assert_ne!(p.text.notice, "stale");
                p.event(
                    Event::ClipboardCopied {
                        serial: 9,
                        message: "copied".into(),
                    },
                    &c,
                );
                assert_eq!(p.text.notice, "copied");
                for area in [
                    Rect::new(0, 0, 80, 25),
                    Rect::new(0, 0, 120, 40),
                    Rect::new(0, 0, 80, 25),
                ] {
                    c.area = area;
                    p.prepare(&c);
                    let expected = frame(&p, &c);
                    for _ in 0..3 {
                        assert_eq!(frame(&p, &c), expected);
                    }
                    let View::Dialog(spec) = p.view(&c) else {
                        panic!("Shared dialog required");
                    };
                    let windows = spec.windows(c.area);
                    assert_eq!(windows.len(), 2);
                    assert_eq!(windows[1].1, Some(0));
                }
            }
        }
    }
    #[test]
    fn test_connection_needs_no_files_and_nickname_is_an_explicit_draft() {
        let mut c = context();
        c.nearby.local_name = "DN anonymous".into();
        c.nearby.local_id = "public-id".into();
        let mut p = NearbyTransfers::default();
        p.prepare(&c);
        assert_eq!(p.nickname, "DN anonymous");
        p.manual = "manual-card".into();
        p.manual_target = true;
        assert!(
            matches!(&p.action(14,&c)[0],Effect::Nearby(crate::peer::Action::Test(value)) if value=="manual-card")
        );
        assert_eq!(c.nearby.selected, 0);
        p.focus = 23;
        p.nickname.clear();
        p.nickname_cursor = 0;
        p.event(Event::Paste("Лаптоп"), &c);
        assert_eq!(p.nickname, "Лаптоп");
        assert_eq!(c.nearby.local_name, "DN anonymous");
        p.prepare(&c);
        assert_eq!(
            p.nickname, "Лаптоп",
            "Painting must not overwrite an unapplied draft"
        );
        assert!(!p.uses_controls(KeyEvent::new(
            K::Enter,
            crossterm::event::KeyModifiers::NONE
        )));
        let effects = p.event(
            Event::Key(KeyEvent::new(
                K::Enter,
                crossterm::event::KeyModifiers::NONE,
            )),
            &c,
        );
        assert!(
            matches!(&effects[0],Effect::Nearby(crate::peer::Action::Nickname(name)) if name=="Лаптоп")
        );
        assert!(
            matches!(&p.action(16,&c)[0],Effect::Help{text,..} if text.contains("public-id") && text.contains("Local interfaces"))
        );
        p.open(&c);
        p.prepare(&c);
        assert_eq!(
            p.nickname, "DN anonymous",
            "Reopening discards the unapplied draft"
        );
    }
    fn frame(p: &NearbyTransfers, c: &Context) -> String {
        let mut f = crate::painting::NativeFrame::new(c.area, c.palette);
        p.controls(c).append(&mut f);
        format!(
            "{:?}",
            f.finish()
                .cells
                .iter()
                .map(|(pos, g)| (*pos, g.text.clone(), g.attribute))
                .collect::<Vec<_>>()
        )
    }
    #[test]
    fn nearby_pages_are_owned_localized_and_immutable_after_resize() {
        let mut c = context();
        let mut p = NearbyTransfers::default();
        p.open(&c);
        for language in [
            crate::localization::Language::English,
            crate::localization::Language::Russian,
        ] {
            c.language = language;
            for palette in [
                crate::palette::PaletteProfile::ClassicDn,
                crate::palette::PaletteProfile::Terminal,
            ] {
                c.palette = palette;
                for area in [
                    Rect::new(0, 0, 80, 25),
                    Rect::new(8, 4, 100, 32),
                    Rect::new(0, 0, 80, 25),
                ] {
                    c.area = area;
                    for page in 0..3 {
                        c.nearby.pairing =
                            (page == 1).then(|| ("1234 5678".into(), "DN abc12345".into()));
                        c.nearby.offer = if page == 2 {
                            (0..100).map(|n| format!("Папка Ёжик/{n}.txt")).collect()
                        } else {
                            vec![]
                        };
                        c.nearby.count = 200;
                        c.nearby.bytes = 3000;
                        c.nearby.network.phase = if page == 1 {
                            crate::peer::discovery::Phase::Pending
                        } else {
                            Default::default()
                        };
                        p.prepare(&c);
                        if page == 1 {
                            let mut native = crate::painting::NativeFrame::new(c.area, c.palette);
                            p.controls(&c).append(&mut native);
                            let row = native
                                .finish()
                                .cells
                                .iter()
                                .filter(|((_, y), _)| *y == c.area.y + 17)
                                .map(|(_, g)| g.text.as_str())
                                .collect::<String>();
                            assert!(
                                row.contains(c.language.text("Network refresh pending")),
                                "{row}"
                            );
                        }
                        if page == 0 {
                            let mut native = crate::painting::NativeFrame::new(c.area, c.palette);
                            p.controls(&c).append(&mut native);
                            let row = native
                                .finish()
                                .cells
                                .iter()
                                .filter(|((_, y), _)| *y == c.area.y + 15)
                                .map(|(_, g)| g.text.as_str())
                                .collect::<String>();
                            assert!(row.contains(c.language.text("Refresh")), "{row}");
                            assert!(row.contains(c.language.text("Test connection")), "{row}");
                        }
                        let expected = frame(&p, &c);
                        for _ in 0..3 {
                            assert_eq!(frame(&p, &c), expected);
                        }
                        if page == 1 {
                            assert_eq!(p.focus, 6);
                            assert!(matches!(
                                &p.action(6, &c)[0],
                                Effect::Nearby(crate::peer::Action::Confirm(false))
                            ));
                        }
                        if page == 2 {
                            p.focus = 22;
                            p.event(
                                Event::Key(KeyEvent::new(
                                    K::End,
                                    crossterm::event::KeyModifiers::NONE,
                                )),
                                &c,
                            );
                            assert_eq!(p.offer_cursor, 99);
                            assert!(p.controls(&c).controls.iter().any(|control| matches!(&control.kind, ControlKind::List {items,cursor:99,offset:93} if items[99].ends_with("99.txt"))));
                        }
                    }
                }
            }
        }
        p.event(Event::NearbyDirectory(PathBuf::from("путь/Ёжик")), &c);
        assert_eq!(p.picked, Some(PathBuf::from("путь/Ёжик")));
        p.focus = 21;
        p.event(Event::Paste("changed"), &c);
        assert!(p.picked.is_none());
    }
    #[test]
    fn refresh_preserves_selected_identity_and_exposes_shared_keyboard_mouse_controls() {
        let mut c = context();
        let mut p = NearbyTransfers::default();
        p.open(&c);
        c.nearby.network.ready = true;
        c.nearby.selected = 1;
        let peer = |n: u8| {
            let id = iroh::SecretKey::from_bytes(&[n; 32]).public();
            crate::peer::service::Peer {
                name: format!("DN {n}"),
                address: iroh::EndpointAddr::new(id)
                    .with_ip_addr("127.0.0.1:1234".parse().unwrap()),
            }
        };
        c.nearby.peers = vec![peer(3), peer(4)];
        p.prepare(&c);
        p.focus = 0;
        p.event(
            Event::Key(KeyEvent::new(K::Down, crossterm::event::KeyModifiers::NONE)),
            &c,
        );
        let selected = p.selected_peer;
        c.nearby.peers.clear();
        p.prepare(&c);
        assert!(p.peer(&c).is_none());
        c.nearby.peers = vec![peer(4), peer(3)];
        p.prepare(&c);
        assert_eq!(p.selected_peer, selected);
        assert_eq!(p.cursor, 0);
        assert!(matches!(
            &p.action(13, &c)[0],
            Effect::Nearby(crate::peer::Action::Refresh)
        ));
        let spec = p.controls(&c);
        assert!(spec
            .controls
            .iter()
            .any(|control| control.id == 13 && control.enabled));
        for _ in 0..3 {
            let before = frame(&p, &c);
            assert_eq!(frame(&p, &c), before);
        }
    }
}
