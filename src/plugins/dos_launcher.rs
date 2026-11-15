//! Experimental external DOSBox launcher configuration. All effects are host-owned.
use super::*;
use crate::{
    painting::{Paragraph, Wrap},
    turbo::dialog_spec::{centered, Control, ControlKind},
};
use crossterm::event::{KeyCode as K, MouseButton, MouseEventKind};
#[derive(Default)]
pub struct DosLauncher {
    focus: usize,
    path: String,
    cursor: usize,
    open_on_enter: bool,
}
impl ToolPlugin for DosLauncher {
    fn definition(&self) -> Definition {
        Definition::builtin(
            DOS_LAUNCHER,
            "DOS launcher (Experimental)",
            Presentation::Document,
        )
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn open(&mut self, ctx: &Context) {
        self.path = ctx.dos.executable.clone();
        self.cursor = self.path.len();
        self.open_on_enter = ctx.dos.open_on_enter;
        self.focus = 0;
    }
    fn uses_controls(&self, key: KeyEvent) -> bool {
        !(self.focus == 0 && key.code == K::Char(' '))
    }
    fn event(&mut self, event: Event<'_>, ctx: &Context) -> Vec<Effect> {
        let mut action = None;
        match event {
            Event::Focus(id) => self.focus = id,
            Event::Action(id) => action = Some(id.0),
            Event::Mouse(m, Some(id)) if m.kind == MouseEventKind::Down(MouseButton::Left) => {
                self.focus = id.0;
                if id.0 != 0 {
                    action = Some(id.0);
                }
            }
            Event::Paste(text) if self.focus == 0 => {
                if self.path.len() + text.len() <= 4096 {
                    crate::dialog::paste_input(&mut self.path, &mut self.cursor, text);
                }
            }
            Event::Key(k) if self.focus == 0 => {
                let before = self.path.clone();
                let cursor = self.cursor;
                crate::dialog::edit_input(&mut self.path, &mut self.cursor, k.code);
                if self.path.len() > 4096 {
                    self.path = before;
                    self.cursor = cursor;
                }
            }
            _ => {}
        }
        match action {
            Some(1) => self.open_on_enter = !self.open_on_enter,
            Some(2) => {
                return vec![
                    Effect::DosSetup(crate::dos::Preferences {
                        open_on_enter: self.open_on_enter,
                        executable: self.path.trim().to_owned(),
                    }),
                    Effect::Close,
                ]
            }
            Some(3) => return vec![Effect::Close],
            Some(4) => {
                return vec![Effect::Help {
                    title: " DOS launcher Help ",
                    text: ctx.language.text(HELP).into(),
                }]
            }
            _ => {}
        }
        vec![]
    }
    fn controls(&self, ctx: &Context) -> DialogSpec<'_> {
        let r = centered(ctx.area, 74, 15);
        let mut s = ctx.dialog(
            r,
            self.focus,
            ctx.language.text("DOS launcher (Experimental)"),
            2,
            3,
        );
        s.caption(
            Paragraph::new(
                ctx.language
                    .text("DOSBox Staging executable (blank: discover automatically)"),
            ),
            Rect::new(r.x + 3, r.y + 2, r.width - 6, 1),
        );
        let col =
            crate::presentation::text::Line::from(&self.path[..self.cursor.min(self.path.len())])
                .width();
        s.control(Control::new(
            0,
            Rect::new(r.x + 3, r.y + 3, r.width - 6, 1),
            ControlKind::ScrolledInput {
                value: self.path.clone(),
                scroll: col.saturating_sub(usize::from(r.width - 7)),
            },
            ctx.target(0),
        ));
        if self.focus == 0 {
            s.cursor = Some((r.x + 3 + col.min(usize::from(r.width - 7)) as u16, r.y + 3));
        }
        s.control(Control::new(
            1,
            Rect::new(r.x + 3, r.y + 5, r.width - 6, 1),
            ControlKind::Checkbox {
                caption: ctx.language.text("Open DOS programs with DOSBox").into(),
                checked: self.open_on_enter,
            },
            ctx.target(1),
        ));
        s.caption(Paragraph::new(ctx.language.text("DOSBox opens its own graphical window. BAT files require Run in DOSBox. No emulator is bundled.")).wrap(Wrap{trim:false}),Rect::new(r.x+3,r.y+7,r.width-6,3));
        for (id, label, x) in [(2, "OK", 5), (3, "Cancel", 28), (4, "Help", 51)] {
            s.control(Control::new(
                id,
                Rect::new(r.x + x, r.y + 12, 18, 1),
                ControlKind::Button(ctx.language.text(label).into()),
                ctx.target(id),
            ));
        }
        s
    }
}
pub const HELP:&str="Experimental DOS launcher: install DOSBox Staging separately, enable the plugin in Options > Configuration > Plugins, and choose File > Run in DOSBox for the current regular COM, DOS EXE or BAT file.\nThe separate Open DOS programs with DOSBox preference allows Enter/double-click on COM and DOS MZ executables. PE/NE/LE/LX Windows executables are excluded; BAT is always explicit. Ctrl+Enter and Open with default app bypass DOS handling.\nSet a DOSBox Staging executable path here or leave it blank for discovery. OK applies; Cancel/Esc discards. The selected file is rechecked before launch. No host shell or folder-local DOSBox configuration is used. Games open in DOSBox's graphical window, not inside DN; a graphical session is required.\nClosing or disabling the plugin cancels pending launches but does not terminate a game that has already started.";
