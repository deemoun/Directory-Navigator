use dn::operations::{self, conflict::Decision, CopyOptions, ExistingDestination, Kind, Plan};
use std::{
    fs, io,
    path::PathBuf,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "dn-overwrite-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root)?;
        fs::create_dir(root.join("src"))?;
        fs::create_dir(root.join("dst"))?;
        Ok(Self(fs::canonicalize(root)?))
    }
    fn plan(&self, kind: Kind) -> Plan {
        Plan {
            kind,
            sources: vec![self.0.join("src/tree")],
            destination: self.0.join("dst"),
        }
    }
    fn trees(&self) -> io::Result<()> {
        for root in ["src/tree", "dst/tree"] {
            fs::create_dir(self.0.join(root))?;
        }
        for name in ["a", "b"] {
            fs::write(self.0.join("src/tree").join(name), b"new")?;
            fs::write(self.0.join("dst/tree").join(name), b"old")?;
        }
        fs::write(self.0.join("dst/tree/unrelated"), b"keep")?;
        Ok(())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn replace_all_merges_without_removing_unrelated_files() -> io::Result<()> {
    let f = Fixture::new()?;
    f.trees()?;
    let mut prompts = 0;
    let p = operations::run_with_conflicts(
        &f.plan(Kind::Copy),
        CopyOptions::default(),
        &AtomicBool::new(false),
        |_| {},
        &mut |c| {
            prompts += 1;
            assert_eq!(c.source_size, 3);
            assert_eq!(c.destination_size, 3);
            Ok(Decision::ReplaceAll)
        },
    )?;
    assert_eq!(prompts, 1);
    assert_eq!(p.skipped, 0);
    for name in ["a", "b"] {
        assert_eq!(fs::read(f.0.join("dst/tree").join(name))?, b"new");
        assert_eq!(fs::read(f.0.join("src/tree").join(name))?, b"new");
    }
    assert_eq!(fs::read(f.0.join("dst/tree/unrelated"))?, b"keep");
    Ok(())
}
#[test]
fn skip_all_is_per_job_and_move_retains_skipped_sources() -> io::Result<()> {
    let f = Fixture::new()?;
    f.trees()?;
    fs::write(f.0.join("src/tree/fresh"), b"fresh")?;
    let mut prompts = 0;
    let p = operations::run_with_conflicts(
        &f.plan(Kind::Move),
        CopyOptions::default(),
        &AtomicBool::new(false),
        |_| {},
        &mut |_| {
            prompts += 1;
            Ok(Decision::SkipAll)
        },
    )?;
    assert_eq!(prompts, 1);
    assert_eq!(p.skipped, 2);
    assert!(f.0.join("src/tree/a").is_file());
    assert!(!f.0.join("src/tree/fresh").exists());
    assert_eq!(fs::read(f.0.join("dst/tree/a"))?, b"old");
    operations::run_with_conflicts(
        &f.plan(Kind::Copy),
        CopyOptions::default(),
        &AtomicBool::new(false),
        |_| {},
        &mut |_| {
            prompts += 1;
            Ok(Decision::ReplaceAll)
        },
    )?;
    assert_eq!(prompts, 2);
    Ok(())
}
#[test]
fn changed_destination_and_cancel_preserve_old_contents() -> io::Result<()> {
    let f = Fixture::new()?;
    f.trees()?;
    let e = operations::run_with_conflicts(
        &f.plan(Kind::Copy),
        CopyOptions::default(),
        &AtomicBool::new(false),
        |_| {},
        &mut |c| {
            fs::write(&c.destination, b"changed-after-prompt")?;
            Ok(Decision::Replace)
        },
    )
    .unwrap_err();
    assert!(e.to_string().contains("changed"));
    assert!(!fs::read_dir(f.0.join("dst/tree"))?.any(|e| e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".dn-part")));
    let e = operations::run_with_conflicts(
        &f.plan(Kind::Move),
        CopyOptions::default(),
        &AtomicBool::new(false),
        |_| {},
        &mut |_| Ok(Decision::Cancel),
    )
    .unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::Interrupted);
    assert!(f.0.join("src/tree/a").is_file());
    assert!(f.0.join("src/tree/b").is_file());
    Ok(())
}
#[test]
fn explicit_refuse_and_skip_still_apply_to_whole_directories() -> io::Result<()> {
    let f = Fixture::new()?;
    f.trees()?;
    for policy in [ExistingDestination::Refuse, ExistingDestination::Skip] {
        let r = operations::run_with_conflicts(
            &f.plan(Kind::Copy),
            CopyOptions {
                existing: policy,
                ..Default::default()
            },
            &AtomicBool::new(false),
            |_| {},
            &mut |_| panic!("explicit policy must not prompt"),
        );
        if policy == ExistingDestination::Refuse {
            assert!(r.is_err());
        } else {
            assert_eq!(r?.skipped, 1);
        }
    }
    Ok(())
}
#[test]
fn incompatible_destination_is_not_deleted() -> io::Result<()> {
    let f = Fixture::new()?;
    f.trees()?;
    fs::remove_file(f.0.join("dst/tree/a"))?;
    fs::create_dir(f.0.join("dst/tree/a"))?;
    assert!(operations::run_with_conflicts(
        &f.plan(Kind::Copy),
        CopyOptions::default(),
        &AtomicBool::new(false),
        |_| {},
        &mut |_| Ok(Decision::ReplaceAll)
    )
    .is_err());
    assert!(f.0.join("dst/tree/a").is_dir());
    Ok(())
}
#[cfg(unix)]
#[test]
fn symlink_destination_and_same_file_are_not_replaced() -> io::Result<()> {
    let f = Fixture::new()?;
    fs::write(f.0.join("src/a"), b"source")?;
    fs::write(f.0.join("outside"), b"keep")?;
    std::os::unix::fs::symlink(f.0.join("outside"), f.0.join("dst/a"))?;
    let mut plan = Plan {
        kind: Kind::Copy,
        sources: vec![f.0.join("src/a")],
        destination: f.0.join("dst"),
    };
    assert!(operations::run_with_conflicts(
        &plan,
        CopyOptions::default(),
        &AtomicBool::new(false),
        |_| {},
        &mut |_| panic!("links must not prompt")
    )
    .is_err());
    assert_eq!(fs::read(f.0.join("outside"))?, b"keep");
    plan.destination = f.0.join("src");
    assert!(operations::run_with_conflicts(
        &plan,
        CopyOptions::default(),
        &AtomicBool::new(false),
        |_| {},
        &mut |_| panic!("same file must not prompt")
    )
    .is_err());
    Ok(())
}

