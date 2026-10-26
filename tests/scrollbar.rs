use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use dn::testing::{backend::TestBackend, layout::Rect, Terminal};
use dn::{
    app::App,
    input,
    mouse::{self, Target},
    palette::PaletteProfile,
    panel_layout::{scroll_position, PanelGeometry, PanelLayout, ScrollAction},
    ui,
};
use std::{
    fs, io,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};
struct Temp(PathBuf);
impl Temp {
    fn new() -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "dn-scroll-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
    fn app(&self) -> io::Result<App> {
        let mut app = App::new(self.0.clone())?;
        wait(&mut app, |a| a.panels.iter().all(|p| !p.loading));
        Ok(app)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn wait(app: &mut App, done: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done(app) {
        app.poll();
        assert!(Instant::now() < deadline, "background timeout");
        thread::sleep(Duration::from_millis(1));
    }
}
fn key(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    input::handle(app, KeyEvent::new(code, modifiers));
}
fn draw(app: &mut App, w: u16, h: u16) -> io::Result<String> {
    let mut terminal = Terminal::new(TestBackend::new(w, h))?;
    terminal.draw(|f| ui::draw(f, app))?;
    for (r, _) in &app.mouse.regions {
        assert!(r.right() <= w && r.bottom() <= h);
    }
    Ok(terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect())
}
fn click(app: &mut App, target: Target) {
    let r = app
        .mouse
        .regions
        .iter()
        .find(|(_, t)| *t == target)
        .expect("mouse target")
        .0;
    mouse::handle(
        app,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: r.x,
            row: r.y,
            modifiers: KeyModifiers::NONE,
        },
    );
}
fn mouse_event(app: &mut App, kind: MouseEventKind, x: u16, y: u16) {
    mouse::handle(
        app,
        MouseEvent {
            kind,
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        },
    );
}
fn region(app: &App, index: usize, action: ScrollAction) -> Rect {
    app.mouse
        .regions
        .iter()
        .find(|(_, t)| *t == Target::Scrollbar(index, action))
        .expect("scrollbar target")
        .0
}
fn populated(temp: &Temp) -> io::Result<App> {
    for i in 0..180 {
        fs::write(temp.0.join(format!("file{i:03}.txt")), "fixture")?;
    }
    temp.app()
}
#[test]
fn geometry_reserves_track_and_thumb_matches_entry_position() {
    for layout in [PanelLayout::Brief, PanelLayout::Details] {
        for (w, h) in [(38, 19), (70, 34), (1, 1), (0, 0)] {
            let g = PanelGeometry::new(Rect::new(3, 4, w, h), layout);
            assert_eq!(g.scrollbar.width, u16::from(w > 0));
            for column in &g.columns {
                assert!(column.right() <= g.scrollbar.x || w == 0);
            }
            for count in [0, 1, 2, 10, 10_000, usize::MAX] {
                let track = g.scroll_track();
                if count <= 1 || track.is_empty() {
                    assert!(g.scroll_thumb(0, count).is_none());
                    continue;
                }
                assert_eq!(g.scroll_thumb(0, count).expect("thumb").y, track.y);
                if count > 1 {
                    assert_eq!(
                        g.scroll_thumb(count - 1, count).expect("thumb").y,
                        track.bottom() - 1
                    );
                    if track.height > 1 {
                        assert_eq!(scroll_position(track, 0, count), 0);
                        assert_eq!(scroll_position(track, u16::MAX, count), count - 1);
                    }
                }
                let positions = [0, count / 4, count / 2, count - 1]
                    .map(|i| g.scroll_thumb(i, count).expect("thumb").y);
                assert!(positions.windows(2).all(|p| p[0] <= p[1]));
            }
        }
    }
}
#[test]
fn fixed_scrollbar_both_layouts_profiles_and_no_listing_reflow() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = populated(&temp)?;
    for profile in [PaletteProfile::ClassicDn, PaletteProfile::Terminal] {
        app.config.palette = profile;
        for layout in [PanelLayout::Brief, PanelLayout::Details] {
            app.config.layouts = [layout; 2];
            for (w, h) in [(80, 25), (132, 40), (80, 25)] {
                key(&mut app, KeyCode::Home, KeyModifiers::NONE);
                let mut terminal = Terminal::new(TestBackend::new(w, h))?;
                terminal.draw(|f| ui::draw(f, &mut app))?;
                let up = region(&app, 0, ScrollAction::Up);
                let down = region(&app, 0, ScrollAction::Down);
                let track = region(&app, 0, ScrollAction::Track);
                let b = terminal.backend().buffer();
                assert_eq!(b[(up.x, up.y)].symbol(), "▲");
                assert_eq!(b[(down.x, down.y)].symbol(), "▼");
                assert_eq!(b[(track.x, track.y)].symbol(), "█");
                assert_eq!(b[(track.x, track.y)].fg, profile.color(0));
                let background = if profile == PaletteProfile::ClassicDn {
                    15
                } else {
                    7
                };
                assert_eq!(b[(track.x, track.y)].bg, profile.color(background));
                for panel in 0..2 {
                    let track = region(&app, panel, ScrollAction::Track);
                    let up = region(&app, panel, ScrollAction::Up);
                    let down = region(&app, panel, ScrollAction::Down);
                    let foreground = if profile == PaletteProfile::Terminal && panel != app.active {
                        8
                    } else {
                        0
                    };
                    for (x, y, glyph) in [
                        (up.x, up.y, "▲"),
                        (down.x, down.y, "▼"),
                        (track.x, track.y, "█"),
                        (track.x, track.y + 1, "░"),
                    ] {
                        assert_eq!(b[(x, y)].symbol(), glyph);
                        assert_eq!(b[(x, y)].fg, profile.color(foreground));
                        assert_eq!(b[(x, y)].bg, profile.color(background));
                    }
                }
                let slots: Vec<_> = app
                    .mouse
                    .regions
                    .iter()
                    .filter_map(|(r, t)| matches!(t, Target::Entry(0, _)).then_some(*r))
                    .collect();
                key(&mut app, KeyCode::Down, KeyModifiers::NONE);
                terminal.draw(|f| ui::draw(f, &mut app))?;
                assert_eq!(app.panels[0].offset, 0);
                let after: Vec<_> = app
                    .mouse
                    .regions
                    .iter()
                    .filter_map(|(r, t)| matches!(t, Target::Entry(0, _)).then_some(*r))
                    .collect();
                assert_eq!(slots, after);
                key(&mut app, KeyCode::End, KeyModifiers::NONE);
                terminal.draw(|f| ui::draw(f, &mut app))?;
                assert_eq!(region(&app, 0, ScrollAction::Thumb).y, track.bottom() - 1);
                assert_eq!(region(&app, 0, ScrollAction::Track), track);
                for (r, t) in &app.mouse.regions {
                    if matches!(t, Target::Entry(0, _)) {
                        assert!(r.right() <= track.x);
                    }
                }
            }
        }
    }
    Ok(())
}
#[test]
fn arrows_page_clicks_drag_wheel_and_selection_preservation() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = populated(&temp)?;
    draw(&mut app, 80, 25)?;
    app.panels[0].cursor = 1;
    app.panels[0].toggle_selection();
    let marks = app.panels[0].selected.clone();
    key(&mut app, KeyCode::Home, KeyModifiers::NONE);
    draw(&mut app, 80, 25)?;
    click(&mut app, Target::Scrollbar(0, ScrollAction::Down));
    assert_eq!(app.panels[0].cursor, 1);
    click(&mut app, Target::Scrollbar(0, ScrollAction::Up));
    assert_eq!(app.panels[0].cursor, 0);
    let track = region(&app, 0, ScrollAction::Track);
    mouse_event(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        track.x,
        track.bottom() - 1,
    );
    assert_eq!(app.panels[0].cursor, app.panels[0].viewport_rows * 3);
    draw(&mut app, 80, 25)?;
    let thumb = region(&app, 0, ScrollAction::Thumb);
    mouse_event(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        thumb.x,
        thumb.y,
    );
    assert!(app.mouse.scrollbar_drag.is_some());
    mouse_event(
        &mut app,
        MouseEventKind::Drag(MouseButton::Left),
        0,
        u16::MAX,
    );
    assert_eq!(app.panels[0].cursor, app.panels[0].entries.len() - 1);
    draw(&mut app, 80, 25)?;
    mouse_event(&mut app, MouseEventKind::Drag(MouseButton::Left), 0, 0);
    assert_eq!(app.panels[0].cursor, 0);
    mouse_event(&mut app, MouseEventKind::Up(MouseButton::Left), 0, 0);
    assert!(app.mouse.scrollbar_drag.is_none());
    mouse_event(
        &mut app,
        MouseEventKind::Drag(MouseButton::Left),
        0,
        u16::MAX,
    );
    assert_eq!(app.panels[0].cursor, 0);
    draw(&mut app, 80, 25)?;
    mouse_event(&mut app, MouseEventKind::ScrollDown, track.x, track.y);
    assert_eq!(app.panels[0].cursor, 3);
    let other = region(&app, 1, ScrollAction::Down);
    mouse_event(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        other.x,
        other.y,
    );
    assert_eq!(app.active, 1);
    assert_eq!(app.panels[1].cursor, 1);
    assert_eq!(app.panels[0].selected, marks);
    assert!(app.panels.iter().all(|p| !p.loading));
    Ok(())
}
#[test]
fn dragging_cancels_on_key_resize_and_modal_controls_stay_on_top() -> io::Result<()> {
    let temp = Temp::new()?;
    let mut app = populated(&temp)?;
    draw(&mut app, 80, 25)?;
    click(&mut app, Target::Scrollbar(0, ScrollAction::Thumb));
    key(&mut app, KeyCode::Down, KeyModifiers::NONE);
    assert!(app.mouse.scrollbar_drag.is_none());
    draw(&mut app, 80, 25)?;
    click(&mut app, Target::Scrollbar(0, ScrollAction::Thumb));
    draw(&mut app, 120, 40)?;
    assert!(app.mouse.scrollbar_drag.is_none());
    app.prompt_goto();
    draw(&mut app, 80, 25)?;
    assert!(app
        .mouse
        .regions
        .iter()
        .all(|(_, t)| !matches!(t, Target::Scrollbar(..))));
    let cursor = app.panels[0].cursor;
    mouse_event(&mut app, MouseEventKind::Down(MouseButton::Left), 38, 3);
    assert_eq!(app.panels[0].cursor, cursor);
    app.modals.set_prompt(None);
    app.screensaver.start(Instant::now());
    draw(&mut app, 80, 25)?;
    mouse_event(&mut app, MouseEventKind::Up(MouseButton::Left), 38, 3);
    assert!(app.screensaver.active);
    mouse_event(&mut app, MouseEventKind::Down(MouseButton::Left), 38, 3);
    assert!(!app.screensaver.active);
    assert_eq!(app.panels[0].cursor, cursor);
    Ok(())
}
