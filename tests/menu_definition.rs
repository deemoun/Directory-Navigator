use crossterm::event::{KeyCode as K, KeyEvent, KeyModifiers as M};
use dn::{
    app::App,
    input,
    menu::{self, definition::ROOTS, Action, MenuId, RootId},
    shortcuts::Profile,
};
use std::{
    fs, io,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

#[test]
fn hierarchy_has_unique_roots_and_acyclic_submenus() {
    fn visit(items: &[menu::Item], ancestors: &mut Vec<MenuId>) {
        for item in items {
            if let Action::Submenu(id) = item.action {
                assert!(!ancestors.contains(&id), "cyclic submenu: {id:?}");
                ancestors.push(id);
                visit(&menu::submenu_items(id), ancestors);
                ancestors.pop();
            }
        }
    }
    for (index, root) in ROOTS.iter().enumerate() {
        assert!(!ROOTS[..index].iter().any(|r| r.id == root.id));
        assert_eq!(menu::root_index(root.id), Some(index));
        visit(root.entries, &mut Vec::new());
    }
}

struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn fixture() -> io::Result<(Temp, App)> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let t = Temp(std::env::temp_dir().join(format!(
        "dn-menu-definition-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )));
    fs::create_dir(&t.0)?;
    let mut a = App::new(t.0.clone())?;
    a.config.save_on_exit = false;
    let deadline = Instant::now() + Duration::from_secs(5);
    while a.panels.iter().any(|p| p.loading) {
        a.poll();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    Ok((t, a))
}
#[test]
fn launch_and_sort_shortcuts_use_category_identity_not_position() -> io::Result<()> {
    let (_t, mut a) = fixture()?;
    for profile in [Profile::ClassicDn, Profile::Mac] {
        a.config.shortcuts = profile;
        input::handle(&mut a, KeyEvent::new(K::F(9), M::NONE));
        assert_eq!(
            a.modals.menu().unwrap().0,
            menu::root_index(RootId::File).unwrap()
        );
        menu::close(&mut a);
        if profile == Profile::ClassicDn {
            input::handle(&mut a, KeyEvent::new(K::Char('b'), M::ALT));
        } else {
            input::handle(&mut a, KeyEvent::new(K::Char('o'), M::CONTROL));
            input::handle(&mut a, KeyEvent::new(K::Char('z'), M::NONE));
        }
        assert_eq!(
            a.modals.menu().unwrap().0,
            menu::root_index(RootId::Panel).unwrap()
        );
        assert_eq!(a.modals.submenus().last().unwrap().0, MenuId::Sort);
        menu::close(&mut a);
    }
    for (index, root) in ROOTS.iter().enumerate() {
        if let Some(c) = menu::mnemonic(root.label) {
            assert!(menu::open_mnemonic(&mut a, c));
            assert_eq!(a.modals.menu().unwrap().0, index);
            menu::close(&mut a);
        }
    }
    Ok(())
}

#[test]
fn editor_default_category_and_navigation_follow_definition_order() -> io::Result<()> {
    use dn::editor::{self, menus, EditorAction as A};
    use dn::menu::definition::{EditorRootId, EDITOR_ROOTS};
    let (_t, mut a) = fixture()?;
    editor::open(&mut a, true);
    let deadline = Instant::now() + Duration::from_secs(5);
    while a.desktop.active_editor().unwrap().busy() {
        a.poll();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    editor::action(&mut a, A::Menu);
    let file = menus::root_index(EditorRootId::File).unwrap();
    assert_eq!(
        a.desktop
            .active_editor()
            .unwrap()
            .modals
            .editor_menu()
            .unwrap()
            .0,
        file
    );
    let enabled: Vec<_> = EDITOR_ROOTS
        .iter()
        .enumerate()
        .filter(|(_, root)| root.enabled)
        .map(|(i, _)| i)
        .collect();
    let mut expected = file;
    for _ in 0..enabled.len() {
        let i = enabled.iter().position(|i| *i == expected).unwrap();
        expected = enabled[(i + 1) % enabled.len()];
        menus::key(&mut a, K::Tab);
        assert_eq!(
            a.desktop
                .active_editor()
                .unwrap()
                .modals
                .editor_menu()
                .unwrap()
                .0,
            expected
        );
    }
    menus::open_root(&mut a, EditorRootId::Paragraph);
    assert_eq!(
        a.desktop
            .active_editor()
            .unwrap()
            .modals
            .editor_menu()
            .unwrap()
            .0,
        file
    );
    for (index, root) in EDITOR_ROOTS.iter().enumerate() {
        assert_eq!(menus::root_index(root.id), Some(index));
        if root.enabled {
            menus::open_root(&mut a, root.id);
            if let Some(separator) = root.entries.iter().position(|e| e.action.is_none()) {
                menus::activate(&mut a, separator);
                assert_eq!(
                    a.desktop
                        .active_editor()
                        .unwrap()
                        .modals
                        .editor_menu()
                        .unwrap()
                        .0,
                    index
                );
                menus::key(&mut a, K::End);
                menus::key(&mut a, K::Down);
                assert_eq!(
                    a.desktop
                        .active_editor()
                        .unwrap()
                        .modals
                        .editor_menu()
                        .unwrap()
                        .1,
                    root.entries
                        .iter()
                        .position(|e| e.action.is_some())
                        .unwrap()
                );
            }
        }
    }
    Ok(())
}

#[test]
fn viewer_bar_mouse_and_keyboard_agree_and_hex_has_no_text_actions() -> io::Result<()> {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    use dn::{
        localization::Language,
        mouse::{self, Target},
        palette::PaletteProfile,
        testing::{backend::TestBackend, Terminal},
        ui,
        viewer::{self, Action as V, ViewMode, Viewer},
    };
    let (_t, mut a) = fixture()?;
    a.show_viewer(Viewer::from_contents(
        PathBuf::from("fixture.txt"),
        viewer::from_bytes(b"alpha beta\n".to_vec(), ViewMode::Text),
    ));
    for profile in [Profile::ClassicDn, Profile::Mac] {
        a.config.shortcuts = profile;
        for language in [Language::English, Language::Russian] {
            a.config.language = language;
            for palette in [PaletteProfile::ClassicDn, PaletteProfile::Terminal] {
                a.config.palette = palette;
                for (width, height) in [(80, 25), (132, 40)] {
                    let mut terminal = Terminal::new(TestBackend::new(width, height))?;
                    terminal.draw(|f| ui::draw(f, &mut a))?;
                    let before = terminal.backend().buffer().clone();
                    terminal.draw(|f| ui::draw(f, &mut a))?;
                    assert_eq!(*terminal.backend().buffer(), before);
                    let r = a
                        .mouse
                        .regions
                        .iter()
                        .find(|(_, t)| *t == Target::Viewer(V::Wrap))
                        .unwrap()
                        .0;
                    mouse::handle(
                        &mut a,
                        MouseEvent {
                            kind: MouseEventKind::Down(MouseButton::Left),
                            column: r.x,
                            row: r.y,
                            modifiers: M::NONE,
                        },
                    );
                    assert!(a.desktop.active_viewer().unwrap().wrap);
                    input::handle(&mut a, KeyEvent::new(K::F(2), M::NONE));
                    assert!(!a.desktop.active_viewer().unwrap().wrap);
                }
            }
        }
    }
    a.show_viewer(Viewer::from_contents(
        PathBuf::from("fixture.bin"),
        viewer::from_bytes(vec![0, 1], ViewMode::Hex),
    ));
    let mut terminal = Terminal::new(TestBackend::new(80, 25))?;
    terminal.draw(|f| ui::draw(f, &mut a))?;
    assert!(!a
        .mouse
        .regions
        .iter()
        .any(|(_, t)| matches!(t, Target::Viewer(V::Wrap | V::Find | V::Next | V::Reverse))));
    input::handle(&mut a, KeyEvent::new(K::F(2), M::NONE));
    assert!(!a.desktop.active_viewer().unwrap().wrap);
    Ok(())
}
