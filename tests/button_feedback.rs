mod support;
use crossterm::event::{
    KeyCode as K, KeyEvent, KeyModifiers as M, MouseButton as B, MouseEvent, MouseEventKind as E,
};
use dn::{
    app::App,
    input,
    mouse::{self, Target},
    operations::Kind,
    presentation::layout::Rect,
    testing::Buffer,
};
use std::{
    io,
    time::{Duration, Instant},
};
fn pointer(a: &mut App, kind: E, x: u16, y: u16) {
    mouse::handle_ui(
        a,
        MouseEvent {
            kind,
            column: x,
            row: y,
            modifiers: M::NONE,
        },
    );
}
fn target(a: &App, t: Target) -> Rect {
    a.mouse
        .regions
        .iter()
        .rev()
        .find(|(_, v)| *v == t)
        .unwrap()
        .0
}
fn finish(a: &mut App) {
    let end = Instant::now() + Duration::from_millis(110);
    while Instant::now() < end {
        a.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    a.poll();
}
fn depressed(before: &Buffer, after: &Buffer, r: Rect) {
    assert_ne!(before, after, "press must visibly change the face");
    for x in r.x..r.right() {
        assert_eq!(
            after[(x, r.bottom())].bg,
            before[(r.x, r.y)].bg,
            "shadow removed at {x}"
        );
    }
    for x in r.x + 3..r.right() - 1 {
        assert_eq!(
            after[(x, r.y)].symbol(),
            before[(x - 1, r.y)].symbol(),
            "label shifts right"
        );
    }
}
#[test]
fn injected_clock_checks_exact_press_deadline_and_escape_cancellation() -> io::Result<()> {
    let f = support::Fixture::new("button-clock")?;
    let mut a = f.app()?;
    let mut clock = dn::testing::InputClock::default();
    for cancel in [false, true] {
        a.prompt_operation(Kind::Mkdir, false);
        support::draw(&mut a, 80, 25)?;
        // Focus Cancel without relying on elapsed wall-clock time.
        input::handle(&mut a, KeyEvent::new(K::Tab, M::NONE));
        input::handle(&mut a, KeyEvent::new(K::Tab, M::NONE));
        support::draw(&mut a, 80, 25)?;
        clock.key(&mut a, KeyEvent::new(K::Enter, M::NONE));
        assert!(clock.busy(&a));
        assert!(!clock.advance(&mut a, Duration::from_millis(89)));
        assert!(a.modals.prompt().is_some());
        if cancel {
            clock.key(&mut a, KeyEvent::new(K::Esc, M::NONE));
            assert!(!clock.busy(&a));
            assert!(!clock.advance(&mut a, Duration::from_secs(30)));
            assert!(a.modals.prompt().is_some());
            input::handle(&mut a, KeyEvent::new(K::Esc, M::NONE));
        } else {
            assert!(clock.advance(&mut a, Duration::from_millis(1)));
            assert!(a.modals.prompt().is_none());
            assert!(!clock.busy(&a));
            assert!(!clock.advance(&mut a, Duration::from_secs(30)));
        }
    }
    Ok(())
}

#[test]
fn injected_clock_mouse_hold_and_early_release_dispatch_only_once() -> io::Result<()> {
    for held in [false, true] {
        let f = support::Fixture::new("button-clock-mouse")?;
        let mut a = f.app()?;
        let mut clock = dn::testing::InputClock::default();
        a.prompt_operation(Kind::Mkdir, false);
        support::draw(&mut a, 80, 25)?;
        let r = target(&a, Target::CancelPrompt);
        let event = |kind| MouseEvent {
            kind,
            column: r.x + 3,
            row: r.y,
            modifiers: M::NONE,
        };
        clock.mouse(&mut a, event(E::Down(B::Left)));
        assert!(clock.busy(&a));
        if held {
            assert!(!clock.advance(&mut a, Duration::from_secs(30)));
            assert!(a.modals.prompt().is_some());
        }
        clock.mouse(&mut a, event(E::Up(B::Left)));
        if !held {
            assert!(a.modals.prompt().is_some());
            assert!(!clock.advance(&mut a, Duration::from_millis(89)));
            assert!(clock.advance(&mut a, Duration::from_millis(1)));
        }
        assert!(a.modals.prompt().is_none());
        assert!(!clock.busy(&a));
        assert!(!clock.advance(&mut a, Duration::from_secs(30)));
    }
    Ok(())
}
#[test]
fn mouse_hold_tracks_leaving_and_returning_and_only_submits_on_release() -> io::Result<()> {
    let f = support::Fixture::new("button-hold")?;
    let mut a = f.app()?;
    a.prompt_operation(Kind::Mkdir, false);
    input::paste(&mut a, "new-folder");
    let normal = support::draw(&mut a, 100, 32)?;
    let r = target(&a, Target::SubmitPrompt);
    pointer(&mut a, E::Down(B::Left), r.x + 3, r.y);
    let pressed = support::draw(&mut a, 100, 32)?;
    depressed(&normal, &pressed, r);
    assert!(!f.0.join("new-folder").exists());
    pointer(&mut a, E::Drag(B::Left), 0, 0);
    assert_eq!(support::draw(&mut a, 100, 32)?, normal);
    pointer(&mut a, E::Drag(B::Left), r.x + 3, r.y);
    assert_eq!(support::draw(&mut a, 100, 32)?, pressed);
    finish(&mut a); // Holding never submits, even after animation time.
    assert!(!f.0.join("new-folder").exists());
    pointer(&mut a, E::Up(B::Left), r.x + 3, r.y);
    let end = Instant::now() + Duration::from_secs(3);
    while a.job.is_some() {
        a.poll();
        assert!(Instant::now() < end);
    }
    assert!(f.0.join("new-folder").is_dir());
    Ok(())
}
#[test]
fn release_outside_escape_and_resize_cancel_without_submitting() -> io::Result<()> {
    let f = support::Fixture::new("button-cancel")?;
    let mut a = f.app()?;
    a.prompt_operation(Kind::Mkdir, false);
    input::paste(&mut a, "never-created");
    for mode in 0..3 {
        support::draw(&mut a, 100, 32)?;
        let r = target(&a, Target::SubmitPrompt);
        pointer(&mut a, E::Down(B::Left), r.x + 3, r.y);
        match mode {
            0 => pointer(&mut a, E::Up(B::Left), 0, 0),
            1 => input::handle_ui(&mut a, KeyEvent::new(K::Esc, M::NONE)),
            _ => {
                support::draw(&mut a, 80, 25)?;
                pointer(&mut a, E::Up(B::Left), r.x + 3, r.y);
            }
        }
        finish(&mut a);
        assert!(a.modals.prompt().is_some());
        assert!(!f.0.join("never-created").exists());
    }
    Ok(())
}
#[test]
fn keyboard_default_and_focused_cancel_show_feedback_without_crossing_owners() -> io::Result<()> {
    let f = support::Fixture::new("button-key")?;
    let mut a = f.app()?;
    a.prompt_operation(Kind::Mkdir, false);
    let normal = support::draw(&mut a, 100, 32)?;
    let r = target(&a, Target::SubmitPrompt);
    input::handle_ui(&mut a, KeyEvent::new(K::Enter, M::NONE));
    depressed(&normal, &support::draw(&mut a, 100, 32)?, r);
    input::handle_ui(&mut a, KeyEvent::new(K::Esc, M::NONE));
    a.modals.prompt_mut().unwrap().focus = 2;
    let normal = support::draw(&mut a, 100, 32)?;
    let r = target(&a, Target::CancelPrompt);
    input::handle_ui(&mut a, KeyEvent::new(K::Char(' '), M::NONE));
    depressed(&normal, &support::draw(&mut a, 100, 32)?, r);
    finish(&mut a);
    assert!(a.modals.prompt().is_none());
    a.prompt_operation(Kind::Mkdir, false);
    support::draw(&mut a, 100, 32)?;
    let r = target(&a, Target::CancelPrompt);
    pointer(&mut a, E::Down(B::Left), r.x + 3, r.y);
    a.modals.set_prompt(None);
    a.prompt_operation(Kind::Mkdir, false);
    support::draw(&mut a, 100, 32)?;
    pointer(&mut a, E::Up(B::Left), r.x + 3, r.y);
    finish(&mut a);
    assert!(
        a.modals.prompt().is_some(),
        "retired button cannot close a replacement"
    );
    Ok(())
}
#[test]
fn plugin_play_symbol_survives_press_and_rendering_is_immutable() -> io::Result<()> {
    let f = support::Fixture::new("button-plugin")?;
    let mut a = f.app()?;
    a.config.set_plugin_enabled(dn::plugins::MP3_PLAYER, true);
    dn::menu::execute(&mut a, dn::menu::Action::Plugin(dn::plugins::MP3_PLAYER));
    let normal = support::draw(&mut a, 80, 25)?;
    let r = target(
        &a,
        Target::Plugin(dn::plugins::MP3_PLAYER, dn::plugins::ActionId(2)),
    );
    input::handle_ui(&mut a, KeyEvent::new(K::Enter, M::NONE));
    let prepared = dn::layout::prepare(&mut a, Rect::new(0, 0, 80, 25));
    let first = Buffer::rendered(dn::ui::render(&a, &prepared));
    let second = Buffer::rendered(dn::ui::render(&a, &prepared));
    assert_eq!(first, second);
    assert_ne!(normal, first);
    assert!(
        (r.x + 3..r.right() - 1).any(|x| first[(x, r.y)].symbol() == "►"),
        "Play caption retained"
    );
    finish(&mut a);
    let restored = support::draw(&mut a, 80, 25)?;
    for y in r.y..=r.bottom() {
        for x in r.x..r.right() {
            assert_eq!(
                normal[(x, y)],
                restored[(x, y)],
                "restored button at {x},{y}"
            );
        }
    }
    Ok(())
}
#[test]
fn game_over_space_does_not_capture_escape_as_a_button_cancellation() -> io::Result<()> {
    use dn::tetris::{self, Screen};
    for (w, h) in [(80, 25), (100, 32)] {
        let f = support::Fixture::new("game-over-space-escape")?;
        let mut a = f.app()?;
        tetris::open(&mut a);
        {
            let g = a.desktop.game_mut().unwrap();
            g.screen = Screen::GameOver;
            g.over = true;
            g.page_focus = 0;
        }
        let normal = support::draw(&mut a, w, h)?;
        input::handle_ui(&mut a, KeyEvent::new(K::Char(' '), M::NONE));
        assert!(
            normal == support::draw(&mut a, w, h)?,
            "Space belongs to the game controller, not the dialog's OK button"
        );
        input::handle_ui(&mut a, KeyEvent::new(K::Esc, M::NONE));
        assert_eq!(a.desktop.game().unwrap().screen, Screen::Play);
        assert!(a.desktop.game().unwrap().over);
        finish(&mut a);
        assert_eq!(a.desktop.game().unwrap().screen, Screen::Play);
        assert!(a.desktop.game().unwrap().over);
    }
    Ok(())
}

#[test]
fn tetris_pages_and_custom_colours_use_the_same_press_face() -> io::Result<()> {
    let f = support::Fixture::new("button-tetris")?;
    let mut a = f.app()?;
    a.config.language = dn::localization::Language::Russian;
    a.config.palette = a
        .config
        .palette
        .with_slot(dn::palette::theme::Slot::DialogText, 0x2f)
        .with_slot(dn::palette::theme::Slot::DialogDefault, 0x5e);
    dn::tetris::open(&mut a);
    {
        let g = a.desktop.game_mut().unwrap();
        g.screen = dn::tetris::Screen::GameOver;
        g.over = true;
        g.page_focus = 0;
    }
    let normal = support::draw(&mut a, 80, 25)?;
    input::handle_ui(&mut a, KeyEvent::new(K::Enter, M::NONE));
    let down = support::draw(&mut a, 80, 25)?;
    assert_ne!(normal, down);
    assert_eq!(
        a.desktop.game().unwrap().screen,
        dn::tetris::Screen::GameOver
    );
    finish(&mut a);
    assert_ne!(
        a.desktop.game().unwrap().screen,
        dn::tetris::Screen::GameOver
    );
    Ok(())
}
#[test]
fn native_file_picker_preserves_held_mouse_and_keyboard_feedback() -> io::Result<()> {
    let f = support::Fixture::new("button-native")?;
    let mut a = f.app()?;
    dn::editor::open(&mut a, true);
    dn::editor::action(&mut a, dn::editor::EditorAction::Open);
    let normal = support::draw(&mut a, 100, 32)?;
    // Tab through the native controls until a button is focused. Cancel is a
    // harmless target, so this also checks focused alternatives beat defaults.
    let mut cancel = None;
    for y in 0..32 {
        for x in 0..94 {
            let text = (x..x + 6)
                .map(|i| normal[(i, y)].symbol())
                .collect::<String>();
            if text == "Cancel" {
                cancel = Some((x, y));
            }
        }
    }
    let (x, y) = cancel.expect("native Cancel caption");
    pointer(&mut a, E::Down(B::Left), x, y);
    let down = support::draw(&mut a, 100, 32)?;
    assert_ne!(
        normal, down,
        "native mouse down is not skinned back to raised"
    );
    assert!(matches!(
        a.desktop.active_editor().unwrap().modals.editor_modal(),
        Some(dn::editor::Modal::File(_))
    ));
    pointer(&mut a, E::Drag(B::Left), 0, 0);
    let out = support::draw(&mut a, 100, 32)?;
    assert_ne!(down, out);
    pointer(&mut a, E::Up(B::Left), 0, 0);
    let normal = support::draw(&mut a, 100, 32)?;
    input::handle_ui(&mut a, KeyEvent::new(K::Enter, M::NONE));
    let down = support::draw(&mut a, 100, 32)?;
    assert_ne!(normal, down, "focused native Cancel animates for Enter");
    finish(&mut a);
    assert!(a
        .desktop
        .active_editor()
        .unwrap()
        .modals
        .editor_modal()
        .is_none());
    Ok(())
}
#[test]
fn copy_move_buttons_and_translated_viewports_keep_their_original_targets() -> io::Result<()> {
    let f = support::Fixture::new("button-fileops")?;
    std::fs::write(f.0.join("source.txt"), "unchanged")?;
    let mut a = f.app()?;
    a.panels[0].cursor = a.panels[0]
        .entries
        .iter()
        .position(|e| e.name == "source.txt")
        .unwrap();
    for kind in [Kind::Copy, Kind::Move] {
        a.prompt_operation(kind, false);
        let prepared = dn::layout::prepare_window(
            &mut a,
            Rect::new(7, 3, 100, 32),
            Rect::new(0, 0, 110, 36),
            Instant::now(),
        );
        let frame = dn::ui::render(&a, &prepared);
        a.mouse.regions = frame.hit_regions().to_vec();
        a.mouse.frame = Some(frame.context);
        let normal = Buffer::rendered(frame);
        let r = target(&a, Target::CancelPrompt);
        pointer(&mut a, E::Down(B::Left), r.x + 3, r.y);
        let prepared = dn::layout::prepare_window(
            &mut a,
            Rect::new(7, 3, 100, 32),
            Rect::new(0, 0, 110, 36),
            Instant::now(),
        );
        let frame = dn::ui::render(&a, &prepared);
        let hits = frame.hit_regions().to_vec();
        assert_eq!(
            hits, a.mouse.regions,
            "pressed face does not shift hit targets"
        );
        depressed(&normal, &Buffer::rendered(frame), r);
        input::paste(&mut a, "must-not-leak");
        assert!(!a.modals.prompt().unwrap().field.contains("must-not-leak"));
        pointer(&mut a, E::Up(B::Left), r.x + 3, r.y);
        finish(&mut a);
        assert!(a.modals.prompt().is_none());
        assert_eq!(
            std::fs::read_to_string(f.0.join("source.txt"))?,
            "unchanged"
        );
    }
    Ok(())
}

#[test]
fn finished_game_buttons_keep_the_clicked_face_until_release() -> io::Result<()> {
    use dn::tetris::{self, GameAction, Screen};
    for (w, h) in [(80, 25), (100, 32), (132, 40)] {
        for action in [GameAction::New, GameAction::Setup, GameAction::Scores] {
            let f = support::Fixture::new("game-over-button-aliases")?;
            let mut a = f.app()?;
            if w == 132 {
                a.config.language = dn::localization::Language::Russian;
                a.config.palette = a
                    .config
                    .palette
                    .with_slot(dn::palette::theme::Slot::DialogText, 0x1e);
            }
            tetris::open(&mut a);
            support::draw(&mut a, w, h)?;
            {
                let g = a.desktop.game_mut().unwrap();
                g.over = true;
                g.score = 166;
                g.screen = Screen::GameOver;
            }
            support::draw(&mut a, w, h)?;
            tetris::action(&mut a, GameAction::Back);
            support::draw(&mut a, w, h)?;
            input::handle(&mut a, KeyEvent::new(K::F(5), M::CONTROL));
            input::handle(&mut a, KeyEvent::new(K::Right, M::NONE));
            input::handle(&mut a, KeyEvent::new(K::Down, M::NONE));
            input::handle(&mut a, KeyEvent::new(K::Enter, M::NONE));
            let normal = support::draw(&mut a, w, h)?;
            let r = a
                .mouse
                .regions
                .iter()
                .find(|(r, t)| *t == tetris::target(action) && r.y < h - 1)
                .expect("Game button face")
                .0;
            pointer(&mut a, E::Down(B::Left), r.x + 3, r.y);
            assert!(a.desktop.game().unwrap().over, "Wait for release");
            assert_eq!(a.desktop.game().unwrap().screen, Screen::Play);
            depressed(&normal, &support::draw(&mut a, w, h)?, r);
            pointer(&mut a, E::Drag(B::Left), 0, 0);
            pointer(&mut a, E::Up(B::Left), 0, 0);
            finish(&mut a);
            assert!(a.desktop.game().unwrap().over);
            assert_eq!(a.desktop.game().unwrap().screen, Screen::Play);
            pointer(&mut a, E::Down(B::Left), r.x + 3, r.y);
            support::draw(&mut a, w, h)?;
            pointer(&mut a, E::Up(B::Left), r.x + 3, r.y);
            finish(&mut a);
            let g = a.desktop.game().unwrap();
            match action {
                GameAction::New => {
                    assert!(!g.over);
                    assert_eq!(g.score, 0);
                }
                GameAction::Setup => assert!(matches!(g.screen, Screen::Setup { .. })),
                GameAction::Scores => assert_eq!(g.screen, Screen::Scores),
                _ => unreachable!(),
            }
        }
    }
    Ok(())
}
