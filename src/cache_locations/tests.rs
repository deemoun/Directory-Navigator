use super::*;
use crate::{
    plugins,
    testing::{backend::TestBackend, Terminal},
};
use std::{
    fs, io,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
struct Temp(PathBuf);
impl Temp {
    fn new() -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "dn-cache-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p)?;
        Ok(Self(p))
    }
    fn rows(&self) -> io::Result<Vec<Location>> {
        for name in ["cache 界", "temp"] {
            fs::create_dir_all(self.0.join(name))?;
        }
        fs::write(self.0.join("cache 界/.hidden"), b"12345")?;
        fs::write(self.0.join("temp/file"), b"123")?;
        Ok(catalog::resolve(
            vec![
                Location::new("User caches", "cache", Some(self.0.join("cache 界"))),
                Location::new(
                    "Current temporary directory",
                    "temp",
                    Some(self.0.join("temp")),
                ),
            ],
            &Cancellation::default(),
        ))
    }
    fn app(&self) -> io::Result<(App, Binding)> {
        let mut app = App::new(self.0.clone())?;
        app.config.show_clock = false;
        let end = Instant::now() + Duration::from_secs(5);
        while app.panels.iter().any(|p| p.loading) {
            app.poll();
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(1));
        }
        plugins::host::open(&mut app, CACHE_LOCATIONS);
        let b = app.desktop.plugins.binding(CACHE_LOCATIONS).unwrap();
        app.cache_locations.service = None;
        app.cache_locations.view = Snapshot {
            rows: self.rows()?,
            ..Default::default()
        };
        Ok((app, b))
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn pump(app: &mut App, done: impl Fn(&App) -> bool) {
    let end = Instant::now() + Duration::from_secs(5);
    while !done(app) {
        app.poll();
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(1));
    }
}
#[test]
fn platform_catalogs_use_injected_native_roots_and_ignore_relative_overrides() -> io::Result<()> {
    let t = Temp::new()?;
    for platform in [
        catalog::Platform::Mac,
        catalog::Platform::Linux,
        catalog::Platform::Windows,
    ] {
        let rows = catalog::candidates(
            platform,
            catalog::Inputs {
                home: Some(t.0.clone()),
                cache: Some("relative".into()),
                local: Some(t.0.join("Local")),
                windows: Some(t.0.join("Windows")),
                temp: Some(t.0.join("Temp")),
                ..Default::default()
            },
        );
        assert_eq!(
            rows.len(),
            match platform {
                catalog::Platform::Mac => 32,
                catalog::Platform::Linux => 6,
                catalog::Platform::Windows => 4,
            }
        );
        assert!(rows
            .iter()
            .filter_map(|r| r.path.as_ref())
            .all(|p| p.is_absolute()));
        match platform {
            catalog::Platform::Mac => assert_eq!(rows[0].path, Some(t.0.join("Library/Caches"))),
            catalog::Platform::Linux => assert_eq!(rows[0].path, Some(t.0.join(".cache"))),
            catalog::Platform::Windows => assert_eq!(rows[0].path, Some(t.0.join("Local/Temp"))),
        }
    }
    let rows = catalog::candidates(
        catalog::Platform::Linux,
        catalog::Inputs {
            cache: Some(t.0.join("redirected")),
            ..Default::default()
        },
    );
    assert_eq!(rows[0].path, Some(t.0.join("redirected")));
    Ok(())
}
#[test]
fn discovery_deduplicates_aliases_and_distinguishes_missing_unresolved_invalid() -> io::Result<()> {
    let t = Temp::new()?;
    let rows = t.rows()?;
    let p = rows[0].path.clone();
    let rows = catalog::resolve(
        vec![
            Location::new("a", "a", p.clone()),
            Location::new("b", "b", p),
            Location::new("missing", "m", Some(t.0.join("absent"))),
            Location::new("unknown", "u", None),
            Location::new("file", "f", Some(t.0.join("temp/file"))),
        ],
        &Cancellation::default(),
    );
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0].status, Status::NotAnalyzed);
    assert_eq!(rows[1].status, Status::Missing);
    assert_eq!(rows[2].status, Status::Unresolved);
    assert_eq!(rows[3].status, Status::Unavailable);
    assert!(rows.iter().all(|r| r.measurement.is_none()));
    Ok(())
}
#[test]
fn selected_and_sequential_all_measure_hidden_entries_without_mutation() -> io::Result<()> {
    let t = Temp::new()?;
    let rows = t.rows()?;
    let view = Snapshot {
        rows,
        ..Default::default()
    };
    let latest = Arc::new(Mutex::new(None));
    let selected = scan(view, Some(0), Cancellation::default(), latest.clone());
    assert_eq!(selected.rows[0].measurement.as_ref().unwrap().bytes, 5);
    assert_eq!(selected.rows[0].status, Status::Complete);
    assert!(selected.rows[1].measurement.is_none());
    let all = scan(selected, None, Cancellation::default(), latest);
    assert_eq!(all.rows[1].measurement.as_ref().unwrap().bytes, 3);
    assert!(all.rows.iter().all(|r| r.status == Status::Complete));
    assert_eq!(fs::read(t.0.join("cache 界/.hidden"))?, b"12345");
    let cancel = Cancellation::default();
    cancel.cancel();
    let partial = scan(all, None, cancel, Arc::default());
    assert!(partial.rows.iter().all(|r| r.status == Status::Cancelled));
    Ok(())
}
#[test]
fn changed_roots_stale_generations_navigation_and_closed_owners_are_safe() -> io::Result<()> {
    let t = Temp::new()?;
    let (mut app, b) = t.app()?;
    let other = app.panels[1].path.clone();
    action(&mut app, b, 0, Action::Analyze(Some(0)));
    action(&mut app, b, 0, Action::Stop);
    assert!(!app.cache_locations.view.stopping);
    app.desktop.activate(crate::desktop::WindowId::MANAGER);
    pump(&mut app, |a| !a.cache_locations.view.busy);
    assert_eq!(app.desktop.active().id, crate::desktop::WindowId::MANAGER);
    assert_eq!(app.cache_locations.view.rows[0].status, Status::Complete);
    let current = app.cache_locations.view.generation;
    let target = app.cache_locations.view.rows[0].path.clone().unwrap();
    action(&mut app, b, current, Action::Open(0));
    pump(&mut app, |a| !a.panels[0].loading);
    assert_eq!(app.panels[0].path, target);
    assert_eq!(app.panels[1].path, other);
    assert_eq!(app.desktop.plugins.binding(CACHE_LOCATIONS), Some(b));
    action(&mut app, b, current, Action::Analyze(None));
    app.config.set_plugin_enabled(CACHE_LOCATIONS, false);
    app.poll();
    assert!(app.cache_locations.service.is_none());
    let view = Snapshot {
        rows: t.rows()?,
        ..Default::default()
    };
    fs::remove_file(t.0.join("cache 界/.hidden"))?;
    fs::remove_dir(t.0.join("cache 界"))?;
    let changed = scan(view, Some(0), Cancellation::default(), Arc::default());
    assert_eq!(changed.rows[0].status, Status::Changed);
    Ok(())
}
#[cfg(unix)]
#[test]
fn aliases_links_hardlinks_and_native_names_are_not_lost() -> io::Result<()> {
    use std::os::unix::{ffi::OsStringExt, fs::symlink};
    let t = Temp::new()?;
    let mut rows = t.rows()?;
    let root = rows[0].path.clone().unwrap();
    symlink(&root, t.0.join("alias"))?;
    rows.push(Location::new("alias", "alias", Some(t.0.join("alias"))));
    assert_eq!(catalog::resolve(rows, &Cancellation::default()).len(), 2);
    fs::hard_link(root.join(".hidden"), root.join("hard"))?;
    // Native non-Unicode values survive the model without asking APFS to
    // create an invalid UTF-8 filename (which macOS correctly rejects).
    let native = root.join(std::ffi::OsString::from_vec(vec![b'n', 0xff]));
    let candidates = catalog::candidates(
        catalog::Platform::Linux,
        catalog::Inputs {
            cache: Some(native.clone()),
            ..Default::default()
        },
    );
    assert_eq!(candidates[0].path, Some(native.clone()));
    #[cfg(target_os = "linux")]
    fs::write(native, b"12")?;
    symlink(t.0.join("temp"), root.join("link"))?;
    let link_len = fs::symlink_metadata(root.join("link"))?.len();
    let value = scan(
        Snapshot {
            rows: t.rows()?,
            ..Default::default()
        },
        Some(0),
        Cancellation::default(),
        Arc::default(),
    );
    let m = value.rows[0].measurement.as_ref().unwrap();
    assert_eq!(
        m.bytes,
        (if cfg!(target_os = "linux") { 12 } else { 10 }) + u128::from(link_len)
    );
    assert_eq!(m.links, 1);
    Ok(())
}
#[test]
fn localized_controls_are_immutable_and_paint_never_starts_analysis() -> io::Result<()> {
    let t = Temp::new()?;
    for language in [
        crate::localization::Language::English,
        crate::localization::Language::Russian,
    ] {
        let (mut app, binding) = t.app()?;
        app.cache_locations.view.rows = catalog::candidates(
            catalog::Platform::Mac,
            catalog::Inputs {
                home: Some(t.0.clone()),
                temp: Some(t.0.join("temporary")),
                ..Default::default()
            },
        );
        // Exercise the expanded catalog below the initial seven visible rows.
        plugins::host::dispatch(
            &mut app,
            binding,
            plugins::Event::Key(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::End,
                crossterm::event::KeyModifiers::NONE,
            )),
        );
        app.config.language = language;
        app.config.palette = app
            .config
            .palette
            .with_slot(crate::palette::theme::Slot::DialogText, 0x2f);
        for (w, h) in [(80, 25), (120, 40), (80, 25)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h))?;
            let mut old = None;
            for _ in 0..3 {
                terminal.draw(|f| crate::ui::draw(f, &mut app))?;
                assert!(app.cache_locations.service.is_none());
                let next = terminal.backend().buffer().content.clone();
                if let Some(previous) = old.replace(next.clone()) {
                    assert_eq!(previous, next);
                }
                assert!(app
                    .mouse
                    .regions
                    .iter()
                    .all(|(r, _)| r.right() <= w && r.bottom() <= h));
            }
        }
    }
    Ok(())
}

