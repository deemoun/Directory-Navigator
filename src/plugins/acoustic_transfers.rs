//! Session-only presentation for the host-owned one-way acoustic modem.
use super::*;
use crate::windowing::geometry::Capabilities;
use crate::{
    painting::{Frame, NativeFrame, Paragraph, Wrap},
    turbo::{
        dialog_spec::{centered, Control, ControlKind},
        file_dialog::{Mode, Outcome, Picker},
    },
};
use crossterm::event::{KeyCode as K, MouseButton, MouseEventKind};
use zeroize::Zeroizing;
const PICKER: u64 = 2;
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
    draft_revision: u64,
    session: u64,
    notice: String,
    passphrase: Zeroizing<String>,
    pass_cursor: usize,
    speed: crate::acoustic::Speed,
    repeats: Option<u8>,
    picker: Option<Picker>,
    export: bool,
}
impl AcousticTransfers {
    fn changed(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.draft_revision = self.draft_revision.wrapping_add(1);
        self.notice.clear();
    }
    fn token(&self) -> ClipboardToken {
        ClipboardToken {
            session: self.session,
            revision: self.revision,
            cursor: match self.focus {
                1 => self.dest_cursor,
                21 => self.pass_cursor,
                _ => self.cursor,
            },
            anchor: Some(self.focus),
        }
    }
    fn options(&self) -> crate::acoustic::Options {
        crate::acoustic::Options {
            passphrase: self.passphrase.clone(),
            speed: self.speed,
            repeats: self.repeats.unwrap_or(2),
        }
    }
    fn request(&self, operation: crate::acoustic::Operation) -> crate::acoustic::Request {
        let input = if matches!(
            operation,
            crate::acoustic::Operation::Receive | crate::acoustic::Operation::Import(_)
        ) {
            crate::acoustic::Input::None
        } else if self.file {
            crate::acoustic::Input::File
        } else {
            crate::acoustic::Input::Text(self.text.clone())
        };
        crate::acoustic::Request {
            operation,
            input,
            options: self.options(),
            revision: self.draft_revision,
        }
    }
    fn start(&mut self, operation: crate::acoustic::Operation, ctx: &Context) -> Vec<Effect> {
        if let Err(e) = self.options().validate() {
            self.notice = e.to_string();
            self.focus = 21;
            return vec![];
        }
        vec![Effect::Acoustic {
            generation: ctx.acoustic.generation,
            action: crate::acoustic::Action::Start(self.request(operation)),
        }]
    }
    fn pick(&mut self, export: bool, ctx: &Context) {
        self.revision = self.revision.wrapping_add(1);
        self.export = export;
        let base = self
            .picked
            .clone()
            .unwrap_or_else(|| PathBuf::from(&self.destination));
        let mut picker = Picker::new(
            if export { Mode::SaveAs } else { Mode::Open },
            base,
            ctx.language,
            true,
        )
        .with_target(ctx.target(900));
        if export {
            picker.set_filename("acoustic-transfer.wav".into());
        }
        self.picker = Some(picker);
    }
    fn action(&mut self, id: usize, ctx: &Context) -> Vec<Effect> {
        use crate::acoustic::{Action as A, Operation as O};
        let action = match id {
            2 if !ctx.acoustic.busy => {
                self.file = !self.file;
                self.changed();
                return vec![];
            }
            3 if !ctx.acoustic.busy => {
                self.file = true;
                self.changed();
                A::Capture
            }
            4 if !ctx.acoustic.busy => return self.start(O::Send, ctx),
            5 if !ctx.acoustic.busy => return self.start(O::Receive, ctx),
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
                    text: format!(
                        "{}\n\n{}",
                        ctx.language.text(HELP),
                        ctx.acoustic
                            .conflict
                            .as_ref()
                            .map(|c| format!(
                                "{} → {}",
                                crate::filesystem::display_name(&c.source.display().to_string()),
                                crate::filesystem::display_name(
                                    &c.destination.display().to_string()
                                )
                            ))
                            .or_else(|| ctx.acoustic.offer.clone())
                            .unwrap_or_default()
                    ),
                }]
            }
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
                return ctx
                    .acoustic
                    .received
                    .as_ref()
                    .map_or_else(Vec::new, |text| {
                        vec![Effect::CopyTextOwned {
                            text: text.clone(),
                            serial: ctx.acoustic.generation,
                        }]
                    })
            }
            16 => A::Dismiss,
            17 => A::Conflict(crate::operations::conflict::Decision::Replace),
            18 => A::Conflict(crate::operations::conflict::Decision::Skip),
            19 => A::Conflict(crate::operations::conflict::Decision::Cancel),
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
            22 if !ctx.acoustic.busy => {
                self.speed = self.speed.next();
                self.changed();
                return vec![];
            }
            23 if !ctx.acoustic.busy => {
                self.repeats = Some(match self.repeats.unwrap_or(2) {
                    0 => 1,
                    1 => 2,
                    2 => 3,
                    _ => 0,
                });
                self.changed();
                return vec![];
            }
            24 | 25 if !ctx.acoustic.busy => {
                if let Err(e) = self.options().validate() {
                    self.notice = e.to_string();
                    self.focus = 21;
                    return vec![];
                }
                if id == 24 && self.options().repeats == 0 {
                    self.notice = "WAV export needs a finite repeat count".into();
                    return vec![];
                }
                self.pick(id == 24, ctx);
                return vec![];
            }
            26 if !ctx.acoustic.busy => return self.start(O::Estimate, ctx),
            _ => return vec![],
        };
        vec![Effect::Acoustic {
            generation: ctx.acoustic.generation,
            action,
        }]
    }
    fn main_spec(&self, ctx: &Context) -> DialogSpec<'_> {
        let r = centered(ctx.area, 76, 23);
        let v = &ctx.acoustic;
        let enabled = crate::acoustic::available();
        let default = if v.offer.is_some() {
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
            if v.busy { 6 } else { 9 },
        );
        let caption = |s: &mut DialogSpec<'_>, text: String, y: u16, h: u16| {
            s.caption(
                Paragraph::new(text).wrap(Wrap { trim: false }),
                Rect::new(r.x + 3, r.y + y, r.width - 6, h),
            )
        };
        caption(
            &mut s,
            ctx.language
                .text("One-way audio · max 1 MiB · Send: speaker only · Receive: microphone only")
                .into(),
            1,
            2,
        );
        let button = |s: &mut DialogSpec<'_>, id, caption: String, x, y, w, active| {
            let mut c = Control::new(
                id,
                Rect::new(r.x + x, r.y + y, w, 1),
                ControlKind::Button(caption),
                ctx.target(id),
            );
            c.enabled = active;
            s.control(c);
        };
        let mut c = Control::new(
            2,
            Rect::new(r.x + 3, r.y + 3, 18, 1),
            ControlKind::Checkbox {
                caption: ctx.language.text("File mode").into(),
                checked: self.file,
            },
            ctx.target(2),
        );
        c.enabled = enabled && !v.busy;
        s.control(c);
        button(
            &mut s,
            3,
            ctx.language.text("Use current file").into(),
            24,
            3,
            25,
            enabled && !v.busy,
        );
        button(
            &mut s,
            7,
            ctx.language.text("Paste text").into(),
            53,
            3,
            18,
            enabled && !v.busy && !self.file,
        );
        let value = if self.file {
            v.source.clone()
        } else {
            crate::filesystem::display_name(&self.text)
        };
        let col = if self.file {
            0
        } else {
            crate::presentation::text::Line::from(crate::filesystem::display_name(
                &self.text[..self.cursor.min(self.text.len())],
            ))
            .width()
        };
        let scroll = col.saturating_sub(usize::from(r.width - 7));
        let mut c = Control::new(
            0,
            Rect::new(r.x + 3, r.y + 5, r.width - 6, 1),
            ControlKind::ScrolledInput { value, scroll },
            ctx.target(0),
        );
        c.enabled = enabled && !v.busy && !self.file;
        s.control(c);
        if self.focus == 0 && enabled && !v.busy && !self.file {
            s.cursor = Some((r.x + 3 + (col - scroll) as u16, r.y + 5));
        }
        caption(
            &mut s,
            ctx.language
                .text("Shared passphrase (same on both devices; never saved)")
                .into(),
            6,
            1,
        );
        let n = self.passphrase.chars().count();
        let col = self.passphrase[..self.pass_cursor.min(self.passphrase.len())]
            .chars()
            .count();
        let scroll = col.saturating_sub(usize::from(r.width - 7));
        let mut c = Control::new(
            21,
            Rect::new(r.x + 3, r.y + 7, r.width - 6, 1),
            ControlKind::ScrolledInput {
                value: "*".repeat(n),
                scroll,
            },
            ctx.target(21),
        );
        c.enabled = enabled && !v.busy;
        s.control(c);
        if self.focus == 21 && enabled && !v.busy {
            s.cursor = Some((r.x + 3 + (col - scroll) as u16, r.y + 7));
        }
        button(
            &mut s,
            22,
            format!(
                "{}: {}",
                ctx.language.text("Speed"),
                ctx.language.text(self.speed.caption())
            ),
            3,
            8,
            30,
            enabled && !v.busy,
        );
        button(
            &mut s,
            23,
            format!(
                "{}: {}",
                ctx.language.text("Repeats"),
                if self.repeats.unwrap_or(2) == 0 {
                    ctx.language.text("Until Stop").into()
                } else {
                    self.repeats.unwrap_or(2).to_string()
                }
            ),
            38,
            8,
            33,
            enabled && !v.busy,
        );
        let status = if !enabled {
            "Acoustic support is unavailable in this build; rebuild with --features acoustic."
        } else if v.notice.is_empty() {
            "Devices closed. Estimate opens no audio device."
        } else {
            &v.notice
        };
        let message = if !self.notice.is_empty() {
            ctx.language.text(&self.notice).to_owned()
        } else if let Some(cause) = &v.cause {
            format!(
                "{}: {}",
                ctx.language.text(status),
                ctx.language.message(cause)
            )
        } else {
            ctx.language.text(status).into()
        };
        caption(&mut s, message, 10, 2);
        let remaining = v
            .remaining
            .map(|n| format!("~{}", time(n)))
            .unwrap_or_else(|| "--:--:--".into());
        let progress = if v.busy {
            format!(
                "{} {} / {} · {} {} · {} {}",
                ctx.language.text("Blocks"),
                v.completed,
                if v.total == 0 {
                    "--".into()
                } else {
                    v.total.to_string()
                },
                ctx.language.text("Elapsed:"),
                time(v.elapsed),
                ctx.language.text("Remaining:"),
                remaining
            )
        } else if v.estimate.is_some() && v.plan_revision == self.draft_revision {
            format!(
                "{} {} · {}",
                ctx.language.text(if self.repeats == Some(0) {
                    "Estimated playback per cycle:"
                } else {
                    "Estimated playback:"
                }),
                time(v.estimate.unwrap()),
                ctx.language.text("After compression")
            )
        } else {
            format!(
                "{} {} · {} {} / {}",
                ctx.language.text("Elapsed:"),
                time(v.elapsed),
                ctx.language.text("Packets sent / received:"),
                v.packets.0,
                v.packets.1
            )
        };
        caption(&mut s, progress, 12, 1);
        caption(
            &mut s,
            if v.devices.is_empty() {
                ctx.language.text("Devices closed").into()
            } else {
                format!(
                    "{} · {}% / {}% · {} {}",
                    crate::filesystem::display_name(&v.devices),
                    v.input,
                    v.output,
                    ctx.language.text("Dropouts:"),
                    v.dropped
                )
            },
            13,
            1,
        );
        if let Some(conflict) = &v.conflict {
            caption(
                &mut s,
                format!(
                    "{}\n{} → {}",
                    ctx.language.text("Waiting for confirmation"),
                    crate::filesystem::display_name(&conflict.source.display().to_string()),
                    crate::filesystem::display_name(&conflict.destination.display().to_string())
                ),
                14,
                2,
            );
            for (id, label, x) in [(17, "Replace", 3), (18, "Skip", 26), (19, "Cancel", 49)] {
                button(&mut s, id, ctx.language.text(label).into(), x, 16, 19, true);
            }
        } else if let Some(offer) = &v.offer {
            caption(
                &mut s,
                format!("{} {offer}", ctx.language.text("Verified incoming data:")),
                14,
                1,
            );
            if v.file_offer {
                let col = crate::presentation::text::Line::from(
                    &self.destination[..self.dest_cursor.min(self.destination.len())],
                )
                .width();
                let scroll = col.saturating_sub(49);
                if self.focus == 1 {
                    s.cursor = Some((r.x + 3 + (col - scroll) as u16, r.y + 15));
                }
                s.control(Control::new(
                    1,
                    Rect::new(r.x + 3, r.y + 15, 50, 1),
                    ControlKind::ScrolledInput {
                        value: self.destination.clone(),
                        scroll,
                    },
                    ctx.target(1),
                ));
                button(
                    &mut s,
                    8,
                    ctx.language.text("Tree").into(),
                    56,
                    15,
                    14,
                    true,
                );
            }
            for (id, label, x, y, w) in [(13, "Accept", 3, 16, 20), (14, "Decline", 27, 16, 20)] {
                button(&mut s, id, ctx.language.text(label).into(), x, y, w, true);
            }
        } else if let Some(text) = &v.received {
            caption(
                &mut s,
                crate::filesystem::display_name(&text.chars().take(256).collect::<String>()),
                14,
                2,
            );
            for (id, label, x) in [(20, "Preview", 3), (15, "Copy", 27), (16, "Dismiss", 51)] {
                button(&mut s, id, ctx.language.text(label).into(), x, 16, 19, true);
            }
        }
        for (id, label, x, w) in [
            (24, "Export WAV...", 3, 24),
            (25, "Receive WAV...", 29, 24),
            (26, "Estimate", 55, 16),
        ] {
            button(
                &mut s,
                id,
                ctx.language.text(label).into(),
                x,
                18,
                w,
                enabled && !v.busy,
            );
        }
        for (id, label, x, w) in [
            (4, "Send", 3, 12),
            (5, "Receive", 18, 12),
            (6, "Stop", 33, 12),
            (10, "Help", 48, 10),
            (9, "Close", 61, 10),
        ] {
            button(
                &mut s,
                id,
                ctx.language.text(label).into(),
                x,
                20,
                w,
                match id {
                    4 | 5 => enabled && !v.busy,
                    6 => v.busy,
                    _ => true,
                },
            );
        }
        s
    }
}
fn time(seconds: u64) -> String {
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
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
    fn close(&mut self) {
        let session = self.session;
        *self = Self {
            session,
            ..Default::default()
        };
    }
    fn prepare(&mut self, ctx: &Context) {
        let phase = if ctx.acoustic.offer.is_some() {
            1
        } else if ctx.acoustic.conflict.is_some() {
            2
        } else if ctx.acoustic.received.is_some() {
            3
        } else {
            0
        };
        let page = ctx.acoustic.generation.wrapping_mul(4).wrapping_add(phase);
        if page != self.page {
            self.page = page;
            self.notice.clear();
            self.focus = match phase {
                1 => 14,
                2 => 18,
                3 => 20,
                _ => 0,
            };
            self.revision = self.revision.wrapping_add(1);
        }
        if let Some(p) = &mut self.picker {
            p.prepare(
                crate::presentation::layout::Size::new(ctx.area.width, ctx.area.height),
                ctx.palette,
            );
        }
    }
    fn clipboard_token(&self) -> Option<ClipboardToken> {
        if self.picker.is_some() {
            return None;
        }
        (self.focus == 1 || self.focus == 21 || !self.file).then(|| self.token())
    }
    fn uses_controls(&self, key: KeyEvent) -> bool {
        self.picker.is_none() && !matches!((self.focus, key.code), (0 | 1 | 21, K::Char(' ')))
    }
    fn native_input_page(&self) -> Option<u64> {
        self.picker.as_ref().map(|_| PICKER)
    }
    fn event(&mut self, event: Event<'_>, ctx: &Context) -> Vec<Effect> {
        if let Some(p) = &mut self.picker {
            let outcome = match event {
                Event::Key(k) => p.key(k),
                Event::Mouse(m, _) => p.mouse(m, (0, 0)),
                Event::Paste(t) => {
                    p.paste(t);
                    None
                }
                Event::Action(ActionId(900)) => Some(Outcome::Cancelled),
                _ => None,
            };
            if let Some(outcome) = outcome {
                self.picker = None;
                self.revision = self.revision.wrapping_add(1);
                if let Outcome::Selected(_, path) = outcome {
                    return self.start(
                        if self.export {
                            crate::acoustic::Operation::Export(path)
                        } else {
                            crate::acoustic::Operation::Import(path)
                        },
                        ctx,
                    );
                }
            }
            return vec![];
        }
        let mut action = None;
        match event {
            Event::Focus(id) => self.focus = id,
            Event::Action(id) => action = Some(id.0),
            Event::Mouse(m, Some(id)) if m.kind == MouseEventKind::Down(MouseButton::Left) => {
                self.focus = id.0;
                if !matches!(id.0, 0 | 1 | 21) {
                    action = Some(id.0);
                }
            }
            Event::AcousticDirectory(path) => {
                self.destination = path.to_string_lossy().into();
                self.dest_cursor = self.destination.len();
                self.picked = Some(path);
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
                if self.text.len() + text.len() <= crate::acoustic::MAX_BYTES {
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
                    self.changed();
                } else {
                    self.notice = "Text limit is 1 MiB; nothing inserted.".into();
                }
            }
            Event::Paste(text) if self.focus == 1 => {
                if self.destination.len() + text.len() <= 4096 {
                    crate::dialog::paste_input(&mut self.destination, &mut self.dest_cursor, text);
                    self.picked = None;
                    self.changed();
                }
            }
            Event::Paste(text) if self.focus == 21 && !ctx.acoustic.busy => {
                if self.passphrase.len() + text.len() <= crate::acoustic::MAX_PASSWORD
                    && !text.chars().any(char::is_control)
                {
                    self.passphrase.insert_str(self.pass_cursor, text);
                    self.pass_cursor += text.len();
                    self.changed();
                }
            }
            Event::Key(k)
                if self.focus == 1 || matches!(self.focus, 0 | 21) && !ctx.acoustic.busy =>
            {
                let (value, cursor, limit): (&mut String, &mut usize, usize) = match self.focus {
                    0 => (&mut self.text, &mut self.cursor, crate::acoustic::MAX_BYTES),
                    21 => (
                        &mut self.passphrase,
                        &mut self.pass_cursor,
                        crate::acoustic::MAX_PASSWORD,
                    ),
                    _ => (&mut self.destination, &mut self.dest_cursor, 4096),
                };
                let before = Zeroizing::new(value.clone());
                let old = *cursor;
                crate::dialog::edit_input(value, cursor, k.code);
                if value.len() > limit {
                    *value = before.to_string();
                    *cursor = old;
                } else {
                    self.changed();
                }
            }
            _ => {}
        }
        action.map_or_else(Vec::new, |id| self.action(id, ctx))
    }
    fn controls(&self, ctx: &Context) -> DialogSpec<'_> {
        if let Some(p) = &self.picker {
            let mut s = ctx.dialog(p.bounds(), 0, ctx.language.text("Open file"), 900, 900);
            let mut c = Control::new(900, s.bounds, ControlKind::Surface, ctx.target(900));
            c.paint = false;
            s.control(c);
            s
        } else {
            self.main_spec(ctx)
        }
    }
    fn windows(&self, ctx: &Context) -> Vec<LocalWindow> {
        let r = self.main_spec(ctx).bounds;
        let mut out = vec![LocalWindow {
            id: 0,
            parent: None,
            bounds: r,
            caps: Capabilities::fixed_zoom(r.width, r.height),
        }];
        if let Some(p) = &self.picker {
            let r = p.bounds();
            out.push(LocalWindow {
                id: PICKER,
                parent: Some(0),
                bounds: r,
                caps: Capabilities::fixed(r.width, r.height),
            });
        }
        out
    }
    fn view(&self, ctx: &Context) -> View<'_> {
        let s = self.main_spec(ctx);
        let mut f = NativeFrame::new(ctx.area, ctx.palette);
        f.render_widget(
            crate::turbo::widgets::dialog(ctx.language.text("Acoustic transfers (Experimental)")),
            s.bounds,
        );
        let mut layers = vec![Layer {
            id: 0,
            interactive: self.picker.is_none(),
            scene: f.finish(),
            controls: Some(s),
        }];
        if let Some(p) = &self.picker {
            if let Some(mut scene) = p.scene().cloned() {
                scene.regions = vec![(p.bounds(), ctx.target(900))];
                layers.push(Layer {
                    id: PICKER,
                    interactive: true,
                    scene,
                    controls: None,
                });
            }
        }
        View::Layers(layers)
    }
}
pub const HELP:&str="Experimental one-way acoustic transfers require --features acoustic and activation in Plugins. Send opens only the default speaker; Receive opens only the default microphone. No acknowledgements, pairing handshake or synchronized start are needed. The receiver collects numbered blocks across repetitions. Start Receive before playback, or keep repeating so late receivers can collect a complete cycle.\nEnter the same strong passphrase on both devices (8–1024 UTF-8 bytes); it is never saved. Argon2id derives a key and XChaCha20-Poly1305 authenticates the complete encrypted envelope. Wrong passwords, missing blocks and damaged data never produce a published file. Protocol 2 is incompatible with the old bidirectional modem.\nText or one regular file may contain up to 1 MiB. Folders, links and archive/remote members are excluded. Compression is used only when smaller. Normal/Fast/Fastest are audible profiles; faster modes need suitable hardware and quiet conditions. Repeats selects 1/2/3 or Until Stop. Estimate computes playback duration without opening audio devices; large files can take hours or days. No speed or reliability promise is made.\nExport WAV streams standard WAV (RF64 for large recordings) without opening audio devices; choose finite repeats. Receive WAV decodes PCM16/24/32 or float32 recordings, 1–8 channels, 8–192 kHz, up to 64 GiB, without playback. A shared file picker chooses recordings and export names. Existing outputs use Replace/Skip/Cancel safeguards.\nReceived data is authenticated and checked before local review. Accept retains text or publishes a file to the chosen local directory; Decline discards it. Preview/Copy/Dismiss remain available for retained text. The sender reports only Transmission finished; it cannot confirm delivery. Stop/close cancels owned work and cleans incomplete staging; completed publications remain. Real speaker/microphone reliability remains unverified without hardware testing.";

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
            dos_status: Default::default(),
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
        p.passphrase = Zeroizing::new("private test passphrase".into());
        let text = "Ёжик\nlink\r\nnext\tcolumn";
        p.event(Event::Paste(text), &ctx);
        assert_eq!(p.text, text);
        let spec = p.controls(&ctx);
        let field = spec.controls.iter().find(|c| c.id == 0).unwrap();
        assert!(
            matches!(&field.kind,ControlKind::ScrolledInput{value,..} if !value.contains(['\n','\r','\t']))
        );
        assert!(
            matches!(p.action(4,&ctx).as_slice(),[Effect::Acoustic{action:crate::acoustic::Action::Start(crate::acoustic::Request{input:crate::acoustic::Input::Text(value),..}),..}] if value==text)
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
    #[test]
    fn passphrases_are_masked_and_estimates_survive_background_generation_changes() {
        let mut ctx = context();
        ctx.acoustic.received = None;
        let mut p = AcousticTransfers::default();
        p.open(&ctx);
        p.prepare(&ctx);
        p.focus = 21;
        p.event(Event::Paste("private fixture passphrase"), &ctx);
        let spec = p.controls(&ctx);
        let field = spec.controls.iter().find(|c| c.id == 21).unwrap();
        assert!(
            matches!(&field.kind,ControlKind::ScrolledInput{value,..}if value.chars().all(|c|c=='*')&&!value.is_empty())
        );
        assert_eq!(p.token().anchor, Some(21));
        drop(spec);
        p.focus = 0;
        p.event(Event::Paste("Ёжик ".repeat(500).as_str()), &ctx);
        assert!(p.text.len() > 1024);
        let request = p.request(crate::acoustic::Operation::Estimate);
        let revision = request.revision;
        ctx.acoustic.generation += 1;
        ctx.acoustic.busy = true;
        p.prepare(&ctx);
        ctx.acoustic.estimate = Some(42);
        ctx.acoustic.plan_revision = revision;
        ctx.acoustic.busy = false;
        p.prepare(&ctx);
        assert_eq!(p.draft_revision, revision);
        p.action(22, &ctx);
        assert_ne!(p.draft_revision, revision);
        p.action(24, &ctx);
        assert!(p.picker.is_some());
        p.event(Event::Action(ActionId(900)), &ctx);
        assert!(p.picker.is_none());
        p.close();
        assert!(p.passphrase.is_empty() && p.text.is_empty());
    }
    #[test]
    fn one_way_requests_capture_mode_repeats_and_do_not_expose_pairing_controls() {
        let mut ctx = context();
        ctx.acoustic.received = None;
        let mut p = AcousticTransfers::default();
        p.open(&ctx);
        p.prepare(&ctx);
        p.passphrase = Zeroizing::new("private test password".into());
        p.text = "a link".into();
        p.cursor = p.text.len();
        assert!(matches!(
            p.action(4, &ctx).as_slice(),
            [Effect::Acoustic {
                action: crate::acoustic::Action::Start(crate::acoustic::Request {
                    operation: crate::acoustic::Operation::Send,
                    input: crate::acoustic::Input::Text(_),
                    ..
                }),
                ..
            }]
        ));
        assert!(matches!(
            p.action(5, &ctx).as_slice(),
            [Effect::Acoustic {
                action: crate::acoustic::Action::Start(crate::acoustic::Request {
                    operation: crate::acoustic::Operation::Receive,
                    input: crate::acoustic::Input::None,
                    ..
                }),
                ..
            }]
        ));
        p.action(23, &ctx);
        p.action(23, &ctx);
        assert_eq!(p.options().repeats, 0);
        let spec = p.controls(&ctx);
        assert!(!spec.controls.iter().any(|c| matches!(c.id, 11 | 12)));
        ctx.acoustic.busy = true;
        let spec = p.controls(&ctx);
        assert!(spec
            .controls
            .iter()
            .filter(|c| matches!(c.id, 4 | 5 | 21 | 22 | 23 | 24 | 25 | 26))
            .all(|c| !c.enabled));
    }
}
