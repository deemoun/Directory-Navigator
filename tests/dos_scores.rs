use dn::testing::{backend::TestBackend, Terminal};
use dn::{
    app::App,
    config::Store as ConfigStore,
    tetris::{
        self,
        dos_scores::{decode, Encoding},
        scores::{Board, Store},
        GameAction, Record,
    },
    ui,
};
use std::{
    fs, io,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct Temp(PathBuf);
impl Temp {
    fn new() -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "dn-dos-scores-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
    fn path(&self) -> PathBuf {
        self.0.join("tetris.conf")
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
// Independent fixtures mirror the Pascal short-string, byte levels, LE LongInt
// and countdown XOR loop. No DOS runtime or original binary is needed.
fn plain(width: usize) -> Vec<u8> {
    let mut bytes = vec![0; 20 * width];
    for record in bytes.chunks_exact_mut(width) {
        record[0] = 9;
        record[1..10].copy_from_slice(b"Anonymous");
        record[31] = 1;
        record[32] = 1;
    }
    bytes
}
fn set(bytes: &mut [u8], width: usize, slot: usize, name: &[u8], score: i32, start: u8, end: u8) {
    let raw = &mut bytes[slot * width..(slot + 1) * width];
    raw[0] = name.len() as u8;
    raw[1..1 + name.len()].copy_from_slice(name);
    raw[31] = start;
    raw[32] = end;
    raw[width - 4..].copy_from_slice(&score.to_le_bytes());
}
fn encoded(mut bytes: Vec<u8>) -> Vec<u8> {
    let length = bytes.len();
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte ^= ((length - i) & 255) as u8 ^ 0xaa;
    }
    bytes
}
fn sample(width: usize) -> Vec<u8> {
    let mut bytes = plain(width);
    set(&mut bytes, width, 0, b"Caf\x82", 500, 2, 7);
    set(&mut bytes, width, 1, b"Same", 100, 1, 3);
    set(&mut bytes, width, 10, b"Pentix", i32::MAX, 1, 10);
    encoded(bytes)
}
fn modern(score: u64, name: &str) -> Record {
    Record {
        name: name.into(),
        score,
        lines: Some(4),
        start_level: 1,
        level: 2,
    }
}
#[test]
fn packed_and_aligned_dos_tables_decode_scores_levels_names_and_ignore_pentix() -> io::Result<()> {
    for width in [37, 38] {
        let bytes = sample(width);
        let records = decode(&bytes, Encoding::Cp437)?;
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].name, "Café");
        assert_eq!(records[0].score, 500);
        assert_eq!((records[0].start_level, records[0].level), (2, 7));
        assert_eq!(records[0].lines, None);
        assert!(records.iter().all(|r| r.name != "Pentix"));
    }
    Ok(())
}
#[test]
fn code_pages_empty_tables_and_safe_graphical_controls_are_explicit() -> io::Result<()> {
    let mut bytes = plain(37);
    set(
        &mut bytes,
        37,
        0,
        &[0x84, 0xac, 0xa8, 0xe2, 0xe0, 0xa8, 0xa9],
        123,
        1,
        2,
    );
    assert_eq!(decode(&encoded(bytes), Encoding::Cp866)?[0].name, "Дмитрий");
    assert!(decode(&plain(37), Encoding::Cp437)?.is_empty());
    assert!(decode(&encoded(plain(37)), Encoding::Cp437)?.is_empty());
    let mut bytes = plain(37);
    set(&mut bytes, 37, 0, &[27], 1, 1, 1);
    let name = decode(&encoded(bytes), Encoding::Cp437)?[0].name.clone();
    assert!(!name.chars().any(char::is_control));
    Ok(())
}
#[test]
fn malformed_truncated_oversized_and_unencoded_scored_files_are_refused() {
    for bytes in [
        vec![],
        vec![0; 739],
        vec![0; 741],
        vec![0; 761],
        vec![0xff; 740],
    ] {
        assert!(decode(&bytes, Encoding::Cp437).is_err());
    }
    for (offset, value) in [(0, 31), (31, 0), (32, 11)] {
        let mut bytes = plain(37);
        bytes[offset] = value;
        assert!(decode(&encoded(bytes), Encoding::Cp437).is_err());
    }
    let mut bytes = plain(37);
    set(&mut bytes, 37, 0, b"negative", -1, 1, 1);
    assert!(decode(&encoded(bytes), Encoding::Cp437).is_err());
    let mut bytes = plain(37);
    set(&mut bytes, 37, 0, b"unencoded", 42, 1, 1);
    assert!(decode(&bytes, Encoding::Cp437).is_err());
}
#[test]
fn copied_files_merge_once_with_existing_unicode_scores_and_survive_restart() -> io::Result<()> {
    let t = Temp::new()?;
    let path = t.path();
    let mut store = Store::default();
    store.load(Some(path.clone()))?;
    store.add(modern(1000, "世界 👩‍💻"))?;
    store.save()?;
    let source = sample(37);
    fs::write(t.0.join("dn.tet"), &source)?;
    store.load(Some(path.clone()))?;
    assert_eq!(store.board.records.len(), 3);
    assert_eq!(store.board.last_name, "世界 👩‍💻");
    let saved = fs::read(&path)?;
    assert_eq!(Board::parse(&saved)?, store.board);
    assert!(String::from_utf8_lossy(&saved).contains("tetris_version=3"));
    for _ in 0..3 {
        store.load(Some(path.clone()))?;
        store.save()?;
        assert_eq!(fs::read(&path)?, saved);
    }
    let mut another = Store::default();
    another.load_with_encoding(Some(path), Encoding::Cp866)?;
    assert_eq!(another.board, store.board); // Changing UI language cannot reimport names.
    assert_eq!(another.board.dos_encoding, Some(Encoding::Cp437));
    assert_eq!(fs::read(t.0.join("dn.tet"))?, source);
    Ok(())
}
#[test]
fn imports_preserve_duplicate_slots_and_concurrent_native_records_without_reimporting(
) -> io::Result<()> {
    let t = Temp::new()?;
    let mut bytes = plain(37);
    for slot in 0..2 {
        set(&mut bytes, 37, slot, b"Same", 100, 1, 2);
    }
    fs::write(t.0.join("DN.TET"), encoded(bytes))?;
    let mut a = Store::default();
    let mut b = Store::default();
    a.load(Some(t.path()))?;
    b.load(Some(t.path()))?;
    assert_eq!(a.board.records.len(), 2);
    assert_eq!(b.board.records.len(), 2);
    a.add(modern(100, "Same"))?;
    b.add(modern(200, "Other"))?;
    a.save()?;
    b.save()?;
    a.load(Some(t.path()))?;
    assert_eq!(a.board.records.len(), 4);
    assert_eq!(
        a.board.records.iter().filter(|r| r.lines.is_none()).count(),
        2
    );
    let mut changed = plain(37);
    set(&mut changed, 37, 0, b"Added later", 300, 1, 2);
    fs::write(t.0.join("DN.TET"), encoded(changed))?;
    a.load(Some(t.path()))?;
    assert_eq!(a.board.records[0].name, "Added later");
    assert_eq!(a.board.records.len(), 5);
    Ok(())
}
#[test]
fn bad_import_does_not_hide_or_overwrite_modern_scores_and_bad_modern_is_not_replaced(
) -> io::Result<()> {
    let t = Temp::new()?;
    let mut a = Store::default();
    a.load(Some(t.path()))?;
    a.add(modern(1000, "Keep me"))?;
    a.save()?;
    let before = fs::read(t.path())?;
    fs::write(t.0.join("dn.tet"), b"invalid")?;
    let mut b = Store::default();
    assert!(b.load(Some(t.path())).is_err());
    assert_eq!(b.board.records[0].name, "Keep me");
    assert_eq!(fs::read(t.path())?, before);
    fs::write(t.0.join("dn.tet"), sample(37))?;
    fs::write(t.path(), b"bad modern file")?;
    assert!(b.load(Some(t.path())).is_err());
    assert_eq!(fs::read(t.path())?, b"bad modern file");
    Ok(())
}
#[test]
fn read_only_and_link_inputs_keep_source_untouched_and_failed_import_can_retry() -> io::Result<()> {
    let t = Temp::new()?;
    let source = sample(37);
    let original = t.0.join("dn.tet");
    fs::write(&original, &source)?;
    let mut permissions = fs::metadata(&original)?.permissions();
    let previous = permissions.clone();
    permissions.set_readonly(true);
    fs::set_permissions(&original, permissions)?;
    let mut a = Store::default();
    a.load(Some(t.path()))?;
    assert_eq!(a.board.records.len(), 2);
    assert_eq!(fs::read(&original)?, source);
    fs::set_permissions(&original, previous)?;
    #[cfg(unix)]
    {
        fs::remove_file(&original)?;
        std::os::unix::fs::symlink(t.0.join("missing"), &original)?;
        assert!(a.load(Some(t.path())).is_err());
        assert_eq!(a.board.records.len(), 2);
        fs::remove_file(&original)?;
    }
    fs::write(&original, sample(37))?;
    fs::remove_file(t.path())?;
    fs::write(t.0.join("tetris.conf.lock"), b"locked")?;
    assert!(a.load(Some(t.path())).is_err());
    assert!(a.has_pending());
    assert_eq!(a.board.records.len(), 2);
    assert!(!t.path().exists());
    fs::remove_file(t.0.join("tetris.conf.lock"))?;
    a.save()?;
    assert!(!a.has_pending());
    assert_eq!(Board::parse(&fs::read(t.path())?)?.records.len(), 2);
    Ok(())
}
#[test]
fn game_import_uses_selected_language_and_shows_unknown_lines_at_both_sizes() -> io::Result<()> {
    let t = Temp::new()?;
    let mut bytes = plain(37);
    set(
        &mut bytes,
        37,
        0,
        &[0x84, 0xac, 0xa8, 0xe2, 0xe0, 0xa8, 0xa9],
        123,
        1,
        2,
    );
    fs::write(t.0.join("dn.tet"), encoded(bytes))?;
    let mut app = App::new(t.0.clone())?;
    app.config_store = ConfigStore::new(Some(t.0.join("config.conf")));
    app.config.language = dn::localization::Language::Russian;
    tetris::open(&mut app);
    assert_eq!(app.desktop.game().unwrap().records[0].name, "Дмитрий");
    tetris::action(&mut app, GameAction::Scores);
    for (w, h) in [(80, 25), (120, 40), (80, 25)] {
        let mut terminal = Terminal::new(TestBackend::new(w, h))?;
        terminal.draw(|f| ui::draw(f, &mut app))?;
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        assert!(text.contains("Дмитрий"));
        assert!(text.contains('—'));
        assert!(app
            .mouse
            .regions
            .iter()
            .all(|(r, _)| r.right() <= w && r.bottom() <= h));
    }
    assert!(!t.0.join("config.conf").exists());
    Ok(())
}

