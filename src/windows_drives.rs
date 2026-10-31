//! Windows drive letters. Enumeration never probes media or network roots.
//! The pure strip layout is tested on every host; production callers are Windows-only.
use crate::presentation::layout::Rect;

fn letters(mask: u32) -> Vec<u8> {
    (b'A'..=b'Z')
        .filter(|letter| mask & (1 << (letter - b'A')) != 0)
        .collect()
}

/// Letter hits and the trailing all-drives button share their painting geometry.
/// Keep the current drive visible if a narrow panel cannot fit every letter.
pub(crate) fn strip(area: Rect, mask: u32, current: Option<u8>) -> Vec<(Rect, Option<u8>)> {
    if area.width < 7 || area.height < 2 {
        return Vec::new();
    }
    let capacity = usize::from((area.width - 7) / 2);
    let all = letters(mask);
    let mut visible: Vec<_> = all.iter().copied().take(capacity).collect();
    if let Some(current) = current.filter(|c| all.contains(c)) {
        if !visible.contains(&current) {
            if let Some(last) = visible.last_mut() {
                *last = current;
            }
        }
    }
    visible
        .into_iter()
        .map(Some)
        .chain([None])
        .enumerate()
        .map(|(i, letter)| {
            (
                Rect::new(area.x + 3 + i as u16 * 2, area.bottom() - 1, 1, 1),
                letter,
            )
        })
        .collect()
}

#[cfg(windows)]
pub(crate) use platform::*;
#[cfg(windows)]
mod platform {
    use super::*;
    use crate::{
        app::App,
        mouse::Target,
        navigation_tools::{Action, Modal},
        panel::Panel,
        turbo::dialog_spec::{scope, Control, ControlKind, DialogSpec},
    };
    use std::{
        io,
        path::{Component, Path, PathBuf, Prefix},
        time::{Duration, Instant},
    };

