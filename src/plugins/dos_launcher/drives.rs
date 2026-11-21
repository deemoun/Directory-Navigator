//! Owned, ordinary-DOS mount preferences. Windows image boot never reads these.
use super::*;
use crate::dos::mount::{Mode, Preferences};
pub(super) struct Draft {
    pub preferences: Preferences,
    pub(super) cursor: usize,
    pub(super) error: String,
}
impl Draft {
    pub fn new(preferences: Preferences) -> Self {
        let cursor = preferences.folder.len();
        Self {
            preferences,
            cursor,
            error: String::new(),
        }
    }
}
impl DosLauncher {
    pub(super) fn drives_event(&mut self, event: Event<'_>, ctx: &Context) -> Vec<Effect> {
        let d = self.mount_page.as_mut().expect("DOS drives page");
        let mut action = None;
        match event {
            Event::Focus(id) => self.focus = id,
            Event::Action(id) => action = Some(id.0),
            Event::Mouse(m, Some(id)) if m.kind == MouseEventKind::Down(MouseButton::Left) => {
                self.focus = id.0;
                if id.0 != 302 {
                    action = Some(id.0);
                }
            }
            Event::Paste(t) if self.focus == 302 && d.preferences.mode == Mode::Folder => {
                if d.preferences.folder.len() + t.len() <= 4096 && !t.chars().any(char::is_control)
                {
                    crate::dialog::paste_input(&mut d.preferences.folder, &mut d.cursor, t);
                }
            }
            Event::Key(k) if k.code == K::F(1) => action = Some(306),
            Event::Key(k) if self.focus == 302 && d.preferences.mode == Mode::Folder => {
                if k.modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL)
                    && k.code == K::Char('a')
                {
                    d.preferences.folder.clear();
                    d.cursor = 0;
                } else {
                    let before = (d.preferences.folder.clone(), d.cursor);
                    crate::dialog::edit_input(&mut d.preferences.folder, &mut d.cursor, k.code);
                    if d.preferences.folder.len() > 4096
                        || d.preferences.folder.chars().any(char::is_control)
                    {
                        (d.preferences.folder, d.cursor) = before;
                    }
                }
            }
            _ => {}
        }
        match action {
            Some(300) => {
                d.preferences.mode = d.preferences.mode.next();
                d.error.clear();
            }
            Some(301) if d.preferences.mode != Mode::Off => {
                d.preferences.drive = if d.preferences.drive == b'X' {
                    b'D'
                } else {
                    d.preferences.drive + 1
                }
            }
            Some(303) if d.preferences.mode == Mode::Folder => {
                return vec![Effect::DosMountTree {
                    generation: self.page,
                    base: self.base.clone(),
                }]
            }
            Some(304) => match d.preferences.validate() {
                Ok(()) => {
                    self.settings.as_mut().expect("DOS settings").mount = d.preferences.clone();
                    self.mount_page = None;
                    self.focus = 109;
                    self.page = self.page.wrapping_add(1);
                }
                Err(e) => d.error = e.to_string(),
            },
            Some(305) => {
                self.mount_page = None;
                self.focus = 109;
                self.page = self.page.wrapping_add(1);
            }
            Some(306) => {
                return vec![Effect::Help {
                    title: " DOS drives Help ",
                    text: ctx.language.text(HELP).into(),
                }]
            }
            _ => {}
        }
        vec![]
    }
    pub(super) fn drives_spec(&self, ctx: &Context) -> DialogSpec<'_> {
        let d = self.mount_page.as_ref().expect("DOS drives page");
        let p = &d.preferences;
        let l = ctx.language;
        let r = centered(ctx.area, 76, 19);
        let x = r.x + 3;
        let w = r.width - 6;
        let mut s = ctx.dialog(r, self.focus, l.text("Extra DOS drive"), 304, 305);
        s.caption(
            Paragraph::new(l.text("Ordinary DOS launches only; Windows 9x images are separate."))
                .wrap(Wrap { trim: false }),
            Rect::new(x, r.y + 1, w, 2),
        );
        for (id, y, label, value) in [
            (300, 4, "Mount", l.text(p.mode.label()).to_owned()),
            (301, 6, "DOS drive", format!("{}:", char::from(p.drive))),
        ] {
            s.caption(Paragraph::new(l.text(label)), Rect::new(x, r.y + y, 17, 1));
            let mut c = Control::new(
                id,
                Rect::new(x + 18, r.y + y, w - 18, 1),
                ControlKind::Choice(vec![value]),
                ctx.target(id),
            );
            c.enter_action = true;
            c.enabled = id == 300 || p.mode != Mode::Off;
            s.control(c);
        }
        s.caption(
            Paragraph::new(l.text("Folder")),
            Rect::new(x, r.y + 8, w, 1),
        );
        let col = crate::presentation::text::Line::from(
            &p.folder[..p.folder.floor_char_boundary(d.cursor.min(p.folder.len()))],
        )
        .width();
        let width = w - 13;
        let mut input = Control::new(
            302,
            Rect::new(x, r.y + 9, width, 1),
            ControlKind::ScrolledInput {
                value: p.folder.clone(),
                scroll: if self.focus == 302 {
                    col.saturating_sub(usize::from(width - 1))
                } else {
                    0
                },
            },
            ctx.target(302),
        );
        input.enabled = p.mode == Mode::Folder;
        s.control(input);
        let mut browse = Control::new(
            303,
            Rect::new(x + w - 12, r.y + 9, 12, 1),
            ControlKind::Button(l.text("Browse…").into()),
            ctx.target(303),
        );
        browse.enabled = p.mode == Mode::Folder;
        s.control(browse);
        if self.focus == 302 && p.mode == Mode::Folder {
            s.cursor = Some((x + col.min(usize::from(width - 1)) as u16, r.y + 9));
        }
        s.caption(
            Paragraph::new(l.text("C: runs the program; Y:/Z: are reserved. Choose D:–X:."))
                .wrap(Wrap { trim: false }),
            Rect::new(x, r.y + 11, w, 1),
        );
        s.caption(
            Paragraph::new(
                l.text("The chosen extra drive is remapped for this launch. Files are writable."),
            )
            .wrap(Wrap { trim: false }),
            Rect::new(x, r.y + 12, w, 2),
        );
        s.caption(
            Paragraph::new(if d.error.is_empty() {
                l.text("OK updates the DOS settings draft; launcher OK applies.")
            } else {
                l.text(&d.error)
            })
            .wrap(Wrap { trim: false }),
            Rect::new(x, r.y + 14, w, 2),
        );
        let labels = [l.text("OK"), l.text("Cancel"), l.text("Help")];
        let bounds = crate::dialog_theme::button_row(Rect::new(x, r.y + 16, w, 1), &labels, 10, 1);
        for ((id, label), bounds) in [304, 305, 306].into_iter().zip(labels).zip(bounds) {
            s.control(Control::new(
                id,
                bounds,
                ControlKind::Button(label.into()),
                ctx.target(id),
            ));
        }
        s.tab_order = vec![300, 301, 302, 303, 304, 305, 306];
        s
    }
}
pub(super) const HELP:&str="Extra DOS drive applies only to ordinary DOS program launches, on macOS, Linux and Windows hosts. Off preserves existing launch arguments. DN Temp Drive shares DN's existing persistent Temp Drive; it is created only when launching and is not cleared on emulator exit. Folder shares the selected real native directory; Browse opens the owned Directory tree picker. Choose D:–X:. C: stays assigned to the launched program's directory and Y:/Z: stay with the emulator. For this launch DN changes to Z:, unmounts the selected extra letter and mounts the chosen folder as a native working directory before starting the program. DOSBox starts with that host working directory and uses MOUNT with a relative dot, preserving Unicode host paths; relative user autoexec paths resolve from that folder. Existing data is not erased; DOS programs can modify the shared folder's files. The global user configuration stays loaded; automatic game-local configuration loading remains disabled. No host shell is used. Control characters, directory links and unavailable folders are rejected before launch. Check DOSBox previews the drive commands without creating a missing Temp Drive. Settings use nested OK/Cancel drafts and normal configuration saving. Reset to emulator settings turns the extra mount off. Windows 9x saved images and boot commands are unaffected.";
