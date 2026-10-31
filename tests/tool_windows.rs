mod support;
use crossterm::event::{
    KeyCode as K, KeyEvent, KeyModifiers as M, MouseButton as B, MouseEvent, MouseEventKind as E,
};
use dn::{
    app::App,
    desktop::{WindowId, WindowKind},
    input, layout, menu,
    mouse::{self, Target},
    presentation::layout::Rect,
    tool_windows::Kind,
    windowing::geometry::SurfaceKey,
};
use std::{
    io,
    time::{Duration, Instant},
};
fn key(a: &mut App, k: K, m: M) {
    input::handle(a, KeyEvent::new(k, m));
}
fn pointer(a: &mut App, x: u16, y: u16) {
    mouse::handle(
        a,
        MouseEvent {
            kind: E::Down(B::Left),
            column: x,
            row: y,
            modifiers: M::NONE,
        },
    );
}
fn wait(a: &mut App, done: impl Fn(&App) -> bool) {
    let end = Instant::now() + Duration::from_secs(5);
    while !done(a) {
        a.poll();
        assert!(Instant::now() < end);
    }
}
#[test]
fn tree_is_a_reusable_document_with_menus_and_switching() -> io::Result<()> {
    let f = support::Fixture::new("tools-tree")?;
    let mut a = f.app()?;
    dn::directory_tree::open(&mut a);
    let id = a.desktop.active().id;
    support::draw(&mut a, 100, 32)?;
    assert_eq!(a.desktop.active().kind, WindowKind::Tool(Kind::Tree));
    assert!(a.modal_top().is_none());
    wait(&mut a, |a| a.directory_tree().is_some_and(|t| !t.loading()));
    let cursor = a.directory_tree().unwrap().cursor;
    key(&mut a, K::F(9), M::NONE);
    assert_eq!(dn::focus::current(&a), dn::focus::Focus::Menu);
    key(&mut a, K::Esc, M::NONE);
    assert_eq!(a.desktop.active().id, id);
    key(&mut a, K::F(6), M::ALT);
    assert_eq!(a.desktop.active().id, WindowId::MANAGER);
    dn::directory_tree::open(&mut a);
    assert_eq!(a.desktop.active().id, id);
    assert_eq!(a.directory_tree().unwrap().cursor, cursor);
    let before = a.desktop.windows().count();
    dn::editor::open(&mut a, true);
    let editor = a.desktop.active().id;
    support::draw(&mut a, 100, 32)?;
    key(&mut a, K::Tab, M::CONTROL);
    assert_eq!(a.desktop.active().id, WindowId::MANAGER);
    key(&mut a, K::Tab, M::CONTROL | M::SHIFT);
    assert_eq!(a.desktop.active().id, editor);
    assert_eq!(a.desktop.windows().count(), before + 1);
    Ok(())
}
#[test]
fn exposed_windows_activate_and_menu_clicks_work_from_tools() -> io::Result<()> {
    let f = support::Fixture::new("tools-overlap")?;
    let mut a = f.app()?;
    dn::editor::open(&mut a, true);
    let editor = a.desktop.active().id;
    support::draw(&mut a, 120, 40)?;
    key(&mut a, K::Char('z'), M::ALT);
    dn::directory_tree::open(&mut a);
    let tree = a.desktop.active().id;
    support::draw(&mut a, 120, 40)?;
    menu::execute(&mut a, menu::Action::WindowCascade);
    let snap = layout::prepare(&mut a, Rect::new(0, 0, 120, 40));
    let e = snap
        .surfaces
        .iter()
        .find(|s| s.id.key == SurfaceKey::Document(editor))
        .unwrap()
        .bounds;
    support::draw(&mut a, 120, 40)?;
    // Move the tree clear of the editor's title.
    pointer(&mut a, e.x + 8, e.y);
    assert_eq!(a.desktop.active().id, editor);
    key(&mut a, K::Esc, M::NONE); // cancel any title drag
    dn::directory_tree::open(&mut a);
    assert_eq!(a.desktop.active().id, tree);
    support::draw(&mut a, 120, 40)?;
    let r = a
        .mouse
        .regions
        .iter()
        .find(|(_, t)| *t == Target::Menu(3))
        .unwrap()
        .0;
    pointer(&mut a, r.x, r.y);
    assert_eq!(dn::focus::current(&a), dn::focus::Focus::Menu);
    key(&mut a, K::Esc, M::NONE);
    a.desktop.close(editor);
    support::draw(&mut a, 120, 40)?;
    pointer(&mut a, 0, 3);
    assert_eq!(a.desktop.active().id, WindowId::MANAGER);
    assert!(a.desktop.contains(tree));
    Ok(())
}
#[test]
fn utilities_coexist_and_calculator_is_a_document() -> io::Result<()> {
    let f = support::Fixture::new("tools-utilities")?;
    let mut a = f.app()?;
    let mut ids = vec![];
    for kind in [
        dn::utilities::Kind::Characters,
        dn::utilities::Kind::System,
        dn::utilities::Kind::Memory,
        dn::utilities::Kind::Calculator,
    ] {
        dn::utilities::open(&mut a, kind);
        let id = a.desktop.active().id;
        assert!(!ids.contains(&id));
        ids.push(id);
        support::draw(&mut a, 100, 32)?;
        assert!(a.modal_top().is_none());
    }
    assert_eq!(
        a.desktop.active().kind,
        WindowKind::Plugin(dn::plugins::CALCULATOR)
    );
    key(&mut a, K::F(9), M::NONE);
    key(&mut a, K::Esc, M::NONE);
    dn::utilities::open(&mut a, dn::utilities::Kind::Characters);
    assert_eq!(a.desktop.active().id, ids[0]);
    a.poll();
    assert_eq!(a.desktop.active().id, ids[0]);
    assert_eq!(a.desktop.windows().count(), 5);
    Ok(())
}
#[test]
fn decisions_and_destination_picker_remain_blocking() -> io::Result<()> {
    let f = support::Fixture::new("tools-blocking")?;
    let mut a = f.app()?;
    dn::directory_tree::open(&mut a);
    let id = a.desktop.active().id;
    support::draw(&mut a, 100, 32)?;
    a.prompt_operation(dn::operations::Kind::Mkdir, false);
    support::draw(&mut a, 100, 32)?;
    key(&mut a, K::F(6), M::ALT);
    assert_eq!(a.desktop.active().id, id);
    key(&mut a, K::F(9), M::NONE);
    assert_ne!(dn::focus::current(&a), dn::focus::Focus::Menu);
    key(&mut a, K::Esc, M::NONE);
    assert!(a.modal_top().is_none());
    Ok(())
}
#[test]
fn standalone_help_can_switch_but_nested_help_restores_its_parent() -> io::Result<()> {
    let f = support::Fixture::new("tools-help")?;
    let mut a = f.app()?;
    key(&mut a, K::F(1), M::NONE);
    let help = a.desktop.active().id;
    assert_eq!(a.desktop.active().kind, WindowKind::Tool(Kind::Help));
    support::draw(&mut a, 80, 25)?;
    assert!(a.modal_top().is_none());
    key(&mut a, K::F(6), M::ALT);
    assert_eq!(a.desktop.active().id, WindowId::MANAGER);
    a.prompt_operation(dn::operations::Kind::Mkdir, false);
    support::draw(&mut a, 80, 25)?;
    let parent = a.modal_top().unwrap().id;
    key(&mut a, K::F(1), M::NONE);
    support::draw(&mut a, 80, 25)?;
    assert_ne!(a.modal_top().unwrap().id, parent);
    key(&mut a, K::F(6), M::ALT);
    assert_eq!(a.desktop.active().id, WindowId::MANAGER);
    key(&mut a, K::Esc, M::NONE);
    assert_eq!(a.modal_top().unwrap().id, parent);
    assert!(a.desktop.contains(help));
    Ok(())
}