#[test]
fn significant_dos_name_spaces_survive_storage_and_do_not_duplicate() -> io::Result<()> {
    let t = Temp::new()?;
    let mut bytes = plain(37);
    set(&mut bytes, 37, 0, b"  Player  ", 200, 1, 2);
    fs::write(t.0.join("dn.tet"), encoded(bytes))?;
    let mut store = Store::default();
    for _ in 0..3 {
        store.load(Some(t.path()))?;
        assert_eq!(store.board.records.len(), 1);
        assert_eq!(store.board.records[0].name, "  Player  ");
        assert_eq!(Board::parse(&fs::read(t.path())?)?, store.board);
    }
    Ok(())
}

#[test]
fn both_dos_leaderboards_import_once_including_existing_v2_migrations() -> io::Result<()> {
    use tetris::{dos_scores::decode_all, GameMode};
    for width in [37, 38] {
        let t = Temp::new()?;
        let bytes = sample(width);
        let decoded = decode_all(&bytes, Encoding::Cp437)?;
        assert_eq!(decoded[0].len(), 2);
        assert_eq!(decoded[1].len(), 1);
        fs::write(t.0.join("dn.tet"), &bytes)?;
        // An earlier DN build already imported the Classic half.
        fs::write(t.path(),"tetris_version=2\nlast_name=Kept\ndn_tet_encoding=cp437\nscore=500,-,2,7,Café\nscore=100,-,1,3,Same\n")?;
        let mut store = Store::default();
        store.load(Some(t.path()))?;
        assert_eq!(store.board.records.len(), 2);
        assert_eq!(store.board.records_for(GameMode::Pentix)[0].name, "Pentix");
        assert_eq!(store.board.last_name, "Kept");
        let saved = fs::read(t.path())?;
        assert!(String::from_utf8_lossy(&saved).contains("pentix_score="));
        store.load(Some(t.path()))?;
        store.save()?;
        assert_eq!(fs::read(t.path())?, saved);
        assert_eq!(fs::read(t.0.join("dn.tet"))?, bytes);
    }
    Ok(())
}

