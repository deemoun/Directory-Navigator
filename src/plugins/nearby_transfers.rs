//! Presentation only: the host captures paths, owns networking and publishes files.
use super::*;
use crate::{
    painting::{Paragraph, Wrap},
    turbo::dialog_spec::{Control, ControlKind},
};
use crossterm::event::{KeyCode as K, MouseButton, MouseEventKind};
#[derive(Default)]
pub struct NearbyTransfers {
    page: u8,
    focus: usize,
    cursor: usize,
    offer_cursor: usize,
    manual: String,
    manual_cursor: usize,
    destination: String,
    destination_cursor: usize,
    picked: Option<PathBuf>,
}
impl NearbyTransfers {
    fn action(&mut self, id: usize, ctx: &Context) -> Vec<Effect> {
        use crate::peer::Action as A;
        let action = match id {
            1 => A::Receive(!ctx.nearby.receiving),
            2 => A::Capture,
            3 => {
                let Some(peer) = ctx.nearby.peers.get(self.cursor) else {
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
    fn paste(&mut self, text: &str) {
        match self.focus {
            20 => crate::dialog::paste_input(&mut self.manual, &mut self.manual_cursor, text),
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
    }
    fn prepare(&mut self, ctx: &Context) {
        let page = if ctx.nearby.pairing.is_some() {
            1
        } else if !ctx.nearby.offer.is_empty() {
            2
        } else {
            0
        };
        if self.page != page {
            self.page = page;
            self.focus = match page {
                1 => 6,
                2 => 21,
                _ => 1,
            };
        }
        self.cursor = self.cursor.min(ctx.nearby.peers.len().saturating_sub(1));
        self.offer_cursor = self
            .offer_cursor
            .min(ctx.nearby.offer.len().saturating_sub(1));
    }
    fn uses_controls(&self, key: KeyEvent) -> bool {
        !(matches!(self.focus, 20 | 21) && key.code == K::Char(' ')
            || self.focus == 0 && key.code == K::Enter)
    }
    fn event(&mut self, event: Event<'_>, ctx: &Context) -> Vec<Effect> {
        match event {
            Event::NearbyDirectory(path) => {
                self.destination = crate::filesystem::display_name(&path.to_string_lossy());
                self.destination_cursor = self.destination.len();
                self.picked = Some(path);
            }
            Event::Focus(focus) => self.focus = focus,
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
                } else {
                    self.focus = id.0;
                    return self.action(id.0, ctx);
                }
            }
            Event::Key(key) if key.code == K::F(1) => return self.action(11, ctx),
            Event::Key(key) if matches!(self.focus, 20 | 21) => {
                if self.focus == 20 {
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
            Event::Key(key) if self.focus == 0 => match key.code {
                K::Enter if ctx.nearby.selected > 0 && !ctx.nearby.busy => {
                    return self.action(3, ctx)
                }
                K::Up => self.cursor = self.cursor.saturating_sub(1),
                K::Down => {
                    self.cursor = (self.cursor + 1).min(ctx.nearby.peers.len().saturating_sub(1))
                }
                K::Home => self.cursor = 0,
                K::End => self.cursor = ctx.nearby.peers.len().saturating_sub(1),
                _ => {}
            },
            Event::Paste(text) => self.paste(text),
            _ => {}
        }
        vec![]
    }
    fn controls(&self, ctx: &Context) -> DialogSpec<'_> {
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
        let field = |spec: &mut DialogSpec<'_>, id, value: &str, cursor: usize, y| {
            let width = r.width - 19;
            let column =
                crate::presentation::text::Line::from(&value[..cursor.min(value.len())]).width();
            spec.control(Control::new(
                id,
                Rect::new(r.x + 3, r.y + y, width, 1),
                ControlKind::ScrolledInput {
                    value: value.to_owned(),
                    scroll: column.saturating_sub(usize::from(width - 1)),
                },
                ctx.target(id),
            ));
            if focus == id {
                spec.cursor = Some((r.x + 3 + column.min(usize::from(width - 1)) as u16, r.y + y));
            }
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
                ctx.nearby.selected > 0 && !ctx.nearby.peers.is_empty() && !ctx.nearby.busy,
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
                crate::filesystem::display_name(&ctx.nearby.details),
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
                ctx.nearby.selected > 0 && !self.manual.is_empty() && !ctx.nearby.busy,
            );
            text(
                &mut spec,
                format!(
                    "{}: {}",
                    ctx.language.text("Selected items"),
                    ctx.nearby.selected
                ),
                15,
                1,
            );
        }
        text(
            &mut spec,
            crate::filesystem::display_name(&ctx.language.field(
                &ctx.nearby.notice,
                crate::regional::DateTimeFormat::default(),
            )),
            17,
            2,
        );
        button(&mut spec, 11, "Help", 5, r.height - 2, 14, true);
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
                        p.prepare(&c);
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
}
