use crossterm::event::{KeyCode as K, KeyEvent, KeyModifiers as M};
use dn::{
    app::App,
    editor::{self, storage, EditorAction as E, Modal},
    input,
    text_encoding::{self, TextEncoding as C, TextFormat},
    viewer::{self, Action as V, ViewMode},
};
use std::{
    fs, io,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};
struct Temp(PathBuf);
impl Temp {
    fn new() -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "dn-encoding-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p)?;
        Ok(Self(fs::canonicalize(p)?))
    }
    fn app(&self) -> io::Result<App> {
        let mut app = App::new(self.0.clone())?;
        app.config_store = dn::config::Store::new(Some(self.0.join("config.conf")));
        app.config.save_on_exit = false;
        app.config.show_clock = false;
        wait(&mut app, |a| a.panels.iter().all(|p| !p.loading));
        Ok(app)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn wait(app: &mut App, ready: impl Fn(&App) -> bool) {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        app.poll();
        if ready(app) {
            return;
        }
        assert!(
            Instant::now() < end,
            "{}",
            dn::testing::diagnostic_state(app)
        );
        thread::sleep(Duration::from_millis(1));
    }
}
fn key(app: &mut App, code: K) {
    input::handle(app, KeyEvent::new(code, M::NONE));
}
fn picker_ready(app: &mut App) {
    wait(app, |a| {
        a.desktop.active_editor().is_some_and(
            |e| matches!(e.modals.editor_modal(), Some(Modal::Encoding(d)) if !d.busy()),
        )
    });
}
fn draw(app: &mut App, w: u16, h: u16) -> io::Result<String> {
    let mut t = dn::testing::Terminal::new(dn::testing::backend::TestBackend::new(w, h))?;
    t.draw(|f| dn::ui::draw(f, app))?;
    assert!(app
        .mouse
        .regions
        .iter()
        .all(|(r, _)| r.right() <= w && r.bottom() <= h));
    Ok(t.backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect())
}
#[test]
fn strict_roundtrips_preserve_endings_unicode_and_legacy_graphics() {
    for encoding in C::ALL {
        let text = if encoding == C::Windows1252 {
            "café £ €\r\nline\rnext\n"
        } else {
            "Привет Ёё\r\nстрока\rследующая\n"
        };
        for bom in [false, true]
            .into_iter()
            .filter(|b| !*b || encoding.unicode())
        {
            let f = TextFormat { encoding, bom };
            let bytes = f.encode(text).unwrap();
            assert_eq!(f.decode(&bytes, false).unwrap(), text);
            if encoding.unicode() && (bom || encoding == C::Utf8) {
                assert_eq!(text_encoding::detect(&bytes, false).unwrap(), Some(f));
            }
        }
    }
    let f = C::Cp866.default_format();
    assert_eq!(
        f.decode(&f.encode("╔═╤╗║╚╝").unwrap(), false).unwrap(),
        "╔═╤╗║╚╝"
    );
    assert!(C::Windows1251.default_format().encode("hello 😀").is_err());
    assert!(C::Cp866.default_format().encode("e\u{301}").is_err());
}
#[test]
fn malformed_sequences_bom_order_and_truncated_character_boundaries() {
    assert!(text_encoding::detect(b"\xff\xfe\0\0", false)
        .unwrap_err()
        .contains("UTF-32"));
    for encoding in [C::Utf16Le, C::Utf16Be] {
        let f = encoding.default_format();
        let mut bytes = f.encode("a😀").unwrap();
        bytes.pop();
        assert!(f.decode(&bytes, false).is_err());
        assert_eq!(f.decode(&bytes, true).unwrap(), "a");
        let mut bytes = f.encode("a").unwrap();
        bytes.extend_from_slice(if encoding == C::Utf16Le {
            b"\0\xdc"
        } else {
            b"\xdc\0"
        });
        assert!(f.decode(&bytes, true).is_err());
    }
    assert_eq!(
        TextFormat::default().decode(b"a\xe2\x82", true).unwrap(),
        "a"
    );
    assert!(TextFormat::default().decode(b"a\xe2x", true).is_err());
    assert!(!text_encoding::plausible_text(b"text\0binary"));
    assert!(text_encoding::detect(
        &C::Windows1251.default_format().encode("Привет").unwrap(),
        false
    )
    .unwrap()
    .is_none());
}
#[test]
fn storage_preserves_exact_bytes_converts_explicitly_and_refuses_loss() -> io::Result<()> {
    let t = Temp::new()?;
    for encoding in C::ALL {
        let f = encoding.default_format();
        let text = if encoding == C::Windows1252 {
            "café\r\nline\rnext\n"
        } else {
            "Привет\r\nстрока\rещё\n"
        };
        let p = t.0.join(encoding.name());
        let bytes = f.encode(text).unwrap();
        fs::write(&p, &bytes)?;
        let (b, original, got) = storage::load_format(&p, Some(f)).unwrap();
        assert_eq!(got, f);
        let out = storage::save(&p, &b.text, f, Some(&original)).unwrap();
        assert_eq!(fs::read(&p)?, bytes);
        if !encoding.unicode() {
            assert!(storage::save(&p, "😀", f, Some(&out.snapshot)).is_err());
            assert_eq!(fs::read(&p)?, bytes);
        }
        let out = storage::save(
            &p,
            &b.text,
            C::Utf16Be.default_format(),
            Some(&out.snapshot),
        )
        .unwrap();
        let (b2, _, f2) = storage::load(&p).unwrap();
        assert_eq!(b2.text, text);
        assert_eq!(f2, C::Utf16Be.default_format());
        fs::write(&p, "external")?;
        assert!(storage::save(&p, text, f2, Some(&out.snapshot)).is_err());
        assert_eq!(fs::read(&p)?, b"external");
    }
    Ok(())
}
#[test]
fn empty_bom_files_and_decoded_or_encoded_size_limits() -> io::Result<()> {
    let t = Temp::new()?;
    for encoding in [C::Utf8, C::Utf16Le, C::Utf16Be] {
        let f = TextFormat {
            encoding,
            bom: true,
        };
        let p = t.0.join(encoding.name());
        fs::write(&p, f.encode("").unwrap())?;
        assert!(storage::load(&p).unwrap().0.text.is_empty());
    }
    let text = "я".repeat(editor::buffer::MAX_BYTES / 2 + 1);
    let f = C::Cp866.default_format();
    let bytes = f.encode(&text).unwrap();
    let p = t.0.join("expanded");
    fs::write(&p, &bytes)?;
    assert!(storage::load_format(&p, Some(f)).is_err());
    let c = viewer::from_encoded_bytes(bytes.into(), f, false).unwrap();
    assert_eq!(c.source.as_deref(), Some(text.as_str()));
    assert!(!c.binary);
    let p = t.0.join("too-large");
    assert!(storage::save(
        &p,
        &"a".repeat(editor::buffer::MAX_BYTES),
        C::Utf16Le.default_format(),
        None
    )
    .is_err());
    assert!(!p.exists());
    Ok(())
}
#[test]
fn viewer_chooses_encoding_from_original_snapshot_then_hex_uses_original_bytes() -> io::Result<()> {
    let t = Temp::new()?;
    let p = t.0.join("legacy.txt");
    let f = C::Windows1251.default_format();
    let bytes = f.encode("Привет\nстрока").unwrap();
    fs::write(&p, &bytes)?;
    let mut app = t.app()?;
    app.show_viewer(viewer::Viewer::open(p.clone())?);
    wait(&mut app, |a| {
        a.desktop
            .active_viewer()
            .is_some_and(|v| v.modals.viewer_encoding().is_some_and(|d| !d.busy()))
    });
    fs::write(&p, "external")?;
    viewer::action(&mut app, V::EncodingChoice(4));
    wait(&mut app, |a| {
        !a.desktop
            .active_viewer()
            .unwrap()
            .modals
            .viewer_encoding()
            .unwrap()
            .busy()
    });
    key(&mut app, K::Enter);
    wait(&mut app, |a| {
        a.desktop
            .active_viewer()
            .unwrap()
            .contents
            .as_ref()
            .is_some_and(|c| !c.binary)
    });
    let v = app.desktop.active_viewer().unwrap();
    assert_eq!(
        v.contents.as_ref().unwrap().raw.as_deref(),
        Some(bytes.as_slice())
    );
    assert!(v.contents.as_ref().unwrap().lines[0].contains("Привет"));
    key(&mut app, K::F(8));
    viewer::action(&mut app, V::EncodingHex);
    wait(&mut app, |a| {
        a.desktop
            .active_viewer()
            .unwrap()
            .contents
            .as_ref()
            .unwrap()
            .binary
    });
    assert_eq!(
        app.desktop
            .active_viewer()
            .unwrap()
            .contents
            .as_ref()
            .unwrap()
            .raw
            .as_deref(),
        Some(bytes.as_slice())
    );
    assert_eq!(fs::read(p)?, b"external");
    Ok(())
}
#[test]
fn editor_encoding_dialog_keyboard_mouse_bounds_cancel_and_conversion() -> io::Result<()> {
    let t = Temp::new()?;
    let p = t.0.join("legacy.md");
    let f = C::Windows1251.default_format();
    fs::write(&p, f.encode("# Привет\r\nстрока\n").unwrap())?;
    let mut app = t.app()?;
    editor::open_path(&mut app, p.clone());
    picker_ready(&mut app);
    editor::action(&mut app, E::EncodingChoice(4));
    picker_ready(&mut app);
    for language in [
        dn::localization::Language::English,
        dn::localization::Language::Russian,
    ] {
        app.config.language = language;
        for (w, h) in [(80, 25), (120, 40), (55, 16), (80, 25)] {
            let first = draw(&mut app, w, h)?;
            for _ in 0..3 {
                assert_eq!(draw(&mut app, w, h)?, first);
            }
        }
    }
    app.config.palette =
        dn::palette::PaletteProfile::Custom(dn::palette::theme::DEFAULTS.map(|v| v ^ 1));
    draw(&mut app, 80, 25)?;
    let rect = app
        .mouse
        .regions
        .iter()
        .find(|(_, target)| *target == dn::mouse::Target::Editor(E::EncodingChoice(4)))
        .unwrap()
        .0;
    for kind in [
        crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        crossterm::event::MouseEventKind::Up(crossterm::event::MouseButton::Left),
    ] {
        dn::mouse::handle(
            &mut app,
            crossterm::event::MouseEvent {
                kind,
                column: rect.x,
                row: rect.y,
                modifiers: M::NONE,
            },
        );
    }
    picker_ready(&mut app);
    key(&mut app, K::Enter);
    wait(&mut app, |a| {
        a.desktop
            .active_editor()
            .is_some_and(|e| !e.busy() && e.buffer.is_some())
    });
    assert_eq!(app.desktop.active_editor().unwrap().text_format(), f);
    editor::action(&mut app, E::SaveEncoding);
    editor::action(&mut app, E::EncodingChoice(2));
    key(&mut app, K::Esc);
    assert_eq!(app.desktop.active_editor().unwrap().text_format(), f);
    editor::action(&mut app, E::SaveEncoding);
    editor::action(&mut app, E::EncodingChoice(2));
    editor::action(&mut app, E::EncodingAccept);
    wait(&mut app, |a| !a.desktop.active_editor().unwrap().busy());
    assert_eq!(
        app.desktop.active_editor().unwrap().text_format(),
        C::Utf16Be.default_format()
    );
    assert_eq!(storage::load(&p).unwrap().0.text, "# Привет\r\nстрока\n");
    // A failed conversion keeps the format and exact published bytes.
    let original = fs::read(&p)?;
    editor::paste(&mut app, "😀");
    editor::action(&mut app, E::SaveEncoding);
    editor::action(&mut app, E::EncodingChoice(4));
    editor::action(&mut app, E::EncodingAccept);
    assert_eq!(fs::read(&p)?, original);
    assert_eq!(
        app.desktop.active_editor().unwrap().text_format(),
        C::Utf16Be.default_format()
    );
    assert!(app.desktop.active_editor().unwrap().dirty());
    Ok(())
}
#[test]
fn dirty_reopen_cancel_discard_and_cursor_clamping() -> io::Result<()> {
    let t = Temp::new()?;
    let p = t.0.join("text.txt");
    fs::write(&p, "one\ntwo\nthree")?;
    let mut a = t.app()?;
    editor::open_path(&mut a, p.clone());
    wait(&mut a, |a| {
        a.desktop.active_editor().is_some_and(|e| !e.busy())
    });
    editor::paste(&mut a, "draft");
    key(&mut a, K::F(8));
    assert!(matches!(
        a.desktop.active_editor().unwrap().modals.editor_modal(),
        Some(Modal::ConfirmClose { focus: 2 })
    ));
    key(&mut a, K::Esc);
    assert!(a
        .desktop
        .active_editor()
        .unwrap()
        .buffer
        .as_ref()
        .unwrap()
        .text
        .contains("draft"));
    key(&mut a, K::F(8));
    editor::action(&mut a, E::Discard);
    picker_ready(&mut a);
    editor::action(&mut a, E::EncodingAccept);
    wait(&mut a, |a| !a.desktop.active_editor().unwrap().busy());
    assert_eq!(
        a.desktop
            .active_editor()
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .text,
        "one\ntwo\nthree"
    );
    assert!(!a.desktop.active_editor().unwrap().dirty());
    assert!(a
        .desktop
        .active_editor()
        .unwrap()
        .buffer
        .as_ref()
        .unwrap()
        .anchor
        .is_none());
    assert_eq!(
        a.desktop
            .active_editor()
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .column(),
        3
    );
    editor::action(&mut a, E::Undo);
    assert_eq!(
        a.desktop
            .active_editor()
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .text,
        "one\ntwo\nthree"
    );
    assert_eq!(fs::read(p)?, b"one\ntwo\nthree");
    Ok(())
}
#[test]
fn initial_choice_cancel_retires_placeholder_and_binary_is_not_offered_as_text() -> io::Result<()> {
    let t = Temp::new()?;
    let p = t.0.join("unknown");
    fs::write(&p, [0xc0, 0xc1])?;
    let mut a = t.app()?;
    editor::open_path(&mut a, p);
    picker_ready(&mut a);
    key(&mut a, K::Esc);
    assert!(a.desktop.active_editor().is_none());
    let c = viewer::from_bytes(b"a\0b".to_vec(), ViewMode::Auto);
    assert!(c.binary);
    assert!(!c.encoding_needed);
    Ok(())
}
#[test]
fn stale_preview_choices_and_inactive_completions_do_not_steal_focus() -> io::Result<()> {
    let t = Temp::new()?;
    let p = t.0.join("legacy");
    fs::write(
        &p,
        C::Windows1251.default_format().encode("Привет").unwrap(),
    )?;
    let mut a = t.app()?;
    a.show_viewer(viewer::Viewer::open(p)?);
    wait(&mut a, |a| {
        a.desktop
            .active_viewer()
            .unwrap()
            .modals
            .viewer_encoding()
            .is_some()
    });
    for i in [3, 4, 5, 4, 3, 4] {
        viewer::action(&mut a, V::EncodingChoice(i));
    }
    wait(&mut a, |a| {
        !a.desktop
            .active_viewer()
            .unwrap()
            .modals
            .viewer_encoding()
            .unwrap()
            .busy()
    });
    assert_eq!(
        a.desktop
            .active_viewer()
            .unwrap()
            .modals
            .viewer_encoding()
            .unwrap()
            .preview,
        "Привет"
    );
    viewer::action(&mut a, V::EncodingAccept);
    let owner = a.desktop.active().id;
    a.desktop.activate(dn::desktop::WindowId::MANAGER);
    wait(&mut a, |a| {
        a.desktop
            .viewer(owner)
            .unwrap()
            .contents
            .as_ref()
            .is_some_and(|c| !c.binary)
    });
    assert_eq!(a.desktop.active().id, dn::desktop::WindowId::MANAGER);
    Ok(())
}
#[test]
fn unicode_markdown_preview_is_not_reinterpretable() -> io::Result<()> {
    let t = Temp::new()?;
    let p = t.0.join("source.md");
    fs::write(&p, "# Заголовок\n")?;
    let mut a = t.app()?;
    editor::open_path(&mut a, p);
    wait(&mut a, |a| {
        a.desktop.active_editor().is_some_and(|e| !e.busy())
    });
    editor::paste(&mut a, "unsaved\n");
    key(&mut a, K::F(5));
    assert!(!a.desktop.active_viewer().unwrap().encoding_available());
    key(&mut a, K::F(8));
    assert!(a
        .desktop
        .active_viewer()
        .unwrap()
        .modals
        .viewer_encoding()
        .is_none());
    key(&mut a, K::Esc);
    assert!(a.desktop.active_editor().unwrap().dirty());
    Ok(())
}
#[test]
fn recovery_keeps_legacy_format_and_bom() -> io::Result<()> {
    let t = Temp::new()?;
    let store = editor::recovery::store::Store::open(&t.0.join("recovery"))?;
    let p = store.current.dir.join("encoding.rec");
    for encoding in C::ALL {
        let record = editor::recovery::Record {
            text: "Привет\r\n".into(),
            path: None,
            base: t.0.clone(),
            cursor: 0,
            anchor: None,
            bom: encoding.unicode(),
            encoding,
            time: 1,
        };
        editor::recovery::store::write(&p, &record)?;
        let got = editor::recovery::store::read(&p)?;
        assert_eq!(got.encoding, encoding);
        assert_eq!(got.bom, record.bom);
        assert_eq!(got.text, record.text);
    }
    Ok(())
}
#[test]
fn viewer_utf16_auto_and_large_legacy_decoding_keep_raw_bytes() {
    let f = C::Utf16Le.default_format();
    let bytes = f.encode("# Заголовок\ntext").unwrap();
    let c = viewer::from_bytes(bytes.clone(), ViewMode::Auto);
    assert!(!c.binary);
    assert_eq!(c.format, Some(f));
    assert_eq!(c.lines[0], "# Заголовок");
    assert_eq!(c.raw.as_deref(), Some(bytes.as_slice()));
    let bytes: Arc<[u8]> = vec![0x80; viewer::MAX_BYTES].into();
    let c = viewer::from_encoded_bytes(bytes.clone(), C::Cp866.default_format(), false).unwrap();
    assert_eq!(c.source.as_ref().unwrap().len(), viewer::MAX_BYTES * 2);
    assert_eq!(c.raw, Some(bytes));
    assert!(c.truncated);
}

