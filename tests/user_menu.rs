use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use dn::testing::{backend::TestBackend, Terminal};
#[cfg(unix)]
use dn::user_menu::runner;
use dn::{
    app::App,
    focus::{self, Focus},
    input,
    mouse::{self, Target},
    ui,
    user_menu::{
        self,
        format::{self, parse, words},
        Action,
    },
};
use std::{
    fs, io,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    thread,
    time::{Duration, Instant},
};
struct Temp(PathBuf);
impl Temp {
    fn new() -> io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "dn-user-menu-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path)?;
        Ok(Self(fs::canonicalize(path)?))
    }
    fn app(&self) -> io::Result<App> {
        let mut a = App::new(self.0.clone())?;
        wait(&mut a, |a| a.panels.iter().all(|p| !p.loading));
        Ok(a)
    }
    fn open(&self, a: &mut App, text: &str) -> io::Result<()> {
        let p = self.0.join("dn.mnu");
        fs::write(&p, text)?;
        user_menu::open_file(a, p, true);
        wait(a, |a| !user_menu::pending(&a.user_menu));
        Ok(())
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn wait(a: &mut App, ready: impl Fn(&App) -> bool) {
    let end = Instant::now() + Duration::from_secs(8);
    loop {
        a.poll();
        if ready(a) {
            return;
        }
        assert!(Instant::now() < end, "worker timeout");
        thread::sleep(Duration::from_millis(1));
    }
}
fn key(a: &mut App, code: KeyCode) {
    input::handle(a, KeyEvent::new(code, KeyModifiers::NONE));
}
fn draw(a: &mut App, w: u16, h: u16) -> io::Result<String> {
    let mut t = Terminal::new(TestBackend::new(w, h))?;
    t.draw(|f| ui::draw(f, a))?;
    Ok(t.backend()
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
            modifiers: KeyModifiers::NONE,
        },
    );
}
#[test]
fn original_nesting_mnemonics_separators_and_multiline() -> io::Result<()> {
    let items=parse("; comment\n>1 ~T~ools\n>2 ~P~ath\nprintf '%s\\n' '{dir}'\nprintf done\n>1 -\n>1 F8 ~A~ction\ntrue\n")?;
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].children[0].commands.len(), 2);
    assert!(items[1].separator());
    assert_eq!(dn::menu::mnemonic(&items[0].label), Some('t'));
    assert!(!parse(format::DEFAULT)?.is_empty());
    Ok(())
}
#[test]
fn invalid_files_are_bounded_and_report_line_numbers() {
    for text in [
        ">2 Skip\ntrue",
        ">1 P\ntrue\n>2 C\ntrue",
        ">1 -\ntrue",
        ">1 Empty\n\"bad",
        "true",
        ">9 Deep",
        ">0 Zero",
        ">1 ",
        ">1 X\n\0",
    ] {
        assert!(parse(text).is_err(), "{text:?}");
    }
    assert!(parse(&"a".repeat(format::MAX_BYTES + 1)).is_err());
    assert!(parse(&format!(">1 X\n{}", "true\n".repeat(33))).is_err());
}
#[test]
fn argument_lexer_preserves_empty_unicode_windows_paths_and_literal_metacharacters(
) -> io::Result<()> {
    assert_eq!(
        words(r#"program "" '文 a' "C:\Tools\app.exe" a\ b ';$(touch bad)'"#)?,
        [
            "program",
            "",
            "文 a",
            r"C:\Tools\app.exe",
            "a b",
            ";$(touch bad)"
        ]
    );
    assert!(words("program 'bad").is_err());
    Ok(())
}
#[test]
fn native_arguments_marks_filters_and_context_are_snapshots() -> io::Result<()> {
    let t = Temp::new()?;
    for n in ["a ;$(touch BAD).txt", "文.txt"] {
        fs::write(t.0.join(n), b"x")?;
    }
    let mut a = t.app()?;
    a.panels[0].cursor = a.panels[0]
        .entries
        .iter()
        .position(|e| e.name == "文.txt")
        .unwrap();
    a.panels[0].select_visible();
    let commands = vec![vec![
        "printf".into(),
        "{file}".into(),
        "{name}".into(),
        "{dir}".into(),
        "{other_dir}".into(),
        "{selected}".into(),
    ]];
    let p = user_menu::plan(&a, &commands)?;
    assert_eq!(p.cwd, t.0);
    assert_eq!(p.commands[0].args[0], t.0.join("文.txt"));
    assert_eq!(p.commands[0].args[1], "文.txt");
    assert_eq!(p.commands[0].args.len(), 6);
    a.panels[0].set_filter("no-match".into());
    assert!(user_menu::plan(&a, &commands).is_err());
    assert_eq!(p.commands[0].args.len(), 6);
    assert!(user_menu::plan(&a, &[vec!["echo".into(), "prefix{file}".into()]]).is_err());
    Ok(())
}
#[test]
fn keyboard_submenus_review_cancel_run_and_resizing() -> io::Result<()> {
    let t = Temp::new()?;
    let mut a = t.app()?;
    t.open(
        &mut a,
        ">1 ~T~ools\n>2 ~P~ath\nprintf '{dir}'\n>1 -\n>1 ~O~ther\nprintf done\n",
    )?;
    for (w, h) in [(80, 25), (132, 40), (20, 10), (80, 25)] {
        draw(&mut a, w, h)?;
    }
    assert_eq!(focus::current(&a), Focus::UserMenu);
    key(&mut a, KeyCode::Char('t'));
    assert_eq!(a.user_menu.modals.user_menu().unwrap().branch, [0]);
    key(&mut a, KeyCode::Char('p'));
    assert!(a.user_menu.modals.user_menu().unwrap().review.is_some());
    assert!(a.user_menu.pending.is_none());
    draw(&mut a, 80, 25)?;
    key(&mut a, KeyCode::Esc);
    assert!(a.user_menu.modals.user_menu().unwrap().review.is_none());
    key(&mut a, KeyCode::Enter);
    draw(&mut a, 80, 25)?;
    click(&mut a, Target::UserMenu(Action::Run));
    assert!(a.user_menu.pending.is_some());
    assert!(a.user_menu.modals.user_menu().is_none());
    Ok(())
}
#[test]
fn edit_missing_default_reload_and_invalid_menu_repair() -> io::Result<()> {
    let t = Temp::new()?;
    let mut a = t.app()?;
    let path = t.0.join("dn.mnu");
    user_menu::open_file(&mut a, path.clone(), true);
    wait(&mut a, |a| !user_menu::pending(&a.user_menu));
    assert!(!path.exists());
    key(&mut a, KeyCode::F(4));
    wait(&mut a, |a| {
        a.desktop
            .active_editor()
            .is_some_and(|e| e.buffer.is_some())
    });
    assert!(path.exists());
    assert_eq!(fs::read_to_string(&path)?, format::DEFAULT);
    key(&mut a, KeyCode::Esc);
    wait(&mut a, |a| {
        a.desktop.active_editor().is_none() && !user_menu::pending(&a.user_menu)
    });
    t.open(&mut a, ">3 Bad\n")?;
    assert!(a
        .user_menu
        .modals
        .user_menu()
        .unwrap()
        .status
        .contains("line 1"));
    key(&mut a, KeyCode::F(4));
    wait(&mut a, |a| {
        a.desktop
            .active_editor()
            .is_some_and(|e| e.buffer.is_some())
    });
    assert_eq!(fs::read_to_string(&path)?, ">3 Bad\n");
    Ok(())
}
#[test]
fn f2_local_discovery_editor_precedence_stale_close_and_menu_targets() -> io::Result<()> {
    let t = Temp::new()?;
    fs::create_dir(t.0.join("child"))?;
    fs::write(t.0.join("dn.mnu"), ">1 ~P~ath\nprintf '{dir}'\n")?;
    let mut a = App::new(t.0.join("child"))?;
    wait(&mut a, |a| a.panels.iter().all(|p| !p.loading));
    key(&mut a, KeyCode::F(2));
    wait(&mut a, |a| !user_menu::pending(&a.user_menu));
    assert_eq!(
        a.user_menu
            .modals
            .user_menu()
            .unwrap()
            .data
            .as_ref()
            .unwrap()
            .path,
        t.0.join("dn.mnu")
    );
    let text = draw(&mut a, 80, 25)?;
    assert!(text.contains("User menu"));
    assert!(text.contains(if a.config.shortcuts.is_mac() {
        "^O2 User"
    } else {
        "F2 User"
    }));
    key(&mut a, KeyCode::Esc);
    assert!(a.user_menu.modals.user_menu().is_none());
    user_menu::open_file(&mut a, t.0.join("dn.mnu"), true);
    key(&mut a, KeyCode::Esc);
    for _ in 0..5 {
        a.poll();
    }
    assert!(a.user_menu.modals.user_menu().is_none());
    let item = dn::menu::items(2)
        .into_iter()
        .find(|i| i.action == dn::menu::Action::UserMenu)
        .unwrap();
    assert_eq!(item.shortcut, "F2");
    Ok(())
}
#[test]
fn mouse_submenu_back_review_cancel_and_modal_precedence() -> io::Result<()> {
    let t = Temp::new()?;
    let mut a = t.app()?;
    t.open(&mut a, ">1 ~T~ools\n>2 ~P~ath\nprintf '{dir}'\n")?;
    draw(&mut a, 80, 25)?;
    click(&mut a, Target::UserMenu(Action::Choose(0)));
    draw(&mut a, 80, 25)?;
    click(&mut a, Target::UserMenu(Action::Choose(0)));
    draw(&mut a, 80, 25)?;
    a.error("Keep modal precedence");
    draw(&mut a, 80, 25)?;
    key(&mut a, KeyCode::Enter);
    assert!(a.user_menu.modals.user_menu().unwrap().review.is_some());
    draw(&mut a, 80, 25)?;
    click(&mut a, Target::UserMenu(Action::Cancel));
    key(&mut a, KeyCode::Left);
    assert!(a.user_menu.modals.user_menu().unwrap().branch.is_empty());
    draw(&mut a, 80, 25)?;
    click(&mut a, Target::UserMenu(Action::Close));
    assert!(a.user_menu.modals.user_menu().is_none());
    Ok(())
}
#[cfg(unix)]
#[test]
fn runner_literal_arguments_environment_failure_stop_and_native_names() -> io::Result<()> {
    use std::os::unix::{
        ffi::{OsStrExt, OsStringExt},
        fs::symlink,
    };
    let t = Temp::new()?;
    let name = std::ffi::OsString::from_vec(b"x-\xff;$(touch BAD)".to_vec());
    fs::write(t.0.join("placeholder"), b"data")?;
    let mut a = t.app()?;
    // macOS may reject undecodable filenames; cached native arguments need no
    // filesystem access, so exercise their exact byte preservation directly.
    let index = a.panels[0].entries.iter().position(|e| !e.parent).unwrap();
    a.panels[0].entries[index].name = name.clone();
    a.panels[0].entries[index].path = t.0.join(&name);
    a.panels[0].cursor = index;
    let commands = vec![vec![
        "/bin/sh".into(),
        "-c".into(),
        "printf '%s' \"$1\" > captured; printf '%s' \"$DN_DIR\" > directory".into(),
        "test".into(),
        "{file}".into(),
    ]];
    let plan = user_menu::plan(&a, &commands)?;
    assert!(runner::run(&plan)
        .map_err(|e| io::Error::other(format!("execute: {e}")))?
        .success());
    assert_eq!(
        fs::read(t.0.join("captured"))?,
        t.0.join(&name).as_os_str().as_bytes()
    );
    assert_eq!(fs::read(t.0.join("directory"))?, t.0.as_os_str().as_bytes());
    assert!(!t.0.join("BAD").exists());
    let plan = user_menu::plan(
        &a,
        &[
            vec!["/bin/sh".into(), "-c".into(), "exit 7".into()],
            vec!["touch".into(), "must-not-exist".into()],
        ],
    )?;
    assert_eq!(runner::run(&plan)?.code(), Some(7));
    assert!(!t.0.join("must-not-exist").exists());
    let plan = user_menu::plan(
        &a,
        &[vec![t
            .0
            .join("missing-program")
            .to_string_lossy()
            .into_owned()]],
    )?;
    assert!(runner::run(&plan).is_err());
    symlink(t.0.join("captured"), t.0.join("link.mnu"))?;
    assert!(user_menu::load(t.0.join("link.mnu"), true)?.error.is_some());
    Ok(())
}

#[test]
fn review_wraps_unicode_arguments_and_expansion_is_bounded() -> io::Result<()> {
    let t = Temp::new()?;
    let mut a = t.app()?;
    t.open(&mut a, &format!(">1 Long\nprintf '{}'\n", "界".repeat(200)))?;
    key(&mut a, KeyCode::Enter);
    let m = a.user_menu.modals.user_menu().unwrap();
    assert!(m.review_lines.len() > 10);
    assert!(m
        .review_lines
        .iter()
        .all(|s| dn::testing::text::Span::raw(s).width() <= 72));
    assert!(user_menu::plan(&a, &[vec!["printf".into(), "x".repeat(70 * 1024)]]).is_err());
    assert!(user_menu::plan(&a, &[vec!["{selected}".into()]]).is_err());
    key(&mut a, KeyCode::F(1));
    assert_eq!(focus::current(&a), Focus::Message);
    key(&mut a, KeyCode::Esc);
    assert!(a.user_menu.modals.user_menu().unwrap().review.is_some());
    Ok(())
}
