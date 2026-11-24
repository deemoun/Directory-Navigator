//! Cached filename filtering; native extension keys never come from display text.
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    io,
    path::Path,
};

pub const MAX_TYPES: usize = 256;
pub const MAX_SELECTION_BYTES: usize = 4096;
pub type Extension = Option<OsString>;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TypesFilter {
    pub hide: bool,
    pub selected: BTreeSet<Extension>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FilenameFilter {
    Wildcard(String),
    Types(TypesFilter),
}
#[derive(Clone, Debug)]
pub struct TypeRow {
    pub extension: Extension,
    pub count: usize,
}
pub fn extension(name: &std::ffi::OsStr) -> Extension {
    Path::new(name).extension().map(ToOwned::to_owned)
}
impl TypesFilter {
    pub fn matches(&self, entry: &crate::filesystem::Entry) -> bool {
        entry.parent
            || entry.directory
            || self.selected.is_empty()
            || self.selected.contains(&extension(&entry.name)) != self.hide
    }
    pub fn encoded(&self) -> Vec<String> {
        self.selected
            .iter()
            .map(|key| match key {
                None => String::new(),
                Some(ext) => {
                    let mut name = OsString::from("x.");
                    name.push(ext);
                    crate::bookmarks::encode(Path::new(&name))
                }
            })
            .collect()
    }
    pub fn decode(hide: bool, values: &[String]) -> io::Result<Self> {
        let mut selected = BTreeSet::new();
        if values.len() > MAX_TYPES {
            return Err(invalid());
        }
        for value in values {
            let key = if value.is_empty() {
                None
            } else {
                let name = crate::bookmarks::decode(value).ok_or_else(invalid)?;
                let ext = name.extension().ok_or_else(invalid)?.to_owned();
                let mut expected = OsString::from("x.");
                expected.push(&ext);
                if name.as_os_str() != expected {
                    return Err(invalid());
                }
                Some(ext)
            };
            if !selected.insert(key) {
                return Err(invalid());
            }
        }
        let filter = Self { hide, selected };
        filter.validate()?;
        Ok(filter)
    }
    pub fn validate(&self) -> io::Result<()> {
        for ext in self.selected.iter().flatten() {
            let mut name = OsString::from("x.");
            name.push(ext);
            if Path::new(&name).extension() != Some(ext.as_os_str()) {
                return Err(invalid());
            }
        }
        let mut table = toml::Table::new();
        table.insert("hide".into(), toml::Value::Boolean(self.hide));
        table.insert(
            "extensions".into(),
            toml::Value::Array(
                self.encoded()
                    .into_iter()
                    .map(toml::Value::String)
                    .collect(),
            ),
        );
        if self.selected.len() > MAX_TYPES
            || toml::to_string(&table).map_err(io::Error::other)?.len() > MAX_SELECTION_BYTES
        {
            return Err(invalid());
        }
        Ok(())
    }
}
fn invalid() -> io::Error {
    io::Error::other(
        "File type selection exceeds 256 types / 4 KiB or contains invalid native extensions.",
    )
}
impl super::Panel {
    pub fn filename_filter(&self) -> FilenameFilter {
        self.types_filter.clone().map_or_else(
            || FilenameFilter::Wildcard(self.filter.clone()),
            FilenameFilter::Types,
        )
    }
    pub fn set_filename_filter(&mut self, filter: FilenameFilter) -> io::Result<()> {
        if self.loading {
            return Err(io::Error::other("Wait for directory navigation to finish."));
        }
        match filter {
            FilenameFilter::Wildcard(pattern) => self.set_filter(pattern),
            FilenameFilter::Types(types) => {
                types.validate()?;
                self.filter.clear();
                self.types_filter = (!types.selected.is_empty()).then_some(types);
                self.apply_filter_current();
            }
        }
        Ok(())
    }
    pub fn file_types(&self) -> (Vec<TypeRow>, bool) {
        let mut counts = BTreeMap::<Extension, usize>::from([(None, 0)]);
        let mut limited = false;
        // A restored selection stays editable even if those types have disappeared.
        if let Some(types) = &self.types_filter {
            counts.extend(types.selected.iter().cloned().map(|key| (key, 0)));
        }
        for entry in self
            .entries
            .iter()
            .chain(&self.filtered_out)
            .filter(|e| !e.parent && !e.directory)
        {
            let key = extension(&entry.name);
            *counts.entry(key).or_default() += 1;
            if counts.len() > MAX_TYPES {
                limited = true;
                let key = counts
                    .keys()
                    .rev()
                    .find(|key| {
                        self.types_filter
                            .as_ref()
                            .is_none_or(|f| !f.selected.contains(*key))
                    })
                    .cloned()
                    .expect("at most 256 selected types");
                counts.remove(&key);
            }
        }
        (
            counts
                .into_iter()
                .map(|(extension, count)| TypeRow { extension, count })
                .collect(),
            limited,
        )
    }
}
