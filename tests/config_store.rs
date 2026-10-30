use dn::{
    app::App,
    columns::ColumnPreset,
    config::{Config, Store},
    panel::SortMode,
    panel_layout::PanelLayout,
    settings::{self, SettingsKind},
};
use std::{
    fs, io,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct Temp(PathBuf);
impl Temp {
    fn new() -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "dn-config-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
    fn path(&self) -> PathBuf {
        self.0.join("preferences/config.conf")
    }
    fn app(&self) -> io::Result<App> {
        let mut app = App::new(self.0.clone())?;
        app.config_store = Store::new(Some(self.path()));
        app.load_config();
        Ok(app)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(windows)]
#[test]
fn windows_settings_use_the_new_directory_without_importing_old_preferences() -> io::Result<()> {
    const CHILD_ROOT: &str = "DN_CONFIG_LOCATION_TEST_ROOT";
    if let Some(root) = std::env::var_os(CHILD_ROOT) {
        let root = PathBuf::from(root);
        let expected = root.join("Directory Navigator").join("config.conf");
        let mut app = App::new(root.clone())?;
        assert_eq!(app.config_store.path()?, expected);
        assert_eq!(
            dn::associations::path(&app)?,
            expected.with_file_name("associations.conf")
        );
        app.load_initial_config();
        assert!(app.dialogs().is_empty());
        assert!(app.welcome.modals.welcome().is_some());
        assert_eq!(app.config, Config::default());
        assert!(app.save_settings_on_exit());
        assert_eq!(Config::read(&expected)?, Config::default());
        app.config.show_clock = false;
        app.load_config();
        assert!(app.config.show_clock);
        return Ok(());
    }

    // Use a child process so APPDATA changes cannot affect parallel tests.
    let temp = Temp::new()?;
    let root = temp.0.join("App Data 世界");
    let old_path = root.join("DOSNavigator").join("config.conf");
    let old_config = Config {
        show_clock: false,
        ..Config::default()
    };
    old_config.save(&old_path)?;
    let old_bytes = fs::read(&old_path)?;
    let output = std::process::Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "windows_settings_use_the_new_directory_without_importing_old_preferences",
            "--nocapture",
        ])
        .env(CHILD_ROOT, &root)
        .env("APPDATA", &root)
        .output()?;
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read(old_path)?, old_bytes);
    assert!(root.join("Directory Navigator/config.conf").is_file());
    Ok(())
}

#[test]
fn first_exit_creates_configuration_then_unchanged_exit_does_not_write() -> io::Result<()> {
    let t = Temp::new()?;
    let mut store = Store::new(Some(t.path()));
    let defaults = store.load()?;
    assert!(defaults.save_on_exit);
    assert!(!t.path().exists());
    assert!(store.save(&defaults, false)?);
    assert!(!store.save(&defaults, false)?);
    assert_eq!(Config::read(&t.path())?, defaults);
    let text = fs::read_to_string(t.path())?;
    assert!(text.starts_with("# Directory Navigator preferences\n"));
    assert!(text.contains("config_version = 2"));
    assert!(text.contains("[application]"));
    assert!(text.contains("save_on_exit = true"));
    assert_eq!(fs::read_dir(t.path().parent().unwrap())?.count(), 1);
    Ok(())
}

#[test]
fn complete_preferences_roundtrip_and_unknown_keys_survive() -> io::Result<()> {
    let t = Temp::new()?;
    fs::create_dir_all(t.path().parent().unwrap())?;
    fs::write(
        t.path(),
        "config_version=2\nfuture_setting=\"keep=exact\"\n[file_manager]\nhidden=true\n",
    )?;
    let mut store = Store::new(Some(t.path()));
    let mut config = store.load()?;
    assert_eq!(config.layouts, [PanelLayout::Brief; 2]);
    config.language = dn::localization::Language::Russian;
    config.datetime = dn::regional::DateTimeFormat {
        date_order: dn::regional::DateOrder::Dmy,
        date_separator: '.',
        twelve_hour: true,
        time_separator: '.',
    };
    config.save_on_exit = false;
    config.show_hidden = false;
    config.highlight_files = false;
    config.show_tags = false;
    config.space_select = false;
    config.delete_key = false;
    config.use_trash = false;
    config.backspace_parent = false;
    config.show_clock = false;
    config.saver_minutes = 5;
    config.palette = dn::palette::PaletteProfile::ClassicDn
        .with_slot(dn::palette::theme::Slot::PanelText, 0x1e)
        .with_slot(dn::palette::theme::Slot::TimerColor, 0x72);
    config.layouts = [PanelLayout::Brief, PanelLayout::Details];
    config.columns = [ColumnPreset::NameSize, ColumnPreset::NameSizeModified];
    config.sorts = [SortMode::Extension, SortMode::Date];
    config.editor.auto_indent = false;
    config.editor.syntax = false;
    config.editor.expand_tabs = false;
    config.editor.tab_size = 3;
    config.bookmarks[0] = Some(t.0.join("Unicode ☺ = name"));
    store.save(&config, true)?;
    assert_eq!(store.load()?, config);
    let saved: toml::Table = fs::read_to_string(t.path())?.parse().unwrap();
    assert_eq!(saved["future_setting"].as_str(), Some("keep=exact"));
    config.bookmarks[0] = None;
    store.save(&config, true)?;
    assert!(Config::read(&t.path())?.bookmarks[0].is_none());
    Ok(())
}

