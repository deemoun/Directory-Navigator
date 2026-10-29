//! Cross-boundary sequences use real loaders and private filesystem fixtures.
mod support;
use dn::{
    desktop::WindowKind,
    editor::{self, EditorAction},
    input,
};
use std::{fs, io, time::Duration};

#[test]
fn background_documents_finish_after_switch_and_resize_without_changing_foreground(
) -> io::Result<()> {
    let f = support::Fixture::new("lifecycle-loads")?;
    let first = f.0.join("first.txt");
    let second = f.0.join("second.txt");
    fs::write(&first, "界 e\u{301}\n".repeat(200))?;
    fs::write(&second, "second document\n".repeat(120))?;
    for (w, h) in [(80, 25), (132, 40), (100, 32)] {
        let mut a = f.app()?;
        editor::open_path(&mut a, first.clone());
        let owner = a.desktop.active().id;
        editor::open_path(&mut a, second.clone());
        let active = a.desktop.active().id;
        assert_ne!(owner, active);
        // Do not poll before switching: both actual loading and path identity
        // replies must be delivered to their original documents in the background.
        a.desktop.activate(dn::desktop::WindowId::MANAGER);
        support::draw(&mut a, w, h)?;
        support::wait_until(&mut a, Duration::from_secs(5), |a| {
            [owner, active].iter().all(|id| {
                !a.desktop.editor(*id).unwrap().busy() && !a.desktop.identity_pending(*id)
            })
        });
        assert_eq!(a.desktop.active().id, dn::desktop::WindowId::MANAGER);
        assert!(a
            .desktop
            .editor(owner)
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .text
            .starts_with("界"));
        assert!(a
            .desktop
            .editor(active)
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .text
            .starts_with("second"));
        editor::open_path(&mut a, first.clone());
        assert_eq!(a.desktop.active().id, owner);
        assert_eq!(a.desktop.document_ids(WindowKind::Editor).len(), 2);
        editor::action(&mut a, EditorAction::Close);
        assert!(!a.desktop.contains(owner));
        assert!(a.desktop.contains(active));
    }
    Ok(())
}

#[test]
fn dirty_close_cancel_nested_help_resize_then_discard_keeps_other_document_independent(
) -> io::Result<()> {
    let f = support::Fixture::new("lifecycle-dirty")?;
    let mut a = f.app()?;
    editor::open(&mut a, true);
    let first = a.desktop.active().id;
    input::paste(&mut a, "first 界");
    editor::open(&mut a, true);
    let second = a.desktop.active().id;
    input::paste(&mut a, "second e\u{301}");
    support::draw(&mut a, 100, 32)?;
    editor::action(&mut a, EditorAction::Close);
    let guard = a.modal_top().expect("dirty close guard");
    editor::action(&mut a, EditorAction::Help);
    support::draw(&mut a, 80, 25)?;
    support::key(&mut a, crossterm::event::KeyCode::Esc);
    assert_eq!(a.modal_top().unwrap().id, guard.id);
    editor::action(&mut a, EditorAction::Cancel);
    a.sync_modals();
    assert!(a.desktop.contains(second));
    assert!(a.desktop.editor(second).unwrap().dirty());
    a.desktop.activate(first);
    assert_eq!(
        a.desktop
            .editor(first)
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .text,
        "first 界"
    );
    editor::action(&mut a, EditorAction::Undo);
    assert_eq!(
        a.desktop
            .editor(first)
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .text,
        ""
    );
    assert_eq!(
        a.desktop
            .editor(second)
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .text,
        "second e\u{301}"
    );
    a.desktop.activate(second);
    editor::action(&mut a, EditorAction::Close);
    editor::action(&mut a, EditorAction::Discard);
    a.sync_modals();
    assert!(!a.desktop.contains(second));
    assert_eq!(a.desktop.active().id, first);
    assert!(a.modal_top().is_none());
    assert!(!f.0.join("config.conf").exists());
    Ok(())
}