#[test]
fn v3_modes_merge_independently_and_failed_preferences_retry_after_reopen() -> io::Result<()> {
    use tetris::{GameMode, Preferences};
    let t = Temp::new()?;
    fs::write(
        t.path(),
        "tetris_version=1\nlast_name=Legacy\nscore=50,2,1,2,Old\n",
    )?;
    let mut a = Store::default();
    let mut b = Store::default();
    a.load(Some(t.path()))?;
    b.load(Some(t.path()))?;
    a.add_for(GameMode::Pentix, modern(500, "Пентикс 世界"))?;
    let prefs = Preferences {
        mode: GameMode::Pentix,
        level: 7,
        preview: false,
    };
    a.set_preferences(prefs);
    fs::write(t.0.join("tetris.conf.lock"), b"busy")?;
    assert!(a.save().is_err());
    assert!(a.has_pending());
    a.load(Some(t.path()))?;
    assert_eq!(a.board.preferences, prefs);
    assert_eq!(a.board.pentix_records[0].score, 500);
    fs::remove_file(t.0.join("tetris.conf.lock"))?;
    b.add(modern(100, "Classic"))?;
    b.save()?;
    a.save()?;
    let parsed = Board::parse(&fs::read(t.path())?)?;
    assert_eq!(parsed.preferences, prefs);
    assert_eq!(parsed.records.len(), 2);
    assert_eq!(parsed.records[0].score, 100);
    assert_eq!(parsed.pentix_records.len(), 1);
    assert_eq!(parsed.pentix_records[0].score, 500);
    assert!(!a.has_pending());
    let bytes = fs::read(t.path())?;
    a.save()?;
    assert_eq!(bytes, fs::read(t.path())?);
    for bad in [
        "mode=other",
        "start_level=0",
        "start_level=11",
        "preview=yes",
        "pentix_score=10,-,8,2,bad",
    ] {
        assert!(Board::parse(format!("tetris_version=3\n{bad}\n").as_bytes()).is_err());
    }
    Ok(())
}
