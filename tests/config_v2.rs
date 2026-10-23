use dn::{
    config::{Config, Store},
    panel_layout::PanelLayout,
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
            "dn-toml-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
    fn config(&self) -> PathBuf {
        self.0.join("config.conf")
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn grouped_defaults_types_and_partial_configuration() -> io::Result<()> {
    let t = Temp::new()?;
    Config::default().save(&t.config())?;
    let table: toml::Table = fs::read_to_string(t.config())?.parse().unwrap();
    assert_eq!(table["config_version"].as_integer(), Some(2));
    for section in [
        "application",
        "interface",
        "file_manager",
        "panels",
        "editor",
        "regional",
        "screensaver",
        "bookmarks",
    ] {
        assert!(table[section].is_table(), "{section}");
    }
    assert_eq!(table["editor"]["tab_size"].as_integer(), Some(8));
    assert_eq!(table["interface"]["show_clock"].as_bool(), Some(true));
    assert_eq!(Config::parse(b"config_version = 2\n")?, Config::default());
    let partial = Config::parse(b"config_version = 2\n[editor]\nauto_indent = false\n")?;
    assert!(!partial.editor.auto_indent);
    assert_eq!(partial.layouts, [PanelLayout::Brief; 2]);
    Ok(())
}
#[test]
fn invalid_known_types_values_sections_and_versions_are_preserved_and_reset() -> io::Result<()> {
    for content in [
        "config_version=3",
        "config_version='2'",
        "[editor]\nauto_indent=true",
        "config_version=2\n[interface]\nshow_clock='false'",
        "config_version=2\neditor=4",
        "config_version=2\n[editor]\ntab_size=0",
        "config_version=2\n[editor]\ntab_size=17",
        "config_version=2\n[panels]\nleft=false",
        "config_version=2\n[panels.left]\nsort=42",
        "config_version=2\n[panels.left]\nsort='unknown'",
        "config_version=2\n[regional]\ntime_format='24'",
        "config_version=2\n[regional]\ndate_separator='bad'",
        "config_version=2\n[screensaver]\nminutes=3",
        "config_version=2\n[bookmarks]\n1=5",
        "config_version=2\n[interface]\nshow_clock=",
    ] {
        let t = Temp::new()?;
        fs::write(t.config(), content)?;
        assert!(Config::parse(content.as_bytes()).is_err());
        let mut store = Store::new(Some(t.config()));
        assert_eq!(store.load()?, Config::default(), "{content}");
        assert!(!store.is_new_installation());
        assert_eq!(fs::read_to_string(t.0.join("_old_config.conf"))?, content);
        assert!(!t.config().exists());
        store.save(&Config::default(), true)?;
        assert_eq!(Config::read(&t.config())?, Config::default());
        assert_eq!(fs::read_dir(&t.0)?.count(), 2);
    }
    Ok(())
}
#[test]
fn unknown_toml_values_survive_multiple_saves_and_cleared_bookmarks() -> io::Result<()> {
    let t = Temp::new()?;
    let input = "config_version=2\nfuture=[1, 2]\n[editor]\nauto_indent=false\nunknown={enabled=true, word='子'}\n[future_group]\narray=['a', 'b']\n[bookmarks]\nfuture='literal'\n";
    fs::write(t.config(), input)?;
    let mut store = Store::new(Some(t.config()));
    let mut config = store.load()?;
    config.bookmarks[0] = Some(t.0.join("子=hello"));
    store.save(&config, true)?;
    assert_eq!(store.load()?, config);
    config.bookmarks[0] = None;
    config.show_clock = false;
    store.save(&config, true)?;
    let actual: toml::Table = fs::read_to_string(t.config())?.parse().unwrap();
    let original: toml::Table = input.parse().unwrap();
    for section in ["future", "future_group"] {
        assert_eq!(actual[section], original[section]);
    }
    assert_eq!(actual["editor"]["unknown"], original["editor"]["unknown"]);
    assert_eq!(actual["bookmarks"]["future"].as_str(), Some("literal"));
    assert!(actual["bookmarks"].get("1").is_none());
    assert_eq!(store.load()?, config);
    Ok(())
}
#[test]
fn old_flat_empty_and_unversioned_files_are_preserved_without_conversion() -> io::Result<()> {
    for bytes in [
        b"hidden=false\n".as_slice(),
        b"config_version=1\nhidden=true\n",
        b"",
        b"[application]\nsave_on_exit=true\n",
    ] {
        let t = Temp::new()?;
        fs::write(t.config(), bytes)?;
        let mut store = Store::new(Some(t.config()));
        assert_eq!(store.load()?, Config::default());
        assert!(!store.is_new_installation());
        assert_eq!(fs::read(t.0.join("_old_config.conf"))?, bytes);
        assert!(!t.config().exists());
        assert_eq!(fs::read_dir(&t.0)?.count(), 1);
    }
    Ok(())
}
#[test]
fn saving_toml_does_not_create_or_change_old_backup_files() -> io::Result<()> {
    let t = Temp::new()?;
    let backup = t.0.join("config.conf.v1.bak");
    fs::write(&backup, b"untouched")?;
    let mut store = Store::new(Some(t.config()));
    let mut config = store.load()?;
    store.save(&config, true)?;
    config.show_clock = false;
    store.save(&config, true)?;
    assert_eq!(store.load()?, config);
    assert_eq!(fs::read(backup)?, b"untouched");
    assert_eq!(fs::read_dir(&t.0)?.count(), 2);
    Ok(())
}
#[cfg(unix)]
#[test]
fn native_bookmarks_remain_lossless_in_toml() -> io::Result<()> {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let t = Temp::new()?;
    let mut store = Store::new(Some(t.config()));
    let mut config = store.load()?;
    config.bookmarks[3] = Some(t.0.join(OsString::from_vec(vec![b'x', 255])));
    store.save(&config, true)?;
    assert_eq!(store.load()?, config);
    assert_eq!(fs::read_dir(&t.0)?.count(), 1);
    Ok(())
}
#[test]
fn disabled_autosave_keeps_toml_unchanged_until_explicit_save() -> io::Result<()> {
    let t = Temp::new()?;
    let bytes = b"config_version=2\n[application]\nsave_on_exit=false\n";
    fs::write(t.config(), bytes)?;
    let mut app = dn::app::App::new(t.0.clone())?;
    app.config_store = Store::new(Some(t.config()));
    app.load_config();
    app.config.show_clock = false;
    assert!(app.save_settings_on_exit());
    assert_eq!(fs::read(t.config())?, bytes);
    app.save_config();
    assert!(!Config::read(&t.config())?.show_clock);
    assert_eq!(fs::read_dir(&t.0)?.count(), 1);
    Ok(())
}

#[test]
fn recovery_preserves_collisions_and_allows_subsequent_saves_and_reloads() -> io::Result<()> {
    let t = Temp::new()?;
    fs::write(t.0.join("_old_config.conf"), b"earlier recovery")?;
    fs::create_dir(t.0.join("_old_1_config.conf"))?;
    let invalid = b"config_version=2\n[editor]\ntab_size='broken'";
    fs::write(t.config(), invalid)?;
    let mut store = Store::new(Some(t.config()));
    assert_eq!(store.load()?, Config::default());
    assert_eq!(fs::read(t.0.join("_old_2_config.conf"))?, invalid);
    assert_eq!(fs::read(t.0.join("_old_config.conf"))?, b"earlier recovery");
    assert!(t.0.join("_old_1_config.conf").is_dir());
    assert!(!store.is_new_installation());
    assert_eq!(store.load()?, Config::default());
    assert!(!store.is_new_installation());
    let config = Config {
        show_clock: false,
        ..Config::default()
    };
    assert!(store.save(&config, false)?);
    assert_eq!(store.load()?, config);
    assert!(!store.save(&config, false)?);
    assert_eq!(fs::read(t.0.join("_old_2_config.conf"))?, invalid);
    Ok(())
}

#[test]
fn recovery_respects_locks_and_preserves_oversized_files() -> io::Result<()> {
    let t = Temp::new()?;
    let invalid = vec![b' '; 1024 * 1024 + 1];
    fs::write(t.config(), &invalid)?;
    fs::write(t.0.join("config.conf.lock"), b"another instance")?;
    let mut store = Store::new(Some(t.config()));
    assert!(store.load().is_err());
    assert_eq!(fs::read(t.config())?, invalid);
    assert!(!t.0.join("_old_config.conf").exists());
    fs::remove_file(t.0.join("config.conf.lock"))?;
    assert_eq!(store.load()?, Config::default());
    assert_eq!(fs::read(t.0.join("_old_config.conf"))?, invalid);
    assert!(!t.0.join("config.conf.lock").exists());
    store.save(&Config::default(), true)?;
    assert_eq!(Config::read(&t.config())?, Config::default());
    Ok(())
}

#[cfg(unix)]
#[test]
fn recovery_refuses_readonly_files_and_symlinks() -> io::Result<()> {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let t = Temp::new()?;
    fs::write(t.config(), b"broken")?;
    fs::set_permissions(t.config(), fs::Permissions::from_mode(0o400))?;
    assert!(Store::new(Some(t.config())).load().is_err());
    assert_eq!(fs::read(t.config())?, b"broken");
    fs::set_permissions(t.config(), fs::Permissions::from_mode(0o600))?;
    fs::rename(t.config(), t.0.join("target"))?;
    symlink(t.0.join("target"), t.config())?;
    assert!(Store::new(Some(t.config())).load().is_err());
    assert!(fs::symlink_metadata(t.config())?.is_symlink());
    assert_eq!(fs::read(t.0.join("target"))?, b"broken");
    assert!(!t.0.join("_old_config.conf").exists());
    Ok(())
}
