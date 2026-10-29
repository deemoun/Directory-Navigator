//! Topic help following DN's context help, cross-references and Alt+F1 history.
//! The pinned toolkit HelpWindow is driven by Session, never execute_modal.
use super::{
    scene::{Glyph, Scene},
    widgets::TextCells,
};
use crate::{
    localization::Language,
    mouse::Target,
    palette::PaletteProfile,
    presentation::layout::{Rect, Size},
    shortcuts::Profile,
};
use crossterm::event::{
    KeyCode as K, KeyEvent, KeyModifiers as M, MouseButton, MouseEvent, MouseEventKind,
};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};
use turbo_vision::{
    core::{
        event::*,
        geometry::{Point, Rect as TvRect},
        state::State,
    },
    views::{help_file::HelpFile, help_window::HelpWindow, View, WindowLike},
};
pub struct Browser {
    title: String,
    body: String,
    window: Option<HelpWindow>,
    text: TextCells,
    scene: Option<Scene>,
    rect: Rect,
    placement: Option<Rect>,
    language: Option<Language>,
    pub revision: u64,
    last_link: Option<(String, std::time::Instant)>,
    landing: Option<Vec<(&'static str, &'static str)>>,
    selection: Option<usize>,
}
impl Browser {
    pub fn new(title: &str, body: &str) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            window: None,
            text: TextCells::default(),
            scene: None,
            rect: Rect::default(),
            placement: None,
            language: None,
            revision: 0,
            last_link: None,
            landing: None,
            selection: None,
        }
    }
    pub(crate) fn landing(title: &str, actions: Vec<(&'static str, &'static str)>) -> Self {
        let mut browser = Self::new(title, "");
        browser.landing = Some(actions);
        browser
    }
    pub(crate) fn set_body(&mut self, body: String) {
        if self.body != body {
            self.body = body;
            self.window = None;
            self.revision += 1;
        }
    }
    pub(crate) fn select_link(&mut self, index: usize) {
        if self.selection != Some(index) {
            self.selection = Some(index);
            self.revision += 1;
        }
        if let Some(window) = &mut self.window {
            let (delta, _) = window.viewer().get_scroll_state();
            window.viewer_mut().set_scroll_state(delta, index + 1);
        }
    }
    pub(crate) fn link_regions(&mut self) -> Vec<(String, Rect)> {
        let Some(window) = &mut self.window else {
            return Vec::new();
        };
        let (delta, selected) = window.viewer().get_scroll_state();
        let body = Rect::new(
            self.rect.x + 1,
            self.rect.y + 1,
            self.rect.width - 3,
            self.rect.height - 2,
        );
        let mut links = Vec::new();
        let count = window.viewer().num_cross_refs();
        for index in 1..=count {
            window.viewer_mut().set_scroll_state(delta, index);
            if let Some(link) = window.viewer().get_selected_cross_ref() {
                let x = body.x as i16 + link.offset - delta.x;
                let y = body.y as i16 + link.line - 1 - delta.y;
                let left = x.max(body.x as i16);
                let right = (x + i16::from(link.length)).min(body.right() as i16);
                if y >= body.y as i16 && y < body.bottom() as i16 && right > left {
                    links.push((
                        link.target.clone(),
                        Rect::new(left as u16, y as u16, (right - left) as u16, 1),
                    ));
                }
            }
        }
        window.viewer_mut().set_scroll_state(delta, selected);
        links
    }
    pub fn topic(&self) -> Option<String> {
        self.window.as_ref().and_then(|w| w.current_topic())
    }
    pub fn offset(&self) -> usize {
        self.window
            .as_ref()
            .map_or(0, |w| w.viewer().get_scroll_state().0.y.max(0) as usize)
    }
    pub(crate) fn place(&mut self, r: Rect) {
        if self.placement != Some(r) {
            self.placement = Some(r);
            self.revision += 1;
        }
    }
    pub(crate) fn bounds(&self) -> Rect {
        self.rect
    }
    pub fn scene(&self) -> Option<&Scene> {
        self.scene.as_ref()
    }
    pub fn prepare(
        &mut self,
        size: Size,
        profile: PaletteProfile,
        language: Language,
        _shortcuts: Profile,
    ) {
        if size.width < 80 || size.height < 25 {
            self.scene = None;
            return;
        }
        // HELPFILE.PAS centers a 72×20 help window.
        let rect = self.placement.unwrap_or(Rect::new(
            (size.width - 72) / 2,
            (size.height - 20) / 2,
            72,
            20,
        ));
        if self.window.is_none() || self.language != Some(language) {
            self.text = TextCells::default();
            let body = crate::painting::wrap_plain(language.text(&self.body), 66).join("\n");
            let contents = language.text("Contents");
            let pages=[("panels",language.text("File manager"),"Tab switches panels. Arrows and page keys move through entries. Insert/Space marks files. Enter opens; Backspace goes to the parent. Ctrl+Q toggles quick view. Ctrl+T opens the directory tree. Ctrl+O reveals terminal output (Mac: Ctrl+O twice); Ctrl+O, Esc or Enter returns."),("editor",language.text("Editor"),"F2 saves; Shift+F2 saves to a new destination. F3 opens another document. F7 finds text; Ctrl+F7 replaces. Alt+0 switches documents. Close and quit review unsaved changes."),("viewer",language.text("Viewer"),"F2 toggles wrapping; F7 finds text. Shift+F7 repeats; Ctrl+F7 searches backward. Arrows/page keys scroll. Esc returns. Previews remain bounded and read-only."),("operations",language.text("File operations"),"F5 copies; F6 moves or renames; F7 creates a directory. F8 uses Trash or confirmed permanent deletion. Existing destinations are refused. Esc cancels pending work; completed items are retained.")];
            let links = pages
                .iter()
                .map(|(id, title, _)| format!("[{title}](#{id})"))
                .collect::<Vec<_>>()
                .join("\n");
            let mut content=format!("# {} {{#context}}\n{body}\n\n[{contents}](#contents)\n\n# {contents} {{#contents}}\n[{}](#context)\n{links}\n",language.text(&self.title),language.text("Back"));
            for (id, title, body) in pages {
                content.push_str(&format!(
                    "\n# {title} {{#{id}}}\n{}\n\n[{contents}](#contents)\n",
                    language.text(body)
                ));
            }
            if let Some(actions) = &self.landing {
                let actions = actions
                    .iter()
                    .map(|(label, target)| {
                        format!("[{}](#{target})", crate::menu::plain(language.text(label)))
                    })
                    .collect::<Vec<_>>()
                    .join("   ");
                content = format!(
                    "# {} {{#context}}\n{actions}\n\n{body}\n",
                    language.text(&self.title)
                );
            }
            let encoded = content
                .lines()
                .map(|line| self.text.text(line))
                .collect::<Vec<_>>()
                .join("\n");
            // The pinned parser scans italic '*' by byte. Escape literal
            // arithmetic/wildcard characters into the same grapheme cell plane.
            let star = self.text.literal("*");
            let tick = self.text.literal("`");
            let encoded = encoded
                .replace('*', &star.to_string())
                .replace('`', &tick.to_string());
            let file = Rc::new(RefCell::new(HelpFile::from_content(&encoded)));
            let title = self.text.text(language.text(&self.title));
            let mut window = HelpWindow::new(TvRect::new(0, 0, 72, 20), &title, file);
            window.window_mut().set_resizable(self.landing.is_none());
            window.set_state_flag(State::MODAL, true);
            window.show_topic("context");
            self.window = Some(window);
            self.language = Some(language);
        }
        if let Some(index) = self.selection {
            let window = self.window.as_mut().unwrap();
            let (delta, _) = window.viewer().get_scroll_state();
            window.viewer_mut().set_scroll_state(delta, index + 1);
        }
        let window = self.window.as_mut().unwrap();
        // Set Window bounds directly: the pinned HelpWindow override uses absolute
        // child coordinates; its viewer must stay interior-relative for Group.
        window.window_mut().set_bounds(TvRect::new(
            rect.x as i16,
            rect.y as i16,
            rect.right() as i16,
            rect.bottom() as i16,
        ));
        window.viewer_mut().set_bounds(TvRect::new(
            0,
            0,
            (rect.width - 2) as i16,
            (rect.height - 2) as i16,
        ));
        window.viewer_mut().set_focus(true);
        let mut terminal = super::surface::terminal(size.width, size.height);
        terminal.draw_view(window);
        let mut scene = Scene {
            width: size.width,
            height: size.height,
            profile,
            cells: BTreeMap::new(),
            regions: vec![(
                Rect::new(rect.x + 2, rect.y, 3, 1),
                if self.title.contains("Editor") {
                    Target::Editor(crate::editor::EditorAction::Cancel)
                } else {
                    Target::CloseDialog
                },
            )],
            cursor: None,
        };
        for y in rect.y..rect.bottom() {
            for x in rect.x..rect.right() {
                let cell = terminal.read_cell(x as i16, y as i16).unwrap();
                let text = self
                    .text
                    .0
                    .borrow()
                    .glyphs
                    .get(&cell.ch)
                    .cloned()
                    .unwrap_or_else(|| {
                        if cell.ch == '\0' {
                            String::new()
                        } else {
                            cell.ch.to_string()
                        }
                    });
                scene.cells.insert(
                    (x, y),
                    Glyph {
                        text,
                        attribute: cell.attr.to_u8(),
                        style: None,
                    },
                );
            }
        }
        self.scene = Some(scene);
        if self.rect != rect {
            self.revision += 1;
        }
        self.rect = rect;
    }
    pub fn key(&mut self, key: KeyEvent) -> bool {
        self.last_link = None;
        let Some(window) = self.window.as_mut() else {
            return key.code == K::Esc;
        };
        if key.code == K::Esc {
            return true;
        }
        if key.code == K::Enter && window.viewer().get_selected_target().is_none() {
            return true;
        }
        if key.code == K::F(1) && !key.modifiers.contains(M::ALT) {
            window.switch_to_topic("contents");
        } else {
            let code = match key.code {
                K::F(1) if key.modifiers.contains(M::ALT) => KB_ALT_F1,
                K::Backspace => KB_BACKSPACE,
                K::Tab => KB_TAB,
                K::BackTab => KB_SHIFT_TAB,
                K::Enter => KB_ENTER,
                K::Up => KB_UP,
                K::Down => KB_DOWN,
                K::PageUp => KB_PGUP,
                K::PageDown => KB_PGDN,
                K::Home => KB_HOME,
                K::End => KB_END,
                K::Left => KB_LEFT,
                K::Right => KB_RIGHT,
                _ => return false,
            };
            window.handle_event(&mut Event::keyboard(code));
        }
        self.revision += 1;
        false
    }
    pub fn mouse(&mut self, event: MouseEvent, offset: (u16, u16)) -> bool {
        let x = event.column.saturating_sub(offset.0);
        let y = event.row.saturating_sub(offset.1);
        if event.kind == MouseEventKind::Down(MouseButton::Left)
            && Rect::new(self.rect.x + 2, self.rect.y, 3, 1).contains((x, y).into())
        {
            return true;
        }
        let Some(window) = self.window.as_mut() else {
            return false;
        };
        if !self.rect.contains((x, y).into()) {
            return false;
        }
        let pos = Point::new(x as i16 - self.rect.x as i16, y as i16 - self.rect.y as i16);
        if event.kind == MouseEventKind::Down(MouseButton::Left) {
            let selected = window
                .viewer()
                .get_cross_ref_at_public(pos.x - 1, pos.y - 1);
            if selected > 0 {
                let delta = window.viewer().get_scroll_state().0;
                window.viewer_mut().set_scroll_state(delta, selected);
                let target = window.viewer().get_selected_target().map(str::to_owned);
                if let Some(target) = target {
                    let now = std::time::Instant::now();
                    let double = self.last_link.as_ref().is_some_and(|(last, time)| {
                        *last == target
                            && now.duration_since(*time) < std::time::Duration::from_millis(400)
                    });
                    self.last_link = Some((target.clone(), now));
                    if double {
                        window.switch_to_topic(&target);
                        self.last_link = None;
                    }
                }
                self.revision += 1;
                return false;
            }
        }
        let what = match event.kind {
            MouseEventKind::ScrollUp => EventType::MouseWheelUp,
            MouseEventKind::ScrollDown => EventType::MouseWheelDown,
            MouseEventKind::Down(MouseButton::Left) => EventType::MouseDown,
            _ => return false,
        };
        window.handle_event(&mut Event::mouse(what, pos, MB_LEFT_BUTTON, false));
        self.revision += 1;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn links_select_on_single_click_follow_on_double_and_restore_history() {
        let mut help = Browser::new("Help", "Context");
        help.prepare(
            Size::new(100, 35),
            PaletteProfile::ClassicDn,
            Language::English,
            Profile::ClassicDn,
        );
        help.key(KeyEvent::new(K::F(1), M::NONE));
        help.prepare(
            Size::new(100, 35),
            PaletteProfile::ClassicDn,
            Language::English,
            Profile::ClassicDn,
        );
        let scene = help.scene().unwrap();
        let (x, y) = (0..scene.height)
            .find_map(|y| {
                let line = (0..scene.width)
                    .map(|x| scene.cells.get(&(x, y)).map_or(" ", |g| g.text.as_str()))
                    .collect::<String>();
                line.find("File manager").map(|x| (x as u16, y))
            })
            .expect("contents link");
        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: x + 7,
            row: y + 3,
            modifiers: M::NONE,
        };
        assert!(!help.mouse(click, (7, 3)));
        assert_eq!(help.topic().as_deref(), Some("contents"));
        assert!(!help.mouse(click, (7, 3)));
        assert_eq!(help.topic().as_deref(), Some("panels"));
        help.key(KeyEvent::new(K::F(1), M::ALT));
        assert_eq!(help.topic().as_deref(), Some("contents"));
    }
}
