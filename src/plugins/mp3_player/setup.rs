//! Owned draft page. Only the host commits saved preferences.
use super::*;
pub(super) const PAGE: u64 = 6;
const STEP: usize = 600;
const DOWN: usize = 604;
const UP: usize = 605;
const OK: usize = 606;
const CANCEL: usize = 607;
const STEPS: [u8; 4] = [1, 5, 10, 30];
impl Mp3Player {
    pub(super) fn setup_action(&mut self, id: usize) -> Vec<Effect> {
        let Some(draft) = &mut self.setup_draft else {
            return Vec::new();
        };
        match id {
            STEP..=603 => {
                draft.seek_seconds = STEPS[id - STEP];
                self.setup_focus = id - STEP;
            }
            DOWN => {
                draft.default_volume = draft.default_volume.saturating_sub(10);
                self.setup_focus = 4;
            }
            UP => {
                draft.default_volume = draft.default_volume.saturating_add(10).min(100);
                self.setup_focus = 5;
            }
            OK => {
                self.preferences = self.setup_draft.take().unwrap();
                self.request.volume = self.preferences.default_volume;
                self.submit();
                self.focus = 8;
                return vec![Effect::Mp3Setup {
                    seek_seconds: self.preferences.seek_seconds,
                    default_volume: self.preferences.default_volume,
                }];
            }
            CANCEL => {
                self.setup_draft = None;
                self.focus = 8;
            }
            _ => {}
        }
        Vec::new()
    }
    pub(super) fn setup_controls(&self, ctx: &Context) -> DialogSpec<'static> {
        let r = crate::turbo::dialog_spec::centered(ctx.area, 48, 16);
        let prefs = self.setup_draft.unwrap_or(self.preferences);
        let mut spec = ctx.dialog(
            r,
            self.setup_focus,
            ctx.language.text("Music player setup"),
            OK,
            CANCEL,
        );
        let style = ctx
            .palette
            .styles_for(crate::palette::theme::Group::Dialogs);
        spec.caption(
            Paragraph::new(ctx.language.text("Seek step (seconds)")).style(style(0x70)),
            Rect::new(r.x + 3, r.y + 2, 42, 1),
        );
        for (index, seconds) in STEPS.iter().enumerate() {
            spec.control(Control::new(
                index,
                Rect::new(r.x + 4, r.y + 3 + index as u16, 38, 1),
                ControlKind::Radio {
                    caption: seconds.to_string(),
                    checked: prefs.seek_seconds == *seconds,
                },
                ctx.target(STEP + index),
            ));
        }
        spec.caption(
            Paragraph::new(format!(
                "{}: {}%",
                ctx.language.text("Default volume"),
                prefs.default_volume
            ))
            .style(style(0x70)),
            Rect::new(r.x + 3, r.y + 8, 42, 1),
        );
        for (index, label, action, x, y) in [
            (4, "-10%", DOWN, 6, 10),
            (5, "+10%", UP, 28, 10),
            (6, "OK", OK, 6, 13),
            (7, "Cancel", CANCEL, 28, 13),
        ] {
            spec.control(Control::new(
                index,
                Rect::new(r.x + x, r.y + y, 12, 1),
                ControlKind::Button(ctx.language.text(label).into()),
                ctx.target(action),
            ));
        }
        spec
    }
}
