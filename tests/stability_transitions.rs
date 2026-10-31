//! Cross-owner input transitions, using private fixtures and explicit press time.
mod support;
use crossterm::event::{
    KeyCode as K, KeyEvent, KeyModifiers as M, MouseButton, MouseEvent, MouseEventKind,
};
use dn::{
    desktop::WindowId,
    editor, input,
    tetris::{self, GameAction as G, Screen},
    windowing::{self, Action as W},
};
use std::{io, time::Duration};

fn key(a: &mut dn::app::App, code: K, modifiers: M) {
    input::handle_ui(a, KeyEvent::new(code, modifiers));
}
#[test]
fn repeated_switches_cancel_old_presses_and_preserve_modal_and_geometry_ownership() -> io::Result<()>
{
    for (w, h) in [(80, 25), (120, 40)] {
        let f = support::Fixture::new("transition-stress")?;
        let mut a = f.app()?;
        editor::open(&mut a, true);
        editor::paste(&mut a, "first 界");
        let first = a.desktop.active().id;
        editor::open(&mut a, true);
        editor::paste(&mut a, "second e\u{301}");
        let second = a.desktop.active().id;
        tetris::open(&mut a);
        let mut game = a.desktop.active().id;
        let mut clock = dn::testing::InputClock::default();
        for round in 0..24 {
            a.desktop.activate(game);
            a.sync_modals();
            a.desktop.game_mut().unwrap().score = 123;
            support::draw(&mut a, w, h)?;
            let r = a
                .mouse
                .regions
                .iter()
                .find(|(_, t)| *t == tetris::target(G::New))
                .unwrap()
                .0;
            let pointer = |kind| MouseEvent {
                kind,
                column: r.x + 3,
                row: r.y,
                modifiers: M::NONE,
            };
            clock.mouse(&mut a, pointer(MouseEventKind::Down(MouseButton::Left)));
            assert!(clock.busy(&a));
            windowing::action(&mut a, W::Next);
            assert_eq!(a.desktop.active().id, WindowId::MANAGER);
            assert!(!clock.busy(&a));
            support::draw(&mut a, w, h)?;
            clock.mouse(&mut a, pointer(MouseEventKind::Up(MouseButton::Left)));
            clock.advance(&mut a, Duration::from_secs(1));
            assert_eq!(
                a.desktop.game().unwrap().score,
                123,
                "retired press in round {round}"
            );
            // Production keyboard cycling follows creation order and wraps.
            for expected in [first, second, game, WindowId::MANAGER] {
                key(&mut a, K::F(6), M::ALT);
                assert_eq!(a.desktop.active().id, expected);
                support::draw(&mut a, w, h)?;
            }
            key(&mut a, K::F(6), M::ALT | M::SHIFT);
            assert_eq!(a.desktop.active().id, game);
            support::draw(&mut a, w, h)?;
            tetris::action(&mut a, G::Setup);
            support::draw(&mut a, w, h)?;
            key(&mut a, K::F(6), M::ALT);
            assert_eq!(a.desktop.active().id, game);
            tetris::action(&mut a, G::Help);
            support::draw(&mut a, w, h)?;
            key(&mut a, K::F(6), M::ALT);
            assert_eq!(a.desktop.active().id, game);
            tetris::action(&mut a, G::Back);
            assert!(matches!(
                a.desktop.game().unwrap().screen,
                Screen::Setup { .. }
            ));
            tetris::action(&mut a, G::Back);
            support::draw(&mut a, w, h)?;
            a.desktop.activate(first);
            a.sync_modals();
            support::draw(&mut a, w, h)?;
            key(&mut a, K::F(5), M::CONTROL);
            key(&mut a, K::Right, M::SHIFT);
            key(&mut a, K::F(6), M::ALT);
            input::paste(&mut a, "must not leak");
            assert_eq!(a.desktop.active().id, first);
            // Resizing cancels manipulation and stale press ownership.
            support::draw(&mut a, w + 1, h + 1)?;
            assert!(dn::layout::prepare(
                &mut a,
                dn::presentation::layout::Rect::new(0, 0, w + 1, h + 1)
            )
            .manipulation
            .is_none());
            assert_eq!(
                a.desktop
                    .editor(first)
                    .unwrap()
                    .buffer
                    .as_ref()
                    .unwrap()
                    .text,
                "first 界"
            );
            assert_eq!(
                a.desktop
                    .editor(second)
                    .unwrap()
                    .buffer
                    .as_ref()
                    .unwrap()
                    .text,
                "second e\u{301}"
            );
            if round % 4 == 0 {
                a.desktop.activate(game);
                a.sync_modals();
                support::draw(&mut a, w, h)?;
                let r = a
                    .mouse
                    .regions
                    .iter()
                    .find(|(_, t)| *t == tetris::target(G::New))
                    .unwrap()
                    .0;
                let event = |kind| MouseEvent {
                    kind,
                    column: r.x + 3,
                    row: r.y,
                    modifiers: M::NONE,
                };
                clock.mouse(&mut a, event(MouseEventKind::Down(MouseButton::Left)));
                let retired = game;
                a.desktop.close(retired);
                a.sync_modals();
                assert!(!clock.busy(&a));
                tetris::open(&mut a);
                game = a.desktop.active().id;
                assert_ne!(game, retired);
                assert!(!a.desktop.contains(retired));
                a.desktop.game_mut().unwrap().score = 999;
                support::draw(&mut a, w, h)?;
                clock.mouse(&mut a, event(MouseEventKind::Up(MouseButton::Left)));
                clock.advance(&mut a, Duration::from_secs(1));
                assert_eq!(
                    a.desktop.game().unwrap().score,
                    999,
                    "release belongs to retired window"
                );
            }
        }
    }
    Ok(())
}
