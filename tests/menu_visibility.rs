use crossterm::event::{
    KeyCode as K, KeyEvent, KeyModifiers as M, MouseButton, MouseEvent, MouseEventKind,
};
use dn::{
    app::App,
    config::Config,
    input,
    mouse::{self, Target},
    settings::{self, Setting, SettingsKind},
    testing::{backend::TestBackend, Buffer, Terminal},
    ui,
};
use std::{
    fs, io,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn fixture() -> io::Result<(Temp, App)> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let temp = Temp(std::env::temp_dir().join(format!(
        "dn-menu-hide-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )));
    fs::create_dir(&temp.0)?;
    for i in 0..80 {
        fs::write(temp.0.join(format!("item-{i:03}")), "fixture")?;
    }
    let mut app = App::new(temp.0.clone())?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.panels.iter().any(|p| p.loading) {
        app.poll();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    app.config.save_on_exit = false;
    Ok((temp, app))
}
fn draw(app: &mut App) -> io::Result<Buffer> {
    let mut t = Terminal::new(TestBackend::new(80, 25))?;
    t.draw(|f| ui::draw(f, app))?;
    Ok(t.backend().buffer().clone())
}
fn key(app: &mut App, code: K, mods: M) {
    input::handle(app, KeyEvent::new(code, mods));
}
fn click(app: &mut App, target: Target) {
    let r = app
        .mouse
        .regions
        .iter()
        .rev()
        .find(|(_, t)| *t == target)
        .unwrap()
        .0;
    mouse::handle(
        app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: r.x,
            row: r.y,
            modifiers: M::NONE,
        },
    );
}
fn top(buffer: &Buffer) -> String {
    (0..80).map(|x| buffer[(x, 0)].symbol()).collect()
}
#[test]
fn hidden_menu_reclaims_row_temporarily_reveals_and_keeps_mouse_hits_above_panes() -> io::Result<()>
{
    let (_temp, mut app) = fixture()?;
    assert!(top(&draw(&mut app)?).contains("Utilities"));
    let shown = app.panels[0].viewport_rows;
    app.config.hide_menu = true;
    assert!(!top(&draw(&mut app)?).contains("Utilities"));
    assert_eq!(app.panels[0].viewport_rows, shown + 1);
    let hidden_rows = app.panels[0].viewport_rows;
    assert!(!app
        .mouse
        .regions
        .iter()
        .any(|(_, t)| matches!(t, Target::Menu(_))));
    key(&mut app, K::F(9), M::NONE);
    assert!(top(&draw(&mut app)?).contains("Utilities"));
    assert_eq!(app.panels[0].viewport_rows, hidden_rows);
    click(&mut app, Target::Menu(5));
    assert_eq!(app.modals.menu().unwrap().0, 5);
    key(&mut app, K::Esc, M::NONE);
    assert!(!top(&draw(&mut app)?).contains("Utilities"));
    key(&mut app, K::Char('f'), M::ALT);
    assert!(top(&draw(&mut app)?).contains("Utilities"));
    key(&mut app, K::Esc, M::NONE);
    draw(&mut app)?;
    click(&mut app, Target::RevealMenu);
    assert!(app.modals.menu().is_some());
    key(&mut app, K::Esc, M::NONE);
    app.config.hide_menu = false;
    assert!(top(&draw(&mut app)?).contains("Utilities"));
    assert_eq!(app.panels[0].viewport_rows, shown);
    Ok(())
}
#[test]
fn interface_hide_option_is_a_cancelable_draft_and_round_trips() -> io::Result<()> {
    let (temp, mut app) = fixture()?;
    app.open_settings(SettingsKind::Interface);
    let index = app
        .modals
        .settings()
        .unwrap()
        .rows()
        .iter()
        .position(|s| matches!(s, Setting::HideMenu))
        .unwrap();
    app.modals.settings_mut().unwrap().toggle(index);
    assert!(!app.config.hide_menu);
    key(&mut app, K::Esc, M::NONE);
    assert!(!app.config.hide_menu);
    app.open_settings(SettingsKind::Interface);
    app.modals.settings_mut().unwrap().toggle(index);
    settings::accept(&mut app);
    assert!(app.config.hide_menu);
    let path = temp.0.join("settings.conf");
    app.config.save(&path)?;
    assert_eq!(Config::read(&path)?, app.config);
    Ok(())
}
#[test]
fn tetris_hides_manager_menus_and_uses_dn_faces_mnemonics_and_offset_shadows() -> io::Result<()> {
    let (_temp, mut app) = fixture()?;
    for profile in [
        dn::palette::PaletteProfile::ClassicDn,
        dn::palette::PaletteProfile::Terminal,
    ] {
        app.config.palette = profile;
        dn::tetris::open(&mut app);
        let b = draw(&mut app)?;
        assert!(!top(&b).contains("Utilities"));
        assert!(!app
            .mouse
            .regions
            .iter()
            .any(|(_, t)| matches!(t, Target::Menu(_) | Target::RevealMenu)));
        for (_, action) in dn::tetris::BUTTONS {
            let r = app
                .mouse
                .regions
                .iter()
                .find(|(r, t)| *t == dn::tetris::target(action) && r.width == 11)
                .unwrap()
                .0;
            for x in r.x + 1..r.right() - 1 {
                assert_eq!(b[(x, r.y)].bg, profile.color(8));
            }
            assert!((r.x..r.right() - 1).any(|x| b[(x, r.y)].fg == profile.color(14)));
            assert_eq!(b[(r.right() - 1, r.y)].symbol(), " ");
            assert_eq!(b[(r.right() - 1, r.y)].bg, profile.color(0));
            assert_eq!(b[(r.x + 2, r.y + 1)].symbol(), " ");
            assert_eq!(b[(r.x + 2, r.y + 1)].bg, profile.color(0));
        }
        key(&mut app, K::F(9), M::NONE);
        assert!(app.modals.menu().is_none());
        key(&mut app, K::Char('p'), M::ALT);
        assert!(app.desktop.game().unwrap().paused);
        app.config.language = dn::localization::Language::Russian;
        key(&mut app, K::Char('п'), M::ALT);
        assert!(!app.desktop.game().unwrap().paused);
        key(&mut app, K::Char('а'), M::ALT);
        assert!(matches!(
            app.desktop.game().unwrap().screen,
            dn::tetris::Screen::Setup { .. }
        ));
        key(&mut app, K::Esc, M::NONE);
        app.config.language = dn::localization::Language::English;
        key(&mut app, K::Esc, M::NONE);
        assert!(top(&draw(&mut app)?).contains("Utilities"));
    }
    Ok(())
}

#[test]
fn hidden_menu_keeps_modal_guards_and_translated_mouse_targets() -> io::Result<()> {
    use dn::{layout, presentation::layout::Rect};
    let (_temp, mut app) = fixture()?;
    app.config.hide_menu = true;
    let snapshot = layout::prepare_window(
        &mut app,
        Rect::new(7, 3, 80, 25),
        Rect::new(0, 0, 90, 30),
        Instant::now(),
    );
    let frame = ui::render(&app, &snapshot);
    app.mouse.regions = frame.hit_regions().to_vec();
    app.mouse.frame = Some(frame.context);
    click(&mut app, Target::RevealMenu);
    assert!(app.modals.menu().is_some());
    key(&mut app, K::Esc, M::NONE);
    app.prompt_operation(dn::operations::Kind::Mkdir, false);
    draw(&mut app)?;
    key(&mut app, K::F(9), M::NONE);
    key(&mut app, K::Char('f'), M::ALT);
    assert!(app.modals.menu().is_none());
    assert!(app.modals.prompt().is_some());
    key(&mut app, K::Esc, M::NONE);
    dn::quick_view::toggle(&mut app);
    key(&mut app, K::F(9), M::NONE);
    assert!(top(&draw(&mut app)?).contains("Options"));
    click(&mut app, Target::Menu(5));
    assert_eq!(app.modals.menu().unwrap().0, 5);
    Ok(())
}
