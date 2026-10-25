use dn::testing::{backend::TestBackend, Terminal};
use dn::{app::App, panel::Panel, panel_layout::PanelLayout, ui};
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
            "dn-refresh-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn finish(panel: &mut Panel) -> Vec<String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut errors = Vec::new();
    while panel.loading || panel.background_refreshing() {
        if let Some(error) = panel.poll() {
            errors.push(error);
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    errors
}
fn passive(panel: &mut Panel) {
    panel.refresh_if_due(true, Instant::now() + Duration::from_secs(3));
    assert!(!panel.loading);
    assert!(panel.background_refreshing());
}
fn focus(panel: &mut Panel, name: &str) {
    panel.cursor = panel.entries.iter().position(|e| e.name == name).unwrap();
}

#[test]
fn quiet_refresh_updates_removed_added_and_modified_entries_preserving_state() -> io::Result<()> {
    let temp = Temp::new()?;
    fs::create_dir(temp.0.join("removed"))?;
    for i in 0..50 {
        fs::write(temp.0.join(format!("file{i:02}")), "old")?;
    }
    let mut panel = Panel::new(temp.0.clone(), true)?;
    assert!(finish(&mut panel).is_empty());
    focus(&mut panel, "file40");
    panel.selected.extend(["file40".into(), "removed".into()]);
    panel.keep_visible_grid(8, 3);
    let offset = panel.offset;
    fs::remove_dir(temp.0.join("removed"))?;
    fs::write(temp.0.join("added"), "new")?;
    fs::write(temp.0.join("file40"), "updated data")?;
    passive(&mut panel);
    // Movement while a scan runs must survive its reply.
    focus(&mut panel, "file41");
    assert!(finish(&mut panel).is_empty());
    assert_eq!(panel.entries[panel.cursor].name, "file41");
    assert_eq!(panel.offset, offset);
    assert!(!panel.entries.iter().any(|e| e.name == "removed"));
    assert!(panel.entries.iter().any(|e| e.name == "added"));
    assert_eq!(
        panel
            .entries
            .iter()
            .find(|e| e.name == "file40")
            .unwrap()
            .size,
        Some(12)
    );
    assert_eq!(panel.selected.len(), 1);
    assert!(panel.selected.contains(std::ffi::OsStr::new("file40")));
    Ok(())
}

#[test]
fn manual_refresh_retains_filter_hidden_marks_and_drops_removed_marks() -> io::Result<()> {
    let temp = Temp::new()?;
    for name in ["a.rs", "b.rs", "excluded.txt", "deleted.txt", ".hidden.rs"] {
        fs::write(temp.0.join(name), "")?;
    }
    let mut panel = Panel::new(temp.0.clone(), true)?;
    finish(&mut panel);
    panel
        .selected
        .extend(["a.rs".into(), "excluded.txt".into(), "deleted.txt".into()]);
    panel.set_filter("*.rs".into());
    focus(&mut panel, "b.rs");
    fs::remove_file(temp.0.join("deleted.txt"))?;
    panel.refresh(false);
    assert!(finish(&mut panel).is_empty());
    assert_eq!(panel.filter, "*.rs");
    assert_eq!(panel.entries[panel.cursor].name, "b.rs");
    assert_eq!(panel.filtered_marks(), 1);
    assert_eq!(panel.selected.len(), 2);
    assert!(!panel.entries.iter().any(|e| e.name == ".hidden.rs"));
    Ok(())
}

#[test]
fn removed_current_directory_recovers_to_nearest_existing_ancestor() -> io::Result<()> {
    let temp = Temp::new()?;
    let nested = temp.0.join("gone/child");
    fs::create_dir_all(&nested)?;
    let mut panel = Panel::new(nested.clone(), true)?;
    finish(&mut panel);
    panel.set_filter("*.txt".into());
    panel.selected.insert("old.txt".into());
    fs::remove_dir_all(temp.0.join("gone"))?;
    passive(&mut panel);
    let errors = finish(&mut panel);
    assert_eq!(panel.path, temp.0);
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("Showing"));
    assert!(panel.filter.is_empty() && panel.selected.is_empty());
    passive(&mut panel);
    assert!(finish(&mut panel).is_empty());
    Ok(())
}

#[test]
fn replaced_directory_recovers_but_failed_navigation_retains_current_listing() -> io::Result<()> {
    let temp = Temp::new()?;
    let child = temp.0.join("child");
    fs::create_dir(&child)?;
    let mut panel = Panel::new(child.clone(), true)?;
    finish(&mut panel);
    fs::remove_dir(&child)?;
    fs::write(&child, "replacement")?;
    panel.refresh(true);
    assert_eq!(finish(&mut panel).len(), 1);
    assert_eq!(panel.path, temp.0);
    let revision = panel.revision;
    panel.go_to(temp.0.join("does-not-exist"), true);
    assert_eq!(finish(&mut panel).len(), 1);
    assert_eq!(panel.path, temp.0);
    assert_eq!(panel.revision, revision);
    Ok(())
}

#[test]
fn navigation_supersedes_passive_scan_without_applying_stale_reply() -> io::Result<()> {
    let temp = Temp::new()?;
    let child = temp.0.join("child");
    fs::create_dir(&child)?;
    let mut panel = Panel::new(temp.0.clone(), true)?;
    finish(&mut panel);
    passive(&mut panel);
    panel.go_to(child.clone(), true);
    assert!(finish(&mut panel).is_empty());
    assert_eq!(panel.path, child);
    Ok(())
}

#[test]
fn listing_renders_directory_names_without_slashes_in_both_layouts() -> io::Result<()> {
    let temp = Temp::new()?;
    fs::create_dir(temp.0.join("Folder文"))?;
    let mut app = App::new(temp.0.clone())?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.panels.iter().any(|p| p.loading) {
        app.poll();
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    for layout in [PanelLayout::Brief, PanelLayout::Details] {
        app.config.layouts = [layout; 2];
        let mut terminal = Terminal::new(TestBackend::new(80, 25))?;
        terminal.draw(|f| ui::draw(f, &mut app))?;
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(text.contains("Folder文"));
        assert!(!text.contains("/Folder文"));
        assert!(!text.contains("/.."));
        assert_eq!(
            app.panels[0]
                .entries
                .iter()
                .find(|e| e.name == "Folder文")
                .unwrap()
                .path,
            temp.0.join("Folder文")
        );
    }
    Ok(())
}

#[test]
fn app_defers_passive_replies_under_dialogs_then_repaints() -> io::Result<()> {
    let temp = Temp::new()?;
    fs::create_dir(temp.0.join("removed"))?;
    let mut app = App::new(temp.0.clone())?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.panels.iter().any(|p| p.loading) {
        app.poll();
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    fs::remove_dir(temp.0.join("removed"))?;
    passive(&mut app.panels[0]);
    let revision = app.panels[0].revision;
    app.error("Keep this dialog stable");
    app.poll();
    assert_eq!(app.panels[0].revision, revision);
    assert!(app.panels[0].background_refreshing());
    app.dialogs.clear();
    while app.panels[0].background_refreshing() {
        if app.panels[0].revision != revision {
            break;
        }
        let changed = app.poll();
        if app.panels[0].revision != revision {
            assert!(changed);
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    assert!(app.panels[0].revision > revision);
    assert!(!app.panels[0].entries.iter().any(|e| e.name == "removed"));
    Ok(())
}
