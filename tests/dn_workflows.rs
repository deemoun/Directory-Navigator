mod support;
use crossterm::event::{KeyCode as K, KeyEvent, KeyModifiers as M};
use dn::{
    app::App,
    discovery,
    focus::Focus,
    input, menu,
    settings::{self, SettingsKind},
    traversal::{self, Event, Job, Request},
    user_menu::{self, Action as U},
};
use std::{
    fs, io,
    path::PathBuf,
    time::{Duration, Instant},
};
fn key(a: &mut App, k: K) {
    input::handle(a, KeyEvent::new(k, M::NONE));
}
fn wait(a: &mut App, ready: impl Fn(&App) -> bool) {
    let end = Instant::now() + Duration::from_secs(8);
    while !ready(a) {
        a.poll();
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn search(
    root: PathBuf,
    options: traversal::find::Options,
) -> io::Result<(Vec<traversal::Hit>, traversal::Stats)> {
    let job = Job::start(Request::AdvancedFind {
        root,
        pattern: "*".into(),
        recursive: true,
        hidden: true,
        options,
    })?;
    let mut hits = Vec::new();
    loop {
        match job.events.recv_timeout(Duration::from_secs(5)).unwrap() {
            Event::Found(mut v) => hits.append(&mut v),
            Event::Done { stats, .. } => return Ok((hits, stats)),
            _ => {}
        }
    }
}
#[test]
fn content_search_handles_unicode_chunk_boundaries_and_reports_line() -> io::Result<()> {
    let f = support::Fixture::new("advanced-find-stream")?;
    let mut bytes = b"one\ntwo\n".to_vec();
    bytes.resize(65535, b'x');
    bytes.extend_from_slice("ПРИВЕТ界\nlast".as_bytes());
    fs::write(f.0.join("unicode.txt"), &bytes)?;
    fs::write(f.0.join("other.txt"), "unrelated")?;
    let options = traversal::find::Options {
        text: "привет界".into(),
        ..Default::default()
    };
    let (hits, stats) = search(f.0.clone(), options.clone())?;
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].line, Some(3));
    assert_eq!(stats.skipped, 0);
    assert!(search(
        f.0.clone(),
        traversal::find::Options {
            case_sensitive: true,
            ..options
        }
    )?
    .0
    .is_empty());
    assert_eq!(fs::read(f.0.join("unicode.txt"))?, bytes);
    Ok(())
}
#[test]
fn advanced_filters_validate_ranges_dates_types_and_skip_links() -> io::Result<()> {
    use traversal::find::{date, size, FileKind, Options};
    let f = support::Fixture::new("advanced-find-filters")?;
    fs::write(f.0.join("small"), "abc")?;
    fs::write(f.0.join("large"), vec![b'x'; 2048])?;
    fs::create_dir(f.0.join("directory"))?;
    let hits = search(
        f.0.clone(),
        Options {
            min_size: size("1K")?,
            max_size: size("2KiB")?,
            kind: FileKind::Files,
            ..Default::default()
        },
    )?
    .0;
    assert_eq!(hits.len(), 1);
    assert!(hits[0].path.ends_with("large"));
    assert_eq!(
        search(
            f.0.clone(),
            Options {
                kind: FileKind::Directories,
                ..Default::default()
            }
        )?
        .0
        .len(),
        1
    );
    assert!(Options {
        min_size: Some(10),
        max_size: Some(1),
        ..Default::default()
    }
    .validate()
    .is_err());
    assert!(date("2026-02-30", false).is_err());
    assert!(size("18446744073709551615G").is_err());
    let from = date("2026-10-30", false)?.unwrap();
    let to = date("2026-10-30", true)?.unwrap();
    assert_eq!(
        to.duration_since(from).unwrap(),
        Duration::new(86399, 999_999_999)
    );
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(f.0.join("small"), f.0.join("link"))?;
        let (hits, _) = search(
            f.0.clone(),
            Options {
                text: "abc".into(),
                ..Default::default()
            },
        )?;
        assert_eq!(hits.len(), 1);
        assert!(hits[0].path.ends_with("small"));
        assert_eq!(
            search(
                f.0.clone(),
                Options {
                    kind: FileKind::Links,
                    ..Default::default()
                }
            )?
            .0
            .len(),
            1
        );
    }
    Ok(())
}
#[test]
fn advanced_find_validation_background_results_and_match_navigation() -> io::Result<()> {
    let f = support::Fixture::new("advanced-find-owner")?.canonicalized()?;
    fs::write(f.0.join("match.txt"), "first\nsecond\nneedle\nlast")?;
    let mut a = f.app()?;
    discovery::open_find(&mut a);
    support::draw(&mut a, 80, 25)?;
    let find = a.desktop.active().id;
    a.discovery_mut().unwrap().prefs.advanced.fields[1] = "invalid".into();
    discovery::action(&mut a, discovery::Action::Start);
    assert!(a.discovery().unwrap().job.is_none());
    assert!(a.last_dialog().is_some());
    key(&mut a, K::Esc);
    let m = a.discovery_mut().unwrap();
    m.prefs.advanced.fields[1].clear();
    m.prefs.advanced.fields[0] = "needle".into();
    discovery::action(&mut a, discovery::Action::Start);
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
    a.desktop.activate(find);
    a.sync_modals();
    assert_eq!(a.discovery().unwrap().hits.len(), 1);
    discovery::action(&mut a, discovery::Action::Edit);
    let match_editor = a.desktop.active().id;
    wait(&mut a, |a| {
        a.desktop.editor(match_editor).unwrap().buffer.is_some()
    });
    assert_eq!(
        a.desktop
            .editor(match_editor)
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .row(),
        2
    );
    a.desktop.activate(find);
    a.sync_modals();
    discovery::action(&mut a, discovery::Action::View);
    let viewer = a.desktop.active().id;
    wait(&mut a, |a| {
        a.desktop.viewer(viewer).unwrap().contents.is_some()
    });
    support::draw(&mut a, 80, 25)?;
    assert_eq!(a.desktop.viewer(viewer).unwrap().anchor().0, 2);
    assert!(a.desktop.contains(find));
    Ok(())
}
fn open_menu(a: &mut App, path: PathBuf) -> io::Result<()> {
    user_menu::open_file(a, path, true);
    wait(a, |a| {
        a.user_menu
            .modals
            .user_menu()
            .is_some_and(|m| m.data.is_some())
    });
    support::draw(a, 80, 25)?;
    Ok(())
}
#[test]
fn parameter_prompts_keep_single_arguments_cancel_and_owned_help() -> io::Result<()> {
    let f = support::Fixture::new("f2-parameters")?;
    let mut a = f.app()?;
    let menu_path = f.0.join("dn.mnu");
    fs::write(
        &menu_path,
        ">1 Action\n? query 'Search term' 'default'\nprintf '%s' '{param:query}'\n",
    )?;
    open_menu(&mut a, menu_path)?;
    user_menu::action(&mut a, U::Choose(0));
    support::draw(&mut a, 80, 25)?;
    assert!(a.user_menu.modals.user_menu().unwrap().review.is_none());
    a.user_menu
        .modals
        .user_menu_mut()
        .unwrap()
        .parameters
        .as_mut()
        .unwrap()
        .values[0]
        .clear();
    a.user_menu
        .modals
        .user_menu_mut()
        .unwrap()
        .parameters
        .as_mut()
        .unwrap()
        .cursors[0] = 0;
    input::paste(&mut a, "界 $(touch never-created) ; spaces");
    key(&mut a, K::F(1));
    assert_eq!(dn::focus::current(&a), Focus::Message);
    key(&mut a, K::Esc);
    assert_eq!(
        a.user_menu
            .modals
            .user_menu()
            .unwrap()
            .parameters
            .as_ref()
            .unwrap()
            .values[0],
        "界 $(touch never-created) ; spaces"
    );
    key(&mut a, K::Enter);
    let m = a.user_menu.modals.user_menu().unwrap();
    assert!(m.parameters.is_none());
    assert_eq!(
        m.review.as_ref().unwrap().commands[0].args[1],
        "界 $(touch never-created) ; spaces"
    );
    assert!(a.user_menu.pending.is_none());
    assert!(!f.0.join("never-created").exists());
    key(&mut a, K::Esc);
    user_menu::action(&mut a, U::Choose(0));
    key(&mut a, K::Esc);
    assert!(a.user_menu.modals.user_menu().unwrap().parameters.is_none());
    assert!(a.user_menu.pending.is_none());
    Ok(())
}
#[test]
fn both_pane_selections_lists_are_dry_until_run_and_clean_on_failure() -> io::Result<()> {
    let f = support::Fixture::new("f2-lists")?.canonicalized()?;
    fs::write(f.0.join("one space.txt"), "preserved")?;
    fs::write(f.0.join("two.txt"), "preserved")?;
    let mut a = f.app()?;
    for (side, name) in [(0, "one space.txt"), (1, "two.txt")] {
        a.panels[side].selected.insert(name.into());
    }
    let commands = vec![vec![
        "/bin/cp".into(),
        "{selected_list}".into(),
        f.0.join("output").to_string_lossy().into_owned(),
    ]];
    let p = user_menu::plan(&a, &commands)?;
    let list = p.list_files[0].path().to_owned();
    assert!(!list.exists());
    assert!(user_menu::runner::run(&p)?.success());
    assert!(!list.exists());
    assert!(!list.parent().unwrap().exists());
    assert_eq!(
        fs::read(f.0.join("output"))?,
        format!("{}\n", f.0.join("one space.txt").display()).as_bytes()
    );
    let p = user_menu::plan(
        &a,
        &[vec![
            "missing-dn-test-executable".into(),
            "{other_selected_list}".into(),
            "{other_selected}".into(),
        ]],
    )?;
    assert_eq!(p.commands[0].args[1], f.0.join("two.txt"));
    let list = p.list_files[0].path().to_owned();
    assert!(user_menu::runner::run(&p).is_err());
    assert!(!list.exists());
    assert!(!list.parent().unwrap().exists());
    assert_eq!(fs::read_to_string(f.0.join("one space.txt"))?, "preserved");
    Ok(())
}
#[test]
fn list_collisions_and_unrepresentable_names_are_rejected_without_cleanup_of_others(
) -> io::Result<()> {
    let f = support::Fixture::new("f2-list-collision")?;
    let list = user_menu::runner::SelectedList::new(vec![f.0.join("a")])?;
    let root = list.path().parent().unwrap().to_owned();
    fs::create_dir(&root)?;
    fs::write(root.join("keep"), "preserved")?;
    let plan = user_menu::runner::Plan {
        cwd: f.0.clone(),
        commands: vec![user_menu::runner::CommandSpec {
            program: "/usr/bin/true".into(),
            args: vec![],
        }],
        env: vec![],
        list_files: vec![list],
    };
    assert!(user_menu::runner::run(&plan).is_err());
    assert_eq!(fs::read_to_string(root.join("keep"))?, "preserved");
    fs::remove_dir_all(root)?;
    assert!(user_menu::runner::SelectedList::new(vec![f.0.join("line\nbreak")]).is_err());
    assert!(user_menu::format::parse(">1 Bad\n? x Label\n? x Again\nprintf '{param:x}'").is_err());
    Ok(())
}
#[test]
fn highlight_groups_apply_order_case_and_preserve_directories_links_and_config() -> io::Result<()> {
    let f = support::Fixture::new("highlight-config")?;
    fs::write(f.0.join("SOURCE.RS"), "file")?;
    fs::create_dir(f.0.join("folder.rs"))?;
    let entries = dn::filesystem::scan(&f.0, true)?;
    let source = entries.iter().find(|e| e.name == "SOURCE.RS").unwrap();
    let dir = entries.iter().find(|e| e.name == "folder.rs").unwrap();
    let mut c = dn::config::Config::default();
    assert_eq!(c.highlight_groups.color(source), None);
    c.highlight_groups.0[0] = dn::highlight_groups::Group {
        enabled: true,
        masks: "*.rs;*.txt".into(),
        color: 4,
    };
    c.highlight_groups.0[1] = dn::highlight_groups::Group {
        enabled: true,
        masks: "*".into(),
        color: 2,
    };
    assert_eq!(c.highlight_groups.color(source), Some(0x84));
    assert_eq!(c.highlight_groups.color(dir), None);
    let path = f.0.join("config.conf");
    c.save(&path)?;
    assert_eq!(dn::config::Config::read(&path)?, c);
    let text = fs::read_to_string(&path)?;
    assert!(dn::config::Config::parse(text.replace("color = 4", "color = 99").as_bytes()).is_err());
    let old = dn::config::Config::parse(b"config_version=2\n[future]\nvalue=7\n")?;
    fs::write(&path, b"config_version=2\n[future]\nvalue=7\n")?;
    old.save(&path)?;
    assert!(fs::read_to_string(&path)?.contains("[future]"));
    Ok(())
}
#[test]
fn highlight_drafts_keyboard_mouse_paste_and_repeated_frames() -> io::Result<()> {
    let f = support::Fixture::new("highlight-ui")?;
    let mut a = f.app()?;
    let original = a.config.clone();
    menu::execute(&mut a, menu::Action::Settings(SettingsKind::Highlight));
    support::draw(&mut a, 80, 25)?;
    key(&mut a, K::Tab);
    key(&mut a, K::Char(' '));
    key(&mut a, K::Tab);
    assert_eq!(a.modals.settings().unwrap().cursor, 2);
    input::paste(&mut a, ";*.界");
    key(&mut a, K::Tab);
    key(&mut a, K::Right);
    assert!(a.modals.settings().unwrap().draft.highlight_groups.0[0].enabled);
    key(&mut a, K::Esc);
    assert_eq!(a.config, original);
    menu::execute(&mut a, menu::Action::Settings(SettingsKind::Highlight));
    a.modals.settings_mut().unwrap().toggle(1);
    settings::accept(&mut a);
    assert!(a.config.highlight_groups.0[0].enabled);
    for language in [
        dn::localization::Language::English,
        dn::localization::Language::Russian,
    ] {
        a.config.language = language;
        discovery::open_find(&mut a);
        for (w, h) in [(80, 25), (132, 40)] {
            let prepared =
                dn::layout::prepare(&mut a, dn::presentation::layout::Rect::new(0, 0, w, h));
            let frame = dn::ui::render(&a, &prepared);
            let hits = frame.hit_regions().to_vec();
            assert_eq!(
                dn::testing::Buffer::rendered(frame),
                dn::testing::Buffer::rendered(dn::ui::render(&a, &prepared))
            );
            assert!(hits.iter().all(|(r, _)| r.right() <= w && r.bottom() <= h));
        }
        key(&mut a, K::Esc);
    }
    Ok(())
}

