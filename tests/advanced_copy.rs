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
            "dn-advanced-copy-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p)?;
        Ok(Self(fs::canonicalize(p)?))
    }
    fn plan(&self, names: &[&str], destination: &str) -> Plan {
        Plan {
            kind: Kind::Copy,
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
fn refuse_preflights_the_batch_and_skip_retains_existing_bytes_and_sources() -> io::Result<()> {
    let t = Sandbox::new()?;
    fs::write(t.0.join("a"), b"new-a")?;
    fs::write(t.0.join("b"), b"new-b")?;
    fs::create_dir(t.0.join("out"))?;
    fs::write(t.0.join("out/b"), b"existing-b")?;
    let plan = t.plan(&["a", "b"], "out");
    assert!(run(&plan, CopyOptions::default()).is_err());
    assert!(!t.0.join("out/a").exists());
    let progress = run(
        &plan,
        CopyOptions {
            existing: ExistingDestination::Skip,
            verify_writes: true,
            ..Default::default()
        },
    )?;
    assert_eq!((progress.files, progress.skipped), (1, 1));
    assert_eq!(fs::read(t.0.join("out/a"))?, b"new-a");
    assert_eq!(fs::read(t.0.join("out/b"))?, b"existing-b");
    assert_eq!(fs::read(t.0.join("b"))?, b"new-b");
    fs::write(t.0.join("c"), b"new-c")?;
    let p = run(
        &t.plan(&["c", "b"], "out"),
        CopyOptions {
            existing: ExistingDestination::Skip,
            remove_sources: true,
            ..Default::default()
        },
    )?;
    assert_eq!(p.skipped, 1);
    assert!(!t.0.join("c").exists());
    assert_eq!(fs::read(t.0.join("out/c"))?, b"new-c");
    assert!(t.0.join("b").exists());
    Ok(())
}
#[test]
fn verification_failure_and_cancellation_never_remove_sources_or_publish_partial_bytes(
) -> io::Result<()> {
    for cancel_case in [false, true] {
        let t = Sandbox::new()?;
        let bytes = vec![42; 300_000];
        fs::write(t.0.join("source"), &bytes)?;
        let cancel = AtomicBool::new(false);
        let injected = AtomicBool::new(false);
        let result = operations::run_with_options(
            &t.plan(&["source"], "dest"),
            CopyOptions {
                remove_sources: true,
                ..Default::default()
            },
            &cancel,
            |p| {
                if p.bytes > 0 && !injected.swap(true, Ordering::Relaxed) {
                    if cancel_case {
                        cancel.store(true, Ordering::Relaxed);
                    } else {
                        let stage = fs::read_dir(&t.0)
                            .unwrap()
                            .map(Result::unwrap)
                            .find(|e| e.file_name().to_string_lossy().starts_with(".dn-part-"))
                            .unwrap()
                            .path();
                        let mut file = fs::OpenOptions::new().write(true).open(stage).unwrap();
                        file.write_all(&[99]).unwrap();
                    }
                }
            },
            MoveStrategy::PreferRename,
        );
        assert!(result.is_err());
        assert!(injected.load(Ordering::Relaxed));
        assert_eq!(fs::read(t.0.join("source"))?, bytes);
        assert!(!t.0.join("dest").exists());
        assert_eq!(
            fs::read_dir(&t.0)?.count(),
            1,
            "owned staging files must be cleaned"
        );
    }
    Ok(())
}
#[test]
fn skip_does_not_replace_a_destination_created_during_copy() -> io::Result<()> {
    let t = Sandbox::new()?;
    fs::write(t.0.join("source"), vec![7; 300_000])?;
    let injected = AtomicBool::new(false);
    let result = operations::run_with_options(
        &t.plan(&["source"], "dest"),
        CopyOptions {
            existing: ExistingDestination::Skip,
            verify_writes: true,
            ..Default::default()
        },
        &AtomicBool::new(false),
        |p| {
            if p.bytes > 0 && !injected.swap(true, Ordering::Relaxed) {
                fs::write(t.0.join("dest"), b"winner").unwrap();
            }
        },
        MoveStrategy::PreferRename,
    );
    assert!(result.is_err());
    assert_eq!(fs::read(t.0.join("dest"))?, b"winner");
    assert!(t.0.join("source").exists());
    assert_eq!(fs::read_dir(&t.0)?.count(), 2);
    Ok(())
}
#[test]
fn existing_directories_are_skipped_as_a_whole_without_merge_or_source_cleanup() -> io::Result<()> {
    let t = Sandbox::new()?;
    fs::create_dir_all(t.0.join("tree/sub"))?;
    fs::write(t.0.join("tree/sub/new"), b"new")?;
    fs::create_dir_all(t.0.join("out/tree"))?;
    fs::write(t.0.join("out/tree/sentinel"), b"keep")?;
    let p = run(
        &t.plan(&["tree"], "out"),
        CopyOptions {
            existing: ExistingDestination::Skip,
            remove_sources: true,
            verify_writes: true,
            ..Default::default()
        },
    )?;
    assert_eq!((p.files, p.skipped), (0, 1));
    assert!(t.0.join("tree/sub/new").exists());
    assert!(!t.0.join("out/tree/sub").exists());
    assert_eq!(fs::read(t.0.join("out/tree/sentinel"))?, b"keep");
    Ok(())
}
#[cfg(unix)]
#[test]
fn links_are_copied_as_links_and_verified_source_removal_never_follows_them() -> io::Result<()> {
    let t = Sandbox::new()?;
    let outside = Sandbox::new()?;
    fs::write(outside.0.join("sentinel"), b"keep")?;
    std::os::unix::fs::symlink(&outside.0, t.0.join("link"))?;
    run(
        &t.plan(&["link"], "dest"),
        CopyOptions {
            verify_writes: true,
            remove_sources: true,
            check_free_space: true,
            ..Default::default()
        },
    )?;
    assert!(!t.0.join("link").exists());
    assert!(fs::symlink_metadata(t.0.join("dest"))?.is_symlink());
    assert_eq!(fs::read_link(t.0.join("dest"))?, outside.0);
    assert_eq!(fs::read(outside.0.join("sentinel"))?, b"keep");
    Ok(())
}