#[test]
fn competing_instances_and_external_edits_are_not_overwritten() -> io::Result<()> {
    let t = Temp::new()?;
    let mut a = Store::new(Some(t.path()));
    let mut b = Store::new(Some(t.path()));
    let mut ca = a.load()?;
    let mut cb = b.load()?;
    ca.show_clock = false;
    a.save(&ca, true)?;
    let original = fs::read(t.path())?;
    cb.use_trash = false;
    assert!(b
        .save(&cb, true)
        .unwrap_err()
        .to_string()
        .contains("changed outside"));
    assert_eq!(fs::read(t.path())?, original);
    cb = b.load()?;
    cb.use_trash = false;
    b.save(&cb, true)?;
    assert!(!Config::read(&t.path())?.use_trash);
    assert!(a.save(&ca, false).is_ok()); // unchanged session need not write
    fs::remove_file(t.path())?;
    ca.show_tags = false;
    assert!(a.save(&ca, true).is_err());
    assert!(!t.path().exists());
    Ok(())
}

#[test]
fn invalid_configuration_recovers_but_unavailable_settings_fail() -> io::Result<()> {
    let t = Temp::new()?;
    fs::create_dir_all(t.path().parent().unwrap())?;
    for invalid in [
        b"not a record".as_slice(),
        b"config_version=3\n",
        b"\xff\xfe",
    ] {
        fs::write(t.path(), invalid)?;
        let mut store = Store::new(Some(t.path()));
        assert_eq!(store.load()?, Config::default());
        assert!(!store.is_new_installation());
        assert!(!t.path().exists());
        store.save(&Config::default(), true)?;
        assert_eq!(Config::read(&t.path())?, Config::default());
    }
    fs::write(t.path(), vec![b' '; 1024 * 1024 + 1])?;
    assert!(Config::read(&t.path()).is_err());
    assert!(Store::new(None).load().is_err());
    Ok(())
}

#[test]
fn save_failures_preserve_file_and_remove_owned_staging_and_lock() -> io::Result<()> {
    let t = Temp::new()?;
    let mut store = Store::new(Some(t.path()));
    let mut config = store.load()?;
    store.save(&config, true)?;
    let original = fs::read(t.path())?;
    let lock = t.path().with_file_name("config.conf.lock");
    fs::write(&lock, "another save")?;
    config.show_clock = false;
    assert!(store
        .save(&config, true)
        .unwrap_err()
        .to_string()
        .contains("locked"));
    assert_eq!(fs::read(&lock)?, b"another save");
    fs::remove_file(&lock)?;
    let mut permissions = fs::metadata(t.path())?.permissions();
    let original_permissions = permissions.clone();
    permissions.set_readonly(true);
    fs::set_permissions(t.path(), permissions)?;
    assert!(store.save(&config, true).is_err());
    assert!(!lock.exists());
    assert_eq!(fs::read(t.path())?, original);
    fs::set_permissions(t.path(), original_permissions)?;
    store.save(&config, true)?;
    assert_eq!(fs::read_dir(t.path().parent().unwrap())?.count(), 1);
    Ok(())
}

#[test]
fn orderly_exit_persists_live_sorts_and_preferences_and_manual_reload() -> io::Result<()> {
    let t = Temp::new()?;
    let mut app = t.app()?;
    app.panels[1].set_sort(SortMode::Date);
    app.config.editor.tab_size = 4;
    app.config.layouts[0] = PanelLayout::Details;
    assert!(app.save_settings_on_exit());
    let mut reopened = t.app()?;
    assert_eq!(reopened.panels[1].sort, SortMode::Date);
    assert_eq!(reopened.config.editor.tab_size, 4);
    assert_eq!(reopened.config.layouts[0], PanelLayout::Details);
    reopened.config.editor.tab_size = 7;
    reopened.load_config();
    assert_eq!(reopened.config.editor.tab_size, 4);
    reopened.config.editor.tab_size = 6;
    reopened.save_config();
    assert_eq!(Config::read(&t.path())?.editor.tab_size, 6);
    Ok(())
}

#[test]
fn settings_apply_cancel_and_autosave_opt_out() -> io::Result<()> {
    let t = Temp::new()?;
    let mut app = t.app()?;
    app.open_settings(SettingsKind::System);
    app.modals.settings_mut().unwrap().draft.save_on_exit = false;
    settings::key(&mut app, crossterm::event::KeyCode::Esc);
    assert!(app.config.save_on_exit);
    app.open_settings(SettingsKind::System);
    settings::key(&mut app, crossterm::event::KeyCode::Char('x'));
    settings::accept(&mut app);
    assert!(!app.config.save_on_exit);
    assert!(app.save_settings_on_exit());
    assert!(!t.path().exists());
    app.save_config();
    assert!(!Config::read(&t.path())?.save_on_exit);
    Ok(())
}