#[test]
fn background_search_updates_its_document_without_stealing_editor_focus() -> io::Result<()> {
    let f = support::Fixture::new("tools-background")?;
    std::fs::write(f.0.join("match.txt"), "fixture")?;
    let mut a = f.app()?;
    dn::discovery::open_find(&mut a);
    let find = a.desktop.active().id;
    a.discovery_mut().unwrap().prefs.pattern = "*.txt".into();
    dn::discovery::action(&mut a, dn::discovery::Action::Start);
    dn::editor::open(&mut a, true);
    let editor = a.desktop.active().id;
    wait(&mut a, |a| {
        a.desktop
            .tool(find)
            .unwrap()
            .discovery()
            .unwrap()
            .job
            .is_none()
    });
    assert_eq!(a.desktop.active().id, editor);
    assert_eq!(
        a.desktop
            .tool(find)
            .unwrap()
            .discovery()
            .unwrap()
            .hits
            .len(),
        1
    );
    a.desktop.close(find);
    a.poll();
    assert_eq!(a.desktop.active().id, editor);
    dn::discovery::open_find(&mut a);
    assert_ne!(a.desktop.active().id, find);
    assert!(a.discovery().unwrap().hits.is_empty());
    Ok(())
}
#[test]
fn resized_translated_tool_frames_keep_clipped_hits_and_immutable_paint() -> io::Result<()> {
    for language in [
        dn::localization::Language::English,
        dn::localization::Language::Russian,
    ] {
        let f = support::Fixture::new("tools-resize")?;
        let mut a = f.app()?;
        a.config.language = language;
        a.config.palette = a
            .config
            .palette
            .with_slot(dn::palette::theme::Slot::DialogText, 0x2f);
        dn::directory_tree::open(&mut a);
        let id = a.desktop.active().id;
        support::draw(&mut a, 100, 32)?;
        key(&mut a, K::F(5), M::CONTROL);
        for _ in 0..8 {
            key(&mut a, K::Left, M::SHIFT);
        }
        key(&mut a, K::Down, M::SHIFT);
        key(&mut a, K::Enter, M::NONE);
        for (w, h) in [(80, 25), (132, 40)] {
            let prepared = layout::prepare_window(
                &mut a,
                Rect::new(3, 2, w, h),
                Rect::new(0, 0, w + 5, h + 4),
                Instant::now(),
            );
            let surface = prepared
                .surfaces
                .iter()
                .find(|s| s.id.key == SurfaceKey::Document(id))
                .unwrap();
            assert!(surface.bounds.width >= 48 && surface.bounds.height >= 10);
            let frame = dn::ui::render(&a, &prepared);
            let hits = frame.hit_regions().to_vec();
            let first = dn::testing::Buffer::rendered(frame);
            assert_eq!(
                first,
                dn::testing::Buffer::rendered(dn::ui::render(&a, &prepared))
            );
            assert!(hits
                .iter()
                .all(|(r, _)| r.right() <= w + 5 && r.bottom() <= h + 4));
        }
    }
    Ok(())
}
#[test]
fn tree_open_keeps_the_tool_and_updates_its_original_panel() -> io::Result<()> {
    let f = support::Fixture::new("tools-navigation")?.canonicalized()?;
    std::fs::create_dir(f.0.join("child"))?;
    let mut a = f.app()?;
    dn::directory_tree::open(&mut a);
    let tree = a.desktop.active().id;
    wait(&mut a, |a| a.directory_tree().is_some_and(|t| !t.loading()));
    let target = f.0.join("child");
    let t = a.directory_tree_mut().unwrap();
    t.cursor = t.rows.iter().position(|r| r.path == target).unwrap();
    a.active = 1;
    dn::directory_tree::action(&mut a, dn::directory_tree::Action::Open);
    wait(&mut a, |a| !a.panels[0].loading);
    assert_eq!(a.desktop.active().id, WindowId::MANAGER);
    assert!(a.desktop.tool(tree).unwrap().tree().is_some());
    assert_eq!(a.panels[0].path, target);
    assert_eq!(a.panels[1].path, f.0);
    Ok(())
}

