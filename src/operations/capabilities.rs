//! Per-transfer publication capabilities, tied to live native volume identities.
use super::*;
use crate::{platform_fs::EntryIdentity, publication::StagedFile};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Capability {
    ExclusiveRename,
    HardLink,
    Compatibility,
    Unknown,
}
struct Entry {
    directory: PathBuf,
    identity: EntryIdentity,
    capability: Capability,
}
#[derive(Default)]
pub(super) struct Cache {
    entries: Vec<Entry>,
}
impl Cache {
    pub fn get(&mut self, target: &Path, guards: &DirectoryGuards) -> io::Result<Capability> {
        self.get_using(target, guards, detect)
    }
    fn get_using(
        &mut self,
        target: &Path,
        guards: &DirectoryGuards,
        detect: impl FnOnce(&Path) -> io::Result<Capability>,
    ) -> io::Result<Capability> {
        guards.verify_parent(target)?;
        let parent = target
            .parent()
            .ok_or_else(|| io::Error::other("Missing destination parent"))?;
        let identity = crate::platform_fs::entry_identity(parent)?;
        self.entries.retain(|entry| {
            crate::platform_fs::entry_identity(&entry.directory).ok() == Some(entry.identity)
        });
        if let Some(entry) = self
            .entries
            .iter()
            .find(|entry| entry.identity.same_volume(identity))
        {
            return Ok(entry.capability);
        }
        let capability = detect(parent)?;
        guards.verify_parent(target)?;
        if crate::platform_fs::entry_identity(parent)? != identity {
            return Err(io::Error::other(
                "Destination changed during capability detection",
            ));
        }
        self.remember(parent, identity, capability);
        Ok(capability)
    }
    fn remember(&mut self, parent: &Path, identity: EntryIdentity, capability: Capability) {
        self.entries
            .retain(|entry| !entry.identity.same_volume(identity));
        if self.entries.len() < 64 {
            self.entries.push(Entry {
                directory: parent.to_owned(),
                identity,
                capability,
            });
        }
    }
    pub fn learned_compatibility(&mut self, target: &Path) {
        if let Some(parent) = target.parent() {
            if let Ok(identity) = crate::platform_fs::entry_identity(parent) {
                self.remember(parent, identity, Capability::Compatibility);
            }
        }
    }
    #[cfg(test)]
    pub fn force(&mut self, parent: &Path, capability: Capability) -> io::Result<()> {
        self.remember(
            parent,
            crate::platform_fs::entry_identity(parent)?,
            capability,
        );
        Ok(())
    }
}

fn detect(parent: &Path) -> io::Result<Capability> {
    if let Some(capability) = native_hint(parent) {
        return Ok(capability);
    }
    match probe(parent, crate::platform_fs::rename_no_replace, |a, b| {
        fs::hard_link(a, b)
    }) {
        // Failure to query/probe is not evidence of an unsupported operation.
        Err(error) if error.kind() != io::ErrorKind::InvalidData => Ok(Capability::Unknown),
        result => result,
    }
}