#[test]
fn keyboard_mouse_copy_and_double_click_preserve_the_originating_pane() -> io::Result<()> {
    use crossterm::event::{
        KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    let t = Temp::new()?;
    let (mut app, b) = t.app()?;
    assert!(crate::config::Config::default().plugin_enabled(CACHE_LOCATIONS));
    use crate::plugins::ToolPlugin;
    let now = Instant::now();
    let ctx = plugins::host::context(
        &app,
        b,
        crate::presentation::layout::Rect::new(0, 0, 80, 25),
        now,
    )
    .unwrap();
    let mut tool = plugins::cache_directories::CacheDirectories::default();
    // A fixed clock makes the 400 ms boundary deterministic on busy CI hosts.
    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 0,
        row: 0,
        modifiers: KeyModifiers::NONE,
    };
    let old = app.panels[0].path.clone();
    assert!(tool
        .event(
            plugins::Event::Mouse(click, Some(plugins::ActionId(100))),
            &ctx
        )
        .is_empty());
    assert!(tool
        .event(
            plugins::Event::Mouse(click, Some(plugins::ActionId(101))),
            &ctx
        )
        .is_empty());
    let effects = tool.event(
        plugins::Event::Mouse(click, Some(plugins::ActionId(101))),
        &ctx,
    );
    assert!(effects.iter().any(|e| matches!(
        e,
        plugins::Effect::CacheLocations {
            action: Action::Open(1),
            ..
        }
    )));
    action(&mut app, b, 0, Action::Open(1));
    pump(&mut app, |a| !a.panels[0].loading);
    assert_eq!(app.panels[0].path, fs::canonicalize(t.0.join("temp"))?);
    assert_eq!(app.panels[1].path, old);
    let text = app.cache_locations.view.rows[1]
        .path
        .as_ref()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    action(&mut app, b, 0, Action::Copy(1));
    assert_eq!(app.editor_clipboard, text);
    // Keyboard input belongs to the live foreground document.
    let Owner::Document(owner) = b.owner else {
        unreachable!()
    };
    crate::windowing::activate(&mut app, owner);
    // Keyboard navigation resets the double click and Enter opens the selected row.
    plugins::host::dispatch(
        &mut app,
        b,
        plugins::Event::Key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE)),
    );
    plugins::host::dispatch(&mut app, b, plugins::Event::Action(plugins::ActionId(0)));
    pump(&mut app, |a| !a.panels[0].loading);
    assert_eq!(app.panels[0].path, fs::canonicalize(t.0.join("cache 界"))?);
    Ok(())
}

