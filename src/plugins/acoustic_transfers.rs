//! Experimental modem presentation. Devices, encryption and publication belong to the host.
use super::*;
use crate::{
    painting::{Paragraph, Wrap},
    turbo::dialog_spec::{centered, Control, ControlKind},
};
use crossterm::event::{KeyCode as K, MouseButton, MouseEventKind};
#[derive(Default)]
pub struct AcousticTransfers {
    focus: usize,
    page: u64,
    file: bool,
    text: String,
    cursor: usize,
    destination: String,
    dest_cursor: usize,
    picked: Option<PathBuf>,
    revision: u64,
    session: u64,
    notice: String,
}
impl AcousticTransfers {
    fn token(&self) -> ClipboardToken {
        ClipboardToken {
            session: self.session,
            revision: self.revision,
            cursor: if self.focus == 1 {
                self.dest_cursor
            } else {
                self.cursor
            },
            anchor: Some(self.focus),
        }
    }
    fn action(&mut self, id: usize, ctx: &Context) -> Vec<Effect> {
        use crate::acoustic::Action as A;
        let action = match id {
            2 if !ctx.acoustic.busy => {
                self.file = !self.file;
                return vec![];
            }
            3 if !ctx.acoustic.busy => {
                self.file = true;
                A::Capture
            }
            4 if !ctx.acoustic.busy => {
                if self.file {
                    A::SendFile
                } else {
                    A::SendText(self.text.clone())
                }
            }
            5 if !ctx.acoustic.busy => A::Receive,
            6 => A::Stop,
            7 => {
                self.focus = 0;
                return vec![Effect::PasteText(self.token())];
            }
            8 => A::Tree,
            9 => return vec![Effect::Close],
            10 => {
                return vec![Effect::Help {
                    title: " Acoustic transfers Help ",
                    text: ctx.language.text(HELP).into(),
                }]
            }
            11 => A::Confirm(true),
            12 => A::Confirm(false),
            13 => A::Approve {
                accept: true,
                destination: self
                    .picked
                    .clone()
                    .filter(|p| p.to_string_lossy() == self.destination)
                    .unwrap_or_else(|| PathBuf::from(&self.destination)),
            },
            14 => A::Approve {
                accept: false,
                destination: PathBuf::new(),
            },
            15 => {
                if let Some(text) = &ctx.acoustic.received {
                    return vec![Effect::CopyTextOwned {
                        text: text.clone(),
                        serial: ctx.acoustic.generation,
                    }];
                } else {
                    return vec![];
                }
            }
            20 => {
                return ctx
                    .acoustic
                    .received
                    .as_ref()
                    .map_or_else(Vec::new, |text| {
                        vec![Effect::Help {
                            title: " Acoustic text preview ",
                            text: text.clone(),
                        }]
                    })
            }
            16 => A::Dismiss,
            17 => A::Conflict(crate::operations::conflict::Decision::Replace),
            18 => A::Conflict(crate::operations::conflict::Decision::Skip),
            19 => A::Conflict(crate::operations::conflict::Decision::Cancel),
            _ => return vec![],
        };
        vec![Effect::Acoustic {
            generation: ctx.acoustic.generation,
            action,
        }]
    }
}
impl ToolPlugin for AcousticTransfers {
    fn definition(&self) -> Definition {
        Definition::builtin(
            ACOUSTIC_TRANSFERS,
            "Acoustic transfers (Experimental)",
            Presentation::Document,
        )
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn open(&mut self, _: &Context) {
        let session = self.session.wrapping_add(1);
        *self = Self {
            session,
            ..Default::default()
        };
    }
    fn prepare(&mut self, ctx: &Context) {
        let v = &ctx.acoustic;
        let phase = if v.code.is_some() {
            1
        } else if v.offer.is_some() {
            2
        } else if v.conflict.is_some() {
            3
        } else if v.received.is_some() {
            4
        } else {
            0
        };
        let page = v.generation.wrapping_mul(8).wrapping_add(phase);
        if self.page != page {
            if self.page / 8 != v.generation {
                self.notice.clear();
            }
            self.page = page;
            self.revision = self.revision.wrapping_add(1);
            self.focus = match phase {
                1 => 12,
                2 => 14,
                3 => 18,
                4 => 20,
                _ => 0,
            };
        }
    }
    fn close(&mut self) {
        let session = self.session;
        *self = Self {
            session,
            ..Default::default()
        };
    }
    fn clipboard_token(&self) -> Option<ClipboardToken> {
        (self.focus == 1 || !self.file).then(|| self.token())
    }
    fn uses_controls(&self, key: KeyEvent) -> bool {
        !matches!((self.focus, key.code), (0 | 1, K::Char(' ')))
    }
    fn event(&mut self, event: Event<'_>, ctx: &Context) -> Vec<Effect> {
        let mut action = None;
        match event {
            Event::Focus(id) => self.focus = id,
            Event::Action(id) => action = Some(id.0),
            Event::Mouse(m, Some(id)) if m.kind == MouseEventKind::Down(MouseButton::Left) => {
                self.focus = id.0;
                if id.0 > 1 {
                    action = Some(id.0);
                }
            }
            Event::AcousticDirectory(path) => {
                self.destination = path.to_string_lossy().into_owned();
                self.picked = Some(path);
                self.dest_cursor = self.destination.len();
            }
            Event::ClipboardCopied { serial, message }
                if serial == ctx.acoustic.generation && ctx.acoustic.received.is_some() =>
            {
                self.notice = message
            }
            Event::ClipboardPasteNotice { token, message } if token == self.token() => {
                self.notice = message
            }
            Event::Paste(text) if self.focus == 0 && !ctx.acoustic.busy => {
                if self.text.len() + text.len() <= 1024 {
                    if text
                        .chars()
                        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
                    {
                        self.notice =
                            "Control characters cannot be composed; nothing was inserted.".into();
                        return vec![];
                    }
                    self.text.insert_str(self.cursor, text);
                    self.cursor += text.len();
                    self.revision = self.revision.wrapping_add(1);
                } else {
                    self.notice = "Text limit is 1 KiB; nothing inserted.".into();
                }
            }
            Event::Paste(text) if self.focus == 1 => {
                if self.destination.len() + text.len() <= 4096 {
                    crate::dialog::paste_input(&mut self.destination, &mut self.dest_cursor, text);
                    self.revision = self.revision.wrapping_add(1);
                    self.picked = None;
                }
            }
            Event::Key(k) if self.focus == 1 || self.focus == 0 && !ctx.acoustic.busy => {
                let (value, cursor, limit) = if self.focus == 0 {
                    (&mut self.text, &mut self.cursor, 1024)
                } else {
                    (&mut self.destination, &mut self.dest_cursor, 4096)
                };
                let before = value.clone();
                let old_cursor = *cursor;
                crate::dialog::edit_input(value, cursor, k.code);
                if value.len() > limit {
                    *value = before;
                    *cursor = old_cursor;
                } else {
                    self.revision = self.revision.wrapping_add(1);
                }
            }
            _ => {}
        }
        action.map_or_else(Vec::new, |id| self.action(id, ctx))
    }
    fn controls(&self, ctx: &Context) -> DialogSpec<'_> {
        let r = centered(ctx.area, 76, 23);
        let v = &ctx.acoustic;
        let cancel = if v.busy { 6 } else { 9 };
        let default = if v.code.is_some() {
            12
        } else if v.offer.is_some() {
            14
        } else if v.conflict.is_some() {
            18
        } else {
            4
        };
        let mut s = ctx.dialog(
            r,
            self.focus,
            ctx.language.text("Acoustic transfers (Experimental)"),
            default,
            cancel,
        );
        let caption = |s: &mut DialogSpec<'_>, text: String, y: u16, h: u16| {
            s.caption(
                Paragraph::new(text).wrap(Wrap { trim: false }),
                Rect::new(r.x + 3, r.y + y, r.width - 6, h),
            )
        };
        caption(&mut s,ctx.language.text("Audible modem · max 1 KiB · both devices need microphones · minutes, not LAN speed").into(),1,2);
        let enabled = crate::acoustic::available();
        s.control({
            let mut control = Control::new(
                2,
                Rect::new(r.x + 3, r.y + 3, 18, 1),
                ControlKind::Checkbox {
                    caption: ctx.language.text("File mode").into(),
                    checked: self.file,
                },
                ctx.target(2),
            );
            control.enabled = enabled && !v.busy;
            control
        });
        s.control({
            let mut control = Control::new(
                3,
                Rect::new(r.x + 24, r.y + 3, 25, 1),
                ControlKind::Button(ctx.language.text("Use current file").into()),
                ctx.target(3),
            );
            control.enabled = enabled && !v.busy;
            control
        });
        s.control({
            let mut control = Control::new(
                7,
                Rect::new(r.x + 53, r.y + 3, 18, 1),
                ControlKind::Button(ctx.language.text("Paste text").into()),
                ctx.target(7),
            );
            control.enabled = enabled && !v.busy && !self.file;
            control
        });
        let value = if self.file {
            v.source.clone()
        } else {
            crate::filesystem::display_name(&self.text)
        };
        let col = if self.file {
            0
        } else {
            crate::presentation::text::Line::from(crate::filesystem::display_name(
                &self.text[..self.cursor],
            ))
            .width()
        };
        let scroll = col.saturating_sub(usize::from(r.width - 7));
        if self.focus == 0 && enabled && !v.busy && !self.file {
            s.cursor = Some((r.x + 3 + (col - scroll) as u16, r.y + 5));
        }
        s.control({
            let mut control = Control::new(
                0,
                Rect::new(r.x + 3, r.y + 5, r.width - 6, 1),
                ControlKind::ScrolledInput { value, scroll },
                ctx.target(0),
            );
            control.enabled = enabled && !v.busy && !self.file;
            control
        });
        caption(
            &mut s,
            format!(
                "{} {}% / {}%",
                ctx.language.text("Input / output levels:"),
                v.input,
                v.output
            ),
            7,
            1,
        );
        caption(
            &mut s,
            if v.busy {
                v.devices.clone()
            } else {
                ctx.language.text("Devices closed").into()
            },
            6,
            1,
        );
        let status = if !enabled {
            "Acoustic support is unavailable in this build; rebuild with --features acoustic."
        } else if v.notice.is_empty() {
            "Devices closed. Send/Receive opens the system defaults."
        } else {
            &v.notice
        };
        caption(
            &mut s,
            if let Some(cause) = &v.cause {
                format!(
                    "{}: {}",
                    ctx.language.text(status),
                    ctx.language.text(cause)
                )
            } else {
                ctx.language.text(status).into()
            },
            8,
            3,
        );
        caption(
            &mut s,
            format!(
                "{} {} / {}",
                ctx.language.text("Packets sent / received:"),
                v.packets.0,
                v.packets.1
            ),
            11,
            1,
        );
        if let Some(code) = &v.code {
            caption(
                &mut s,
                format!(
                    "{} {code}",
                    ctx.language.text("Compare code on both devices:")
                ),
                12,
                1,
            );
            for (id, label, x) in [(11, "Codes match", 3), (12, "Cancel pairing", 38)] {
                s.control(Control::new(
                    id,
                    Rect::new(r.x + x, r.y + 14, 30, 1),
                    ControlKind::Button(ctx.language.text(label).into()),
                    ctx.target(id),
                ));
            }
        } else if let Some(conflict) = &v.conflict {
            caption(
                &mut s,
                format!(
                    "{} → {}\n{} / {} bytes",
                    conflict.source.display(),
                    conflict.destination.display(),
                    conflict.source_size,
                    conflict.destination_size
                ),
                12,
                3,
            );
            for (id, label, x) in [(17, "Replace", 3), (18, "Skip", 26), (19, "Cancel", 49)] {
                s.control(Control::new(
                    id,
                    Rect::new(r.x + x, r.y + 16, 19, 1),
                    ControlKind::Button(ctx.language.text(label).into()),
                    ctx.target(id),
                ));
            }
        } else if let Some(offer) = &v.offer {
            caption(
                &mut s,
                format!("{} {offer}", ctx.language.text("Incoming offer:")),
                12,
                1,
            );
            let col = crate::presentation::text::Line::from(&self.destination[..self.dest_cursor])
                .width();
            let scroll = col.saturating_sub(49);
            if self.focus == 1 {
                s.cursor = Some((r.x + 3 + (col - scroll) as u16, r.y + 14));
            }
            s.control(Control::new(
                1,
                Rect::new(r.x + 3, r.y + 14, 50, 1),
                ControlKind::ScrolledInput {
                    value: self.destination.clone(),
                    scroll,
                },
                ctx.target(1),
            ));
            for (id, label, x) in [(8, "Tree", 56), (13, "Accept", 3), (14, "Decline", 27)] {
                s.control(Control::new(
                    id,
                    Rect::new(
                        r.x + x,
                        r.y + if id == 8 { 14 } else { 16 },
                        if id == 8 { 14 } else { 20 },
                        1,
                    ),
                    ControlKind::Button(ctx.language.text(label).into()),
                    ctx.target(id),
                ));
            }
        } else if let Some(text) = &v.received {
            caption(&mut s, text.clone(), 12, 4);
            for (id, label, x) in [(20, "Preview", 3), (15, "Copy", 27), (16, "Dismiss", 51)] {
                s.control(Control::new(
                    id,
                    Rect::new(r.x + x, r.y + 16, 19, 1),
                    ControlKind::Button(ctx.language.text(label).into()),
                    ctx.target(id),
                ));
            }
        }
        if !self.notice.is_empty() {
            caption(&mut s, ctx.language.text(&self.notice).into(), 18, 1);
        }
        for (id, label, x, width) in [
            (4, "Send", 3, 12),
            (5, "Receive", 18, 12),
            (6, "Stop", 33, 12),
            (10, "Help", 48, 10),
            (9, "Close", 61, 10),
        ] {
            s.control({
                let mut control = Control::new(
                    id,
                    Rect::new(r.x + x, r.y + 20, width, 1),
                    ControlKind::Button(ctx.language.text(label).into()),
                    ctx.target(id),
                );
                control.enabled = match id {
                    4 | 5 => enabled && !v.busy,
                    6 => v.busy,
                    _ => true,
                };
                control
            });
        }
        s
    }
}
pub const HELP:&str="Experimental acoustic transfers require a build with --features acoustic and explicit activation in Options > Configuration > Plugins. No audio device opens until Send or Receive. Both devices need microphones for acknowledgements. The default input/output devices are used, with audible tones; DN never changes system volume.\nText is limited to 1 KiB of UTF-8; File mode accepts one regular file of at most 1 KiB. Use current file captures the cursor entry. Folders, links, archive and remote members are excluded. Extract/download first. Transfers can take minutes; this is not a replacement for Nearby LAN transfers.\nChoose Send on one device and Receive on the other. Compare the eight-digit code on both screens and confirm only if they match. Noise XX encrypts and authenticates the session; approval is required again for each offer. Identities and previews are session-only. There is no automatic acceptance.\nReceivers can retain and Copy text using DN's clipboard bridge. Files require a destination; Tree selects it. Existing files use guarded Replace/Skip/Cancel handling; Skip is the safe default. Completed publications remain after cancellation. Delivery is confirmed only by the final receipt; missing receipts mean unconfirmed delivery.\nStop or closing this window closes devices. Music paused by this session resumes only if its state has not changed. Audio-device loss and exhausted retries stop the session. Reliability depends on the room, devices and audio processing and is not established without hardware testing.";

