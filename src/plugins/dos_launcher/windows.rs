//! An owned Windows boot draft, independent from DOS program preferences.
use super::*;
use crate::{
    dos::windows::Preferences,
    turbo::file_dialog::{Mode, Outcome, Picker},
};
/// A bounded, display-only name followed by the profile's identifying settings.
pub(super) fn image_summary(p: &Preferences, width: u16) -> String {
    let suffix = format!(" · {} · {} MiB", p.edition.label(), p.memory_mb);
    let name = crate::filesystem::display_name(
        &std::path::Path::new(&p.image)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy(),
    );
    let reserved = crate::presentation::text::Span::raw(&suffix)
        .width()
        .min(u16::MAX as usize) as u16;
    format!(
        "{}{}",
        crate::presentation::text::ellipsize_tail(&name, width.saturating_sub(reserved)),
        suffix
    )
}
pub(super) struct Draft {
    pub preferences: Preferences,
    cursor: usize,
    error: String,
}
impl Draft {
    pub fn new(preferences: Preferences) -> Self {
        let cursor = preferences.image.len();
        Self {
            preferences,
            cursor,
            error: String::new(),
        }
    }
}
impl DosLauncher {
    pub(super) fn picker_event(&mut self, event: Event<'_>, _ctx: &Context) -> Vec<Effect> {
        let p = self.picker.as_mut().expect("Windows image picker");
        let outcome = match event {
            Event::Key(k) => p.key(k),
            Event::Paste(t) => {
                p.paste(t);
                None
            }
            Event::Mouse(m, _) => p.mouse(m, (0, 0)),
            Event::Action(ActionId(900)) => Some(Outcome::Cancelled),
            _ => None,
        };
        if let Some(outcome) = outcome {
            self.picker = None;
            self.focus = 200;
            self.page = self.page.wrapping_add(1);
            if let Outcome::Selected(_, path) = outcome {
                let d = self.win_page.as_mut().expect("Windows page");
                if let Some(text) = path.to_str() {
                    d.preferences.image = text.into();
                    d.cursor = text.len();
                } else {
                    d.error = "This image path cannot be represented as a Unicode path.".into();
                }
            }
        }
        vec![]
    }
    pub(super) fn windows_event(&mut self, event: Event<'_>, ctx: &Context) -> Vec<Effect> {
        let d = self.win_page.as_mut().expect("Windows page");
        let mut action = None;
        match event {
            Event::Focus(id) => {
                self.focus = id;
                d.cursor = if id == 201 {
                    d.preferences.executable.len()
                } else {
                    d.preferences.image.len()
                };
            }
            Event::Action(id) => action = Some(id.0),
            Event::Mouse(m, Some(id)) if m.kind == MouseEventKind::Down(MouseButton::Left) => {
                self.focus = id.0;
                d.cursor = if id.0 == 201 {
                    d.preferences.executable.len()
                } else {
                    d.preferences.image.len()
                };
                if !matches!(id.0, 200 | 201) {
                    action = Some(id.0);
                }
            }
            Event::Paste(t) if matches!(self.focus, 200 | 201) => {
                let value = if self.focus == 200 {
                    &mut d.preferences.image
                } else {
                    &mut d.preferences.executable
                };
                if value.len() + t.len() <= 4096 && !t.chars().any(char::is_control) {
                    crate::dialog::paste_input(value, &mut d.cursor, t);
                }
            }
            Event::Key(k) if k.code == K::F(1) => action = Some(213),
            Event::Key(k) if matches!(self.focus, 200 | 201) => {
                let value = if self.focus == 200 {
                    &mut d.preferences.image
                } else {
                    &mut d.preferences.executable
                };
                if k.modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL)
                    && k.code == K::Char('a')
                {
                    value.clear();
                    d.cursor = 0;
                } else {
                    let before = (value.clone(), d.cursor);
                    crate::dialog::edit_input(value, &mut d.cursor, k.code);
                    if value.len() > 4096 || value.chars().any(char::is_control) {
                        (*value, d.cursor) = before;
                    }
                }
            }
            _ => {}
        }
        match action {
            Some(202) => {
                self.picker = Some(
                    Picker::new(Mode::Open, self.base.clone(), ctx.language, true)
                        .with_target(ctx.target(900)),
                );
                self.page = self.page.wrapping_add(1);
            }
            Some(203) => {
                d.preferences.edition = d.preferences.edition.next();
                if !d
                    .preferences
                    .memory_choices()
                    .contains(&d.preferences.memory_mb)
                {
                    d.preferences.memory_mb = 128;
                }
            }
            Some(204) => {
                let choices = d.preferences.memory_choices();
                let i = choices
                    .iter()
                    .position(|v| *v == d.preferences.memory_mb)
                    .unwrap_or(0);
                d.preferences.memory_mb = choices[(i + 1) % choices.len()];
            }
            Some(205) => d.preferences.mouse = !d.preferences.mouse,
            Some(206) => d.preferences.sound = !d.preferences.sound,
            Some(207) => d.preferences.fullscreen = !d.preferences.fullscreen,
            Some(208) => d.preferences.voodoo = !d.preferences.voodoo,
            Some(209) if !ctx.dos_status.busy => match d.preferences.validate() {
                Ok(()) => {
                    d.error.clear();
                    return vec![Effect::WindowsBoot {
                        preferences: d.preferences.clone(),
                        boot: false,
                    }];
                }
                Err(e) => d.error = e.to_string(),
            },
            Some(214) => {
                self.image_choice = Some(
                    self.images
                        .iter()
                        .position(|p| p.image == d.preferences.image)
                        .unwrap_or(self.images.len()),
                );
                self.image_offset = self.image_choice.unwrap().saturating_sub(7);
                self.focus = 230;
                self.page = self.page.wrapping_add(1);
            }
            Some(215) => {
                let mut images = self.images.clone();
                images.retain(|p| p.image != d.preferences.image);
                let current = images.first().cloned().unwrap_or_default();
                return vec![Effect::WindowsImagesSave {
                    expected: self.images.clone(),
                    current,
                    images,
                }];
            }
            Some(211) => {
                let mut images = self.images.clone();
                if let Some(p) = images.iter_mut().find(|p| p.image == d.preferences.image) {
                    *p = d.preferences.clone();
                } else {
                    images.push(d.preferences.clone());
                }
                match crate::dos::windows::validate_images(&images) {
                    Ok(()) => {
                        return vec![Effect::WindowsImagesSave {
                            expected: self.images.clone(),
                            current: d.preferences.clone(),
                            images,
                        }]
                    }
                    Err(e) => d.error = e.to_string(),
                }
            }
            Some(212) => {
                self.win_page = None;
                self.focus = if self.settings.is_some() { 108 } else { 9 };
                self.page = self.page.wrapping_add(1);
                return vec![Effect::DosStop];
            }
            Some(213) => {
                return vec![Effect::Help {
                    title: " Windows 9x Help ",
                    text: ctx.language.text(HELP).into(),
                }]
            }
            _ => {}
        }
        vec![]
    }
    pub(super) fn picker_spec(&self, ctx: &Context) -> DialogSpec<'_> {
        let p = self.picker.as_ref().expect("Windows picker");
        let mut s = ctx.dialog(p.bounds(), 0, ctx.language.text("Open file"), 900, 900);
        let mut c = Control::new(900, s.bounds, ControlKind::Surface, ctx.target(900));
        c.paint = false;
        s.control(c);
        if let Some(scene) = p.scene() {
            let mut scene = scene.clone();
            scene.regions = vec![(s.bounds, ctx.target(900))];
            s.scene(scene);
        }
        s
    }
    pub(super) fn windows_spec(&self, ctx: &Context) -> DialogSpec<'_> {
        let d = self.win_page.as_ref().expect("Windows page");
        let p = &d.preferences;
        let l = ctx.language;
        let r = centered(ctx.area, 76, 23);
        let x = r.x + 3;
        let w = r.width - 6;
        let mut s = ctx.dialog(r, self.focus, l.text("Windows 9x — DOSBox-X"), 211, 212);
        for (id, y, label, value) in [
            (200, 3, "Installed raw HDD image (.img / .ima)", &p.image),
            (
                201,
                6,
                "DOSBox-X executable (blank: find automatically)",
                &p.executable,
            ),
        ] {
            s.caption(
                Paragraph::new(l.text(label)),
                Rect::new(x, r.y + y - 1, w, 1),
            );
            let width = if id == 200 { w - 13 } else { w };
            let cursor = d.cursor.min(value.len());
            let col =
                crate::presentation::text::Line::from(&value[..value.floor_char_boundary(cursor)])
                    .width();
            s.control(Control::new(
                id,
                Rect::new(x, r.y + y, width, 1),
                ControlKind::ScrolledInput {
                    value: value.clone(),
                    scroll: if self.focus == id {
                        col.saturating_sub(usize::from(width - 1))
                    } else {
                        0
                    },
                },
                ctx.target(id),
            ));
            if self.focus == id {
                s.cursor = Some((x + col.min(usize::from(width - 1)) as u16, r.y + y));
            }
        }
        s.control(Control::new(
            202,
            Rect::new(x + w - 12, r.y + 3, 12, 1),
            ControlKind::Button(l.text("Browse…").into()),
            ctx.target(202),
        ));
        for (id, bounds, value) in [
            (
                203,
                Rect::new(x, r.y + 8, w / 2 - 1, 1),
                p.edition.label().to_owned(),
            ),
            (
                204,
                Rect::new(x + w / 2 + 1, r.y + 8, w / 2 - 1, 1),
                format!("{}: {} MiB", l.text("Memory"), p.memory_mb),
            ),
        ] {
            let mut c = Control::new(id, bounds, ControlKind::Choice(vec![value]), ctx.target(id));
            c.enter_action = true;
            s.control(c);
        }
        for (id, y, right, label, checked) in [
            (205, 10, false, "Capture mouse", p.mouse),
            (206, 10, true, "Sound Blaster 16", p.sound),
            (207, 12, false, "Fullscreen", p.fullscreen),
            (208, 12, true, "Voodoo (guest driver needed)", p.voodoo),
        ] {
            s.control(Control::new(
                id,
                Rect::new(x + if right { w / 2 + 1 } else { 0 }, r.y + y, w / 2 - 1, 1),
                ControlKind::Checkbox {
                    caption: l.text(label).into(),
                    checked,
                },
                ctx.target(id),
            ));
        }
        s.caption(
            Paragraph::new(
                l.text("Windows writes to this image. Use a backup; close other emulators."),
            )
            .wrap(Wrap { trim: false }),
            Rect::new(x, r.y + 14, w, 2),
        );
        let notice = if !d.error.is_empty() {
            l.message(&d.error)
        } else if !ctx.dos_status.notice.is_empty() {
            l.message(&ctx.dos_status.notice)
        } else {
            l.text("Use an already installed Windows image. No installation wizard.")
                .into()
        };
        s.caption(
            Paragraph::new(notice).wrap(Wrap { trim: false }),
            Rect::new(x, r.y + 16, w, 2),
        );
        let selection = if !self.images.iter().any(|saved| saved.image == p.image) {
            l.text("New image profile…").into()
        } else {
            let prefix = format!("{}: ", l.text("Saved image"));
            let reserved = crate::presentation::text::Span::raw(&prefix).width() as u16;
            let name = crate::filesystem::display_name(
                &std::path::Path::new(&p.image)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy(),
            );
            format!(
                "{}{}",
                prefix,
                crate::presentation::text::ellipsize_tail(&name, w.saturating_sub(reserved + 4))
            )
        };
        let mut c = Control::new(
            214,
            Rect::new(x, r.y + 1, w, 1),
            ControlKind::Choice(vec![selection]),
            ctx.target(214),
        );
        c.enter_action = true;
        s.control(c);
        for (y, buttons) in [
            (18, vec![(209, "Check"), (215, "Remove profile")]),
            (
                20,
                vec![(211, "Save profile"), (212, "Cancel"), (213, "Help")],
            ),
        ] {
            let labels: Vec<_> = buttons.iter().map(|(_, v)| l.text(v)).collect();
            let bounds =
                crate::dialog_theme::button_row(Rect::new(x, r.y + y, w, 1), &labels, 8, 1);
            for ((id, _), (label, bounds)) in
                buttons.into_iter().zip(labels.into_iter().zip(bounds))
            {
                let mut c = Control::new(
                    id,
                    bounds,
                    ControlKind::Button(label.into()),
                    ctx.target(id),
                );
                c.enabled = match id {
                    209 => !ctx.dos_status.busy,
                    215 => self.images.iter().any(|v| v.image == p.image),
                    _ => true,
                };
                s.control(c);
            }
        }
        s.tab_order = vec![
            214, 200, 202, 201, 203, 204, 205, 206, 207, 208, 209, 215, 211, 212, 213,
        ];
        s
    }
}
pub(super) const HELP:&str="Windows 9x uses DOSBox-X only. Install DOSBox-X separately. Choose an existing writable, partitioned FAT raw HDD image (.img or .ima) with installed Windows 95, 95 OSR2 or 98/98 SE. Save profile immediately saves that image, edition, RAM, executable override and checkboxes through the configuration store, even with Save settings on exit off. Up to 32 images can be saved. Saved image opens a selector; choose an entry to load its settings or New image profile to add another. Remove profile removes only the saved entry. Cancel discards unsaved edits; previously saved profiles remain. Boot Windows is on the main DOS launcher and starts the selected saved profile, not an editor draft. The guest can change the image: keep a backup and close other emulators. Check verifies the executable and image header, not a working Windows installation. No installer, image or license is supplied. Normal DOS preferences remain independent. Windows uses a compatible normal CPU core, Pentium MMX, S3 graphics, selected RAM and optional SB16/Voodoo. Voodoo needs a guest driver. Normal core may be slow, especially on ARM. Guest networking and host-folder conversion are disabled; user autoexec is skipped. A second launch of the same image within DN is refused; DOSBox-X image locking is enabled. User/game configuration files are not modified; private fragments follow emulator lifetime. Unsupported image paths containing DOS command characters are refused.";