#[test]
fn failed_or_cancelled_workers_never_leave_running_or_complete_labels() -> io::Result<()> {
    let t = Temp::new()?;
    for cancelled in [false, true] {
        let mut view = Snapshot {
            generation: 4,
            rows: t.rows()?,
            busy: true,
            ..Default::default()
        };
        view.rows[0].status = Status::Scanning;
        view.rows[0].measurement = Some(Measurement {
            bytes: 5,
            ..Default::default()
        });
        failed(&mut view, cancelled, "private mock input error");
        assert_eq!(view.generation, 5);
        assert_eq!(
            view.rows[0].status,
            if cancelled {
                Status::Cancelled
            } else {
                Status::Partial
            }
        );
        assert!(view.rows[0].measurement.as_ref().unwrap().partial);
        assert_eq!(view.rows[0].cause, "private mock input error");
        assert_eq!(view.rows[1].status, Status::NotAnalyzed);
    }
    Ok(())
}

#[test]
fn cancelled_refresh_reply_preserves_the_previous_catalog_and_measurements() -> io::Result<()> {
    let t = Temp::new()?;
    let (mut app, binding) = t.app()?;
    app.cache_locations.view.rows[0].measurement = Some(Measurement {
        bytes: 5,
        ..Default::default()
    });
    app.cache_locations.view.rows[0].status = Status::Complete;
    let paths: Vec<_> = app
        .cache_locations
        .view
        .rows
        .iter()
        .map(|r| r.path.clone())
        .collect();
    let Owner::Document(owner) = binding.owner else {
        unreachable!()
    };
    let worker = app.workers.spawn(
        "dn-cache-cancel-test",
        &app.cache_locations.requests,
        crate::requests::Owner::Document(owner),
        "cache-locations",
        |_| Ok(Snapshot::default()),
    )?;
    app.cache_locations.service = Some(Service {
        binding,
        refreshing: true,
        worker,
        latest: Arc::default(),
    });
    app.cache_locations.view.busy = true;
    app.cache_locations.view.stopping = true;
    pump(&mut app, |a| !a.cache_locations.view.busy);
    assert_eq!(
        app.cache_locations
            .view
            .rows
            .iter()
            .map(|r| r.path.clone())
            .collect::<Vec<_>>(),
        paths
    );
    let measured = &app.cache_locations.view.rows[0];
    assert_eq!(measured.measurement.as_ref().unwrap().bytes, 5);
    assert_eq!(measured.status, Status::Complete);
    assert!(measured.stale);
    assert_eq!(
        app.cache_locations.view.notice,
        "Refresh cancelled; previous locations retained. Measurements are stale."
    );
    assert!(!app.cache_locations.view.stopping);
    Ok(())
}