    fn read() -> io::Result<u32> {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetLogicalDrives() -> u32;
        }
        // This reads assigned letters only. Readiness is checked by the existing
        // asynchronous directory scan when the user actually selects a drive.
        let mask = unsafe { GetLogicalDrives() };
        if mask == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(mask)
        }
    }

    #[derive(Default)]
    pub(crate) struct Snapshot {
        pub mask: u32,
        attempted: Option<Instant>,
    }
    impl Snapshot {
        pub fn poll(&mut self, now: Instant) -> bool {
            if self
                .attempted
                .is_some_and(|last| now.saturating_duration_since(last) < Duration::from_secs(2))
            {
                return false;
            }
            self.attempted = Some(now);
            if let Ok(mask) = read() {
                let changed = self.mask != mask;
                self.mask = mask;
                changed
            } else {
                false
            }
        }
    }
    pub(crate) fn current(path: &Path) -> Option<u8> {
        match path.components().next()? {
            Component::Prefix(prefix) => match prefix.kind() {
                Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
                    Some(letter.to_ascii_uppercase())
                }
                _ => None,
            },
            _ => None,
        }
    }
    pub(crate) fn paint(scene: &mut crate::turbo::scene::Scene, panel: &Panel, area: Rect) {
        let current = current(&panel.path);
        let hits = super::strip(area, panel.drives.mask, current);
        let Some((last, _)) = hits.last() else {
            return;
        };
        let x = area.x + 1;
        let width = last.right() + 2 - x;
        scene.fill(x, last.y, width, 1, 0x8f);
        scene.text(x, last.y, 1, "[", 0x8f);
        scene.text(last.right() + 1, last.y, 1, "]", 0x8f);
        for (rect, letter) in hits {
            let text = letter.map_or_else(|| "*".into(), |c| char::from(c).to_string());
            scene.text(
                rect.x,
                rect.y,
                rect.width,
                &text,
                if letter.is_some() && letter == current {
                    0x8e
                } else {
                    0x8f
                },
            );
        }
    }
    fn available(app: &App, panel: usize) -> bool {
        panel < app.panels.len()
            && app.desktop.target() == crate::desktop::WindowId::MANAGER
            && !crate::windowing::blocked(app)
            && !app.panels[panel].loading
    }
    pub(crate) fn navigate(app: &mut App, panel: usize, letter: u8) {
        if !available(app, panel) || !letter.is_ascii_uppercase() {
            return;
        }
        crate::menu::close(app);
        // Recheck assignment so a removed drive cannot become a stale action.
        match read() {
            Ok(mask) if mask & (1 << (letter - b'A')) != 0 => {
                app.panels[panel].drives.mask = mask;
                app.active = panel;
                app.quick_view = None;
                app.modals.set_quick_search(None);
                app.panels[panel].go_to(
                    PathBuf::from(format!("{}:\\", char::from(letter))),
                    app.config.show_hidden,
                );
            }
            Ok(_) => app.error(app.config.language.text("Drive is no longer available.")),
            Err(error) => app.error(error.to_string()),
        }
    }
    pub(crate) fn open(app: &mut App, panel: usize) {
        if !available(app, panel) {
            return;
        }
        let mask = match read() {
            Ok(mask) => mask,
            Err(error) => {
                app.error(error.to_string());
                return;
            }
        };
        crate::menu::close(app);
        app.active = panel;
        app.quick_view = None;
        app.modals.set_quick_search(None);
        app.panels[panel].drives.mask = mask;
        let letters = super::letters(mask);
        let focus = current(&app.panels[panel].path)
            .and_then(|c| letters.iter().position(|letter| *letter == c))
            .unwrap_or(0);
        app.modals.set_navigation_dialog(Some(Modal::Drives {
            panel,
            letters,
            focus,
        }));
    }
    pub(crate) fn key(app: &mut App, event: crossterm::event::KeyEvent) {
        use crossterm::event::{KeyCode as K, KeyModifiers as M};
        let area = crate::turbo::dialog_spec::input_area(app);
        let columns = usize::from(area.width.min(52).saturating_sub(4) / 8).clamp(1, 6);
        let Some(Modal::Drives { letters, focus, .. }) = app.navigation_dialog_mut() else {
            return;
        };
        if event.modifiers.intersects(M::ALT | M::CONTROL) {
            return;
        }
        let count = letters.len() + 1;
        match event.code {
            K::Left => *focus = focus.saturating_sub(1),
            K::Right => *focus = (*focus + 1).min(count - 1),
            K::Up => *focus = focus.saturating_sub(columns),
            K::Down => *focus = (*focus + columns).min(count - 1),
            K::Home => *focus = 0,
            K::End => *focus = count - 1,
            K::Char(c) if c.is_ascii_alphabetic() => {
                let letter = c.to_ascii_uppercase() as u8;
                if letters.contains(&letter) {
                    crate::navigation_tools::action(app, Action::Drive(letter));
                }
            }
            _ => {}
        }
    }
    pub(crate) fn spec<'a>(
        app: &'a App,
        area: Rect,
        letters: &[u8],
        focus: usize,
    ) -> Option<DialogSpec<'a>> {
        let rect = crate::dialog_theme::centered(area, 52, 15);
        let default = letters
            .get(focus)
            .map_or(Action::Close, |c| Action::Drive(*c));
        let mut spec = DialogSpec::new(
            rect,
            scope(app, crate::focus::Focus::Navigation)?,
            focus,
            app.config.language.text(" Change drive "),
            Target::Navigation(default),
            Target::Navigation(Action::Close),
        );
        let columns = usize::from(rect.width.saturating_sub(4) / 8).clamp(1, 6);
        let rows = usize::from(rect.height.saturating_sub(5) / 2).max(1);
        let offset = (focus.min(letters.len().saturating_sub(1)) / columns)
            .saturating_sub(rows - 1)
            * columns;
        for (i, letter) in letters.iter().enumerate().skip(offset).take(columns * rows) {
            let slot = i - offset;
            spec.control(Control::new(
                i,
                Rect::new(
                    rect.x + 2 + (slot % columns) as u16 * 8,
                    rect.y + 2 + (slot / columns) as u16 * 2,
                    7,
                    1,
                ),
                ControlKind::Button(format!("[ {} ]", char::from(*letter))),
                Target::Navigation(Action::Drive(*letter)),
            ));
        }
        spec.control(Control::new(
            letters.len(),
            Rect::new(
                rect.x + rect.width.saturating_sub(18) / 2,
                rect.bottom().saturating_sub(3),
                18.min(rect.width),
                1,
            ),
            ControlKind::Button(app.config.language.text("[ Cancel ]").into()),
            Target::Navigation(Action::Close),
        ));
        Some(spec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[test]
    fn native_path_prefixes_distinguish_drives_from_unc_and_relative_paths() {
        use std::path::Path;
        for (path, expected) in [
            (r"C:\music", Some(b'C')),
            (r"d:\", Some(b'D')),
            (r"\\?\Z:\music", Some(b'Z')),
            (r"\\server\share", None),
            (r"\\?\UNC\server\share", None),
            ("relative", None),
        ] {
            assert_eq!(platform::current(Path::new(path)), expected, "{path}");
        }
    }
    #[test]
    fn assigned_letters_only_in_alphabetical_order() {
        assert_eq!(letters((1 << 2) | (1 << 25) | (1 << 31)), b"CZ");
        assert!(letters(0).is_empty());
        assert_eq!(letters(u32::MAX), (b'A'..=b'Z').collect::<Vec<_>>());
    }
    #[test]
    fn reference_strip_and_overflow_preserve_current_letter() {
        let area = Rect::new(0, 1, 40, 22);
        let strip = strip(area, (1 << 2) | (1 << 25), Some(b'C'));
        assert_eq!(
            strip,
            vec![
                (Rect::new(3, 22, 1, 1), Some(b'C')),
                (Rect::new(5, 22, 1, 1), Some(b'Z')),
                (Rect::new(7, 22, 1, 1), None)
            ]
        );
        let narrow = strip_at_width(11, u32::MAX, Some(b'Z'));
        assert_eq!(
            narrow.iter().filter_map(|(_, c)| *c).collect::<Vec<_>>(),
            b"AZ"
        );
        assert_eq!(narrow.last().unwrap().1, None);
    }
    fn strip_at_width(width: u16, mask: u32, current: Option<u8>) -> Vec<(Rect, Option<u8>)> {
        strip(Rect::new(3, 2, width, 20), mask, current)
    }
    #[test]
    fn narrow_and_empty_strips_stay_inside_bottom_border() {
        for width in 0..160 {
            for mask in [0, 4, u32::MAX] {
                let a = strip_at_width(width, mask, Some(b'Z'));
                assert_eq!(a, strip_at_width(width, mask, Some(b'Z')));
                for (rect, _) in a {
                    assert!(rect.x > 3 && rect.right() + 2 < 3 + width);
                    assert_eq!(rect.y, 21);
                }
            }
        }
        assert!(strip(Rect::new(0, 0, 80, 1), 4, None).is_empty());
    }
}