#[test]
fn group_foregrounds_respect_custom_background_and_yield_to_marks() -> io::Result<()> {
    use dn::palette::theme::Slot;
    let f = support::Fixture::new("highlight-custom-priority")?;
    fs::write(f.0.join("SOURCE.RS"), "preserved")?;
    let mut a = f.app()?;
    a.config.palette = a
        .config
        .palette
        .with_slot(Slot::PanelSource, 0x82)
        .with_slot(Slot::PanelText, 0x17);
    a.config.highlight_groups.0[0] = dn::highlight_groups::Group {
        enabled: true,
        masks: "*.rs".into(),
        color: 11,
    };
    fn first_cell(a: &mut App) -> io::Result<dn::presentation::style::Color> {
        let snap = dn::layout::prepare(a, dn::presentation::layout::Rect::new(0, 0, 80, 25));
        let b = dn::testing::Buffer::rendered(dn::ui::render(a, &snap));
        for y in 1..23 {
            for x in 0..31 {
                let value = (x..x + 9).map(|x| b[(x, y)].symbol()).collect::<String>();
                if value == "SOURCE.RS" {
                    return Ok(b[(x, y)].fg);
                }
            }
        }
        Err(io::Error::other("file name not rendered"))
    }
    assert_eq!(first_cell(&mut a)?, a.config.palette.color(11));
    a.panels[0].selected.insert("SOURCE.RS".into());
    assert_eq!(
        first_cell(&mut a)?,
        a.config
            .palette
            .color(a.config.palette.slot(Slot::PanelHeader) & 15)
    );
    Ok(())
}