#[test]
fn exit_save_error_retains_session_and_cancels_quit() -> io::Result<()> {
    let t = Temp::new()?;
    let mut app = t.app()?;
    app.save_config();
    fs::write(t.path(), "config_version=2\n[file_manager]\nhidden=false\n")?;
    app.config.show_clock = false;
    app.quit = true;
    assert!(!app.save_settings_on_exit());
    assert!(!app.quit);
    assert!(!app.config.show_clock);
    assert!(app
        .dialogs()
        .last()
        .unwrap()
        .message
        .contains("Exit cancelled"));
    assert_eq!(
        fs::read_to_string(t.path())?,
        "config_version=2\n[file_manager]\nhidden=false\n"
    );
    app.load_config();
    app.quit = true;
    assert!(app.save_settings_on_exit());
    Ok(())
}

#[test]
fn cancelling_palette_preview_does_not_persist_preview() -> io::Result<()> {
    let t = Temp::new()?;
    let mut app = t.app()?;
    app.save_config();
    let original = fs::read(t.path())?;
    dn::menu::execute(&mut app, dn::menu::Action::Colors);
    dn::palette::action(&mut app, dn::palette::Action::Foreground(12));
    dn::palette::cancel(&mut app);
    assert!(app.save_settings_on_exit());
    assert_eq!(fs::read(t.path())?, original);
    Ok(())
}

#[cfg(unix)]
#[test]
fn native_bookmarks_and_symlink_configuration_safety() -> io::Result<()> {
    use std::{
        ffi::OsString,
        os::unix::{ffi::OsStringExt, fs::symlink},
    };
    let t = Temp::new()?;
    let mut store = Store::new(Some(t.path()));
    let mut config = store.load()?;
    config.bookmarks[4] = Some(t.0.join(OsString::from_vec(vec![b'a', 0xff])));
    store.save(&config, true)?;
    assert_eq!(store.load()?.bookmarks, config.bookmarks);
    let link = t.0.join("link");
    symlink(t.path(), &link)?;
    assert!(Config::read(&link).is_err());
    assert!(config.save(&link).is_err());
    assert!(fs::symlink_metadata(&link)?.is_symlink());
    Ok(())
}

#[test]
fn txt_is_ignored_and_conf_is_created_with_defaults() -> io::Result<()> {
    let t = Temp::new()?;
    fs::create_dir_all(t.path().parent().unwrap())?;
    let legacy = t.path().with_file_name("config.txt");
    let original = "hidden=false\nsaver_minutes=1\nfuture_key=preserved\n";
    fs::write(&legacy, original)?;
    let mut store = Store::new(Some(t.path()));
    let config = store.load()?;
    assert!(config.show_hidden);
    assert_eq!(config.saver_minutes, dn::screensaver::DEFAULT_DELAY);
    assert!(!t.path().exists());
    assert!(store.save(&config, false)?);
    assert_eq!(Config::read(&t.path())?, config);
    assert!(!fs::read_to_string(t.path())?.contains("future_key=preserved"));
    assert_eq!(fs::read_to_string(legacy)?, original);
    assert!(!store.save(&config, false)?);
    Ok(())
}

#[test]
fn conf_takes_precedence_and_invalid_conf_does_not_fall_back() -> io::Result<()> {
    let t = Temp::new()?;
    fs::create_dir_all(t.path().parent().unwrap())?;
    fs::write(
        t.path().with_file_name("config.txt"),
        "config_version=2\n[file_manager]\nhidden=false\n",
    )?;
    fs::write(t.path(), "config_version=2\n[file_manager]\nhidden=true\n")?;
    let mut store = Store::new(Some(t.path()));
    assert!(store.load()?.show_hidden);
    fs::write(t.path(), "config_version=3\n")?;
    assert_eq!(store.load()?, Config::default());
    assert_eq!(
        fs::read_to_string(t.path().with_file_name("_old_config.conf"))?,
        "config_version=3\n"
    );
    store.save(&Config::default(), true)?;
    Ok(())
}

#[test]
fn txt_changes_are_ignored_but_new_conf_still_blocks_save() -> io::Result<()> {
    let t = Temp::new()?;
    fs::create_dir_all(t.path().parent().unwrap())?;
    let legacy = t.path().with_file_name("config.txt");
    fs::write(&legacy, "config_version=2\n[file_manager]\nhidden=false\n")?;
    let mut store = Store::new(Some(t.path()));
    let config = store.load()?;
    fs::write(&legacy, "config_version=2\n[file_manager]\nhidden=true\n")?;
    assert!(store.save(&config, false)?);
    assert_eq!(Config::read(&t.path())?, Config::default());
    store.load()?;
    fs::write(t.path(), "show_clock=false\n")?;
    let mut changed = config.clone();
    changed.show_hidden = false;
    assert!(store.save(&changed, false).is_err());
    assert_eq!(fs::read_to_string(t.path())?, "show_clock=false\n");
    Ok(())
}
