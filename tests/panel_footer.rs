use dn::{
    app::App,
    columns::ColumnPreset,
    localization::Language,
    palette::PaletteProfile,
    panel_layout::PanelLayout,
    ui,
    volume_space::{self, Space},
};
use ratatui::{backend::TestBackend, Terminal};
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
        let p = std::env::temp_dir().join(format!(
            "dn-footer-{}-{}",
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
fn draw(a: &mut App, w: u16, h: u16) -> io::Result<Terminal<TestBackend>> {
    let mut t = Terminal::new(TestBackend::new(w, h))?;
    t.draw(|f| ui::draw(f, a))?;
    for (r, _) in &a.mouse.regions {
        assert!(r.right() <= w && r.bottom() <= h);
    }
    Ok(t)
}
fn line(t: &Terminal<TestBackend>, y: u16, width: u16) -> String {
    (0..width)
        .map(|x| t.backend().buffer()[(x, y)].symbol())
        .collect()
}
#[test]
fn classic_footer_matches_selection_structure_and_color_roles() -> io::Result<()> {
    let t = Temp::new()?;
    fs::write(t.0.join("one"), vec![0; 4096])?;
    fs::write(t.0.join("two"), vec![0; 4096])?;
    let mut a = t.app()?;
    for p in &mut a.panels {
        p.space.result = Some(Ok(Space {
            available: 262_144_000,
            total: 500_000_000,
        }));
    }
    let screen = draw(&mut a, 80, 25)?;
    let summary = line(&screen, 20, 40);
    assert!(summary.contains("No files selected"));
    let start = summary.find("No files selected").unwrap() as u16;
    assert_eq!(
        screen.backend().buffer()[(start, 20)].fg,
        a.config.palette.dos(0x8e).fg.unwrap()
    );
    a.panels[0].select_visible();
    let before = a.panels[0].selected.clone();
    let screen = draw(&mut a, 80, 25)?;
    let summary = line(&screen, 20, 40);
    assert!(
        summary.contains("8,192 bytes in 2 selected files"),
        "{summary}"
    );
    let x = summary.find("8,192").unwrap() as u16;
    assert_eq!(
        screen.backend().buffer()[(x, 20)].fg,
        a.config.palette.dos(0x8f).fg.unwrap()
    );
    let x = summary.find("bytes").unwrap() as u16;
    assert_eq!(
        screen.backend().buffer()[(x, 20)].fg,
        a.config.palette.dos(0x8e).fg.unwrap()
    );
    let space = line(&screen, 21, 40);
    assert!(space.contains("free") && space.contains("total"), "{space}");
    assert!(space.contains("MiB"));
    assert_eq!(a.panels[0].selected, before);
    assert!(line(&screen, 23, 80).contains('>'));
    Ok(())
}
#[test]
fn folders_hidden_marks_unknown_metadata_and_locales_remain_truthful() -> io::Result<()> {
    let t = Temp::new()?;
    fs::create_dir(t.0.join("folder"))?;
    fs::write(t.0.join("folder/nested"), vec![0; 20000])?;
    fs::write(t.0.join("file"), vec![0; 123])?;
    let mut a = t.app()?;
    a.panels[0].select_visible();
    let screen = draw(&mut a, 80, 25)?;
    let summary = line(&screen, 20, 40);
    assert!(summary.contains("123 bytes"));
    assert!(summary.contains("1 file + 1 folder"), "{summary}");
    a.panels[0].set_filter("folder".into());
    let screen = draw(&mut a, 80, 25)?;
    assert!(line(&screen, 20, 40).contains("1 selected folder"));
    a.panels[0].set_filter("absent".into());
    let screen = draw(&mut a, 80, 25)?;
    assert!(line(&screen, 20, 40).contains("No visible entries selected"));
    assert!(line(&screen, 18, 40).contains("filtered marks"));
    a.panels[0].set_filter("file".into());
    a.panels[0]
        .entries
        .iter_mut()
        .find(|e| e.name == "file")
        .unwrap()
        .size = None;
    assert!(line(&draw(&mut a, 80, 25)?, 20, 40).contains("+ ?"));
    for language in [Language::English, Language::Russian] {
        a.config.language = language;
        for profile in [PaletteProfile::ClassicDn, PaletteProfile::Terminal] {
            a.config.palette = profile;
            for layout in [PanelLayout::Brief, PanelLayout::Details] {
                a.config.layouts = [layout; 2];
                a.config.columns = [ColumnPreset::NameSizeModified; 2];
                for (w, h) in [(80, 25), (120, 40), (80, 25), (60, 20)] {
                    draw(&mut a, w, h)?;
                }
            }
        }
    }
    Ok(())
}
#[test]
fn volume_line_formats_exact_compact_and_unavailable_without_scans() -> io::Result<()> {
    let t = Temp::new()?;
    let mut a = t.app()?;
    a.panels[0].space.result = Some(Ok(Space {
        available: 1000,
        total: 2000,
    }));
    assert!(line(&draw(&mut a, 80, 25)?, 21, 40).contains("1,000 free bytes / 2,000 total bytes"));
    a.panels[0].space.result = Some(Err("injected failure".into()));
    assert!(line(&draw(&mut a, 80, 25)?, 21, 40).contains("Volume space unavailable"));
    a.panels[0].space.result = None;
    assert!(line(&draw(&mut a, 80, 25)?, 21, 40).contains("Reading volume space"));
    let revisions = a.panels.each_ref().map(|p| p.revision);
    for _ in 0..3 {
        draw(&mut a, 80, 25)?;
    }
    assert_eq!(a.panels.each_ref().map(|p| p.revision), revisions);
    let space = volume_space::read(&t.0)?;
    assert!(space.total > 0);
    assert!(space.available <= space.total);
    assert!(volume_space::read(&t.0.join("missing")).is_err());
    Ok(())
}