#[cfg(target_os = "macos")]
fn native_hint(parent: &Path) -> Option<Capability> {
    use std::os::fd::AsRawFd;
    #[repr(C)]
    struct Reply {
        length: u32,
        capabilities: libc::vol_capabilities_attr_t,
    }
    let directory = crate::platform_fs::open_directory(parent).ok()?;
    let mut attributes: libc::attrlist = unsafe { std::mem::zeroed() };
    attributes.bitmapcount = libc::ATTR_BIT_MAP_COUNT;
    attributes.volattr = libc::ATTR_VOL_INFO | libc::ATTR_VOL_CAPABILITIES;
    let mut reply: Reply = unsafe { std::mem::zeroed() };
    // Fixed-size, read-only volume query; validity bits are required for negatives.
    if unsafe {
        libc::fgetattrlist(
            directory.as_raw_fd(),
            (&mut attributes as *mut libc::attrlist).cast(),
            (&mut reply as *mut Reply).cast(),
            std::mem::size_of::<Reply>(),
            0,
        )
    } != 0
        || (reply.length as usize) < std::mem::size_of::<Reply>()
    {
        return None;
    }
    let caps = reply.capabilities;
    from_flags(
        flag(
            caps.capabilities[libc::VOL_CAPABILITIES_INTERFACES],
            caps.valid[libc::VOL_CAPABILITIES_INTERFACES],
            libc::VOL_CAP_INT_RENAME_EXCL,
        ),
        flag(
            caps.capabilities[libc::VOL_CAPABILITIES_FORMAT],
            caps.valid[libc::VOL_CAPABILITIES_FORMAT],
            libc::VOL_CAP_FMT_HARDLINKS,
        ),
    )
}
#[cfg(not(target_os = "macos"))]
fn native_hint(_: &Path) -> Option<Capability> {
    None
}
#[cfg(any(target_os = "macos", test))]
fn flag(value: u32, valid: u32, mask: u32) -> Option<bool> {
    (valid & mask != 0).then_some(value & mask != 0)
}
#[cfg(any(target_os = "macos", test))]
fn from_flags(rename: Option<bool>, links: Option<bool>) -> Option<Capability> {
    match (rename, links) {
        (Some(true), _) => Some(Capability::ExclusiveRename),
        (_, Some(true)) => Some(Capability::HardLink),
        (Some(false), Some(false)) => Some(Capability::Compatibility),
        _ => None,
    }
}
// The alias owns only the source inode if a rename/link succeeds (even if the
// filesystem later reports an ambiguous error). Never delete an unrelated entry.
struct Alias<'a> {
    path: &'a Path,
    file: &'a fs::File,
}
impl Drop for Alias<'_> {
    fn drop(&mut self) {
        if crate::platform_fs::owns_file(self.path, self.file) {
            let _ = fs::remove_file(self.path);
        }
    }
}
fn method_probe(
    parent: &Path,
    mut operation: impl FnMut(&Path, &Path) -> io::Result<()>,
) -> io::Result<bool> {
    let source = StagedFile::create(&parent.join("probe"))?;
    let existing = StagedFile::create(&parent.join("probe"))?;
    let target = existing.path().to_owned();
    let _alias = Alias {
        path: &target,
        file: &source.file,
    };
    let collision = operation(source.path(), &target);
    if collision.is_ok()
        || !crate::platform_fs::owns_file(source.path(), &source.file)
        || !crate::platform_fs::owns_file(&target, &existing.file)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Filesystem did not preserve existing probe files during no-overwrite publication",
        ));
    }
    let collision = collision.unwrap_err();
    if collision.kind() != io::ErrorKind::AlreadyExists && !compatible::unavailable(&collision) {
        return Err(collision);
    }
    drop(existing);
    match operation(source.path(), &target) {
        Ok(())
            if collision.kind() == io::ErrorKind::AlreadyExists
                && crate::platform_fs::owns_file(&target, &source.file) =>
        {
            Ok(true)
        }
        Ok(()) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Filesystem publication capability could not be verified",
        )),
        Err(error) if compatible::unavailable(&error) => Ok(false),
        Err(error) => Err(error),
    }
}
fn probe(
    parent: &Path,
    rename: impl FnMut(&Path, &Path) -> io::Result<()>,
    link: impl FnMut(&Path, &Path) -> io::Result<()>,
) -> io::Result<Capability> {
    if method_probe(parent, rename)? {
        return Ok(Capability::ExclusiveRename);
    }
    if method_probe(parent, link)? {
        return Ok(Capability::HardLink);
    }
    Ok(Capability::Compatibility)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> io::Result<Self> {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "dn-capabilities-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path)?;
            Ok(Self(fs::canonicalize(path)?))
        }
        fn guards(&self, target: &Path) -> io::Result<DirectoryGuards> {
            let mut guards = DirectoryGuards::default();
            guards.capture_parent(target)?;
            Ok(guards)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn unsupported(_: &Path, _: &Path) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "unsupported"))
    }
    #[test]
    fn native_negative_flags_require_validity_and_both_missing_operations() {
        assert_eq!(flag(0, 0, 1), None);
        assert_eq!(flag(0, 1, 1), Some(false));
        assert_eq!(flag(1, 1, 1), Some(true));
        assert_eq!(from_flags(Some(false), None), None);
        assert_eq!(from_flags(None, Some(false)), None);
        assert_eq!(
            from_flags(Some(false), Some(false)),
            Some(Capability::Compatibility)
        );
        assert_eq!(
            from_flags(Some(true), Some(false)),
            Some(Capability::ExclusiveRename)
        );
        assert_eq!(
            from_flags(Some(false), Some(true)),
            Some(Capability::HardLink)
        );
    }
    #[test]
    fn unsupported_probes_are_bounded_and_remove_all_private_files() -> io::Result<()> {
        let t = Fixture::new()?;
        let mut calls = 0;
        assert_eq!(
            probe(
                &t.0,
                |a, b| {
                    calls += 1;
                    unsupported(a, b)
                },
                unsupported
            )?,
            Capability::Compatibility
        );
        assert_eq!(calls, 2);
        assert_eq!(fs::read_dir(&t.0)?.count(), 0);
        Ok(())
    }
    #[test]
    fn native_probe_tests_collision_and_cleans_successful_aliases() -> io::Result<()> {
        let t = Fixture::new()?;
        let mode = probe(&t.0, crate::platform_fs::rename_no_replace, |a, b| {
            fs::hard_link(a, b)
        });
        // A driver may refuse probing; the production detector then keeps Unknown.
        if let Ok(mode) = mode {
            assert_ne!(mode, Capability::Unknown);
        }
        assert_eq!(fs::read_dir(&t.0)?.count(), 0);
        Ok(())
    }
    #[test]
    fn overwrite_or_ambiguous_failure_never_enables_a_fast_path() -> io::Result<()> {
        let t = Fixture::new()?;
        assert!(method_probe(&t.0, |a, b| fs::rename(a, b)).is_err());
        assert_eq!(fs::read_dir(&t.0)?.count(), 0);
        let mut calls = 0;
        let error = probe(
            &t.0,
            |a, b| {
                calls += 1;
                if calls == 1 {
                    return Err(io::ErrorKind::AlreadyExists.into());
                }
                crate::platform_fs::rename_no_replace(a, b)?;
                Err(io::Error::other("ambiguous server failure"))
            },
            |_, _| panic!("ambiguous rename must not attempt hard linking"),
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert_eq!(fs::read_dir(&t.0)?.count(), 0);
        Ok(())
    }
    #[test]
    fn denied_and_readonly_probes_are_not_unsupported() -> io::Result<()> {
        let t = Fixture::new()?;
        for kind in [
            io::ErrorKind::PermissionDenied,
            io::ErrorKind::ReadOnlyFilesystem,
            io::ErrorKind::StorageFull,
            io::ErrorKind::Other,
        ] {
            let error = probe(
                &t.0,
                |_, _| Err(io::Error::new(kind, "device failure")),
                |_, _| panic!("must not infer missing rename support"),
            )
            .unwrap_err();
            assert_eq!(error.kind(), kind);
            assert_eq!(fs::read_dir(&t.0)?.count(), 0);
        }
        Ok(())
    }
    #[test]
    fn cache_reuses_same_volume_but_discards_replaced_parent_and_obeys_budget() -> io::Result<()> {
        let t = Fixture::new()?;
        let parent = t.0.join("parent");
        fs::create_dir(&parent)?;
        let target = parent.join("file");
        let guards = t.guards(&target)?;
        let mut cache = Cache::default();
        assert_eq!(
            cache.get_using(&target, &guards, |_| Ok(Capability::Compatibility))?,
            Capability::Compatibility
        );
        assert_eq!(
            cache.get_using(&target, &guards, |_| panic!("must reuse confirmed volume"))?,
            Capability::Compatibility
        );
        fs::rename(&parent, t.0.join("held"))?;
        fs::create_dir(&parent)?;
        assert!(cache
            .get_using(&target, &guards, |_| panic!(
                "stale parent must be rejected first"
            ))
            .is_err());
        let guards = t.guards(&target)?;
        assert_eq!(
            cache.get_using(&target, &guards, |_| Ok(Capability::Unknown))?,
            Capability::Unknown
        );
        let mut identity = crate::platform_fs::entry_identity(&parent)?;
        for _ in 0..100 {
            identity = identity.on_other_volume();
            cache.remember(&parent, identity, Capability::HardLink);
        }
        assert_eq!(cache.entries.len(), 64);
        Ok(())
    }
}
