use crossterm::event::{
    KeyCode as K, KeyEvent, KeyModifiers as M, MouseButton, MouseEvent, MouseEventKind,
};
use dn::testing::{backend::TestBackend, Terminal};
use dn::{
    app::App,
    config::Store,
    focus::{self, Focus},
    input, menu,
    mouse::{self, Target},
    shortcuts::Profile,
    ui,
    welcome::{self, Action},
};
use std::{
    fs, io,
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};
struct Temp(PathBuf);
impl Temp {
    fn new() -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "dn-welcome-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p)?;
        fs::write(p.join("file.txt"), "fixture")?;
        Ok(Self(p))
    }
    fn config(&self) -> PathBuf {
        self.0.join("settings/config.conf")
    }
    fn app(&self) -> io::Result<App> {
        let mut a = App::new(self.0.clone())?;
        a.config_store = Store::new(Some(self.config()));
        let deadline = Instant::now() + Duration::from_secs(5);
        while a.panels.iter().any(|p| p.loading) {
            a.poll();
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(1));
        }
        Ok(a)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn key(a: &mut App, code: K) {
    input::handle(a, KeyEvent::new(code, M::NONE));
}
fn draw(a: &mut App, w: u16, h: u16) -> io::Result<String> {
    let mut terminal = Terminal::new(TestBackend::new(w, h))?;
    terminal.draw(|f| ui::draw(f, a))?;
    assert!(a
        .mouse
        .regions
        .iter()
        .all(|(r, _)| r.right() <= w && r.bottom() <= h));
    Ok(terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect())
}
fn click(a: &mut App, target: Target) {
    let r = a
        .mouse
        .regions
        .iter()
        .rev()
        .find(|(_, t)| *t == target)
        .unwrap()
        .0;
    mouse::handle(
        a,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: r.x,
            row: r.y,
            modifiers: M::NONE,
        },
    );
}
#[test]
fn only_initial_missing_settings_show_welcome_and_normal_exit_establishes_installation(
) -> io::Result<()> {
    let t = Temp::new()?;
    let mut a = t.app()?;
    assert!(!a.config_store.is_new_installation());
    a.load_initial_config();
    assert!(a.welcome.modals.welcome().is_some());
    assert_eq!(a.config.shortcuts, Profile::ClassicDn);
    draw(&mut a, 80, 25)?;
    key(&mut a, K::Enter);
    assert!(a.welcome.modals.welcome().is_none());
    assert!(!t.config().exists());
    a.load_initial_config();
    assert!(a.welcome.modals.welcome().is_none());
    a.load_config();
    assert!(a.welcome.modals.welcome().is_none());
    assert!(a.save_settings_on_exit());
    let mut b = t.app()?;
    b.load_initial_config();
    assert!(b.welcome.modals.welcome().is_none());
    Ok(())
}
#[test]
fn disabled_saving_txt_is_ignored_and_empty_conf_skips_welcome() -> io::Result<()> {
    let t = Temp::new()?;
    let mut a = t.app()?;
    a.load_initial_config();
    draw(&mut a, 80, 25)?;
    key(&mut a, K::Esc);
    a.config.save_on_exit = false;
    assert!(a.save_settings_on_exit());
    assert!(!t.config().exists());
    let mut b = t.app()?;
    b.load_initial_config();
    assert!(b.welcome.modals.welcome().is_some());
    fs::create_dir_all(t.config().parent().unwrap())?;
    fs::write(
        t.config().with_file_name("config.txt"),
        "show_clock=false\n",
    )?;
    let mut c = t.app()?;
    c.load_initial_config();
    assert!(c.welcome.modals.welcome().is_some());
    assert!(c.config.show_clock);
    fs::remove_file(t.config().with_file_name("config.txt"))?;
    fs::write(t.config(), "")?;
    let mut d = t.app()?;
    d.load_initial_config();
    assert!(d.welcome.modals.welcome().is_none());
    Ok(())
}
#[test]
fn invalid_settings_recover_and_inaccessible_settings_report_errors_without_welcome(
) -> io::Result<()> {
    let t = Temp::new()?;
    fs::create_dir_all(t.config().parent().unwrap())?;
    fs::write(t.config(), "config_version=999\n")?;
    let mut a = t.app()?;
    a.load_initial_config();
    assert!(a.welcome.modals.welcome().is_none());
    assert!(a.dialogs().is_empty());
    assert_eq!(
        fs::read_to_string(t.config().with_file_name("_old_config.conf"))?,
        "config_version=999\n"
    );
    assert!(!t.config().exists());
    fs::create_dir(t.config())?;
    let mut a = t.app()?;
    a.load_initial_config();
    assert!(a.welcome.modals.welcome().is_none());
    assert!(!a.dialogs().is_empty());
    let mut a = t.app()?;
    a.config_store = Store::new(None);
    a.load_initial_config();
    assert!(a.welcome.modals.welcome().is_none());
    assert!(!a.dialogs().is_empty());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::remove_dir(t.config())?;
        fs::write(
            t.config(),
            "config_version=2\n[file_manager]\nhidden=true\n",
        )?;
        fs::set_permissions(t.config(), fs::Permissions::from_mode(0o0))?;
        if unsafe { libc::geteuid() } != 0 {
            let mut a = t.app()?;
            a.load_initial_config();
            assert!(a.welcome.modals.welcome().is_none());
            assert!(!a.dialogs().is_empty());
        }
        fs::set_permissions(t.config(), fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}
#[test]
#[cfg(unix)]
fn dangling_configuration_links_are_errors_not_new_installations() -> io::Result<()> {
    use std::os::unix::fs::symlink;
    for name in ["config.conf", "config.txt"] {
        let t = Temp::new()?;
        fs::create_dir_all(t.config().parent().unwrap())?;
        symlink(t.0.join("missing"), t.config().with_file_name(name))?;
        let mut a = t.app()?;
        a.load_initial_config();
        assert_eq!(a.welcome.modals.welcome().is_some(), name == "config.txt");
        assert_eq!(a.dialogs().is_empty(), name == "config.txt");
    }
    Ok(())
}
#[test]
fn welcome_blocks_background_input_and_screensaver_and_returns_from_help_settings() -> io::Result<()>
{
    let t = Temp::new()?;
    let mut a = t.app()?;
    a.load_initial_config();
    a.command_line.text = "draft".into();
    a.command_line.cursor = 2;
    draw(&mut a, 80, 25)?;
    let cursor = a.panels[0].cursor;
    let active = a.active;
    input::paste(&mut a, "should not paste");
    key(&mut a, K::F(5));
    key(&mut a, K::Char('x'));
    input::handle(&mut a, KeyEvent::new(K::Char('o'), M::CONTROL));
    assert!(a.modals.prompt().is_none());
    assert!(!dn::shortcuts::armed(&a));
    mouse::handle(
        &mut a,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 2,
            row: 3,
            modifiers: M::NONE,
        },
    );
    assert_eq!(a.panels[0].cursor, cursor);
    assert_eq!(a.active, active);
    assert_eq!(a.command_line.text, "draft");
    assert_eq!(a.command_line.cursor, 2);
    a.config.saver_minutes = 1;
    dn::screensaver::poll(&mut a, Instant::now() + Duration::from_secs(600));
    assert!(!a.screensaver.active);
    key(&mut a, K::Tab);
    key(&mut a, K::Enter);
    assert_eq!(focus::current(&a), Focus::Settings);
    key(&mut a, K::Right);
    key(&mut a, K::Enter);
    assert_eq!(a.config.shortcuts, Profile::Mac);
    assert_eq!(focus::current(&a), Focus::Welcome);
    assert!(draw(&mut a, 80, 25)?.contains("^O5 Copy"));
    key(&mut a, K::F(1));
    assert_eq!(focus::current(&a), Focus::Message);
    input::paste(&mut a, "ignored");
    key(&mut a, K::Esc);
    assert_eq!(focus::current(&a), Focus::Welcome);
    key(&mut a, K::Esc);
    assert_eq!(focus::current(&a), Focus::Manager);
    assert_eq!(a.command_line.text, "draft");
    assert_eq!(a.command_line.cursor, 2);
    Ok(())
}
#[test]
fn localized_profiles_mouse_controls_and_small_terminal_retention() -> io::Result<()> {
    let t = Temp::new()?;
    let mut a = t.app()?;
    for language in [
        dn::localization::Language::English,
        dn::localization::Language::Russian,
    ] {
        a.config.language = language;
        for profile in [Profile::ClassicDn, Profile::Mac] {
            a.config.shortcuts = profile;
            for (w, h) in [(80, 25), (132, 40), (80, 25)] {
                welcome::open(&mut a);
                let text = draw(&mut a, w, h)?;
                assert!(text.contains("Directory Navigator"));
                assert!(text.contains(dn::DISPLAY_VERSION));
                assert!(text.contains(if profile == Profile::Mac { "^O5" } else { "F5" }));
                assert!(a
                    .mouse
                    .regions
                    .iter()
                    .all(|(_, t)| matches!(t, Target::Welcome(_))));
                click(&mut a, Target::Welcome(Action::Keyboard));
                draw(&mut a, w, h)?;
                key(&mut a, K::Esc);
                draw(&mut a, w, h)?;
                click(&mut a, Target::Welcome(Action::Help));
                draw(&mut a, w, h)?;
                key(&mut a, K::Esc);
                draw(&mut a, w, h)?;
                click(&mut a, Target::Welcome(Action::Start));
                assert!(a.welcome.modals.welcome().is_none());
            }
        }
    }
    welcome::open(&mut a);
    draw(&mut a, 40, 12)?;
    key(&mut a, K::Esc);
    key(&mut a, K::Enter);
    assert!(a.welcome.modals.welcome().is_some());
    assert!(a.mouse.regions.is_empty());
    draw(&mut a, 80, 25)?;
    click(&mut a, Target::Welcome(Action::Start));
    assert!(a.welcome.modals.welcome().is_none());
    Ok(())
}
#[test]
fn manual_menu_reopening_close_control_and_setting_changes_preserve_panels() -> io::Result<()> {
    let t = Temp::new()?;
    let mut a = t.app()?;
    let cursor = a.panels[0].cursor;
    let revision = a.panels[0].revision;
    let row = menu::items(0)
        .iter()
        .position(|i| i.action == menu::Action::Welcome)
        .unwrap();
    menu::open(&mut a, 0);
    menu::activate(&mut a, 0, row);
    a.config.space_select = false;
    a.config.use_trash = false;
    let text = draw(&mut a, 80, 25)?;
    assert!(text.contains("permanent deletion"));
    assert!(text.contains("Space marking is disabled"));
    let close = a
        .mouse
        .regions
        .iter()
        .find(|(r, t)| r.width == 3 && *t == Target::Welcome(Action::Start))
        .unwrap()
        .0;
    mouse::handle(
        &mut a,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: close.x,
            row: close.y,
            modifiers: M::NONE,
        },
    );
    assert!(a.welcome.modals.welcome().is_none());
    assert_eq!(a.panels[0].cursor, cursor);
    assert_eq!(a.panels[0].revision, revision);
    assert!(!t.config().exists());
    Ok(())
}
#[test]
fn cli_flags_skip_welcome_and_new_configuration_creation() -> io::Result<()> {
    let t = Temp::new()?;
    for flag in ["--help", "-h", "--version", "-V"] {
        let out = Command::new(env!("CARGO_BIN_EXE_dn"))
            .arg(flag)
            .current_dir(&t.0)
            .env("XDG_CONFIG_HOME", t.0.join("absent"))
            .env("APPDATA", t.0.join("absent"))
            .env("HOME", &t.0)
            .stdin(Stdio::null())
            .output()?;
        assert!(out.status.success());
        assert!(out.stderr.is_empty());
        assert!(!out.stdout.contains(&27));
        assert!(!t.0.join("absent").exists());
    }
    Ok(())
}
