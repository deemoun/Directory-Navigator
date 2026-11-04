use crossterm::event::{
    KeyCode as K, KeyEvent, KeyModifiers as M, MouseButton, MouseEvent, MouseEventKind,
};
use dn::{
    app::App,
    editor, input,
    mouse::{self, Target},
    testing::{backend::TestBackend, Terminal},
    ui,
    viewer::{self, ViewMode},
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
        let p = std::env::temp_dir().join(format!(
            "dn-markdown-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p)?;
        Ok(Self(fs::canonicalize(p)?))
    }
    fn app(&self) -> io::Result<App> {
        let mut a = App::new(self.0.clone())?;
        a.config_store = dn::config::Store::new(Some(self.0.join("config.conf")));
        a.config.save_on_exit = false;
        a.config.show_clock = false;
        wait(&mut a, |a| a.panels.iter().all(|p| !p.loading));
        Ok(a)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn wait(a: &mut App, p: impl Fn(&App) -> bool) {
    let end = Instant::now() + Duration::from_secs(12);
    loop {
        a.poll();
        if p(a) {
            return;
        }
        assert!(Instant::now() < end, "{}", dn::testing::diagnostic_state(a));
        thread::sleep(Duration::from_millis(1));
    }
}
fn key(a: &mut App, k: K) {
    input::handle(a, KeyEvent::new(k, M::NONE));
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
fn open(a: &mut App, path: PathBuf, mode: ViewMode) -> io::Result<()> {
    a.show_viewer(viewer::Viewer::open_mode(path, mode)?);
    wait(a, |a| {
        a.desktop.active_viewer().is_some_and(|v| {
            v.contents.is_some()
                && (!dn::markdown::is_path(&v.path)
                    || v.markdown_available()
                    || v.contents.as_ref().unwrap().binary)
        })
    });
    Ok(())
}
const SAMPLE:&str="# Heading 界\r\n\r\nParagraph **bold** and [site](https://example.org).\r\ncontinued line\r\n\r\n| Name | Value |\r\n| --- | ---: |\r\n| 界 | 42 |\r\n\r\n```rust\r\nfn main() {\r\n\t42\r\n}\r\n```\r\n";
#[test]
fn defaults_toggle_search_goto_and_readonly_source_preserve_original_bytes() -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("README.MARKDOWN");
    fs::write(&path, SAMPLE)?;
    let mut a = t.app()?;
    open(&mut a, path.clone(), ViewMode::Auto)?;
    assert!(a.desktop.active_viewer().unwrap().markdown.rendered);
    let text = draw(&mut a, 80, 25)?;
    assert!(text.contains("Heading 界") && !text.contains("**bold**"));
    key(&mut a, K::F(6));
    let source = draw(&mut a, 80, 25)?;
    assert!(
        source.contains("# Heading 界") && source.contains("**bold**"),
        "{source}"
    );
    key(&mut a, K::F(6));
    {
        let v = a.desktop.active_viewer_mut().unwrap();
        assert!(v
            .find(
                &viewer::SearchMemory {
                    query: "bold".into(),
                    case_sensitive: true
                },
                false,
                false
            )
            .unwrap());
        assert_eq!(
            v.contents.as_ref().unwrap().lines[v.current.as_ref().unwrap().line].find("bold"),
            Some(v.current.as_ref().unwrap().range.start)
        );
        v.go_to("11").unwrap();
        assert_eq!(v.source_line(), 10);
    }
    key(&mut a, K::F(6));
    assert!(!a.desktop.active_viewer().unwrap().markdown.rendered);
    assert_eq!(a.desktop.active_viewer().unwrap().source_line(), 10);
    assert!(a
        .desktop
        .active_viewer()
        .unwrap()
        .contents
        .as_ref()
        .unwrap()
        .lines
        .iter()
        .any(|l| l.contains("**bold**")));
    key(&mut a, K::F(6));
    assert!(a.desktop.active_viewer().unwrap().markdown.rendered);
    assert_eq!(a.desktop.active_viewer().unwrap().source_line(), 10);
    assert_eq!(fs::read(&path)?, SAMPLE.as_bytes());
    key(&mut a, K::Esc);
    open(&mut a, path.clone(), ViewMode::Text)?;
    assert!(!a.desktop.active_viewer().unwrap().markdown.rendered);
    key(&mut a, K::Esc);
    open(&mut a, path, ViewMode::Hex)?;
    assert!(
        a.desktop
            .active_viewer()
            .unwrap()
            .contents
            .as_ref()
            .unwrap()
            .binary
    );
    assert!(!a.desktop.active_viewer().unwrap().markdown_available());
    Ok(())
}
#[test]
fn editor_unsaved_preview_reuses_document_and_preserves_cursor_dirty_undo_and_owner(
) -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("notes.md");
    fs::write(&path, "# Saved\n\n```rust\nfn main() {}\n```\n")?;
    let mut a = t.app()?;
    editor::open_path(&mut a, path.clone());
    wait(&mut a, |a| {
        a.desktop
            .active_editor()
            .is_some_and(|e| !e.busy() && e.highlight.color(0, 0) != 0x1f)
    });
    let owner = a.desktop.active().id;
    assert_eq!(
        a.desktop.active_editor().unwrap().highlight.language,
        editor::highlight::Language::Markdown
    );
    assert_eq!(
        a.desktop.active_editor().unwrap().highlight.color(3, 0),
        0x1b
    );
    input::paste(&mut a, "Unsaved 界\n");
    let e = a.desktop.active_editor().unwrap();
    let before = e.buffer.as_ref().unwrap().text.clone();
    let cursor = e.buffer.as_ref().unwrap().cursor;
    assert!(e.dirty());
    key(&mut a, K::F(5));
    let preview = a.desktop.active().id;
    assert_ne!(preview, owner);
    wait(&mut a, |a| {
        a.desktop
            .active_viewer()
            .is_some_and(|v| v.markdown.rendered)
    });
    // Preview starts at the editing line, which is below the inserted first
    // line. Home reveals that unsaved line without moving the editor cursor.
    assert_eq!(a.desktop.active_viewer().unwrap().source_line(), 1);
    key(&mut a, K::Home);
    assert!(draw(&mut a, 80, 25)?.contains("Unsaved 界"));
    assert!(!a
        .file_tools
        .histories
        .get(dn::file_tools::history::HistoryKind::View)
        .iter()
        .any(|p| p == &path));
    key(&mut a, K::F(5));
    assert_eq!(a.desktop.active().id, owner);
    assert_eq!(
        a.desktop
            .active_editor()
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .cursor,
        cursor
    );
    input::paste(&mut a, "new ");
    key(&mut a, K::F(5));
    assert_eq!(a.desktop.active().id, preview);
    wait(&mut a, |a| {
        a.desktop
            .active_viewer()
            .is_some_and(|v| v.markdown.rendered)
    });
    assert!(draw(&mut a, 80, 25)?.contains("new "));
    key(&mut a, K::Esc);
    assert_eq!(a.desktop.active().id, owner);
    assert!(a.desktop.active_editor().unwrap().dirty());
    editor::action(&mut a, editor::EditorAction::Undo);
    assert_eq!(
        a.desktop
            .active_editor()
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .text,
        before
    );
    assert_eq!(
        fs::read_to_string(path)?,
        "# Saved\n\n```rust\nfn main() {}\n```\n"
    );
    Ok(())
}
#[test]
fn untitled_preview_source_close_and_multiple_editors_stay_independent() -> io::Result<()> {
    let t = Temp::new()?;
    let mut a = t.app()?;
    editor::open(&mut a, true);
    input::paste(&mut a, "# First");
    let owner = a.desktop.active().id;
    key(&mut a, K::F(5));
    let preview = a.desktop.active().id;
    wait(&mut a, |a| {
        a.desktop
            .active_viewer()
            .is_some_and(|v| v.markdown.rendered)
    });
    key(&mut a, K::F(5));
    editor::open(&mut a, true);
    input::paste(&mut a, "# Second");
    let second = a.desktop.active().id;
    key(&mut a, K::F(5));
    assert_ne!(a.desktop.active().id, preview);
    wait(&mut a, |a| {
        a.desktop
            .active_viewer()
            .is_some_and(|v| v.markdown.rendered)
    });
    key(&mut a, K::Esc);
    assert_eq!(a.desktop.active().id, second);
    a.desktop.activate(owner);
    a.sync_modals();
    editor::action(&mut a, editor::EditorAction::Close);
    editor::action(&mut a, editor::EditorAction::Discard);
    a.desktop.activate(preview);
    a.sync_modals();
    assert!(a.desktop.active_viewer().is_some());
    key(&mut a, K::F(5));
    assert_ne!(a.desktop.active().id, preview);
    assert!(!a.desktop.contains(owner));
    assert!(a.desktop.editor(second).is_some());
    Ok(())
}
#[test]
fn immutable_theme_language_resize_and_mouse_toggle_preserve_formatted_content() -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("test.md");
    fs::write(&path, SAMPLE)?;
    let mut a = t.app()?;
    open(&mut a, path, ViewMode::Auto)?;
    for language in [
        dn::localization::Language::English,
        dn::localization::Language::Russian,
    ] {
        a.config.language = language;
        for palette in [
            dn::palette::PaletteProfile::ClassicDn,
            dn::palette::PaletteProfile::Custom(dn::palette::theme::DEFAULTS.map(|v| v ^ 1)),
        ] {
            a.config.palette = palette;
            for (w, h) in [(80, 25), (132, 40), (50, 15), (80, 25)] {
                let text = draw(&mut a, w, h)?;
                let hits = a.mouse.regions.clone();
                for _ in 0..3 {
                    assert_eq!(text, draw(&mut a, w, h)?);
                    assert_eq!(hits, a.mouse.regions);
                }
            }
        }
    }
    draw(&mut a, 80, 25)?;
    let rect = a
        .mouse
        .regions
        .iter()
        .find(|(_, target)| *target == Target::Viewer(viewer::Action::ToggleMarkdown))
        .unwrap()
        .0;
    mouse::handle(
        &mut a,
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: rect.x,
            row: rect.y,
            modifiers: M::NONE,
        },
    );
    assert!(!a.desktop.active_viewer().unwrap().markdown.rendered);
    Ok(())
}
#[test]
fn ordinary_text_hex_and_quick_view_stay_plain_and_f5_does_not_close_them() -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("plain.txt");
    fs::write(&path, "# Literal **source**")?;
    let mut a = t.app()?;
    open(&mut a, path, ViewMode::Auto)?;
    let id = a.desktop.active().id;
    key(&mut a, K::F(5));
    assert_eq!(a.desktop.active().id, id);
    assert!(!a.desktop.active_viewer().unwrap().markdown_available());
    assert!(draw(&mut a, 80, 25)?.contains("# Literal **source**"));
    let path = t.0.join("quick.md");
    fs::write(&path, "# Quick source")?;
    let mut v = viewer::Viewer::open_preview(path)?;
    let end = Instant::now() + Duration::from_secs(5);
    while v.contents.is_none() {
        v.poll();
        assert!(Instant::now() < end);
        thread::sleep(Duration::from_millis(1));
    }
    assert!(!v.markdown_available());
    assert_eq!(v.contents.unwrap().lines, ["# Quick source"]);
    Ok(())
}

