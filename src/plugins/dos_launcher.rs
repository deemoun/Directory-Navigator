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
    row: usize,
    offset: usize,
    page: u64,
    generation: u64,
}
impl DosLauncher {
    fn select(&mut self, ctx: &Context, row: usize) -> Vec<Effect> {
        let custom = !self.path.is_empty()
            && !ctx
                .dos_status
                .tools
                .iter()
                .any(|t| t.executable.to_str() == Some(self.path.as_str()));
        if row > ctx.dos_status.tools.len() + usize::from(custom) {
            return vec![];
        }
        if row == 0 {
            self.path.clear();
        } else if let Some(tool) = ctx.dos_status.tools.get(row - 1) {
            let Some(path) = tool.executable.to_str() else {
                return vec![Effect::Help {
                    title: " DOS launcher Help ",
                    text: ctx
                        .language
                        .text("This emulator path cannot be represented as a saved Unicode path.")
                        .into(),
                }];
            };
            self.path = path.into();
        }
        self.row = row;
        self.cursor = self.path.len();
        self.focus = 6;
        self.offset = row.saturating_sub(3);
        vec![]
    }
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
    fn prepare(&mut self, ctx: &Context) {
        if self.generation != ctx.dos_status.generation {
            self.generation = ctx.dos_status.generation;
            self.page = self.page.wrapping_add(1);
        }
        self.row = if self.path.is_empty() {
            0
        } else {
            ctx.dos_status
                .tools
                .iter()
                .position(|t| t.executable.to_str() == Some(self.path.as_str()))
                .map_or(ctx.dos_status.tools.len() + 1, |i| i + 1)
        };
        self.offset = self.offset.min(self.row).max(self.row.saturating_sub(3));
    }
    fn native_input_page(&self) -> Option<u64> {
        Some(self.page)
    }
    fn uses_controls(&self, key: KeyEvent) -> bool {
        if self.focus == 6
            && matches!(
                key.code,
                K::Up
                    | K::Down
                    | K::Home
                    | K::End
                    | K::PageUp
                    | K::PageDown
                    | K::Enter
                    | K::Char(' ')
            )
        {
            return false;
        }
        !(self.focus == 0 && key.code == K::Char(' '))
    }
    fn event(&mut self, event: Event<'_>, ctx: &Context) -> Vec<Effect> {
        let mut action = None;
        match event {
            Event::Focus(id) => self.focus = id,
            Event::Action(id) if id.0 >= 1000 => return self.select(ctx, id.0 - 1000),
            Event::Action(id) => action = Some(id.0),
            Event::Mouse(m, Some(id)) if m.kind == MouseEventKind::Down(MouseButton::Left) => {
                if id.0 >= 1000 {
                    return self.select(ctx, id.0 - 1000);
                }
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
            Event::Key(k) if self.focus == 6 => {
                let count = ctx.dos_status.tools.len()
                    + 1
                    + usize::from(
                        !self.path.is_empty()
                            && !ctx
                                .dos_status
                                .tools
                                .iter()
                                .any(|t| t.executable.to_str() == Some(self.path.as_str())),
                    );
                let row = match k.code {
                    K::Up => self.row.saturating_sub(1),
                    K::Down => (self.row + 1).min(count - 1),
                    K::Home => 0,
                    K::End => count - 1,
                    K::PageUp => self.row.saturating_sub(4),
                    K::PageDown => (self.row + 4).min(count - 1),
                    _ => self.row,
                };
                return self.select(ctx, row);
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
            Some(6) => return self.select(ctx, self.row),
            Some(7) => {
                return vec![if ctx.dos_status.busy {
                    Effect::DosStop
                } else {
                    Effect::DosDiscover
                }]
            }
            Some(5) if !ctx.dos_status.busy => {
                return vec![Effect::DosCheck(crate::dos::Preferences {
                    open_on_enter: self.open_on_enter,
                    executable: self.path.trim().to_owned(),
                })]
            }
            Some(4) => {
                return vec![Effect::Help {
                    title: " DOS launcher Help ",
                    text: format!(
                        "{}\n\n{}\n\n{}",
                        ctx.language.text(HELP),
                        ctx.language.message(&ctx.dos_status.notice),
                        ctx.dos_status.last_error
                    ),
                }]
            }
            _ => {}
        }
        vec![]
    }
    fn controls(&self, ctx: &Context) -> DialogSpec<'_> {
        let r = centered(ctx.area, 76, 23);
        let mut s = ctx.dialog(
            r,
            self.focus,
            ctx.language.text("DOS launcher (Experimental)"),
            2,
            3,
        );
        s.caption(
            Paragraph::new(ctx.language.text("Emulator executable (blank: Automatic)")),
            Rect::new(r.x + 3, r.y + 1, r.width - 6, 1),
        );
        let col =
            crate::presentation::text::Line::from(&self.path[..self.cursor.min(self.path.len())])
                .width();
        s.control(Control::new(
            0,
            Rect::new(r.x + 3, r.y + 2, r.width - 6, 1),
            ControlKind::ScrolledInput {
                value: self.path.clone(),
                scroll: col.saturating_sub(usize::from(r.width - 7)),
            },
            ctx.target(0),
        ));
        if self.focus == 0 {
            s.cursor = Some((r.x + 3 + col.min(usize::from(r.width - 7)) as u16, r.y + 2));
        }
        s.caption(
            Paragraph::new(ctx.language.text("Installed emulators")),
            Rect::new(r.x + 3, r.y + 4, r.width.saturating_sub(6), 1),
        );
        let mut items = vec![ctx.language.text("Automatic (first available)").to_owned()];
        items.extend(ctx.dos_status.tools.iter().map(|tool| tool.label()));
        if !self.path.is_empty()
            && !ctx
                .dos_status
                .tools
                .iter()
                .any(|t| t.executable.to_str() == Some(self.path.as_str()))
        {
            items.push(ctx.language.text("Custom executable path").into());
        }
        let count = items.len();
        let offset = self.offset.min(self.row);
        let mut list = Control::new(
            6,
            Rect::new(r.x + 3, r.y + 5, r.width.saturating_sub(6), 4),
            ControlKind::List {
                items,
                cursor: self.row.min(count - 1),
                offset,
            },
            ctx.target(6),
        );
        list.dn_list = true;
        list.enter_action = true;
        list.hits = (0..4)
            .filter(|i| offset + i < count)
            .map(|i| {
                (
                    Rect::new(r.x + 3, r.y + 5 + i as u16, r.width.saturating_sub(6), 1),
                    ctx.target(1000 + offset + i),
                )
            })
            .collect();
        s.control(list);
        s.control(Control::new(
            1,
            Rect::new(r.x + 3, r.y + 10, r.width - 6, 1),
            ControlKind::Checkbox {
                caption: ctx.language.text("Open DOS programs with DOSBox").into(),
                checked: self.open_on_enter,
            },
            ctx.target(1),
        ));
        s.caption(Paragraph::new(ctx.language.text("DOSBox opens a graphical window. Enter requires the checkbox above. Use File > Run in DOSBox for BAT files. No emulator is bundled.")).wrap(Wrap{trim:false}),Rect::new(r.x+3,r.y+12,r.width-6,3));
        s.caption(
            Paragraph::new(ctx.language.message(&ctx.dos_status.notice)).wrap(Wrap { trim: false }),
            Rect::new(r.x + 3, r.y + 16, r.width.saturating_sub(6), 3),
        );
        let controls = [
            (2, "OK"),
            (3, "Cancel"),
            (5, "Check DOSBox"),
            (
                7,
                if ctx.dos_status.busy {
                    "Stop"
                } else {
                    "Find emulators"
                },
            ),
            (4, "Help"),
        ];
        let labels = controls.map(|(_, label)| ctx.language.text(label));
        let bounds = crate::dialog_theme::button_row(
            Rect::new(r.x + 3, r.y + 20, r.width.saturating_sub(6), 1),
            &labels,
            10,
            1,
        );
        for ((id, label), bounds) in controls.into_iter().zip(bounds) {
            let mut c = Control::new(
                id,
                bounds,
                ControlKind::Button(ctx.language.text(label).into()),
                ctx.target(id),
            );
            c.enabled = id != 5 || !ctx.dos_status.busy;
            s.control(c);
        }
        s
    }
}
pub const HELP: &str = "Experimental DOS launcher: install DOSBox, DOSBox-X or DOSBox Staging separately and enable the plugin in Options > Configuration > Plugins. File > Run in DOSBox launches the current regular COM, DOS EXE or BAT file.\nFind emulators builds a selectable installed list. Blank executable means Automatic, preferring Staging, then DOSBox-X, then classic DOSBox. You can also type an executable path. Check DOSBox tests the draft without saving or starting a game; it does not test graphical output. OK applies; Cancel/Esc discards.\nThe separate Open DOS programs with DOSBox checkbox allows Enter/double-click on COM and DOS MZ executables before associations. PE/NE/LE/LX Windows executables are excluded; BAT is always explicit. Ctrl+Enter and Open with default app bypass DOS handling.\nGames open in a separate graphical window, not inside DN; a graphical session is required. The source is rechecked before launch and arguments are passed without a host shell. Global emulator settings are retained; automatic game-local configuration loading is disabled. Launch diagnostics remain in this window and Help. Closing or disabling the plugin cancels pending checks/launches without terminating games already started.";