#[test]
fn changed_locations_cannot_open_through_button_enter_or_double_click() -> io::Result<()> {
    use crate::plugins::ToolPlugin;
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    let t = Temp::new()?;
    let (mut app, binding) = t.app()?;
    app.cache_locations.view.rows[0].status = Status::Changed;
    let old = app.panels[0].path.clone();
    action(&mut app, binding, 0, Action::Open(0));
    assert!(!app.panels[0].loading);
    assert_eq!(app.panels[0].path, old);
    let ctx = plugins::host::context(
        &app,
        binding,
        crate::presentation::layout::Rect::new(0, 0, 80, 25),
        Instant::now(),
    )
    .unwrap();
    let mut tool = plugins::cache_directories::CacheDirectories::default();
    assert!(
        !tool
            .controls(&ctx)
            .controls
            .iter()
            .find(|c| c.id == 5)
            .unwrap()
            .enabled
    );
    for id in [0, 5] {
        assert!(tool
            .event(plugins::Event::Action(plugins::ActionId(id)), &ctx)
            .is_empty());
    }
    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 0,
        row: 0,
        modifiers: KeyModifiers::NONE,
    };
    for _ in 0..2 {
        assert!(tool
            .event(
                plugins::Event::Mouse(click, Some(plugins::ActionId(100))),
                &ctx
            )
            .is_empty());
    }
    Ok(())
}

