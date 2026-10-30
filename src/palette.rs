pub mod theme;
use crate::presentation::{
    style::{Color, Style},
    text::{Line, Span},
};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PaletteProfile {
    #[default]
    ClassicDn,
    /// Compatibility renderer for legacy frame tests; not selectable or loaded from settings.
    Terminal,
    Custom([u8; theme::COUNT]),
}
// Nearest standard xterm entries 16–255 to VGA RGB values. Squared RGB
// distance, lowest index on ties; no dependence on configurable ANSI colors.
pub const CLASSIC: [u8; 16] = [
    16, 19, 34, 37, 124, 127, 130, 248, 240, 63, 83, 87, 203, 207, 227, 231,
];
pub const TERMINAL: [u8; 16] = [0, 4, 2, 6, 1, 5, 3, 7, 8, 12, 10, 14, 9, 13, 11, 15];
impl PaletteProfile {
    pub fn key(self) -> &'static str {
        match self {
            Self::ClassicDn | Self::Custom(_) => "classic",
            Self::Terminal => "terminal",
        }
    }
    pub fn parse(_value: &str) -> Self {
        Self::ClassicDn
    }
    pub fn color(self, index: u8) -> Color {
        Color::Indexed(match self {
            Self::ClassicDn | Self::Custom(_) => CLASSIC[(index & 15) as usize],
            Self::Terminal => TERMINAL[(index & 15) as usize],
        })
    }
    pub fn dos(self, attribute: u8) -> Style {
        Style::default()
            .fg(self.color(attribute & 15))
            .bg(self.color(attribute >> 4))
    }
    pub fn styles(self) -> impl Fn(u8) -> Style + Copy {
        move |attribute| self.dos(attribute)
    }
    pub fn bar(self, enabled: bool) -> Style {
        self.for_group(
            theme::Group::Status,
            match (self.is_classic(), enabled) {
                (true, true) => 0x70,
                (true, false) => 0x78,
                (false, true) => 0x30,
                (false, false) => 0x38,
            },
        )
    }
    pub fn function(self, number: u8, label: &str, enabled: bool) -> Line<'static> {
        let key = if self.is_classic() {
            format!("F{number} ")
        } else {
            number.to_string()
        };
        self.function_key(key, label, enabled)
    }
    pub fn function_key(self, key: impl Into<String>, label: &str, enabled: bool) -> Line<'static> {
        let style = if self.is_classic() {
            self.for_group(theme::Group::Status, if enabled { 0x74 } else { 0x78 })
        } else {
            self.dos(0x07)
        };
        Line::from(vec![
            Span::styled(key.into(), style),
            Span::styled(label.to_string(), self.bar(enabled)),
        ])
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Focus(usize),
    Group(usize),
    Item(usize),
    Foreground(u8),
    Background(u8),
    Defaults,
    Color,
    BlackWhite,
    Mono,
    Scroll { items: bool, down: bool, page: bool },
}
pub struct ColorDialog {
    pub original: PaletteProfile,
    pub cursor: usize,
    pub group: usize,
    pub item: usize,
    pub group_offset: usize,
    pub item_offset: usize,
    pub defaults: bool,
}
impl ColorDialog {
    pub fn selected(&self) -> &'static theme::Item {
        theme::items(self.group)
            .nth(self.item)
            .unwrap_or_else(|| theme::items(self.group).next().unwrap())
    }
    fn keep_visible(&mut self) {
        self.group = self.group.min(theme::GROUPS.len() - 1);
        self.item = self.item.min(theme::items(self.group).count() - 1);
        self.group_offset = self
            .group_offset
            .min(self.group)
            .max(self.group.saturating_sub(12));
        self.item_offset = self
            .item_offset
            .min(self.item)
            .max(self.item.saturating_sub(12));
    }
}
pub fn open(app: &mut crate::app::App) {
    crate::menu::close(app);
    // Legacy terminal-palette files migrate to the fixed DN palette on load.
    let original = app.config.palette;
    if matches!(original, PaletteProfile::Terminal) {
        app.config.palette = PaletteProfile::ClassicDn;
    }
    app.modals.set_color_dialog(Some(ColorDialog {
        original,
        cursor: 4,
        group: 0,
        item: 0,
        group_offset: 0,
        item_offset: 0,
        defaults: app.config.palette == PaletteProfile::ClassicDn,
    }));
}
pub fn action(app: &mut crate::app::App, action: Action) {
    let Some(dialog) = app.modals.color_dialog_mut() else {
        return;
    };
    match action {
        Action::Focus(id) => {
            if id < 10 {
                dialog.cursor = id;
            }
        }
        Action::Group(row) => {
            dialog.group = row.min(theme::GROUPS.len() - 1);
            dialog.item = 0;
            dialog.item_offset = 0;
            dialog.cursor = 4;
        }
        Action::Item(row) => {
            dialog.item = row.min(theme::items(dialog.group).count() - 1);
            dialog.cursor = 5;
        }
        Action::Foreground(color) | Action::Background(color) => {
            let item = dialog.selected();
            let old = app.config.palette.slot(item.slot);
            let value = if matches!(action, Action::Foreground(_)) {
                (old & 0xf0) | (color & 15)
            } else {
                (old & 15) | ((color & 15) << 4)
            };
            if value != old {
                app.config.palette = app.config.palette.with_slot(item.slot, value);
                dialog.defaults = false;
            }
            dialog.cursor = if matches!(action, Action::Foreground(_)) {
                6
            } else {
                7
            };
        }
        Action::Defaults => {
            dialog.defaults = !dialog.defaults;
            if dialog.defaults {
                app.config.palette = PaletteProfile::ClassicDn;
            }
            dialog.cursor = 8;
        }
        Action::Color | Action::BlackWhite | Action::Mono => {
            let mut profile = PaletteProfile::ClassicDn;
            if action != Action::Color {
                let gray = |c: u8| {
                    if action == Action::Mono {
                        match c {
                            0 => 0,
                            1..=7 => 7,
                            8 => 8,
                            _ => 15,
                        }
                    } else {
                        match c {
                            0 => 0,
                            1..=6 => 8,
                            7 => 7,
                            8 => 8,
                            _ => 15,
                        }
                    }
                };
                for item in theme::ITEMS {
                    use theme::Slot;
                    let v = item.attribute;
                    let bg = if action == Action::Mono {
                        if matches!(v >> 4, 0 | 1 | 2 | 8) {
                            0
                        } else {
                            7
                        }
                    } else {
                        gray(v >> 4)
                    };
                    let mut fg = gray(v & 15);
                    if fg == bg && (v & 15) != (v >> 4) {
                        fg = if matches!(bg, 0 | 8) { 15 } else { 0 };
                    }
                    if bg == 8 && fg == 0 {
                        fg = 7;
                    }
                    // Keep selections distinct after reducing several source
                    // colors to the same gray; glyphs and focus order stay native.
                    let attribute = match item.slot {
                        Slot::MenuSelected
                        | Slot::MenuSelectedShortcut
                        | Slot::DialogListCursor
                        | Slot::HelpSelected => 0x0f,
                        Slot::PanelCursor
                        | Slot::PanelMarkedCursor
                        | Slot::EditorSelection
                        | Slot::ViewerMatch => 0x70,
                        _ => fg | (bg << 4),
                    };
                    profile = profile.with_slot(item.slot, attribute);
                }
            }
            app.config.palette = profile;
            dialog.defaults = profile == PaletteProfile::ClassicDn;
            dialog.cursor = match action {
                Action::Color => 0,
                Action::BlackWhite => 1,
                _ => 9,
            };
        }
        Action::Scroll { items, down, page } => {
            let (current, count) = if items {
                (dialog.item, theme::items(dialog.group).count())
            } else {
                (dialog.group, theme::GROUPS.len())
            };
            let next = crate::turbo::widgets::scroll_step(
                current,
                count.saturating_sub(1),
                13,
                down,
                page,
            );
            if items {
                dialog.item = next;
                dialog.cursor = 5;
            } else {
                dialog.group = next;
                dialog.item = 0;
                dialog.item_offset = 0;
                dialog.cursor = 4;
            }
        }
    }
    dialog.keep_visible();
    app.layout_revision = app
        .layout_revision
        .checked_add(1)
        .expect("layout revision exhausted");
}
pub fn cancel(app: &mut crate::app::App) {
    if let Some(dialog) = app.modals.take_color_dialog() {
        app.config.palette = dialog.original;
    }
}
pub fn key(app: &mut crate::app::App, code: crossterm::event::KeyCode) {
    key_event(
        app,
        crossterm::event::KeyEvent::new(code, crossterm::event::KeyModifiers::NONE),
    );
}
pub fn key_event(app: &mut crate::app::App, event: crossterm::event::KeyEvent) {
    let code = event.code;
    if event
        .modifiers
        .intersects(crossterm::event::KeyModifiers::CONTROL | crossterm::event::KeyModifiers::SUPER)
    {
        return;
    }
    use crossterm::event::KeyCode as K;
    let Some(dialog) = app.modals.color_dialog() else {
        return;
    };
    let cursor = dialog.cursor;
    if matches!(cursor, 4 | 5) {
        let (row, count) = if cursor == 4 {
            (dialog.group, theme::GROUPS.len())
        } else {
            (dialog.item, theme::items(dialog.group).count())
        };
        if let Some(row) = crate::turbo::widgets::list_key(row, count, 13, code) {
            action(
                app,
                if cursor == 4 {
                    Action::Group(row)
                } else {
                    Action::Item(row)
                },
            );
            return;
        }
    }
    if matches!(cursor, 6 | 7) {
        let value = app.config.palette.slot(dialog.selected().slot);
        if let Some(color) = crate::turbo::widgets::color_key(
            if cursor == 6 { value & 15 } else { value >> 4 },
            code,
        ) {
            action(
                app,
                if cursor == 6 {
                    Action::Foreground(color)
                } else {
                    Action::Background(color)
                },
            );
            return;
        }
    }
    if let K::Char(c) = code {
        if c != ' ' {
            let proposal = crate::ui::remaining_spec(
                app,
                crate::turbo::dialog_spec::input_area(app),
                crate::focus::Focus::Colors,
            )
            .and_then(|spec| spec.mnemonic(c).map(|id| spec.activate(id)));
            if let Some(proposal) = proposal {
                crate::dialog_controls::apply(app, proposal, crate::dialog_controls::tool_action);
                return;
            }
        }
    }
    if crate::dialog_controls::key(
        app,
        crossterm::event::KeyEvent::new(
            if !matches!(cursor, 4..=7) {
                match code {
                    K::Up => K::BackTab,
                    K::Down => K::Tab,
                    _ => code,
                }
            } else {
                code
            },
            event.modifiers,
        ),
        crate::focus::Focus::Colors,
    ) {
        return;
    }
    match code {
        K::Char('g' | 'G' | 'г' | 'Г') => app.modals.color_dialog_mut().unwrap().cursor = 4,
        K::Char('i' | 'I' | 'э' | 'Э') => app.modals.color_dialog_mut().unwrap().cursor = 5,
        K::Char('f' | 'F' | 'п' | 'П') => app.modals.color_dialog_mut().unwrap().cursor = 6,
        K::Char('b' | 'B' | 'ф' | 'Ф') => app.modals.color_dialog_mut().unwrap().cursor = 7,
        K::Char('d' | 'D') => action(app, Action::Defaults),
        K::Char('c' | 'C') => action(app, Action::Color),
        K::Char('w' | 'W') => action(app, Action::BlackWhite),
        K::Char('m' | 'M') => action(app, Action::Mono),
        _ => {}
    }
}
pub fn mouse(
    app: &mut crate::app::App,
    event: crossterm::event::MouseEvent,
    target: Option<crate::mouse::Target>,
) {
    use crossterm::event::{MouseButton, MouseEventKind as K};
    match event.kind {
        K::Down(MouseButton::Left) => {
            if let Some(t) = target {
                crate::dialog_controls::tool_action(app, t);
            }
        }
        K::ScrollUp | K::ScrollDown => {
            let items = match target {
                Some(crate::mouse::Target::Colors(Action::Item(_))) => true,
                Some(crate::mouse::Target::Colors(Action::Group(_))) => false,
                _ => match app.modals.color_dialog().map(|d| d.cursor) {
                    Some(4) => false,
                    Some(5) => true,
                    _ => return,
                },
            };
            action(
                app,
                Action::Scroll {
                    items,
                    down: event.kind == K::ScrollDown,
                    page: false,
                },
            );
        }
        _ => {}
    }
}