#[cfg(all(test, feature = "acoustic"))]
mod tests {
    use super::*;
    fn context() -> Context {
        Context {
            binding: Binding {
                plugin: ACOUSTIC_TRANSFERS,
                owner: Owner::Document(crate::desktop::WindowId::MANAGER),
            },
            scope: ControlScope::Document {
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
            dos: Default::default(),
            video: Default::default(),
            video_preferences: Default::default(),
            acoustic: crate::acoustic::Snapshot {
                generation: 7,
                received: Some("received".into()),
                ..Default::default()
            },
            nearby: Default::default(),
            bluetooth: Default::default(),
        }
    }
    #[test]
    fn clipboard_results_belong_to_the_current_received_session() {
        let mut ctx = context();
        let mut p = AcousticTransfers::default();
        p.open(&ctx);
        p.prepare(&ctx);
        assert!(matches!(
            p.action(15, &ctx).as_slice(),
            [Effect::CopyTextOwned { serial: 7, .. }]
        ));
        p.event(
            Event::ClipboardCopied {
                serial: 6,
                message: "old copy".into(),
            },
            &ctx,
        );
        assert!(p.notice.is_empty());
        p.event(
            Event::ClipboardCopied {
                serial: 7,
                message: "copied".into(),
            },
            &ctx,
        );
        assert_eq!(p.notice, "copied");
        ctx.acoustic.generation = 8;
        ctx.acoustic.received = None;
        p.prepare(&ctx);
        assert!(p.notice.is_empty());
        p.event(
            Event::ClipboardCopied {
                serial: 7,
                message: "late copy".into(),
            },
            &ctx,
        );
        assert!(p.notice.is_empty());
    }
    #[test]
    fn reopening_discards_old_destinations_and_keeps_multiline_payload_exact() {
        let mut ctx = context();
        ctx.acoustic.received = None;
        let mut p = AcousticTransfers {
            text: "old".into(),
            cursor: 3,
            destination: "/previous/destination".into(),
            picked: Some(PathBuf::from("/previous/destination")),
            file: true,
            ..Default::default()
        };
        p.close();
        assert_eq!(p.cursor, 0);
        assert!(p.destination.is_empty() && p.picked.is_none());
        assert!(!p.file);
        p.open(&ctx);
        p.prepare(&ctx);
        let _ = p.controls(&ctx);
        let text = "Ёжик\nlink\r\nnext\tcolumn";
        p.event(Event::Paste(text), &ctx);
        assert_eq!(p.text, text);
        let spec = p.controls(&ctx);
        let field = spec.controls.iter().find(|c| c.id == 0).unwrap();
        assert!(
            matches!(&field.kind,ControlKind::ScrolledInput{value,..} if !value.contains(['\n','\r','\t']))
        );
        assert!(
            matches!(p.action(4,&ctx).as_slice(),[Effect::Acoustic{action:crate::acoustic::Action::SendText(value),..}] if value==text)
        );
        p.text = "x".repeat(200);
        p.cursor = p.text.len();
        p.file = true;
        ctx.acoustic.source = "tiny.bin (4 bytes)".into();
        let spec = p.controls(&ctx);
        let field = spec.controls.iter().find(|c| c.id == 0).unwrap();
        assert!(
            matches!(&field.kind,ControlKind::ScrolledInput{value,scroll:0} if value.contains("tiny.bin"))
        );
        p.close();
        p.open(&ctx);
        assert!(p.text.is_empty());
        let _ = p.controls(&ctx);
    }
}
