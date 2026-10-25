use crossterm::event::{
    KeyCode as K, KeyEvent, KeyModifiers as M, MouseButton, MouseEvent, MouseEventKind,
};
use dn::{
    app::App,
    input, layout,
    mouse::{self, Target},
    presentation::layout::Rect,
    settings::{self, Settings, SettingsKind},
    testing::Buffer,
    ui,
};
use std::{
    fs, io,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};
struct Temp(PathBuf);
impl Temp {
    fn app() -> io::Result<(Self, App)> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "dn-native-controls-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root)?;
        let temp = Self(fs::canonicalize(root)?);
        let mut app = App::new(temp.0.clone())?;
        app.config.save_on_exit = false;
        Ok((temp, app))
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn key(app: &mut App, code: K) {
    input::handle(app, KeyEvent::new(code, M::NONE));
}
fn render(app: &mut App) -> Buffer {
    let snapshot = layout::prepare_window(
        app,
        Rect::new(7, 3, 80, 25),
        Rect::new(0, 0, 90, 30),
        Instant::now(),
    );
    let frame = ui::render(app, &snapshot);
    app.mouse.regions = frame.hit_regions().to_vec();
    app.mouse.frame = Some(frame.context);
    Buffer::rendered(frame)
}
#[test]
fn native_choice_cancel_and_accept_remain_inside_the_parent_draft() -> io::Result<()> {
    let (_temp, mut app) = Temp::app()?;
    let original = app.config.shortcuts;
    app.modals
        .set_settings(Some(Settings::new(SettingsKind::Keyboard, &app.config)));
    app.sync_modals();
    let parent = app.modal_top().unwrap().id;
    key(&mut app, K::F(4));
    app.sync_modals();
    assert_eq!(app.modal_top().unwrap().parent, Some(parent));
    key(&mut app, K::Down);
    key(&mut app, K::Esc);
    assert_eq!(app.modal_top().unwrap().id, parent);
    assert_eq!(app.modals.settings().unwrap().draft.shortcuts, original);
    key(&mut app, K::F(4));
    key(&mut app, K::Down);
    key(&mut app, K::Enter);
    assert_eq!(app.config.shortcuts, original);
    assert_eq!(
        app.modals.settings().unwrap().draft.shortcuts,
        original.next(false)
    );
    key(&mut app, K::Esc);
    assert_eq!(app.config.shortcuts, original);
    Ok(())
}
#[test]
fn native_choice_render_is_immutable_clipped_and_mouse_accepts_only_the_child() -> io::Result<()> {
    let (_temp, mut app) = Temp::app()?;
    app.modals
        .set_settings(Some(Settings::new(SettingsKind::Regional, &app.config)));
    settings::open_choice(&mut app, 1);
    key(&mut app, K::Down);
    let snapshot = layout::prepare_window(
        &mut app,
        Rect::new(7, 3, 80, 25),
        Rect::new(0, 0, 83, 27),
        Instant::now(),
    );
    let first = ui::render(&app, &snapshot);
    let second = ui::render(&app, &snapshot);
    assert_eq!(Buffer::rendered(first), Buffer::rendered(second));
    render(&mut app);
    assert!(app
        .mouse
        .regions
        .iter()
        .all(|(_, t)| matches!(t, Target::SettingsChoice(_) | Target::CancelSettingsChoice)));
    let row = app
        .mouse
        .regions
        .iter()
        .find(|(_, t)| *t == Target::SettingsChoice(1))
        .unwrap()
        .0;
    mouse::handle(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: row.x,
            row: row.y,
            modifiers: M::NONE,
        },
    );
    assert!(app.modals.settings_choice().is_none());
    assert_ne!(
        app.modals.settings().unwrap().draft.datetime.date_order,
        app.config.datetime.date_order
    );
    Ok(())
}
#[test]
fn retiring_a_parent_retires_choices_and_stale_mouse_cannot_edit_replacement() -> io::Result<()> {
    let (_temp, mut app) = Temp::app()?;
    app.modals
        .set_settings(Some(Settings::new(SettingsKind::Keyboard, &app.config)));
    settings::open_choice(&mut app, 0);
    render(&mut app);
    let row = app
        .mouse
        .regions
        .iter()
        .find(|(_, t)| *t == Target::SettingsChoice(1))
        .unwrap()
        .0;
    app.modals.set_settings(None);
    app.sync_modals();
    assert!(app.modals.settings_choice().is_none());
    app.modals
        .set_settings(Some(Settings::new(SettingsKind::Keyboard, &app.config)));
    app.sync_modals();
    mouse::handle(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: row.x,
            row: row.y,
            modifiers: M::NONE,
        },
    );
    assert_eq!(
        app.modals.settings().unwrap().draft.shortcuts,
        app.config.shortcuts
    );
    Ok(())
}

#[test]
fn native_outline_leaf_name_double_click_opens_the_directory() -> io::Result<()> {
    let (temp, mut app) = Temp::app()?;
    let leaf = temp.0.join("empty");
    fs::create_dir(&leaf)?;
    fn settle(app: &mut App) {
        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        loop {
            app.poll();
            if !app.panels.iter().any(|p| p.loading)
                && !app
                    .modals
                    .directory_tree()
                    .is_some_and(|tree| tree.loading())
            {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
    settle(&mut app);
    dn::directory_tree::open(&mut app);
    settle(&mut app);
    let index = app
        .modals
        .directory_tree()
        .unwrap()
        .rows
        .iter()
        .position(|row| row.path == leaf)
        .unwrap();
    dn::directory_tree::action(&mut app, dn::directory_tree::Action::Row(index));
    key(&mut app, K::Right);
    settle(&mut app);
    let tree = app.modals.directory_tree().unwrap();
    let index = tree.rows.iter().position(|row| row.path == leaf).unwrap();
    let depth = tree.rows[index].depth;
    assert_eq!(tree.marker(&tree.rows[index]), " ");
    render(&mut app);
    let row = app
        .mouse
        .regions
        .iter()
        .find(|(_, t)| *t == Target::Tree(dn::directory_tree::Action::Row(index)))
        .unwrap()
        .0;
    let x = row.x + (depth * 2 + if depth > 0 { 2 } else { 0 }) as u16 + 1;
    for _ in 0..2 {
        mouse::handle(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: x,
                row: row.y,
                modifiers: M::NONE,
            },
        );
        render(&mut app);
    }
    settle(&mut app);
    assert!(app.modals.directory_tree().is_none());
    assert_eq!(app.panels[app.active].path, leaf);
    Ok(())
}
