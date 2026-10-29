//! One cleanup path after controllers have accepted a close. No save/discard policy here.
use crate::{
    app::App,
    desktop::WindowId,
    modals::{ModalId, ModalOwner},
};
use std::collections::HashSet;
#[derive(Default)]
pub(crate) struct State {
    modals: HashSet<ModalId>,
    pending: bool,
}
impl App {
    /// Called only after the existing controller guards succeed. A running save
    /// still prevents removal; reads can be dropped with their document owner.
    pub(crate) fn retire_document(&mut self, id: WindowId) -> bool {
        if id == WindowId::MANAGER || !self.desktop.contains(id) {
            return false;
        }
        self.desktop.close(id);
        if self.desktop.contains(id) {
            return false;
        }
        self.sync_modals();
        true
    }
}
pub(crate) fn documents(app: &mut App) {
    let mut retired = app
        .desktop
        .take_retired()
        .into_iter()
        .collect::<HashSet<_>>();
    // Include owners removed through compatibility low-level APIs before this turn.
    for modal in app.modal_infos() {
        if let ModalOwner::Document(id) = modal.owner {
            if !app.desktop.contains(id) {
                retired.insert(id);
            }
        }
    }
    app.retirement.pending |= !retired.is_empty();
    for id in retired {
        app.visit_modals_mut(|stack| stack.retire(ModalOwner::Document(id)));
    }
}
pub(crate) fn reconcile(app: &mut App) {
    let live = app
        .modal_infos()
        .into_iter()
        .map(|m| m.id)
        .collect::<HashSet<_>>();
    let changed = std::mem::take(&mut app.retirement.pending)
        || app.retirement.modals.iter().any(|id| !live.contains(id));
    for id in app.retirement.modals.difference(&live) {
        app.desktop.native().retire_modal(*id);
    }
    app.retirement.modals = live;
    crate::plugins::host::reconcile(app);
    // Published hits never outlive their document or modal, even before paint.
    if changed {
        app.layout_revision = app
            .layout_revision
            .checked_add(1)
            .expect("layout revision exhausted");
    }
    if changed
        || app.mouse.frame.is_some_and(|f| {
            !app.desktop.contains(f.owner)
                || f.modal
                    .is_some_and(|id| !app.retirement.modals.contains(&id))
        })
    {
        app.mouse.regions.clear();
        app.mouse.dialogs.clear();
        app.mouse.surfaces.clear();
        // Retain the stale receipt until the next published frame, so an event
        // already queued cannot fall through to replacement content.
        app.mouse.reset_click();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        editor, eligibility::Gate, input, layout, presentation::layout::Rect, ui,
        windowing::geometry::SurfaceKey,
    };
    use crossterm::event::{KeyCode as K, KeyEvent, KeyModifiers as M};
    use std::{
        io,
        path::PathBuf,
        time::{Duration, Instant},
    };
    struct Temp(PathBuf);
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn fixture(label: &str) -> io::Result<(Temp, App)> {
        let root = std::env::temp_dir().join(format!("dn-retire-{label}-{}", std::process::id()));
        std::fs::create_dir(&root)?;
        let mut app = App::new(root.clone())?;
        app.config.save_on_exit = false;
        app.config_store = crate::config::Store::new(Some(root.join("config.conf")));
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.panels.iter().any(|p| p.loading) {
            app.poll();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        Ok((Temp(root), app))
    }
    fn publish(app: &mut App, w: u16, h: u16) -> crate::layout::FrameContext {
        let s = layout::prepare(app, Rect::new(0, 0, w, h));
        let f = ui::render(app, &s);
        ui::publish(app, &f);
        f.context
    }
    #[test]
    fn retiring_document_releases_modal_requests_native_geometry_and_capture() -> io::Result<()> {
        let (_temp, mut app) = fixture("owner")?;
        editor::open(&mut app, true);
        let id = app.desktop.active().id;
        let lease = app.desktop.editor(id).unwrap().requests.lease("late");
        app.push_owned_dialog(
            ModalOwner::Document(id),
            crate::dialog::Dialog {
                title: "Child",
                message: "owned".into(),
            },
        );
        let old = publish(&mut app, 100, 32);
        let child = app.modal_top().unwrap();
        assert!(app
            .desktop
            .native()
            .surface(SurfaceKey::Modal(child.id))
            .is_some());
        input::handle(&mut app, KeyEvent::new(K::F(5), M::CONTROL));
        assert!(app.desktop.native().manipulating());
        assert!(app.retire_document(id));
        assert!(!lease.current());
        assert!(app.modal_infos().is_empty());
        assert!(!app.desktop.native().manipulating());
        assert!(app
            .desktop
            .native()
            .surface(SurfaceKey::Modal(child.id))
            .is_none());
        assert!(!Gate::new(&app).frame(Some(old)));
        assert!(app.mouse.regions.is_empty());
        assert_eq!(app.desktop.active().id, WindowId::MANAGER);
        Ok(())
    }
    #[test]
    fn inactive_low_level_retirement_is_reconciled_before_delivery_without_focus_steal(
    ) -> io::Result<()> {
        let (_temp, mut app) = fixture("inactive")?;
        editor::open(&mut app, true);
        let first = app.desktop.active().id;
        editor::open(&mut app, true);
        let active = app.desktop.active().id;
        let old = publish(&mut app, 100, 32);
        app.desktop.close(first);
        app.sync_modals();
        assert_eq!(app.desktop.active().id, active);
        assert!(!Gate::new(&app).frame(Some(old)));
        assert!(app.mouse.regions.is_empty());
        assert!(!app.retire_document(WindowId::MANAGER));
        Ok(())
    }
    #[test]
    fn modal_proposals_and_mouse_share_revision_and_blocking_checks() -> io::Result<()> {
        let (_temp, mut app) = fixture("eligibility")?;
        app.prompt_goto();
        let old = publish(&mut app, 80, 25);
        let modal = app.modal_top().unwrap();
        let surface = app
            .desktop
            .native()
            .surface(SurfaceKey::Modal(modal.id))
            .unwrap();
        let proposal = crate::ui::prompt_spec(&app, crate::turbo::dialog_spec::input_area(&app))
            .unwrap()
            .proposal(Some(crate::mouse::Target::SubmitPrompt));
        assert!(proposal.current(&app));
        assert!(Gate::new(&app).frame(Some(old)));
        assert!(Gate::new(&app).surface(surface.id).is_some());
        publish(&mut app, 100, 32);
        assert!(!proposal.current(&app));
        assert!(!Gate::new(&app).frame(Some(old)));
        let fresh = crate::ui::prompt_spec(&app, crate::turbo::dialog_spec::input_area(&app))
            .unwrap()
            .proposal(None);
        app.output_visible = true;
        assert!(!fresh.current(&app));
        assert!(Gate::new(&app).surface(surface.id).is_none());
        app.output_visible = false;
        app.error("Nested child");
        app.sync_modals();
        let child = app.modal_top().unwrap();
        publish(&mut app, 100, 32);
        assert!(!fresh.current(&app));
        app.pop_dialog();
        app.sync_modals();
        assert_eq!(app.modal_top().unwrap().id, modal.id);
        assert!(app
            .desktop
            .native()
            .surface(SurfaceKey::Modal(child.id))
            .is_none());
        assert!(!fresh.current(&app));
        assert!(
            crate::ui::prompt_spec(&app, crate::turbo::dialog_spec::input_area(&app))
                .unwrap()
                .proposal(None)
                .current(&app)
        );
        Ok(())
    }
    #[test]
    fn running_save_cannot_be_retired_and_guarded_close_cleans_up_after_success() -> io::Result<()>
    {
        let (temp, mut app) = fixture("save")?;
        let path = temp.0.join("file.txt");
        std::fs::write(&path, "initial")?;
        editor::open_path(&mut app, path.clone());
        let id = app.desktop.active().id;
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.desktop.editor(id).unwrap().busy() || app.desktop.identity_pending(id) {
            app.poll();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        publish(&mut app, 100, 32);
        editor::paste(&mut app, "changed");
        editor::action(&mut app, editor::EditorAction::Save);
        assert!(app.desktop.editor(id).unwrap().saving());
        assert!(!app.retire_document(id));
        assert!(app.desktop.contains(id));
        while app.desktop.editor(id).unwrap().busy() {
            app.poll();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(!app.desktop.editor(id).unwrap().dirty());
        assert!(std::fs::read_to_string(path)?.contains("changed"));
        editor::action(&mut app, editor::EditorAction::Close);
        assert!(!app.desktop.contains(id));
        Ok(())
    }
    /// The worker is already running before UI changes, so cancellation cannot
    /// hide the stale-result case by merely removing a queued closure.
    #[test]
    fn delayed_replies_across_resize_nested_modals_switch_and_retirement() -> io::Result<()> {
        use std::sync::mpsc::{self, TryRecvError};
        for retire in [false, true] {
            let (_temp, mut app) = fixture(if retire {
                "sequence-retired"
            } else {
                "sequence-live"
            })?;
            editor::open(&mut app, true);
            let owner = app.desktop.active().id;
            let (started, ready) = mpsc::channel();
            let (release, wait) = mpsc::channel();
            let task =
                app.desktop
                    .editor(owner)
                    .unwrap()
                    .requests
                    .spawn("sequence", move |_| {
                        started.send(()).unwrap();
                        wait.recv_timeout(Duration::from_secs(5)).unwrap();
                        "late result"
                    })?;
            ready.recv_timeout(Duration::from_secs(5)).unwrap();
            let token = task.token().unwrap();
            publish(&mut app, 100, 32);
            editor::action(&mut app, editor::EditorAction::Find);
            publish(&mut app, 100, 32);
            let old = crate::ui::editor_spec(&app, crate::turbo::dialog_spec::input_area(&app))
                .unwrap()
                .proposal(Some(crate::mouse::Target::Editor(
                    editor::EditorAction::Cancel,
                )));
            app.push_owned_dialog(
                ModalOwner::Document(owner),
                crate::dialog::Dialog {
                    title: "Nested",
                    message: "sequence".into(),
                },
            );
            publish(&mut app, 80, 25);
            assert!(!old.current(&app));
            app.pop_dialog();
            app.sync_modals();
            publish(&mut app, 100, 32);
            assert!(!old.current(&app));
            editor::action(&mut app, editor::EditorAction::Cancel);
            app.sync_modals();
            editor::open(&mut app, true);
            let foreground = app.desktop.active().id;
            publish(&mut app, 100, 32);
            if retire {
                assert!(app.retire_document(owner));
            } else {
                // Replacing a request invalidates the first generation even
                // though its owner remains alive in the background.
                app.desktop
                    .editor(owner)
                    .unwrap()
                    .requests
                    .start("sequence");
            }
            release.send(()).unwrap();
            assert!(matches!(task.try_recv(), Err(TryRecvError::Disconnected)));
            app.poll();
            assert_eq!(app.desktop.active().id, foreground);
            assert!(!app.desktop.editor(foreground).unwrap().dirty());
            if !retire {
                assert!(!app.desktop.editor(owner).unwrap().requests.accepts(token));
            }
            assert!(!old.current(&app));
        }
        Ok(())
    }
    #[test]
    fn current_background_reply_survives_focus_and_geometry_changes_without_activation(
    ) -> io::Result<()> {
        use std::sync::mpsc;
        let (_temp, mut app) = fixture("sequence-success")?;
        editor::open(&mut app, true);
        let owner = app.desktop.active().id;
        let (started, ready) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let task = app
            .desktop
            .editor(owner)
            .unwrap()
            .requests
            .spawn("sequence", move |_| {
                started.send(()).unwrap();
                wait.recv_timeout(Duration::from_secs(5)).unwrap();
                "owned result"
            })?;
        ready.recv_timeout(Duration::from_secs(5)).unwrap();
        let token = task.token().unwrap();
        editor::open(&mut app, true);
        let foreground = app.desktop.active().id;
        publish(&mut app, 100, 32);
        input::handle(&mut app, KeyEvent::new(K::F(5), M::CONTROL));
        input::handle(&mut app, KeyEvent::new(K::Right, M::NONE));
        publish(&mut app, 80, 25); // resize cancels capture
        assert!(!app.desktop.native().manipulating());
        assert!(app.desktop.editor(owner).unwrap().requests.accepts(token));
        release.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let result = loop {
            match task.try_recv() {
                Ok(value) => break value,
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    app.poll();
                    assert!(Instant::now() < deadline);
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(e) => panic!("current owner lost its reply: {e}"),
            }
        };
        assert_eq!(result, "owned result");
        assert!(!app.desktop.editor(owner).unwrap().requests.accepts(token));
        assert_eq!(app.desktop.active().id, foreground);
        Ok(())
    }
}