#[test]
fn cancelled_content_reads_and_parameter_snapshots_keep_their_context() -> io::Result<()> {
    let f = support::Fixture::new("advanced-cancel-context")?;
    let path = f.0.join("read.txt");
    fs::write(&path, "text")?;
    let cancel = dn::task::Cancellation::default();
    cancel.cancel();
    let error = traversal::find::Options {
        text: "text".into(),
        ..Default::default()
    }
    .content(&path, &fs::symlink_metadata(&path)?, &cancel)
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Interrupted);
    let mut a = f.app()?;
    let cwd = a.panels[0].path.clone();
    let menu_path = f.0.join("dn.mnu");
    fs::write(
        &menu_path,
        ">1 Test\n? value Label default\nprintf '{dir}' '{param:value}'\n",
    )?;
    open_menu(&mut a, menu_path)?;
    user_menu::action(&mut a, U::Choose(0));
    a.panels[0].path = f.0.join("another");
    user_menu::action(&mut a, U::ParametersAccept);
    let p = a
        .user_menu
        .modals
        .user_menu()
        .unwrap()
        .review
        .as_ref()
        .unwrap();
    assert_eq!(p.cwd, cwd);
    assert_eq!(p.commands[0].args[0], cwd);
    assert!(a.user_menu.pending.is_none());
    Ok(())
}
