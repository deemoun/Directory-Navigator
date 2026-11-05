//! APK Workshop backend. ZIP operations reuse DN's bounded, identity-checked
//! archive reader. Binary Android data is inspected without invoking apktool.
mod binary;
pub mod dex;
mod job;
pub mod manifest;
pub mod project;
pub mod resources;
pub mod tools;
use crate::{
    archive::reader::{self, Catalog, Kind},
    task::Cancellation,
};
use std::{io, path::PathBuf, sync::Arc};
pub struct Package {
    pub catalog: Arc<Catalog>,
    pub manifest: manifest::Manifest,
    pub dex: Vec<(String, Result<dex::Dex, String>)>,
    pub notes: Vec<String>,
    pub resources: Option<resources::Resources>,
}
impl Package {
    pub fn open(path: PathBuf, cancel: &Cancellation) -> io::Result<Self> {
        let catalog = Arc::new(reader::list_zip(path, cancel)?);
        if catalog.note.is_some() || catalog.skipped != 0 {
            return Err(io::Error::other(
                "APK catalog is incomplete or contains unsupported members",
            ));
        }
        let decoded_size = catalog
            .nodes
            .iter()
            .filter(|n| n.kind == Kind::File)
            .try_fold(0u64, |sum, n| {
                sum.checked_add(n.size)
                    .ok_or_else(|| io::Error::other("APK member size overflow"))
            })?;
        if decoded_size > reader::MAX_STREAM {
            return Err(io::Error::other("APK decoded size exceeds 256 MiB"));
        }
        let id = catalog
            .nodes
            .iter()
            .position(|n| n.path == b"AndroidManifest.xml")
            .ok_or_else(|| io::Error::other("APK has no AndroidManifest.xml"))?;
        let manifest = manifest::Manifest::parse(
            &reader::zip_bytes(&catalog, id, 8 * 1024 * 1024, cancel)?,
            cancel,
        )?;
        if manifest.split || catalog.nodes.iter().any(|n| n.path == b"toc.pb") {
            return Err(io::Error::other("APK sets are not supported"));
        }
        let mut dex = Vec::new();
        let mut total = 0;
        for (id, node) in catalog.nodes.iter().enumerate() {
            cancel.check()?;
            if node.kind == Kind::File
                && node.path.starts_with(b"classes")
                && node.path.ends_with(b".dex")
                && !node.path.contains(&b'/')
            {
                total += node.size;
                if total > 128 * 1024 * 1024 || dex.len() >= 64 {
                    return Err(io::Error::other("APK DEX inspection exceeds limit"));
                }
                let value = reader::zip_bytes(&catalog, id, 64 * 1024 * 1024, cancel)
                    .and_then(|data| dex::Dex::parse(data, cancel))
                    .map_err(|e| e.to_string());
                dex.push((String::from_utf8_lossy(&node.path).into_owned(), value));
            }
        }
        let mut notes = Vec::new();
        for (name, value) in &dex {
            if let Err(e) = value {
                notes.push(format!("{name}: {e}"));
            }
        }
        let resources = if let Some(id) = catalog
            .nodes
            .iter()
            .position(|n| n.path == b"resources.arsc")
        {
            match reader::zip_bytes(&catalog, id, 32 * 1024 * 1024, cancel)
                .and_then(|data| resources::Resources::parse(&data, cancel))
            {
                Ok(r) => Some(r),
                Err(e) => {
                    notes.push(format!("resources.arsc: {e}"));
                    None
                }
            }
        } else {
            None
        };
        catalog.validate()?;
        Ok(Self {
            catalog,
            manifest,
            dex,
            notes,
            resources,
        })
    }
    pub fn overview(&self) -> Vec<String> {
        self.overview_manifest(&self.manifest)
    }
    pub fn overview_manifest(&self, m: &manifest::Manifest) -> Vec<String> {
        let mut rows = vec![
            format!("Package: {}", m.package),
            format!("Version: {} ({})", m.version_name, m.version_code),
            format!("SDK: min {} / target {}", m.min_sdk, m.target_sdk),
            format!("Debuggable: {}", m.debuggable),
            format!(
                "Files: {}",
                self.catalog
                    .nodes
                    .iter()
                    .filter(|n| n.kind == Kind::File)
                    .count()
            ),
        ];
        let mut sizes = std::collections::BTreeMap::<String, u64>::new();
        let mut abi = std::collections::BTreeSet::new();
        for n in &self.catalog.nodes {
            if n.kind != Kind::File {
                continue;
            }
            let p = String::from_utf8_lossy(&n.path);
            let group = p.split('/').next().unwrap_or("root").to_owned();
            *sizes.entry(group).or_default() += n.size;
            if p.starts_with("lib/") {
                if let Some(a) = p.split('/').nth(1) {
                    abi.insert(a.to_owned());
                }
            }
        }
        rows.push(format!(
            "ABI: {}",
            abi.into_iter().collect::<Vec<_>>().join(", ")
        ));
        rows.extend(
            sizes
                .into_iter()
                .map(|(name, size)| format!("{name}: {size} bytes")),
        );
        rows.push("Permissions:".into());
        rows.extend(m.permissions.iter().cloned());
        rows.push("Components:".into());
        rows.extend(m.components.iter().cloned());
        rows.extend(
            self.notes
                .iter()
                .map(|s| format!("Partial inspection: {s}")),
        );
        rows
    }
}