#[test]
fn malformed_or_oversized_rendering_falls_back_and_inactive_completions_do_not_steal_focus(
) -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("deep.md");
    let source = format!("{}deep", "> ".repeat(65));
    fs::write(&path, &source)?;
    let mut a = t.app()?;
    a.show_viewer(viewer::Viewer::open(path.clone())?);
    wait(&mut a, |a| {
        a.desktop
            .active_viewer()
            .is_some_and(|v| v.notice.contains("limit reached"))
    });
    let v = a.desktop.active_viewer().unwrap();
    assert!(!v.markdown.rendered);
    assert!(v.contents.as_ref().unwrap().lines[0].contains("deep"));
    assert_eq!(fs::read_to_string(path)?, source);
    key(&mut a, K::Esc);
    let path = t.0.join("inactive.md");
    fs::write(&path, SAMPLE)?;
    a.show_viewer(viewer::Viewer::open(path)?);
    let viewer = a.desktop.active().id;
    editor::open(&mut a, true);
    let active = a.desktop.active().id;
    wait(&mut a, |a| {
        a.desktop
            .viewer(viewer)
            .is_some_and(|v| v.markdown.rendered)
    });
    assert_eq!(a.desktop.active().id, active);
    a.desktop.close(viewer);
    a.sync_modals();
    input::paste(&mut a, "# Latest buffer");
    key(&mut a, K::F(5));
    wait(&mut a, |a| {
        a.desktop
            .active_viewer()
            .is_some_and(|v| v.markdown.rendered)
    });
    assert!(draw(&mut a, 80, 25)?.contains("Latest buffer"));
    Ok(())
}
#[test]
fn markdown_archive_members_respect_auto_text_hex_and_authentication() -> io::Result<()> {
    use std::io::Write;
    let t = Temp::new()?;
    let path = t.0.join("docs.zip");
    let mut zip = zip::ZipWriter::new(fs::File::create(&path)?);
    zip.start_file(
        "README.MD",
        zip::write::SimpleFileOptions::default()
            .with_aes_encryption(zip::AesMode::Aes256, "password"),
    )?;
    zip.write_all(SAMPLE.as_bytes())?;
    zip.finish()?;
    let mut a = t.app()?;
    dn::archive::open(&mut a, path);
    wait(&mut a, |a| a.panels[0].is_archive() && !a.panels[0].loading);
    a.panels[0].cursor = a.panels[0]
        .entries
        .iter()
        .position(|e| e.name == "README.MD")
        .unwrap();
    a.open_viewer();
    assert_eq!(dn::focus::current(&a), dn::focus::Focus::ArchiveDialog);
    input::paste(&mut a, "password");
    key(&mut a, K::Enter);
    wait(&mut a, |a| {
        a.desktop
            .active_viewer()
            .is_some_and(|v| v.markdown.rendered)
    });
    assert!(draw(&mut a, 80, 25)?.contains("Heading 界"));
    assert!(a
        .file_tools
        .histories
        .get(dn::file_tools::history::HistoryKind::View)
        .is_empty());
    key(&mut a, K::Esc);
    a.open_viewer_mode(ViewMode::Text);
    wait(&mut a, |a| {
        a.desktop
            .active_viewer()
            .is_some_and(|v| v.contents.is_some())
    });
    assert!(!a.desktop.active_viewer().unwrap().markdown.rendered);
    assert!(a
        .desktop
        .active_viewer()
        .unwrap()
        .contents
        .as_ref()
        .unwrap()
        .lines[2]
        .contains("**bold**"));
    key(&mut a, K::Esc);
    a.open_viewer_mode(ViewMode::Hex);
    wait(&mut a, |a| {
        a.desktop
            .active_viewer()
            .is_some_and(|v| v.contents.is_some())
    });
    assert!(
        a.desktop
            .active_viewer()
            .unwrap()
            .contents
            .as_ref()
            .unwrap()
            .binary
    );
    Ok(())
}