#[test]
fn export_validation_from_history_keeps_both_the_form_and_history_owner() -> io::Result<()> {
    let f = support::Fixture::new("tools-export-guard")?.canonicalized()?;
    let path = f.0.join("source.txt");
    std::fs::write(&path, "preserved")?;
    let mut a = f.app()?;
    a.panels[0].cursor = a.panels[0]
        .entries
        .iter()
        .position(|e| e.path == path)
        .unwrap();
    a.file_tools
        .histories
        .record(dn::file_tools::history::HistoryKind::Edit, path.clone());
    dn::file_tools::open_history(&mut a, dn::file_tools::history::HistoryKind::Edit);
    let history = a.desktop.active().id;
    support::draw(&mut a, 100, 32)?;
    menu::execute(&mut a, menu::Action::ExportList);
    support::draw(&mut a, 100, 32)?;
    assert!(a.modal_top().is_some());
    if let Some(dn::file_tools::ToolDialog::Export { field, cursor, .. }) =
        a.file_tools.modals.file_tools_mut()
    {
        field.clear();
        *cursor = 0;
    } else {
        panic!("workspace export form");
    }
    dn::file_tools::accept(&mut a);
    support::draw(&mut a, 100, 32)?;
    assert!(a.last_dialog().is_some());
    key(&mut a, K::Esc, M::NONE);
    assert!(matches!(
        a.file_tools.modals.file_tools(),
        Some(dn::file_tools::ToolDialog::Export { .. })
    ));
    key(&mut a, K::Esc, M::NONE);
    assert_eq!(a.desktop.active().id, history);
    assert!(matches!(
        a.file_tools(),
        Some(dn::file_tools::ToolDialog::History { .. })
    ));
    assert_eq!(std::fs::read_to_string(path)?, "preserved");
    assert!(!a.file_tools.busy());
    Ok(())
}

