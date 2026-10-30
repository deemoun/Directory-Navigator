//! Host contract tests use a tool that is not known to any central enum/router.
use super::*;
use crate::{
    app::App,
    focus::Focus,
    input, menu,
    painting::Paragraph,
    testing::{backend::TestBackend, Buffer, Terminal},
    turbo::dialog_spec::{Control, ControlKind},
};
use crossterm::event::{KeyCode as K, KeyModifiers as M, MouseButton, MouseEventKind};
use std::{
    fs, io,
    sync::atomic::{AtomicUsize, Ordering},
};
const PROBE: PluginId = PluginId("test.counter");
struct Probe {
    kind: Presentation,
    value: usize,
    focus: usize,
    closed: usize,
    active: bool,
}
impl ToolPlugin for Probe {
    fn definition(&self) -> Definition {
        Definition {
            id: PROBE,
            title: "Probe tool",
            menu_label: "Probe tool",
            shortcut: "",
            menu: Some(menu::RootId::Utilities),
            presentation: self.kind,
        }
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn open(&mut self, _: &Context) {
        self.focus = 0;
    }
    fn event(&mut self, e: Event<'_>, _ctx: &Context) -> Vec<Effect> {
        match e {
            Event::Key(k) if k.code == K::Char('+') => self.value += 1,
            Event::Paste(s) => self.value += s.chars().count(),
            Event::Action(ActionId(1)) => return vec![Effect::CopyText(self.value.to_string())],
            Event::Action(ActionId(2)) => return vec![Effect::Close],
            Event::Action(ActionId(3)) => {
                return vec![Effect::Help {
                    title: " Probe Help ",
                    text: "Child owned by the tool".into(),
                }]
            }
            Event::Mouse(e, Some(a)) if e.kind == MouseEventKind::Down(MouseButton::Left) => {
                return self.event(Event::Action(a), _ctx)
            }
            Event::Focus(f) => self.focus = f,
            Event::Activity(a) => {
                self.active = a;
                if !a {
                    return vec![
                        Effect::CopyText("background".into()),
                        Effect::Help {
                            title: " Hidden Help ",
                            text: "Must not steal foreground".into(),
                        },
                    ];
                }
            }
            _ => {}
        }
        Vec::new()
    }
    fn controls(&self, c: &Context) -> DialogSpec<'_> {
        let r = crate::turbo::dialog_spec::centered(c.area, 44, 8);
        let mut s = c.dialog(r, self.focus, "Probe tool", 1, 2);
        s.caption(
            Paragraph::new(self.value.to_string()).style(c.palette.dos(0x70)),
            Rect::new(r.x + 2, r.y + 2, 10, 1),
        );
        for (i, a, label) in [(0, 1, "Copy"), (1, 2, "Close"), (2, 3, "Help")] {
            s.control(Control::new(
                i,
                Rect::new(r.x + 2 + i as u16 * 13, r.bottom() - 3, 12, 1),
                ControlKind::Button(label.into()),
                c.target(a),
            ));
        }
        s
    }
    fn close(&mut self) {
        self.closed += 1;
    }
}
struct Temp(PathBuf);
impl Temp {
    fn app() -> io::Result<(Self, App)> {
        static N: AtomicUsize = AtomicUsize::new(0);
        let t = Self(std::env::temp_dir().join(format!(
            "dn-plugins-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        )));
        fs::create_dir(&t.0)?;
        fs::write(t.0.join("keep.txt"), "unchanged")?;
        let mut a = App::new(t.0.clone())?;
        let end = Instant::now() + Duration::from_secs(5);
        while a.panels.iter().any(|p| p.loading) {
            a.poll();
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(1));
        }
        a.config.save_on_exit = false;
        a.config_store = crate::config::Store::new(Some(t.0.join("config.conf")));
        Ok((t, a))
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn register(a: &mut App, kind: Presentation) {
    a.desktop
        .plugins
        .register(Box::new(Probe {
            kind,
            value: 0,
            focus: 0,
            closed: 0,
            active: false,
        }))
        .unwrap();
}
fn probe(a: &App) -> &Probe {
    a.desktop.plugins.typed(PROBE)
}
fn key(a: &mut App, k: K) {
    input::handle(a, KeyEvent::new(k, M::NONE));
}
fn draw(a: &mut App) -> io::Result<Buffer> {
    let mut t = Terminal::new(TestBackend::new(80, 25))?;
    t.draw(|f| crate::ui::draw(f, a))?;
    Ok(t.backend().buffer().clone())
}
#[test]
fn registered_tool_uses_generic_menu_input_mouse_and_owned_lifecycle() -> io::Result<()> {
    let (t, mut a) = Temp::app()?;
    register(&mut a, Presentation::Dialog);
    assert!(a
        .desktop
        .plugins
        .register(Box::new(Probe {
            kind: Presentation::Dialog,
            value: 0,
            focus: 0,
            closed: 0,
            active: false
        }))
        .is_err());
    menu::open(&mut a, 2);
    let row = menu::level_items(&a, 0)
        .iter()
        .position(|i| i.action == menu::Action::Plugin(PROBE))
        .unwrap();
    menu::activate(&mut a, 0, row);
    assert_eq!(crate::focus::current(&a), Focus::Plugin);
    let old = host::active(&a).unwrap();
    key(&mut a, K::Char('+'));
    input::paste(&mut a, "界e\u{301}");
    assert_eq!(probe(&a).value, 4);
    key(&mut a, K::Enter);
    assert_eq!(a.editor_clipboard, "4");
    key(&mut a, K::Tab);
    assert_eq!(probe(&a).focus, 1);
    input::handle(&mut a, KeyEvent::new(K::Tab, M::SHIFT));
    assert_eq!(probe(&a).focus, 0);
    a.output_visible = true;
    host::dispatch(&mut a, old, Event::Action(ActionId(2)));
    assert!(a.modals.plugin_dialog().is_some());
    a.output_visible = false;
    host::dispatch(&mut a, old, Event::Action(ActionId(3)));
    assert_eq!(crate::focus::current(&a), Focus::Message);
    host::open(&mut a, CALCULATOR);
    assert_eq!(a.modals.plugin_dialog(), Some(PROBE));
    input::paste(&mut a, "blocked");
    host::dispatch(&mut a, old, Event::Action(ActionId(2)));
    assert_eq!(probe(&a).closed, 0);
    key(&mut a, K::Esc);
    assert_eq!(host::active(&a), Some(old));
    draw(&mut a)?;
    let r = a
        .mouse
        .regions
        .iter()
        .find(|(_, t)| *t == Target::Plugin(PROBE, ActionId(2)))
        .unwrap()
        .0;
    let snap = crate::layout::prepare_window(
        &mut a,
        Rect::new(7, 3, 80, 25),
        Rect::new(0, 0, 100, 35),
        Instant::now(),
    );
    let f = crate::ui::render(&a, &snap);
    let hits = f.hit_regions().to_vec();
    assert_eq!(
        Buffer::rendered(f),
        Buffer::rendered(crate::ui::render(&a, &snap))
    );
    assert!(hits
        .iter()
        .all(|(r, _)| r.x >= 7 && r.y >= 3 && r.right() <= 87 && r.bottom() <= 28));
    draw(&mut a)?; // Publish a regular frame before clicking.
    crate::mouse::handle(
        &mut a,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: r.x,
            row: r.y,
            modifiers: M::NONE,
        },
    );
    assert!(a.modals.plugin_dialog().is_none());
    assert_eq!(probe(&a).closed, 1);
    host::open(&mut a, PROBE);
    let next = host::active(&a).unwrap();
    assert_ne!(old, next);
    crate::mouse::handle(
        &mut a,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: r.x,
            row: r.y,
            modifiers: M::NONE,
        },
    );
    assert_eq!(host::active(&a), Some(next));
    host::dispatch(&mut a, old, Event::Action(ActionId(2)));
    assert_eq!(host::active(&a), Some(next));
    assert_eq!(probe(&a).value, 4);
    key(&mut a, K::Esc);
    assert_eq!(probe(&a).closed, 2);
    assert_eq!(fs::read_to_string(t.0.join("keep.txt"))?, "unchanged");
    Ok(())
}
#[test]
fn document_plugin_reuses_window_and_retired_owner_cannot_target_replacement() -> io::Result<()> {
    let (_t, mut a) = Temp::app()?;
    register(&mut a, Presentation::Document);
    host::open(&mut a, PROBE);
    let b = host::active(&a).unwrap();
    let id = a.desktop.active().id;
    key(&mut a, K::Char('+'));
    a.desktop.activate(WindowId::MANAGER);
    host::open(&mut a, PROBE);
    assert_eq!(a.desktop.active().id, id);
    assert_eq!(probe(&a).value, 1);
    let clipboard = a.editor_clipboard.clone();
    crate::windowing::open(&mut a);
    assert!(a.modals.window_list().unwrap().ids.contains(&id));
    draw(&mut a)?;
    assert_eq!(crate::focus::current(&a), Focus::WindowList);
    assert_eq!(a.editor_clipboard, clipboard);
    assert!(!probe(&a).active);
    a.modals.set_window_list(None);
    a.sync_modals();
    draw(&mut a)?;
    assert!(probe(&a).active);
    host::dispatch(&mut a, b, Event::Action(ActionId(3)));
    assert!(a.last_dialog().is_some());
    a.desktop.close(id);
    a.sync_modals();
    assert!(a.last_dialog().is_none());
    assert_eq!(probe(&a).closed, 1);
    host::open(&mut a, PROBE);
    assert_ne!(a.desktop.active().id, id);
    host::dispatch(&mut a, b, Event::Action(ActionId(2)));
    assert!(host::active(&a).is_some());
    Ok(())
}
#[test]
fn calculator_and_tetris_keep_independent_state_and_pause_without_catchup() -> io::Result<()> {
    let (_t, mut a) = Temp::app()?;
    host::open(&mut a, TETRIS);
    draw(&mut a)?;
    let game = a.desktop.active().id;
    let base = Instant::now();
    a.desktop.game_mut().unwrap().restart(base);
    let delay = a.desktop.game().unwrap().delay();
    let y = a.desktop.game().unwrap().piece.y;
    assert!(!host::poll(&mut a, base + delay - Duration::from_nanos(1)));
    assert_eq!(a.desktop.game().unwrap().piece.y, y);
    assert!(host::poll(&mut a, base + delay));
    assert_eq!(a.desktop.game().unwrap().piece.y, y + 1);
    host::open(&mut a, CALCULATOR);
    input::paste(&mut a, "6*7");
    key(&mut a, K::Enter);
    assert_eq!(
        a.desktop.plugins.calculator().result,
        Some(crate::utilities::calculator::Value::Integer(42))
    );
    let y = a.desktop.game().unwrap().piece.y;
    host::poll(&mut a, base + Duration::from_secs(60));
    assert_eq!(a.desktop.game().unwrap().piece.y, y);
    key(&mut a, K::Esc);
    host::update_activity(&mut a, base + Duration::from_secs(120));
    assert_eq!(a.desktop.active().id, game);
    host::poll(&mut a, base + Duration::from_secs(120) + delay);
    assert_eq!(a.desktop.game().unwrap().piece.y, y + 1);
    let small = crate::layout::prepare(&mut a, Rect::new(0, 0, 60, 15));
    let y = a.desktop.game().unwrap().piece.y;
    host::poll(&mut a, base + Duration::from_secs(180));
    assert_eq!(a.desktop.game().unwrap().piece.y, y);
    let _ = small;
    a.desktop.activate(WindowId::MANAGER);
    host::poll(&mut a, base + Duration::from_secs(240));
    host::open(&mut a, CALCULATOR);
    assert_eq!(
        a.desktop.plugins.calculator().result,
        Some(crate::utilities::calculator::Value::Integer(42))
    );
    Ok(())
}

#[test]
fn plugins_settings_lists_registered_tools_not_a_builtin_whitelist() -> io::Result<()> {
    let (_t, mut a) = Temp::app()?;
    register(&mut a, Presentation::Document);
    host::open(&mut a, PROBE);
    let id = a.desktop.active().id;
    a.desktop.activate(WindowId::MANAGER);
    a.open_settings(crate::settings::SettingsKind::Plugins);
    let settings = a.modals.settings().unwrap();
    assert_eq!(settings.control_count(), 4);
    let index = settings
        .plugins
        .iter()
        .position(|(id, _)| *id == PROBE)
        .unwrap();
    crate::settings::action(&mut a, Target::Setting(index));
    crate::settings::accept(&mut a);
    assert!(!a.config.plugin_enabled(PROBE));
    assert!(!a.desktop.contains(id));
    assert_eq!(probe(&a).closed, 1);
    host::open(&mut a, PROBE);
    assert_eq!(a.desktop.active().id, WindowId::MANAGER);
    menu::open(&mut a, 2);
    assert!(!menu::level_items(&a, 0)
        .iter()
        .any(|i| i.action == menu::Action::Plugin(PROBE)));
    menu::close(&mut a);
    a.open_settings(crate::settings::SettingsKind::Plugins);
    crate::settings::action(&mut a, Target::Setting(index));
    crate::settings::accept(&mut a);
    menu::open(&mut a, 2);
    assert_eq!(
        menu::level_items(&a, 0)
            .iter()
            .filter(|i| i.action == menu::Action::Plugin(PROBE))
            .count(),
        1
    );
    menu::close(&mut a);
    Ok(())
}

#[test]
fn long_plugin_settings_list_wraps_clips_and_rejects_scrolled_mouse_targets() -> io::Result<()> {
    let (_t, mut a) = Temp::app()?;
    a.open_settings(crate::settings::SettingsKind::Plugins);
    a.modals.settings_mut().unwrap().plugins = (0..24)
        .map(|i| {
            (
                PluginId(Box::leak(format!("test.{i:02}").into_boxed_str())),
                "Test tool",
            )
        })
        .collect();
    draw(&mut a)?;
    let stale = a
        .mouse
        .regions
        .iter()
        .find(|(_, t)| *t == Target::Setting(0))
        .unwrap()
        .0;
    let revision = a.layout_revision;
    for _ in 0..18 {
        key(&mut a, K::Tab);
    }
    assert_eq!(a.modals.settings().unwrap().cursor, 18);
    assert!(a.layout_revision > revision);
    crate::mouse::handle(
        &mut a,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: stale.x,
            row: stale.y,
            modifiers: M::NONE,
        },
    );
    assert!(a
        .modals
        .settings()
        .unwrap()
        .draft
        .disabled_plugins
        .is_empty());
    draw(&mut a)?;
    assert!(a
        .mouse
        .regions
        .iter()
        .any(|(_, t)| *t == Target::Setting(18)));
    assert!(!a
        .mouse
        .regions
        .iter()
        .any(|(_, t)| *t == Target::Setting(0)));
    key(&mut a, K::Char(' '));
    assert!(a
        .modals
        .settings()
        .unwrap()
        .draft
        .disabled_plugins
        .contains("test.18"));
    for _ in 0..8 {
        key(&mut a, K::Tab);
    }
    assert_eq!(a.modals.settings().unwrap().cursor, 0);
    key(&mut a, K::BackTab);
    assert_eq!(a.modals.settings().unwrap().cursor, 25);
    key(&mut a, K::Enter);
    assert!(a.config.disabled_plugins.is_empty());
    Ok(())
}
