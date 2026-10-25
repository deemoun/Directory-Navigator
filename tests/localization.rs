use chrono::{TimeZone, Utc};
use crossterm::event::{KeyCode, KeyModifiers};
use dn::testing::{backend::TestBackend, Terminal};
use dn::{
    app::App,
    clock_view::Clock,
    config::Config,
    editor, input,
    localization::{Catalog, Language},
    menu::{self, MenuId},
    mouse::Target,
    regional::{DateOrder, DateTimeFormat},
    settings::{self, Settings, SettingsKind},
    ui,
    viewer::Viewer,
};
use std::{
    collections::BTreeSet,
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
        let p = std::env::temp_dir().join(format!(
            "dn-locale-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p)?;
        Ok(Self(p))
    }
    fn app(&self) -> io::Result<App> {
        let mut a = App::new(self.0.clone())?;
        let end = Instant::now() + Duration::from_secs(5);
        while a.panels.iter().any(|p| p.loading) {
            a.poll();
            assert!(Instant::now() < end);
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
fn draw(a: &mut App, w: u16, h: u16) -> io::Result<String> {
    let mut t = Terminal::new(TestBackend::new(w, h))?;
    t.draw(|f| ui::draw(f, a))?;
    for (r, _) in &a.mouse.regions {
        assert!(r.right() <= w && r.bottom() <= h, "{r:?}");
    }
    Ok(t.backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect())
}
#[test]
fn catalogs_are_complete_pairs_safe_and_fallback_without_touching_content() {
    let en = Catalog::parse(include_str!("../locales/en.lang")).unwrap();
    let ru = Catalog::parse(include_str!("../locales/ru.lang")).unwrap();
    assert_eq!(
        en.strings.keys().collect::<Vec<_>>(),
        ru.strings.keys().collect::<Vec<_>>()
    );
    assert!(en.strings.len() > 290);
    for (k, v) in &en.strings {
        assert_eq!(k, v);
    }
    assert_eq!(Language::Russian.text("Name"), "Имя");
    for s in ["/Name/Size", "user supplied text", "Name\nSize"] {
        assert_eq!(Language::Russian.message(s), s);
    }
    assert_eq!(Language::parse("fr"), Language::English);
    for bad in [
        "missing tab",
        "key\t",
        "\tvalue",
        "k\tv\nk\tw",
        "k\t\\x",
        "k\t\x1b",
    ] {
        assert!(Catalog::parse(bad).is_err(), "{bad:?}");
    }
    assert!(Catalog::parse(&"a".repeat(1024 * 1024 + 1)).is_err());
    assert_eq!(
        Catalog::parse("k\tline\\nnext\\tend\\\\\n")
            .unwrap()
            .strings["k"],
        "line\nnext\tend\\"
    );
    assert_eq!(
        [1, 2, 5, 11, 21, 22, 25].map(|n| Language::Russian.files(n)),
        [
            "файл",
            "файла",
            "файлов",
            "файлов",
            "файл",
            "файла",
            "файлов"
        ]
    );
}
fn unique<'a>(labels: impl IntoIterator<Item = &'a str>) {
    let mut found = BTreeSet::new();
    for label in labels {
        if let Some(c) = menu::mnemonic(Language::Russian.text(label)) {
            assert!(found.insert(c), "Duplicate {c}: {label}");
        }
    }
}
#[test]
fn translated_mnemonics_are_unique_and_keep_english_aliases() {
    unique(menu::LABELS);
    for root in 0..menu::LABELS.len() {
        unique(menu::items(root).iter().map(|i| i.label));
    }
    for m in [
        MenuId::Associations,
        MenuId::CopyNames,
        MenuId::View,
        MenuId::Sort,
        MenuId::Layout,
        MenuId::Configuration,
        MenuId::FileManager,
    ] {
        unique(menu::submenu_items(m).iter().map(|i| i.label));
    }
    unique(editor::menus::LABELS);
    for root in 0..7 {
        unique(editor::menus::items(root).iter().map(|i| i.label));
    }
    assert!(Language::Russian.matches("~F~ile", 'f'));
    assert!(Language::Russian.matches("~F~ile", 'Ф'));
}
#[test]
fn formats_cover_orders_leap_dates_midnight_noon_and_offsets() {
    let t = Utc.with_ymd_and_hms(2024, 2, 29, 0, 5, 9).unwrap();
    let mut f = DateTimeFormat::default();
    assert_eq!(f.stamp(&t, false, true), "2024-02-29 00:05:09");
    f.date_order = DateOrder::Dmy;
    f.date_separator = '.';
    f.time_separator = '.';
    f.twelve_hour = true;
    assert_eq!(f.stamp(&t, false, true), "29.02.2024 12.05.09 AM");
    assert_eq!(f.stamp(&t, true, false), "29.02.24 12.05 AM");
    assert_eq!(
        f.time(&Utc.with_ymd_and_hms(2024, 2, 29, 12, 5, 9).unwrap(), true),
        "12.05.09 PM"
    );
    assert_eq!(
        f.information("2024-02-29 23:05:09 +01:30"),
        "29.02.2024 11.05.09 PM +01:30"
    );
    assert_eq!(
        f.archive("2024-02-29 23:05:09 UTC"),
        "29.02.2024 11.05.09 PM UTC"
    );
    assert_eq!(f.archive("Unavailable"), "Unavailable");
    f.date_order = DateOrder::Mdy;
    f.date_separator = '/';
    assert_eq!(f.date(&t, false), "02/29/2024");
    assert_eq!(f.clock(&t).len(), usize::from(f.clock_width()));
    let mut clock = Clock::default();
    assert!(clock.update(t));
    assert!(!clock.update(t));
    assert!(clock.update_with(t, f));
    assert!(!clock.update_with(t, f));
    assert!(clock.update_with(t - chrono::Duration::seconds(1), f));
}
#[test]
fn new_and_invalid_config_defaults_and_regional_roundtrip() -> io::Result<()> {
    let t = Temp::new()?;
    let p = t.0.join("config.conf");
    fs::write(&p, "config_version=2\n[file_manager]\nhidden=true\n")?;
    let c = Config::read(&p)?;
    assert_eq!(c.language, Language::English);
    assert_eq!(c.datetime, DateTimeFormat::default());
    fs::write(&p,"config_version=2\n[regional]\nlanguage=\"unknown\"\ndate_order=\"unknown\"\ndate_separator=\"x\"\ntime_format=\"invalid\"\ntime_separator=\"x\"\n")?;
    assert!(Config::read(&p).is_err());
    fs::write(&p, "config_version=2\n")?;
    let mut store = dn::config::Store::new(Some(p));
    let mut c = store.load()?;
    c.language = Language::Russian;
    c.datetime = DateTimeFormat {
        date_order: DateOrder::Mdy,
        date_separator: '/',
        twelve_hour: true,
        time_separator: '.',
    };
    store.save(&c, true)?;
    assert_eq!(store.load()?, c);
    Ok(())
}
#[test]
fn regional_apply_cancel_navigation_and_mouse_geometry() -> io::Result<()> {
    let t = Temp::new()?;
    fs::write(t.0.join("Name"), "Name\nSize\nтекст")?;
    let mut a = t.app()?;
    let originals = a
        .panels
        .each_ref()
        .map(|p| p.entries.iter().map(|e| e.name.clone()).collect::<Vec<_>>());
    a.modals
        .set_settings(Some(Settings::new(SettingsKind::Regional, &a.config)));
    for (i, _) in (0..5).enumerate() {
        a.modals.settings_mut().unwrap().regional_choice(i, false);
    }
    settings::key(&mut a, KeyCode::Esc);
    assert_eq!(a.config.language, Language::English);
    a.modals
        .set_settings(Some(Settings::new(SettingsKind::Regional, &a.config)));
    for i in 0..5 {
        a.modals.settings_mut().unwrap().regional_choice(i, false);
    }
    for (w, h) in [(80, 25), (120, 40)] {
        let s = draw(&mut a, w, h)?;
        assert!(s.contains("Русский"));
        for i in 0..5 {
            assert!(a
                .mouse
                .regions
                .iter()
                .any(|(_, t)| *t == Target::RegionalChoice(i)));
        }
    }
    settings::accept(&mut a);
    assert_eq!(a.config.language, Language::Russian);
    assert!(a.config.datetime.twelve_hour);
    assert_eq!(
        a.panels
            .each_ref()
            .map(|p| p.entries.iter().map(|e| e.name.clone()).collect::<Vec<_>>()),
        originals
    );
    assert!(a.panels.iter().all(|p| !p.loading));
    assert!(menu::open_mnemonic(&mut a, 'н'));
    assert_eq!(a.modals.menu().unwrap().0, 5);
    menu::close(&mut a);
    assert!(menu::open_mnemonic(&mut a, 'o'));
    menu::close(&mut a);
    for (w, h) in [(80, 25), (120, 40), (80, 25)] {
        let s = draw(&mut a, w, h)?;
        assert!(s.contains("Имя"));
        let clock_start = w - a.config.datetime.clock_width();
        for (r, t) in &a.mouse.regions {
            if matches!(t, Target::Menu(_)) {
                assert!(r.right() <= clock_start);
            }
        }
    }
    // Ordinary viewer content remains byte-for-byte untranslated.
    a.show_viewer(Viewer::from_contents(
        t.0.join("Name"),
        dn::viewer::from_bytes(b"Name\nSize".to_vec(), dn::viewer::ViewMode::Text),
    ));
    assert!(draw(&mut a, 80, 25)?.contains("Name"));
    a.desktop.close_viewer();
    input::handle(
        &mut a,
        crossterm::event::KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE),
    );
    let help = draw(&mut a, 80, 25)?;
    assert!(help.contains("Помощь"));
    Ok(())
}

#[test]
fn russian_menus_editor_modals_and_metadata_fit_without_translating_paths() -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("Name");
    fs::write(&path, "Name\nSize\nФайл\n")?;
    let mut a = t.app()?;
    let mut c = a.config.clone();
    c.language = Language::Russian;
    c.datetime.twelve_hour = true;
    c.datetime.date_order = DateOrder::Dmy;
    c.datetime.date_separator = '.';
    a.apply_config(c);
    for (w, h) in [(80, 25), (120, 40), (80, 25)] {
        for root in 0..menu::LABELS.len() {
            menu::open(&mut a, root);
            draw(&mut a, w, h)?;
        }
        menu::close(&mut a);
        for id in [
            MenuId::Associations,
            MenuId::CopyNames,
            MenuId::View,
            MenuId::Sort,
            MenuId::Layout,
            MenuId::Configuration,
            MenuId::FileManager,
        ] {
            menu::open(&mut a, 5);
            a.modals.submenus_mut().push((id, 0));
            draw(&mut a, w, h)?;
            menu::close(&mut a);
        }
    }
    editor::open_path(&mut a, path.clone());
    let end = Instant::now() + Duration::from_secs(5);
    while a.desktop.active_editor().is_some_and(|e| e.busy()) {
        a.poll();
        assert!(Instant::now() < end);
        thread::sleep(Duration::from_millis(1));
    }
    assert!(a.desktop.active_editor().is_some());
    let original = a
        .desktop
        .active_editor()
        .unwrap()
        .buffer
        .as_ref()
        .unwrap()
        .text
        .clone();
    for (w, h) in [(80, 25), (120, 40), (80, 25)] {
        let s = draw(&mut a, w, h)?;
        assert!(s.contains("Редактор"));
        assert!(s.contains("Name"));
        for (r, target) in &a.mouse.regions {
            if matches!(target, Target::Editor(editor::EditorAction::MenuRoot(_))) {
                assert!(r.width > 3);
                assert!(r.right() <= w - 13);
            }
        }
        for root in [0, 1, 2, 4, 5, 6] {
            editor::menus::open(&mut a, root);
            draw(&mut a, w, h)?;
            editor::menus::key(&mut a, KeyCode::Esc);
        }
    }
    editor::action(&mut a, editor::EditorAction::Replace);
    let s = draw(&mut a, 80, 25)?;
    assert!(s.contains("Направление"));
    assert!(s.contains("Найти:"));
    editor::action(&mut a, editor::EditorAction::Cancel);
    assert_eq!(
        a.desktop
            .active_editor()
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .text,
        original
    );
    let info = dn::file_info::inspect(&path)?;
    let f = a.config.datetime;
    let modified = info
        .lines
        .iter()
        .find(|s| s.starts_with("Modified (local):"))
        .unwrap();
    let translated = Language::Russian.field(modified, f);
    assert!(translated.starts_with("Изменён (местное):"));
    assert!(translated.contains(" AM ") || translated.contains(" PM "));
    assert_eq!(
        Language::Russian.field("Full path: /Name/Modified", f),
        "Полный путь: /Name/Modified"
    );
    assert_eq!(
        Language::Russian.field("Entry type: Regular file", f),
        "Тип объекта: Обычный файл"
    );
    Ok(())
}
