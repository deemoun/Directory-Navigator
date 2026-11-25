//! Presentation and session-only exercises; the host owns microphone capture.
use super::*;
use crate::{
    notes,
    painting::Paragraph,
    turbo::dialog_spec::{centered, Control, ControlKind},
    windowing::geometry::Capabilities,
};
use crossterm::event::{KeyCode as K, MouseButton, MouseEventKind};
#[derive(Default)]
pub(crate) struct NoteTrainer {
    focus: usize,
    practice: bool,
    octave: u8,
    target: u8,
    accidentals: bool,
    challenge: usize,
    correct: u32,
    won: bool,
    needs_release: bool,
    observed: (u64, u64),
    board: u8,
}
impl NoteTrainer {
    fn target(&self) -> u8 {
        if self.target == 0 {
            60
        } else {
            self.target
        }
    }
    fn octave(&self) -> u8 {
        if self.octave == 0 {
            60
        } else {
            self.octave
        }
    }
    fn reset_target(&mut self, midi: u8, current: Option<notes::Reading>) {
        self.target = midi;
        self.won = false;
        self.needs_release = current.is_some_and(|r| r.midi == midi);
    }
    fn advance(&mut self, current: Option<notes::Reading>) {
        const WHITE: [u8; 7] = [0, 4, 7, 2, 5, 9, 11];
        const CHROMATIC: [u8; 12] = [0, 4, 7, 2, 5, 9, 11, 1, 6, 10, 3, 8];
        self.challenge = self.challenge.wrapping_add(1);
        let choices: &[u8] = if self.accidentals { &CHROMATIC } else { &WHITE };
        self.reset_target(
            self.octave() + choices[self.challenge % choices.len()],
            current,
        );
    }
    fn observe(&mut self, reading: Option<notes::Reading>) {
        if !self.practice || self.won {
            return;
        }
        if reading.is_none_or(|r| r.midi != self.target()) {
            self.needs_release = false;
        }
        if !self.needs_release && reading.is_some_and(|r| r.midi == self.target()) {
            self.won = true;
            self.correct = self.correct.saturating_add(1);
        }
    }
    fn action(&mut self, id: usize, ctx: &Context) -> Vec<Effect> {
        match id {
            0 => {
                return vec![Effect::Notes {
                    generation: ctx.notes.generation,
                    action: notes::Action::Listen,
                }]
            }
            1 => {
                return vec![Effect::Notes {
                    generation: ctx.notes.generation,
                    action: notes::Action::Stop,
                }]
            }
            2 => {
                self.practice = !self.practice;
                self.reset_target(self.target(), ctx.notes.reading);
            }
            3 | 4 => {
                self.octave = if id == 3 {
                    self.octave().saturating_sub(12).max(36)
                } else {
                    (self.octave() + 12).min(84)
                };
                self.reset_target(self.octave(), ctx.notes.reading);
            }
            5 => {
                self.accidentals = !self.accidentals;
                self.reset_target(self.octave(), ctx.notes.reading);
            }
            6 => self.advance(ctx.notes.reading),
            7 => {
                return vec![Effect::Help {
                    title: " Note Trainer Help ",
                    text: format!(
                        "{}\n\n{}",
                        ctx.language.text(HELP),
                        crate::filesystem::display_name(&ctx.notes.cause)
                    ),
                }]
            }
            8 => {
                return vec![
                    Effect::Notes {
                        generation: ctx.notes.generation,
                        action: notes::Action::Stop,
                    },
                    Effect::Close,
                ]
            }
            121..=208 => {
                self.practice = true;
                self.reset_target((id - 100) as u8, ctx.notes.reading);
            }
            _ => {}
        }
        vec![]
    }
    fn spec(&self, ctx: &Context) -> DialogSpec<'static> {
        let r = centered(ctx.area, 74, 22);
        let lang = ctx.language;
        let style = ctx
            .palette
            .styles_for(crate::palette::theme::Group::Dialogs);
        let mut s = ctx.dialog(
            r,
            self.focus,
            lang.text("Note Trainer (Experimental)"),
            0,
            8,
        );
        let caption = |s: &mut DialogSpec<'static>, text: String, y, height| {
            s.caption(
                Paragraph::new(text).style(style(0x70)),
                Rect::new(r.x + 2, r.y + y, r.width.saturating_sub(4), height),
            );
        };
        if r.width < 66 || r.height < 22 {
            caption(
                &mut s,
                lang.text("Enlarge the terminal to 80x25 for the piano board.")
                    .into(),
                1,
                r.height.saturating_sub(5),
            );
            for (id, label, x) in [(1, "Stop", r.x + 2), (8, "Close", r.x + r.width / 2)] {
                s.control(Control::new(
                    id,
                    Rect::new(
                        x,
                        r.bottom().saturating_sub(3),
                        r.width.saturating_sub(6) / 2,
                        1,
                    ),
                    ControlKind::Button(lang.text(label).into()),
                    ctx.target(id),
                ));
            }
            return s;
        }
        let heard = ctx.notes.reading.map_or_else(
            || lang.text("Detected: —").into(),
            |reading| {
                format!(
                    "{} {} · {:.1} Hz · {:+.1} {} · {:.0}%",
                    lang.text("Detected:"),
                    notes::name(reading.midi),
                    reading.frequency,
                    reading.cents,
                    lang.text("cents"),
                    reading.confidence * 100.
                )
            },
        );
        caption(&mut s, heard, 1, 1);
        caption(
            &mut s,
            if self.practice {
                format!(
                    "{} {} · {} {}{}",
                    lang.text("Play:"),
                    notes::name(self.target()),
                    lang.text("Correct:"),
                    self.correct,
                    if self.won {
                        format!(" · {}", lang.text("Correct — choose Next note"))
                    } else {
                        String::new()
                    }
                )
            } else {
                lang.text("Live board — play a single note; no audio is saved.")
                    .into()
            },
            2,
            1,
        );
        let labels = [
            lang.text("Listen"),
            lang.text("Stop"),
            lang.text(if self.practice {
                "Mode: Practice"
            } else {
                "Mode: Live"
            }),
            lang.text("Next note"),
        ];
        for ((id, enabled), bounds) in [
            (0, notes::available() && !ctx.notes.listening),
            (1, ctx.notes.listening),
            (2, true),
            (6, self.practice),
        ]
        .into_iter()
        .zip(crate::dialog_theme::button_row(
            Rect::new(r.x + 2, r.y + 4, r.width - 4, 1),
            &labels,
            16,
            1,
        )) {
            let label = match id {
                0 => labels[0],
                1 => labels[1],
                2 => labels[2],
                _ => labels[3],
            };
            let mut c = Control::new(
                id,
                bounds,
                ControlKind::Button(label.into()),
                ctx.target(id),
            );
            c.enabled = enabled;
            s.control(c);
        }
        let base = if self.practice {
            (self.target() / 12 * 12).saturating_sub(12).max(12)
        } else {
            self.board.max(12)
        };
        let board = Rect::new(r.x + (r.width - 56) / 2, r.y + 6, 56, 6);
        for key in keys(board, base) {
            if !(21..=108).contains(&key.midi) {
                continue;
            }
            let active = ctx.notes.reading.is_some_and(|v| v.midi == key.midi);
            let target = self.practice && self.target() == key.midi;
            let attribute = if active {
                0x2f
            } else if target {
                0x3f
            } else if key.black {
                0x07
            } else {
                0x70
            };
            let marker = if active {
                ">"
            } else if target {
                "*"
            } else {
                " "
            };
            let text = if key.black {
                format!("{marker} \n  \n# ")
            } else {
                let name = notes::name(key.midi);
                format!("   |\n   |\n   |\n   |\n   |\n{marker}{name:<2}|")
            };
            s.caption(Paragraph::new(text).style(style(attribute)), key.bounds);
            if (21..=108).contains(&key.midi) {
                s.hit(key.bounds, ctx.target(100 + usize::from(key.midi)));
            }
        }
        caption(
            &mut s,
            lang.text("> Heard · * Target · Click a piano key to set a practice target")
                .into(),
            12,
            1,
        );
        let labels = [
            lang.text("Octave -"),
            lang.text("Octave +"),
            lang.text(if self.accidentals {
                "Sharps: On"
            } else {
                "Sharps: Off"
            }),
        ];
        for ((id, label), bounds) in [(3, labels[0]), (4, labels[1]), (5, labels[2])]
            .into_iter()
            .zip(crate::dialog_theme::button_row(
                Rect::new(r.x + 2, r.y + 13, r.width - 4, 1),
                &labels,
                20,
                1,
            ))
        {
            let mut control = Control::new(
                id,
                bounds,
                ControlKind::Button(label.into()),
                ctx.target(id),
            );
            control.enabled = self.practice;
            s.control(control);
        }
        caption(
            &mut s,
            format!(
                "{} [{}{}] {}%{}",
                lang.text("Microphone level:"),
                "#".repeat(usize::from(ctx.notes.level) / 5),
                " ".repeat(20 - usize::from(ctx.notes.level.min(100)) / 5),
                ctx.notes.level,
                if ctx.notes.clipped {
                    format!(" · {}", lang.text("Clipping"))
                } else {
                    String::new()
                }
            ),
            15,
            1,
        );
        let notice = if !notes::available() {
            lang.text(notes::UNAVAILABLE).into()
        } else if ctx.notes.notice.is_empty() {
            lang.text("Press Listen to open the default microphone.")
                .into()
        } else {
            lang.text(&ctx.notes.notice).into()
        };
        caption(&mut s, notice, 16, 1);
        caption(
            &mut s,
            crate::filesystem::display_name(if ctx.notes.cause.is_empty() {
                &ctx.notes.device
            } else {
                &ctx.notes.cause
            }),
            17,
            1,
        );
        caption(
            &mut s,
            lang.text("Single notes · A4 = 440 Hz · Avoid background audio")
                .into(),
            18,
            1,
        );
        for (i, label, x) in [(7, "Help", r.right() - 32), (8, "Close", r.right() - 17)] {
            s.control(Control::new(
                i,
                Rect::new(x, r.bottom() - 3, 14, 1),
                ControlKind::Button(lang.text(label).into()),
                ctx.target(i),
            ));
        }
        s
    }
}
struct Key {
    midi: u8,
    bounds: Rect,
    black: bool,
}
fn keys(board: Rect, base: u8) -> Vec<Key> {
    let mut white = Vec::new();
    let mut black = Vec::new();
    let mut column = 0;
    for step in 0..24 {
        let midi = base + step;
        let is_black = matches!(midi % 12, 1 | 3 | 6 | 8 | 10);
        if is_black {
            black.push(Key {
                midi,
                bounds: Rect::new(board.x + column * 4 - 1, board.y, 2, 3),
                black: true,
            });
        } else {
            white.push(Key {
                midi,
                bounds: Rect::new(board.x + column * 4, board.y, 4, 6),
                black: false,
            });
            column += 1;
        }
    }
    white.extend(black);
    white
}
impl ToolPlugin for NoteTrainer {
    fn definition(&self) -> Definition {
        Definition::builtin(
            NOTE_TRAINER,
            "Note Trainer (Experimental)",
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
        *self = Self {
            octave: 60,
            target: 60,
            board: 48,
            ..Default::default()
        };
    }
    fn close(&mut self) {
        *self = Self::default();
    }
    fn close_action(&self) -> Option<ActionId> {
        Some(ActionId(8))
    }
    fn windows(&self, ctx: &Context) -> Vec<LocalWindow> {
        vec![LocalWindow {
            id: 0,
            parent: None,
            bounds: centered(ctx.area, 74, 22),
            caps: Capabilities::fixed_zoom(74, 22),
        }]
    }
    fn controls(&self, ctx: &Context) -> DialogSpec<'_> {
        self.spec(ctx)
    }
    fn prepare(&mut self, ctx: &Context) {
        let serial = (ctx.notes.generation, ctx.notes.serial);
        if serial != self.observed {
            self.observed = serial;
            self.observe(ctx.notes.reading);
        }
        if !self.practice {
            if let Some(note) = ctx.notes.reading {
                self.board = (note.midi / 12 * 12).saturating_sub(12).clamp(12, 96);
            }
        }
    }
    fn event(&mut self, event: Event<'_>, ctx: &Context) -> Vec<Effect> {
        match event {
            Event::Focus(id) => {
                self.focus = id;
                vec![]
            }
            Event::Action(id) => self.action(id.0, ctx),
            Event::Mouse(e, Some(id)) if e.kind == MouseEventKind::Down(MouseButton::Left) => {
                self.focus = if id.0 >= 100 { 2 } else { id.0 };
                self.action(id.0, ctx)
            }
            Event::Key(key) if key.code == K::F(1) => self.action(7, ctx),
            _ => vec![],
        }
    }
}
pub(crate) const HELP: &str = "Experimental Note Trainer is a separate optional build: cargo build --release --locked --features note-trainer. Enable it in Options > Configuration > Plugins, then open Utilities > Note Trainer. Normal builds and acoustic transfers are unchanged.\nListen opens the system-default microphone; Stop, closing, disabling the plugin or exiting releases it. Opening/enabling alone never records or opens audio. No audio is saved or transmitted. Other documents remain usable while listening. Use system input/privacy settings to choose the microphone and allow access for DN or its terminal. Linux builds require pkg-config and ALSA development libraries.\nLive shows one detected note, octave, frequency and cents relative to A4=440 Hz (middle C is C4). The two-octave piano follows the note; > marks heard and * marks the practice target. Practice asks for a note: play it steadily, then choose Next note. Octave -/+ changes the exercise octave (C2–C6); Sharps adds black keys. Clicking a piano key sets a target. Each challenge scores once; holding a note does not repeatedly score. Progress lasts only until this window closes.\nPlay one note at a time, not chords. Let the microphone hear the keyboard and pause background music; background sounds and harmonics can produce wrong octaves. Very low/high notes, quiet instruments, sustain and noisy microphones need real-device validation. Green/cyan accents follow DN palettes; markers remain meaningful with custom colors. Detection is experimental, not a certified tuner. If the level stays at zero, check input/gain/privacy. Clipping means lower input gain or move the microphone. Device failure stops listening; Listen retries explicitly. Below 80x25 the global resize notice is shown and capture stops; restore the terminal and choose Listen to resume.";
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn practice_scores_once_and_repeated_targets_require_release() {
        let reading = notes::Reading {
            midi: 60,
            frequency: 261.626,
            cents: 0.,
            confidence: 1.,
        };
        let mut trainer = NoteTrainer {
            practice: true,
            target: 60,
            ..Default::default()
        };
        trainer.observe(Some(reading));
        trainer.observe(Some(reading));
        assert_eq!(trainer.correct, 1);
        trainer.reset_target(60, Some(reading));
        trainer.observe(Some(reading));
        assert_eq!(trainer.correct, 1);
        trainer.observe(None);
        trainer.observe(Some(reading));
        assert_eq!(trainer.correct, 2);
        trainer.reset_target(61, None);
        trainer.observe(Some(reading));
        assert_eq!(trainer.correct, 2);
        trainer.reset_target(60, None);
        trainer.observe(Some(notes::Reading {
            cents: 45.,
            ..reading
        }));
        assert_eq!(trainer.correct, 3); // Learning matches the note, not tuner accuracy.
    }
    #[test]
    fn keyboard_geometry_covers_two_octaves_with_black_keys_on_top() {
        let board = Rect::new(3, 7, 56, 6);
        let keys = keys(board, 48);
        assert_eq!(keys.len(), 24);
        assert_eq!(keys.iter().filter(|k| k.black).count(), 10);
        assert!(keys.iter().all(
            |key| key.bounds.right() <= board.right() && key.bounds.bottom() <= board.bottom()
        ));
        assert!(keys[14..].iter().all(|key| key.black));
    }
}
