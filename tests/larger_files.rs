use dn::{
    editor::{buffer::Buffer, storage},
    text_encoding::TextFormat,
    viewer,
};

#[test]
fn files_above_old_beta_limit_are_viewable_and_editable() {
    let text = format!("{}\n", "a".repeat(95)).repeat(32768);
    let contents = viewer::from_bytes(text.as_bytes().to_vec(), viewer::ViewMode::Auto);
    assert!(!contents.truncated);
    assert_eq!(contents.source.as_deref(), Some(text.as_str()));
    assert!(Buffer::new(text).is_ok());
}

#[test]
fn raised_limits_are_consistent() {
    assert_eq!(viewer::MAX_BYTES, 64 * 1024 * 1024);
    assert_eq!(dn::editor::buffer::MAX_BYTES, 16 * 1024 * 1024);
    assert_eq!(dn::editor::buffer::MAX_LINES, 250_000);
}

#[test]
fn larger_editor_roundtrip_preserves_format() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::temp_dir().join(format!("dn-larger-roundtrip-{}", std::process::id()));
    std::fs::create_dir(&root)?;
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(root.clone());
    let path = root.join("large.txt");
    let text = "Привет\r\ntext\n".repeat(180_000 / 2);
    let text = format!("{}{}", "x".repeat(3 * 1024 * 1024), text);
    let outcome = storage::save(&path, &text, TextFormat::default(), None)?;
    assert!(outcome.warning.is_none());
    let (buffer, _, format) = storage::load(&path)?;
    assert_eq!(buffer.text, text);
    assert_eq!(format, TextFormat::default());
    Ok(())
}

mod support;
#[test]
fn larger_utf16_document_and_recovery_keep_bom_and_mixed_endings(
) -> Result<(), Box<dyn std::error::Error>> {
    use dn::{
        editor::recovery::{store, Record},
        text_encoding::TextEncoding,
    };
    let f = support::Fixture::new("large-unicode-recovery")?.canonicalized()?;
    let text = "Привет e\u{301}\t\r\nline\n".repeat(100_000);
    assert!(text.len() > 2 * 1024 * 1024);
    let format = TextFormat {
        encoding: TextEncoding::Utf16Be,
        bom: true,
    };
    let path = f.0.join("utf16.txt");
    storage::save(&path, &text, format, None)?;
    let (loaded, _, actual) = storage::load(&path)?;
    assert_eq!(loaded.text, text);
    assert_eq!(actual, format);
    let session = store::Store::open(&f.0.join("recovery"))?;
    let checkpoint = session.current.dir.join("large.rec");
    let record = Record {
        text,
        path: Some(path),
        base: f.0.clone(),
        cursor: 0,
        anchor: None,
        bom: true,
        encoding: TextEncoding::Utf16Be,
        time: 1_700_000_000,
    };
    store::write(&checkpoint, &record)?;
    let restored = store::read(&checkpoint)?;
    assert_eq!(restored.text, record.text);
    assert_eq!(restored.encoding, record.encoding);
    assert!(restored.bom);
    Ok(())
}