#[test]
fn encrypted_archive_markdown_uses_member_bytes_for_encoding_and_search() -> io::Result<()> {
    use std::io::Write;
    let t = Temp::new()?;
    let path = t.0.join("private.zip");
    let f = C::Windows1251.default_format();
    let bytes = f.encode("# Привет\n\nстрока **текста**\n").unwrap();
    let mut zip = zip::ZipWriter::new(fs::File::create(&path)?);
    zip.start_file(
        "README.MD",
        zip::write::SimpleFileOptions::default()
            .with_aes_encryption(zip::AesMode::Aes256, "fixture"),
    )?;
    zip.write_all(&bytes)?;
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
    input::paste(&mut a, "fixture");
    key(&mut a, K::Enter);
    wait(&mut a, |a| {
        a.desktop
            .active_viewer()
            .is_some_and(|v| v.modals.viewer_encoding().is_some())
    });
    viewer::action(&mut a, V::EncodingChoice(4));
    wait(&mut a, |a| {
        !a.desktop
            .active_viewer()
            .unwrap()
            .modals
            .viewer_encoding()
            .unwrap()
            .busy()
    });
    viewer::action(&mut a, V::EncodingAccept);
    wait(&mut a, |a| {
        a.desktop.active_viewer().unwrap().markdown.rendered
    });
    let v = a.desktop.active_viewer_mut().unwrap();
    assert_eq!(
        v.contents.as_ref().unwrap().raw.as_deref(),
        Some(bytes.as_slice())
    );
    v.layout(78);
    assert!(v
        .find(
            &viewer::SearchMemory {
                query: "текста".into(),
                case_sensitive: true
            },
            false,
            false
        )
        .unwrap());
    v.go_to("3").unwrap();
    key(&mut a, K::F(6));
    assert!(!a.desktop.active_viewer().unwrap().markdown.rendered);
    assert!(a
        .desktop
        .active_viewer()
        .unwrap()
        .contents
        .as_ref()
        .unwrap()
        .lines[2]
        .contains("**текста**"));
    assert!(a
        .file_tools
        .histories
        .get(dn::file_tools::history::HistoryKind::View)
        .is_empty());
    Ok(())
}

