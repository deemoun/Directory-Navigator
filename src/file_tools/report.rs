//! Read-only, bounded directory comparison, owned by the existing tool document.
use crate::{
    platform_fs,
    requests::RequestScope,
    task::{Cancellation, Task},
};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
pub const LIMIT: usize = 20_000;
const DEPTH: usize = 512;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Left,
    Right,
    Different,
    Identical,
    Unknown,
}
impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Self::Left => "Left only",
            Self::Right => "Right only",
            Self::Different => "Different",
            Self::Identical => "Identical",
            Self::Unknown => "Not compared",
        }
    }
}
#[derive(Debug)]
pub struct Row {
    pub path: PathBuf,
    pub status: Status,
    pub present: [bool; 2],
    pub cause: String,
}
#[derive(Default)]
pub struct Result {
    pub entries: Vec<Row>,
    pub partial: bool,
    pub notice: String,
}
pub struct Report {
    pub(crate) requests: RequestScope,
    pub roots: [PathBuf; 2],
    pub result: Result,
    pub cursor: usize,
    pub offset: usize,
    pub rows: usize,
    pub focus: usize,
    pub last_click: Option<(usize, std::time::Instant)>,
    stop: Cancellation,
    count: Arc<AtomicUsize>,
    seen: usize,
    pending: Option<Task<Result>>,
}
impl Report {
    pub fn open(roots: [PathBuf; 2], owner: crate::desktop::WindowId) -> io::Result<Self> {
        let requests = RequestScope::new(crate::requests::Owner::Document(owner));
        let stop = Cancellation::default();
        let count = Arc::new(AtomicUsize::new(0));
        let input = roots.clone();
        let stopped = stop.clone();
        let progress = count.clone();
        let task = requests.spawn("dn-compare-report", move |cancel| {
            scan(input, &cancel, &stopped, &progress, LIMIT, DEPTH)
        })?;
        Ok(Self {
            requests,
            roots,
            result: Result::default(),
            cursor: 0,
            offset: 0,
            rows: 10,
            focus: 0,
            last_click: None,
            stop,
            count,
            seen: 0,
            pending: Some(task),
        })
    }
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }
    pub fn scanned(&self) -> usize {
        self.seen
    }
    pub fn stop(&self) {
        self.stop.cancel();
    }
    pub fn poll(&mut self) -> bool {
        let now = self.count.load(Ordering::Relaxed);
        let mut changed = now != self.seen;
        self.seen = now;
        if let Some(reply) = crate::task::take_reply(&mut self.pending) {
            self.result = reply.unwrap_or_else(|_| Result {
                partial: true,
                notice: "Comparison worker stopped".into(),
                ..Default::default()
            });
            changed = true;
        }
        changed
    }
    pub fn fit(&mut self) {
        crate::list_state::ListViewport::fit(
            &mut self.cursor,
            &mut self.offset,
            self.rows,
            Some(self.result.entries.len().saturating_sub(1)),
            None,
        );
    }
}
// Capture and check all traversed directory identities, including ancestors. As
// with the existing traversal this detects redirection, rather than providing a
// descriptor-relative sandbox against a hostile concurrent filesystem writer.
type Guards = BTreeMap<PathBuf, platform_fs::EntryIdentity>;
fn checked_dir(path: &Path, guards: &mut Guards) -> io::Result<()> {
    for p in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
        let m = fs::symlink_metadata(p)?;
        if !m.is_dir() || platform_fs::directory_link(&m) {
            return Err(io::Error::other(
                "Directory link or changed directory; not followed",
            ));
        }
        let id = platform_fs::entry_identity(p)?;
        if guards.get(p).is_some_and(|old| *old != id) {
            return Err(io::Error::other("Directory changed during comparison"));
        }
        guards.insert(p.to_owned(), id);
    }
    Ok(())
}
fn same_file(a: &Path, b: &Path, check: &impl Fn() -> io::Result<()>) -> io::Result<bool> {
    let mut a_file = crate::filesystem::open_regular(a)?;
    let mut b_file = crate::filesystem::open_regular(b)?;
    let before = [a_file.metadata()?, b_file.metadata()?];
    let equal = before[0].len() == before[1].len()
        && equal_bytes(&mut a_file, &mut b_file, before[0].len(), check)?;
    check()?;
    for ((path, file), old) in [(a, &a_file), (b, &b_file)].into_iter().zip(before) {
        let after = file.metadata()?;
        if after.len() != old.len()
            || after.modified().ok() != old.modified().ok()
            || !platform_fs::owns_file(path, file)
        {
            return Err(io::Error::other("File changed during comparison"));
        }
    }
    Ok(equal)
}
fn equal_bytes(
    a: &mut impl Read,
    b: &mut impl Read,
    bytes: u64,
    check: &impl Fn() -> io::Result<()>,
) -> io::Result<bool> {
    let capacity = bytes.clamp(1, 128 * 1024) as usize;
    let mut x = vec![0; capacity];
    let mut y = vec![0; capacity];
    let mut remaining = bytes;
    while remaining > 0 {
        check()?;
        let n = remaining.min(capacity as u64) as usize;
        a.read_exact(&mut x[..n])?;
        b.read_exact(&mut y[..n])?;
        if x[..n] != y[..n] {
            return Ok(false);
        }
        remaining -= n as u64;
    }
    check()?;
    // Some pseudo-files report zero length despite having readable contents.
    // Never classify them (or a growing file) as identical based on length alone.
    if a.read(&mut x[..1])? != 0 || b.read(&mut y[..1])? != 0 {
        return Err(io::Error::other("File length does not match its contents"));
    }
    Ok(true)
}
fn listing(
    path: &Path,
    guards: &mut Guards,
    budget: usize,
    check: &impl Fn() -> io::Result<()>,
) -> io::Result<BTreeMap<std::ffi::OsString, fs::Metadata>> {
    checked_dir(path, guards)?;
    let mut out = BTreeMap::new();
    for entry in fs::read_dir(path)? {
        check()?;
        if out.len() >= budget {
            return Err(io::Error::other("Comparison entry limit reached"));
        }
        let entry = entry?;
        out.insert(entry.file_name(), fs::symlink_metadata(entry.path())?);
    }
    checked_dir(path, guards)?;
    Ok(out)
}
fn scan(
    roots: [PathBuf; 2],
    cancel: &Cancellation,
    stop: &Cancellation,
    count: &AtomicUsize,
    limit: usize,
    depth_limit: usize,
) -> Result {
    let mut result = Result::default();
    let roots = match roots
        .iter()
        .map(fs::canonicalize)
        .collect::<io::Result<Vec<_>>>()
    {
        Ok(v) => [v[0].clone(), v[1].clone()],
        Err(e) => {
            result.partial = true;
            result.notice = "Partial comparison; review unexamined entries".into();
            result.entries.push(Row {
                path: PathBuf::new(),
                status: Status::Unknown,
                present: [true, true],
                cause: e.to_string(),
            });
            return result;
        }
    };
    let mut guards = [Guards::new(), Guards::new()];
    let check = || {
        cancel.check()?;
        stop.check()
    };
    let mut stack = vec![(PathBuf::new(), [true, true], 0)];
    let mut visited = 0;
    while let Some((relative, present, depth)) = stack.pop() {
        if check().is_err() {
            result.partial = true;
            result.notice = "Stopped; partial comparison".into();
            break;
        }
        if depth > depth_limit {
            result.partial = true;
            result.entries.push(Row {
                path: relative,
                status: Status::Unknown,
                present,
                cause: "Directory depth limit reached".into(),
            });
            continue;
        }
        let mut listings = [BTreeMap::new(), BTreeMap::new()];
        let mut failed = None;
        for side in 0..2 {
            if present[side] {
                match listing(
                    &roots[side].join(&relative),
                    &mut guards[side],
                    limit.saturating_sub(visited),
                    &check,
                ) {
                    Ok(v) => listings[side] = v,
                    Err(e) => {
                        failed = Some(e.to_string());
                        break;
                    }
                }
            }
        }
        if let Some(cause) = failed {
            result.partial = true;
            result.entries.push(Row {
                path: relative,
                status: Status::Unknown,
                present,
                cause,
            });
            continue;
        }
        let names = listings[0]
            .keys()
            .chain(listings[1].keys())
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        if names.is_empty() {
            result.entries.push(Row {
                path: relative.clone(),
                status: match present {
                    [true, true] => Status::Identical,
                    [true, false] => Status::Left,
                    _ => Status::Right,
                },
                present,
                cause: String::new(),
            });
        }
        for name in names {
            if visited >= limit
                || result.entries.len() >= limit
                || stack.len() >= limit.saturating_sub(visited)
            {
                result.partial = true;
                result.notice = "Comparison entry limit reached".into();
                stack.clear();
                break;
            }
            if check().is_err() {
                result.partial = true;
                result.notice = "Stopped; partial comparison".into();
                stack.clear();
                break;
            }
            visited += 1;
            count.store(visited, Ordering::Relaxed);
            let path = relative.join(&name);
            let m = [listings[0].get(&name), listings[1].get(&name)];
            let exists = [m[0].is_some(), m[1].is_some()];
            let real_dir = |v: Option<&fs::Metadata>| {
                v.is_none_or(|v| v.is_dir() && !platform_fs::directory_link(v))
            };
            if real_dir(m[0]) && real_dir(m[1]) {
                stack.push((path, exists, depth + 1));
                continue;
            }
            let mut cause = String::new();
            let status = if m.iter().flatten().any(|v| {
                v.file_type().is_symlink()
                    || platform_fs::directory_link(v)
                    || (!v.is_dir() && !v.is_file())
            }) {
                cause = "Links and special files are not compared".into();
                Status::Unknown
            } else {
                match (m[0], m[1]) {
                    (Some(a), Some(b)) if a.is_file() && b.is_file() => {
                        match (|| {
                            for side in 0..2 {
                                checked_dir(
                                    roots[side].join(&path).parent().expect("member parent"),
                                    &mut guards[side],
                                )?;
                            }
                            let equal =
                                same_file(&roots[0].join(&path), &roots[1].join(&path), &check)?;
                            for side in 0..2 {
                                checked_dir(
                                    roots[side].join(&path).parent().expect("member parent"),
                                    &mut guards[side],
                                )?;
                            }
                            Ok::<_, io::Error>(equal)
                        })() {
                            Ok(true) => Status::Identical,
                            Ok(false) => Status::Different,
                            Err(e) => {
                                cause = e.to_string();
                                Status::Unknown
                            }
                        }
                    }
                    (Some(_), Some(_)) => Status::Different,
                    (Some(_), None) => Status::Left,
                    _ => Status::Right,
                }
            };
            if status == Status::Unknown {
                result.partial = true;
            }
            result.entries.push(Row {
                path,
                status,
                present: exists,
                cause,
            });
        }
    }
    if result.entries.len() > limit {
        result.entries.truncate(limit);
        result.partial = true;
        result.notice = "Comparison entry limit reached".into();
    }
    result.entries.sort_by(|a, b| a.path.cmp(&b.path));
    if result.notice.is_empty() {
        result.notice = if result.partial {
            "Partial comparison; review unexamined entries"
        } else {
            "Comparison complete"
        }
        .into();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> io::Result<Self> {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "dn-comparison-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path)?;
            Ok(Self(fs::canonicalize(path)?))
        }
        fn roots(&self) -> io::Result<[PathBuf; 2]> {
            let roots = [self.0.join("left"), self.0.join("right")];
            for root in &roots {
                fs::create_dir(root)?;
            }
            Ok(roots)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn run(roots: [PathBuf; 2], limit: usize, depth: usize) -> Result {
        scan(
            roots,
            &Cancellation::default(),
            &Cancellation::default(),
            &AtomicUsize::new(0),
            limit,
            depth,
        )
    }
    #[test]
    fn byte_comparison_checks_eof_short_reads_and_cancellation() {
        let compare = |a: &[u8], b: &[u8], len: u64| {
            equal_bytes(
                &mut io::Cursor::new(a),
                &mut io::Cursor::new(b),
                len,
                &|| Ok(()),
            )
        };
        assert!(compare(b"", b"", 0).unwrap());
        assert!(compare(b"same", b"same", 4).unwrap());
        assert!(!compare(b"same", b"diff", 4).unwrap());
        assert!(compare(b"nonempty", b"nonempty", 0).is_err());
        assert!(compare(b"short", b"short", 10).is_err());
        let cancel = Cancellation::default();
        cancel.cancel();
        assert!(equal_bytes(
            &mut io::Cursor::new(b""),
            &mut io::Cursor::new(b""),
            0,
            &|| cancel.check()
        )
        .is_err());
    }
    #[test]
    fn recursive_bytes_hidden_unicode_empty_directories_and_type_conflicts() -> io::Result<()> {
        let temp = Temp::new()?;
        let roots = temp.roots()?;
        for root in &roots {
            fs::create_dir(root.join("Ёжик"))?;
            fs::create_dir(root.join("empty"))?;
            fs::write(root.join("Ёжик/same"), vec![7; 300_000])?;
            fs::write(root.join(".hidden"), b"same")?;
        }
        fs::write(roots[0].join("equal-size"), b"abcd")?;
        fs::write(roots[1].join("equal-size"), b"abce")?;
        fs::write(roots[0].join("only-left"), b"left")?;
        fs::write(roots[1].join("only-right"), b"right")?;
        fs::write(roots[0].join("type"), b"file")?;
        fs::create_dir(roots[1].join("type"))?;
        let report = run(roots.clone(), LIMIT, DEPTH);
        assert!(!report.partial);
        let statuses = report
            .entries
            .iter()
            .map(|r| (r.path.clone(), r.status))
            .collect::<BTreeMap<_, _>>();
        for (path, status) in [
            ("Ёжик/same", Status::Identical),
            ("empty", Status::Identical),
            (".hidden", Status::Identical),
            ("equal-size", Status::Different),
            ("only-left", Status::Left),
            ("only-right", Status::Right),
            ("type", Status::Different),
        ] {
            assert_eq!(statuses.get(Path::new(path)), Some(&status), "{path}");
        }
        assert_eq!(fs::read(roots[0].join("equal-size"))?, b"abcd");
        Ok(())
    }
    #[test]
    fn cancellation_limits_missing_roots_and_changed_files_are_never_identical() -> io::Result<()> {
        let temp = Temp::new()?;
        let roots = temp.roots()?;
        for i in 0..4 {
            fs::write(roots[0].join(format!("file{i}")), b"original")?;
        }
        let limited = run(roots.clone(), 2, DEPTH);
        assert!(limited.partial);
        assert!(limited.entries.len() <= 2);
        let stop = Cancellation::default();
        stop.cancel();
        let stopped = scan(
            roots.clone(),
            &Cancellation::default(),
            &stop,
            &AtomicUsize::new(0),
            LIMIT,
            DEPTH,
        );
        assert!(stopped.partial);
        assert!(stopped.entries.is_empty());
        fs::create_dir(roots[0].join("nested"))?;
        assert!(run(roots.clone(), LIMIT, 0).partial);
        let path = roots[0].join("file0");
        let other = roots[1].join("other");
        fs::write(&other, b"original")?;
        let calls = AtomicUsize::new(0);
        assert!(same_file(&path, &other, &|| {
            if calls.fetch_add(1, Ordering::Relaxed) == 0 {
                fs::write(&path, b"changed length")?;
            }
            Ok(())
        })
        .is_err());
        assert!(run([roots[0].clone(), temp.0.join("missing")], LIMIT, DEPTH).partial);
        Ok(())
    }
    #[test]
    fn replaced_directory_identity_is_rejected() -> io::Result<()> {
        let temp = Temp::new()?;
        let roots = temp.roots()?;
        let mut guards = Guards::new();
        checked_dir(&roots[0], &mut guards)?;
        fs::rename(&roots[0], temp.0.join("old"))?;
        fs::create_dir(&roots[0])?;
        assert!(checked_dir(&roots[0], &mut guards).is_err());
        Ok(())
    }
    #[cfg(unix)]
    #[test]
    fn directory_links_and_non_utf8_names_are_preserved_without_traversal() -> io::Result<()> {
        use std::os::unix::fs::symlink;
        let temp = Temp::new()?;
        let roots = temp.roots()?;
        let outside = temp.0.join("outside");
        fs::create_dir(&outside)?;
        fs::write(outside.join("secret"), b"untouched")?;
        symlink(&outside, roots[0].join("link"))?;
        #[cfg(target_os = "linux")]
        let native = {
            use std::os::unix::ffi::OsStringExt;
            let native = std::ffi::OsString::from_vec(vec![b'x', 255]);
            fs::write(roots[0].join(&native), b"native")?;
            native
        };
        let report = run(roots, LIMIT, DEPTH);
        assert!(report.partial);
        assert!(report
            .entries
            .iter()
            .any(|r| r.path == Path::new("link") && r.status == Status::Unknown));
        #[cfg(target_os = "linux")]
        assert!(report.entries.iter().any(|r| r.path == Path::new(&native)));
        assert!(!report.entries.iter().any(|r| r.path.ends_with("secret")));
        Ok(())
    }
}