#[test]
fn worker_admission_error_is_available_in_help_without_losing_results() -> io::Result<()> {
    use crate::plugins::ToolPlugin;
    let t = Temp::new()?;
    let (mut app, binding) = t.app()?;
    pump(&mut app, |a| !a.workers.pending());
    let Owner::Document(owner) = binding.owner else {
        unreachable!()
    };
    let mut scopes = Vec::new();
    let mut jobs = Vec::new();
    let mut cause = None;
    for _ in 0..9 {
        scopes.push(crate::requests::RequestScope::default());
        match app.workers.spawn(
            "dn-cache-admission-test",
            scopes.last().unwrap(),
            crate::requests::Owner::Document(owner),
            "admission",
            |cancel| {
                while !cancel.cancelled() {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Ok(())
            },
        ) {
            Ok(job) => jobs.push(job),
            Err(error) => {
                cause = Some(error.to_string());
                break;
            }
        }
    }
    let cause = cause.expect("supervisor admission must be bounded");
    let paths: Vec<_> = app
        .cache_locations
        .view
        .rows
        .iter()
        .map(|r| r.path.clone())
        .collect();
    action(&mut app, binding, 0, Action::Refresh);
    assert!(!app.cache_locations.view.busy);
    assert_eq!(
        app.cache_locations
            .view
            .rows
            .iter()
            .map(|r| r.path.clone())
            .collect::<Vec<_>>(),
        paths
    );
    let ctx = plugins::host::context(
        &app,
        binding,
        crate::presentation::layout::Rect::new(0, 0, 80, 25),
        Instant::now(),
    )
    .unwrap();
    let mut tool = plugins::cache_directories::CacheDirectories::default();
    let effects = tool.event(plugins::Event::Action(plugins::ActionId(7)), &ctx);
    assert!(effects
        .iter()
        .any(|e| matches!(e,plugins::Effect::Help{text,..} if text.contains(&cause))));
    drop(jobs);
    Ok(())
}

#[test]
fn opening_while_originating_pane_loads_explains_the_wait_without_navigation() -> io::Result<()> {
    let t = Temp::new()?;
    let (mut app, binding) = t.app()?;
    let old = [app.panels[0].path.clone(), app.panels[1].path.clone()];
    app.active = 1;
    app.panels[0].loading = true;
    action(&mut app, binding, 0, Action::Open(0));
    assert_eq!(
        app.cache_locations.view.notice,
        "Wait for directory navigation to finish, then Open again."
    );
    assert_eq!(
        [app.panels[0].path.clone(), app.panels[1].path.clone()],
        old
    );
    assert_eq!(app.desktop.plugins.binding(CACHE_LOCATIONS), Some(binding));
    assert_eq!(
        app.desktop.active().id,
        match binding.owner {
            Owner::Document(id) => id,
            _ => unreachable!(),
        }
    );
    app.panels[0].loading = false;
    Ok(())
}

#[test]
fn refresh_cancellation_before_or_during_discovery_discards_partial_catalogs() -> io::Result<()> {
    let t = Temp::new()?;
    let mut original = Snapshot {
        generation: 31,
        rows: t.rows()?,
        busy: true,
        ..Default::default()
    };
    original.rows[0].measurement = Some(Measurement {
        bytes: 5,
        ..Default::default()
    });
    original.rows[0].status = Status::Complete;
    let paths: Vec<_> = original.rows.iter().map(|r| r.path.clone()).collect();
    for before in [true, false] {
        let cancel = Cancellation::default();
        if before {
            cancel.cancel();
        }
        let result = refresh(original.clone(), &cancel, |c| {
            assert!(!before, "cancelled discovery must not run");
            c.cancel();
            Vec::new()
        });
        assert_eq!(
            result
                .rows
                .iter()
                .map(|r| r.path.clone())
                .collect::<Vec<_>>(),
            paths
        );
        assert_eq!(result.generation, 31);
        assert_eq!(result.rows[0].measurement.as_ref().unwrap().bytes, 5);
        assert_eq!(result.rows[0].status, Status::Complete);
        assert!(result.rows[0].stale);
        assert!(!result.busy);
        assert!(!result.stopping);
    }
    // A normal refresh still merges measurements by native identity.
    let rows = t.rows()?;
    let result = refresh(original, &Cancellation::default(), |_| rows);
    assert_eq!(result.rows[0].measurement.as_ref().unwrap().bytes, 5);
    assert!(result.rows[0].stale);
    assert!(result.rows[1].measurement.is_none());
    Ok(())
}

#[test]
fn refresh_cancelled_before_worker_entry_preserves_rows_and_retires_actions() -> io::Result<()> {
    let t = Temp::new()?;
    let (mut app, binding) = t.app()?;
    let paths: Vec<_> = app
        .cache_locations
        .view
        .rows
        .iter()
        .map(|r| r.path.clone())
        .collect();
    let Owner::Document(owner) = binding.owner else {
        unreachable!()
    };
    let worker = app.workers.spawn(
        "dn-cache-interrupted-test",
        &app.cache_locations.requests,
        crate::requests::Owner::Document(owner),
        "cache-locations",
        |_| Err(io::Error::from(io::ErrorKind::Interrupted)),
    )?;
    app.cache_locations.service = Some(Service {
        binding,
        refreshing: true,
        worker,
        latest: Arc::default(),
    });
    app.cache_locations.view.busy = true;
    pump(&mut app, |a| !a.cache_locations.view.busy);
    assert_eq!(
        app.cache_locations
            .view
            .rows
            .iter()
            .map(|r| r.path.clone())
            .collect::<Vec<_>>(),
        paths
    );
    assert_eq!(app.cache_locations.view.generation, 1);
    assert_eq!(
        app.cache_locations.view.notice,
        "Refresh cancelled; previous locations retained. Measurements are stale."
    );
    action(&mut app, binding, 0, Action::Open(0));
    assert!(!app.panels[0].loading);
    Ok(())
}

#[test]
fn mac_developer_locations_are_curated_and_preserve_existing_order() -> io::Result<()> {
    let t = Temp::new()?;
    let rows = catalog::candidates(
        catalog::Platform::Mac,
        catalog::Inputs {
            home: Some(t.0.join("Home 世界")),
            temp: Some(t.0.join("temporary")),
            ..Default::default()
        },
    );
    assert_eq!(rows.len(), 32);
    assert_eq!(rows[0].name, "User caches");
    assert_eq!(rows[7].name, "System diagnostic logs");
    let find = |name| rows.iter().find(|r| r.name == name).unwrap();
    let home = t.0.join("Home 世界");
    assert_eq!(
        find("Xcode archives (valuable)").path,
        Some(home.join("Library/Developer/Xcode/Archives"))
    );
    assert!(find("Xcode archives (valuable)")
        .purpose
        .contains("valuable"));
    assert!(find("Simulator devices (data)")
        .purpose
        .contains("user data"));
    assert!(find("Rust toolchains (installed)")
        .purpose
        .contains("Installed"));
    assert_eq!(
        find("Cargo registry cache").path,
        Some(home.join(".cargo/registry"))
    );
    assert_eq!(
        find("Gradle distributions").path,
        Some(home.join(".gradle/wrapper/dists"))
    );
    assert_eq!(
        find("Android virtual devices (data)").path,
        Some(home.join(".android/avd"))
    );
    assert!(rows
        .iter()
        .all(|r| r.identity.is_none() && r.measurement.is_none()));
    assert!(
        !home.exists(),
        "catalog construction must not create or scan developer directories"
    );
    assert!(!rows
        .iter()
        .filter_map(|r| r.path.as_ref())
        .any(|p| p == &home
            || p.ends_with("Documents")
            || p.ends_with("node_modules")
            || p.ends_with("target")));
    Ok(())
}

#[test]
fn mac_developer_overrides_are_absolute_native_paths_without_affecting_other_platforms(
) -> io::Result<()> {
    let t = Temp::new()?;
    let input = || catalog::Inputs {
        home: Some(t.0.join("home")),
        developer: catalog::DeveloperHomes {
            cargo: Some(t.0.join("Rust 世界")),
            rustup: Some("relative".into()),
            gradle: Some(PathBuf::new()),
            npm: Some(t.0.join("packages")),
            android: Some(t.0.join("SDK")),
            avd: Some(t.0.join("devices")),
        },
        ..Default::default()
    };
    let rows = catalog::candidates(catalog::Platform::Mac, input());
    for (name, path) in [
        ("Cargo Git cache", t.0.join("Rust 世界/git")),
        (
            "Rust toolchains (installed)",
            t.0.join("home/.rustup/toolchains"),
        ),
        ("Gradle caches", t.0.join("home/.gradle/caches")),
        ("npm cache", t.0.join("packages")),
        ("Android SDK (installed)", t.0.join("SDK")),
        ("Android virtual devices (data)", t.0.join("devices")),
    ] {
        assert_eq!(
            rows.iter().find(|r| r.name == name).unwrap().path,
            Some(path)
        );
    }
    for (platform, count) in [
        (catalog::Platform::Linux, 6),
        (catalog::Platform::Windows, 4),
    ] {
        assert_eq!(catalog::candidates(platform, input()).len(), count);
    }
    let missing = catalog::candidates(catalog::Platform::Mac, Default::default());
    assert!(missing
        .iter()
        .find(|r| r.name == "Xcode archives (valuable)")
        .unwrap()
        .path
        .is_none());
    Ok(())
}

#[test]
fn mac_application_locations_are_labelled_read_only_candidates() -> io::Result<()> {
    let t = Temp::new()?;
    let home = t.0.join("Redirected home 世界");
    let rows = catalog::candidates(
        catalog::Platform::Mac,
        catalog::Inputs {
            home: Some(home.clone()),
            ..Default::default()
        },
    );
    let expected = [
        (
            "Applications (installed)",
            PathBuf::from("/Applications"),
            "Installed",
        ),
        (
            "User applications (installed)",
            home.join("Applications"),
            "Installed",
        ),
        (
            "Shared application data",
            PathBuf::from("/Library/Application Support"),
            "valuable",
        ),
        (
            "User application data",
            home.join("Library/Application Support"),
            "valuable",
        ),
        (
            "App containers (user data)",
            home.join("Library/Containers"),
            "valuable",
        ),
        (
            "Group containers (user data)",
            home.join("Library/Group Containers"),
            "valuable",
        ),
        (
            "Device backups (valuable)",
            home.join("Library/Application Support/MobileSync/Backup"),
            "backups",
        ),
    ];
    assert_eq!(rows.len(), 32);
    assert_eq!(rows[24].name, "uv cache (default)");
    for (row, (name, path, warning)) in rows[25..].iter().zip(expected) {
        assert_eq!(row.name, name);
        // Fixed macOS system paths are intentionally unresolved on Windows.
        let path = path.is_absolute().then_some(path);
        assert_eq!(row.path, path);
        assert!(row.purpose.contains(warning));
        assert!(row.identity.is_none() && row.measurement.is_none());
    }
    assert!(
        !home.exists(),
        "listing candidates must not create or scan app data"
    );
    assert!(!rows
        .iter()
        .filter_map(|r| r.path.as_ref())
        .any(|p| p.ends_with("System/Applications")));
    let backup = rows[31].path.as_ref().unwrap();
    fs::create_dir_all(backup)?;
    fs::write(backup.join("private-backup"), b"123456789")?;
    let resolved = catalog::resolve(vec![rows[31].clone()], &Cancellation::default());
    assert_eq!(resolved[0].status, Status::NotAnalyzed);
    let result = scan(
        Snapshot {
            rows: resolved,
            ..Default::default()
        },
        Some(0),
        Cancellation::default(),
        Arc::default(),
    );
    assert_eq!(result.rows[0].measurement.as_ref().unwrap().bytes, 9);
    assert_eq!(fs::read(backup.join("private-backup"))?, b"123456789");
    let missing = catalog::candidates(catalog::Platform::Mac, Default::default());
    for row in &missing[26..] {
        if row.name != "Shared application data" {
            assert!(row.path.is_none());
        }
    }
    Ok(())
}