#[test]
fn tree_origin_follows_panel_swaps_without_retargeting_navigation() -> io::Result<()> {
    let f = support::Fixture::new("tools-swap")?.canonicalized()?;
    let target = f.0.join("child");
    std::fs::create_dir(&target)?;
    let mut a = f.app()?;
    dn::directory_tree::open(&mut a);
    let tree = a.desktop.active().id;
    wait(&mut a, |a| a.directory_tree().is_some_and(|t| !t.loading()));
    let t = a.directory_tree_mut().unwrap();
    t.cursor = t.rows.iter().position(|r| r.path == target).unwrap();
    menu::execute(&mut a, menu::Action::SwapPanels);
    assert_eq!(a.desktop.tool(tree).unwrap().panel, 1);
    dn::directory_tree::action(&mut a, dn::directory_tree::Action::Open);
    wait(&mut a, |a| !a.panels[1].loading);
    assert_eq!(a.panels[1].path, target);
    assert_eq!(a.panels[0].path, f.0);
    assert_eq!(a.active, 1);
    Ok(())
}

#[test]
fn reopening_sizes_retains_results_even_with_a_different_manager_selection() -> io::Result<()> {
    let f = support::Fixture::new("tools-sizes-reopen")?.canonicalized()?;
    let directory = f.0.join("folder");
    std::fs::create_dir(&directory)?;
    std::fs::write(directory.join("content.txt"), "retained")?;
    let regular = f.0.join("regular.txt");
    std::fs::write(&regular, "not a directory")?;
    let mut a = f.app()?;
    a.panels[0].cursor = a.panels[0]
        .entries
        .iter()
        .position(|e| e.path == directory)
        .unwrap();
    dn::discovery::open_sizes(&mut a);
    let sizes = a.desktop.active().id;
    wait(&mut a, |a| {
        a.desktop
            .tool(sizes)
            .unwrap()
            .discovery()
            .unwrap()
            .job
            .is_none()
    });
    let sources = a
        .desktop
        .tool(sizes)
        .unwrap()
        .discovery()
        .unwrap()
        .sources
        .clone();
    key(&mut a, K::F(6), M::ALT);
    a.panels[0].cursor = a.panels[0]
        .entries
        .iter()
        .position(|e| e.path == regular)
        .unwrap();
    dn::discovery::open_sizes(&mut a);
    assert_eq!(a.desktop.active().id, sizes);
    assert!(a.modal_top().is_none());
    assert_eq!(
        a.desktop.tool(sizes).unwrap().discovery().unwrap().sources,
        sources
    );
    assert!(a
        .desktop
        .tool(sizes)
        .unwrap()
        .discovery()
        .unwrap()
        .job
        .is_none());
    Ok(())
}

