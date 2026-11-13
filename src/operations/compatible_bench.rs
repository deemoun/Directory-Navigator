//! Explicit private-fixture benchmark; no timing assertions or normal CI execution.
use super::*;
use capabilities::{Cache, Capability};
use std::time::Instant;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> io::Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "dn-copy-bench-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&path)?;
        Ok(Self(fs::canonicalize(path)?))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn two_pass_baseline(sources: &[PathBuf], destination: &Path) -> io::Result<u64> {
    let mut written = 0;
    let cancel = AtomicBool::new(false);
    let mut buffer = vec![0; 128 * 1024];
    for source in sources {
        let target = destination.join(source.file_name().unwrap());
        let mut input = open_regular(source)?;
        let metadata = input.metadata()?;
        let mut stage = crate::publication::StagedFile::create(&target)?;
        copy_bytes(
            &mut input,
            &mut stage.file,
            metadata.len(),
            &mut buffer,
            &cancel,
            |n| {
                written += n;
                Ok(())
            },
        )?;
        stage.file.sync_all()?;
        crate::platform_fs::copy_extended_metadata(&input, &stage.file)?;
        crate::platform_fs::copy_basic_metadata(&stage.file, &metadata)?;
        stage.file.sync_all()?;
        // The prior publication attempt synchronized staging again before it
        // discovered unsupported rename/link operations.
        stage.file.sync_all()?;
        let mut staged_input = stage.file.try_clone()?;
        staged_input.rewind()?;
        let mut output = Output::create(&target)?;
        copy_bytes(
            &mut staged_input,
            &mut output.file,
            metadata.len(),
            &mut buffer,
            &cancel,
            |n| {
                written += n;
                Ok(())
            },
        )?;
        output.file.sync_all()?;
        copy_options::verify(&mut staged_input, &mut output.file, metadata.len(), &cancel)?;
        crate::platform_fs::copy_extended_metadata(&stage.file, &output.file)?;
        crate::platform_fs::copy_basic_metadata(&output.file, &metadata)?;
        output.file.sync_all()?;
        output.commit()?;
    }
    Ok(written)
}
#[test]
#[ignore = "manual transfer benchmark: writes private large-file and small-file fixtures"]
fn verified_two_pass_vs_direct() -> io::Result<()> {
    let t = Fixture::new()?;
    for (label, count, length) in [("large", 1, 64 * 1024 * 1024), ("small", 256, 4096)] {
        let input = t.0.join(format!("{label}-input"));
        fs::create_dir(&input)?;
        let mut sources = Vec::new();
        let data = vec![0x5A; 128 * 1024];
        for index in 0..count {
            let path = input.join(format!("file-{index:04}"));
            let mut file = fs::File::create(&path)?;
            let mut remaining = length;
            while remaining > 0 {
                let n = remaining.min(data.len());
                file.write_all(&data[..n])?;
                remaining -= n;
            }
            file.sync_all()?;
            sources.push(path);
        }
        let total = (count * length) as u64;
        let mut baseline = Vec::new();
        let mut direct = Vec::new();
        for round in 0..3 {
            for old in if round % 2 == 0 {
                [true, false]
            } else {
                [false, true]
            } {
                let destination = t.0.join(format!("{label}-{round}-{old}"));
                fs::create_dir(&destination)?;
                let start = Instant::now();
                let written = if old {
                    two_pass_baseline(&sources, &destination)?
                } else {
                    let mut cache = Cache::default();
                    cache.force(&destination, Capability::Compatibility)?;
                    let plan = Plan {
                        kind: Kind::Copy,
                        sources: sources.clone(),
                        destination: destination.clone(),
                    };
                    run_with_capabilities(
                        &plan,
                        CopyOptions {
                            existing: ExistingDestination::Refuse,
                            ..Default::default()
                        },
                        &AtomicBool::new(false),
                        |_| {},
                        MoveStrategy::Copy,
                        |_| Ok(u128::MAX),
                        None,
                        &mut conflict::no_prompt,
                        &mut cache,
                    )?
                    .written
                };
                let elapsed = start.elapsed();
                if old {
                    baseline.push(elapsed);
                } else {
                    direct.push(elapsed);
                }
                assert_eq!(written, total * if old { 2 } else { 1 });
                for source in &sources {
                    copy_options::verify(
                        &mut fs::File::open(source)?,
                        &mut fs::File::open(destination.join(source.file_name().unwrap()))?,
                        fs::metadata(source)?.len(),
                        &AtomicBool::new(false),
                    )?;
                }
                assert_eq!(fs::read_dir(&destination)?.count(), sources.len());
                fs::remove_dir_all(destination)?;
            }
        }
        baseline.sort();
        direct.sort();
        println!("{label}: payload={total} bytes, files={count}, median of 3: two-pass={:.3}s, direct={:.3}s; destination writes {} -> {} bytes",baseline[1].as_secs_f64(),direct[1].as_secs_f64(),total*2,total);
    }
    Ok(())
}
