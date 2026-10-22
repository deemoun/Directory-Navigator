//! Separate, bounded UTF-8 score configuration; merge under the settings lock convention.
use super::Record;
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
    pub last_name: String,
}
impl Default for Board {
    fn default() -> Self {
        Self {
            records: Vec::new(),
            last_name: ANONYMOUS.into(),
        }
    }
}
impl Board {
    pub fn parse(bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() > 16 * 1024 {
            return Err(io::Error::other("Tetris configuration exceeds 16 KiB"));
        }
        let text = std::str::from_utf8(bytes).map_err(io::Error::other)?;
        let mut board = Self::default();
        let mut version = false;
        for line in text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
        {
            let (k, v) = line
                .split_once('=')
                .ok_or_else(|| io::Error::other("Invalid Tetris configuration record"))?;
            match k {
                "tetris_version" if v == "1" && !version => version = true,
                "last_name" if validate_name(v) => board.last_name = v.to_owned(),
                "score" => {
                    let fields = v.splitn(5, ',').collect::<Vec<_>>();
                    if fields.len() != 5 || board.records.len() >= 10 {
                        return Err(io::Error::other("Invalid Tetris score record"));
                    }
                    let score = fields[0].parse::<u64>().map_err(io::Error::other)?;
                    let lines = fields[1].parse::<u32>().map_err(io::Error::other)?;
                    let start_level = fields[2].parse::<u8>().map_err(io::Error::other)?;
                    let level = fields[3].parse::<u8>().map_err(io::Error::other)?;
                    if !validate_name(fields[4])
                        || score == 0
                        || !(1..=10).contains(&start_level)
                        || !(start_level..=10).contains(&level)
                    {
                        return Err(io::Error::other("Invalid Tetris score/level"));
                    }
                    board.records.push(Record {
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
        if !version {
            return Err(io::Error::other("Missing Tetris configuration version"));
        }
        board.records.sort_by_key(|r| std::cmp::Reverse(r.score));
        Ok(board)
    }
    fn encode(&self) -> Vec<u8> {
        let mut text = format!(
            "# Directory Navigator Tetris leaderboard\ntetris_version=1\nlast_name={}\n",
            self.last_name
        );
        for r in &self.records {
            text.push_str(&format!(
                "score={},{},{},{},{}\n",
                r.score, r.lines, r.start_level, r.level, r.name
            ));
        }
        text.into_bytes()
    }
}
#[derive(Default)]
pub struct Store {
    pub board: Board,
    path: Option<PathBuf>,
    loaded: bool,
    pending: Vec<Record>,
}
impl Store {
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }
    pub fn load(&mut self, path: Option<PathBuf>) -> io::Result<()> {
        if self.loaded && self.path == path {
            return Ok(());
        }
        self.path = path;
        self.loaded = false;
        let path = self
            .path
            .as_deref()
            .ok_or_else(|| io::Error::other("Tetris configuration directory is unavailable"))?;
        let mut board = read(path)?;
        if !self.pending.is_empty() {
            board.records.extend(self.pending.iter().cloned());
            board.records.sort_by_key(|r| std::cmp::Reverse(r.score));
            board.records.truncate(10);
            board.last_name = self.board.last_name.clone();
        }
        self.board = board;
        self.loaded = true;
        Ok(())
    }
    pub fn add(&mut self, record: Record) -> io::Result<()> {
        if !validate_name(&record.name)
            || record.score == 0
            || !(1..=10).contains(&record.start_level)
            || !(record.start_level..=10).contains(&record.level)
        {
            return Err(io::Error::other("Invalid Tetris record"));
        }
        self.board.last_name = record.name.clone();
        self.pending.push(record.clone());
        self.pending.sort_by_key(|r| std::cmp::Reverse(r.score));
        self.pending.truncate(10);
        self.board.records.push(record);
        self.board
            .records
            .sort_by_key(|r| std::cmp::Reverse(r.score));
        self.board.records.truncate(10);
        Ok(())
    }
    pub fn save(&mut self) -> io::Result<()> {
        let path = self
            .path
            .as_deref()
            .ok_or_else(|| io::Error::other("Tetris configuration directory is unavailable"))?;
        if self.pending.is_empty() {
            self.board = read(path)?;
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let _lock = crate::config::store::SaveLock::acquire(path)?;
        let mut disk = read(path)?;
        if let Ok(m) = fs::symlink_metadata(path) {
            if m.permissions().readonly() {
                return Err(io::Error::other("Tetris configuration is read-only"));
            }
        }
        disk.records.extend(self.pending.iter().cloned());
        disk.records.sort_by_key(|r| std::cmp::Reverse(r.score));
        disk.records.truncate(10);
        disk.last_name = self.board.last_name.clone();
        let mut staged = crate::publication::StagedFile::create(path)?;
        staged.file.write_all(&disk.encode())?;
        staged.file.sync_all()?;
        fs::rename(staged.path(), path)?;
        self.board = disk;
        self.pending.clear();
        crate::platform_fs::sync_parent(path)
    }
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
            lines: u32::MAX,
            start_level: 1,
            level: 10,
        }
    }
    #[test]
    fn readable_configuration_roundtrips_unicode_delimiters_and_integer_limits() {
        let board = Board {
            records: vec![record(u64::MAX, "Дмитрий,世界=#"), record(1, "é 👩‍💻")],
            last_name: "Дмитрий,世界=#".into(),
        };
        assert_eq!(Board::parse(&board.encode()).unwrap(), board);
    }
    #[test]
    fn parser_rejects_versions_overflow_missing_fields_and_controls() {
        for text in [
            "",
            "tetris_version=1\ntetris_version=1\n",
            "tetris_version=2\n",
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
        assert_eq!(store.pending.len(), 10);
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