#[test]
fn long_paragraph_and_fixed_code_keep_their_place_across_modes_and_resize() -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.0.join("long.md");
    fs::write(
        &path,
        format!(
            "{}\n\n```\n{}\n```\n",
            "word 界 ".repeat(200),
            "0123456789".repeat(30)
        ),
    )?;
    let mut a = t.app()?;
    open(&mut a, path, ViewMode::Auto)?;
    draw(&mut a, 80, 25)?;
    a.desktop.active_viewer_mut().unwrap().step(2);
    let anchor = a.desktop.active_viewer().unwrap().anchor();
    assert!(anchor.1 > 0);
    key(&mut a, K::F(6));
    assert_eq!(a.desktop.active_viewer().unwrap().anchor(), anchor);
    key(&mut a, K::F(6));
    assert_eq!(a.desktop.active_viewer().unwrap().anchor(), anchor);
    draw(&mut a, 132, 40)?;
    let v = a.desktop.active_viewer().unwrap();
    let row = &v.rows[v.offset];
    assert!(row.range.contains(&anchor.1));
    a.desktop.active_viewer_mut().unwrap().go_to("4").unwrap();
    for _ in 0..5 {
        key(&mut a, K::Right);
    }
    assert_eq!(a.desktop.active_viewer().unwrap().horizontal, 20);
    key(&mut a, K::F(6));
    assert_eq!(a.desktop.active_viewer().unwrap().horizontal, 20);
    key(&mut a, K::F(6));
    assert_eq!(a.desktop.active_viewer().unwrap().horizontal, 20);
    Ok(())
}
#[test]
fn rapid_edits_replace_markdown_highlight_generations_without_altering_the_buffer() -> io::Result<()>
{
    let t = Temp::new()?;
    let path = t.0.join("changes.md");
    fs::write(&path, "# Old\n")?;
    let mut a = t.app()?;
    editor::open_path(&mut a, path.clone());
    wait(&mut a, |a| {
        a.desktop.active_editor().is_some_and(|e| !e.busy())
    });
    editor::action(&mut a, editor::EditorAction::SelectAll);
    input::paste(&mut a, "```rust\nfn interim() {}\n```\n");
    editor::action(&mut a, editor::EditorAction::SelectAll);
    input::paste(&mut a, "**Latest**\n");
    wait(&mut a, |a| {
        a.desktop
            .active_editor()
            .is_some_and(|e| e.highlight.color(0, 0) == 0x1b)
    });
    for _ in 0..4 {
        a.poll();
        assert_eq!(
            a.desktop.active_editor().unwrap().highlight.color(0, 0),
            0x1b
        );
    }
    assert_eq!(
        a.desktop
            .active_editor()
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .text,
        "**Latest**\n"
    );
    assert!(a.desktop.active_editor().unwrap().dirty());
    assert_eq!(fs::read_to_string(path)?, "# Old\n");
    Ok(())
}
