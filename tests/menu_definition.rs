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
#[test]
fn launch_and_sort_shortcuts_use_category_identity_not_position() -> io::Result<()> {
    let t = Temp(std::env::temp_dir().join(format!("dn-menu-definition-{}", std::process::id())));
    fs::create_dir(&t.0)?;
    let mut a = App::new(t.0.clone())?;
    a.config.save_on_exit = false;
    let deadline = Instant::now() + Duration::from_secs(5);
    while a.panels.iter().any(|p| p.loading) {
        a.poll();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
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
