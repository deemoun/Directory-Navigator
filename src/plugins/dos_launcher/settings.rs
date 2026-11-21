use super::*;
use crate::dos::{CpuSpeed, Profile};
pub(super) struct Draft {
    pub profile: Profile,
    pub mount: crate::dos::mount::Preferences,
    pub windows: crate::dos::windows::Preferences,
    pub cycles: String,
    pub cursor: usize,
    pub error: String,
}
impl Draft {
    pub fn new(
        profile: Profile,
        windows: crate::dos::windows::Preferences,
        mount: crate::dos::mount::Preferences,
    ) -> Self {
        let cycles = profile.cycles.to_string();
        let cursor = cycles.len();
        Self {
            profile,
            mount,
            windows,
            cycles,
            cursor,
            error: String::new(),
        }
    }
    pub fn accept(&mut self) -> std::io::Result<Profile> {
        let mut p = self.profile.clone();
        if p.cpu == CpuSpeed::Fixed {
            p.cycles = self
                .cycles
                .parse()
                .map_err(|_| std::io::Error::other("Enter CPU cycles from 50 to 2,000,000."))?;
        }
        p.validate()?;
        self.mount.validate()?;
        Ok(p)
    }
}
impl DosLauncher {
    pub(super) fn settings_event(&mut self, event: Event<'_>, ctx: &Context) -> Vec<Effect> {
        let mut action = None;
        let draft = self.settings.as_mut().expect("settings page");
        match event {
            Event::Focus(id) => self.focus = id,
            Event::Action(id) => action = Some(id.0),
            Event::Mouse(m, Some(id)) if m.kind == MouseEventKind::Down(MouseButton::Left) => {
                self.focus = id.0;
                if id.0 != 101 {
                    action = Some(id.0)
                }
            }
            Event::Paste(text) if self.focus == 101 && draft.profile.cpu == CpuSpeed::Fixed => {
                if text.chars().all(|c| c.is_ascii_digit()) && draft.cycles.len() + text.len() <= 7
                {
                    crate::dialog::paste_input(&mut draft.cycles, &mut draft.cursor, text);
                }
            }
            Event::Key(k) if k.code == K::F(1) => action = Some(107),
            Event::Key(k) if self.focus == 101 && draft.profile.cpu == CpuSpeed::Fixed => {
                if k.modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL)
                    && k.code == K::Char('a')
                {
                    draft.cycles.clear();
                    draft.cursor = 0;
                } else if !matches!(k.code,K::Char(c) if !c.is_ascii_digit()) {
                    let before = (draft.cycles.clone(), draft.cursor);
                    crate::dialog::edit_input(&mut draft.cycles, &mut draft.cursor, k.code);
                    if draft.cycles.len() > 7 {
                        (draft.cycles, draft.cursor) = before;
                    }
                }
            }
            _ => {}
        }
        match action {
            Some(100) => draft.profile.cpu = draft.profile.cpu.next(),
            Some(102) => {
                let values = [0, 1, 2, 4, 8, 16, 32, 63];
                let i = values
                    .iter()
                    .position(|v| *v == draft.profile.memory_mb)
                    .unwrap_or(0);
                draft.profile.memory_mb = values[(i + 1) % values.len()];
            }
            Some(103) => draft.profile.display = draft.profile.display.next(),
            Some(110) => draft.profile.resolution = draft.profile.resolution.next(),
            Some(104) => {
                *draft = Draft::new(
                    Profile::default(),
                    draft.windows.clone(),
                    crate::dos::mount::Preferences::default(),
                );
                self.focus = 100;
            }
            Some(105) => match draft.accept() {
                Ok(profile) => {
                    self.profile = profile;
                    self.mount = draft.mount.clone();
                    self.windows = draft.windows.clone();
                    self.settings = None;
                    self.focus = 8;
                    self.page = self.page.wrapping_add(1);
                }
                Err(error) => {
                    draft.error = error.to_string();
                    self.focus = 101;
                }
            },
            Some(106) => {
                self.settings = None;
                self.focus = 8;
                self.page = self.page.wrapping_add(1);
            }
            Some(109) => {
                self.mount_page = Some(drives::Draft::new(draft.mount.clone()));
                self.focus = 300;
                self.page = self.page.wrapping_add(1);
            }
            Some(108) => {
                self.win_page = Some(windows::Draft::new(draft.windows.clone()));
                self.focus = 200;
                self.page = self.page.wrapping_add(1);
            }
            Some(107) => {
                return vec![Effect::Help {
                    title: " DOS settings Help ",
                    text: ctx.language.text(SETTINGS_HELP).into(),
                }]
            }
            _ => {}
        }
        vec![]
    }
    pub(super) fn settings_spec(&self, ctx: &Context) -> DialogSpec<'_> {
        let d = self.settings.as_ref().expect("settings page");
        let l = ctx.language;
        let r = centered(ctx.area, 74, 22);
        let mut spec = ctx.dialog(r, self.focus, l.text("DOS settings"), 105, 106);
        let x = r.x + 3;
        let w = r.width - 6;
        spec.caption(
            Paragraph::new(l.text("Shared settings for subsequent DOS launches.")),
            Rect::new(x, r.y + 1, w, 1),
        );
        for (id, y, label, value) in [
            (
                100,
                3,
                "CPU speed",
                l.text(d.profile.cpu.label()).to_owned(),
            ),
            (
                102,
                7,
                "Memory",
                if d.profile.memory_mb == 0 {
                    l.text("Use emulator settings").into()
                } else {
                    format!("{} MiB", d.profile.memory_mb)
                },
            ),
            (
                103,
                9,
                "Display",
                l.text(d.profile.display.label()).to_owned(),
            ),
            (
                110,
                11,
                "Window resolution",
                l.text(d.profile.resolution.label()).to_owned(),
            ),
        ] {
            spec.caption(Paragraph::new(l.text(label)), Rect::new(x, r.y + y, 18, 1));
            let mut control = Control::new(
                id,
                Rect::new(x + 19, r.y + y, w - 19, 1),
                ControlKind::Choice(vec![value]),
                ctx.target(id),
            );
            control.enter_action = true;
            spec.control(control);
        }
        spec.caption(
            Paragraph::new(l.text("Fixed cycles")),
            Rect::new(x, r.y + 5, 18, 1),
        );
        let mut control = Control::new(
            101,
            Rect::new(x + 19, r.y + 5, 14, 1),
            ControlKind::Input(d.cycles.clone()),
            ctx.target(101),
        );
        control.enabled = d.profile.cpu == CpuSpeed::Fixed;
        spec.control(control);
        if self.focus == 101 && d.profile.cpu == CpuSpeed::Fixed {
            spec.cursor = Some((x + 19 + d.cursor as u16, r.y + 5));
        }
        spec.caption(
            Paragraph::new(
                l.text("Window size only; fullscreen uses emulator settings. Cycles are not MHz."),
            )
            .wrap(Wrap { trim: false }),
            Rect::new(x, r.y + 12, w, 2),
        );
        spec.caption(
            Paragraph::new(if d.error.is_empty() {
                l.text("OK updates the launcher draft; launcher OK applies.")
            } else {
                l.text(&d.error)
            })
            .wrap(Wrap { trim: false }),
            Rect::new(x, r.y + 16, w, 2),
        );
        let labels = [l.text("Windows 9x…"), l.text("DOS drives…")];
        let bounds = crate::dialog_theme::button_row(Rect::new(x, r.y + 14, w, 1), &labels, 16, 1);
        for ((id, label), bounds) in [108, 109].into_iter().zip(labels).zip(bounds) {
            spec.control(Control::new(
                id,
                bounds,
                ControlKind::Button(label.into()),
                ctx.target(id),
            ));
        }
        let buttons = [
            (104, "Reset to emulator settings"),
            (105, "OK"),
            (106, "Cancel"),
            (107, "Help"),
        ];
        let labels = buttons.map(|(_, s)| l.text(s));
        let bounds = crate::dialog_theme::button_row(Rect::new(x, r.y + 19, w, 1), &labels, 8, 1);
        for ((id, _), (label, bounds)) in buttons.into_iter().zip(labels.into_iter().zip(bounds)) {
            spec.control(Control::new(
                id,
                bounds,
                ControlKind::Button(label.into()),
                ctx.target(id),
            ));
        }
        spec.tab_order = vec![100, 101, 102, 103, 110, 108, 109, 104, 105, 106, 107];
        spec
    }
}
pub(super) const SETTINGS_HELP:&str="These shared settings affect subsequent DOS launches only. Use emulator settings leaves that setting unchanged. Fixed CPU cycles accept 50–2,000,000; cycles are not MHz. Maximum varies with host speed and may run old programs too fast. Memory choices are 1–63 MiB for compatibility. Window resolution sets initial window bounds: Automatic inherits emulator settings, or choose 640x480, 800x600, 1024x768, 1280x960, 1600x1200 or 1920x1080. It does not change the DOS video mode or fullscreen resolution. OS scaling, aspect correction and desktop bounds may adjust the result. Classic DOSBox and DOSBox-X surface output may ignore sizing; choose a scaling renderer in the emulator configuration if needed. DN leaves the renderer unchanged. Sound, renderer and other settings remain with the emulator. Reset changes only this draft. This page's OK returns to the launcher draft; launcher OK applies it through normal configuration saving. DN never edits emulator or game configuration files. Explicit overrides are generated in a private temporary fragment, loaded after user settings. Game-local configuration loading remains disabled. Windows 9x opens a separate DOSBox-X profile for booting an existing installed raw HDD image; its settings do not affect DOS programs. Saved Windows images have individual settings. Save profile writes immediately; choose an entry with Saved image. Boot Windows is on the main launcher. DOS drives opens a separate draft for an extra D:–X: drive: Off, DN Temp Drive or a real folder. The selected folder becomes the emulator host working directory. The program folder remains C:; Windows image booting is unchanged.";
