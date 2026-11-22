//! Experimental external DOSBox launcher configuration. All effects are host-owned.
use super::*;
mod drives;
mod settings;
mod windows;
use crate::{
    painting::{Paragraph, Wrap},
    turbo::dialog_spec::{centered, Control, ControlKind},
};
use crossterm::event::{KeyCode as K, MouseButton, MouseEventKind};
#[derive(Default)]
pub struct DosLauncher {
    focus: usize,
    profile: crate::dos::Profile,
    mount: crate::dos::mount::Preferences,
    mount_page: Option<drives::Draft>,
    windows: crate::dos::windows::Preferences,
    images: Vec<crate::dos::windows::Preferences>,
    image_choice: Option<usize>,
    image_offset: usize,
    win_page: Option<windows::Draft>,
    picker: Option<crate::turbo::file_dialog::Picker>,
    base: std::path::PathBuf,
    settings: Option<settings::Draft>,
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
        self.profile = ctx.dos.profile.clone();
        self.mount = ctx.dos.mount.clone();
        self.mount_page = None;
        self.windows = ctx.dos.windows.clone();
        self.images = ctx.dos.windows_images.clone();
        self.win_page = None;
        self.image_choice = None;
        self.picker = None;
        self.base = ctx
            .storage
            .as_ref()
            .and_then(|p| p.parent())
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(std::env::temp_dir);
        self.settings = None;
        self.path = ctx.dos.executable.clone();
        self.cursor = self.path.len();
        self.open_on_enter = ctx.dos.open_on_enter;
        self.focus = 0;
    }
    fn prepare(&mut self, ctx: &Context) {
        if let Some(p) = &mut self.picker {
            p.prepare(
                crate::presentation::layout::Size::new(ctx.area.width, ctx.area.height),
                ctx.palette,
            );
        }
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
        Some(
            self.page
                .wrapping_mul(8)
                .wrapping_add(if self.mount_page.is_some() {
                    5
                } else if self.image_choice.is_some() {
                    4
                } else if self.picker.is_some() {
                    3
                } else if self.win_page.is_some() {
                    2
                } else if self.settings.is_some() {
                    1
                } else {
                    0
                }),
        )
    }
    fn uses_controls(&self, key: KeyEvent) -> bool {
        if self.mount_page.is_some() {
            return !(self.focus == 302 && key.code == K::Char(' '));
        }
        if self.image_choice.is_some() {
            return !matches!(
                key.code,
                K::Up | K::Down | K::Home | K::End | K::PageUp | K::PageDown
            );
        }
        if self.picker.is_some() {
            return false;
        }
        if self.win_page.is_some() {
            return !(matches!(self.focus, 200 | 201) && key.code == K::Char(' '));
        }
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
        !((self.focus == 0 || self.settings.is_some() && self.focus == 101)
            && key.code == K::Char(' '))
    }
    fn event(&mut self, event: Event<'_>, ctx: &Context) -> Vec<Effect> {
        if let Event::DosMountFolder { generation, path } = event {
            if generation == self.page {
                if let Some(d) = &mut self.mount_page {
                    if let Some(s) = path.to_str() {
                        d.preferences.folder = s.into();
                        d.cursor = s.len();
                        self.page = self.page.wrapping_add(1);
                    } else {
                        d.error =
                            "This folder path cannot be represented as a Unicode DOS mount path."
                                .into();
                    }
                }
            }
            return vec![];
        }
        if let Event::WindowsImagesSaved { current, images } = event {
            self.windows = current.clone();
            self.images = images;
            if let Some(d) = &mut self.settings {
                d.windows = current;
            }
            self.win_page = None;
            self.focus = if self.settings.is_some() { 108 } else { 9 };
            self.page = self.page.wrapping_add(1);
            return vec![];
        }
        if let Event::DosDirectory(p) = event {
            self.base = p;
            return vec![];
        }
        if self.mount_page.is_some() {
            return self.drives_event(event, ctx);
        }
        if self.image_choice.is_some() {
            return self.images_event(event, ctx);
        }
        if self.picker.is_some() {
            return self.picker_event(event, ctx);
        }
        if self.win_page.is_some() {
            return self.windows_event(event, ctx);
        }
        if self.settings.is_some() {
            return self.settings_event(event, ctx);
        }
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
            Some(9) => {
                self.win_page = Some(windows::Draft::new(self.windows.clone()));
                self.focus = 200;
                self.page = self.page.wrapping_add(1);
            }
            Some(10) if !ctx.dos_status.busy && !self.windows.image.is_empty() => {
                return vec![Effect::WindowsBoot {
                    preferences: self.windows.clone(),
                    boot: true,
                }]
            }
            Some(8) => {
                self.settings = Some(settings::Draft::new(
                    self.profile.clone(),
                    self.windows.clone(),
                    self.mount.clone(),
                ));
                self.focus = 100;
                self.page = self.page.wrapping_add(1);
            }
            Some(1) => self.open_on_enter = !self.open_on_enter,
            Some(2) => {
                return vec![
                    Effect::DosSetup(crate::dos::Preferences {
                        open_on_enter: self.open_on_enter,
                        executable: self.path.trim().to_owned(),
                        profile: self.profile.clone(),
                        mount: self.mount.clone(),
                        windows: self.windows.clone(),
                        windows_images: self.images.clone(),
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
                    profile: self.profile.clone(),
                    mount: self.mount.clone(),
                    windows: self.windows.clone(),
                    windows_images: self.images.clone(),
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
        if self.mount_page.is_some() {
            self.drives_spec(ctx)
        } else if self.image_choice.is_some() {
            self.images_spec(ctx)
        } else if self.picker.is_some() {
            self.picker_spec(ctx)
        } else if self.win_page.is_some() {
            self.windows_spec(ctx)
        } else if self.settings.is_some() {
            self.settings_spec(ctx)
        } else {
            self.main_spec(ctx)
        }
    }
    fn view(&self, ctx: &Context) -> View<'_> {
        let mut spec = self.main_spec(ctx);
        if self.settings.is_some() {
            spec.overlay(self.settings_spec(ctx));
        }
        if self.win_page.is_some() {
            spec.overlay(self.windows_spec(ctx));
        }
        if self.picker.is_some() {
            spec.overlay(self.picker_spec(ctx));
        }
        if self.image_choice.is_some() {
            spec.overlay(self.images_spec(ctx));
        }
        if self.mount_page.is_some() {
            spec.overlay(self.drives_spec(ctx));
        }
        View::Dialog(Box::new(spec))
    }
}
impl DosLauncher {
    fn main_spec(&self, ctx: &Context) -> DialogSpec<'_> {
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
        s.caption(
            Paragraph::new(if ctx.dos_status.notice.is_empty() {
                ctx.language
                    .text("Not checked. Use Find emulators or Check DOSBox.")
                    .into()
            } else {
                ctx.language.message(&ctx.dos_status.notice)
            })
            .wrap(Wrap { trim: false }),
            Rect::new(r.x + 3, r.y + 16, r.width.saturating_sub(6), 2),
        );
        s.caption(
            Paragraph::new(if self.windows.image.is_empty() {
                ctx.language
                    .text("Requires an installed DOSBox emulator.")
                    .into()
            } else {
                format!(
                    "Windows: {}",
                    windows::image_summary(&self.windows, r.width.saturating_sub(15))
                )
            }),
            Rect::new(r.x + 3, r.y + 11, r.width - 6, 1),
        );
        s.caption(
            Paragraph::new(self.profile.summary(ctx.language)).wrap(Wrap { trim: false }),
            Rect::new(r.x + 3, r.y + 12, r.width - 6, 2),
        );
        let labels = [
            ctx.language.text("Windows 9x…"),
            ctx.language.text("Boot Windows"),
        ];
        let bounds = crate::dialog_theme::button_row(
            Rect::new(r.x + 3, r.y + 14, r.width - 6, 1),
            &labels,
            14,
            1,
        );
        for ((id, label), bounds) in [9, 10].into_iter().zip(labels).zip(bounds) {
            let mut c = Control::new(
                id,
                bounds,
                ControlKind::Button(label.into()),
                ctx.target(id),
            );
            c.enabled = !ctx.dos_status.busy && (id != 10 || !self.windows.image.is_empty());
            s.control(c);
        }
        for (y, controls) in [
            (
                18,
                [
                    (8, "DOS settings…"),
                    (5, "Check DOSBox"),
                    (
                        7,
                        if ctx.dos_status.busy {
                            "Stop"
                        } else {
                            "Find emulators"
                        },
                    ),
                ],
            ),
            (20, [(2, "OK"), (3, "Cancel"), (4, "Help")]),
        ] {
            let labels = controls.map(|(_, label)| ctx.language.text(label));
            let bounds = crate::dialog_theme::button_row(
                Rect::new(r.x + 3, r.y + y, r.width - 6, 1),
                &labels,
                12,
                1,
            );
            for ((id, _), (label, bounds)) in
                controls.into_iter().zip(labels.into_iter().zip(bounds))
            {
                let mut control = Control::new(
                    id,
                    bounds,
                    ControlKind::Button(label.into()),
                    ctx.target(id),
                );
                control.enabled = !matches!(id, 5 | 8) || !ctx.dos_status.busy;
                s.control(control);
            }
        }
        s
    }
}
pub const HELP: &str = "Experimental DOS launcher: install DOSBox, DOSBox-X or DOSBox Staging separately and enable the plugin in Options > Configuration > Plugins. File > Run in DOSBox launches the current regular COM, DOS EXE or BAT file.\nFind emulators builds a selectable installed list. Blank executable means Automatic, preferring Staging, then DOSBox-X, then classic DOSBox. You can also type an executable path. Check DOSBox tests the draft without saving or starting a game; it does not test graphical output. OK applies; Cancel/Esc discards.\nThe separate Open DOS programs with DOSBox checkbox allows Enter/double-click on COM and DOS MZ executables before associations. PE/NE/LE/LX Windows executables are excluded; BAT is always explicit. Ctrl+Enter and Open with default app bypass DOS handling.\nGames open in a separate graphical window, not inside DN; a graphical session is required. The source is rechecked before launch and arguments are passed without a host shell. Global emulator settings are retained; automatic game-local configuration loading is disabled. DOS settings opens an owned draft for shared CPU speed, memory and window/fullscreen overrides. All default to Use emulator settings. Settings OK updates the launcher draft; launcher OK applies it for subsequent launches. DN generates a private override fragment and never edits emulator or game configuration files. Launch diagnostics remain in this window and Help. Closing or disabling the plugin cancels pending checks/launches without terminating games already started. Windows 9x opens a separate DOSBox-X profile for booting an existing installed raw HDD image; its settings do not affect DOS programs. Saved Windows images have individual settings. Save profile writes immediately; choose an entry with Saved image. Boot Windows is on the main launcher. DOS drives opens a separate draft for an extra D:–X: drive: Off, DN Temp Drive or a real folder. The selected folder becomes the emulator host working directory. The program folder remains C:; Windows image booting is unchanged.";
