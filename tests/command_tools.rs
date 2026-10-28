use crossterm::event::{KeyCode, KeyModifiers};
use dn::{
    app::App,
    command_line::{
        self,
        history::{self, Action},
    },
    file_tools::clipboard,
    focus::{self, Focus},
    input, menu,
    mouse::Target,
};
use std::{fs, io, path::Path};
mod support;
use support::{click, modified as key, Fixture as Temp};

fn plain(a: &mut App, c: KeyCode) {
    key(a, c, KeyModifiers::NONE);
}
fn remember(a: &mut App, s: &str) {
    command_line::paste(a, s);
    plain(a, KeyCode::Enter);
    a.command_line.pending = None;
}
fn draw(app: &mut App, w: u16, h: u16) -> io::Result<String> {
    support::text(app, w, h)
}

#[test]
fn history_order_dedup_limit_filter_and_safe_recall() -> io::Result<()> {
    let t = Temp::new("command-tools")?;
    let mut a = t.app()?;
    for i in 0..105 {
        remember(&mut a, &format!("echo {i}"));
    }
    assert_eq!(a.command_line.history().len(), 100);
    assert_eq!(a.command_line.history()[0], "echo 104");
    assert_eq!(a.command_line.history()[99], "echo 5");
    remember(&mut a, "echo 8");
    assert_eq!(a.command_line.history()[0], "echo 8");
    assert_eq!(
        a.command_line
            .history()
            .iter()
            .filter(|s| *s == "echo 8")
            .count(),
        1
    );
    remember(&mut a, "  echo ПрИвЕт 世界  ");
    let snapshot = a.command_line.history().to_vec();
    command_line::paste(&mut a, "draft");
    plain(&mut a, KeyCode::Left);
    let cursor = a.command_line.cursor;
    key(&mut a, KeyCode::F(8), KeyModifiers::ALT);
    assert_eq!(focus::current(&a), Focus::CommandHistory);
    input::paste(&mut a, "привет");
    assert_eq!(
        a.command_line
            .modals
            .command_history()
            .unwrap()
            .matches
            .len(),
        1
    );
    assert_eq!(a.command_line.history(), snapshot);
    plain(&mut a, KeyCode::Esc);
    assert_eq!(a.command_line.text, "draft");
    assert_eq!(a.command_line.cursor, cursor);
    history::open(&mut a);
    input::paste(&mut a, "世界");
    plain(&mut a, KeyCode::Enter);
    assert_eq!(a.command_line.text, "  echo ПрИвЕт 世界  ");
    assert_eq!(a.command_line.cursor, a.command_line.text.len());
    assert!(a.command_line.pending.is_none());
    assert_eq!(a.command_line.history(), snapshot);
    // Existing recall still returns to the newly restored prompt draft.
    key(&mut a, KeyCode::Char('e'), KeyModifiers::CONTROL);
    key(&mut a, KeyCode::Char('x'), KeyModifiers::CONTROL);
    assert_eq!(a.command_line.text, "  echo ПрИвЕт 世界  ");
    Ok(())
}
#[test]
fn history_empty_editing_navigation_resize_mouse_and_modal_precedence() -> io::Result<()> {
    let t = Temp::new("command-tools")?;
    let mut a = t.app()?;
    history::open(&mut a);
    assert!(draw(&mut a, 80, 25)?.contains("No commands"));
    assert!(!a
        .mouse
        .regions
        .iter()
        .any(|(_, t)| *t == Target::CommandHistory(Action::Use)));
    plain(&mut a, KeyCode::Enter);
    assert!(a.command_line.modals.command_history().is_some());
    plain(&mut a, KeyCode::Esc);
    for i in 0..30 {
        remember(&mut a, &format!("echo {i}"));
    }
    history::open(&mut a);
    draw(&mut a, 80, 25)?;
    let rows = a.command_line.modals.command_history().unwrap().rows;
    plain(&mut a, KeyCode::PageDown);
    assert_eq!(
        a.command_line.modals.command_history().unwrap().cursor,
        rows
    );
    plain(&mut a, KeyCode::End);
    assert_eq!(a.command_line.modals.command_history().unwrap().cursor, 29);
    draw(&mut a, 120, 40)?;
    plain(&mut a, KeyCode::Home);
    assert_eq!(a.command_line.modals.command_history().unwrap().cursor, 0);
    input::paste(&mut a, "absent👩‍💻");
    assert!(draw(&mut a, 80, 25)?.contains("No matching"));
    plain(&mut a, KeyCode::Backspace);
    assert_eq!(
        a.command_line.modals.command_history().unwrap().query,
        "absent"
    );
    input::paste(&mut a, "\n");
    assert_eq!(focus::current(&a), Focus::Message);
    plain(&mut a, KeyCode::Esc);
    assert_eq!(focus::current(&a), Focus::CommandHistory);
    plain(&mut a, KeyCode::Esc);
    history::open(&mut a);
    draw(&mut a, 80, 25)?;
    click(&mut a, Target::CommandHistory(Action::Row(3)));
    assert!(a.command_line.modals.command_history().is_some());
    click(&mut a, Target::CommandHistory(Action::Row(3)));
    assert!(a.command_line.modals.command_history().is_none());
    assert_eq!(a.command_line.text, "echo 26");
    assert!(a.command_line.pending.is_none());
    history::open(&mut a);
    draw(&mut a, 120, 40)?;
    click(&mut a, Target::CommandHistory(Action::Cancel));
    assert_eq!(a.command_line.text, "echo 26");
    a.show_viewer(dn::viewer::Viewer::from_contents(
        t.0.join("test"),
        dn::viewer::from_bytes(b"text".to_vec(), dn::viewer::ViewMode::Text),
    ));
    key(&mut a, KeyCode::F(8), KeyModifiers::ALT);
    assert!(a.command_line.modals.command_history().is_none());
    a.desktop.close_viewer();
    dn::editor::open(&mut a, true);
    key(&mut a, KeyCode::F(8), KeyModifiers::ALT);
    assert!(a.command_line.modals.command_history().is_none());
    Ok(())
}
#[test]
fn copy_selection_order_filters_and_clipboard_failures() -> io::Result<()> {
    let t = Temp::new("command-tools")?;
    fs::create_dir(t.0.join("folder"))?;
    fs::write(t.0.join("a 世界.txt"), "")?;
    fs::write(t.0.join("b.txt"), "")?;
    let mut a = t.app()?;
    let p = &mut a.panels[0];
    p.cursor = p
        .entries
        .iter()
        .position(|e| e.name == "a 世界.txt")
        .unwrap();
    clipboard::copy(&mut a, false);
    assert_eq!(a.editor_clipboard, "a 世界.txt");
    clipboard::copy(&mut a, true);
    assert_eq!(a.editor_clipboard, t.0.join("a 世界.txt").to_str().unwrap());
    a.panels[0].select_visible();
    let names = a.panels[0]
        .entries
        .iter()
        .filter(|e| !e.parent)
        .map(|e| e.name.to_str().unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    let cursor = a.panels[0].cursor;
    let marks = a.panels[0].selected.clone();
    clipboard::copy(&mut a, false);
    assert_eq!(a.editor_clipboard, names);
    assert!(!a.editor_clipboard.ends_with('\n'));
    assert_eq!(a.panels[0].cursor, cursor);
    assert_eq!(a.panels[0].selected, marks);
    a.panels[0].set_filter("b*".into());
    clipboard::copy(&mut a, false);
    assert_eq!(a.editor_clipboard, "b.txt");
    a.panels[0].set_filter("no-match".into());
    clipboard::copy(&mut a, false);
    assert_eq!(a.editor_clipboard, "b.txt");
    assert!(a.last_dialog().unwrap().message.contains("filter"));
    a.clear_dialogs();
    a.panels[0].selected.clear();
    a.panels[0].set_filter(String::new());
    a.panels[0].cursor = 0;
    clipboard::copy(&mut a, false);
    assert_eq!(a.editor_clipboard, "b.txt");
    assert_eq!(
        clipboard::encode([Path::new("literal ' $name")], false)?.0,
        "literal ' $name"
    );
    assert!(clipboard::encode([Path::new("a\nb")], false).is_err());
    assert!(clipboard::encode([Path::new("a\rb")], true).is_err());
    let large = "x".repeat(clipboard::MAX_BYTES);
    assert_eq!(
        clipboard::encode([Path::new(&large)], false)?.0.len(),
        clipboard::MAX_BYTES
    );
    assert!(clipboard::encode([Path::new(&large), Path::new("x")], false).is_err());
    Ok(())
}
#[cfg(unix)]
#[test]
fn undecodable_and_newline_native_entries_do_not_replace_clipboard() -> io::Result<()> {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let t = Temp::new("command-tools")?;
    let bad = OsString::from_vec(vec![b'x', 0xff]);
    fs::write(t.0.join("normal"), "")?;
    fs::write(t.0.join("newline\nfile"), "")?;
    let mut a = t.app()?;
    a.editor_clipboard = "keep".into();
    let index = a.panels[0]
        .entries
        .iter()
        .position(|e| e.name == "normal")
        .unwrap();
    a.panels[0].entries[index].name = bad.clone();
    a.panels[0].entries[index].path = t.0.join(&bad);
    for name in [bad, OsString::from("newline\nfile")] {
        a.panels[0].cursor = a.panels[0]
            .entries
            .iter()
            .position(|e| e.name == name)
            .unwrap();
        for full in [true, false] {
            clipboard::copy(&mut a, full);
            assert_eq!(a.editor_clipboard, "keep");
            assert!(!a.dialogs().is_empty());
            a.clear_dialogs();
        }
    }
    Ok(())
}
#[test]
fn menu_actions_and_translated_geometry() -> io::Result<()> {
    let t = Temp::new("command-tools")?;
    let mut a = t.app()?;
    assert!(menu::items(2)
        .iter()
        .any(|i| i.action == menu::Action::CommandHistory));
    assert_eq!(menu::submenu_items(menu::MenuId::CopyNames).len(), 2);
    for language in [
        dn::localization::Language::English,
        dn::localization::Language::Russian,
    ] {
        a.config.language = language;
        menu::execute(&mut a, menu::Action::CommandHistory);
        for (w, h) in [(80, 25), (120, 40), (80, 25)] {
            draw(&mut a, w, h)?;
        }
        history::action(&mut a, Action::Cancel);
    }
    Ok(())
}
