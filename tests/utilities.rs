use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use dn::testing::{backend::TestBackend, Terminal};
use dn::{
    app::App,
    input, menu,
    mouse::{self, Target},
    ui,
    utilities::{
        self,
        calculator::{evaluate, Value},
        characters::{cp437, Chart, Page},
        information::{self, Information},
        Action, Kind,
    },
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
            "dn-utilities-{}-{}",
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
#[test]
fn calculator_precedence_functions_and_exact_integers() {
    for (text, n) in [
        ("2+3*4", 14),
        ("(2+3)*4", 20),
        ("2^3^2", 512),
        ("-2^2", -4),
        ("(-2)^2", 4),
        ("0xFF+0b10+0o7", 264),
        ("abs(-17)", 17),
        ("9223372036854775807", i64::MAX),
        ("-9223372036854775808", i64::MIN),
        ("-0x8000000000000000", i64::MIN),
        ("9007199254740993+2", 9007199254740995),
        ("9/3", 3),
        ("-17%5", -2),
    ] {
        assert_eq!(evaluate(text), Ok(Value::Integer(n)), "{text}");
    }
    for (text, n) in [
        ("sqrt(81)", 9.0),
        ("7/2", 3.5),
        ("2^-2", 0.25),
        ("1e2+2.5", 102.5),
        ("abs(-0.5)", 0.5),
    ] {
        assert_eq!(evaluate(text), Ok(Value::Real(n)), "{text}");
    }
}
#[test]
fn calculator_errors_limits_and_base_conversions() {
    for text in [
        "",
        "1+",
        "2 3",
        "1/0",
        "1%0",
        "sqrt(-1)",
        "0^-1",
        "(-1)^0.5",
        "1e999",
        "1e308*1e308",
        "9223372036854775808",
        "9223372036854775807+1",
        "-9223372036854775808/-1",
        "abs(-9223372036854775808)",
        "2^63",
        "2^999999999999",
        "0xZZ",
        "0b2",
        "0o8",
        "0x",
        "sin(1)",
        "1..2",
        "(1",
        "😀",
    ] {
        assert!(evaluate(text).is_err(), "{text}");
    }
    assert!(evaluate(&format!("{}1{}", "(".repeat(80), ")".repeat(80))).is_err());
    assert!(evaluate(&"1".repeat(3000)).is_err());
    assert_eq!(
        Value::Integer(i64::MIN).representations()[1],
        Ok("-0x8000000000000000".into())
    );
    assert_eq!(Value::Integer(-5).representations()[2], Ok("-0b101".into()));
    assert_eq!(Value::Real(4.0).representations()[3], Ok("0o4".into()));
    assert!(
        Value::Real(1e308).representations()[0]
            .as_ref()
            .unwrap()
            .len()
            < 40
    );
    assert!(Value::Real(3.5).representations()[1].is_err());
    assert!(Value::Real(9223372036854775808.0).representations()[1].is_err());
}
#[test]
fn calculator_history_bounded_unique_and_draft_recall() {
    let mut c = utilities::calculator::Calculator::default();
    for n in 0..105 {
        c.expression = n.to_string();
        c.evaluate();
    }
    assert_eq!(c.history.len(), 100);
    c.expression = "44".into();
    c.evaluate();
    assert_eq!(c.history.len(), 100);
    assert_eq!(c.history[0], "44");
    c.expression = "draft".into();
    c.recall(false);
    assert_eq!(c.expression, "draft");
    c.recall(true);
    assert_eq!(c.expression, "44");
    c.recall(true);
    assert_eq!(c.expression, "104");
    c.recall(false);
    c.recall(false);
    assert_eq!(c.expression, "draft");
    let before = c.history.clone();
    c.expression = "bad".into();
    c.evaluate();
    assert_eq!(c.history, before);
    assert!(c.copy_text().is_err());
}
#[test]
fn character_mapping_all_bytes_reference_fixture_and_controls() {
    let reference = include_str!("fixtures/cp437-high.txt")
        .lines()
        .map(|line| u32::from_str_radix(line, 16).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(reference.len(), 128);
    for b in 128..=255u8 {
        assert_eq!(cp437(b) as u32, reference[(b - 128) as usize], "byte {b}");
    }
    let mut chart = Chart::default();
    for byte in 0..=127 {
        chart.select(byte);
        assert_eq!(chart.mapped(byte), char::from(byte));
        assert_eq!(chart.copy_text().is_some(), (32..127).contains(&byte));
    }
    chart.page = Page::Cp437;
    for byte in 1..=255 {
        chart.select(byte);
        assert!(chart.copy_text().is_some());
        assert!(!cp437(byte).is_control());
    }
    for (b, c) in [
        (1, '☺'),
        (3, '♥'),
        (16, '►'),
        (24, '↑'),
        (31, '▼'),
        (127, '⌂'),
        (179, '│'),
        (219, '█'),
        (255, '\u{a0}'),
    ] {
        assert_eq!(cp437(b), c);
    }
    chart.select(0);
    assert!(chart.copy_text().is_none());
    chart.select(179);
    assert!(chart.details().contains("U+2502"));
    assert!(chart.details().contains("E2 94 82"));
}
#[test]
fn calculator_mouse_keyboard_paste_copy_reopen_and_editor_clipboard() -> io::Result<()> {
    let t = Temp::new()?;
    let mut app = t.app()?;
    draw(&mut app, 80, 25)?;
    key(&mut app, KeyCode::Char('k'), KeyModifiers::CONTROL);
    assert_eq!(app.utilities.dialog, Some(Kind::Calculator));
    app.utilities.paste("(0x20+1)*2");
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(app.utilities.calculator.result, Some(Value::Integer(66)));
    let screen = draw(&mut app, 80, 25)?;
    assert!(screen.contains("HEX"));
    assert!(screen.contains("0x42"));
    click(&mut app, Target::Utility(Action::Base(1)));
    click(&mut app, Target::Utility(Action::Copy));
    assert_eq!(app.editor_clipboard, "0x42");
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    key(&mut app, KeyCode::Char('k'), KeyModifiers::CONTROL);
    assert_eq!(app.utilities.calculator.result, Some(Value::Integer(66)));
    draw(&mut app, 120, 40)?;
    click(&mut app, Target::Utility(Action::Field));
    key(&mut app, KeyCode::Char('+'), KeyModifiers::NONE);
    assert!(app.utilities.calculator.result.is_none());
    key(&mut app, KeyCode::Up, KeyModifiers::NONE);
    assert_eq!(app.utilities.calculator.expression, "(0x20+1)*2");
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    key(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert_eq!(app.editor_clipboard, "0x42");
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    dn::editor::open(&mut app, true);
    key(&mut app, KeyCode::Char('v'), KeyModifiers::CONTROL);
    assert_eq!(
        app.desktop.editor().unwrap().buffer.as_ref().unwrap().text,
        "0x42"
    );
    Ok(())
}
#[test]
fn character_grid_mouse_navigation_pages_copy_and_resize() -> io::Result<()> {
    let t = Temp::new()?;
    let mut app = t.app()?;
    key(&mut app, KeyCode::Char('b'), KeyModifiers::CONTROL);
    let screen = draw(&mut app, 80, 25)?;
    assert!(screen.contains("Copy disabled"));
    assert!(!app
        .mouse
        .regions
        .iter()
        .any(|(_, t)| *t == Target::Utility(Action::Copy)));
    click(&mut app, Target::Utility(Action::Cell(65)));
    draw(&mut app, 80, 25)?;
    click(&mut app, Target::Utility(Action::Copy));
    assert_eq!(app.editor_clipboard, "A");
    click(&mut app, Target::Utility(Action::Page(Page::Cp437)));
    draw(&mut app, 80, 25)?;
    click(&mut app, Target::Utility(Action::Cell(179)));
    draw(&mut app, 120, 40)?;
    click(&mut app, Target::Utility(Action::Copy));
    assert_eq!(app.editor_clipboard, "│");
    key(&mut app, KeyCode::Right, KeyModifiers::NONE);
    assert_eq!(app.utilities.chart.selected(), 180);
    key(&mut app, KeyCode::Home, KeyModifiers::NONE);
    assert_eq!(app.utilities.chart.selected(), 0);
    key(&mut app, KeyCode::End, KeyModifiers::NONE);
    assert_eq!(app.utilities.chart.selected(), 255);
    utilities::action(&mut app, Action::Page(Page::Ascii));
    assert_eq!(app.utilities.chart.selected(), 65);
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    utilities::open(&mut app, Kind::Characters);
    assert_eq!(app.utilities.chart.selected(), 65);
    Ok(())
}
#[test]
fn linux_memory_parser_units_errors_and_overflow() {
    let text = "MemTotal: 1000 kB\nMemFree: 12 kB\nMemAvailable: 200 kB\nSwapTotal: 0 kB\n";
    assert_eq!(information::kib_field(text, "MemTotal"), Ok(1024000));
    assert_eq!(information::kib_field(text, "MemAvailable"), Ok(204800));
    assert_eq!(information::kib_field(text, "SwapTotal"), Ok(0));
    for text in [
        "",
        "Key: -1 kB",
        "Key: 1 MB",
        "Key: 1",
        "Key: 18446744073709551615 kB",
        "Key: 1 kB extra",
    ] {
        assert!(information::kib_field(text, "Key").is_err());
    }
}
#[test]
fn native_information_refresh_close_and_stale_replies() -> io::Result<()> {
    let t = Temp::new()?;
    let mut app = t.app()?;
    draw(&mut app, 80, 25)?;
    utilities::open(&mut app, Kind::System);
    wait(&mut app, |a| {
        a.utilities
            .information
            .as_ref()
            .is_some_and(|i| !i.loading())
    });
    let info = app.utilities.information.as_ref().unwrap();
    assert!(info
        .fields
        .iter()
        .any(|(name, value)| name == "Terminal size" && value == "80 × 25"));
    assert!(info
        .fields
        .iter()
        .any(|(name, value)| name == "Active directory" && value == &t.0.to_string_lossy()));
    draw(&mut app, 120, 40)?;
    click(&mut app, Target::Utility(Action::Refresh));
    wait(&mut app, |a| {
        a.utilities
            .information
            .as_ref()
            .is_some_and(|i| !i.loading())
    });
    assert!(app
        .utilities
        .information
        .as_ref()
        .unwrap()
        .fields
        .iter()
        .any(|(k, v)| k == "Terminal size" && v == "120 × 40"));
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.utilities.information.is_none());
    key(&mut app, KeyCode::Char('y'), KeyModifiers::ALT);
    utilities::open(&mut app, Kind::System);
    wait(&mut app, |a| {
        a.utilities
            .information
            .as_ref()
            .is_some_and(|i| !i.loading())
    });
    assert_eq!(
        app.utilities.information.as_ref().unwrap().kind,
        information::Kind::System
    );
    utilities::open(&mut app, Kind::Memory);
    wait(&mut app, |a| {
        a.utilities
            .information
            .as_ref()
            .is_some_and(|i| !i.loading())
    });
    let fields = &app.utilities.information.as_ref().unwrap().fields;
    assert!(fields.iter().any(|(k, _)| k == "Physical RAM"));
    assert!(fields.iter().any(|(k, _)| k == "DN resident memory"));
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        for label in ["Physical RAM", "DN resident memory"] {
            assert!(
                fields
                    .iter()
                    .any(|(k, v)| k == label && v.contains("bytes")),
                "{fields:?}"
            );
        }
    }
    draw(&mut app, 80, 25)?;
    click(&mut app, Target::Utility(Action::Refresh));
    click(&mut app, Target::Utility(Action::Close));
    thread::sleep(Duration::from_millis(10));
    app.poll();
    assert!(app.utilities.information.is_none());
    assert!(app.utilities.dialog.is_none());
    Ok(())
}
#[test]
fn information_long_fields_scroll_sanitization_and_unavailable() -> io::Result<()> {
    let t = Temp::new()?;
    let mut app = t.app()?;
    utilities::open(&mut app, Kind::System);
    wait(&mut app, |a| {
        a.utilities
            .information
            .as_ref()
            .is_some_and(|i| !i.loading())
    });
    let info = app.utilities.information.as_mut().unwrap();
    info.fields = (0..40)
        .map(|n| {
            (
                format!("Field {n}"),
                format!("{}\x1b[31m", "长".repeat(100)),
            )
        })
        .collect();
    info.fields
        .push(("Missing".into(), "Unavailable: injected failure".into()));
    let screen = draw(&mut app, 80, 25)?;
    assert!(!screen.contains('\x1b'));
    key(&mut app, KeyCode::End, KeyModifiers::NONE);
    assert!(draw(&mut app, 80, 25)?.contains("Unavailable: injected failure"));
    key(&mut app, KeyCode::Right, KeyModifiers::NONE);
    assert_eq!(app.utilities.information.as_ref().unwrap().horizontal, 8);
    draw(&mut app, 130, 40)?;
    let mut info = Information::open(information::Kind::System, t.0.clone(), (80, 25))?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while info.loading() {
        info.poll();
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    Ok(())
}
#[test]
fn utility_menus_modal_precedence_editor_keys_and_small_terminal() -> io::Result<()> {
    let t = Temp::new()?;
    let mut app = t.app()?;
    for kind in [
        Kind::Calculator,
        Kind::Characters,
        Kind::System,
        Kind::Memory,
    ] {
        assert!(menu::items(2)
            .iter()
            .any(|i| i.action == menu::Action::Utility(kind)));
        menu::execute(&mut app, menu::Action::Utility(kind));
        draw(&mut app, 80, 25)?;
        draw(&mut app, 120, 40)?;
        draw(&mut app, 60, 15)?;
        utilities::action(&mut app, Action::Close);
    }
    key(&mut app, KeyCode::F(6), KeyModifiers::CONTROL);
    assert_eq!(app.panels[0].sort, dn::panel::SortMode::Size);
    utilities::open(&mut app, Kind::Calculator);
    app.error("top dialog");
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.utilities.dialog.is_some());
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.utilities.dialog.is_none());
    dn::editor::open(&mut app, true);
    key(&mut app, KeyCode::Char('b'), KeyModifiers::CONTROL);
    assert!(app.utilities.dialog.is_none());
    assert!(app.desktop.editor().is_some());
    Ok(())
}

