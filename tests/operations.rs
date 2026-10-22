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

#[test]
fn cancellation_at_entry_notification_prevents_the_next_change() -> io::Result<()> {
    for kind in [Kind::Copy, Kind::Move, Kind::Delete] {
        let temp = Temp::new()?;
        fs::write(temp.0.join("source"), b"keep")?;
        let cancel = AtomicBool::new(false);
        let result = operations::run(&temp.plan(kind, &["source"], "target"), &cancel, |_| {
            cancel.store(true, Ordering::Relaxed);
        });
        assert_eq!(
            result.expect_err("cancelled").kind(),
            io::ErrorKind::Interrupted
        );
        assert_eq!(fs::read(temp.0.join("source"))?, b"keep");
        assert!(!temp.0.join("target").exists());
    }
    Ok(())
}

#[test]
fn growing_or_shrinking_source_stops_without_publishing_partial_data() -> io::Result<()> {
    use std::io::Write;
    for grow in [true, false] {
        let temp = Temp::new()?;
        let source = temp.0.join("source");
        let initial_length = 256 * 1024;
        fs::write(&source, vec![5; initial_length])?;
        let mut changed = false;
        let mut bytes_copied = 0;
        let result = operations::run_with_strategy(
            &temp.plan(Kind::Move, &["source"], "target"),
            &AtomicBool::new(false),
            |p| {
                bytes_copied = p.bytes;
                if p.bytes > 0 && !changed {
                    changed = true;
                    if grow {
                        fs::OpenOptions::new()
                            .append(true)
                            .open(&source)
                            .expect("fixture")
                            .write_all(&vec![9; initial_length])
                            .expect("fixture");
                    } else {
                        fs::OpenOptions::new()
                            .write(true)
                            .open(&source)
                            .expect("fixture")
                            .set_len(0)
                            .expect("fixture");
                    }
                }
            },
            operations::MoveStrategy::Copy,
        );
        assert!(changed && result.is_err());
        assert!(
            bytes_copied <= initial_length as u64,
            "reads stay bounded by original length"
        );
        assert_eq!(
            fs::metadata(&source)?.len(),
            if grow { (initial_length * 2) as u64 } else { 0 }
        );
        assert!(!temp.0.join("target").exists());
        assert_eq!(
            fs::read_dir(&temp.0)?.count(),
            1,
            "owned staging cleaned up"
        );
    }
    Ok(())
}

#[test]
fn overlapping_or_duplicate_sources_are_rejected_before_any_changes() -> io::Result<()> {
    for kind in [Kind::Copy, Kind::Move, Kind::Delete] {
        for sources in [&["tree", "tree/file"][..], &["tree/file", "tree/file"][..]] {
            let temp = Temp::new()?;
            fs::create_dir(temp.0.join("tree"))?;
            fs::create_dir(temp.0.join("out"))?;
            fs::write(temp.0.join("tree/file"), b"keep")?;
            assert!(run(&temp.plan(kind, sources, "out")).is_err());
            assert_eq!(fs::read(temp.0.join("tree/file"))?, b"keep");
            assert_eq!(fs::read_dir(temp.0.join("out"))?.count(), 0);
        }
    }
    Ok(())
}

