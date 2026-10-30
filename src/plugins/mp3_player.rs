//! UI-only MP3 player, adapted from DN 1.51 CDPLAYER.PAS's counter and layout.
//! Copyright (C) 1991-99 RIT Research Labs.
//! Based on Dos Navigator by RIT Research Labs; see THIRD_PARTY_NOTICES.md.
use super::*;
use crate::{
    painting::{Frame, NativeFrame, Paragraph},
    presentation::text::{Line, Span},
    turbo::dialog_spec::{Control, ControlKind},
    windowing::geometry::Capabilities,
};
use crossterm::event::{KeyCode as K, MouseButton, MouseEventKind};

const PLAY: usize = 2;
const ZOOM: usize = 100;
const CLOSE: usize = 101;
const HELP: usize = 102;
const HELP_TEXT: &str = "This is a UI-only MP3 player prototype inspired by DOS Navigator.\nNo audio is played and no files are opened or read.\nTransport and secondary buttons only demonstrate native focus and pressed feedback.\nTab / Shift+Tab select controls; Enter / Space press the selected button.\nAlt+Z, the title zoom button or a title double-click switch compact/expanded views.\nCtrl+F5 moves the player; Esc closes it. No preferences are saved.";

#[derive(Default)]
pub(crate) struct Mp3Player {
    expanded: bool,
    focus: usize,
}
impl Mp3Player {
    fn bounds(&self, ctx: &Context) -> Rect {
        crate::turbo::dialog_spec::centered(ctx.area, 36, if self.expanded { 16 } else { 9 })
    }
    fn count(&self) -> usize {
        if self.expanded {
            14
        } else {
            5
        }
    }
    fn action(&mut self, id: usize) -> Vec<Effect> {
        match id {
            CLOSE => return vec![Effect::Close],
            HELP => {
                return vec![Effect::Help {
                    title: " MP3 Player Help ",
                    text: HELP_TEXT.into(),
                }]
            }
            ZOOM => {
                self.expanded = !self.expanded;
                if self.focus >= self.count() {
                    self.focus = PLAY;
                }
            }
            id if id < self.count() => self.focus = id,
            _ => {}
        }
        Vec::new()
    }
}
impl ToolPlugin for Mp3Player {
    fn definition(&self) -> Definition {
        Definition::builtin(MP3_PLAYER, "MP3 Player", Presentation::Document)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn open(&mut self, _: &Context) {
        self.expanded = false;
        self.focus = PLAY;
    }
    fn zoom_action(&self) -> Option<ActionId> {
        Some(ActionId(ZOOM))
    }
    fn event(&mut self, event: Event<'_>, _: &Context) -> Vec<Effect> {
        match event {
            Event::Action(ActionId(id)) => self.action(id),
            Event::Focus(id) if id < self.count() => {
                self.focus = id;
                Vec::new()
            }
            Event::Key(k) if k.code == K::Esc => self.action(CLOSE),
            Event::Key(k) if k.code == K::F(1) => self.action(HELP),
            Event::Mouse(e, Some(ActionId(id)))
                if e.kind == MouseEventKind::Down(MouseButton::Left) =>
            {
                self.action(id)
            }
            _ => Vec::new(),
        }
    }
    fn windows(&self, ctx: &Context) -> Vec<LocalWindow> {
        let bounds = self.bounds(ctx);
        vec![LocalWindow {
            id: 0,
            parent: None,
            bounds,
            caps: Capabilities {
                resize: false,
                zoom: true,
                minimum: (bounds.width, bounds.height),
            },
        }]
    }
    fn controls(&self, ctx: &Context) -> DialogSpec<'_> {
        let r = self.bounds(ctx);
        let language = ctx.language;
        let style = ctx
            .palette
            .styles_for(crate::palette::theme::Group::Dialogs);
        let mut spec = ctx.dialog(r, self.focus, language.text("MP3 Player"), PLAY, CLOSE);
        spec.native(
            crate::turbo::widgets::group_box(""),
            Rect::new(r.x + 1, r.y + 1, 34, 5),
        );
        for (label, x, w) in [("Track", r.x + 4, 8), ("Track Time", r.x + 17, 12)] {
            spec.caption(
                Paragraph::new(language.text(label)).style(style(0x70)),
                Rect::new(x, r.y + 1, w, 1),
            );
        }
        // DN's original 3x3 zero glyph, decoded from CP437. No timer or
        // metadata state: both counters remain permanently at zero.
        for x in [r.x + 3, r.x + 7, r.x + 12, r.x + 16, r.x + 22, r.x + 26] {
            for (row, text) in ["▄▄▄", "█ █", "█▄█"].into_iter().enumerate() {
                spec.caption(
                    Paragraph::new(text).style(style(0x70)),
                    Rect::new(x, r.y + 2 + row as u16, 3, 1),
                );
            }
        }
        for y in [r.y + 3, r.y + 4] {
            spec.caption(
                Paragraph::new("▪").style(style(0x70)),
                Rect::new(r.x + 20, y, 1, 1),
            );
        }
        for (row, text) in ["M", "P", "3"].into_iter().enumerate() {
            spec.caption(
                Paragraph::new(text).style(style(0x70)),
                Rect::new(r.x + 32, r.y + 2 + row as u16, 1, 1),
            );
        }
        let labels = [
            "◄◄", "<", "►", ">", "►►", "Pause", "Stop", "Open", "Set up", "Time", "Mode", "Volume",
            "List", "Info",
        ];
        for (id, label) in labels.iter().take(self.count()).enumerate() {
            let (x, y, w) = if id < 5 {
                let (x, w) = [(1, 6), (7, 5), (12, 11), (23, 5), (28, 6)][id];
                (r.x + x, r.y + 6, w)
            } else {
                let n = id - 5;
                (
                    r.x + 1 + (n % 3) as u16 * 11,
                    r.y + 8 + (n / 3) as u16 * 2,
                    11,
                )
            };
            spec.control(Control::new(
                id,
                Rect::new(x, y, w, 1),
                ControlKind::Button(language.text(label).into()),
                ctx.target(id),
            ));
        }
        if self.expanded {
            spec.caption(
                Paragraph::new(language.text("UI prototype · no playback")).style(style(0x78)),
                Rect::new(r.x + 2, r.y + 14, 32, 1),
            );
        }
        spec.caption(
            Paragraph::new(Line::from(vec![
                Span::styled("[", style(0x7f)),
                Span::styled("■", style(0x7a)),
                Span::styled("]", style(0x7f)),
            ])),
            Rect::new(r.x + 2, r.y, 3, 1),
        );
        spec
    }
    fn view(&self, ctx: &Context) -> View<'_> {
        let mut frame = NativeFrame::new(ctx.area, ctx.palette);
        frame.render_widget(
            crate::turbo::widgets::dialog(ctx.language.text("MP3 Player")),
            self.bounds(ctx),
        );
        View::Layers(vec![Layer {
            id: 0,
            interactive: true,
            scene: frame.finish(),
            controls: Some(self.controls(ctx)),
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode as K, KeyEvent, KeyModifiers as M};
    #[test]
    fn native_traversal_wraps_and_collapsing_hidden_focus_returns_to_play() {
        let ctx = Context {
            binding: Binding {
                plugin: MP3_PLAYER,
                owner: Owner::Document(crate::desktop::WindowId::MANAGER),
            },
            scope: ControlScope::Document {
                window: crate::desktop::WindowId::MANAGER,
                revision: 0,
            },
            area: Rect::new(0, 0, 100, 32),
            language: crate::localization::Language::English,
            palette: crate::palette::PaletteProfile::ClassicDn,
            active: true,
            now: Instant::now(),
            storage: None,
        };
        let mut player = Mp3Player::default();
        player.open(&ctx);
        let step = |player: &mut Mp3Player, code, modifiers| {
            let proposal = player
                .controls(&ctx)
                .key(KeyEvent::new(code, modifiers))
                .unwrap();
            player.event(Event::Focus(proposal.focus), &ctx);
            if let Some(Target::Plugin(_, a)) = proposal.action {
                assert!(player.event(Event::Action(a), &ctx).is_empty());
            }
        };
        assert_eq!(player.focus, PLAY);
        for n in [3, 4, 0, 1, 2] {
            step(&mut player, K::Tab, M::NONE);
            assert_eq!(player.focus, n);
        }
        step(&mut player, K::Tab, M::SHIFT);
        assert_eq!(player.focus, 1);
        step(&mut player, K::BackTab, M::NONE);
        assert_eq!(player.focus, 0);
        player.action(ZOOM);
        for _ in 0..13 {
            step(&mut player, K::Tab, M::NONE);
        }
        assert_eq!(player.focus, 13);
        step(&mut player, K::Enter, M::NONE);
        assert_eq!(player.focus, 13);
        player.action(ZOOM);
        assert_eq!(player.focus, PLAY);
        assert!(player.next_wake(ctx.now).is_none());
    }
}
