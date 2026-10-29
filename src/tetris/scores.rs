//! Separate, bounded UTF-8 score configuration; merge under the settings lock convention.
use super::{GameMode, Preferences, Record};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};
use unicode_segmentation::UnicodeSegmentation;
pub const ANONYMOUS: &str = "Anonymous";
pub fn validate_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 240
        && name.graphemes(true).count() <= 30
        && !name.chars().any(char::is_control)
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Board {
    pub records: Vec<Record>,
    pub pentix_records: Vec<Record>,
    pub preferences: Preferences,
    pub last_name: String,
    pub dos_encoding: Option<super::dos_scores::Encoding>,
}
impl Default for Board {
    fn default() -> Self {
        Self {
            records: Vec::new(),
            pentix_records: Vec::new(),
            preferences: Preferences::default(),
            last_name: ANONYMOUS.into(),
            dos_encoding: None,
        }
    }
}
impl Board {
    pub fn records_for(&self, mode: GameMode) -> &Vec<Record> {
        match mode {
            GameMode::Tetris => &self.records,
            GameMode::Pentix => &self.pentix_records,
        }
    }
    fn records_mut(&mut self, mode: GameMode) -> &mut Vec<Record> {
        match mode {
            GameMode::Tetris => &mut self.records,
            GameMode::Pentix => &mut self.pentix_records,
        }
    }

