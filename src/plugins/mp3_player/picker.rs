//! Native DN file picker owned by the music-player document.
use super::*;
use crate::turbo::file_dialog::{Mode, Outcome, Picker};
impl Mp3Player {
    pub(super) fn open_picker(&mut self, base: PathBuf, hidden: bool, play: bool, ctx: &Context) {
        if self.import.is_some() || self.picker.is_some() || self.setup_draft.is_some() {
            return;
        }
        self.picker_play = play;
        self.picker = Some(
            Picker::new(
                Mode::Open,
                self.picker_directory.clone().unwrap_or(base),
                ctx.language,
                hidden,
            )
            .with_target(ctx.target(PICKER_INPUT)),
        );
    }
    pub(super) fn picker_event(&mut self, event: Event<'_>, ctx: &Context) -> Vec<Effect> {
        let picker = self.picker.as_mut().unwrap();
        let outcome = match event {
            Event::Key(k) => picker.key(k),
            Event::Mouse(e, _) => picker.mouse(e, (0, 0)),
            Event::Paste(text) => {
                picker.paste(text);
                None
            }
            Event::Action(ActionId(PICKER_CANCEL | CLOSE)) => Some(Outcome::Cancelled),
            _ => None,
        };
        if let Some(outcome) = outcome {
            self.picker_directory = Some(picker.directory());
            self.picker = None;
            self.finish_picker(outcome, ctx);
        }
        Vec::new()
    }
    pub(super) fn finish_picker(&mut self, outcome: Outcome, ctx: &Context) {
        if let Outcome::Selected(_, path) = outcome {
            if !supports_path(&path) {
                self.notice = ctx
                    .language
                    .text("Choose an MP3, FLAC, WAV or Ogg Vorbis file")
                    .into();
            } else if self.picker_play {
                self.play_file(path, ctx);
            } else if !self.playlist.order.iter().any(|t| t.path == path) {
                self.add_files(vec![path], ctx);
            }
        }
    }
}