mod support;
#[test]
fn editor_save_as_requires_confirmation_and_cancel_retains_dirty_text() -> io::Result<()> {
    use crossterm::event::KeyCode as K;
    use dn::editor::{self, EditorAction as A, Modal};
    use std::time::Duration;
    let f = support::Fixture::new("save-as-replace")?.canonicalized()?;
    fs::write(f.0.join("existing.txt"), "old")?;
    let mut app = f.app()?;
    editor::open(&mut app, true);
    editor::paste(&mut app, "new contents");
    for replace in [false, true] {
        editor::action(&mut app, A::SaveAs);
        editor::paste(&mut app, "existing.txt");
        support::key(&mut app, K::Enter);
        support::wait_until(&mut app, Duration::from_secs(5), |a| {
            matches!(
                a.desktop
                    .active_editor()
                    .and_then(|e| e.modals.editor_modal()),
                Some(Modal::ConfirmOverwrite { .. })
            )
        });
        assert_eq!(fs::read(f.0.join("existing.txt"))?, b"old");
        support::draw(&mut app, 80, 25)?;
        if replace {
            support::key(&mut app, K::BackTab);
            support::key(&mut app, K::Enter);
            support::wait_until(&mut app, Duration::from_secs(5), |a| {
                a.desktop.active_editor().is_some_and(|e| !e.dirty())
            });
            assert_eq!(fs::read(f.0.join("existing.txt"))?, b"new contents");
        } else {
            support::key(&mut app, K::Enter); // Cancel is initially focused.
            assert!(app.desktop.active_editor().unwrap().dirty());
            assert_eq!(
                app.desktop
                    .active_editor()
                    .unwrap()
                    .buffer
                    .as_ref()
                    .unwrap()
                    .text,
                "new contents"
            );
        }
    }
    Ok(())
}
