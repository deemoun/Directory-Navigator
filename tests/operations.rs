use dn::operations::{self, Kind, Plan};
use std::{
    fs, io,
    path::PathBuf,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};

struct Temp(PathBuf);
impl Temp {
    fn new() -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "dn-operations-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
    fn plan(&self, kind: Kind, sources: &[&str], destination: &str) -> Plan {
        Plan {
            kind,
            sources: sources.iter().map(|s| self.0.join(s)).collect(),
            destination: self.0.join(destination),
        }
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn run(plan: &Plan) -> io::Result<operations::Progress> {
    operations::run(plan, &AtomicBool::new(false), |_| {})
}

#[test]
fn recursive_unicode_copy_and_move_then_delete() -> io::Result<()> {
    let temp = Temp::new()?;
    fs::create_dir_all(temp.0.join("source/sub"))?;
    fs::write(temp.0.join("source/sub/文.txt"), b"hello")?;
    run(&temp.plan(Kind::Copy, &["source"], "copied"))?;
    assert_eq!(fs::read(temp.0.join("copied/sub/文.txt"))?, b"hello");
    assert!(temp.0.join("source/sub/文.txt").exists());
    run(&temp.plan(Kind::Move, &["copied"], "moved"))?;
    assert!(!temp.0.join("copied").exists());
    assert_eq!(fs::read(temp.0.join("moved/sub/文.txt"))?, b"hello");
    run(&temp.plan(Kind::Delete, &["moved"], ""))?;
    assert!(!temp.0.join("moved").exists());
    assert!(temp.0.join("source").exists());
    run(&temp.plan(Kind::Mkdir, &[], "new directory"))?;
    assert!(temp.0.join("new directory").is_dir());
    Ok(())
}

#[test]
fn collisions_and_invalid_destinations_preserve_existing_data() -> io::Result<()> {
    let temp = Temp::new()?;
    fs::write(temp.0.join("a"), b"original")?;
    fs::write(temp.0.join("b"), b"existing")?;
    for kind in [Kind::Copy, Kind::Move] {
        assert_eq!(
            run(&temp.plan(kind, &["a"], "b")).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert!(run(&temp.plan(kind, &["a"], "a")).is_err());
        assert!(run(&temp.plan(kind, &["absent"], "new")).is_err());
        assert!(run(&temp.plan(kind, &["a", "b"], "new")).is_err());
        assert!(run(&temp.plan(kind, &["a"], "absent/file")).is_err());
    }
    assert_eq!(fs::read(temp.0.join("a"))?, b"original");
    assert_eq!(fs::read(temp.0.join("b"))?, b"existing");
    fs::create_dir(temp.0.join("tree"))?;
    fs::create_dir(temp.0.join("tree/sub"))?;
    assert!(run(&temp.plan(Kind::Copy, &["tree"], "tree/sub/copy")).is_err());
    assert!(!temp.0.join("tree/sub/copy").exists());
    // A collision in any batch item is caught before the first write.
    fs::create_dir(temp.0.join("out"))?;
    fs::write(temp.0.join("out/b"), b"keep")?;
    assert!(run(&temp.plan(Kind::Copy, &["a", "b"], "out")).is_err());
    assert!(!temp.0.join("out/a").exists());
    Ok(())
}

#[test]
fn cancellation_keeps_move_sources() -> io::Result<()> {
    let temp = Temp::new()?;
    fs::write(temp.0.join("large"), vec![5; 512 * 1024])?;
    let cancel = AtomicBool::new(false);
    let plan = temp.plan(Kind::Move, &["large"], "new");
    let result = operations::run_with_strategy(
        &plan,
        &cancel,
        |p| {
            if p.bytes > 0 {
                cancel.store(true, Ordering::Relaxed);
            }
        },
        operations::MoveStrategy::Copy,
    );
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Interrupted);
    assert_eq!(fs::metadata(temp.0.join("large"))?.len(), 512 * 1024);
    assert!(!temp.0.join("new").exists());
    Ok(())
}

#[test]
fn new_children_are_never_removed_by_move_cleanup() -> io::Result<()> {
    let temp = Temp::new()?;
    fs::create_dir(temp.0.join("tree"))?;
    fs::write(temp.0.join("tree/file"), b"copied")?;
    let source = temp.0.join("tree");
    let result = operations::run_with_strategy(
        &temp.plan(Kind::Move, &["tree"], "moved"),
        &AtomicBool::new(false),
        |p| {
            if p.path == source && p.files > 0 {
                fs::write(source.join("late"), b"new data").unwrap();
            }
        },
        operations::MoveStrategy::Copy,
    );
    assert!(result.is_err());
    assert_eq!(fs::read(source.join("late"))?, b"new data");
    assert_eq!(fs::read(temp.0.join("moved/file"))?, b"copied");
    Ok(())
}

#[test]
fn modified_sources_or_destinations_stop_move_cleanup() -> io::Result<()> {
    for change_destination in [false, true] {
        let temp = Temp::new()?;
        fs::write(temp.0.join("source"), b"original")?;
        let source = temp.0.join("source");
        let target = temp.0.join("target");
        let mut changed = false;
        let result = operations::run_with_strategy(
            &temp.plan(Kind::Move, &["source"], "target"),
            &AtomicBool::new(false),
            |p| {
                if p.files > 0 && !changed {
                    changed = true;
                    fs::write(
                        if change_destination { &target } else { &source },
                        b"new data of a different size",
                    )
                    .unwrap();
                }
            },
            operations::MoveStrategy::Copy,
        );
        assert!(result.is_err());
        assert!(source.exists());
        assert_eq!(
            fs::read(if change_destination { &source } else { &target })?,
            b"original"
        );
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn readonly_metadata_is_preserved_and_permission_failures_are_reported() -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let temp = Temp::new()?;
    let source = temp.0.join("readonly");
    fs::write(&source, b"readonly contents")?;
    fs::set_permissions(&source, fs::Permissions::from_mode(0o444))?;
    let modified = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1000);
    fs::File::open(&source)?.set_times(fs::FileTimes::new().set_modified(modified))?;
    run(&temp.plan(Kind::Copy, &["readonly"], "copy"))?;
    let metadata = fs::metadata(temp.0.join("copy"))?;
    assert_eq!(metadata.permissions().mode() & 0o777, 0o444);
    assert_eq!(metadata.modified()?, modified);
    fs::create_dir(temp.0.join("blocked"))?;
    fs::set_permissions(temp.0.join("blocked"), fs::Permissions::from_mode(0o000))?;
    let permissions_enforced = fs::read_dir(temp.0.join("blocked")).is_err();
    let result = run(&temp.plan(Kind::Copy, &["blocked"], "blocked-copy"));
    // Restore access before asserting, so the isolated fixture always cleans up.
    fs::set_permissions(temp.0.join("blocked"), fs::Permissions::from_mode(0o700))?;
    if permissions_enforced {
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlink_cycles_broken_links_and_outside_targets_are_not_traversed() -> io::Result<()> {
    use std::os::unix::fs::symlink;
    let temp = Temp::new()?;
    fs::create_dir_all(temp.0.join("outside"))?;
    fs::write(temp.0.join("outside/keep"), b"safe")?;
    fs::create_dir(temp.0.join("tree"))?;
    symlink(&temp.0, temp.0.join("tree/cycle"))?;
    symlink(temp.0.join("outside"), temp.0.join("tree/link"))?;
    symlink("missing", temp.0.join("tree/broken"))?;
    run(&temp.plan(Kind::Copy, &["tree"], "copied"))?;
    for name in ["cycle", "link", "broken"] {
        assert!(fs::symlink_metadata(temp.0.join("copied").join(name))?.is_symlink());
    }
    run(&temp.plan(Kind::Delete, &["copied"], ""))?;
    run(&temp.plan(Kind::Delete, &["tree/link"], ""))?;
    assert_eq!(fs::read(temp.0.join("outside/keep"))?, b"safe");
    symlink("outside/keep", temp.0.join("file-link"))?;
    assert!(dn::filesystem::open_regular(&temp.0.join("file-link")).is_err());
    symlink("missing", temp.0.join("collision"))?;
    assert!(run(&temp.plan(Kind::Copy, &["outside/keep"], "collision")).is_err());
    // Special files must fail rather than block waiting for a reader/writer.
    use std::os::unix::ffi::OsStrExt;
    let pipe = temp.0.join("pipe");
    let c_path = std::ffi::CString::new(pipe.as_os_str().as_bytes())?;
    // SAFETY: c_path is a live, NUL-terminated pathname in this isolated fixture.
    if unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) } != 0 {
        return Err(io::Error::last_os_error());
    }
    assert!(run(&temp.plan(Kind::Copy, &["pipe"], "pipe-copy")).is_err());
    assert!(dn::filesystem::open_regular(&pipe).is_err());
    Ok(())
}
