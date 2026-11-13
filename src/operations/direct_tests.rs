use super::*;
use capabilities::{Cache, Capability};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> io::Result<Self> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "dn-direct-copy-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path)?;
        let path = fs::canonicalize(path)?;
        fs::create_dir(path.join("in"))?;
        fs::create_dir(path.join("out"))?;
        Ok(Self(path))
    }
    fn plan(&self, kind: Kind) -> Plan {
        Plan {
            kind,
            sources: vec![self.0.join("in/Ёжик.txt")],
            destination: self.0.join("out"),
        }
    }
    fn run(
        &self,
        plan: &Plan,
        options: CopyOptions,
        cancel: &AtomicBool,
        notify: impl FnMut(&Progress),
        ask: &mut conflict::Ask<'_>,
    ) -> io::Result<Progress> {
        let mut cache = Cache::default();
        cache.force(&self.0.join("out"), Capability::Compatibility)?;
        run_with_capabilities(
            plan,
            options,
            cancel,
            notify,
            MoveStrategy::Copy,
            |_| Ok(u128::MAX),
            None,
            ask,
            &mut cache,
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn refuse() -> CopyOptions {
    CopyOptions {
        existing: ExistingDestination::Refuse,
        ..Default::default()
    }
}
#[test]
fn direct_copy_writes_once_and_verifies_even_when_checkbox_is_off() -> io::Result<()> {
    for data in [Vec::new(), vec![0xA5; 2 * 1024 * 1024 + 7]] {
        let t = Fixture::new()?;
        let plan = t.plan(Kind::Copy);
        fs::write(&plan.sources[0], &data)?;
        let mut verified = false;
        let result = t.run(
            &plan,
            refuse(),
            &AtomicBool::new(false),
            |p| {
                verified |= p.phase == Phase::Verifying;
            },
            &mut conflict::no_prompt,
        )?;
        assert!(verified);
        assert_eq!(result.bytes, data.len() as u64);
        assert_eq!(result.written, result.bytes);
        assert_eq!(fs::read(t.0.join("out/Ёжик.txt"))?, data);
        assert_eq!(fs::read(&plan.sources[0])?, data);
        assert_eq!(fs::read_dir(t.0.join("out"))?.count(), 1);
    }
    Ok(())
}
#[test]
fn direct_moves_remove_sources_only_after_verified_success() -> io::Result<()> {
    for remove_sources in [false, true] {
        let t = Fixture::new()?;
        let plan = t.plan(if remove_sources {
            Kind::Copy
        } else {
            Kind::Move
        });
        fs::write(&plan.sources[0], "original")?;
        let mut options = refuse();
        options.remove_sources = remove_sources;
        t.run(
            &plan,
            options,
            &AtomicBool::new(false),
            |_| {},
            &mut conflict::no_prompt,
        )?;
        assert!(!plan.sources[0].exists());
        assert_eq!(fs::read(t.0.join("out/Ёжик.txt"))?, b"original");
    }
    Ok(())
}
#[test]
fn direct_cancellation_corruption_and_source_changes_keep_sources_and_remove_partial_output(
) -> io::Result<()> {
    for scenario in ["cancel", "corrupt", "source"] {
        let t = Fixture::new()?;
        let plan = t.plan(Kind::Move);
        let target = t.0.join("out/Ёжик.txt");
        fs::write(&plan.sources[0], vec![7; 400_000])?;
        let cancel = AtomicBool::new(false);
        let mut injected = false;
        let result = t.run(
            &plan,
            refuse(),
            &cancel,
            |p| {
                if injected {
                    return;
                }
                if scenario == "cancel" && p.bytes > 0 {
                    cancel.store(true, Ordering::Relaxed);
                    injected = true;
                } else if p.phase == Phase::Verifying {
                    injected = true;
                    if scenario == "corrupt" {
                        fs::OpenOptions::new()
                            .write(true)
                            .open(&target)
                            .unwrap()
                            .write_all(b"X")
                            .unwrap();
                    } else {
                        fs::write(&plan.sources[0], b"external change").unwrap();
                    }
                }
            },
            &mut conflict::no_prompt,
        );
        assert!(injected);
        assert!(result.is_err());
        assert!(plan.sources[0].exists());
        assert!(!target.exists());
    }
    Ok(())
}
#[test]
fn direct_cleanup_does_not_remove_a_replaced_destination() -> io::Result<()> {
    let t = Fixture::new()?;
    let plan = t.plan(Kind::Copy);
    let target = t.0.join("out/Ёжик.txt");
    fs::write(&plan.sources[0], "original")?;
    let mut replaced = false;
    let result = t.run(
        &plan,
        refuse(),
        &AtomicBool::new(false),
        |p| {
            if !replaced && p.phase == Phase::Verifying {
                replaced = true;
                fs::rename(&target, t.0.join("held-output")).unwrap();
                fs::write(&target, "replacement").unwrap();
            }
        },
        &mut conflict::no_prompt,
    );
    assert!(result.is_err());
    assert_eq!(fs::read(&target)?, b"replacement");
    assert!(plan.sources[0].exists());
    Ok(())
}
#[test]
fn existing_destinations_still_use_conflict_approval_and_staged_replacement() -> io::Result<()> {
    let t = Fixture::new()?;
    let plan = t.plan(Kind::Copy);
    let target = t.0.join("out/Ёжик.txt");
    fs::write(&plan.sources[0], "new")?;
    fs::write(&target, "old")?;
    let mut options = CopyOptions::default();
    t.run(&plan, options, &AtomicBool::new(false), |_| {}, &mut |_| {
        Ok(conflict::Decision::Skip)
    })?;
    assert_eq!(fs::read(&target)?, b"old");
    let mut called = false;
    t.run(
        &plan,
        options,
        &AtomicBool::new(false),
        |p| {
            // The approved old target remains intact until staged publication.
            if p.phase == Phase::Copying && p.bytes > 0 && p.files == 0 {
                assert_eq!(fs::read(&target).unwrap(), b"old");
            }
        },
        &mut |_| {
            called = true;
            Ok(conflict::Decision::Replace)
        },
    )?;
    assert!(called);
    assert_eq!(fs::read(&target)?, b"new");
    options.existing = ExistingDestination::Refuse;
    assert!(t
        .run(
            &plan,
            options,
            &AtomicBool::new(false),
            |_| {},
            &mut conflict::no_prompt
        )
        .is_err());
    Ok(())
}
#[test]
fn direct_directory_merge_includes_hidden_entries_empty_folders_and_available_times(
) -> io::Result<()> {
    let t = Fixture::new()?;
    let source = t.0.join("in/tree");
    fs::create_dir(&source)?;
    fs::write(source.join(".hidden"), b"hidden")?;
    fs::create_dir(source.join("empty"))?;
    fs::create_dir(t.0.join("out/tree"))?;
    fs::write(t.0.join("out/tree/unrelated"), b"keep")?;
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(source.join(".hidden"))?;
    let time = std::time::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
    file.set_times(fs::FileTimes::new().set_modified(time))?;
    let actual = file.metadata()?.modified()?;
    drop(file);
    let plan = Plan {
        kind: Kind::Copy,
        sources: vec![source],
        destination: t.0.join("out"),
    };
    t.run(
        &plan,
        CopyOptions::default(),
        &AtomicBool::new(false),
        |_| {},
        &mut |_| panic!("merging directories does not require a file decision"),
    )?;
    assert_eq!(fs::read(t.0.join("out/tree/unrelated"))?, b"keep");
    assert_eq!(fs::read(t.0.join("out/tree/.hidden"))?, b"hidden");
    assert!(t.0.join("out/tree/empty").is_dir());
    assert_eq!(
        fs::metadata(t.0.join("out/tree/.hidden"))?.modified()?,
        actual
    );
    Ok(())
}
#[test]
fn device_space_and_file_size_failures_clean_owned_direct_output() -> io::Result<()> {
    struct Failing<'a> {
        file: &'a mut fs::File,
        remaining: usize,
        kind: io::ErrorKind,
    }
    impl Write for Failing<'_> {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.remaining == 0 {
                return Err(io::Error::new(self.kind, "injected filesystem limit"));
            }
            let n = bytes.len().min(self.remaining);
            let n = self.file.write(&bytes[..n])?;
            self.remaining -= n;
            Ok(n)
        }
        fn flush(&mut self) -> io::Result<()> {
            self.file.flush()
        }
    }
    for kind in [io::ErrorKind::StorageFull, io::ErrorKind::FileTooLarge] {
        let t = Fixture::new()?;
        let target = t.0.join("out/partial");
        let result = {
            let mut output = compatible::Output::create(&target)?;
            copy_bytes(
                &mut b"larger than device capacity".as_slice(),
                &mut Failing {
                    file: &mut output.file,
                    remaining: 3,
                    kind,
                },
                26,
                &mut [0; 8],
                &AtomicBool::new(false),
                |_| Ok(()),
            )
        };
        assert_eq!(result.unwrap_err().kind(), kind);
        assert!(!target.exists());
    }
    Ok(())
}