#[test]
fn changed_later_move_source_keeps_the_entire_copied_batch() -> io::Result<()> {
    let temp = Temp::new()?;
    fs::create_dir(temp.0.join("out"))?;
    for name in ["a", "b"] {
        fs::write(temp.0.join(name), b"original")?;
    }
    let mut changed = false;
    let result = operations::run_with_strategy(
        &temp.plan(Kind::Move, &["a", "b"], "out"),
        &AtomicBool::new(false),
        |p| {
            if p.files == 2 && !changed {
                changed = true;
                fs::write(temp.0.join("a"), b"externally changed").expect("fixture");
            }
        },
        operations::MoveStrategy::Copy,
    );
    assert!(changed && result.is_err());
    assert_eq!(fs::read(temp.0.join("a"))?, b"externally changed");
    assert_eq!(fs::read(temp.0.join("b"))?, b"original");
    for name in ["a", "b"] {
        assert_eq!(fs::read(temp.0.join("out").join(name))?, b"original");
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn restored_mtime_does_not_hide_source_or_destination_rewrites() -> io::Result<()> {
    for destination in [false, true] {
        let temp = Temp::new()?;
        let source = temp.0.join("source");
        let target = temp.0.join("target");
        fs::write(&source, b"original")?;
        let modified = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1234);
        fs::File::open(&source)?.set_times(fs::FileTimes::new().set_modified(modified))?;
        let mut changed = false;
        let result = operations::run_with_strategy(
            &temp.plan(Kind::Move, &["source"], "target"),
            &AtomicBool::new(false),
            |p| {
                if !changed
                    && (if destination {
                        p.files > 0
                    } else {
                        p.bytes > 0
                    })
                {
                    changed = true;
                    let path = if destination { &target } else { &source };
                    fs::write(path, b"rewritten").expect("fixture");
                    // Preserve both length and modification time; Unix ctime must catch it.
                    fs::OpenOptions::new()
                        .write(true)
                        .open(path)
                        .expect("fixture")
                        .set_len(8)
                        .expect("fixture");
                    fs::File::open(path)
                        .expect("fixture")
                        .set_times(fs::FileTimes::new().set_modified(modified))
                        .expect("fixture");
                }
            },
            operations::MoveStrategy::Copy,
        );
        assert!(changed && result.is_err());
        assert!(source.exists());
        if !destination {
            assert!(!target.exists());
            assert_eq!(
                fs::read_dir(&temp.0)?.count(),
                1,
                "partial output cleaned up"
            );
        } else {
            assert_eq!(fs::read(&source)?, b"original");
        }
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn native_and_fallback_moves_of_hardlinked_sources_remain_supported() -> io::Result<()> {
    for strategy in [
        operations::MoveStrategy::PreferRename,
        operations::MoveStrategy::Copy,
    ] {
        let temp = Temp::new()?;
        fs::write(temp.0.join("a"), b"contents")?;
        fs::hard_link(temp.0.join("a"), temp.0.join("b"))?;
        fs::create_dir(temp.0.join("out"))?;
        operations::run_with_strategy(
            &temp.plan(Kind::Move, &["a", "b"], "out"),
            &AtomicBool::new(false),
            |_| {},
            strategy,
        )?;
        for name in ["a", "b"] {
            assert!(!temp.0.join(name).exists());
            assert_eq!(fs::read(temp.0.join("out").join(name))?, b"contents");
        }
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn source_rewrite_after_batch_validation_is_still_retained() -> io::Result<()> {
    let temp = Temp::new()?;
    let source = temp.0.join("source");
    fs::write(&source, b"original")?;
    let modified = fs::metadata(&source)?.modified()?;
    let mut completed_notifications = 0;
    let result = operations::run_with_strategy(
        &temp.plan(Kind::Move, &["source"], "target"),
        &AtomicBool::new(false),
        |p| {
            if p.files > 0 {
                completed_notifications += 1;
                if completed_notifications == 2 {
                    fs::write(&source, b"new data").expect("fixture");
                    fs::File::open(&source)
                        .expect("fixture")
                        .set_times(fs::FileTimes::new().set_modified(modified))
                        .expect("fixture");
                }
            }
        },
        operations::MoveStrategy::Copy,
    );
    assert_eq!(completed_notifications, 2);
    assert!(result.is_err());
    assert_eq!(fs::read(source)?, b"new data");
    assert_eq!(fs::read(temp.0.join("target"))?, b"original");
    Ok(())
}

#[cfg(unix)]
#[test]
fn queued_delete_entry_replacement_is_retained() -> io::Result<()> {
    use std::os::unix::fs::symlink;
    let temp = Temp::new()?;
    fs::create_dir(temp.0.join("tree"))?;
    let child = temp.0.join("tree/file");
    fs::write(&child, b"original")?;
    fs::write(temp.0.join("outside"), b"keep")?;
    let mut changed = false;
    let result = operations::run(
        &temp.plan(Kind::Delete, &["tree"], ""),
        &AtomicBool::new(false),
        |p| {
            if p.path == child && !changed {
                changed = true;
                fs::rename(&child, temp.0.join("parked")).expect("fixture");
                symlink(temp.0.join("outside"), &child).expect("fixture");
            }
        },
    );
    assert!(changed && result.is_err());
    assert!(fs::symlink_metadata(child)?.is_symlink());
    assert_eq!(fs::read(temp.0.join("outside"))?, b"keep");
    assert_eq!(fs::read(temp.0.join("parked"))?, b"original");
    Ok(())
}

#[cfg(unix)]
#[test]
fn replaced_source_ancestors_never_redirect_queued_copy_or_delete() -> io::Result<()> {
    use std::os::unix::fs::symlink;
    for kind in [Kind::Copy, Kind::Move, Kind::Delete] {
        let temp = Temp::new()?;
        fs::create_dir(temp.0.join("tree"))?;
        fs::create_dir(temp.0.join("outside"))?;
        fs::write(temp.0.join("tree/file"), b"original")?;
        fs::write(temp.0.join("outside/file"), b"keep")?;
        let mut changed = false;
        let result = operations::run_with_strategy(
            &temp.plan(kind, &["tree"], "out"),
            &AtomicBool::new(false),
            |p| {
                if p.path == temp.0.join("tree/file") && !changed {
                    changed = true;
                    fs::rename(temp.0.join("tree"), temp.0.join("parked")).expect("fixture");
                    symlink(temp.0.join("outside"), temp.0.join("tree")).expect("fixture");
                }
            },
            operations::MoveStrategy::Copy,
        );
        assert!(changed && result.is_err());
        assert_eq!(fs::read(temp.0.join("outside/file"))?, b"keep");
        assert_eq!(fs::read(temp.0.join("parked/file"))?, b"original");
        assert!(!temp.0.join("out/file").exists());
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn replaced_destination_ancestor_is_not_used_for_publication_or_cleanup() -> io::Result<()> {
    use std::os::unix::fs::symlink;
    for replace_after_publish in [false, true] {
        let temp = Temp::new()?;
        fs::create_dir(temp.0.join("out"))?;
        fs::create_dir(temp.0.join("outside"))?;
        fs::write(temp.0.join("source"), b"original")?;
        let mut changed = false;
        let result = operations::run_with_strategy(
            &temp.plan(Kind::Move, &["source"], "out"),
            &AtomicBool::new(false),
            |p| {
                if !changed
                    && (if replace_after_publish {
                        p.files > 0
                    } else {
                        p.bytes > 0
                    })
                {
                    changed = true;
                    fs::rename(temp.0.join("out"), temp.0.join("parked")).expect("fixture");
                    symlink(temp.0.join("outside"), temp.0.join("out")).expect("fixture");
                }
            },
            operations::MoveStrategy::Copy,
        );
        assert!(changed && result.is_err());
        assert_eq!(fs::read(temp.0.join("source"))?, b"original");
        assert_eq!(fs::read_dir(temp.0.join("outside"))?.count(), 0);
    }
    Ok(())
}