    pub fn parse(bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() > 16 * 1024 {
            return Err(io::Error::other("Tetris configuration exceeds 16 KiB"));
        }
        let text = std::str::from_utf8(bytes).map_err(io::Error::other)?;
        let mut board = Self::default();
        let mut version = None;
        for line in text
            .lines()
            .map(|line| line.trim_start().trim_end_matches('\r'))
            .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        {
            let (k, raw_value) = line
                .split_once('=')
                .ok_or_else(|| io::Error::other("Invalid Tetris configuration record"))?;
            // DOS names may have significant trailing spaces. Metadata can be
            // trimmed, but names must survive serialization and repeated import.
            let v = if matches!(k, "score" | "pentix_score" | "last_name") {
                raw_value
            } else {
                raw_value.trim()
            };
            match k {
                "tetris_version" if matches!(v, "1" | "2" | "3") && version.is_none() => {
                    version = Some(v)
                }
                "last_name" if validate_name(v) => board.last_name = v.to_owned(),
                "dn_tet_encoding"
                    if matches!(version, Some("2" | "3")) && board.dos_encoding.is_none() =>
                {
                    board.dos_encoding = Some(super::dos_scores::Encoding::parse(v)?)
                }
                "mode" if version == Some("3") => {
                    board.preferences.mode = match v {
                        "tetris" => GameMode::Tetris,
                        "pentix" => GameMode::Pentix,
                        _ => return Err(io::Error::other("Invalid game mode")),
                    }
                }
                "start_level" if version == Some("3") => {
                    board.preferences.level = v.parse().map_err(io::Error::other)?;
                    if !(1..=10).contains(&board.preferences.level) {
                        return Err(io::Error::other("Invalid game level"));
                    }
                }
                "preview" if version == Some("3") => {
                    board.preferences.preview = match v {
                        "true" => true,
                        "false" => false,
                        _ => return Err(io::Error::other("Invalid preview option")),
                    }
                }
                "score" | "pentix_score" => {
                    if k == "pentix_score" && version != Some("3") {
                        return Err(io::Error::other("Pentix requires game format v3"));
                    }
                    let mode = if k == "pentix_score" {
                        GameMode::Pentix
                    } else {
                        GameMode::Tetris
                    };

                    let fields = v.splitn(5, ',').collect::<Vec<_>>();
                    if fields.len() != 5 || board.records_for(mode).len() >= 10 {
                        return Err(io::Error::other("Invalid Tetris score record"));
                    }
                    let score = fields[0].parse::<u64>().map_err(io::Error::other)?;
                    let lines = if fields[1] == "-" && matches!(version, Some("2" | "3")) {
                        None
                    } else {
                        Some(fields[1].parse::<u32>().map_err(io::Error::other)?)
                    };
                    let start_level = fields[2].parse::<u8>().map_err(io::Error::other)?;
                    let level = fields[3].parse::<u8>().map_err(io::Error::other)?;
                    if !validate_name(fields[4])
                        || score == 0
                        || !(1..=10).contains(&start_level)
                        || !(start_level..=10).contains(&level)
                    {
                        return Err(io::Error::other("Invalid Tetris score/level"));
                    }
                    board.records_mut(mode).push(Record {
                        score,
                        lines,
                        start_level,
                        level,
                        name: fields[4].to_owned(),
                    });
                }
                _ => return Err(io::Error::other("Unsupported Tetris configuration record")),
            }
        }
        if version.is_none() {
            return Err(io::Error::other("Missing Tetris configuration version"));
        }
        for mode in GameMode::ALL {
            board
                .records_mut(mode)
                .sort_by_key(|r| std::cmp::Reverse(r.score));
        }
        Ok(board)
    }
    fn encode(&self) -> Vec<u8> {
        let mut text = format!(
            "# Directory Navigator game leaderboards\ntetris_version=3\nlast_name={}\nmode={}\nstart_level={}\npreview={}\n",
            self.last_name, self.preferences.mode.key(), self.preferences.level, self.preferences.preview
        );
        if let Some(encoding) = self.dos_encoding {
            text.push_str(&format!("dn_tet_encoding={}\n", encoding.key()));
        }
        for mode in GameMode::ALL {
            for r in self.records_for(mode) {
                text.push_str(&format!(
                    "{}={},{},{},{},{}\n",
                    if mode == GameMode::Tetris {
                        "score"
                    } else {
                        "pentix_score"
                    },
                    r.score,
                    r.lines
                        .map_or_else(|| "-".into(), |lines| lines.to_string()),
                    r.start_level,
                    r.level,
                    r.name
                ));
            }
        }
        text.into_bytes()
    }
}
#[derive(Default)]
pub struct Store {
    pub board: Board,
    path: Option<PathBuf>,
    imports: [Vec<Record>; 2],
    import_pending: bool,
    pending: [Vec<Record>; 2],
    preferences_pending: Option<Preferences>,
    sync_pending: bool,
}
impl Store {
    pub fn has_pending(&self) -> bool {
        self.pending.iter().any(|p| !p.is_empty())
            || self.import_pending
            || self.preferences_pending.is_some()
            || self.sync_pending
    }
    pub fn set_preferences(&mut self, preferences: Preferences) {
        self.board.preferences = preferences;
        self.preferences_pending = Some(preferences);
    }
    pub fn load(&mut self, path: Option<PathBuf>) -> io::Result<()> {
        self.load_with_encoding(path, super::dos_scores::Encoding::Cp437)
    }
    pub fn load_with_encoding(
        &mut self,
        path: Option<PathBuf>,
        encoding: super::dos_scores::Encoding,
    ) -> io::Result<()> {
        self.path = path;
        self.imports = Default::default();
        self.import_pending = false;
        let path = self
            .path
            .as_deref()
            .ok_or_else(|| io::Error::other("Tetris configuration directory is unavailable"))?;
        let mut board = read(path)?;
        if self.pending.iter().any(|p| !p.is_empty()) {
            board.last_name = self.board.last_name.clone();
        }
        for mode in GameMode::ALL {
            let records = board.records_mut(mode);
            records.extend(self.pending[mode.index()].iter().cloned());
            records.sort_by_key(|r| std::cmp::Reverse(r.score));
            records.truncate(10);
        }
        if let Some(preferences) = self.preferences_pending {
            board.preferences = preferences;
        }
        self.board = board;
        let encoding = self.board.dos_encoding.unwrap_or(encoding);
        self.imports = super::dos_scores::read(path, encoding)?;
        let before = self.board.clone();
        for mode in GameMode::ALL {
            merge_imports(self.board.records_mut(mode), &self.imports[mode.index()]);
        }
        if self.imports.iter().any(|r| !r.is_empty()) {
            self.board.dos_encoding = Some(encoding);
        }
        self.import_pending = self.board != before;
        if self.import_pending
            && self.pending.iter().all(Vec::is_empty)
            && self.preferences_pending.is_none()
        {
            self.save()?;
        }
        Ok(())
    }
    pub fn add(&mut self, record: Record) -> io::Result<()> {
        self.add_for(GameMode::Tetris, record)
    }
    pub fn add_for(&mut self, mode: GameMode, record: Record) -> io::Result<()> {
        if !validate_name(&record.name)
            || record.score == 0
            || !(1..=10).contains(&record.start_level)
            || !(record.start_level..=10).contains(&record.level)
        {
            return Err(io::Error::other("Invalid Tetris record"));
        }
        self.board.last_name = record.name.clone();
        let pending = &mut self.pending[mode.index()];
        pending.push(record.clone());
        pending.sort_by_key(|r| std::cmp::Reverse(r.score));
        pending.truncate(10);
        let records = self.board.records_mut(mode);
        records.push(record);
        records.sort_by_key(|r| std::cmp::Reverse(r.score));
        records.truncate(10);
        Ok(())
    }
    pub fn save(&mut self) -> io::Result<()> {
        let path = self
            .path
            .as_deref()
            .ok_or_else(|| io::Error::other("Tetris configuration directory is unavailable"))?;
        if !self.has_pending() {
            self.board = read(path)?;
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let _lock = crate::config::store::SaveLock::acquire(path)?;
        let mut disk = read(path)?;
        let original = disk.clone();
        if let Some(encoding) = disk
            .dos_encoding
            .filter(|e| Some(*e) != self.board.dos_encoding)
        {
            self.imports = super::dos_scores::read(path, encoding)?;
        }
        for mode in GameMode::ALL {
            let records = disk.records_mut(mode);
            merge_imports(records, &self.imports[mode.index()]);
            records.extend(self.pending[mode.index()].iter().cloned());
            records.sort_by_key(|r| std::cmp::Reverse(r.score));
            records.truncate(10);
        }
        disk.dos_encoding = disk.dos_encoding.or(self.board.dos_encoding);
        if self.pending.iter().any(|p| !p.is_empty()) {
            disk.last_name = self.board.last_name.clone();
        }
        if let Some(preferences) = self.preferences_pending {
            disk.preferences = preferences;
        }
        if disk != original {
            if let Ok(m) = fs::symlink_metadata(path) {
                if m.permissions().readonly() {
                    return Err(io::Error::other("Tetris configuration is read-only"));
                }
            }
            let mut staged = crate::publication::StagedFile::create(path)?;
            staged.file.write_all(&disk.encode())?;
            staged.file.sync_all()?;
            fs::rename(staged.path(), path)?;
            // Publication succeeded: retries must not duplicate already published records,
            // even when syncing the parent directory fails.
            self.board = disk;
            self.pending = Default::default();
            self.import_pending = false;
            self.preferences_pending = None;
            self.sync_pending = true;
            let result = crate::platform_fs::sync_parent(path);
            if result.is_ok() {
                self.sync_pending = false;
            }
            return result;
        }
        self.board = disk;
        self.pending = Default::default();
        self.import_pending = false;
        self.preferences_pending = None;
        if self.sync_pending {
            crate::platform_fs::sync_parent(path)?;
            self.sync_pending = false;
        }
        Ok(())
    }
}
// Imported entries have unknown line counts. Preserve duplicate slots from the
// original table, but do not insert those same slots again on later opens/saves.
fn merge_imports(records: &mut Vec<Record>, imported: &[Record]) {
    for (index, record) in imported.iter().enumerate() {
        let required = imported[..=index].iter().filter(|r| *r == record).count();
        if records.iter().filter(|r| *r == record).count() < required {
            records.push(record.clone());
        }
    }
    records.sort_by_key(|r| std::cmp::Reverse(r.score));
    records.truncate(10);
}
fn read(path: &Path) -> io::Result<Board> {
    crate::config::store::read_bytes(path)?
        .map_or_else(|| Ok(Board::default()), |b| Board::parse(&b))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record(score: u64, name: &str) -> Record {
        Record {
            score,
            name: name.into(),
            lines: Some(u32::MAX),
            start_level: 1,
            level: 10,
        }
    }
    #[test]
    fn readable_configuration_roundtrips_unicode_delimiters_and_integer_limits() {
        let board = Board {
            records: vec![record(u64::MAX, "Дмитрий,世界=#"), record(1, "é 👩‍💻")],
            last_name: "Дмитрий,世界=#".into(),
            dos_encoding: None,
            ..Board::default()
        };
        assert_eq!(Board::parse(&board.encode()).unwrap(), board);
    }
    #[test]
    fn parser_rejects_versions_overflow_missing_fields_and_controls() {
        for text in [
            "",
            "tetris_version=1\ntetris_version=1\n",
            "tetris_version=4\n",
            "tetris_version=1\nfuture=1\n",
            "tetris_version=1\nscore=1,2,3,4\n",
            "tetris_version=1\nscore=0,0,1,1,zero\n",
            "tetris_version=1\nscore=1,4294967296,1,1,overflow\n",
            "tetris_version=1\nscore=1,0,5,4,reversed\n",
            "tetris_version=1\nlast_name=bad\x1bname\n",
        ] {
            assert!(Board::parse(text.as_bytes()).is_err(), "{text:?}");
        }
        assert!(Board::parse(&[0xff, 0xfe]).is_err());
        assert!(Board::parse(&vec![b' '; 16 * 1024 + 1]).is_err());
    }
    #[test]
    fn name_limits_count_graphemes_and_utf8_bytes_independently() {
        assert!(validate_name(&"界".repeat(30)));
        assert!(!validate_name(&"界".repeat(31)));
        assert!(validate_name(&"é".repeat(30)));
        assert!(!validate_name(&"a".repeat(240)));
        // One grapheme can exceed the byte bound through many combining marks.
        assert!(!validate_name(&format!("a{}", "\u{301}".repeat(121))));
        for bad in ["", "a\nb", "a\rb", "a\tb", "a\0b"] {
            assert!(!validate_name(bad));
        }
    }
    #[test]
    fn top_ten_retains_stable_ties_and_legitimate_duplicate_results() {
        let mut store = Store::default();
        for score in 1..=20 {
            store.add(record(score, &format!("player{score}"))).unwrap();
        }
        assert_eq!(
            store
                .board
                .records
                .iter()
                .map(|r| r.score)
                .collect::<Vec<_>>(),
            (11..=20).rev().collect::<Vec<_>>()
        );
        store.add(record(20, "new tie")).unwrap();
        assert_eq!(store.board.records[0].name, "player20");
        assert_eq!(store.board.records[1].name, "new tie");
        store.add(record(20, "new tie")).unwrap();
        assert_eq!(
            store
                .board
                .records
                .iter()
                .filter(|r| r.name == "new tie")
                .count(),
            2
        );
        assert_eq!(store.pending[0].len(), 10);
    }
    #[test]
    fn rejected_records_leave_all_pending_and_display_state_unchanged() {
        let mut store = Store::default();
        store.add(record(10, "keep")).unwrap();
        let before = (store.board.clone(), store.pending.clone());
        let mut bad = record(11, "bad");
        bad.start_level = 0;
        for r in [record(0, "zero"), record(10, ""), bad] {
            assert!(store.add(r).is_err());
            assert_eq!((&store.board, &store.pending), (&before.0, &before.1));
        }
    }
    #[test]
    fn parser_refuses_an_eleventh_record_instead_of_silently_discarding_it() {
        let mut text = "tetris_version=1\n".to_owned();
        for i in 1..=11 {
            text.push_str(&format!("score={i},0,1,1,name\n"));
        }
        assert!(Board::parse(text.as_bytes()).is_err());
    }
}