#[test]
fn calculator_scientific_copy_keeps_exact_decimal_and_preserves_clipboard_on_error(
) -> io::Result<()> {
    for (value, expected) in [
        (Value::Integer(0), "0.00000000000000E+0000"),
        (Value::Integer(-66), "-6.60000000000000E+0001"),
        (Value::Real(0.125), "1.25000000000000E-0001"),
        (Value::Real(1e308), "1.00000000000000E+0308"),
    ] {
        assert_eq!(value.scientific(), expected);
    }
    let t = Temp::new()?;
    let mut app = t.app()?;
    utilities::open(&mut app, Kind::Calculator);
    app.utilities.paste("9007199254740993");
    utilities::action(&mut app, Action::Evaluate);
    utilities::action(&mut app, Action::Copy);
    assert_eq!(app.editor_clipboard, "9007199254740993");
    utilities::action(&mut app, Action::Base(4));
    utilities::action(&mut app, Action::Copy);
    assert_eq!(app.editor_clipboard, "9.00719925474099E+0015");
    app.utilities.calculator.focus = 0;
    app.utilities.calculator.expression.clear();
    app.utilities.calculator.cursor = 0;
    app.utilities.paste("1/0");
    utilities::action(&mut app, Action::Evaluate);
    key(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert_eq!(app.editor_clipboard, "9.00719925474099E+0015");
    assert!(app.utilities.calculator.error.is_some());
    Ok(())
}
#[test]
fn calculator_history_dropdown_recalls_without_evaluation_and_returns_to_draft() -> io::Result<()> {
    let t = Temp::new()?;
    let mut app = t.app()?;
    utilities::open(&mut app, Kind::Calculator);
    draw(&mut app, 80, 25)?;
    click(&mut app, Target::Utility(Action::CalculatorHistory));
    assert!(draw(&mut app, 80, 25)?.contains("No expression history"));
    app.utilities.paste("not inserted");
    assert!(app.utilities.calculator.expression.is_empty());
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(!app.utilities.calculator.history_open);
    assert_eq!(app.utilities.dialog, Some(Kind::Calculator));
    for expression in ["1+2", "sqrt(81)"] {
        let calc = &mut app.utilities.calculator;
        calc.expression = expression.into();
        calc.cursor = expression.len();
        utilities::action(&mut app, Action::Evaluate);
    }
    app.utilities.calculator.expression = "draft 界".into();
    app.utilities.calculator.cursor = "draft 界".len();
    app.utilities.calculator.result = None;
    utilities::action(&mut app, Action::CalculatorHistory);
    key(&mut app, KeyCode::End, KeyModifiers::NONE);
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(app.utilities.calculator.expression, "1+2");
    assert!(app.utilities.calculator.result.is_none());
    key(&mut app, KeyCode::Down, KeyModifiers::NONE);
    key(&mut app, KeyCode::Down, KeyModifiers::NONE);
    assert_eq!(app.utilities.calculator.expression, "draft 界");
    utilities::action(&mut app, Action::CalculatorHistory);
    draw(&mut app, 80, 25)?;
    click(&mut app, Target::Utility(Action::CalculatorRecall(0)));
    assert_eq!(app.utilities.calculator.expression, "sqrt(81)");
    assert!(app.utilities.calculator.result.is_none());
    assert!(!app.utilities.calculator.history_open);
    Ok(())
}
#[test]
fn calculator_classic_layout_results_help_mouse_focus_and_resize() -> io::Result<()> {
    use dn::{
        focus::{self, Focus},
        localization::Language,
        palette::PaletteProfile,
    };
    let t = Temp::new()?;
    for language in [Language::English, Language::Russian] {
        for profile in [PaletteProfile::ClassicDn, PaletteProfile::Terminal] {
            let mut app = t.app()?;
            app.config.language = language;
            app.config.palette = profile;
            utilities::open(&mut app, Kind::Calculator);
            app.utilities.paste("9223372036854775807");
            utilities::action(&mut app, Action::Evaluate);
            for (w, h) in [(80, 25), (120, 40), (80, 25)] {
                let mut terminal = Terminal::new(TestBackend::new(w, h))?;
                terminal.draw(|f| ui::draw(f, &mut app))?;
                let field = app
                    .mouse
                    .regions
                    .iter()
                    .find(|(_, t)| *t == Target::Utility(Action::Field))
                    .unwrap()
                    .0;
                let evaluate = app
                    .mouse
                    .regions
                    .iter()
                    .find(|(_, t)| *t == Target::Utility(Action::Evaluate))
                    .unwrap()
                    .0;
                let b = terminal.backend().buffer();
                let history = app
                    .mouse
                    .regions
                    .iter()
                    .find(|(_, t)| *t == Target::Utility(Action::CalculatorHistory))
                    .unwrap()
                    .0;
                assert_eq!(b[(history.x, history.y)].bg, profile.color(9));
                assert_eq!(field.width, 60);
                assert_eq!(evaluate.y - field.y, 12);
                assert_eq!(b[(evaluate.x + 1, evaluate.y + 1)].bg, profile.color(0));
                assert_eq!(
                    b[(field.x, field.y)].bg,
                    profile.color(if profile == PaletteProfile::ClassicDn {
                        0
                    } else {
                        1
                    })
                );
                assert!(app
                    .mouse
                    .regions
                    .iter()
                    .all(|(r, _)| r.right() <= w && r.bottom() <= h));
                for action in [
                    Action::Evaluate,
                    Action::Copy,
                    Action::Close,
                    Action::CalculatorHelp,
                    Action::CalculatorHistory,
                    Action::Base(4),
                ] {
                    assert!(app
                        .mouse
                        .regions
                        .iter()
                        .any(|(_, t)| *t == Target::Utility(action)));
                }
                let screen: String = b.content.iter().map(|c| c.symbol()).collect();
                assert!(screen.contains(language.text("Expression")));
                assert!(screen.contains(language.text("Copy as")));
                assert!(screen.contains("EXP"));
                assert!(
                    screen.contains('…'),
                    "long binary value should be visibly shortened"
                );
            }
            click(&mut app, Target::Utility(Action::Base(2)));
            click(&mut app, Target::Utility(Action::Copy));
            assert_eq!(app.editor_clipboard, format!("0b{}", "1".repeat(63)));
            key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
            assert_eq!(app.utilities.calculator.focus, 4);
            key(&mut app, KeyCode::Down, KeyModifiers::NONE);
            assert_eq!(app.utilities.calculator.base, 4);
            draw(&mut app, 80, 25)?;
            click(&mut app, Target::Utility(Action::CalculatorHelp));
            assert_eq!(focus::current(&app), Focus::Message);
            input::paste(&mut app, "blocked");
            assert_eq!(app.utilities.calculator.expression, "9223372036854775807");
            key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
            assert_eq!(focus::current(&app), Focus::Utility);
            key(&mut app, KeyCode::F(1), KeyModifiers::NONE);
            assert_eq!(focus::current(&app), Focus::Message);
            key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
            draw(&mut app, 60, 15)?;
            assert_eq!(app.utilities.dialog, Some(Kind::Calculator));
            draw(&mut app, 80, 25)?;
            key(&mut app, KeyCode::Char('e'), KeyModifiers::ALT);
            assert_eq!(
                app.utilities.calculator.result,
                Some(Value::Integer(i64::MAX))
            );
        }
    }
    Ok(())
}
