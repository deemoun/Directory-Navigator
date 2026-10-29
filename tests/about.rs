mod support;
use dn::{
    localization::Language, mouse::Target, palette::PaletteProfile, windowing::geometry::SurfaceKey,
};
use std::io;
const URL: &str = "https://github.com/deemoun/Directory-Navigator";
#[test]
fn about_centers_preserved_text_and_repository_link_in_native_dialog() -> io::Result<()> {
    let fixture = support::Fixture::new("about")?;
    for language in [Language::English, Language::Russian] {
        for palette in [PaletteProfile::ClassicDn, PaletteProfile::Terminal] {
            for (w, h) in [(80, 25), (132, 40)] {
                let mut app = fixture.app()?;
                app.config.language = language;
                app.config.palette = palette;
                dn::menu::execute(&mut app, dn::menu::Action::About);
                let body = app.last_dialog().unwrap().message.clone();
                assert!(body.contains("Dmitry Yarygin") && body.contains(URL));
                assert!(!body.contains("RIT") && !body.contains("FREEWARE"));
                let b = support::draw(&mut app, w, h)?;
                let modal = app.modal_top().unwrap();
                let s =
                    dn::layout::prepare(&mut app, dn::presentation::layout::Rect::new(0, 0, w, h));
                let rect = s
                    .surfaces
                    .iter()
                    .find(|s| s.id.key == SurfaceKey::Modal(modal.id))
                    .unwrap()
                    .bounds;
                assert_eq!(rect.width, 64);
                assert_eq!(rect.height, 20);
                let link = app
                    .mouse
                    .regions
                    .iter()
                    .find(|(_, t)| *t == Target::AboutRepository)
                    .unwrap()
                    .0;
                let rendered = (link.x..link.right())
                    .map(|x| b[(x, link.y)].symbol())
                    .collect::<String>();
                assert_eq!(rendered, URL);
                assert!(
                    (i32::from(link.x) * 2 + i32::from(link.width)
                        - i32::from(rect.x) * 2
                        - i32::from(rect.width))
                    .abs()
                        <= 1
                );
                assert!(b[(link.x, link.y)]
                    .modifier
                    .contains(dn::presentation::style::Modifier::UNDERLINED));
                let f = dn::ui::render(&app, &s);
                assert_eq!(
                    dn::testing::Buffer::rendered(f),
                    dn::testing::Buffer::rendered(dn::ui::render(&app, &s))
                );
                assert_eq!(app.last_dialog().unwrap().message, body);
                support::click(&mut app, Target::CloseDialog);
                assert!(app.last_dialog().is_none());
            }
        }
    }
    Ok(())
}
#[test]
fn about_keyboard_close_and_ordinary_messages_keep_existing_behaviour() -> io::Result<()> {
    let f = support::Fixture::new("about-close")?;
    let mut a = f.app()?;
    for key in [
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyCode::Esc,
    ] {
        dn::menu::execute(&mut a, dn::menu::Action::About);
        support::draw(&mut a, 80, 25)?;
        support::key(&mut a, key);
        assert!(a.last_dialog().is_none());
    }
    a.error("ordinary error");
    support::draw(&mut a, 80, 25)?;
    assert!(!a
        .mouse
        .regions
        .iter()
        .any(|(_, t)| *t == Target::AboutRepository));
    Ok(())
}

#[test]
fn repository_click_and_keyboard_report_opener_failure_without_losing_about() -> io::Result<()> {
    let f = support::Fixture::new("about-link")?;
    let mut a = f.app()?;
    // Occupy the existing launcher with a harmless stub. The real browser must
    // never be started by this test; pending admission exercises its error path.
    a.launcher.request_with(0, f.0.join("stub"), |_| {
        Ok(dn::launcher::Launched::Submitted)
    })?;
    dn::menu::execute(&mut a, dn::menu::Action::About);
    support::draw(&mut a, 80, 25)?;
    support::click(&mut a, Target::AboutRepository);
    assert!(a.last_dialog().unwrap().message.contains("still opening"));
    a.pop_dialog();
    assert!(a.last_dialog().unwrap().message.contains(URL));
    support::key(&mut a, crossterm::event::KeyCode::Char('g'));
    assert!(a.last_dialog().unwrap().message.contains("still opening"));
    a.pop_dialog();
    assert!(a.last_dialog().unwrap().message.contains(URL));
    Ok(())
}
