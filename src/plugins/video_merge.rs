//! Video Merge presentation; all media I/O is delegated to the host.
use super::*;
use crate::{
    painting::{Frame, NativeFrame, Paragraph, Wrap},
    turbo::{
        dialog_spec::{centered, Control, ControlKind},
        file_dialog::{Mode, Outcome, Picker},
    },
    video::{Action as A, Codec, Preferences, Request, Settings},
    windowing::geometry::Capabilities,
};
use crossterm::event::{KeyCode as K, KeyModifiers as M, MouseButton, MouseEventKind};
const TITLE: &str = "Video Merge";
const LIST: usize = 20;
const PICKER: u64 = 2;
const PAGE: u64 = 1;
#[derive(Default)]
pub struct VideoMerge {
    focus: usize,
    cursor: usize,
    offset: usize,
    sources: Vec<PathBuf>,
    base: PathBuf,
    output: String,
    output_cursor: usize,
    picked_output: Option<PathBuf>,
    recursive: bool,
    csv: bool,
    settings: Settings,
    preferences: Preferences,
    form: Option<Form>,
    confirm: bool,
    busy: bool,
    conflict: bool,
    popup_focus: usize,
    picker: Option<Picker>,
    save_picker: bool,
    revision: u64,
    notice: String,
}
struct Form {
    focus: usize,
    fields: Vec<String>,
    cursors: Vec<usize>,
    processing: usize,
    selecting: bool,
    choice_cursor: usize,
    advanced: bool,
    match_size: bool,
    match_fps: bool,
    preset: usize,
    audio: bool,
    encoders: Vec<String>,
}
impl VideoMerge {
    fn status(&self, ctx: &Context) -> String {
        if !self.notice.is_empty() {
            return localized(ctx, &self.notice);
        }
        if (ctx.video.busy || ctx.video.timing.is_some() || ctx.video.ready)
            && !ctx.video.notice.is_empty()
        {
            return localized(ctx, &ctx.video.notice);
        }
        if ctx.video.checking {
            return ctx.language.text("Checking encoders…").into();
        }
        if !ctx.video.discovery_error.is_empty() {
            return format!("{}\n{}", ctx.language.text("Tool check failed. Install FFmpeg/FFprobe; set paths in Settings > Advanced. F1: details."), localized(ctx, &ctx.video.discovery_error));
        }
        let availability = ctx
            .language
            .text(if ctx.video.capabilities.is_empty() {
                "FFmpeg and FFprobe not checked. Open Settings to check tools."
            } else {
                "Last tool check succeeded. Analyze verifies the selected clips."
            })
            .to_owned();
        if ctx.video.notice.is_empty() {
            availability
        } else {
            format!("{availability}\n{}", localized(ctx, &ctx.video.notice))
        }
    }
    fn effect(a: A, ctx: &Context) -> Vec<Effect> {
        vec![Effect::Video {
            generation: ctx.video.generation,
            action: a,
        }]
    }
    fn invalidate(&mut self, ctx: &Context) -> Vec<Effect> {
        self.notice.clear();
        Self::effect(A::Invalidate, ctx)
    }
    fn output_path(&self) -> PathBuf {
        self.picked_output
            .clone()
            .filter(|p| p.to_string_lossy() == self.output)
            .unwrap_or_else(|| {
                let p = PathBuf::from(&self.output);
                if p.is_absolute() {
                    p
                } else {
                    self.base.join(p)
                }
            })
    }
    fn pick(&mut self, save: bool, ctx: &Context) {
        self.save_picker = save;
        self.picker = Some(
            Picker::new(
                if save { Mode::SaveAs } else { Mode::Open },
                self.base.clone(),
                ctx.language,
                true,
            )
            .with_target(ctx.target(900)),
        );
    }
    fn setup(&mut self, ctx: &Context) {
        let s = &self.settings;
        let fields = vec![
            s.width.map_or(String::new(), |n| n.to_string()),
            s.height.map_or(String::new(), |n| n.to_string()),
            s.fps.clone(),
            s.quality.map_or(String::new(), |n| n.to_string()),
            if s.bitrate_kbps == 0 {
                String::new()
            } else {
                format!("{}", f64::from(s.bitrate_kbps) / 1000.0)
            },
            self.preferences.ffmpeg.clone(),
            self.preferences.ffprobe.clone(),
        ];
        let mut encoders = vec![String::new(), "libx264".into(), "libx265".into()];
        encoders.extend(
            ctx.video
                .capabilities
                .iter()
                .filter(|e| !e.name.starts_with("lib"))
                .map(|e| e.name.clone()),
        );
        self.form = Some(Form {
            focus: 40,
            cursors: fields.iter().map(String::len).collect(),
            fields,
            processing: if !s.hardware.is_empty() {
                encoders.iter().position(|e| e == &s.hardware).unwrap_or(0)
            } else {
                match s.codec {
                    Codec::Auto => 0,
                    Codec::H264 => 1,
                    Codec::H265 => 2,
                }
            },
            selecting: false,
            choice_cursor: 0,
            advanced: false,
            match_size: s.width.is_none(),
            match_fps: s.fps.is_empty(),
            preset: ["fast", "medium", "slow"]
                .iter()
                .position(|p| *p == s.preset)
                .unwrap_or(1),
            audio: s.audio,
            encoders,
        });
    }
    fn action(&mut self, id: usize, ctx: &Context) -> Vec<Effect> {
        if ctx.video.conflict.is_some() {
            return match id {
                70 => Self::effect(
                    A::Conflict(crate::operations::conflict::Decision::Replace),
                    ctx,
                ),
                71 => Self::effect(
                    A::Conflict(crate::operations::conflict::Decision::Cancel),
                    ctx,
                ),
                _ => vec![],
            };
        }
        if self.confirm {
            return match id {
                80 => {
                    self.confirm = false;
                    Self::effect(A::Stop, ctx)
                }
                81 => {
                    self.confirm = false;
                    vec![]
                }
                _ => vec![],
            };
        }
        if self.form.is_some() {
            let f = self.form.as_mut().unwrap();
            match id {
                40 => {
                    f.selecting = true;
                    f.choice_cursor = f.processing;
                    f.focus = 60;
                }
                41 => f.preset = (f.preset + 1) % 3,
                46 => {
                    f.processing = f.choice_cursor;
                    f.selecting = false;
                    f.focus = 40;
                }
                47 => {
                    f.selecting = false;
                    f.focus = 40;
                }
                48 => {
                    f.advanced = false;
                    f.focus = 49;
                }
                49 => {
                    f.advanced = true;
                    f.focus = 33;
                }
                38 => {
                    f.match_size = !f.match_size;
                }
                39 => {
                    f.match_fps = !f.match_fps;
                }
                id if id >= 500 && id < 500 + f.encoders.len() => {
                    f.processing = id - 500;
                    f.selecting = false;
                    f.focus = 40;
                }
                43 => f.audio = !f.audio,
                42 => {
                    return Self::effect(
                        A::CheckTools(Preferences {
                            ffmpeg: f.fields[5].clone(),
                            ffprobe: f.fields[6].clone(),
                        }),
                        ctx,
                    );
                }
                45 => {
                    self.form = None;
                    return Self::effect(A::Discover(self.preferences.clone()), ctx);
                }
                44 => {
                    let parsed = (|| -> std::io::Result<(Settings, Preferences)> {
                        let opt = |i: usize| -> std::io::Result<Option<u32>> {
                            if f.fields[i].trim().is_empty() {
                                Ok(None)
                            } else {
                                f.fields[i]
                                    .parse()
                                    .map(Some)
                                    .map_err(|_| std::io::Error::other("Invalid video number"))
                            }
                        };
                        let s = Settings {
                            codec: match f.processing {
                                0 => Codec::Auto,
                                1 => Codec::H264,
                                2 => Codec::H265,
                                _ => {
                                    if f.encoders[f.processing].starts_with("hevc") {
                                        Codec::H265
                                    } else {
                                        Codec::H264
                                    }
                                }
                            },
                            width: if f.match_size {
                                None
                            } else {
                                Some(opt(0)?.ok_or_else(|| {
                                    std::io::Error::other("Enter custom width and height")
                                })?)
                            },
                            height: if f.match_size {
                                None
                            } else {
                                Some(opt(1)?.ok_or_else(|| {
                                    std::io::Error::other("Enter custom width and height")
                                })?)
                            },
                            fps: if f.match_fps {
                                String::new()
                            } else {
                                if f.fields[2].trim().is_empty() {
                                    return Err(std::io::Error::other("Enter custom FPS"));
                                }
                                f.fields[2].trim().into()
                            },
                            quality: opt(3)?
                                .map(u8::try_from)
                                .transpose()
                                .map_err(|_| std::io::Error::other("Invalid quality"))?,
                            preset: ["fast", "medium", "slow"][f.preset].into(),
                            hardware: if f.processing < 3 {
                                String::new()
                            } else {
                                f.encoders[f.processing].clone()
                            },
                            bitrate_kbps: if f.fields[4].trim().is_empty() {
                                0
                            } else {
                                let mbps = f.fields[4]
                                    .parse::<f64>()
                                    .map_err(|_| std::io::Error::other("Invalid bitrate"))?;
                                if !mbps.is_finite() || !(0.1..=500.0).contains(&mbps) {
                                    return Err(std::io::Error::other(
                                        "Use a bitrate from 0.1 to 500 Mbps",
                                    ));
                                }
                                (mbps * 1000.0).round() as u32
                            },
                            audio: f.audio,
                        };
                        s.validate()?;
                        if f.processing > 0 && !ctx.video.discovery_error.is_empty() {
                            return Err(std::io::Error::other(ctx.video.discovery_error.clone()));
                        }
                        if f.processing > 0 {
                            if let Some(e) = ctx
                                .video
                                .capabilities
                                .iter()
                                .find(|e| e.name == f.encoders[f.processing] && !e.available())
                            {
                                return Err(std::io::Error::other(format!(
                                    "{}: {}",
                                    ctx.language.text("Selected encoder is unavailable"),
                                    e.reason
                                )));
                            }
                        }
                        let p = Preferences {
                            ffmpeg: f.fields[5].trim().into(),
                            ffprobe: f.fields[6].trim().into(),
                        };
                        if [&p.ffmpeg, &p.ffprobe]
                            .iter()
                            .any(|p| p.len() > 4096 || p.chars().any(char::is_control))
                        {
                            return Err(std::io::Error::other("Invalid executable path"));
                        }
                        Ok((s, p))
                    })();
                    match parsed {
                        Ok((s, p)) => {
                            self.settings = s;
                            self.preferences = p.clone();
                            self.form = None;
                            self.notice.clear();
                            return vec![
                                Effect::Video {
                                    generation: ctx.video.generation,
                                    action: A::Preferences(p),
                                },
                                Effect::Video {
                                    generation: ctx.video.generation,
                                    action: A::Invalidate,
                                },
                            ];
                        }
                        Err(e) => self.notice = e.to_string(),
                    }
                }
                _ => (),
            }
            return vec![];
        }
        if id >= 100 && id < 100 + self.sources.len() {
            self.cursor = id - 100;
            self.focus = LIST;
            return vec![];
        }
        match id {
            0 => {
                if ctx.video.busy {
                    self.confirm = true;
                    self.popup_focus = 81;
                } else {
                    return vec![Effect::Close];
                }
            }
            1 if !ctx.video.busy => self.pick(false, ctx),
            2 if !ctx.video.busy => return Self::effect(A::Tree, ctx),
            3 if !ctx.video.busy => {
                if self.cursor < self.sources.len() {
                    self.sources.remove(self.cursor);
                    self.cursor = self.cursor.min(self.sources.len().saturating_sub(1));
                    return self.invalidate(ctx);
                }
            }
            4 | 5 if !ctx.video.busy => {
                let next = if id == 4 {
                    self.cursor.saturating_sub(1)
                } else {
                    (self.cursor + 1).min(self.sources.len().saturating_sub(1))
                };
                if self.cursor < self.sources.len() {
                    self.sources.swap(self.cursor, next);
                    self.cursor = next;
                    return self.invalidate(ctx);
                }
            }
            6 if !ctx.video.busy => self.pick(true, ctx),
            7 if !ctx.video.busy => {
                self.recursive = !self.recursive;
                return self.invalidate(ctx);
            }
            8 if !ctx.video.busy => {
                self.csv = !self.csv;
                return self.invalidate(ctx);
            }
            9 if !ctx.video.busy => {
                self.setup(ctx);
                return Self::effect(A::Discover(self.preferences.clone()), ctx);
            }
            10 if !ctx.video.busy => {
                return Self::effect(
                    A::Analyze(Request {
                        sources: self.sources.clone(),
                        output: self.output_path(),
                        recursive: self.recursive,
                        csv: self.csv,
                        settings: self.settings.clone(),
                    }),
                    ctx,
                )
            }
            11 if ctx.video.ready && !ctx.video.busy => return Self::effect(A::Start, ctx),
            12 => return Self::effect(A::Stop, ctx),
            14 if ctx.video.ready && !ctx.video.busy => return Self::effect(A::TestSpeed, ctx),
            13 => {
                return vec![Effect::Help {
                    title: " Video Merge Help ",
                    text: format!(
                        "{}\n\n{}\n\n{}",
                        ctx.language.text(HELP),
                        safe_block(&localized(ctx, &ctx.video.notice)),
                        safe_block(&localized(ctx, &ctx.video.discovery_error)),
                    ),
                }]
            }
            _ => (),
        }
        vec![]
    }
    fn main_spec(&self, ctx: &Context) -> DialogSpec<'static> {
        let r = centered(ctx.area, 78, 23);
        let mut s = ctx.dialog(r, self.focus, ctx.language.text(TITLE), 10, 0);
        s.caption(
            Paragraph::new(ctx.language.text("Clips in merge order")),
            Rect::new(r.x + 2, r.y + 1, 73, 1),
        );
        let mut list = Control::new(
            LIST,
            Rect::new(r.x + 2, r.y + 2, 73, 5),
            ControlKind::List {
                items: self
                    .sources
                    .iter()
                    .map(|p| {
                        let name = crate::video::display(
                            PathBuf::from(p.file_name().unwrap_or_default()).as_path(),
                        );
                        let name_width =
                            crate::presentation::text::Span::raw(&name).width().min(55) as u16;
                        let name = crate::presentation::text::ellipsize_tail(&name, name_width);
                        let parent = crate::presentation::text::ellipsize_tail(
                            &crate::video::display(p.parent().unwrap_or(p)),
                            71u16.saturating_sub(name_width + 3),
                        );
                        format!("{name} — {parent}")
                    })
                    .collect(),
                cursor: self.cursor,
                offset: self.offset,
            },
            ctx.target(LIST),
        );
        list.dn_list = true;
        list.enabled = !ctx.video.busy;
        list.hits = (0..5)
            .filter(|i| self.offset + i < self.sources.len())
            .map(|i| {
                (
                    Rect::new(r.x + 2, r.y + 2 + i as u16, 73, 1),
                    ctx.target(100 + self.offset + i),
                )
            })
            .collect();
        s.control(list);
        let source_buttons = [
            (1, "Add file"),
            (2, "Add folder"),
            (3, "Remove"),
            (4, "Move up"),
            (5, "Move down"),
        ];
        let labels = source_buttons.map(|(_, label)| ctx.language.text(label));
        let bounds =
            crate::dialog_theme::button_row(Rect::new(r.x + 2, r.y + 8, 73, 1), &labels, 11, 1);
        for ((id, label), bounds) in source_buttons.into_iter().zip(bounds) {
            button(&mut s, ctx, id, bounds, label, !ctx.video.busy);
        }
        s.caption(
            Paragraph::new(ctx.language.text("Output MP4")),
            Rect::new(r.x + 2, r.y + 10, 14, 1),
        );
        let mut input = Control::new(
            21,
            Rect::new(r.x + 16, r.y + 10, 45, 1),
            ControlKind::ScrolledInput {
                value: self.output.clone(),
                scroll: crate::presentation::text::Line::from(
                    &self.output[..self.output_cursor.min(self.output.len())],
                )
                .width()
                .saturating_sub(44),
            },
            ctx.target(21),
        );
        input.enabled = !ctx.video.busy;
        s.control(input);
        if self.focus == 21 && !ctx.video.busy {
            let col = crate::presentation::text::Line::from(
                &self.output[..self.output_cursor.min(self.output.len())],
            )
            .width();
            s.cursor = Some((r.x + 16 + col.min(44) as u16, r.y + 10));
        }
        button(
            &mut s,
            ctx,
            6,
            Rect::new(r.x + 63, r.y + 10, 12, 1),
            "Browse…",
            !ctx.video.busy,
        );
        for (id, label, checked, x, w) in [
            (7, "Include subfolders", self.recursive, 2, 24),
            (8, "Export timeline CSV", self.csv, 28, 25),
        ] {
            let mut c = Control::new(
                id,
                Rect::new(r.x + x, r.y + 12, w, 1),
                ControlKind::Checkbox {
                    caption: ctx.language.text(label).into(),
                    checked,
                },
                ctx.target(id),
            );
            c.enabled = !ctx.video.busy;
            s.control(c);
        }
        s.caption(
            Paragraph::new(localized(ctx, &ctx.video.summary)).wrap(Wrap { trim: false }),
            Rect::new(r.x + 2, r.y + 14, 73, 2),
        );
        button(
            &mut s,
            ctx,
            14,
            Rect::new(r.x + 54, r.y + 12, 21, 1),
            "Test speed",
            ctx.video.ready && !ctx.video.busy,
        );
        let notice = self.status(ctx);
        let (notice_y, notice_h) = if let Some(t) = &ctx.video.timing {
            let status = if !t.outcome.is_empty() {
                format!(
                    "{} {}",
                    ctx.language.text(t.outcome),
                    crate::video::time(t.elapsed)
                )
            } else if t.waiting {
                ctx.language.text("Waiting for confirmation").into()
            } else if t.clips > 0 {
                format!("{} {}/{}", ctx.language.text(t.stage), t.clip, t.clips)
            } else {
                ctx.language
                    .text(if t.stage.is_empty() {
                        "Preparing video job"
                    } else {
                        t.stage
                    })
                    .into()
            };
            s.caption(
                Paragraph::new(status.clone()),
                Rect::new(r.x + 2, r.y + 16, 73, 1),
            );
            if t.outcome.is_empty() {
                let pct = if t.total > 0.0 {
                    100.0 * t.processed / t.total
                } else {
                    0.0
                };
                s.caption(
                    Paragraph::new(format!(
                        "{} · {pct:.1}% · {} / {}",
                        status,
                        crate::video::media_time(t.processed),
                        crate::video::media_time(t.total)
                    )),
                    Rect::new(r.x + 2, r.y + 16, 73, 1),
                );
                let eta = t.remaining.map_or_else(
                    || {
                        ctx.language
                            .text(if t.stage == "Converting" {
                                "Calculating…"
                            } else {
                                "Estimating…"
                            })
                            .to_owned()
                    },
                    |d| format!("~{}", crate::video::time(d)),
                );
                s.caption(
                    Paragraph::new(format!(
                        "{} {} · {} {eta}",
                        ctx.language.text("Elapsed:"),
                        crate::video::time(t.elapsed),
                        ctx.language.text(if t.stage == "Converting" {
                            "Conversion remaining:"
                        } else {
                            "Remaining:"
                        })
                    )),
                    Rect::new(r.x + 2, r.y + 17, 73, 1),
                );
                (18, 2)
            } else {
                (17, 3)
            }
        } else {
            (16, 4)
        };
        s.caption(
            Paragraph::new(safe_block(&notice)).wrap(Wrap { trim: false }),
            Rect::new(r.x + 2, r.y + notice_y, 73, notice_h),
        );
        let actions = [
            (9, "Settings…", !ctx.video.busy),
            (10, "Analyze", !ctx.video.busy && !self.sources.is_empty()),
            (11, "Start", ctx.video.ready && !ctx.video.busy),
            (12, "Stop", ctx.video.busy),
            (13, "Help", true),
            (0, "Close", true),
        ];
        let labels = actions.map(|(_, label, _)| ctx.language.text(label));
        let bounds =
            crate::dialog_theme::button_row(Rect::new(r.x + 2, r.y + 20, 73, 1), &labels, 10, 1);
        for ((id, label, enabled), bounds) in actions.into_iter().zip(bounds) {
            button(&mut s, ctx, id, bounds, label, enabled);
        }
        s
    }
    fn page_spec(&self, ctx: &Context) -> DialogSpec<'static> {
        let r = centered(ctx.area, 76, 23);
        if let Some(conflict) = &ctx.video.conflict {
            let mut s = ctx.dialog(
                r,
                self.popup_focus,
                ctx.language.text("Replace output?"),
                71,
                71,
            );
            let text = format!(
                "{}\n{}\n{}: {}\n{}",
                ctx.language
                    .text("An output file already exists. Replace only this file?"),
                crate::video::display(&conflict.destination),
                ctx.language.text("Size"),
                conflict.destination_size,
                ctx.language
                    .text("Existing data remains until the completed output is published.")
            );
            s.caption(
                Paragraph::new(text).wrap(Wrap { trim: false }),
                Rect::new(r.x + 3, r.y + 3, 69, 12),
            );
            button(
                &mut s,
                ctx,
                70,
                Rect::new(r.x + 9, r.y + 18, 23, 1),
                "Replace",
                true,
            );
            button(
                &mut s,
                ctx,
                71,
                Rect::new(r.x + 42, r.y + 18, 23, 1),
                "Cancel",
                true,
            );
            return s;
        }
        if self.confirm {
            let mut s = ctx.dialog(
                r,
                self.popup_focus,
                ctx.language.text("Stop video job?"),
                81,
                81,
            );
            s.caption(Paragraph::new(ctx.language.text("Stop the active job? Temporary work will be removed. Close again after cancellation finishes.")).wrap(Wrap{trim:false}),Rect::new(r.x+3,r.y+4,69,5));
            button(
                &mut s,
                ctx,
                80,
                Rect::new(r.x + 9, r.y + 18, 23, 1),
                "Stop",
                true,
            );
            button(
                &mut s,
                ctx,
                81,
                Rect::new(r.x + 42, r.y + 18, 23, 1),
                "Cancel",
                true,
            );
            return s;
        }
        let f = self.form.as_ref().unwrap();
        let mut s = ctx.dialog(
            r,
            f.focus,
            ctx.language.text("Video Merge settings"),
            44,
            45,
        );
        if f.selecting {
            s.default = ctx.target(46);
            s.cancel = ctx.target(47);
            // A native list plus explicit selection, not a cycling button.
            let offset = f.choice_cursor.saturating_sub(4);
            let items = f
                .encoders
                .iter()
                .map(|e| {
                    let name = processing_name(e, ctx);
                    if ctx
                        .video
                        .capabilities
                        .iter()
                        .any(|c| c.name == *e && !c.available())
                    {
                        format!("{name} — {}", ctx.language.text("Unavailable"))
                    } else {
                        name
                    }
                })
                .collect();
            let mut c = Control::new(
                60,
                Rect::new(r.x + 3, r.y + 3, 69, 6),
                ControlKind::List {
                    items,
                    cursor: f.choice_cursor,
                    offset,
                },
                ctx.target(60),
            );
            c.dn_list = true;
            c.hits = (0..6)
                .filter_map(|row| {
                    let i = offset + row;
                    (i < f.encoders.len()).then(|| {
                        (
                            Rect::new(r.x + 3, r.y + 3 + row as u16, 69, 1),
                            ctx.target(500 + i),
                        )
                    })
                })
                .collect();
            s.control(c);
            let explanation = ctx
                .video
                .capabilities
                .iter()
                .find(|e| e.name == f.encoders[f.choice_cursor])
                .map_or_else(
                    || ctx.video.discovery_error.clone(),
                    |e| {
                        if e.available() {
                            ctx.language.text("Available").into()
                        } else {
                            ctx.language.text(&e.reason).to_owned()
                        }
                    },
                );
            s.caption(
                Paragraph::new(safe_block(&explanation)).wrap(Wrap { trim: false }),
                Rect::new(r.x + 3, r.y + 10, 69, 6),
            );
            button(
                &mut s,
                ctx,
                46,
                Rect::new(r.x + 10, r.y + 20, 22, 1),
                "Select",
                true,
            );
            button(
                &mut s,
                ctx,
                47,
                Rect::new(r.x + 43, r.y + 20, 22, 1),
                "Cancel",
                true,
            );
            return s;
        }
        let software = f.processing == 1 || f.processing == 2;
        let hardware = f.processing >= 3;
        let fields = if f.advanced {
            vec![
                (
                    3,
                    "Software quality (CRF; lower is better)",
                    3,
                    software || f.processing == 0,
                ),
                (
                    4,
                    "Hardware bitrate (Mbps; blank: Auto)",
                    8,
                    hardware || f.processing == 0,
                ),
                (5, "FFmpeg path (blank: PATH)", 12, true),
                (6, "FFprobe path (blank: PATH)", 15, true),
            ]
        } else {
            vec![
                (0, "Width", 6, !f.match_size),
                (1, "Height", 8, !f.match_size),
                (2, "Frames per second", 12, !f.match_fps),
            ]
        };
        for (i, label, y, enabled) in fields {
            if f.advanced && !enabled {
                continue;
            }
            let x = 2;
            let w = 70;
            s.caption(
                Paragraph::new(ctx.language.text(label)),
                Rect::new(r.x + x, r.y + y - 1, w, 1),
            );
            let col = crate::presentation::text::Line::from(&f.fields[i][..f.cursors[i]]).width();
            let mut c = Control::new(
                30 + i,
                Rect::new(r.x + x, r.y + y, w, 1),
                ControlKind::ScrolledInput {
                    value: f.fields[i].clone(),
                    scroll: col.saturating_sub(usize::from(w - 1)),
                },
                ctx.target(30 + i),
            );
            c.enabled = enabled;
            s.control(c);
            if f.focus == 30 + i && enabled {
                s.cursor = Some((r.x + x + col.min(usize::from(w - 1)) as u16, r.y + y));
            }
        }
        if f.advanced {
            s.caption(
                Paragraph::new(ctx.language.text("Default CRF: H.264 18 / H.265 20")),
                Rect::new(r.x + 2, r.y + 1, 70, 1),
            );
            if !hardware {
                button(
                    &mut s,
                    ctx,
                    41,
                    Rect::new(r.x + 2, r.y + 5, 70, 1),
                    [
                        "Software speed: Fast",
                        "Software speed: Medium",
                        "Software speed: Slow",
                    ][f.preset],
                    true,
                );
            }
            button(
                &mut s,
                ctx,
                42,
                Rect::new(r.x + 39, r.y + 16, 33, 1),
                "Check tools",
                !ctx.video.checking,
            );
            button(
                &mut s,
                ctx,
                48,
                Rect::new(r.x + 2, r.y + 16, 30, 1),
                "Back to basic settings",
                true,
            );
        } else {
            button(
                &mut s,
                ctx,
                40,
                Rect::new(r.x + 2, r.y + 2, 70, 1),
                &format!(
                    "{} {}",
                    ctx.language.text("Processing:"),
                    processing_name(&f.encoders[f.processing], ctx)
                ),
                true,
            );
            for (id, label, checked, y) in [
                (38, "Match source resolution", f.match_size, 4),
                (39, "Match source frame rate", f.match_fps, 10),
                (43, "Keep audio", f.audio, 14),
            ] {
                s.control(Control::new(
                    id,
                    Rect::new(r.x + 2, r.y + y, 70, 1),
                    ControlKind::Checkbox {
                        caption: ctx.language.text(label).into(),
                        checked,
                    },
                    ctx.target(id),
                ));
            }
            button(
                &mut s,
                ctx,
                49,
                Rect::new(r.x + 2, r.y + 16, 30, 1),
                "Advanced…",
                true,
            );
        }
        let status = if ctx.video.checking {
            ctx.language.text("Checking encoders…").to_owned()
        } else if !ctx.video.discovery_error.is_empty() {
            ctx.language
                .text("Tool check failed. Install FFmpeg/FFprobe. F1: details.")
                .into()
        } else if ctx.video.capabilities.is_empty() {
            ctx.language
                .text("Not checked. Advanced > Check tools verifies the paths.")
                .into()
        } else {
            ctx.language
                .text("Last tool check succeeded; Check tools verifies current paths.")
                .into()
        };
        s.caption(
            Paragraph::new(status).wrap(Wrap { trim: false }),
            Rect::new(r.x + 2, r.y + 18, 70, 1),
        );
        if !self.notice.is_empty() {
            s.caption(
                Paragraph::new(localized(ctx, &self.notice)),
                Rect::new(r.x + 2, r.y + 19, 70, 1),
            );
        } else if !ctx.video.discovery_error.is_empty() {
            s.caption(
                Paragraph::new(
                    ctx.language
                        .text("Set executable paths in Advanced, then Check tools."),
                ),
                Rect::new(r.x + 2, r.y + 19, 70, 1),
            );
        }
        button(
            &mut s,
            ctx,
            44,
            Rect::new(r.x + 10, r.y + 20, 22, 1),
            "OK",
            true,
        );
        button(
            &mut s,
            ctx,
            45,
            Rect::new(r.x + 43, r.y + 20, 22, 1),
            "Cancel",
            true,
        );
        s
    }
}
fn processing_name(encoder: &str, ctx: &Context) -> String {
    ctx.language
        .text(if encoder.is_empty() {
            "Automatic — recommended"
        } else {
            crate::video::encoder_name(encoder)
        })
        .into()
}
fn safe_block(text: &str) -> String {
    text.split('\n')
        .map(crate::filesystem::display_name)
        .collect::<Vec<_>>()
        .join("\n")
}
fn localized(ctx: &Context, text: &str) -> String {
    ctx.language.text(text).into()
}
fn button(
    s: &mut DialogSpec<'static>,
    ctx: &Context,
    id: usize,
    r: Rect,
    label: &str,
    enabled: bool,
) {
    let mut c = Control::new(
        id,
        r,
        ControlKind::Button(ctx.language.text(label).into()),
        ctx.target(id),
    );
    c.enabled = enabled;
    s.control(c);
}
impl ToolPlugin for VideoMerge {
    fn close_action(&self) -> Option<ActionId> {
        Some(ActionId(0))
    }
    fn definition(&self) -> Definition {
        Definition::builtin(VIDEO_MERGE, TITLE, Presentation::Document)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn open(&mut self, ctx: &Context) {
        *self = Self {
            preferences: ctx.video_preferences.clone(),
            ..Default::default()
        };
    }
    fn uses_controls(&self, key: KeyEvent) -> bool {
        if self.form.as_ref().is_some_and(|f| f.selecting)
            && matches!(
                key.code,
                K::Up | K::Down | K::Home | K::End | K::PageUp | K::PageDown
            )
        {
            return false;
        }
        let main = self.form.is_none() && !self.confirm && !self.conflict;
        let input = self
            .form
            .as_ref()
            .map_or(main && !self.busy && self.focus == 21, |f| {
                (30..37).contains(&f.focus)
            });
        self.picker.is_none()
            && !(main
                && !self.busy
                && self.focus == LIST
                && matches!(
                    key.code,
                    K::Up | K::Down | K::PageUp | K::PageDown | K::Home | K::End
                ))
            && !(key.code == K::Char(' ') && input)
    }
    fn event(&mut self, event: Event<'_>, ctx: &Context) -> Vec<Effect> {
        if let Some(p) = &mut self.picker {
            let outcome = match event {
                Event::Key(k) => p.key(k),
                Event::Paste(t) => {
                    p.paste(t);
                    None
                }
                Event::Mouse(m, _) => p.mouse(m, (0, 0)),
                _ => None,
            };
            if let Some(outcome) = outcome {
                self.picker = None;
                if let Outcome::Selected(_, p) = outcome {
                    if self.save_picker {
                        self.output = p.to_string_lossy().into();
                        self.output_cursor = self.output.len();
                        self.picked_output = Some(p);
                    } else {
                        self.sources.push(p);
                    }
                    return self.invalidate(ctx);
                }
            }
            return vec![];
        }
        match event {
            Event::VideoSources { sources, base } => {
                if !ctx.video.busy {
                    self.base = base;
                    if self.output.is_empty() {
                        let p = self.base.join("merged_video.mp4");
                        self.output = p.to_string_lossy().into();
                        self.output_cursor = self.output.len();
                        self.picked_output = Some(p);
                    }
                    let mut known = self
                        .sources
                        .iter()
                        .cloned()
                        .collect::<std::collections::HashSet<_>>();
                    for path in sources {
                        if known.insert(path.clone()) {
                            self.sources.push(path);
                        }
                    }
                    return self.invalidate(ctx);
                }
            }
            Event::VideoFolder(path) => {
                if !ctx.video.busy {
                    self.sources.push(path);
                    return self.invalidate(ctx);
                }
            }
            Event::Focus(id) => {
                if self.confirm || ctx.video.conflict.is_some() {
                    self.popup_focus = id;
                } else if let Some(f) = &mut self.form {
                    f.focus = id;
                } else {
                    self.focus = id;
                }
            }
            Event::Action(ActionId(id)) => return self.action(id, ctx),
            Event::Mouse(m, Some(id)) if m.kind == MouseEventKind::Down(MouseButton::Left) => {
                if self.confirm || ctx.video.conflict.is_some() {
                    self.popup_focus = id.0;
                } else if let Some(f) = &mut self.form {
                    f.focus = id.0;
                } else {
                    self.focus = if id.0 >= 100 { LIST } else { id.0 };
                }
                if !(30..37).contains(&id.0) && id.0 != 21 {
                    return self.action(id.0, ctx);
                }
            }
            Event::Mouse(m, _)
                if self.form.as_ref().is_some_and(|f| f.selecting)
                    && matches!(
                        m.kind,
                        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                    ) =>
            {
                if self
                    .page_spec(ctx)
                    .controls
                    .iter()
                    .any(|c| c.id == 60 && c.bounds.contains((m.column, m.row).into()))
                {
                    let f = self.form.as_mut().unwrap();
                    f.choice_cursor = if m.kind == MouseEventKind::ScrollUp {
                        f.choice_cursor.saturating_sub(1)
                    } else {
                        (f.choice_cursor + 1).min(f.encoders.len() - 1)
                    };
                }
            }
            Event::Mouse(m, _)
                if !ctx.video.busy
                    && self.form.is_none()
                    && !self.confirm
                    && matches!(
                        m.kind,
                        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                    ) =>
            {
                if self
                    .main_spec(ctx)
                    .controls
                    .iter()
                    .any(|c| c.id == LIST && c.bounds.contains((m.column, m.row).into()))
                {
                    self.focus = LIST;
                    self.cursor = if m.kind == MouseEventKind::ScrollUp {
                        self.cursor.saturating_sub(1)
                    } else {
                        (self.cursor + 1).min(self.sources.len().saturating_sub(1))
                    };
                }
            }
            Event::Key(k) if k.code == K::Esc => {
                return self.action(
                    if ctx.video.conflict.is_some() {
                        71
                    } else if self.confirm {
                        81
                    } else if self.form.as_ref().is_some_and(|f| f.selecting) {
                        47
                    } else if self.form.is_some() {
                        45
                    } else {
                        0
                    },
                    ctx,
                )
            }
            Event::Key(k) if k.code == K::F(1) => return self.action(13, ctx),
            Event::Key(k) if !ctx.video.busy && !k.modifiers.intersects(M::ALT | M::CONTROL) => {
                if let Some(f) = &mut self.form {
                    if f.selecting {
                        let max = f.encoders.len() - 1;
                        f.choice_cursor = match k.code {
                            K::Up => f.choice_cursor.saturating_sub(1),
                            K::Down => (f.choice_cursor + 1).min(max),
                            K::Home => 0,
                            K::End => max,
                            K::PageUp => f.choice_cursor.saturating_sub(5),
                            K::PageDown => (f.choice_cursor + 5).min(max),
                            _ => f.choice_cursor,
                        };
                    } else if (30..37).contains(&f.focus) {
                        let i = f.focus - 30;
                        if f.fields[i].len() < 4096 || !matches!(k.code, K::Char(_)) {
                            crate::dialog::edit_input(&mut f.fields[i], &mut f.cursors[i], k.code);
                        }
                    }
                } else if self.focus == 21 {
                    if self.output.len() < 4096 || !matches!(k.code, K::Char(_)) {
                        crate::dialog::edit_input(
                            &mut self.output,
                            &mut self.output_cursor,
                            k.code,
                        );
                        return self.invalidate(ctx);
                    }
                } else if self.focus == LIST {
                    let max = self.sources.len().saturating_sub(1);
                    self.cursor = match k.code {
                        K::Up => self.cursor.saturating_sub(1),
                        K::Down => (self.cursor + 1).min(max),
                        K::PageUp => self.cursor.saturating_sub(5),
                        K::PageDown => (self.cursor + 5).min(max),
                        K::Home => 0,
                        K::End => max,
                        _ => self.cursor,
                    };
                }
            }
            Event::Paste(t) if !ctx.video.busy => {
                if let Some(f) = &mut self.form {
                    if (30..37).contains(&f.focus) {
                        let i = f.focus - 30;
                        if f.fields[i].len() + t.len() <= 4096 {
                            crate::dialog::paste_input(&mut f.fields[i], &mut f.cursors[i], t);
                        }
                    }
                } else if self.focus == 21 && self.output.len() + t.len() <= 4096 {
                    crate::dialog::paste_input(&mut self.output, &mut self.output_cursor, t);
                    return self.invalidate(ctx);
                }
            }
            _ => (),
        }
        vec![]
    }
    fn prepare(&mut self, ctx: &Context) {
        if let Some(f) = &mut self.form {
            let selected = f.encoders[f.processing].clone();
            let highlighted = f.encoders[f.choice_cursor.min(f.encoders.len() - 1)].clone();
            let mut names = vec![String::new(), "libx264".into(), "libx265".into()];
            names.extend(
                ctx.video
                    .capabilities
                    .iter()
                    .filter(|e| !e.name.starts_with("lib"))
                    .map(|e| e.name.clone()),
            );
            // A failed refresh must not silently turn an explicit encoder into Auto.
            if !selected.is_empty() && !names.contains(&selected) {
                names.push(selected.clone());
            }
            if names != f.encoders {
                f.processing = names.iter().position(|e| e == &selected).unwrap_or(0);
                f.choice_cursor = names
                    .iter()
                    .position(|e| e == &highlighted)
                    .unwrap_or(f.processing);
                f.encoders = names;
            }
        }
        if self.revision != ctx.video.revision {
            self.revision = ctx.video.revision;
            self.sources = ctx.video.sources.clone();
            self.cursor = self.cursor.min(self.sources.len().saturating_sub(1));
        }

        if ctx.video.conflict.is_some() && !self.conflict {
            self.popup_focus = 71;
        }
        self.conflict = ctx.video.conflict.is_some();
        if ctx.video.busy != self.busy {
            if ctx.video.busy && !matches!(self.focus, 0 | 12 | 13) {
                self.focus = 12;
            } else if !ctx.video.busy && self.focus == 12 {
                self.focus = if ctx.video.ready { 11 } else { 0 };
            }
        }
        self.busy = ctx.video.busy;

        if let Some(p) = &mut self.picker {
            p.prepare(
                crate::presentation::layout::Size::new(ctx.area.width, ctx.area.height),
                ctx.palette,
            );
        }
        crate::list_state::ListViewport::fit(
            &mut self.cursor,
            &mut self.offset,
            5,
            Some(self.sources.len().saturating_sub(1)),
            None,
        );
    }
    fn native_input_page(&self) -> Option<u64> {
        self.picker.as_ref().map(|_| PICKER)
    }
    fn controls(&self, ctx: &Context) -> DialogSpec<'_> {
        if let Some(p) = &self.picker {
            let mut s = ctx.dialog(p.bounds(), 0, ctx.language.text("Open file"), 900, 900);
            let mut c = Control::new(900, s.bounds, ControlKind::Surface, ctx.target(900));
            c.paint = false;
            s.control(c);
            s
        } else if self.form.is_some() || self.confirm || ctx.video.conflict.is_some() {
            self.page_spec(ctx)
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
        if self.form.is_some() || self.confirm || ctx.video.conflict.is_some() {
            let r = self.page_spec(ctx).bounds;
            out.push(LocalWindow {
                id: PAGE,
                parent: Some(0),
                bounds: r,
                caps: Capabilities::fixed(r.width, r.height),
            });
        }
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
        let page = self.form.is_some() || self.confirm || ctx.video.conflict.is_some();
        let s = self.main_spec(ctx);
        let mut f = NativeFrame::new(ctx.area, ctx.palette);
        f.render_widget(
            crate::turbo::widgets::dialog(ctx.language.text(TITLE)),
            s.bounds,
        );
        let mut layers = vec![Layer {
            id: 0,
            interactive: !page && self.picker.is_none(),
            scene: f.finish(),
            controls: Some(s),
        }];
        if page {
            let s = self.page_spec(ctx);
            let mut f = NativeFrame::new(ctx.area, ctx.palette);
            f.render_widget(
                crate::turbo::widgets::dialog(ctx.language.text("Video Merge")),
                s.bounds,
            );
            layers.push(Layer {
                id: PAGE,
                interactive: true,
                scene: f.finish(),
                controls: Some(s),
            });
        }
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
pub(crate) const HELP:&str="Video Merge uses separately installed FFmpeg and FFprobe. Enable this plugin in Options > Configuration > Plugins. Select regular files/folders and choose File > Merge videos, or add sources here. Folder scanning is nonrecursive unless Include subfolders is checked. Links are never followed.\nAnalyze scans, hashes, excludes exact duplicates and probes clips. Review the displayed order and conversion summary before Start. Move up/down changes the order. Auto joins compatible MP4 streams without re-encoding; otherwise the first clip proposes the dimensions and exact FPS. Explicit codecs or changed dimensions/FPS request conversion. HDR/10-bit converts automatically to SDR (BT.709) when conversion is needed; compatible stream copy retains HDR. Software H.264 uses CRF 18; H.265 uses CRF 20. Hardware encoders use the displayed bitrate and may be unavailable on the current device. Failed clips stop the job.\nOutput is MP4. FFmpeg writes private staging files; existing outputs require confirmation and remain until publication. Stop removes owned temporary work; completed output remains. Timeline CSV is optional and includes only clip names, order and times, never GPS or full paths. Jobs have no persistent resume cache.\nSettings uses a Processing selector and Match source controls. Advanced contains CRF, software speed, hardware bitrate in Mbps and tool paths. Encoder discovery starts when Settings opens; Automatic prefers verified hardware when conversion is needed. Test speed compares a private sample without publishing it. Elapsed time includes the whole job; remaining time estimates conversion only, with joining and validation shown separately. Completion retains total elapsed time. Settings OK applies executable overrides through normal configuration saving; Cancel discards. Blank paths discover tools on PATH. No shell or automatic installation is used. Scans are limited to 10,000 clips, 100,000 entries and 512 levels; process captures to 8 MiB and processes to 24 hours. Missing tools are reported. Automatic can select software before Start if hardware is unavailable; explicit encoders never silently switch. Disable audio to produce silent output.";
