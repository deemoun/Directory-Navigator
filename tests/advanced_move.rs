//! Every fixture is created in a unique temporary sandbox; no real Trash or user files.
use dn::operations::{self, CopyOptions, ExistingDestination, Kind, MoveStrategy, Plan};
use std::{
    fs,
    io::{self, Write},
    path::PathBuf,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};
struct Sandbox(PathBuf);
impl Sandbox {
    fn new() -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "dn-advanced-move-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p)?;
        Ok(Self(fs::canonicalize(p)?))
    }
    fn plan(&self, names: &[&str], destination: &str) -> Plan {
        Plan {
            kind: Kind::Move,
            sources: names.iter().map(|n| self.0.join(n)).collect(),
            destination: self.0.join(destination),
        }
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn run(plan: &Plan, options: CopyOptions) -> io::Result<operations::Progress> {
    operations::run_with_options(
        plan,
        options,
        &AtomicBool::new(false),
        |_| {},
        MoveStrategy::PreferRename,
    )
}
#[test]
fn refuse_preflights_the_batch_and_skip_moves_only_new_items() -> io::Result<()> {
    for verify_writes in [false, true] {
        let t = Sandbox::new()?;
        fs::write(t.0.join("a"), b"new-a")?;
        fs::write(t.0.join("b"), b"new-b")?;
        fs::create_dir(t.0.join("out"))?;
        fs::write(t.0.join("out/b"), b"winner")?;
        let plan = t.plan(&["a", "b"], "out");
        assert!(run(
            &plan,
            CopyOptions {
                verify_writes,
                ..Default::default()
            }
        )
        .is_err());
        assert!(t.0.join("a").exists() && !t.0.join("out/a").exists());
        let p = run(
            &plan,
            CopyOptions {
                existing: ExistingDestination::Skip,
                verify_writes,
                check_free_space: true,
                ..Default::default()
            },
        )?;
        assert_eq!((p.files, p.skipped), (1, 1));
        assert!(!t.0.join("a").exists());
        assert_eq!(fs::read(t.0.join("out/a"))?, b"new-a");
        assert_eq!(fs::read(t.0.join("out/b"))?, b"winner");
        assert_eq!(fs::read(t.0.join("b"))?, b"new-b");
    }
    Ok(())
}
#[test]
fn verified_move_failure_cancel_and_raced_destination_retain_sources() -> io::Result<()> {
    for mode in 0..3 {
        let t = Sandbox::new()?;
        let bytes = vec![42; 300_000];
        fs::write(t.0.join("source"), &bytes)?;
        let cancel = AtomicBool::new(false);
        let injected = AtomicBool::new(false);
        let result = operations::run_with_options(
            &t.plan(&["source"], "dest"),
            CopyOptions {
                existing: ExistingDestination::Skip,
                verify_writes: true,
                ..Default::default()
            },
            &cancel,
            |p| {
                if p.bytes > 0 && !injected.swap(true, Ordering::Relaxed) {
                    match mode {
                        0 => cancel.store(true, Ordering::Relaxed),
                        1 => {
                            let stage = fs::read_dir(&t.0)
                                .unwrap()
                                .map(Result::unwrap)
                                .find(|e| e.file_name().to_string_lossy().starts_with(".dn-part-"))
                                .unwrap()
                                .path();
                            fs::OpenOptions::new()
                                .write(true)
                                .open(stage)
                                .unwrap()
                                .write_all(&[99])
                                .unwrap();
                        }
                        _ => fs::write(t.0.join("dest"), b"winner").unwrap(),
                    }
                }
            },
            MoveStrategy::PreferRename,
        );
        assert!(result.is_err() && injected.load(Ordering::Relaxed));
        assert_eq!(fs::read(t.0.join("source"))?, bytes);
        if mode == 2 {
            assert_eq!(fs::read(t.0.join("dest"))?, b"winner");
        } else {
            assert!(!t.0.join("dest").exists());
        }
        assert!(!fs::read_dir(&t.0)?.any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".dn-part-")));
    }
    Ok(())
}
#[test]
fn verified_move_defers_source_cleanup_until_the_whole_batch_succeeds() -> io::Result<()> {
    let t = Sandbox::new()?;
    fs::create_dir(t.0.join("out"))?;
    fs::write(t.0.join("a"), b"first")?;
    fs::write(t.0.join("b"), b"second")?;
    let changed = AtomicBool::new(false);
    let result = operations::run_with_options(
        &t.plan(&["a", "b"], "out"),
        CopyOptions {
            verify_writes: true,
            ..Default::default()
        },
        &AtomicBool::new(false),
        |p| {
            if p.files == 1 && !changed.swap(true, Ordering::Relaxed) {
                fs::write(t.0.join("b"), b"changed later source").unwrap();
            }
        },
        MoveStrategy::PreferRename,
    );
    assert!(result.is_err() && changed.load(Ordering::Relaxed));
    assert_eq!(fs::read(t.0.join("a"))?, b"first");
    assert_eq!(fs::read(t.0.join("out/a"))?, b"first");
    assert_eq!(fs::read(t.0.join("b"))?, b"changed later source");
    assert!(!t.0.join("out/b").exists());
    Ok(())
}
#[test]
fn verified_directory_move_and_skipped_tree_never_merge_or_follow_links() -> io::Result<()> {
    let t = Sandbox::new()?;
    let outside = Sandbox::new()?;
    fs::write(outside.0.join("sentinel"), b"keep")?;
    fs::create_dir_all(t.0.join("tree/sub"))?;
    fs::write(t.0.join("tree/sub/file"), b"payload")?;
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside.0, t.0.join("tree/link"))?;
    let original = fs::symlink_metadata(t.0.join("tree/sub/file"))?;
    run(
        &t.plan(&["tree"], "moved"),
        CopyOptions {
            verify_writes: true,
            check_free_space: true,
            ..Default::default()
        },
    )?;
    assert!(!t.0.join("tree").exists());
    assert_eq!(fs::read(t.0.join("moved/sub/file"))?, b"payload");
    assert!(!dn::platform_fs::same_identity(
        &original,
        &fs::symlink_metadata(t.0.join("moved/sub/file"))?
    ));
    #[cfg(unix)]
    assert!(fs::symlink_metadata(t.0.join("moved/link"))?.is_symlink());
    fs::create_dir_all(t.0.join("out/moved"))?;
    fs::write(t.0.join("out/moved/existing"), b"winner")?;
    let p = run(
        &t.plan(&["moved"], "out"),
        CopyOptions {
            verify_writes: true,
            existing: ExistingDestination::Skip,
            ..Default::default()
        },
    )?;
    assert_eq!((p.files, p.skipped), (0, 1));
    assert!(t.0.join("moved/sub/file").exists() && !t.0.join("out/moved/sub").exists());
    assert_eq!(fs::read(outside.0.join("sentinel"))?, b"keep");
    Ok(())
}
