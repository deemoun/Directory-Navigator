//! Optional before/after cell-and-hit-target captures for internal refactors.
use dn::{
    app::App, columns::ColumnPreset, operations::Kind, palette::PaletteProfile,
    panel_layout::PanelLayout, ui,
};
use ratatui::{backend::TestBackend, Terminal};
use std::{
    fs, io,
    path::PathBuf,
    thread,
    time::{Duration, Instant, SystemTime},
};
fn ready(app: &mut App) {
    let end = Instant::now() + Duration::from_secs(5);
    while app.panels.iter().any(|p| p.loading)
        || app.viewer.as_ref().is_some_and(|v| v.contents.is_none())
    {
        app.poll();
        assert!(Instant::now() < end);
        thread::sleep(Duration::from_millis(1));
    }
}
#[test]
fn unchanged_cells_styles_and_mouse_targets() -> io::Result<()> {
    let external = std::env::var_os("DN_UI_FIXTURE").map(PathBuf::from);
    let path = external.clone().unwrap_or_else(|| {
        std::env::temp_dir().join(format!("dn-ui-contract-{}", std::process::id()))
    });
    if external.is_none() {
        fs::create_dir(&path)?;
    }
    fs::create_dir_all(path.join("folder"))?;
    fs::write(
        path.join("alpha.rs"),
        "fn main() {\n    println!(\"界é\");\n}\n",
    )?;
    for name in ["ordinary.bin", "archive.zip"] {
        fs::write(path.join(name), "fixture")?;
    }
    let time = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    for entry in fs::read_dir(&path)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            fs::OpenOptions::new()
                .write(true)
                .open(entry.path())?
                .set_times(fs::FileTimes::new().set_modified(time))?;
        }
        #[cfg(unix)]
        if entry.file_type()?.is_dir() {
            fs::File::open(entry.path())?.set_times(fs::FileTimes::new().set_modified(time))?;
        }
    }
    let mut captures = String::new();
    for profile in [PaletteProfile::ClassicDn, PaletteProfile::Terminal] {
        for (w, h) in [(80, 25), (132, 40)] {
            let mut app = App::new(path.clone())?;
            ready(&mut app);
            app.config.palette = profile;
            for scene in 0..10 {
                match scene {
                    0 => {}
                    1 => {
                        app.config.layouts = [PanelLayout::Details; 2];
                        app.config.columns = [ColumnPreset::NameSizeModified; 2];
                    }
                    2 => app.prompt_operation(Kind::Mkdir, false),
                    3 => {
                        app.prompt = None;
                        app.error("A sample error.\nNo files changed.");
                    }
                    4 => {
                        app.dialogs.clear();
                        app.open_pattern(true);
                    }
                    5 => {
                        app.pattern = None;
                        app.open_columns();
                    }
                    6 => {
                        app.columns_dialog = None;
                        dn::palette::open(&mut app);
                    }
                    7 => {
                        app.color_dialog = None;
                        app.viewer = Some(dn::viewer::Viewer::open(path.join("alpha.rs"))?);
                        ready(&mut app);
                    }
                    8 => {
                        app.viewer = None;
                        dn::editor::open(&mut app, true);
                    }
                    _ => dn::editor::action(&mut app, dn::editor::EditorAction::GoTo),
                }
                app.clock.text = " 12:34:56 ".into();
                let mut terminal = Terminal::new(TestBackend::new(w, h))?;
                terminal.draw(|f| ui::draw(f, &mut app))?;
                captures.push_str(&format!(
                    "{profile:?} {w} {h} {scene}\n{:?}\n{:?}\n",
                    terminal.backend().buffer(),
                    app.mouse.regions
                ));
            }
        }
    }
    if let Some(base) = std::env::var_os("DN_UI_BASELINE") {
        if std::env::var_os("DN_UI_RECORD").is_some() {
            fs::write(base, captures)?;
        } else {
            assert_eq!(
                captures,
                fs::read_to_string(base)?,
                "UI cells/styles/targets changed"
            );
        }
    }
    if external.is_none() {
        fs::remove_dir_all(path)?;
    }
    Ok(())
}
