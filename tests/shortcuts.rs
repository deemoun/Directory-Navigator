use crossterm::event::{
    KeyCode as K, KeyEvent, KeyModifiers as M, MouseButton, MouseEvent, MouseEventKind,
};
use dn::{
    app::App,
    config::{Config, Store},
    focus::Screen,
    input, menu, mouse,
    settings::{self, SettingsKind},
    shortcuts::{self, Profile},
    ui,
};
use ratatui::{backend::TestBackend, Terminal};
use std::{
    fs, io,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};
struct Temp(PathBuf);
impl Temp {
    fn new() -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "dn-shortcuts-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path)?;
        fs::write(path.join("file.txt"), "first word\nsecond line\n")?;
        Ok(Self(path))
    }
    fn app(&self) -> io::Result<App> {
        let mut app = App::new(self.0.clone())?;
        app.config_store = Store::new(Some(self.0.join("config.conf")));
        app.config.shortcuts = Profile::Mac;
        wait(&mut app, |a| a.panels.iter().all(|p| !p.loading));
        app.panels[0].cursor = app.panels[0]
            .entries
            .iter()
            .position(|e| e.name == "file.txt")
            .unwrap();
        Ok(app)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn wait(app: &mut App, done: impl Fn(&App) -> bool) {
    let end = Instant::now() + Duration::from_secs(5);
    while !done(app) {
        app.poll();
        assert!(Instant::now() < end);
        thread::sleep(Duration::from_millis(1));
    }
}
fn key(app: &mut App, code: K) {
    input::handle(app, KeyEvent::new(code, M::NONE));
}
fn prefix(app: &mut App, c: char) {
    input::handle(app, KeyEvent::new(K::Char('o'), M::CONTROL));
    key(app, K::Char(c));
}
fn draw(app: &mut App, w: u16, h: u16) -> io::Result<String> {
    let mut terminal = Terminal::new(TestBackend::new(w, h))?;
    terminal.draw(|f| ui::draw(f, app))?;
    assert!(app
        .mouse
        .regions
        .iter()
        .all(|(r, _)| r.right() <= w && r.bottom() <= h));
    Ok(terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect())
}
#[test]
fn defaults_unknown_values_and_configuration_round_trip() -> io::Result<()> {
    assert_eq!(Config::default().shortcuts, Profile::ClassicDn);
    assert!(!Profile::default().is_mac());
    assert_eq!(Profile::Auto.is_mac(), cfg!(target_os = "macos"));
    let temp = Temp::new()?;
    let path = temp.0.join("desktop");
    for profile in [Profile::Auto, Profile::ClassicDn, Profile::Mac] {
        let config = Config {
            shortcuts: profile,
            ..Config::default()
        };
        config.save(&path)?;
        assert_eq!(Config::read(&path)?.shortcuts, profile);
    }
    fs::write(&path, "config_version=2\n")?;
    assert_eq!(Config::read(&path)?.shortcuts, Profile::ClassicDn);
    fs::write(
        &path,
        "config_version=2\n[application]\nshortcuts=\"unknown\"\n",
    )?;
    assert!(Config::read(&path).is_err());
    Ok(())
}
#[test]
fn manager_numbers_use_original_confirmations_and_preserve_command_draft() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    input::paste(&mut app, "draft command");
    let cursor = app.command_line.cursor;
    prefix(&mut app, '5');
    assert!(app.prompt.is_some() && app.job.is_none());
    assert_eq!(app.command_line.text, "draft command");
    assert_eq!(app.command_line.cursor, cursor);
    key(&mut app, K::Esc);
    prefix(&mut app, '8');
    assert!(app.prompt.is_some() && app.job.is_none());
    assert!(temp.0.join("file.txt").exists());
    key(&mut app, K::Esc);
    prefix(&mut app, '7');
    input::paste(&mut app, "new dir");
    key(&mut app, K::Esc);
    assert!(!temp.0.join("new dir").exists());
    assert!(!app.quit);
    app.config.space_select = false;
    prefix(&mut app, ' ');
    assert_eq!(app.panels[0].selected.len(), 1);
    prefix(&mut app, ',');
    assert_eq!(app.panels[0].cursor, 0);
    prefix(&mut app, '.');
    assert_eq!(app.panels[0].cursor, app.panels[0].entries.len() - 1);
    assert_eq!(app.command_line.text, "draft command");
    assert_eq!(app.command_line.cursor, cursor);
    Ok(())
}
#[test]
fn prefix_cancel_unknown_paste_mouse_and_focus_changes_never_insert_commands() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    let arm = KeyEvent::new(K::Char('o'), M::CONTROL);
    input::handle(&mut app, arm);
    assert!(shortcuts::armed(&app));
    key(&mut app, K::Esc);
    assert!(!shortcuts::armed(&app) && app.command_line.text.is_empty());
    input::handle(&mut app, arm);
    key(&mut app, K::Char('?'));
    assert!(app.command_line.text.is_empty());
    input::handle(&mut app, KeyEvent::new(K::Char('q'), M::SUPER));
    assert!(app.command_line.text.is_empty() && !app.quit);
    input::handle(&mut app, arm);
    input::paste(&mut app, "literal text");
    assert_eq!(app.command_line.text, "literal text");
    let cursor = app.panels[0].cursor;
    input::handle(&mut app, arm);
    draw(&mut app, 80, 25)?;
    mouse::handle(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 5,
            row: 5,
            modifiers: M::NONE,
        },
    );
    assert!(!shortcuts::armed(&app));
    assert_eq!(app.panels[0].cursor, cursor);
    input::handle(&mut app, arm);
    app.prompt_goto();
    key(&mut app, K::Char('0'));
    assert!(!app.quit && !shortcuts::armed(&app));
    assert_eq!(app.prompt.as_ref().unwrap().field, "0");
    Ok(())
}
#[test]
fn classic_keys_still_work_and_classic_profile_has_no_prefix() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    key(&mut app, K::F(5));
    assert!(app.prompt.is_some());
    key(&mut app, K::Esc);
    app.config.shortcuts = Profile::ClassicDn;
    input::handle(&mut app, KeyEvent::new(K::Char('o'), M::CONTROL));
    key(&mut app, K::Char('5'));
    assert!(!shortcuts::armed(&app) && app.prompt.is_none());
    assert_eq!(app.command_line.text, "5");
    Ok(())
}
#[test]
fn settings_apply_cancel_mouse_and_desktop_persistence() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    let revision = app.panels[0].revision;
    prefix(&mut app, 'k');
    assert_eq!(app.settings.as_ref().unwrap().kind, SettingsKind::Keyboard);
    key(&mut app, K::Right);
    assert_eq!(
        app.settings.as_ref().unwrap().draft.shortcuts,
        Profile::Auto
    );
    key(&mut app, K::Esc);
    assert_eq!(app.config.shortcuts, Profile::Mac);
    app.open_settings(SettingsKind::Keyboard);
    draw(&mut app, 80, 25)?;
    let rect = app
        .mouse
        .regions
        .iter()
        .find(|(_, t)| *t == mouse::Target::ShortcutProfile)
        .unwrap()
        .0;
    mouse::handle(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: rect.x,
            row: rect.y,
            modifiers: M::NONE,
        },
    );
    assert_eq!(
        app.settings.as_ref().unwrap().draft.shortcuts,
        Profile::Auto
    );
    settings::accept(&mut app);
    assert_eq!(app.config.shortcuts, Profile::Auto);
    assert_eq!(app.panels[0].revision, revision);
    app.config.shortcuts = Profile::Mac;
    app.save_config();
    app.config.shortcuts = Profile::ClassicDn;
    app.load_config();
    wait(&mut app, |a| !a.associations.busy());
    assert_eq!(app.config.shortcuts, Profile::Mac);
    Ok(())
}
#[test]
fn manager_letters_reach_search_history_bookmarks_and_keyboard_settings() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    prefix(&mut app, 's');
    assert!(app.quick_search.is_some());
    key(&mut app, K::Esc);
    prefix(&mut app, 'h');
    assert!(app.command_line.dialog.is_some());
    key(&mut app, K::Esc);
    prefix(&mut app, 'b');
    assert!(app.navigation_dialog.is_some());
    key(&mut app, K::Esc);
    prefix(&mut app, 'z');
    assert_eq!(app.submenus.last().unwrap().0, menu::MenuId::Sort);
    key(&mut app, K::Char('e'));
    assert_eq!(app.panels[0].sort, dn::panel::SortMode::Extension);
    prefix(&mut app, 'k');
    assert!(app.settings.is_some());
    key(&mut app, K::Esc);
    prefix(&mut app, 'i');
    assert!(app.file_info.is_some());
    key(&mut app, K::Esc);
    Ok(())
}
#[test]
fn editor_save_find_replace_close_and_quit_keep_unsaved_guards() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    prefix(&mut app, '4');
    prefix(&mut app, '2'); // Busy editor events must not insert the digit.
    wait(&mut app, |a| a.editor.as_ref().is_some_and(|e| !e.busy()));
    assert!(app
        .editor
        .as_ref()
        .unwrap()
        .buffer
        .as_ref()
        .unwrap()
        .text
        .starts_with("first word"));
    input::paste(&mut app, "new text");
    prefix(&mut app, 'w');
    assert!(matches!(
        app.editor.as_ref().unwrap().modal,
        Some(dn::editor::Modal::ConfirmClose)
    ));
    key(&mut app, K::Esc);
    assert!(!app.quit);
    prefix(&mut app, 'x');
    assert!(matches!(
        app.editor.as_ref().unwrap().modal,
        Some(dn::editor::Modal::ConfirmClose)
    ));
    key(&mut app, K::Esc);
    assert!(!app.quit);
    prefix(&mut app, '2');
    wait(&mut app, |a| a.editor.as_ref().is_some_and(|e| !e.busy()));
    assert!(fs::read_to_string(temp.0.join("file.txt"))?.starts_with("new text"));
    prefix(&mut app, 'r');
    assert!(matches!(
        app.editor.as_ref().unwrap().modal,
        Some(dn::editor::Modal::Search(_))
    ));
    key(&mut app, K::Esc);
    prefix(&mut app, '0');
    assert!(app.editor.as_ref().unwrap().menu.is_some());
    key(&mut app, K::Esc);
    prefix(&mut app, 'w');
    assert!(app.editor.is_none());
    Ok(())
}
#[test]
fn editor_word_navigation_uses_plain_arrows_after_prefix() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    prefix(&mut app, '4');
    wait(&mut app, |a| a.editor.as_ref().is_some_and(|e| !e.busy()));
    input::handle(&mut app, KeyEvent::new(K::Char('o'), M::CONTROL));
    key(&mut app, K::Right);
    let position = app.editor.as_ref().unwrap().buffer.as_ref().unwrap().cursor;
    assert!(position > 1);
    input::handle(&mut app, KeyEvent::new(K::Char('o'), M::CONTROL));
    input::handle(&mut app, KeyEvent::new(K::Right, M::SHIFT));
    assert!(!app
        .editor
        .as_ref()
        .unwrap()
        .buffer
        .as_ref()
        .unwrap()
        .selected()
        .is_empty());
    Ok(())
}
#[test]
fn viewer_wrap_search_and_close_use_the_same_prefix() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    prefix(&mut app, '3');
    wait(&mut app, |a| {
        a.viewer.as_ref().is_some_and(|v| v.contents.is_some())
    });
    prefix(&mut app, '2');
    assert!(app.viewer.as_ref().unwrap().wrap);
    prefix(&mut app, '7');
    assert!(app.viewer.as_ref().unwrap().search_dialog.is_some());
    key(&mut app, K::Esc);
    prefix(&mut app, 'w');
    assert!(app.viewer.is_none());
    Ok(())
}
#[test]
fn profile_labels_guides_and_mouse_targets_fit_both_sizes_and_locales() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    for language in [
        dn::localization::Language::English,
        dn::localization::Language::Russian,
    ] {
        app.config.language = language;
        for (w, h) in [(80, 25), (120, 40)] {
            for profile in [Profile::Mac, Profile::ClassicDn] {
                app.config.shortcuts = profile;
                let text = draw(&mut app, w, h)?;
                assert!(text.contains(if profile == Profile::Mac { "^O5" } else { "F5" }));
                app.open_settings(SettingsKind::Keyboard);
                draw(&mut app, w, h)?;
                key(&mut app, K::Esc);
            }
            app.config.shortcuts = Profile::Mac;
            input::handle(&mut app, KeyEvent::new(K::Char('o'), M::CONTROL));
            draw(&mut app, w, h)?;
            assert!(shortcuts::armed(&app));
            key(&mut app, K::Esc);
        }
    }
    assert_eq!(
        shortcuts::label(Profile::Mac, Screen::Editor, "Shift+F2"),
        "^Oa"
    );
    assert_eq!(
        shortcuts::label(Profile::ClassicDn, Screen::Manager, "F5"),
        "F5"
    );
    Ok(())
}

#[test]
fn user_menu_prefix_preserves_command_review_and_definition_editing() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    let path = temp.0.join("dn.mnu");
    fs::write(&path, ">1 F8 ~T~est\nprintf harmless\n")?;
    dn::user_menu::open_file(&mut app, path.clone(), true);
    wait(&mut app, |a| !dn::user_menu::pending(&a.user_menu));
    assert!(draw(&mut app, 80, 25)?.contains("^O4"));
    prefix(&mut app, '8');
    assert!(app.user_menu.modal.as_ref().unwrap().review.is_some());
    assert!(app.user_menu.pending.is_none());
    prefix(&mut app, 'w');
    assert!(app.user_menu.modal.as_ref().unwrap().review.is_none());
    prefix(&mut app, '4');
    wait(&mut app, |a| {
        a.editor.as_ref().is_some_and(|e| e.buffer.is_some())
    });
    assert_eq!(app.editor.as_ref().unwrap().path, Some(path));
    Ok(())
}