#[test]
fn native_case_collision_rules_protect_existing_contents() -> io::Result<()> {
    let t = Fixture::new()?;
    let plan = t.plan(Kind::Copy);
    fs::write(&plan.sources[0], "new")?;
    let existing = t.0.join("out/ёжик.txt");
    let target = t.0.join("out/Ёжик.txt");
    fs::write(&existing, "existing")?;
    let aliases = target.exists();
    let result = t.run(
        &plan,
        refuse(),
        &AtomicBool::new(false),
        |_| {},
        &mut conflict::no_prompt,
    );
    if aliases {
        assert!(result.is_err());
    } else {
        result?;
        assert_eq!(fs::read(target)?, b"new");
    }
    assert_eq!(fs::read(existing)?, b"existing");
    Ok(())
}

#[test]
fn completed_direct_items_survive_later_cancellation_without_move_source_removal() -> io::Result<()>
{
    let t = Fixture::new()?;
    let first = t.0.join("in/first");
    let second = t.0.join("in/second");
    fs::write(&first, "one")?;
    fs::write(&second, "two")?;
    let plan = Plan {
        kind: Kind::Move,
        sources: vec![first.clone(), second.clone()],
        destination: t.0.join("out"),
    };
    let cancel = AtomicBool::new(false);
    let result = t.run(
        &plan,
        refuse(),
        &cancel,
        |p| {
            if p.files == 1 {
                cancel.store(true, Ordering::Relaxed);
            }
        },
        &mut conflict::no_prompt,
    );
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Interrupted);
    assert_eq!(fs::read(t.0.join("out/first"))?, b"one");
    assert_eq!(fs::read(first)?, b"one");
    assert_eq!(fs::read(second)?, b"two");
    Ok(())
}