impl DosLauncher {
    pub(super) fn images_event(&mut self, event: Event<'_>, _ctx: &Context) -> Vec<Effect> {
        let mut selected = self.image_choice.expect("image selector");
        let mut action = None;
        match event {
            Event::Focus(id) => self.focus = id,
            Event::Action(id) => action = Some(id.0),
            Event::Mouse(m, Some(id)) if m.kind == MouseEventKind::Down(MouseButton::Left) => {
                self.focus = id.0;
                action = Some(id.0);
            }
            Event::Key(k) => {
                selected = match k.code {
                    K::Up => selected.saturating_sub(1),
                    K::Down => (selected + 1).min(self.images.len()),
                    K::Home => 0,
                    K::End => self.images.len(),
                    K::PageUp => selected.saturating_sub(8),
                    K::PageDown => (selected + 8).min(self.images.len()),
                    _ => selected,
                };
            }
            _ => {}
        }
        match action {
            Some(231) => {
                let p = self.images.get(selected).cloned().unwrap_or_default();
                let d = self.win_page.as_mut().expect("Windows page");
                d.cursor = p.image.len();
                d.preferences = p;
                d.error.clear();
                self.image_choice = None;
                self.focus = 214;
                self.page = self.page.wrapping_add(1);
                return vec![];
            }
            Some(232) => {
                self.image_choice = None;
                self.focus = 214;
                self.page = self.page.wrapping_add(1);
                return vec![];
            }
            Some(id) if id >= 240 && id - 240 <= self.images.len() => {
                selected = id - 240;
                self.focus = 230;
            }
            _ => {}
        }
        self.image_choice = Some(selected);
        self.image_offset = self
            .image_offset
            .min(selected)
            .max(selected.saturating_sub(7));
        vec![]
    }
    pub(super) fn images_spec(&self, ctx: &Context) -> DialogSpec<'_> {
        let r = centered(ctx.area, 72, 16);
        let l = ctx.language;
        let mut s = ctx.dialog(r, self.focus, l.text("Saved Windows images"), 231, 232);
        let mut items: Vec<_> = self
            .images
            .iter()
            .map(|p| image_summary(p, r.width.saturating_sub(8)))
            .collect();
        items.push(l.text("New image profile…").into());
        let mut list = Control::new(
            230,
            Rect::new(r.x + 3, r.y + 2, r.width - 6, 8),
            ControlKind::List {
                items,
                cursor: self.image_choice.unwrap_or(0),
                offset: self.image_offset,
            },
            ctx.target(230),
        );
        list.dn_list = true;
        list.hits = (0..8)
            .filter(|i| self.image_offset + i <= self.images.len())
            .map(|i| {
                (
                    Rect::new(r.x + 3, r.y + 2 + i as u16, r.width - 6, 1),
                    ctx.target(240 + self.image_offset + i),
                )
            })
            .collect();
        s.control(list);
        if let Some(p) = self.image_choice.and_then(|i| self.images.get(i)) {
            s.caption(
                Paragraph::new(crate::filesystem::display_name(&p.image))
                    .wrap(Wrap { trim: false }),
                Rect::new(r.x + 3, r.y + 10, r.width - 6, 2),
            );
        }
        s.caption(
            Paragraph::new(l.text("Select an image to load its saved settings."))
                .wrap(Wrap { trim: false }),
            Rect::new(r.x + 3, r.y + 1, r.width - 6, 1),
        );
        let labels = [l.text("Use"), l.text("Cancel")];
        let bounds = crate::dialog_theme::button_row(
            Rect::new(r.x + 3, r.y + 13, r.width - 6, 1),
            &labels,
            10,
            1,
        );
        for ((id, label), bounds) in [231, 232].into_iter().zip(labels).zip(bounds) {
            s.control(Control::new(
                id,
                bounds,
                ControlKind::Button(label.into()),
                ctx.target(id),
            ));
        }
        s.tab_order = vec![230, 231, 232];
        s
    }
}
