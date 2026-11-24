use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use dn::{
    app::App,
    input,
    navigation_tools::{self, Action, Modal},
    panel::filter::{self, FilenameFilter, TypesFilter},
    testing::{backend::TestBackend, Terminal},
    ui,
};
use std::{
    ffi::OsString,
    fs, io,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
struct Temp(PathBuf);
impl Temp {
    fn new() -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "dn-extension-filter-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
    fn app(&self) -> io::Result<App> {
        for name in [
            "a.jpg",
            "b.JPG",
            "c.png",
            "README",
            ".hidden",
            "ёжик.юникод",
        ] {
            fs::write(self.0.join(name), b"test")?;
        }
        fs::create_dir(self.0.join("folder.jpg"))?;
        let mut app = App::new(self.0.clone())?;
        settle(&mut app);
        app.config.show_clock = false;
        app.config.show_hidden = false;
        for panel in &mut app.panels {
            panel.refresh(false);
        }
        settle(&mut app);
        Ok(app)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn settle(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.panels.iter().any(|p| p.loading) {
        app.poll();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn types(hide: bool, names: &[Option<&str>]) -> TypesFilter {
    TypesFilter {
        hide,
        selected: names.iter().map(|n| n.map(OsString::from)).collect(),
    }
}
fn names(app: &App, panel: usize) -> Vec<OsString> {
    app.panels[panel]
        .entries
        .iter()
        .map(|e| e.name.clone())
        .collect()
}
fn key(app: &mut App, code: KeyCode) {
    input::handle(app, KeyEvent::new(code, KeyModifiers::NONE));
}
fn draw(app: &mut App, width: u16, height: u16) -> io::Result<Terminal<TestBackend>> {
    let mut term = Terminal::new(TestBackend::new(width, height))?;
    term.draw(|frame| ui::draw(frame, app))?;
    assert!(app
        .mouse
        .regions
        .iter()
        .all(|(r, _)| r.right() <= width && r.bottom() <= height));
    Ok(term)
}
#[test]
fn cached_inventory_show_hide_keeps_directories_marks_selection_and_other_pane() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    let opposite = names(&app, 1);
    app.panels[0].selected.insert("c.png".into());
    app.panels[0].cursor = app.panels[0]
        .entries
        .iter()
        .position(|e| e.name == "a.jpg")
        .unwrap();
    app.panels[0].set_filter("*.jpg".into());
    let (rows, limited) = app.panels[0].file_types();
    assert!(!limited);
    assert!(rows
        .iter()
        .any(|r| r.extension.as_deref() == Some(std::ffi::OsStr::new("png"))));
    let no_ext = rows.iter().find(|r| r.extension.is_none()).unwrap();
    assert_eq!(no_ext.count, 1); // .hidden was not loaded.
    app.panels[0].set_filename_filter(FilenameFilter::Types(types(
        false,
        &[Some("jpg"), Some("png")],
    )))?;
    assert!(names(&app, 0).contains(&"folder.jpg".into()));
    assert!(names(&app, 0).contains(&"c.png".into()));
    assert!(!names(&app, 0).contains(&"b.JPG".into()));
    assert_eq!(app.panels[0].entries[app.panels[0].cursor].name, "a.jpg");
    assert!(app.panels[0].selected.contains(&OsString::from("c.png")));
    app.panels[0].set_filename_filter(FilenameFilter::Types(types(true, &[Some("jpg")])))?;
    assert!(!names(&app, 0).contains(&"a.jpg".into()));
    assert!(names(&app, 0).contains(&"folder.jpg".into()));
    assert_eq!(names(&app, 1), opposite);
    app.panels[0].cursor = app.panels[0].entries.len() - 1;
    app.panels[0].set_filename_filter(FilenameFilter::Types(types(false, &[Some("png")])))?;
    assert_eq!(app.panels[0].cursor, app.panels[0].entries.len() - 1);
    app.panels[0].set_filename_filter(FilenameFilter::Types(TypesFilter::default()))?;
    assert!(app.panels[0].types_filter.is_none());
    assert_eq!(names(&app, 0), opposite);
    Ok(())
}
#[test]
fn extensions_native_roundtrip_empty_dotfiles_and_limits() -> io::Result<()> {
    assert_eq!(filter::extension(std::ffi::OsStr::new(".profile")), None);
    assert_eq!(
        filter::extension(std::ffi::OsStr::new("x.tar.gz")),
        Some("gz".into())
    );
    assert_eq!(
        filter::extension(std::ffi::OsStr::new("x.")),
        Some("".into())
    );
    let value = types(
        true,
        &[None, Some(""), Some("юникод"), Some("JPG"), Some("x;?*")],
    );
    assert_eq!(TypesFilter::decode(value.hide, &value.encoded())?, value);
    assert!(TypesFilter::decode(false, &["bad".into()]).is_err());
    assert!(TypesFilter::decode(false, &[String::new(), String::new()]).is_err());
    assert!(types(false, &[Some(&"x".repeat(4096))]).validate().is_err());
    let too_many = TypesFilter {
        hide: false,
        selected: (0..257).map(|n| Some(format!("{n}").into())).collect(),
    };
    assert!(too_many.validate().is_err());
    Ok(())
}
#[test]
fn picker_keyboard_mouse_cancel_clear_stale_location_and_immutable_frames() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    navigation_tools::open_filter(&mut app);
    navigation_tools::action(&mut app, Action::FilterMode);
    let row = match app.navigation_dialog() {
        Some(Modal::Filter { draft, .. }) => draft
            .rows
            .iter()
            .position(|r| r.extension.as_deref() == Some(std::ffi::OsStr::new("png")))
            .unwrap(),
        _ => unreachable!(),
    };
    for language in [
        dn::localization::Language::English,
        dn::localization::Language::Russian,
    ] {
        app.config.language = language;
        app.config.palette = dn::palette::PaletteProfile::ClassicDn
            .with_slot(dn::palette::theme::Slot::DialogText, 0x2f);
        for size in [(80, 25), (110, 35), (45, 16), (80, 25)] {
            let term = draw(&mut app, size.0, size.1)?;
            let buffer = term.backend().buffer().clone();
            let regions = app.mouse.regions.clone();
            for _ in 0..3 {
                let term = draw(&mut app, size.0, size.1)?;
                assert_eq!(*term.backend().buffer(), buffer);
                assert_eq!(app.mouse.regions, regions);
            }
        }
    }
    app.config.language = dn::localization::Language::English;
    draw(&mut app, 80, 25)?;
    let target = dn::mouse::Target::Navigation(Action::FilterType(row));
    let rect = app
        .mouse
        .regions
        .iter()
        .find(|(_, t)| *t == target)
        .unwrap()
        .0;
    navigation_tools::mouse(
        &mut app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: rect.x,
            row: rect.y,
            modifiers: KeyModifiers::NONE,
        },
        Some(target),
    );
    key(&mut app, KeyCode::Char(' ')); // toggles the same row off
    key(&mut app, KeyCode::Char(' ')); // on again
    key(&mut app, KeyCode::Esc);
    assert!(app.panels[0].types_filter.is_none());
    navigation_tools::open_filter(&mut app);
    navigation_tools::action(&mut app, Action::FilterMode);
    navigation_tools::action(&mut app, Action::FilterType(row));
    app.active = 1; // applying must still target the captured pane.
    navigation_tools::action(&mut app, Action::ApplyFilter);
    assert_eq!(
        app.panels[0].types_filter,
        Some(types(false, &[Some("png")]))
    );
    assert!(app.panels[1].types_filter.is_none());
    app.active = 0;
    navigation_tools::open_filter(&mut app);
    navigation_tools::action(&mut app, Action::ClearFilter);
    assert!(app.panels[0].types_filter.is_some()); // draft only
    navigation_tools::action(&mut app, Action::ApplyFilter);
    assert!(app.panels[0].types_filter.is_none());
    navigation_tools::open_filter(&mut app);
    app.panels[0].revision = app.panels[0].revision.wrapping_add(1);
    navigation_tools::action(&mut app, Action::ApplyFilter);
    assert!(app.navigation_dialog().is_some());
    assert!(!app.dialogs().is_empty());
    Ok(())
}
#[test]
fn refresh_hidden_settings_bounded_inventory_and_literal_wildcards() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    app.panels[0].set_filename_filter(FilenameFilter::Types(types(false, &[Some("png")])))?;
    fs::write(temp.0.join("new.png"), b"new")?;
    app.panels[0].refresh(true);
    settle(&mut app);
    assert!(names(&app, 0).contains(&"new.png".into()));
    assert!(app.panels[0]
        .file_types()
        .0
        .iter()
        .any(|r| r.extension.is_none() && r.count == 2));
    for n in 0..300 {
        fs::write(temp.0.join(format!("item.{n:03}")), b"x")?;
    }
    app.panels[0].refresh(false);
    settle(&mut app);
    let (rows, limited) = app.panels[0].file_types();
    assert!(limited);
    assert_eq!(rows.len(), 256);
    assert!(rows.iter().any(|r| r.extension == Some("png".into())));
    app.panels[0].set_filter("*.jpg;*.png".into());
    assert!(app.panels[0].entries.iter().all(|e| e.parent));
    Ok(())
}
#[cfg(unix)]
#[test]
fn native_nonunicode_extensions_and_links_are_preserved_without_following() -> io::Result<()> {
    use std::os::unix::{ffi::OsStringExt, fs::symlink};
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    let name = OsString::from_vec(b"native.\xff".to_vec());
    symlink("folder.jpg", temp.0.join("link.png"))?;
    app.panels[0].refresh(false);
    settle(&mut app);
    // APFS cannot create arbitrary invalid UTF-8 names; test the cached native
    // entry directly instead of relying on filesystem-specific name acceptance.
    app.panels[0].entries.push(dn::filesystem::Entry {
        path: temp.0.join(&name),
        name: name.clone(),
        file_type: None,
        directory: false,
        symlink: false,
        size: Some(1),
        parent: false,
        modified: None,
        executable: false,
    });
    let value = TypesFilter {
        hide: false,
        selected: std::collections::BTreeSet::from([Some(OsString::from_vec(vec![255]))]),
    };
    assert_eq!(TypesFilter::decode(false, &value.encoded())?, value);
    app.panels[0].set_filename_filter(FilenameFilter::Types(value))?;
    assert!(names(&app, 0).contains(&name));
    app.panels[0].set_filename_filter(FilenameFilter::Types(types(false, &[Some("png")])))?;
    assert!(names(&app, 0).contains(&"link.png".into()));
    Ok(())
}
#[test]
fn archive_filter_and_outer_native_filter_are_independent() -> io::Result<()> {
    use std::io::Write;
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    let path = temp.0.join("items.zip");
    let mut writer = zip::ZipWriter::new(fs::File::create(&path)?);
    writer.start_file("one.txt", zip::write::SimpleFileOptions::default())?;
    writer.write_all(b"text")?;
    writer.start_file("two.png", zip::write::SimpleFileOptions::default())?;
    writer.write_all(b"image")?;
    writer.finish()?;
    let outer = types(false, &[Some("jpg")]);
    app.panels[0].set_filename_filter(FilenameFilter::Types(outer.clone()))?;
    app.panels[0].open_archive(path)?;
    settle(&mut app);
    assert!(app.panels[0].types_filter.is_none());
    app.panels[0].set_filename_filter(FilenameFilter::Types(types(false, &[Some("png")])))?;
    assert!(names(&app, 0).contains(&"two.png".into()));
    assert!(!names(&app, 0).contains(&"one.txt".into()));
    app.panels[0].leave_archive(false);
    settle(&mut app);
    assert_eq!(app.panels[0].types_filter, Some(outer));
    assert!(names(&app, 0).contains(&"a.jpg".into()));
    assert!(!names(&app, 0).contains(&"c.png".into()));
    Ok(())
}
#[test]
fn oversized_selection_is_rejected_without_changing_the_live_filter() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = temp.app()?;
    app.panels[0].set_filter("*.png".into());
    let original = names(&app, 0);
    navigation_tools::open_filter(&mut app);
    if let Some(Modal::Filter { draft, .. }) = app.navigation_dialog_mut() {
        draft.types_mode = true;
        draft.types = types(false, &[Some(&"x".repeat(4096))]);
    }
    navigation_tools::action(&mut app, Action::ApplyFilter);
    assert_eq!(app.panels[0].filter, "*.png");
    assert_eq!(names(&app, 0), original);
    assert!(app.navigation_dialog().is_some());
    assert!(!app.dialogs().is_empty());
    Ok(())
}

#[test]
fn empty_listing_shows_no_extension_and_empty_selection_clears() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = App::new(temp.0.clone())?;
    app.config.show_clock = false;
    settle(&mut app);
    let (rows, limited) = app.panels[0].file_types();
    assert!(!limited);
    assert_eq!(rows.len(), 1);
    assert!(rows[0].extension.is_none());
    assert_eq!(rows[0].count, 0);
    navigation_tools::open_filter(&mut app);
    navigation_tools::action(&mut app, Action::FilterMode);
    draw(&mut app, 80, 25)?;
    key(&mut app, KeyCode::Enter);
    assert!(app.navigation_dialog().is_none());
    assert!(app.panels[0].types_filter.is_none());
    Ok(())
}

#[cfg(windows)]
#[test]
fn windows_native_surrogate_extension_roundtrips_without_loss() -> io::Result<()> {
    use std::os::windows::ffi::OsStringExt;
    let value = TypesFilter {
        hide: true,
        selected: [Some(OsString::from_wide(&[0xd800]))].into(),
    };
    assert_eq!(TypesFilter::decode(true, &value.encoded())?, value);
    Ok(())
}
