use super::{check, verify_entry, DirectoryGuards, Source};
use std::{
    fs,
    io::{self, Read, Seek},
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExistingDestination {
    #[default]
    Refuse,
    Skip,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CopyOptions {
    pub existing: ExistingDestination,
    pub check_free_space: bool,
    pub verify_writes: bool,
    pub remove_sources: bool,
}
pub(super) fn preflight(
    pairs: &[(Source, PathBuf)],
    guards: &mut DirectoryGuards,
    cancel: &AtomicBool,
    space: &impl Fn(&Path) -> io::Result<u128>,
) -> io::Result<()> {
    let mut required = 0_u128;
    for (source, _) in pairs {
        let mut stack = vec![(source.path.clone(), source.metadata.clone(), 0_usize)];
        while let Some((path, metadata, depth)) = stack.pop() {
            check(cancel)?;
            guards.verify_parent(&path)?;
            verify_entry(&path, &metadata)?;
            if metadata.is_file() {
                required = required
                    .checked_add(u128::from(metadata.len()))
                    .ok_or_else(|| io::Error::other("Copy size overflow"))?;
            } else if metadata.is_dir() {
                if depth >= 512 {
                    return Err(io::Error::other(
                        "Directory nesting exceeds the free-space check limit",
                    ));
                }
                guards.capture(&path)?;
                for child in fs::read_dir(&path)? {
                    check(cancel)?;
                    guards.verify_directory(&path)?;
                    let child = child?;
                    let child_path = child.path();
                    stack.push((
                        child_path.clone(),
                        fs::symlink_metadata(&child_path)?,
                        depth + 1,
                    ));
                }
            } else if !metadata.is_symlink() {
                return Err(io::Error::other(
                    "Unsupported special file during space check",
                ));
            }
        }
    }
    let target = &pairs[0].1;
    guards.verify_parent(target)?;
    let available = space(
        target
            .parent()
            .ok_or_else(|| io::Error::other("Missing destination parent"))?,
    )?;
    if required > available {
        return Err(io::Error::other(format!(
            "Not enough free disk space: {required} bytes required, {available} available"
        )));
    }
    Ok(())
}
pub(super) fn verify(
    input: &mut fs::File,
    output: &mut fs::File,
    length: u64,
    cancel: &AtomicBool,
) -> io::Result<()> {
    input.rewind()?;
    output.rewind()?;
    let (mut a, mut b) = (vec![0; 128 * 1024], vec![0; 128 * 1024]);
    let mut remaining = length;
    while remaining > 0 {
        check(cancel)?;
        let n = remaining.min(a.len() as u64) as usize;
        input.read_exact(&mut a[..n])?;
        output.read_exact(&mut b[..n])?;
        if a[..n] != b[..n] {
            return Err(io::Error::other(
                "Write verification failed; source retained",
            ));
        }
        remaining -= n as u64;
    }
    check(cancel)?;
    if input.read(&mut a[..1])? != 0 || output.read(&mut b[..1])? != 0 {
        return Err(io::Error::other(
            "File size changed during write verification",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::{Kind, MoveStrategy, Plan};
    use std::sync::atomic::{AtomicU64, Ordering};
    #[test]
    fn insufficient_or_unavailable_space_fails_before_any_output_is_created() -> io::Result<()> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        struct Temp(PathBuf);
        impl Drop for Temp {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let t = Temp(std::env::temp_dir().join(format!(
            "dn-copy-space-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )));
        fs::create_dir(&t.0)?;
        fs::write(t.0.join("source"), vec![1; 4096])?;
        let plan = Plan {
            kind: Kind::Copy,
            sources: vec![t.0.join("source")],
            destination: t.0.join("dest"),
        };
        for unavailable in [false, true] {
            let r = crate::operations::run_checked(
                &plan,
                CopyOptions {
                    check_free_space: true,
                    ..Default::default()
                },
                &AtomicBool::new(false),
                |_| {},
                MoveStrategy::PreferRename,
                |_| {
                    if unavailable {
                        Err(io::Error::other("Space unavailable"))
                    } else {
                        Ok(0)
                    }
                },
            );
            assert!(r.is_err());
            assert!(!t.0.join("dest").exists());
            assert_eq!(fs::read(t.0.join("source"))?, vec![1; 4096]);
            assert_eq!(fs::read_dir(&t.0)?.count(), 1);
        }
        Ok(())
    }
}