#[test]
fn quick_directories_geometry_and_controls_stay_stable_across_redraws() -> io::Result<()> {
    use dn::{localization::Language, navigation_tools, palette::PaletteProfile};
    let f = support::Fixture::new("tools-bookmarks-stable")?;
    for language in [Language::English, Language::Russian] {
        for (width, height) in [(80, 25), (132, 40)] {
            let mut a = f.app()?;
            a.config.language = language;
            a.config.show_clock = false;
            a.config.palette = if width == 80 {
                PaletteProfile::ClassicDn
            } else {
                PaletteProfile::ClassicDn.with_slot(dn::palette::theme::Slot::DialogText, 0x1e)
            };
            navigation_tools::open_bookmarks(&mut a);
            let id = a.desktop.active().id;
            support::draw(&mut a, width, height)?;
            let initial = layout::prepare(&mut a, Rect::new(0, 0, width, height))
                .surfaces
                .into_iter()
                .find(|s| s.id.key == SurfaceKey::Document(id))
                .unwrap()
                .bounds;
            for _ in 0..20 {
                let frame = support::draw(&mut a, width, height)?;
                let current = layout::prepare(&mut a, Rect::new(0, 0, width, height))
                    .surfaces
                    .into_iter()
                    .find(|s| s.id.key == SurfaceKey::Document(id))
                    .unwrap()
                    .bounds;
                assert_eq!(
                    current, initial,
                    "Quick directories must not shrink on redraw"
                );
                for slot in 0..9 {
                    let row = a
                        .mouse
                        .regions
                        .iter()
                        .find(|(_, target)| {
                            *target == Target::Navigation(navigation_tools::Action::Slot(slot))
                        })
                        .expect("All nine bookmark rows remain reachable")
                        .0;
                    assert!(row.y > current.y && row.bottom() < current.bottom());
                    assert_eq!(frame[(row.x, row.y)].symbol(), (slot + 1).to_string());
                }
                for action in [
                    navigation_tools::Action::Go,
                    navigation_tools::Action::Set,
                    navigation_tools::Action::Clear,
                    navigation_tools::Action::Close,
                ] {
                    let button = a
                        .mouse
                        .regions
                        .iter()
                        .filter(|(_, target)| *target == Target::Navigation(action))
                        .max_by_key(|(bounds, _)| bounds.y)
                        .expect("Bookmark buttons remain reachable")
                        .0;
                    assert!(button.y > current.y + 9);
                    assert!(button.bottom() + 1 < current.bottom());
                }
                key(&mut a, K::Down, M::NONE);
            }
        }
    }
    Ok(())
}

#[test]
fn calculator_title_close_retires_document_and_keeps_session_state() -> io::Result<()> {
    let f = support::Fixture::new("calculator-title-close")?;
    let mut a = f.app()?;
    dn::utilities::open(&mut a, dn::utilities::Kind::Calculator);
    let id = a.desktop.active().id;
    input::paste(&mut a, "2+3");
    support::draw(&mut a, 100, 32)?;
    key(&mut a, K::F(5), M::CONTROL);
    key(&mut a, K::Right, M::NONE);
    key(&mut a, K::Down, M::NONE);
    key(&mut a, K::Enter, M::NONE);
    let frame = support::draw(&mut a, 100, 32)?;
    let close = a
        .mouse
        .regions
        .iter()
        .filter(|(_, target)| {
            *target == Target::Plugin(dn::plugins::CALCULATOR, dn::plugins::ActionId(0))
        })
        .min_by_key(|(r, _)| r.y)
        .expect("Calculator close control")
        .0;
    assert_eq!(frame[(close.x + 1, close.y)].symbol(), "■");
    for kind in [E::Down(B::Left), E::Up(B::Left)] {
        mouse::handle_ui(
            &mut a,
            MouseEvent {
                kind,
                column: close.x + 1,
                row: close.y,
                modifiers: M::NONE,
            },
        );
    }
    wait(&mut a, |a| !a.desktop.contains(id));
    assert!(!a.desktop.contains(id));
    dn::utilities::open(&mut a, dn::utilities::Kind::Calculator);
    assert_eq!(a.desktop.plugins.calculator().expression, "2+3");
    Ok(())
}

