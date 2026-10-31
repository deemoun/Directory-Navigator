//! Native controls hosted without a nested event loop or terminal I/O.
//!
//! Controllers retain semantic state and guarded actions. Each control is hydrated
//! from that state; native events return proposals, never filesystem operations.
use crate::{
    painting::{Frame, NativeFrame, Widget},
    palette::PaletteProfile,
    presentation::layout::Rect,
};
use std::collections::BTreeMap;
use turbo_vision::{
    core::{event::*, geometry::Rect as TvRect, state::State},
    views::{
        button::Button, checkbox::CheckBox, dialog::Dialog, input_line::InputLine,
        listbox::ListBox, memo::Memo, progress_bar::ProgressBar, radiobutton::RadioButton,
        ListViewer, View,
    },
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// The upstream DrawBuffer holds scalar values. Encode complete graphemes into
/// cells before native layout, then restore them at the scene boundary. This
/// preserves combining sequences, wide glyphs and literal private-use characters.
#[derive(Default)]
pub(super) struct CellEncoding {
    pub(super) glyphs: BTreeMap<char, String>,
    reverse: BTreeMap<String, char>,
    next: u32,
}
#[derive(Clone, Default)]
pub(super) struct TextCells(pub(super) std::rc::Rc<std::cell::RefCell<CellEncoding>>);
impl TextCells {
    pub(super) fn literal(&mut self, value: &str) -> char {
        let mut state = self.0.borrow_mut();
        if let Some(ch) = state.reverse.get(value) {
            return *ch;
        }
        let ch = char::from_u32(0xf0000 + state.next).unwrap();
        state.next += 1;
        state.reverse.insert(value.into(), ch);
        state.glyphs.insert(ch, value.into());
        ch
    }
    pub(super) fn text(&mut self, value: &str) -> String {
        let mut state = self.0.borrow_mut();
        let mut result = String::new();
        for g in value.graphemes(true) {
            if g.is_ascii() && !g.chars().any(char::is_control) {
                result.push_str(g);
                continue;
            }
            if g.chars().any(char::is_control) {
                result.push('�');
                continue;
            }
            let width = g.width();
            if width == 0 {
                continue;
            }
            for cell in 0..width {
                let value = if cell == 0 {
                    g.to_owned()
                } else {
                    String::new()
                };
                if let Some(ch) = state.reverse.get(&value) {
                    result.push(*ch);
                    continue;
                }
                let Some(ch) =
                    char::from_u32(0xf0000 + state.next).filter(|_| state.next < 0x1fffe)
                else {
                    result.push('�');
                    continue;
                };
                state.next += 1;
                state.glyphs.insert(ch, value.clone());
                state.reverse.insert(value, ch);
                result.push(ch);
            }
        }
        result
    }
}
#[derive(Default, Clone, Copy)]
pub(super) enum Appearance {
    #[default]
    Toolkit,
    Button(bool),
    Cluster {
        focused: bool,
        enabled: bool,
    },
    Menu(usize, bool, Option<u16>),
    Input(bool),
    Status(bool),
    Disabled,
    Text,
    SelectedInput,
    DnList(bool),
    History,
    Preview(u8),
}
impl Appearance {
    pub(super) fn group(self) -> crate::palette::theme::Group {
        use crate::palette::theme::Group;
        match self {
            Self::Menu(..) => Group::Menus,
            Self::Status(..) => Group::Status,
            _ => Group::Dialogs,
        }
    }
}
impl Appearance {
    pub(super) fn button_cell(
        self,
        text: &mut String,
        attribute: &mut u8,
        cell: (u16, u16),
        size: (u16, u16),
    ) {
        if let Self::Button(selected) = self {
            button_cell(
                text,
                attribute,
                cell.0,
                cell.1,
                size.0,
                size.1,
                (selected, false),
            );
        }
    }
    /// Shared theme mapping for isolated controls and children of a Dialog.
    pub(super) fn attribute(self, native: u8, profile: PaletteProfile) -> u8 {
        match self {
            Self::Input(selected) => match (profile, selected) {
                (PaletteProfile::ClassicDn | PaletteProfile::Custom(_), true) => 0x0f,
                (PaletteProfile::ClassicDn | PaletteProfile::Custom(_), false) => 0x07,
                (_, true) => 0x1f,
                (_, false) => 0x17,
            },
            Self::Cluster { focused, enabled } => {
                // Upstream Cluster draws marker/label into an otherwise default
                // DrawBuffer. Its padding must share the embedded row background.
                if !enabled {
                    0x78
                } else if native & 15 == 14 {
                    0x7e
                } else if focused {
                    0x7f
                } else {
                    0x70
                }
            }
            Self::Status(enabled) => match profile {
                PaletteProfile::Terminal => {
                    if native == 0x74 {
                        0x07
                    } else if enabled {
                        0x30
                    } else {
                        0x38
                    }
                }
                PaletteProfile::ClassicDn | PaletteProfile::Custom(_) => {
                    if enabled {
                        native
                    } else {
                        0x78
                    }
                }
            },
            Self::Disabled => 0x78,
            Self::Text => 0x70,
            Self::SelectedInput => 0x30,
            Self::DnList(focused) => {
                if native >> 4 == 2 {
                    if focused {
                        0x0f
                    } else {
                        0x3f
                    }
                } else {
                    native
                }
            }
            Self::History => 0x9f,
            Self::Preview(attr) => attr,
            _ => native,
        }
    }
}
pub(crate) struct Native {
    pub(super) theme_group: Option<crate::palette::theme::Group>,
    pub(super) view: Box<dyn View>,
    pub(super) text: TextCells,
    pub(super) appearance: Appearance,
    pub(super) overscan: u16,
    pub(super) inset: u16,
    pub(super) status_text: Option<String>,
}
fn bounds(r: Rect) -> TvRect {
    TvRect::new(
        0,
        0,
        r.width.min(i16::MAX as u16) as i16,
        r.height.min(i16::MAX as u16) as i16,
    )
}
fn focus(view: &mut impl View, selected: bool) {
    view.set_state_flag(State::FOCUSED, selected);
    view.set_state_flag(State::SELECTED, selected);
}
impl Native {
    pub(crate) fn themed(mut self, group: crate::palette::theme::Group) -> Self {
        self.theme_group = Some(group);
        self
    }
}
impl Widget for Native {
    fn is_control(&self) -> bool {
        // Menus use application palette indices, unlike ordinary dialog children.
        // Embed the isolated native painting so a Dialog cannot remap their
        // colours or replace the controller's selected item with group focus.
        !self.view.as_any().is::<turbo_vision::views::MenuBox>()
    }
    fn native(mut self, frame: &mut NativeFrame, area: Rect) {
        if frame.composing() {
            if self.view.as_any().is::<Dialog>() {
                frame.flush_dialog();
            } else {
                frame.stage_control(self, area);
                return;
            }
        }
        if self.view.as_any().is::<Dialog>()
            && (frame.dialog_scope().is_some() || frame.document_owner().is_some())
        {
            frame.start_dialog(self, area);
            return;
        }
        if area.is_empty() {
            return;
        }
        // Compact DN forms reserve one row for buttons. Draw the native face
        // from a two-row Button and clip its bottom shadow to that reservation.
        let height = if self.view.as_any().is::<Button>() {
            area.height.max(2)
        } else {
            area.height
        };
        let width = area.width.saturating_add(self.overscan);
        if let Some(label) = &self.status_text {
            let padding =
                usize::from(area.width).saturating_sub(label.chars().filter(|c| *c != '~').count());
            let label = format!("{label}{}", " ".repeat(padding));
            self.view = Box::new(turbo_vision::views::status_line::StatusLine::new(
                bounds(Rect::new(0, 0, width, height)),
                vec![turbo_vision::views::status_line::StatusItem {
                    text: label,
                    key_code: 0,
                    command: 0xf100,
                }],
            ));
        }
        self.view.set_bounds(bounds(Rect::new(0, 0, width, height)));
        let mut terminal = super::surface::terminal(width, height);
        if !self.view.as_any().is::<Dialog>()
            && !self.view.as_any().is::<turbo_vision::views::MenuBox>()
            && !self
                .view
                .as_any()
                .is::<turbo_vision::views::menu_bar::MenuBar>()
            && !self
                .view
                .as_any()
                .is::<turbo_vision::views::status_line::StatusLine>()
        {
            thread_local! { static PALETTE:turbo_vision::core::palette_chain::PaletteChainNode=turbo_vision::core::palette_chain::PaletteChainNode::new(Dialog::new(TvRect::new(0,0,1,1),"").get_palette(),None); }
            self.view
                .set_palette_chain(Some(PALETTE.with(Clone::clone)));
        }
        terminal.draw_view(&mut *self.view);
        let mut scene = super::scene::Scene {
            width: frame.area().width,
            height: frame.area().height,
            profile: PaletteProfile::ClassicDn,
            cells: BTreeMap::new(),
            regions: vec![],
            cursor: None,
        };
        for y in 0..area.height {
            for x in 0..area.width {
                let Some(cell) = terminal.read_cell((x + self.inset) as i16, y as i16) else {
                    continue;
                };
                let mut text = self
                    .text
                    .0
                    .borrow()
                    .glyphs
                    .get(&cell.ch)
                    .cloned()
                    .unwrap_or_else(|| {
                        if cell.ch == '\0' {
                            String::new()
                        } else if cell.ch.is_control() {
                            "�".into()
                        } else {
                            cell.ch.to_string()
                        }
                    });
                // A scalar DrawBuffer can clip between encoded cells of one
                // wide grapheme. Never let that head overwrite a neighbour.
                if usize::from(x) + text.width() > usize::from(area.width) {
                    text = " ".into();
                }
                let mut attribute = self
                    .appearance
                    .attribute(cell.attr.to_u8(), frame.profile());
                self.appearance.button_cell(
                    &mut text,
                    &mut attribute,
                    (x, y),
                    (area.width, height),
                );
                match self.appearance {
                    Appearance::Menu(row, true, mnemonic)
                        if y as usize == row + 1
                            && x > 0
                            && x + 1 < area.width
                            && attribute != 0x78 =>
                    {
                        // MenuBox paints shortcut text with its normal palette even
                        // on the selected row. Keep its foreground, extend selection.
                        attribute = if mnemonic == Some(x) {
                            0x24
                        } else {
                            (attribute & 15) | 0x20
                        };
                    }
                    _ => {}
                }
                scene.cells.insert(
                    (area.x + x, area.y + y),
                    super::scene::Glyph {
                        text,
                        attribute,
                        style: frame.profile().themed(
                            self.theme_group.unwrap_or_else(|| self.appearance.group()),
                            attribute,
                        ),
                    },
                );
            }
        }
        frame.control_scene(scene);
        if self.view.has_shadow() {
            frame.native_shadow(area);
        }
    }
}
pub(super) fn native(view: impl View + 'static, text: TextCells) -> Native {
    Native {
        theme_group: None,
        view: Box::new(view),
        text,
        appearance: Appearance::Toolkit,
        overscan: 0,
        inset: 0,
        status_text: None,
    }
}
pub(crate) fn dialog(title: &str) -> Native {
    let mut text = TextCells::default();
    let mut dialog = Dialog::new(TvRect::new(0, 0, 1, 1), &text.text(title.trim()));
    dialog.window_mut().set_resizable(false);
    focus(&mut dialog, true);
    native(dialog, text)
}
/// Shared DN-style faces, focus markers and solid black cell shadows.
/// Native Button still owns title layout and command/event handling.
pub(super) fn button_cell(
    text: &mut String,
    attribute: &mut u8,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
    state: (bool, bool),
) {
    let (selected, down) = state;
    if width < 4 || height < 2 {
        return;
    }
    if y + 1 == height {
        *text = " ".into();
        *attribute = if down || x < 2 { 0x70 } else { 0x00 };
    } else if x == 0 || down && x == 1 {
        *text = " ".into();
        *attribute = 0x70;
    } else if x + 1 == width && !down {
        *text = " ".into();
        *attribute = 0x00;
    } else {
        let disabled = *attribute == 0x78;
        let foreground = if disabled {
            7
        } else if selected || *attribute & 15 == 14 {
            14
        } else {
            15
        };
        *attribute = 0x80 | foreground;
        // Keep translated or wide labels intact when a narrow button leaves
        // no spare marker cell. Yellow text still identifies the default.
        if selected && !down && !disabled && text == " " && (x == 1 || x + 2 == width) {
            *text = if x == 1 { "►".into() } else { "◄".into() };
            *attribute = 0x8b;
        }
    }
}
pub(crate) fn button(label: &str, selected: bool) -> Native {
    button_with_default(label, selected, selected)
}
/// The Enter default is visible without taking focus from an input or list.
pub(crate) fn button_with_default(label: &str, focused: bool, default_active: bool) -> Native {
    let mut text = TextCells::default();
    let label = crate::dialog_theme::button_caption(label);
    // Reserve the left owner-coloured cell and center within the actual face.
    let mut view = Button::new(
        TvRect::new(0, 0, 1, 1),
        &text.text(&format!(" {label}")),
        0xf100,
        default_active,
    );
    focus(&mut view, focused);
    let mut control = native(view, text);
    control.appearance = Appearance::Button(focused || default_active);
    control
}
pub(crate) fn checkbox(label: &str, checked: bool, selected: bool, enabled: bool) -> Native {
    let mut text = TextCells::default();
    let mut control = CheckBox::new(TvRect::new(0, 0, 1, 1), &text.text(label));
    control.set_embedded(true);
    control.set_checked(checked);
    focus(&mut control, selected);
    control.set_state_flag(State::DISABLED, !enabled);
    let mut widget = native(control, text);
    widget.appearance = Appearance::Cluster {
        focused: selected,
        enabled,
    };
    widget
}
pub(crate) fn radio(label: &str, checked: bool, selected: bool) -> Native {
    radio_enabled(label, checked, selected, true)
}
pub(crate) fn radio_enabled(label: &str, checked: bool, selected: bool, enabled: bool) -> Native {
    let mut text = TextCells::default();
    let mut control = RadioButton::new(TvRect::new(0, 0, 1, 1), &text.text(label), 1);
    control.set_selected(checked);
    focus(&mut control, selected);
    control.set_state_flag(State::DISABLED, !enabled);
    let mut widget = native(control, text);
    widget.appearance = Appearance::Cluster {
        focused: selected,
        enabled,
    };
    widget
}
pub(crate) fn input(value: &str, selected: bool) -> Native {
    let mut text = TextCells::default();
    let mut control = InputLine::new(TvRect::new(0, 0, 1, 1), 32766);
    focus(&mut control, selected);
    control.set_text(text.text(value));
    let mut result = native(control, text);
    result.appearance = Appearance::Input(selected);
    result
}
pub(crate) fn list(items: Vec<String>, cursor: usize, offset: usize, selected: bool) -> Native {
    list_selection(items, Some(cursor), offset, selected)
}
pub(crate) fn list_selection(
    items: Vec<String>,
    cursor: Option<usize>,
    offset: usize,
    selected: bool,
) -> Native {
    let text = TextCells::default();
    let mut control = ListBox::new(TvRect::new(0, 0, 1, 1), 0xf100);
    let count = items.len();
    struct Items {
        values: Vec<String>,
        pub(super) text: TextCells,
    }
    impl turbo_vision::views::listbox::ListProvider for Items {
        fn len(&self) -> usize {
            self.values.len()
        }
        fn item(&self, index: usize) -> String {
            self.text.clone().text(&self.values[index])
        }
    }
    control.set_provider(Box::new(Items {
        values: items,
        text: text.clone(),
    }));
    control.list_state_mut().focused = cursor
        .filter(|_| count > 0)
        .map(|cursor| cursor.min(count.saturating_sub(1)));
    control.list_state_mut().top_item = offset;
    focus(&mut control, selected);
    native(control, text)
}
pub(crate) fn memo(value: &str) -> Native {
    let mut text = TextCells::default();
    let value = value
        .split('\n')
        .map(|line| text.text(line))
        .collect::<Vec<_>>()
        .join("\n");
    let mut control = Memo::new(TvRect::new(0, 0, 1, 1));
    control.set_read_only(true);
    control.set_text(&value);
    let mut result = native(control, text);
    result.appearance = Appearance::Text;
    result
}

/// Native list navigation, with data supplied lazily so even large result lists
/// need no string allocation to process an arrow key.
pub(crate) fn list_key(
    cursor: usize,
    count: usize,
    rows: usize,
    code: crossterm::event::KeyCode,
) -> Option<usize> {
    use crossterm::event::KeyCode as K;
    let key = match code {
        K::Up => KB_UP,
        K::Down => KB_DOWN,
        K::Home => KB_HOME,
        K::End => KB_END,
        K::PageUp => KB_PGUP,
        K::PageDown => KB_PGDN,
        _ => return None,
    };
    struct Count(usize);
    impl turbo_vision::views::listbox::ListProvider for Count {
        fn len(&self) -> usize {
            self.0
        }
        fn item(&self, _: usize) -> String {
            String::new()
        }
    }
    let mut list = ListBox::new(TvRect::new(0, 0, 1, rows.clamp(1, 32767) as i16), 0xf100);
    list.set_provider(Box::new(Count(count)));
    list.list_state_mut().focused = (count > 0).then_some(cursor.min(count.saturating_sub(1)));
    focus(&mut list, true);
    list.handle_event(&mut Event::keyboard(key));
    Some(list.list_state().focused.unwrap_or(0))
}

use turbo_vision::views::WindowLike;

/// Toolkit clipboard is internal-only (the OS clipboard feature is disabled).
/// Scope and restore it so native paste cannot replace DN's shared clipboard.
pub(crate) fn paste_native(view: &mut impl View, text: &str) {
    static PASTE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _lock = PASTE.lock().unwrap_or_else(|e| e.into_inner());
    struct Restore(String);
    impl Drop for Restore {
        fn drop(&mut self) {
            turbo_vision::core::clipboard::set_clipboard(&self.0);
        }
    }
    let _restore = Restore(turbo_vision::core::clipboard::get_clipboard());
    turbo_vision::core::clipboard::set_clipboard(text);
    view.handle_event(&mut Event::keyboard(KB_CTRL_V));
}
/// Preserve the public byte cursor and byte limit while delegating editing to
/// InputLine. The wide native bounds let us recover its character cursor through
/// update_cursor without depending on private toolkit fields or screen width.
pub(crate) fn edit_input(
    field: &mut String,
    cursor: &mut usize,
    code: crossterm::event::KeyCode,
    paste: Option<&str>,
) {
    use crossterm::event::KeyCode as K;
    if !field.is_char_boundary(*cursor) {
        return;
    }
    let mut input = InputLine::new(TvRect::new(0, 0, 32767, 1), 16384);
    focus(&mut input, true);
    input.set_text(field.as_str());
    let count = field[*cursor..].chars().count();
    for _ in 0..count {
        input.handle_event(&mut Event::keyboard(KB_LEFT));
    }
    if let Some(text) = paste {
        paste_native(&mut input, text);
    } else {
        let mut event = match code {
            K::Char(c) if !c.is_control() => {
                let event = Event::text(c);
                if event.typed_char().is_none() {
                    paste_native(&mut input, &c.to_string());
                    Event::nothing()
                } else {
                    event
                }
            }
            K::Left => Event::keyboard(KB_LEFT),
            K::Right => Event::keyboard(KB_RIGHT),
            K::Home => Event::keyboard(KB_HOME),
            K::End => Event::keyboard(KB_END),
            K::Backspace => Event::keyboard(KB_BACKSPACE),
            K::Delete => Event::keyboard(KB_DEL),
            _ => return,
        };
        input.handle_event(&mut event);
    }
    let result = input.get_text();
    if result.len() > 16384 {
        return;
    }
    thread_local! { static CURSOR: std::cell::RefCell<turbo_vision::terminal::Terminal> = std::cell::RefCell::new(super::surface::terminal(32767,1)); }
    let position = CURSOR.with(|terminal| {
        let mut terminal = terminal.borrow_mut();
        input.update_cursor(&mut terminal);
        terminal.cursor().map_or(0, |p| usize::from(p.0))
    });
    *cursor = result
        .char_indices()
        .nth(position)
        .map_or(result.len(), |(i, _)| i);
    *field = result;
}

#[derive(Clone)]
pub(crate) struct MenuEntry {
    pub label: String,
    pub shortcut: String,
    pub enabled: bool,
    pub checked: bool,
    pub submenu: bool,
}
fn menu_data(entries: Vec<MenuEntry>, text: &mut TextCells) -> turbo_vision::core::menu_data::Menu {
    use turbo_vision::core::menu_data::{Menu, MenuItem};
    Menu::from_items(
        entries
            .into_iter()
            .enumerate()
            .map(|(i, item)| {
                if item.label.is_empty() {
                    MenuItem::Separator
                } else if item.submenu {
                    MenuItem::submenu(&text.text(&item.label), 0, Menu::new(), 0)
                } else {
                    MenuItem::Regular {
                        text: text.text(&item.label),
                        command: 0xe000 + i as u16,
                        key_code: 0,
                        help_ctx: 0,
                        enabled: item.enabled,
                        shortcut: Some(text.text(&item.shortcut)),
                        checked: if item.checked { Some(|| true) } else { None },
                    }
                }
            })
            .collect(),
    )
}
pub(crate) fn menu(entries: Vec<MenuEntry>, selected: usize) -> Native {
    use turbo_vision::{
        core::geometry::Point,
        views::{menu_viewer::MenuViewer, MenuBox},
    };
    let mut text = TextCells::default();
    let enabled = entries.get(selected).is_some_and(|entry| entry.enabled);
    let mnemonic = entries.get(selected).and_then(|entry| {
        let (prefix, suffix) = entry.label.split_once('~')?;
        suffix.chars().next()?;
        Some(2 + prefix.width() as u16)
    });
    // MenuBox consumes a tilde plus the following character, rather than paired
    // markers. Keep only the opening marker; otherwise ~F~ile highlights Fi.
    let entries = entries
        .into_iter()
        .map(|mut entry| {
            let mut first = true;
            entry.label.retain(|ch| {
                if ch != '~' {
                    return true;
                }
                let keep = first;
                first = false;
                keep
            });
            entry
        })
        .collect();
    let mut control = MenuBox::new(Point::new(0, 0), menu_data(entries, &mut text));
    control.set_current_item(Some(selected));
    focus(&mut control, true);
    let mut widget = native(control, text);
    widget.appearance = Appearance::Menu(selected, enabled, mnemonic);
    widget
}
pub(crate) fn menu_title(label: &str, selected: bool, enabled: bool) -> Native {
    use turbo_vision::views::menu_bar::MenuBar;
    let mut text = TextCells::default();
    let data = menu_data(
        vec![MenuEntry {
            label: label.into(),
            shortcut: String::new(),
            enabled,
            checked: false,
            submenu: false,
        }],
        &mut text,
    );
    let mut control = MenuBar::new(TvRect::new(0, 0, 1, 1));
    control.add_submenu(turbo_vision::views::menu_bar::SubMenu::new(
        &text.text(label),
        data,
    ));
    if selected {
        control.handle_event(&mut Event::keyboard(KB_F10));
    }
    control.set_state_flag(State::DISABLED, !enabled);
    let mut result = native(control, text);
    result.theme_group = Some(crate::palette::theme::Group::Menus);
    if !enabled {
        result.appearance = Appearance::Disabled;
    }
    result
}
pub(crate) fn status(label: &str) -> Native {
    use turbo_vision::views::status_line::{StatusItem, StatusLine};
    let mut text = TextCells::default();
    let label = text.text(label);
    let control = StatusLine::new(
        TvRect::new(0, 0, 1, 1),
        vec![StatusItem {
            text: label.clone(),
            key_code: 0,
            command: 0xf100,
        }],
    );
    let mut result = native(control, text);
    result.appearance = Appearance::Status(true);
    result.overscan = 32;
    result.inset = 1;
    result.status_text = Some(label);
    result
}
pub(crate) fn paint_into(scene: &mut super::scene::Scene, widget: Native, area: Rect) {
    let mut frame = NativeFrame::new(Rect::new(0, 0, scene.width, scene.height), scene.profile);
    frame.replace(super::scene::Scene {
        width: scene.width,
        height: scene.height,
        profile: scene.profile,
        cells: std::mem::take(&mut scene.cells),
        regions: Vec::new(),
        cursor: None,
    });
    frame.render_widget(widget, area);
    scene.cells = frame.finish().cells;
}

pub(crate) fn input_scrolled(value: &str, scroll: usize, selected: bool) -> Native {
    let mut x = 0;
    let value = value
        .graphemes(true)
        .filter(|g| {
            let start = x;
            x += g.width();
            start >= scroll
        })
        .collect::<String>();
    input(&value, selected)
}
pub(crate) fn busy() -> Native {
    let mut control = ProgressBar::new(TvRect::new(0, 0, 1, 1), 1);
    control.set_mode(turbo_vision::views::ProgressMode::Marquee);
    control.hide_caption();
    native(control, TextCells::default())
}
pub(crate) fn transfer_progress(value: Option<u8>, width: u16, ticks: u64) -> Native {
    let mut control = ProgressBar::new(
        TvRect::new(0, 0, i16::try_from(width).unwrap_or(i16::MAX), 1),
        100,
    );
    control.hide_caption();
    if let Some(value) = value {
        control.set_value(u64::from(value));
    } else {
        control.set_mode(turbo_vision::views::ProgressMode::Marquee);
        for _ in 0..ticks % (u64::from(width.max(1)) * 2) {
            control.tick();
        }
    }
    native(control, TextCells::default())
}
pub(crate) fn outline(tree: &crate::directory_tree::Tree) -> Native {
    use std::{cell::RefCell, rc::Rc};
    use turbo_vision::views::outline::{Node, OutlineViewer};
    let mut text = TextCells::default();
    let mut roots = Vec::new();
    let mut ancestors: Vec<Rc<RefCell<Node<String>>>> = Vec::new();
    for (index, row) in tree.rows.iter().enumerate() {
        let name = crate::filesystem::display_name(
            &row.path
                .file_name()
                .unwrap_or(row.path.as_os_str())
                .to_string_lossy(),
        );
        let label = if (tree.offset..tree.offset + tree.page_rows).contains(&index) {
            text.text(&format!("{}{}", if row.link { "↗ " } else { "" }, name))
        } else {
            String::new()
        };
        let node = Rc::new(RefCell::new(Node::new(label)));
        node.borrow_mut().expanded = tree.expanded(&row.path);
        if tree.marker(row) == "+" {
            node.borrow_mut()
                .children
                .push(Rc::new(RefCell::new(Node::new(String::new()))));
        }
        ancestors.truncate(row.depth);
        if let Some(parent) = ancestors.last() {
            parent.borrow_mut().children.push(node.clone());
        } else {
            roots.push(node.clone());
        }
        ancestors.push(node);
    }
    let mut control = OutlineViewer::new(TvRect::new(0, 0, 1, 1), |s: &String| s.clone());
    control.set_roots(roots);
    control.list_state_mut().focused = (!tree.rows.is_empty()).then_some(tree.cursor);
    control.list_state_mut().top_item = tree.offset;
    focus(&mut control, tree.focus == 0);
    native(control, text)
}
pub(crate) fn scrollbar(value: usize, max: usize, page: usize) -> Native {
    let mut control =
        turbo_vision::views::scrollbar::ScrollBar::new_vertical(TvRect::new(0, 0, 1, 1));
    control.set_params(
        value.min(i32::MAX as usize) as i32,
        0,
        max.min(i32::MAX as usize) as i32,
        page.min(i32::MAX as usize) as i32,
        1,
    );
    control.set_total(i32::MAX); // DN uses a fixed one-cell thumb.
    native(control, TextCells::default())
}
/// Native arrow/page semantics, with a wide-integer fallback beyond the toolkit's
/// signed range. Thumb dragging retains DN's endpoint-exact geometry adapter.
pub(crate) fn scroll_step(value: usize, max: usize, page: usize, down: bool, paged: bool) -> usize {
    let step = if paged { page } else { 1 };
    if max.saturating_add(step) > i32::MAX as usize {
        return if down {
            value.saturating_add(step).min(max)
        } else {
            value.saturating_sub(step)
        };
    }
    let mut control =
        turbo_vision::views::scrollbar::ScrollBar::new_vertical(TvRect::new(0, 0, 1, 3));
    control.set_params(value as i32, 0, max as i32, page as i32, 1);
    let key = match (down, paged) {
        (false, false) => KB_UP,
        (true, false) => KB_DOWN,
        (false, true) => KB_PGUP,
        (true, true) => KB_PGDN,
    };
    control.handle_event(&mut Event::keyboard(key));
    control.get_value() as usize
}
pub(crate) fn pan(value: &str, scroll: usize) -> String {
    let mut x = 0;
    value
        .graphemes(true)
        .filter(|g| {
            let start = x;
            x += g.width();
            start >= scroll
        })
        .collect()
}

pub(crate) fn menu_row(
    row: usize,
    choices: &[usize],
    code: crossterm::event::KeyCode,
) -> Option<usize> {
    use crossterm::event::KeyCode as K;
    use turbo_vision::views::menu_viewer::MenuViewerState;
    let mut state = MenuViewerState::with_menu(menu_data(
        (0..choices.iter().copied().max().map_or(0, |n| n + 1))
            .map(|i| MenuEntry {
                label: i.to_string(),
                shortcut: String::new(),
                enabled: choices.contains(&i),
                checked: false,
                submenu: false,
            })
            .collect(),
        &mut TextCells::default(),
    ));
    state.current = Some(row);
    match code {
        K::Up => state.select_prev(),
        K::Down => state.select_next(),
        K::Home => return choices.first().copied(),
        K::End => return choices.last().copied(),
        _ => return None,
    }
    state.current
}
pub(crate) fn toggle(checked: bool) -> bool {
    let mut control = CheckBox::new(TvRect::new(0, 0, 1, 1), "");
    control.set_checked(checked);
    focus(&mut control, true);
    control.handle_event(&mut Event::text(' '));
    control.is_checked()
}
pub(crate) fn combo(items: &[String], selected: usize, focused: bool) -> Native {
    let mut text = TextCells::default();
    let mut control = turbo_vision::views::ComboBox::new(TvRect::new(0, 0, 1, 1), 0xf200);
    control.set_items(items.iter().map(|s| text.text(s)).collect());
    control.set_selected(Some(selected));
    focus(&mut control, focused);
    native(control, text)
}

pub(crate) fn function_status(key: &str, label: &str, enabled: bool) -> Native {
    let mut widget = status(&format!("~{key}~{label}"));
    widget.appearance = Appearance::Status(enabled);
    widget
}
pub(crate) fn group_box(title: &str) -> Native {
    let mut text = TextCells::default();
    let control = turbo_vision::views::GroupBox::new(TvRect::new(0, 0, 1, 1), &text.text(title));
    native(control, text)
}
pub(crate) fn selected_input(value: &str, focused: bool) -> Native {
    let mut control = input(value, focused);
    control
        .view
        .as_any_mut()
        .downcast_mut::<InputLine>()
        .unwrap()
        .select_all();
    control.appearance = Appearance::SelectedInput;
    control
}
pub(crate) fn disabled_button(label: &str) -> Native {
    let mut widget = button(label, false);
    widget.view.set_state_flag(State::DISABLED, true);
    widget
}
pub(crate) fn history_button() -> Native {
    // This is an input-row affordance, not a two-row push button. ComboBox
    // supplies the native dropdown arrow without global HistoryManager state.
    let mut widget = combo(&[], 0, false);
    widget.appearance = Appearance::History;
    widget
}

/// Previews receive already bounded and wrapped rows; this never opens a path.
pub(crate) fn text_view(lines: &[String], horizontal: usize, attribute: u8) -> Native {
    let mut text = TextCells::default();
    let content = lines
        .iter()
        .map(|line| text.text(&pan(line, horizontal)))
        .collect::<Vec<_>>()
        .join("\n");
    let mut control = turbo_vision::views::text_viewer::TextViewer::new(TvRect::new(0, 0, 1, 1));
    control.set_text(&content);
    let mut result = native(control, text);
    result.appearance = Appearance::Preview(attribute);
    result
}

/// Focus boundary for custom document surfaces; it never draws over their cells.
pub(super) fn surface() -> Native {
    struct Surface {
        core: turbo_vision::views::ViewCore,
    }
    impl View for Surface {
        fn get_palette(&self) -> Option<turbo_vision::core::palette::Palette> {
            None
        }
        fn core(&self) -> &turbo_vision::views::ViewCore {
            &self.core
        }
        fn core_mut(&mut self) -> &mut turbo_vision::views::ViewCore {
            &mut self.core
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
        fn can_focus(&self) -> bool {
            true
        }
        fn draw(&mut self, _: &mut turbo_vision::terminal::Terminal) {}
        fn handle_event(&mut self, _: &mut Event) {}
    }
    native(
        Surface {
            core: turbo_vision::views::ViewCore::new(TvRect::new(0, 0, 1, 1)),
        },
        TextCells::default(),
    )
}

/// The same native outline type participates in traversal; its populated tree is
/// painted separately so the controller retains expansion and selection state.
pub(super) fn outline_focus() -> Native {
    native(
        turbo_vision::views::outline::OutlineViewer::new(TvRect::new(0, 0, 1, 1), |s: &String| {
            s.clone()
        }),
        TextCells::default(),
    )
}

pub(crate) fn color_grid(value: u8, focused: bool) -> Native {
    let mut control = super::color_grid::ColorGrid::new(value);
    focus(&mut control, focused);
    // Physical color swatches must not inherit editable UI attributes.
    let mut widget = native(control, TextCells::default());
    widget.theme_group = Some(crate::palette::theme::Group::Physical);
    widget
}
pub(crate) fn color_key(value: u8, code: crossterm::event::KeyCode) -> Option<u8> {
    let key = match code {
        crossterm::event::KeyCode::Left => turbo_vision::core::event::KB_LEFT,
        crossterm::event::KeyCode::Right => turbo_vision::core::event::KB_RIGHT,
        crossterm::event::KeyCode::Up => turbo_vision::core::event::KB_UP,
        crossterm::event::KeyCode::Down => turbo_vision::core::event::KB_DOWN,
        _ => return None,
    };
    let mut control = super::color_grid::ColorGrid::new(value);
    control.handle_event(&mut Event::keyboard(key));
    Some(control.selected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyCode as K;
    #[test]
    fn native_drawing_preserves_graphemes_literal_tokens_and_input_cursor() {
        let mut frame = NativeFrame::new(Rect::new(0, 0, 40, 12), PaletteProfile::ClassicDn);
        frame.set_cursor_position((8, 4));
        let text = "e\u{301} 界 👩‍💻 \u{f0000}";
        frame.render_widget(memo(text), Rect::new(2, 2, 30, 2));
        frame.render_widget(button("~О~ткрыть", true), Rect::new(3, 7, 18, 1));
        frame.render_widget(crate::painting::Paragraph::new("Z"), Rect::new(22, 9, 1, 1));
        frame.render_widget(memo("a界"), Rect::new(20, 9, 2, 1));
        let scene = frame.finish();
        assert_eq!(scene.cursor, Some((8, 4)));
        assert_eq!(scene.cells[&(2, 2)].text, "e\u{301}");
        assert_eq!(scene.cells[&(4, 2)].text, "界");
        assert_eq!(scene.cells[&(5, 2)].text, "");
        assert_eq!(scene.cells[&(7, 2)].text, "👩‍💻");
        assert_eq!(scene.cells[&(10, 2)].text, "\u{f0000}");
        assert_eq!(scene.cells[&(22, 9)].text, "Z");
        assert!(scene
            .cells
            .values()
            .all(|c| !c.text.chars().any(char::is_control)));
        assert!(scene.cells.keys().all(|(x, y)| *x < 40 && *y < 12));
    }
    #[test]
    fn menu_selection_covers_shortcut_and_button_shadows_use_solid_cells() {
        for profile in [PaletteProfile::ClassicDn, PaletteProfile::Terminal] {
            let mut frame = NativeFrame::new(Rect::new(0, 0, 50, 15), profile);
            frame.render_widget(
                menu(
                    vec![MenuEntry {
                        label: "Directory ~t~ree".into(),
                        shortcut: "Ctrl+T".into(),
                        enabled: true,
                        checked: false,
                        submenu: false,
                    }],
                    0,
                ),
                Rect::new(2, 2, 35, 3),
            );
            frame.render_widget(button("~O~K", true), Rect::new(2, 7, 12, 2));
            frame.render_widget(button("Cancel", false), Rect::new(18, 7, 12, 2));
            frame.render_widget(disabled_button("~D~elete"), Rect::new(2, 10, 12, 2));
            let scene = frame.finish();
            for x in 3..36 {
                assert_eq!(
                    scene.cells[&(x, 3)].resolved(profile).bg,
                    Some(profile.color(2)),
                    "selected menu cell {x}"
                );
            }
            for (x, width) in [(2, 12), (18, 12)] {
                for dx in 2..width {
                    let shadow = &scene.cells[&(x + dx, 8)];
                    assert_eq!(shadow.text, " ");
                    assert_eq!(shadow.resolved(profile).bg, Some(profile.color(0)));
                }
                assert_eq!(
                    scene.cells[&(x, 8)].resolved(profile).bg,
                    Some(profile.color(7))
                );
            }
            for x in 3..13 {
                assert_eq!(
                    scene.cells[&(x, 7)].resolved(profile).bg,
                    Some(profile.color(8))
                );
            }
            for x in 19..29 {
                assert_eq!(
                    scene.cells[&(x, 7)].resolved(profile).bg,
                    Some(profile.color(8))
                );
            }
            for x in 3..13 {
                assert_eq!(
                    scene.cells[&(x, 10)].resolved(profile).bg,
                    Some(profile.color(8))
                );
                assert_eq!(
                    scene.cells[&(x, 10)].resolved(profile).fg,
                    Some(profile.color(7))
                );
                assert!(!["►", "◄"].contains(&scene.cells[&(x, 10)].text.as_str()));
            }
            assert_eq!(scene.cells[&(3, 7)].text, "►");
            assert_eq!(scene.cells[&(12, 7)].text, "◄");
            assert_eq!(
                scene.cells[&(3, 7)].resolved(profile).fg,
                Some(profile.color(11))
            );
        }
    }
    #[test]
    fn minimum_sized_buttons_keep_both_markers_and_unicode_captions() {
        for profile in [PaletteProfile::ClassicDn, PaletteProfile::Terminal] {
            for label in ["Cancel", "От~м~ена", "~界~e\u{301}"] {
                let width = crate::dialog_theme::minimum_button_width(label);
                let mut frame = NativeFrame::new(Rect::new(0, 0, width + 4, 5), profile);
                frame.render_widget(button(label, true), Rect::new(2, 1, width, 2));
                let scene = frame.finish();
                assert_eq!(scene.cells[&(3, 1)].text, "►");
                assert_eq!(scene.cells[&(width, 1)].text, "◄");
                let text = (2..width + 2)
                    .filter_map(|x| scene.cells.get(&(x, 1)))
                    .map(|g| g.text.as_str())
                    .collect::<String>();
                assert!(text.contains(&label.replace('~', "")), "{text:?}");
            }
        }
    }
    #[test]
    fn native_input_keeps_byte_cursor_unicode_and_limits() {
        let mut field = "a界e\u{301}".to_owned();
        let mut cursor = 1;
        crate::dialog::paste_input(&mut field, &mut cursor, "👩‍💻\n");
        assert_eq!(field, "a👩‍💻界e\u{301}");
        assert_eq!(cursor, "a👩‍💻".len());
        crate::dialog::edit_input(&mut field, &mut cursor, K::Delete);
        assert_eq!(field, "a👩‍💻e\u{301}");
        crate::dialog::edit_input(&mut field, &mut cursor, K::Home);
        crate::dialog::edit_input(&mut field, &mut cursor, K::Char('Ж'));
        assert_eq!(cursor, 2);
        assert!(field.starts_with("Жa"));
        let mut field = "x".repeat(16383);
        let mut cursor = field.len();
        crate::dialog::edit_input(&mut field, &mut cursor, K::Char('界'));
        assert_eq!(field.len(), 16383);
        assert_eq!(cursor, 16383);
        crate::dialog::paste_input(&mut field, &mut cursor, "\x1b\0x");
        assert_eq!(field.len(), 16384);
        assert_eq!(cursor, 16384);
    }
    #[test]
    fn native_navigation_skips_disabled_commands_and_bounds_empty_lists() {
        assert_eq!(menu_row(1, &[1, 3, 7], K::Up), Some(7));
        assert_eq!(menu_row(7, &[1, 3, 7], K::Down), Some(1));
        assert_eq!(menu_row(1, &[1, 3, 7], K::End), Some(7));
        assert_eq!(list_key(0, 0, 4, K::Down), Some(0));
        assert_eq!(list_key(9, 10, 4, K::Down), Some(9));
        assert_eq!(list_key(2, 10, 4, K::PageDown), Some(6));
        assert!(!toggle(true));
        assert!(toggle(false));
    }
    #[test]
    fn native_lists_only_encode_visible_rows() {
        let items = (0..1000)
            .map(|i| char::from_u32(0x4e00 + i).unwrap().to_string())
            .collect();
        let mut control = list(items, 50, 50, true);
        control.view.set_bounds(TvRect::new(0, 0, 10, 3));
        let mut terminal = super::super::surface::terminal(10, 3);
        terminal.draw_view(&mut *control.view);
        // Three visible wide graphemes plus one shared continuation token.
        assert_eq!(control.text.0.borrow().glyphs.len(), 4);
    }
    #[test]
    fn default_button_is_highlighted_without_claiming_keyboard_focus() {
        let button = button_with_default("Continue", false, true);
        assert!(!button.view.is_focused());
        assert!(button
            .view
            .as_any()
            .downcast_ref::<Button>()
            .unwrap()
            .is_default());
        let mut frame = NativeFrame::new(Rect::new(0, 0, 24, 4), PaletteProfile::ClassicDn);
        frame.render_widget(button, Rect::new(2, 1, 18, 2));
        let scene = frame.finish();
        assert_eq!(scene.cells[&(3, 1)].text, "►");
        assert_eq!(scene.cells[&(18, 1)].text, "◄");
        let profile = PaletteProfile::ClassicDn;
        assert_eq!(
            scene.cells[&(3, 1)].resolved(profile).fg,
            Some(profile.color(11))
        );
        assert_eq!(
            scene.cells[&(10, 1)].resolved(profile).fg,
            Some(profile.color(14))
        );
    }
}
