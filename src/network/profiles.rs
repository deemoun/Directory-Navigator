//! Metadata-only profiles. Passwords/passphrases are never serialized.
use super::remote::*;
use std::{
    io,
    path::{Path, PathBuf},
};
const LIMIT: u64 = 64 * 1024;
pub fn load(path: &Path) -> io::Result<Vec<Profile>> {
    let mut file = match crate::filesystem::open_regular(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e),
    };
    use std::io::Read;
    let mut text = String::new();
    file.by_ref().take(LIMIT + 1).read_to_string(&mut text)?;
    if text.len() as u64 > LIMIT {
        return Err(io::Error::other("Remote profiles exceed 64 KiB"));
    }
    decode(&text)
}
fn decode(text: &str) -> io::Result<Vec<Profile>> {
    let data: toml::Table = text.parse().map_err(io::Error::other)?;
    let mut profiles = Vec::new();
    if data.get("connections").is_some_and(|v| !v.is_array()) {
        return Err(io::Error::other("Invalid remote profile list"));
    }
    let mut names = std::collections::HashSet::new();
    if let Some(records) = data.get("connections").and_then(toml::Value::as_array) {
        if records.len() > 128 {
            return Err(io::Error::other("Remote profile limit reached"));
        }
        for record in records {
            let get = |k: &str| {
                record
                    .get(k)
                    .and_then(toml::Value::as_str)
                    .ok_or_else(|| io::Error::other("Invalid remote profile"))
            };
            let protocol = match get("protocol")? {
                "SFTP" => Protocol::Sftp,
                "FTP" => Protocol::Ftp,
                "FTPS" => Protocol::Ftps,
                _ => return Err(io::Error::other("Unsupported profile protocol")),
            };
            let port = record
                .get("port")
                .and_then(toml::Value::as_integer)
                .and_then(|v| u16::try_from(v).ok())
                .ok_or_else(|| io::Error::other("Invalid remote port"))?;
            let p = Profile {
                name: get("name")?.into(),
                host: get("host")?.into(),
                user: get("user")?.into(),
                protocol,
                port,
                directory: RemotePath::new(get("directory")?)?,
                key: record
                    .get("key")
                    .and_then(toml::Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(PathBuf::from),
                fingerprint: record
                    .get("fingerprint")
                    .and_then(toml::Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned),
            };
            p.validate()?;
            if !names.insert(p.name.clone()) {
                return Err(io::Error::other("Duplicate remote profile name"));
            }
            profiles.push(p);
        }
    }
    Ok(profiles)
}

pub fn create(path: &Path, profile: &Profile) -> io::Result<Option<String>> {
    update(path, Change::Create(profile))
}
pub fn replace(path: &Path, original: &Profile, profile: &Profile) -> io::Result<Option<String>> {
    update(path, Change::Replace(original, profile))
}
pub fn delete(path: &Path, original: &Profile) -> io::Result<Option<String>> {
    update(path, Change::Delete(original))
}
enum Change<'a> {
    Create(&'a Profile),
    Replace(&'a Profile, &'a Profile),
    Delete(&'a Profile),
}
fn update(path: &Path, change: Change<'_>) -> io::Result<Option<String>> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("Missing profile directory"))?;
    std::fs::create_dir_all(parent)?;
    let _lock = crate::config::store::SaveLock::acquire(path)?;
    let (mut root, original) = match crate::editor::storage::load(path) {
        Ok((buffer, snapshot, _)) => {
            if buffer.text.len() as u64 > LIMIT {
                return Err(io::Error::other("Remote profiles exceed 64 KiB"));
            }
            let root: toml::Table = buffer.text.parse().map_err(io::Error::other)?;
            (root, Some(snapshot))
        }
        Err(_) if !path.try_exists()? => (toml::Table::new(), None),
        Err(error) => return Err(io::Error::other(error)),
    };
    let profiles = decode(&root.to_string())?;
    let records = root
        .entry("connections")
        .or_insert_with(|| toml::Value::Array(vec![]))
        .as_array_mut()
        .ok_or_else(|| io::Error::other("Invalid remote profile list"))?;
    let (old, new, creating) = match change {
        Change::Create(p) => (None, Some(p), true),
        Change::Replace(old, new) => (Some(old), Some(new), false),
        Change::Delete(old) => (Some(old), None, false),
    };
    let index = if let Some(old) = old {
        Some(
            profiles
                .iter()
                .position(|p| p.name == old.name && p == old)
                .ok_or_else(|| {
                    io::Error::other("Saved profile changed; reopen Profiles and try again.")
                })?,
        )
    } else {
        None
    };
    if let Some(profile) = new {
        profile.validate()?;
        let duplicate = profiles.iter().position(|p| p.name == profile.name);
        if duplicate.is_some() && (creating || old.is_some() && duplicate != index) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "A saved profile with this name already exists.",
            ));
        }
        let index = index.or(duplicate);
        let mut value = index
            .and_then(|i| records[i].as_table().cloned())
            .unwrap_or_default();
        write_profile(&mut value, profile)?;
        if let Some(i) = index {
            records[i] = toml::Value::Table(value);
        } else {
            if records.len() >= 128 {
                return Err(io::Error::other("Remote profile limit reached"));
            }
            records.push(toml::Value::Table(value));
        }
    } else {
        records.remove(index.expect("deleting captured profile"));
    }
    let text = root.to_string();
    if text.len() as u64 > LIMIT {
        return Err(io::Error::other("Remote profiles exceed 64 KiB"));
    }
    crate::editor::storage::save(
        path,
        &text,
        crate::text_encoding::TextFormat::default(),
        original.as_ref(),
    )
    .map(|o| o.warning)
    .map_err(io::Error::other)
}
fn write_profile(t: &mut toml::Table, p: &Profile) -> io::Result<()> {
    for (key, value) in [
        ("name", p.name.clone()),
        ("host", p.host.clone()),
        ("user", p.user.clone()),
        ("protocol", p.protocol.label().into()),
        ("directory", p.directory.as_str().into()),
        ("fingerprint", p.fingerprint.clone().unwrap_or_default()),
        (
            "key",
            p.key
                .as_ref()
                .map(|p| {
                    p.to_str().map(str::to_owned).ok_or_else(|| {
                        io::Error::other("Profile key path cannot be represented as UTF-8")
                    })
                })
                .transpose()?
                .unwrap_or_default(),
        ),
    ] {
        t.insert(key.into(), toml::Value::String(value));
    }
    t.insert("port".into(), toml::Value::Integer(i64::from(p.port)));
    Ok(())
}