#[test]
fn old_recovery_records_default_to_utf8_without_changing_their_bom() -> io::Result<()> {
    let t = Temp::new()?;
    let store = editor::recovery::store::Store::open(&t.0.join("recovery"))?;
    let p = store.current.dir.join("old.rec");
    let record = editor::recovery::Record {
        text: "old\r\n".into(),
        path: None,
        base: t.0.clone(),
        cursor: 0,
        anchor: None,
        bom: true,
        encoding: C::Utf8,
        time: 1,
    };
    editor::recovery::store::write(&p, &record)?;
    let data = fs::read_to_string(&p)?;
    let body = data
        .splitn(3, '\n')
        .nth(2)
        .unwrap()
        .lines()
        .filter(|line| !line.starts_with("encoding ="))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let hash = body.as_bytes().iter().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    });
    fs::write(&p, format!("DNREC1\n{hash:016x}\n{body}"))?;
    let got = editor::recovery::store::read(&p)?;
    assert_eq!(got.encoding, C::Utf8);
    assert!(got.bom);
    assert_eq!(got.text, record.text);
    Ok(())
}

#[test]
fn failed_save_before_reopen_retains_unsaved_text_and_cancels_the_transition() -> io::Result<()> {
    let t = Temp::new()?;
    let p = t.0.join("conflict.txt");
    fs::write(&p, "original")?;
    let mut a = t.app()?;
    editor::open_path(&mut a, p.clone());
    wait(&mut a, |a| {
        a.desktop.active_editor().is_some_and(|e| !e.busy())
    });
    editor::paste(&mut a, "draft");
    fs::write(&p, "external")?;
    key(&mut a, K::F(8));
    editor::action(&mut a, E::Save);
    wait(&mut a, |a| !a.desktop.active_editor().unwrap().busy());
    let e = a.desktop.active_editor().unwrap();
    assert!(e.dirty());
    assert!(e.buffer.as_ref().unwrap().text.contains("draft"));
    assert!(e.modals.editor_modal().is_none());
    assert_eq!(fs::read(&p)?, b"external");
    a.clear_dialogs();
    a.poll();
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
fn untitled_save_conversion_cancel_keeps_utf8_and_live_buffer() -> io::Result<()> {
    let t = Temp::new()?;
    let mut a = t.app()?;
    editor::open(&mut a, true);
    editor::paste(&mut a, "Привет");
    editor::action(&mut a, E::SaveEncoding);
    editor::action(&mut a, E::EncodingChoice(3));
    editor::action(&mut a, E::EncodingAccept);
    assert!(matches!(
        a.desktop.active_editor().unwrap().modals.editor_modal(),
        Some(Modal::File(_))
    ));
    key(&mut a, K::Esc);
    let e = a.desktop.active_editor().unwrap();
    assert_eq!(e.text_format(), TextFormat::default());
    assert_eq!(e.buffer.as_ref().unwrap().text, "Привет");
    assert!(e.dirty());
    assert!(e.path.is_none());
    Ok(())
}

#[test]
fn escape_during_encoding_read_keeps_the_live_editor_and_rejects_late_results() -> io::Result<()> {
    let t = Temp::new()?;
    let p = t.0.join("cancel.txt");
    fs::write(&p, "original\n")?;
    let mut a = t.app()?;
    editor::open_path(&mut a, p);
    wait(&mut a, |a| {
        a.desktop.active_editor().is_some_and(|e| !e.busy())
    });
    let owner = a.desktop.active().id;
    key(&mut a, K::F(8));
    assert!(a.desktop.active_editor().unwrap().busy());
    key(&mut a, K::Esc);
    assert_eq!(a.desktop.active().id, owner);
    assert!(!a.desktop.active_editor().unwrap().busy());
    for _ in 0..4 {
        a.poll();
    }
    assert!(a
        .desktop
        .active_editor()
        .unwrap()
        .modals
        .editor_modal()
        .is_none());
    assert_eq!(
        a.desktop
            .active_editor()
            .unwrap()
            .buffer
            .as_ref()
            .unwrap()
            .text,
        "original\n"
    );
    Ok(())
}