#[test]
fn quick_directories_go_closes_after_success_and_retains_failures() -> io::Result<()> {
    use dn::navigation_tools::{self, Action};
    for mouse in [false, true] {
        let f = support::Fixture::new("bookmarks-go-close")?.canonicalized()?;
        std::fs::create_dir(f.0.join("child"))?;
        let mut a = f.app()?;
        a.config.bookmarks[0] = Some(f.0.join("child"));
        navigation_tools::open_bookmarks(&mut a);
        let id = a.desktop.active().id;
        support::draw(&mut a, 100, 32)?;
        a.active = 1; // The window keeps its captured source pane.
        if mouse {
            support::click(&mut a, Target::Navigation(Action::Go));
        } else {
            key(&mut a, K::Enter, M::NONE);
        }
        assert!(
            a.desktop.contains(id),
            "Keep the window until navigation succeeds"
        );
        wait(&mut a, |a| !a.panels[0].loading);
        assert_eq!(a.panels[0].path, f.0.join("child"));
        assert_eq!(a.panels[1].path, f.0);
        assert!(!a.desktop.contains(id));
        assert_eq!(a.desktop.active().id, WindowId::MANAGER);

        navigation_tools::open_bookmarks(&mut a);
        let retry = a.desktop.active().id;
        navigation_tools::action(&mut a, Action::Slot(1));
        navigation_tools::action(&mut a, Action::Go);
        assert!(
            a.desktop.contains(retry),
            "An empty slot keeps the window open"
        );
        a.config.bookmarks[1] = Some(f.0.join("missing"));
        navigation_tools::action(&mut a, Action::Go);
        wait(&mut a, |a| !a.panels[0].loading);
        assert!(
            a.desktop.contains(retry),
            "Failed navigation keeps the window open"
        );
        assert_eq!(a.panels[0].path, f.0.join("child"));
    }
    Ok(())
}

#[test]
fn quick_directories_completion_cannot_close_a_reopened_window_or_steal_focus() -> io::Result<()> {
    use dn::navigation_tools::{self, Action};
    let f = support::Fixture::new("bookmarks-go-owner")?.canonicalized()?;
    std::fs::create_dir(f.0.join("child"))?;
    let mut a = f.app()?;
    dn::editor::open(&mut a, true);
    let editor = a.desktop.active().id;
    a.desktop.activate(WindowId::MANAGER);
    a.sync_modals();
    a.config.bookmarks[0] = Some(f.0.join("child"));
    navigation_tools::open_bookmarks(&mut a);
    let old = a.desktop.active().id;
    navigation_tools::action(&mut a, Action::Go);
    a.desktop.close(old);
    a.sync_modals();
    navigation_tools::open_bookmarks(&mut a);
    let reopened = a.desktop.active().id;
    a.desktop.activate(editor);
    a.sync_modals();
    wait(&mut a, |a| !a.panels[0].loading);
    assert!(a.desktop.contains(reopened));
    assert_eq!(a.desktop.active().id, editor);
    navigation_tools::open_bookmarks(&mut a);
    navigation_tools::action(&mut a, Action::Go);
    a.desktop.activate(editor);
    a.sync_modals();
    assert_eq!(
        a.desktop.active().id,
        editor,
        "Editor activated before completion"
    );
    wait(&mut a, |a| !a.panels[0].loading);
    assert!(!a.desktop.contains(reopened));
    assert_eq!(a.desktop.active().id, editor);
    Ok(())
}
